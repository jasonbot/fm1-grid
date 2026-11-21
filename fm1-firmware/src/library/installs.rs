//! Per-firmware install history: count and last successful install time. The
//! bookkeeping lives in its own store object so builtins and online offers can
//! share it without changing their persisted rows.

use super::merge::name_key;
use super::util::civil_from_days;
use super::*;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const INSTALLS_OBJECT: &str = "installs";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct InstallRecord {
    pub count: u32,
    /// Unix seconds; 0 only on a malformed/old empty record.
    pub last: u64,
}

/// History follows the firmware identity (normalized name + device/variant), not
/// the package version, so a project keeps its count when it upgrades.
fn install_key(firmware: &Firmware) -> String {
    let mut key = name_key(&firmware.name);
    key.push('|');
    match firmware.device {
        FirmwareDevice::Fm1 => key.push_str("fm1"),
        FirmwareDevice::Smk37 => {
            key.push_str("smk37|");
            key.push_str(
                &firmware
                    .variant()
                    .map(name_key)
                    .unwrap_or_else(|| "unknown".to_string()),
            );
        }
    }
    key
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn load(store: &Store) -> HashMap<String, InstallRecord> {
    store.fetch_object(INSTALLS_OBJECT).unwrap_or_default()
}

/// Add/replace the synthetic install-count tag on a view row.
pub(super) fn apply_tag(firmware: &mut Firmware, installs: &HashMap<String, InstallRecord>) {
    firmware.tags.retain(|tag| !tag.starts_with("installs: "));
    if let Some(record) = installs.get(&install_key(firmware)) {
        if record.count > 0 {
            firmware.tags.push(format!("installs: {}", record.count));
        }
    }
}

impl Library {
    /// Re-read the shared install map. The GUI and TUI both write it, so a stale
    /// in-memory copy must not outlive a refresh.
    pub fn refresh_installs(&mut self) {
        let disk = load(&self.store);
        if disk != self.installs {
            self.installs = disk;
            self.view_dirty = true;
        }
    }

    /// Record a successful install into a pane or a live unit. The count and time
    /// persist in `installs.json`; the next `list()` rebuild carries the tag.
    pub fn record_install(&mut self, firmware: &Firmware) {
        // The GUI and TUI share this file. Lock across read-modify-write so one
        // process cannot overwrite the other's install. The browser store is
        // single-origin localStorage and has no cross-process writers.
        #[cfg(not(target_arch = "wasm32"))]
        let _lock = self.store.lock_file(INSTALLS_OBJECT).ok().map(|lock| {
            let _ = lock.lock();
            lock
        });
        let mut disk = load(&self.store);
        let record = disk.entry(install_key(firmware)).or_default();
        record.count = record.count.saturating_add(1);
        record.last = now_seconds();
        self.installs = disk;
        if let Err(error) = self.store.store_object(INSTALLS_OBJECT, &self.installs) {
            eprintln!("fm1-grid: {error}");
        }
        self.view_dirty = true;
    }

    /// How many times this firmware (name/device identity) has been installed.
    pub fn install_count(&self, firmware: &Firmware) -> u32 {
        self.installs
            .get(&install_key(firmware))
            .map_or(0, |record| record.count)
    }

    /// The last successful install as `YYYY-MM-DD` UTC, if it has ever been installed.
    pub fn last_installed_date(&self, firmware: &Firmware) -> Option<String> {
        let seconds = self.installs.get(&install_key(firmware))?.last;
        if seconds == 0 {
            return None;
        }
        let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
        Some(format!("{year:04}-{month:02}-{day:02}"))
    }
}
