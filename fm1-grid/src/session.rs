//! The UI session record: the sidebar split position, the selected tab (and the
//! Devices tab's section) and pane, and the metronome's tempo (not its run state).
//! Saved on quit, restored on launch; a missing or corrupt record just yields the
//! defaults.

use gpui_kit::App;
use serde::{Deserialize, Serialize};

use crate::app::GridApp;
use fm1_performer::clock::{DEFAULT_BPM, MAX_BPM, MIN_BPM};
use fm1_performer::store::Store;
use crate::theme::{SIDEBAR_INITIAL, SIDEBAR_MAX, SIDEBAR_MIN};

const SESSION_OBJECT: &str = "session";

/// What comes back after a restart. Serde defaults keep old records loading when new
/// fields arrive.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Session {
    /// The emulator split's sidebar width; the grid panel takes the rest.
    pub sidebar_width: f32,
    /// The main tab (0 = Emulator, 1 = Devices, 2 = Library).
    pub tab: usize,
    /// The Devices tab's section (0 = Devices, 1 = MIDI Graph).
    pub devices_tab: usize,
    /// The selected emulator pane.
    pub slot: usize,
    /// The metronome's tempo; whether it runs is not remembered.
    pub bpm: u32,
    /// The Library tab's Download All toggle: on restart, the downloads resume.
    pub download_all: bool,
    /// Favorited firmwares, as normalized name keys; sorted so the record on disk
    /// stays diff-stable.
    pub favorites: std::collections::BTreeSet<String>,
    /// The glyph each favorite shows (normalized name key -> glyph); ★ is implied
    /// when a favorite has no entry.
    pub favorite_glyphs: std::collections::BTreeMap<String, String>,
    /// The Library tab's device checkboxes (FM-1 on, SMK-37 off by default).
    pub device_filter: crate::app::DeviceFilter,
    /// The theme family (the Settings tab's picker; "default" is gpui-kit's pair).
    pub theme: String,
    /// The appearance override (auto/light/dark).
    pub appearance: crate::app::Appearance,
    /// The accent colour id Influenza is computed from; other families ignore it.
    pub accent: String,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_INITIAL,
            tab: 0,
            devices_tab: 0,
            slot: 0,
            bpm: DEFAULT_BPM,
            download_all: false,
            favorites: Default::default(),
            favorite_glyphs: Default::default(),
            device_filter: Default::default(),
            theme: crate::theme::DEFAULT_FAMILY.to_string(),
            appearance: crate::app::Appearance::default(),
            accent: crate::influenza::accent("blue").id.to_string(),
        }
    }
}

impl Session {
    /// The stored record, or the defaults when there is none or it doesn't parse.
    /// Values are clamped into the ranges the layout and the clock accept, so a stale
    /// or hand-edited record can't wedge the window.
    pub(crate) fn load(store: Option<&Store>) -> Self {
        let mut session: Self = store
            .and_then(|store| store.fetch_object(SESSION_OBJECT).ok())
            .unwrap_or_default();
        session.sidebar_width = session.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        session.tab = session.tab.min(3);
        session.devices_tab = session.devices_tab.min(1);
        session.slot = session.slot.min(fm1_performer::MAX_SLOTS - 1);
        session.bpm = session.bpm.clamp(MIN_BPM, MAX_BPM);
        // A stale or hand-edited accent id falls back to Blue.
        session.accent = crate::influenza::accent(&session.accent).id.to_string();
        // The builtin "FM-1+VA" was renamed "Baud Girl (FM-1+VA)"; carry its star
        // over (favorites key on the normalized name, so a rename orphans them).
        if session.favorites.remove("fm-1+va") {
            session.favorites.insert("baud girl (fm-1+va)".to_string());
            if let Some(glyph) = session.favorite_glyphs.remove("fm-1+va") {
                session.favorite_glyphs.insert("baud girl (fm-1+va)".to_string(), glyph);
            }
        }
        // Glyphs only make sense for a favorite.
        session
            .favorite_glyphs
            .retain(|key, _| session.favorites.contains(key));
        session
    }
}

impl GridApp {
    /// Snapshot the session to the store; the quit handler calls this. The sidebar
    /// width comes from the split's bound state entity — an untouched group reports
    /// no sizes, so the initial width is the fallback.
    pub fn save_session(&self, cx: &App) {
        let Some(store) = &self.store else { return };
        let session = Session {
            sidebar_width: self
                .emulator_split
                .read(cx)
                .sizes()
                .get(1)
                .map(|size| size.as_f32())
                .unwrap_or(self.sidebar_width),
            tab: self.active_tab,
            devices_tab: self.devices_tab,
            slot: self.active,
            bpm: self.clock.bpm(),
            download_all: self.download_all,
            favorites: self.favorites.iter().cloned().collect(),
            favorite_glyphs: self
                .favorite_glyphs
                .iter()
                .filter(|(key, _)| self.favorites.contains(*key))
                .map(|(key, glyph)| (key.clone(), glyph.clone()))
                .collect(),
            device_filter: self.device_filter,
            theme: self.theme_id.clone(),
            appearance: self.appearance,
            accent: self.accent.clone(),
        };
        if let Err(e) = store.store_object(SESSION_OBJECT, &session) {
            eprintln!("fm1-grid: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_clamps() {
        let dir = std::env::temp_dir().join(format!("fm1-grid-session-{}", std::process::id()));
        let store = Store::in_dir(&dir);

        // A record with out-of-range values comes back inside the accepted ranges.
        let record = Session {
            sidebar_width: 4000.0,
            tab: 9,
            devices_tab: 9,
            slot: 42,
            bpm: 1,
            download_all: true,
            favorites: ["felucca".to_string(), "fm-1+va".to_string()]
                .into_iter()
                .collect(),
            favorite_glyphs: [("fm-1+va".to_string(), "𓅓".to_string())]
                .into_iter()
                .collect(),
            device_filter: crate::app::DeviceFilter {
                fm1: true,
                smk37: true,
            },
            theme: "ayu".to_string(),
            appearance: crate::app::Appearance::Dark,
            accent: "not-a-colour".to_string(),
        };
        store.store_object(SESSION_OBJECT, &record).unwrap();
        let loaded = Session::load(Some(&store));
        assert_eq!(loaded.sidebar_width, SIDEBAR_MAX);
        assert_eq!(loaded.tab, 3);
        assert_eq!(loaded.devices_tab, 1);
        assert_eq!(loaded.theme, "ayu");
        assert_eq!(loaded.appearance, crate::app::Appearance::Dark);
        // An unknown accent id comes back as the default Blue.
        assert_eq!(loaded.accent, "blue");
        assert_eq!(loaded.slot, fm1_performer::MAX_SLOTS - 1);
        assert_eq!(loaded.bpm, MIN_BPM);
        assert!(loaded.download_all);
        assert!(loaded.favorites.contains("felucca"));
        // The pre-rename builtin's star follows to "Baud Girl (FM-1+VA)".
        assert!(loaded.favorites.contains("baud girl (fm-1+va)"));
        assert!(!loaded.favorites.contains("fm-1+va"));
        assert_eq!(loaded.favorites.len(), 2);
        // The glyph followed the renamed favorite.
        assert_eq!(
            loaded.favorite_glyphs.get("baud girl (fm-1+va)").map(String::as_str),
            Some("𓅓")
        );
        assert!(!loaded.favorite_glyphs.contains_key("fm-1+va"));
        assert!(loaded.device_filter.fm1 && loaded.device_filter.smk37);

        // No record: the defaults; a corrupt record: the defaults too.
        std::fs::remove_dir_all(&dir).ok();
        let store = Store::in_dir(&dir);
        let defaults = Session::load(Some(&store));
        assert_eq!(defaults.sidebar_width, SIDEBAR_INITIAL);
        assert!(defaults.device_filter.fm1 && !defaults.device_filter.smk37);
        assert_eq!(defaults.accent, "blue");
        std::fs::write(dir.join("objects").join(format!("{SESSION_OBJECT}.json")), b"{").unwrap();
        let defaults = Session::load(Some(&store));
        assert_eq!(defaults.sidebar_width, SIDEBAR_INITIAL);
        assert!(defaults.device_filter.fm1 && !defaults.device_filter.smk37);
        assert_eq!(defaults.accent, "blue");
        std::fs::remove_dir_all(&dir).ok();
    }
}
