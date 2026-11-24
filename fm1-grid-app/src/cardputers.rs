//! Cardputer-Adv discovery, serial flashing and firmware uploads.
//!
//! Two background paths feed the Cardputers section: a scanner that polls the ESP32's
//! ROM serial interface and the LAN's fm1-card devices, and one-off jobs for a
//! flash or an upload. The UI only reads snapshots and a generation counter; the
//! completed jobs land in [`CardputerState::take_events`], which the tick turns
//! into notifications.

use fm1_cardputer as cardputer;
pub use fm1_cardputer::FlashableDevice;
use fm1_performer::library::Firmware;
use fm1_performer::store::Store;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCAN_INTERVAL: Duration = Duration::from_secs(5);
const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(700);
const IDLE_SLEEP: Duration = Duration::from_millis(500);
/// How often the catalog is gardened while Cardputers is enabled. New listings
/// trigger a pass at once; this is the blank-filling retry cadence.
const GARDEN_INTERVAL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineDevice {
    pub name: String,
    pub host: String,
    pub status: Option<cardputer::Status>,
    pub firmwares: Vec<cardputer::InstalledFirmware>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Already,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogOutcome {
    Synced { entries: u64, pending: u64 },
    UpToDate,
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
    CatalogSynced {
        host: String,
        device: String,
        result: Result<CatalogOutcome, String>,
    },
}

pub struct CardputerState {
    serial: Arc<Mutex<Vec<FlashableDevice>>>,
    online: Arc<Mutex<Arc<Vec<OnlineDevice>>>>,
    /// A flash is keyed by its serial path; an upload or catalog sync by its host.
    /// One job per key at a time.
    busy: Arc<Mutex<HashSet<String>>>,
    events: Arc<Mutex<Vec<CardputerEvent>>>,
    generation: Arc<AtomicU64>,
    /// Whether a visible place needs the online list, and whether the Cardputers
    /// section needs the serial list. The scanner does no work for a false flag.
    wanted_network: Arc<AtomicBool>,
    wanted_serial: Arc<AtomicBool>,
    /// Hosts whose accordion is open: their next scan asks for full metadata.
    detail_hosts: Arc<Mutex<HashSet<String>>>,
    /// The catalog's persistent home (store objects, never raw files).
    store: Store,
    /// Whether the Cardputers preference is on; the gardener idles otherwise.
    enabled: Arc<AtomicBool>,
    /// The library's last successful online listing, and whether every feed
    /// answered (a partial listing keeps cached rows instead of dropping them).
    listing: Arc<Mutex<Option<(Vec<Firmware>, bool)>>>,
    /// A new listing (or a fresh enable) wants a garden pass now.
    needs_pass: Arc<AtomicBool>,
    /// One catalog pass at a time, shared by the gardener and Sync catalog.
    garden: Arc<Mutex<()>>,
}

impl CardputerState {
    pub fn start(store: Store) -> Self {
        let state = Self {
            serial: Arc::new(Mutex::new(Vec::new())),
            online: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            busy: Arc::new(Mutex::new(HashSet::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
            wanted_network: Arc::new(AtomicBool::new(false)),
            wanted_serial: Arc::new(AtomicBool::new(false)),
            detail_hosts: Arc::new(Mutex::new(HashSet::new())),
            store,
            enabled: Arc::new(AtomicBool::new(false)),
            listing: Arc::new(Mutex::new(None)),
            needs_pass: Arc::new(AtomicBool::new(false)),
            garden: Arc::new(Mutex::new(())),
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
        let detail_hosts = state.detail_hosts.clone();
        let catalog_store = state.store.clone();
        let catalog_listing = state.listing.clone();
        let catalog_needs_pass = state.needs_pass.clone();
        let catalog_garden = state.garden.clone();
        let catalog_enabled = state.enabled.clone();
        std::thread::Builder::new()
            .name("fm1-cardputer-catalog".into())
            .spawn(move || {
                let mut last: Option<Instant> = None;
                loop {
                    if !catalog_enabled.load(Ordering::Relaxed) {
                        std::thread::sleep(IDLE_SLEEP);
                        continue;
                    }
                    let due = catalog_needs_pass.swap(false, Ordering::Relaxed)
                        || last.is_none_or(|at| at.elapsed() >= GARDEN_INTERVAL);
                    if !due {
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    let snapshot = catalog_listing.lock().unwrap().clone();
                    let Some((entries, complete)) = snapshot else {
                        last = Some(Instant::now());
                        continue;
                    };
                    if let Ok(_guard) = catalog_garden.lock() {
                        if let Err(error) = fm1_performer::library::garden_remote_catalog(
                            &catalog_store,
                            entries,
                            complete,
                        ) {
                            eprintln!("fm1-grid: cardputer catalog: {error}");
                        }
                    }
                    last = Some(Instant::now());
                }
            })
            .expect("spawn cardputer catalog gardener");
        std::thread::Builder::new()
            .name("fm1-cardputer-scan".into())
            .spawn(move || {
                // The package names each host's detail listing last showed;
                // collapsing, leaving the section, or a changed file set clears an
                // entry so the next poll fetches detail again.
                let mut detailed: HashMap<String, Vec<String>> = HashMap::new();
                loop {
                    let network_wanted = wanted_network.load(Ordering::Relaxed);
                    let serial_wanted = wanted_serial.load(Ordering::Relaxed);
                    if !network_wanted && !serial_wanted {
                        std::thread::sleep(IDLE_SLEEP);
                        continue;
                    }
                    // Detail is the slow stat/meta pass: ask for it once per expansion,
                    // not on every poll.
                    let detail = serial_wanted.then(|| detail_hosts.lock().unwrap().clone());
                    let mut detail_requested: HashSet<String> = HashSet::new();
                    if let Some(hosts) = &detail {
                        for host in hosts {
                            if !detailed.contains_key(host) {
                                detail_requested.insert(host.clone());
                            }
                        }
                    }
                    // Serial enumeration is Windows SetupAPI (or Linux udev) and can
                    // be slow; the Library only needs the network list.
                    let found_serial = serial_wanted.then(cardputer::find_flashable);
                    let mut found_online = Vec::new();
                    let mut publish_online = !network_wanted;
                    if network_wanted {
                        if let Ok(found) = cardputer::discover(DISCOVERY_TIMEOUT) {
                            publish_online = true;
                            // One hung Cardputer must not hold up the others.
                            found_online = std::thread::scope(|scope| {
                                let handles: Vec<_> = found
                                    .into_iter()
                                    .map(|discovery| {
                                        let detail = detail_requested.contains(&discovery.host);
                                        scope.spawn(move || fetch_online(discovery, detail))
                                    })
                                    .collect();
                                handles
                                    .into_iter()
                                    .filter_map(|handle| handle.join().ok())
                                    .collect()
                            });
                        }
                    }
                    found_online.sort_by(|a, b| a.name.cmp(&b.name).then(a.host.cmp(&b.host)));
                    match &detail {
                        Some(hosts) => {
                            let present: HashSet<&str> = found_online
                                .iter()
                                .map(|device| device.host.as_str())
                                .collect();
                            detailed.retain(|host, _| {
                                hosts.contains(host) && present.contains(host.as_str())
                            });
                            for device in &found_online {
                                if !hosts.contains(&device.host) {
                                    continue;
                                }
                                let names: Vec<String> = device
                                    .firmwares
                                    .iter()
                                    .map(|firmware| firmware.name.clone())
                                    .collect();
                                if detail_requested.contains(&device.host) {
                                    detailed.insert(device.host.clone(), names);
                                } else if detailed.get(&device.host) != Some(&names) {
                                    // A package appeared or vanished since the
                                    // detail fetch; refresh it on the next poll.
                                    detailed.remove(&device.host);
                                }
                            }
                        }
                        None => detailed.clear(),
                    }
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
                    // A failed discovery is not "no devices": keep the last list.
                    let online_changed = if publish_online {
                        // Compare outside the lock and swap only the Arc: the UI never
                        // waits for a large firmware list comparison.
                        let previous = online.lock().unwrap().clone();
                        // A fast name-only scan must not drop the metadata an earlier
                        // detail scan already fetched.
                        for device in &mut found_online {
                            let Some(old) = previous.iter().find(|old| old.host == device.host)
                            else {
                                continue;
                            };
                            merge_firmware_metadata(&mut device.firmwares, &old.firmwares);
                        }
                        let changed = *previous != found_online;
                        if changed {
                            *online.lock().unwrap() = Arc::new(found_online);
                        }
                        changed
                    } else {
                        false
                    };
                    if serial_changed || online_changed {
                        generation.fetch_add(1, Ordering::Relaxed);
                    }
                    std::thread::sleep(SCAN_INTERVAL);
                }
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

    /// Tests publish a fake LAN result; the scanner never runs with
    /// `FM1_GRID_NO_CARDPUTERS` set.
    #[cfg(test)]
    pub(crate) fn set_online_for_test(&self, devices: Vec<OnlineDevice>) {
        *self.online.lock().unwrap() = Arc::new(devices);
        self.generation.fetch_add(1, Ordering::Relaxed);
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
    /// serial ports too and is the only place that asks for detail.
    pub fn set_wanted(&self, network: bool, serial: bool) {
        self.wanted_network.store(network, Ordering::Relaxed);
        self.wanted_serial.store(serial, Ordering::Relaxed);
    }

    /// Cardputers preference on/off. Enabling asks for a catalog pass right away;
    /// disabling parks the gardener.
    pub fn set_enabled(&self, enabled: bool) {
        if self.enabled.load(Ordering::Relaxed) == enabled {
            return;
        }
        self.enabled.store(enabled, Ordering::Relaxed);
        if enabled {
            self.needs_pass.store(true, Ordering::Relaxed);
        }
    }

    /// Hand the gardener the library's last successful online listing. An empty
    /// listing means no fetch has succeeded yet, so it never clears the catalog.
    pub fn set_listing(&self, listing: Vec<Firmware>, complete: bool) {
        if listing.is_empty() {
            return;
        }
        *self.listing.lock().unwrap() = Some((listing, complete));
        self.needs_pass.store(true, Ordering::Relaxed);
    }

    /// The hosts whose accordion is open. Their scans fetch the slower detail
    /// listing; every other host stays on the fast name-only one.
    pub fn set_detail(&self, hosts: HashSet<String>) {
        let mut detail = self.detail_hosts.lock().unwrap();
        if *detail != hosts {
            *detail = hosts;
        }
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
        let failed_path = path.clone();
        let failed_device = description.clone();
        let spawned = std::thread::Builder::new()
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
            });
        if let Err(error) = spawned {
            self.busy.lock().unwrap().remove(&failed_path);
            self.events.lock().unwrap().push(CardputerEvent::Flashed {
                path: failed_path,
                device: failed_device,
                result: Err(format!("could not start the flash thread: {error}")),
            });
            self.generation.fetch_add(1, Ordering::Relaxed);
            return false;
        }
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
        let failed = (host.clone(), device.clone(), firmware.clone(), file.clone());
        let spawned = std::thread::Builder::new()
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
            });
        if let Err(error) = spawned {
            let (host, device, firmware, file) = failed;
            self.busy.lock().unwrap().remove(&host);
            self.events.lock().unwrap().push(CardputerEvent::Sent {
                host,
                device,
                firmware,
                file,
                result: Err(format!("could not start the send thread: {error}")),
            });
            self.generation.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Send the card the catalog built so far, skipping the upload when it already
    /// holds the same manifest. The gardener fills unresolved entries in later.
    pub fn sync_catalog(&self, host: String, device: String) -> bool {
        // Tests and the README screenshot stay offline, like the scanner.
        if std::env::var_os("FM1_GRID_NO_CARDPUTERS").is_some() {
            return false;
        }
        if !self.claim(host.clone()) {
            return false;
        }
        let events = self.events.clone();
        let busy = self.busy.clone();
        let generation = self.generation.clone();
        let failed = (host.clone(), device.clone());
        let store = self.store.clone();
        let garden = self.garden.clone();
        let listing = self.listing.clone();
        let spawned = std::thread::Builder::new()
            .name("fm1-cardputer-catalog".into())
            .spawn(move || {
                let result = sync_catalog_inner(&host, &store, &garden, &listing);
                events.lock().unwrap().push(CardputerEvent::CatalogSynced {
                    host: host.clone(),
                    device,
                    result,
                });
                busy.lock().unwrap().remove(&host);
                generation.fetch_add(1, Ordering::Relaxed);
            });
        if let Err(error) = spawned {
            let (host, device) = failed;
            self.busy.lock().unwrap().remove(&host);
            self.events.lock().unwrap().push(CardputerEvent::CatalogSynced {
                host,
                device,
                result: Err(format!("could not start the catalog thread: {error}")),
            });
            self.generation.fetch_add(1, Ordering::Relaxed);
            return false;
        }
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

fn fetch_online(discovery: cardputer::Discovery, detail: bool) -> OnlineDevice {
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
    match cardputer::firmwares(&discovery.host, detail) {
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

fn sync_catalog_inner(
    host: &str,
    store: &Store,
    garden: &Mutex<()>,
    listing: &Mutex<Option<(Vec<Firmware>, bool)>>,
) -> Result<CatalogOutcome, String> {
    let mut catalog = fm1_performer::library::load_remote_catalog(store);
    if catalog.entries.is_empty() {
        // Nothing gardened yet (or every link failed): build from the listing the
        // library already fetched, once.
        let _guard = garden.lock().unwrap();
        catalog = fm1_performer::library::load_remote_catalog(store);
        if catalog.entries.is_empty() {
            let (entries, complete) = listing
                .lock()
                .unwrap()
                .clone()
                .ok_or("the online firmware listing has not loaded yet")?;
            fm1_performer::library::garden_remote_catalog(store, entries, complete)?;
            catalog = fm1_performer::library::load_remote_catalog(store);
        }
    }
    if catalog.entries.is_empty() {
        return Err(
            "no package links could be resolved yet; the catalog will fill in as the \
             online feeds answer"
                .to_string(),
        );
    }
    let manifest = serde_json::to_vec(&catalog.entries)
        .map_err(|error| format!("serialize catalog: {error}"))?;
    let digest = cardputer::sha256(&manifest);
    let installed = cardputer::catalog(host).map_err(|error| error.to_string())?;
    if catalog_is_current(installed.as_ref(), &digest) {
        return Ok(CatalogOutcome::UpToDate);
    }
    let reply =
        cardputer::upload_catalog(host, &manifest).map_err(|error| error.to_string())?;
    Ok(CatalogOutcome::Synced {
        entries: reply.entries,
        pending: catalog.pending.len() as u64,
    })
}

/// The card's manifest hash is the whole check: same bytes means no upload.
fn catalog_is_current(info: Option<&cardputer::CatalogInfo>, digest: &str) -> bool {
    info.and_then(|info| info.sha256.as_deref())
        .is_some_and(|sha| sha.eq_ignore_ascii_case(digest))
}

/// Fill in what a fast name-only scan can never return from the previous listing.
fn merge_firmware_metadata(
    fresh: &mut [cardputer::InstalledFirmware],
    previous: &[cardputer::InstalledFirmware],
) {
    for entry in fresh {
        let Some(old) = previous.iter().find(|old| old.name == entry.name) else {
            continue;
        };
        entry.size = entry.size.or(old.size);
        if entry.sha256.is_none() {
            entry.sha256 = old.sha256.clone();
        }
        if entry.title.is_empty() {
            entry.title = old.title.clone();
        }
        if entry.author.is_empty() {
            entry.author = old.author.clone();
        }
        if entry.version.is_empty() {
            entry.version = old.version.clone();
        }
        if entry.description.is_empty() {
            entry.description = old.description.clone();
        }
        if entry.tags.is_empty() {
            entry.tags = old.tags.clone();
        }
        if entry.product.is_empty() {
            entry.product = old.product.clone();
        }
    }
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
    fn catalog_upload_is_skipped_only_for_matching_hashes() {
        let info = |sha: Option<&str>| cardputer::CatalogInfo {
            sha256: sha.map(str::to_string),
            ..Default::default()
        };
        assert!(catalog_is_current(Some(&info(Some("ABCD"))), "abcd"));
        assert!(!catalog_is_current(Some(&info(Some("abcd"))), "abce"));
        assert!(!catalog_is_current(Some(&info(None)), "abcd"));
        assert!(!catalog_is_current(None, "abcd"));
    }

    #[test]
    fn a_fast_scan_keeps_the_metadata_it_already_fetched() {
        let mut fresh = vec![entry("a.fwsc", None)];
        let previous = [cardputer::InstalledFirmware {
            name: "a.fwsc".into(),
            size: Some(1024),
            title: "Sloop".into(),
            author: "isod89".into(),
            version: "2.8.5".into(),
            description: "A groovebox".into(),
            tags: "groove".into(),
            product: "FM-1_900".into(),
            ..Default::default()
        }];
        merge_firmware_metadata(&mut fresh, &previous);
        assert_eq!(fresh[0].size, Some(1024));
        assert_eq!(fresh[0].title, "Sloop");
        assert_eq!(fresh[0].author, "isod89");
        assert_eq!(fresh[0].version, "2.8.5");
        assert_eq!(fresh[0].description, "A groovebox");
        assert_eq!(fresh[0].tags, "groove");
        assert_eq!(fresh[0].product, "FM-1_900");
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
            detail_hosts: Arc::new(Mutex::new(HashSet::new())),
            store: Store::in_dir(
                &std::env::temp_dir().join(format!("fm1-grid-cardputer-{}", std::process::id())),
            ),
            enabled: Arc::new(AtomicBool::new(false)),
            listing: Arc::new(Mutex::new(None)),
            needs_pass: Arc::new(AtomicBool::new(false)),
            garden: Arc::new(Mutex::new(())),
        };
        assert!(state.claim("1.2.3.4".into()));
        assert!(!state.claim("1.2.3.4".into()));
        assert!(state.claim("5.6.7.8".into()));
        state.busy.lock().unwrap().remove("1.2.3.4");
        assert!(state.claim("1.2.3.4".into()));
    }
}
