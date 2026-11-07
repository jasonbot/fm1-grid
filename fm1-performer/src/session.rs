//! The UI state the GUI and the TUI share: the metronome tempo, the Library's
//! Download All toggle and its device selector. One record keeps both apps in step
//! while each keeps its own session record for everything else.

use serde::{Deserialize, Serialize};

use fm1_base::clock::{DEFAULT_BPM, MAX_BPM, MIN_BPM};
use fm1_base::store::Store;

const SHARED_OBJECT: &str = "shared";

pub use fm1_firmware::filter::DeviceFilter;

/// Which shared fields an app changed and must write. Everything else on disk is
/// left alone, so two apps do not overwrite each other.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Dirty {
    pub bpm: bool,
    pub download_all: bool,
    pub device_filter: bool,
}

impl Dirty {
    pub fn all() -> Self {
        Self {
            bpm: true,
            download_all: true,
            device_filter: true,
        }
    }

    pub fn any(self) -> bool {
        self.bpm || self.download_all || self.device_filter
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
    /// The Library's device selector.
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

    /// Write only the fields marked dirty. A fresh read keeps the other app's
    /// changes to the other fields.
    pub fn save(&self, store: Option<&Store>, dirty: Dirty) {
        let Some(store) = store else { return };
        if !dirty.any() {
            return;
        }
        let mut record = store
            .fetch_object::<Self>(SHARED_OBJECT)
            .unwrap_or_else(|_| Self::migrate(Some(store)));
        record.bpm = record.bpm.clamp(MIN_BPM, MAX_BPM);
        if dirty.bpm {
            record.bpm = self.bpm;
        }
        if dirty.download_all {
            record.download_all = self.download_all;
        }
        if dirty.device_filter {
            record.device_filter = self.device_filter;
        }
        record.bpm = record.bpm.clamp(MIN_BPM, MAX_BPM);
        if let Err(e) = store.store_object(SHARED_OBJECT, &record) {
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
            device_filter: DeviceFilter::Smk37Pro,
        }
        .save(Some(&store), Dirty::all());

        let loaded = Shared::load(Some(&store));
        assert_eq!(loaded.bpm, MIN_BPM);
        assert!(loaded.download_all);
        assert_eq!(loaded.device_filter, DeviceFilter::Smk37Pro);
        assert_eq!(Shared::load(None).bpm, DEFAULT_BPM);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn dirty_fields_keep_the_other_apps_values() {
        let dir = std::env::temp_dir().join(format!("fm1-shared-dirty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::in_dir(&dir);
        Shared::default().save(Some(&store), Dirty::all());

        // The GUI changes the tempo only.
        Shared {
            bpm: 130,
            ..Shared::default()
        }
        .save(
            Some(&store),
            Dirty {
                bpm: true,
                ..Dirty::default()
            },
        );
        // The TUI toggles Download All only.
        Shared {
            download_all: true,
            ..Shared::default()
        }
        .save(
            Some(&store),
            Dirty {
                download_all: true,
                ..Dirty::default()
            },
        );

        let loaded = Shared::load(Some(&store));
        assert_eq!(loaded.bpm, 130);
        assert!(loaded.download_all);
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
        assert_eq!(migrated.device_filter, DeviceFilter::All);

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
        .save(Some(&store), Dirty::all());
        assert_eq!(Shared::load(Some(&store)).bpm, 111);

        std::fs::remove_dir_all(dir).ok();
    }
}
