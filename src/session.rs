//! The UI session record: the split positions, the selected tab and pane, and the
//! metronome's tempo (not its run state). Saved on quit, restored on launch; a
//! missing or corrupt record just yields the defaults.

use gpui_kit::component::resizable::ResizableState;
use gpui_kit::{App, Entity};
use serde::{Deserialize, Serialize};

use crate::app::GridApp;
use crate::clock::{DEFAULT_BPM, MAX_BPM, MIN_BPM};
use crate::store::Store;
use crate::theme::{SIDEBAR_INITIAL, SIDEBAR_MAX, SIDEBAR_MIN};

const SESSION_OBJECT: &str = "session";

/// What comes back after a restart. Serde defaults keep old records loading when new
/// fields arrive.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Session {
    /// The emulator split's sidebar width; the grid panel takes the rest.
    pub sidebar_width: f32,
    /// The devices split's left panel width; `None` keeps the even split.
    pub devices_width: Option<f32>,
    /// The main tab (0 = Emulator, 1 = Devices, 2 = Library).
    pub tab: usize,
    /// The selected emulator pane.
    pub slot: usize,
    /// The metronome's tempo; whether it runs is not remembered.
    pub bpm: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_INITIAL,
            devices_width: None,
            tab: 0,
            slot: 0,
            bpm: DEFAULT_BPM,
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
        session.tab = session.tab.min(2);
        session.slot = session.slot.min(crate::MAX_SLOTS - 1);
        session.bpm = session.bpm.clamp(MIN_BPM, MAX_BPM);
        session
    }
}

impl GridApp {
    /// Snapshot the session to the store; the quit handler calls this. The split
    /// widths come from the groups' bound state entities — an untouched group
    /// reports no sizes, so the initial widths are the fallback.
    pub fn save_session(&self, cx: &App) {
        let Some(store) = &self.store else { return };
        let panel_size = |state: &Entity<ResizableState>, ix: usize| {
            state.read(cx).sizes().get(ix).map(|size| size.as_f32())
        };
        let session = Session {
            sidebar_width: panel_size(&self.emulator_split, 1).unwrap_or(self.sidebar_width),
            devices_width: panel_size(&self.devices_split, 0).or(self.devices_width),
            tab: self.active_tab,
            slot: self.active,
            bpm: self.clock.bpm(),
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
            devices_width: Some(700.0),
            tab: 9,
            slot: 42,
            bpm: 1,
        };
        store.store_object(SESSION_OBJECT, &record).unwrap();
        let loaded = Session::load(Some(&store));
        assert_eq!(loaded.sidebar_width, SIDEBAR_MAX);
        assert_eq!(loaded.devices_width, Some(700.0));
        assert_eq!(loaded.tab, 2);
        assert_eq!(loaded.slot, crate::MAX_SLOTS - 1);
        assert_eq!(loaded.bpm, MIN_BPM);

        // No record: the defaults; a corrupt record: the defaults too.
        std::fs::remove_dir_all(&dir).ok();
        let store = Store::in_dir(&dir);
        let defaults = Session::load(Some(&store));
        assert_eq!(defaults.sidebar_width, SIDEBAR_INITIAL);
        std::fs::write(dir.join("objects").join(format!("{SESSION_OBJECT}.json")), b"{").unwrap();
        let defaults = Session::load(Some(&store));
        assert_eq!(defaults.sidebar_width, SIDEBAR_INITIAL);
        std::fs::remove_dir_all(&dir).ok();
    }
}
