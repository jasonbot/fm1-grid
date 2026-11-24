//! Cardputer-Adv discovery, serial flashing and firmware uploads.
//!
//! Two background paths feed the Cardputers section: a scanner that polls the ESP32's
//! ROM serial interface and the LAN's fm1-card devices, and one-off jobs for a
//! flash or an upload. The UI only reads snapshots and a generation counter; the
//! completed jobs land in [`CardputerState::take_events`], which the tick turns
//! into notifications.

use fm1_cardputer as cardputer;
pub use fm1_cardputer::FlashableDevice;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SCAN_INTERVAL: Duration = Duration::from_secs(5);
const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(700);
const IDLE_SLEEP: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineDevice {
    pub name: String,
    pub host: String,
    pub status: Option<cardputer::Status>,
    pub firmwares: Vec<cardputer::InstalledFirmware>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Already,
}

#[derive(Clone, Debug)]
pub enum CardputerEvent {
    Flashed {
        path: String,
        device: String,
        result: Result<(), String>,
    },
    Sent {
        host: String,
        device: String,
        firmware: String,
        file: String,
        result: Result<SendOutcome, String>,
    },
}

pub struct CardputerState {
    serial: Arc<Mutex<Vec<FlashableDevice>>>,
    online: Arc<Mutex<Arc<Vec<OnlineDevice>>>>,
    /// A flash is keyed by its serial path; an upload by its host. One job per
    /// key at a time.
    busy: Arc<Mutex<HashSet<String>>>,
    events: Arc<Mutex<Vec<CardputerEvent>>>,
    generation: Arc<AtomicU64>,
    /// Whether a visible place needs the online list, and whether the Cardputers
    /// section needs the serial list. The scanner does no work for a false flag.
    wanted_network: Arc<AtomicBool>,
    wanted_serial: Arc<AtomicBool>,
}

impl CardputerState {
    pub fn start() -> Self {
        let state = Self {
            serial: Arc::new(Mutex::new(Vec::new())),
            online: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            busy: Arc::new(Mutex::new(HashSet::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
            wanted_network: Arc::new(AtomicBool::new(false)),
            wanted_serial: Arc::new(AtomicBool::new(false)),
        };
        // Tests and the README screenshot run offline: no broadcast, no HTTP,
        // and no chance of publishing a real device's address.
        if std::env::var_os("FM1_GRID_NO_CARDPUTERS").is_some() {
            return state;
        }
        let serial = state.serial.clone();
        let online = state.online.clone();
        let generation = state.generation.clone();
        let wanted_network = state.wanted_network.clone();
        let wanted_serial = state.wanted_serial.clone();
        std::thread::Builder::new()
            .name("fm1-cardputer-scan".into())
            .spawn(move || loop {
                let network_wanted = wanted_network.load(Ordering::Relaxed);
                let serial_wanted = wanted_serial.load(Ordering::Relaxed);
                if !network_wanted && !serial_wanted {
                    std::thread::sleep(IDLE_SLEEP);
                    continue;
                }
                // Serial enumeration is Windows SetupAPI (or Linux udev) and can
                // be slow; the Library only needs the network list.
                let found_serial = serial_wanted.then(cardputer::find_flashable);
                let mut found_online = Vec::new();
                if network_wanted {
                    if let Ok(found) = cardputer::discover(DISCOVERY_TIMEOUT) {
                        // One hung Cardputer must not hold up the others.
                        found_online = std::thread::scope(|scope| {
                            let handles: Vec<_> = found
                                .into_iter()
                                .map(|discovery| scope.spawn(move || fetch_online(discovery)))
                                .collect();
                            handles
                                .into_iter()
                                .filter_map(|handle| handle.join().ok())
                                .collect()
                        });
                    }
                }
                found_online.sort_by(|a, b| a.name.cmp(&b.name).then(a.host.cmp(&b.host)));
                let serial_changed = match found_serial {
                    Some(found) => {
                        let mut current = serial.lock().unwrap();
                        let changed = *current != found;
                        if changed {
                            *current = found;
                        }
                        changed
                    }
                    None => false,
                };
                // Compare outside the lock and swap only the Arc: the UI never
                // waits for a large firmware list comparison.
                let previous = online.lock().unwrap().clone();
                let online_changed = *previous != found_online;
                if online_changed {
                    *online.lock().unwrap() = Arc::new(found_online);
                }
                if serial_changed || online_changed {
                    generation.fetch_add(1, Ordering::Relaxed);
                }
                std::thread::sleep(SCAN_INTERVAL);
            })
            .expect("spawn cardputer scanner");
        state
    }

    pub fn serial(&self) -> Vec<FlashableDevice> {
        self.serial.lock().unwrap().clone()
    }

    pub fn online(&self) -> Arc<Vec<OnlineDevice>> {
        self.online.lock().unwrap().clone()
    }

    /// The online list's flash targets, without cloning every firmware entry.
    pub fn targets(&self) -> Vec<(String, String)> {
        let online = self.online.lock().unwrap().clone();
        online
            .iter()
            .map(|device| (device.name.clone(), device.host.clone()))
            .collect()
    }

    /// Scan only while a place that shows the list is visible. Keeping the last
    /// list means leaving and returning does not flash an empty section. The
    /// Library needs only the network list; the Cardputers section shows the
    /// serial ports too.
    pub fn set_wanted(&self, network: bool, serial: bool) {
        self.wanted_network.store(network, Ordering::Relaxed);
        self.wanted_serial.store(serial, Ordering::Relaxed);
    }

    pub fn busy(&self, key: &str) -> bool {
        self.busy.lock().unwrap().contains(key)
    }

    pub fn changed(&self, seen: &mut u64) -> bool {
        let generation = self.generation.load(Ordering::Relaxed);
        let changed = generation != *seen;
        *seen = generation;
        changed
    }

    pub fn take_events(&self) -> Vec<CardputerEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    /// Write the embedded fm1-card image to a download-mode Cardputer. The job
    /// is refused while another flash on that port runs.
    pub fn flash(&self, path: String, description: String) -> bool {
        if !self.claim(path.clone()) {
            return false;
        }
        let events = self.events.clone();
        let busy = self.busy.clone();
        let generation = self.generation.clone();
        std::thread::Builder::new()
            .name("fm1-cardputer-flash".into())
            .spawn(move || {
                let result = cardputer::flash(&path, cardputer::FIRMWARE_IMAGE, &mut |_, _| {})
                    .map_err(|error| error.to_string());
                events.lock().unwrap().push(CardputerEvent::Flashed {
                    path: path.clone(),
                    device: description,
                    result,
                });
                busy.lock().unwrap().remove(&path);
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
        true
    }

    /// Copy a package onto a running Cardputer unless the same bytes are already
    /// there. The duplicate check compares SHA-256, using `/firmwares/info` for a
    /// file whose sidecar hash is absent. `metadata` is stored beside the package
    /// so the card shows its title and author.
    pub fn send(
        &self,
        host: String,
        device: String,
        firmware: String,
        file: String,
        bytes: Vec<u8>,
        metadata: cardputer::UploadMetadata,
    ) -> bool {
        if !self.claim(host.clone()) {
            return false;
        }
        let events = self.events.clone();
        let busy = self.busy.clone();
        let generation = self.generation.clone();
        std::thread::Builder::new()
            .name("fm1-cardputer-send".into())
            .spawn(move || {
                let result = send_inner(&host, &file, &bytes, &metadata)
                    .map_err(|error| error.to_string());
                events.lock().unwrap().push(CardputerEvent::Sent {
                    host: host.clone(),
                    device,
                    firmware,
                    file,
                    result,
                });
                busy.lock().unwrap().remove(&host);
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
        true
    }

    fn claim(&self, key: String) -> bool {
        let mut busy = self.busy.lock().unwrap();
        if busy.contains(&key) {
            return false;
        }
        busy.insert(key);
        self.generation.fetch_add(1, Ordering::Relaxed);
        true
    }
}

fn fetch_online(discovery: cardputer::Discovery) -> OnlineDevice {
    let mut device = OnlineDevice {
        name: discovery.name,
        host: discovery.host.clone(),
        status: None,
        firmwares: Vec::new(),
        error: None,
    };
    match cardputer::status(&discovery.host) {
        Ok(status) => {
            if !status.name.trim().is_empty() {
                device.name = status.name.clone();
            }
            device.status = Some(status);
        }
        Err(error) => device.error = Some(error.to_string()),
    }
    match cardputer::firmwares(&discovery.host, false) {
        Ok(mut list) => {
            list.sort_by(|a, b| a.name.cmp(&b.name));
            device.firmwares = list;
        }
        Err(error) => {
            if device.error.is_none() {
                device.error = Some(error.to_string());
            }
        }
    }
    device
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Duplicate {
    Already,
    CheckInfo,
    Upload,
}

fn duplicate_decision(list: &[cardputer::InstalledFirmware], local: &str, file: &str) -> Duplicate {
    if list
        .iter()
        .any(|entry| entry.sha256.as_deref() == Some(local))
    {
        return Duplicate::Already;
    }
    match list.iter().find(|entry| entry.name == file) {
        Some(entry) if entry.sha256.is_none() => Duplicate::CheckInfo,
        _ => Duplicate::Upload,
    }
}

fn send_inner(
    host: &str,
    file: &str,
    bytes: &[u8],
    metadata: &cardputer::UploadMetadata,
) -> Result<SendOutcome, cardputer::Error> {
    let local = cardputer::sha256(bytes);
    let list = cardputer::firmwares(host, true)?;
    match duplicate_decision(&list, &local, file) {
        Duplicate::Already => Ok(SendOutcome::Already),
        Duplicate::Upload => {
            cardputer::upload(host, file, bytes, metadata)?;
            Ok(SendOutcome::Sent)
        }
        Duplicate::CheckInfo => {
            let info = cardputer::firmware_info(host, file)?;
            if info.sha256.eq_ignore_ascii_case(&local) {
                Ok(SendOutcome::Already)
            } else {
                cardputer::upload(host, file, bytes, metadata)?;
                Ok(SendOutcome::Sent)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, sha256: Option<&str>) -> cardputer::InstalledFirmware {
        cardputer::InstalledFirmware {
            name: name.into(),
            sha256: sha256.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn duplicate_decision_uses_hashes_then_the_same_name() {
        let list = [entry("a.fwsc", Some("AAA")), entry("b.fwsc", None)];
        assert_eq!(duplicate_decision(&list, "AAA", "c.fwsc"), Duplicate::Already);
        assert_eq!(duplicate_decision(&list, "AAA", "a.fwsc"), Duplicate::Already);
        assert_eq!(duplicate_decision(&list, "BBB", "b.fwsc"), Duplicate::CheckInfo);
        assert_eq!(duplicate_decision(&list, "BBB", "c.fwsc"), Duplicate::Upload);
        assert_eq!(duplicate_decision(&list, "BBB", "a.fwsc"), Duplicate::Upload);
    }

    #[test]
    fn busy_keys_are_one_job_at_a_time() {
        let state = CardputerState {
            serial: Arc::new(Mutex::new(Vec::new())),
            online: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            busy: Arc::new(Mutex::new(HashSet::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
            wanted_network: Arc::new(AtomicBool::new(false)),
            wanted_serial: Arc::new(AtomicBool::new(false)),
        };
        assert!(state.claim("1.2.3.4".into()));
        assert!(!state.claim("1.2.3.4".into()));
        assert!(state.claim("5.6.7.8".into()));
        state.busy.lock().unwrap().remove("1.2.3.4");
        assert!(state.claim("1.2.3.4".into()));
    }
}
