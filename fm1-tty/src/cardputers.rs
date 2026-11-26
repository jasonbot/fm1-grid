//! The LAN Cardputers the flash menu can copy a package onto. fm1-card answers a
//! UDP `hello` with its name and HTTP address; that is all the menu needs, so this
//! is discovery only — no status or firmware polling, which the GUI's Cardputers
//! section owns. A failed sweep keeps the previous list.

use fm1_performer::firmware_picker::FirmwarePick;
use fm1_performer::library::Firmware;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How often the LAN is swept, and how long one sweep listens.
const SCAN_INTERVAL: Duration = Duration::from_secs(5);
const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(700);

pub struct Cardputers {
    online: Arc<Mutex<Vec<fm1_cardputer::Discovery>>>,
}

impl Cardputers {
    pub fn start() -> Self {
        let online = Arc::new(Mutex::new(Vec::new()));
        // Same opt-out as the GUI, for headless machines and tests.
        if std::env::var_os("FM1_GRID_NO_CARDPUTERS").is_none() {
            let found = online.clone();
            thread::Builder::new()
                .name("fm1-tty-cardputers".into())
                .spawn(move || loop {
                    if let Ok(devices) = fm1_cardputer::discover(DISCOVERY_TIMEOUT) {
                        if let Ok(mut list) = found.lock() {
                            *list = devices;
                        }
                    }
                    thread::sleep(SCAN_INTERVAL);
                })
                .expect("spawn Cardputer discovery thread");
        }
        Self { online }
    }

    /// The flash menu's entries: the card's display name and its HTTP host.
    pub fn targets(&self) -> Vec<(String, String)> {
        self.online
            .lock()
            .map(|list| {
                list.iter()
                    .map(|device| (device.name.clone(), device.host.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The name the Cardputer stores the package under, mirroring the GUI: the source
/// file's name when it has one, the display name and version otherwise. All of it
/// goes through the card's FAT32-safe renaming.
pub fn file_name(firmware: &Firmware, pick: &FirmwarePick) -> String {
    let candidate = match pick {
        FirmwarePick::Builtin(index) => fm1_performer::builtin::BUILTINS
            .get(*index)
            .map(|builtin| builtin.file.to_string()),
        FirmwarePick::File(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        FirmwarePick::HubBlob(key) => key
            .rsplit('/')
            .next()
            .filter(|name| name.to_ascii_lowercase().ends_with(".fwsc"))
            .map(str::to_string),
    };
    fm1_cardputer::safe_file_name(
        &candidate.unwrap_or_else(|| format!("{}-{}", firmware.name, firmware.version)),
    )
}
