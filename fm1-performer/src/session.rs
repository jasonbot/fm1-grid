//! The UI state the GUI and the TUI share: the metronome tempo, the Library's
//! Download All toggle and its device checkboxes. One record keeps both apps in step
//! while each keeps its own session record for everything else.

use serde::{Deserialize, Serialize};

use crate::clock::{DEFAULT_BPM, MAX_BPM, MIN_BPM};
use crate::library::FirmwareDevice;
use crate::store::Store;

const SHARED_OBJECT: &str = "shared";

/// The Library's device checkboxes: FM-1 on, SMK-37 off until asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceFilter {
    pub fm1: bool,
    pub smk37: bool,
}

impl Default for DeviceFilter {
    fn default() -> Self {
        Self {
            fm1: true,
            smk37: false,
        }
    }
}

impl DeviceFilter {
    pub fn shows(&self, device: FirmwareDevice) -> bool {
        match device {
            FirmwareDevice::Fm1 => self.fm1,
            FirmwareDevice::Smk37 => self.smk37,
        }
    }
}

/// What the two apps' sessions have in common. Serde defaults keep old records
/// loading when new fields arrive.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Shared {
    /// The metronome's tempo; whether it runs is not remembered.
    pub bpm: u32,
    /// The Library's Download All toggle: on restart, the downloads resume.
    pub download_all: bool,
    /// The Library's device checkboxes.
    pub device_filter: DeviceFilter,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            bpm: DEFAULT_BPM,
            download_all: false,
            device_filter: DeviceFilter::default(),
        }
    }
}

impl Shared {
    /// The shared record, or the defaults. A missing record migrates the first app
    /// record that carries these fields (the GUI's `session`, then the old
    /// `tty-session`). The tempo is clamped into the clock's range.
    pub fn load(store: Option<&Store>) -> Self {
        let mut shared: Self = store
            .and_then(|store| store.fetch_object(SHARED_OBJECT).ok())
            .unwrap_or_else(|| Self::migrate(store));
        shared.bpm = shared.bpm.clamp(MIN_BPM, MAX_BPM);
        shared
    }

    fn migrate(store: Option<&Store>) -> Self {
        let Some(store) = store else {
            return Self::default();
        };
        ["session", "tty-session"]
            .iter()
            .find_map(|key| store.fetch_object::<Self>(key).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store: Option<&Store>) {
        let Some(store) = store else { return };
        if let Err(e) = store.store_object(SHARED_OBJECT, self) {
            eprintln!("fm1 session: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_clamps() {
        let dir = std::env::temp_dir().join(format!("fm1-shared-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::in_dir(&dir);

        Shared {
            bpm: 1,
            download_all: true,
            device_filter: DeviceFilter {
                fm1: false,
                smk37: true,
            },
        }
        .save(Some(&store));

        let loaded = Shared::load(Some(&store));
        assert_eq!(loaded.bpm, MIN_BPM);
        assert!(loaded.download_all);
        assert!(!loaded.device_filter.fm1);
        assert!(loaded.device_filter.smk37);
        assert_eq!(Shared::load(None).bpm, DEFAULT_BPM);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_shared_migrates_the_app_records() {
        let dir = std::env::temp_dir().join(format!("fm1-shared-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::in_dir(&dir);

        // The GUI's old record wins over the TUI's while both exist.
        store
            .store_object(
                "session",
                &serde_json::json!({"bpm": 145, "download_all": true, "device_filter": {"fm1": true, "smk37": true}, "theme": "beos"}),
            )
            .unwrap();
        store
            .store_object(
                "tty-session",
                &serde_json::json!({"bpm": 90, "download_all": false, "device_filter": {"fm1": true, "smk37": false}}),
            )
            .unwrap();
        let migrated = Shared::load(Some(&store));
        assert_eq!(migrated.bpm, 145);
        assert!(migrated.download_all);
        assert!(migrated.device_filter.smk37);

        // Without the GUI's record the TUI's is used.
        std::fs::remove_file(dir.join("objects/session.json")).unwrap();
        let migrated = Shared::load(Some(&store));
        assert_eq!(migrated.bpm, 90);
        assert!(!migrated.download_all);

        // A stored shared record always wins.
        Shared {
            bpm: 111,
            download_all: false,
            device_filter: DeviceFilter::default(),
        }
        .save(Some(&store));
        assert_eq!(Shared::load(Some(&store)).bpm, 111);

        std::fs::remove_dir_all(dir).ok();
    }
}
