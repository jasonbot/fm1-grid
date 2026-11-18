//! Download-mode units found through the chip's raw USB recovery path.
//!
//! The MIDI scanner cannot see a unit sitting in the chip's ROM loader; this
//! thread checks the SCSI/USB download-mode list, reads each unit's flash once
//! and keeps the model, firmware and bootloader text for the Devices pane. Full
//! flash backups live in the app's blob store so they can be listed and restored
//! from the same pane.

use fm1_performer::store::Store;
use fm1_recovery::{Probe, FLASH_SIZE};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BACKUP_PREFIX: &str = "recovery/";

#[derive(Clone, Serialize, Deserialize)]
pub struct BackupItem {
    /// The blob key; the metadata object is the same path with `.json`.
    pub key: String,
    /// Milliseconds since the Unix epoch; newest first.
    pub created: u64,
    pub label: String,
    pub model: String,
    pub chip_key: String,
    pub flash: String,
    pub bytes: usize,
}

#[derive(Clone)]
pub struct RecoveryRow {
    pub path: String,
    pub description: String,
    pub probe: Option<Probe>,
    pub error: Option<String>,
    pub busy: bool,
    pub busy_label: Option<String>,
    pub outcome: Option<String>,
}

pub struct RecoveryState {
    rows: Arc<Mutex<Vec<RecoveryRow>>>,
    generation: Arc<AtomicU64>,
    store: Store,
    backups: Arc<Mutex<Vec<BackupItem>>>,
}

impl RecoveryState {
    pub fn start(store: Store) -> Self {
        let rows: Arc<Mutex<Vec<RecoveryRow>>> = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let backups = Arc::new(Mutex::new(load_backups(&store)));
        let state = Self {
            rows: rows.clone(),
            generation: generation.clone(),
            store,
            backups,
        };
        std::thread::Builder::new()
            .name("fm1-recovery-scan".into())
            .spawn(move || loop {
                let found = fm1_recovery::find_devices();
                let old = rows.lock().unwrap().clone();
                let mut current = Vec::with_capacity(found.len());
                let mut changed = false;
                for (path, description) in found {
                    if let Some(mut row) = old.iter().find(|row| row.path == path).cloned() {
                        row.description = description;
                        current.push(row);
                        continue;
                    }
                    let mut row = RecoveryRow {
                        path: path.clone(),
                        description,
                        probe: None,
                        error: None,
                        busy: false,
                        busy_label: None,
                        outcome: None,
                    };
                    // Reading a unit is ~1 MiB and must not hold the UI lock.
                    match fm1_recovery::probe(&path, &mut |_| {}, &mut |_, _| {}) {
                        Ok(probe) => row.probe = Some(probe),
                        Err(error) => row.error = Some(error.to_string()),
                    }
                    current.push(row);
                    changed = true;
                }
                *rows.lock().unwrap() = current;
                if changed {
                    generation.fetch_add(1, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_secs(2));
            })
            .expect("spawn recovery scanner");
        state
    }

    pub fn rows(&self) -> Vec<RecoveryRow> {
        self.rows.lock().unwrap().clone()
    }

    /// The stored backups that belong to this unit: same model, chip key and flash.
    /// A full-flash image carries settings and the chip-key record, so a backup from
    /// another unit is not offered.
    pub fn backups_for(&self, probe: &Probe) -> Vec<BackupItem> {
        self.backups
            .lock()
            .unwrap()
            .iter()
            .filter(|item| belongs_to(item, probe))
            .cloned()
            .collect()
    }

    pub fn changed(&self, seen: &mut u64) -> bool {
        let generation = self.generation.load(Ordering::Relaxed);
        let changed = generation != *seen;
        *seen = generation;
        changed
    }

    /// Read the unit's full flash and store it as a datestamped blob, without
    /// changing anything on the unit.
    pub fn backup(&self, path: &str, probe: Probe) {
        let store = self.store.clone();
        let backups = self.backups.clone();
        let path = path.to_string();
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            if row.busy {
                return;
            }
            row.busy = true;
            row.busy_label = Some("Backing up flash…".into());
            row.outcome = None;
        }
        generation.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("fm1-recovery-backup".into())
            .spawn(move || {
                let result = fm1_recovery::backup_flash(&path, &mut |_| {}, &mut |_, _| {})
                    .and_then(|flash| {
                        save_backup(&store, &backups, &probe, &flash)
                            .map_err(fm1_recovery::Error::Io)
                    });
                if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
                    row.busy = false;
                    row.busy_label = None;
                    row.outcome = Some(match result {
                        Ok(label) => format!("backup saved: {label}"),
                        Err(error) => error.to_string(),
                    });
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }

    /// Write m-bootloader on the chip, in the background. The install path takes a
    /// full-flash backup first and stores it as a datestamped blob.
    pub fn install_bootloader(&self, path: &str, probe: Probe) {
        let store = self.store.clone();
        let backups = self.backups.clone();
        let path = path.to_string();
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            if row.busy {
                return;
            }
            row.busy = true;
            row.busy_label = Some("Installing m-bootloader…".into());
            row.outcome = None;
        }
        generation.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("fm1-recovery-install".into())
            .spawn(move || {
                let mut backup = String::new();
                let result = fm1_recovery::install_bootloader(
                    &path,
                    &mut |flash| save_backup(&store, &backups, &probe, flash),
                    &mut |line| {
                        if let Some(path) = line.strip_prefix("backup saved: ") {
                            backup = path.to_string();
                        }
                    },
                    &mut |_, _| {},
                );
                if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
                    row.busy = false;
                    row.busy_label = None;
                    row.outcome = Some(match result {
                        Ok(()) if backup.is_empty() => {
                            "m-bootloader installed and the boot area verified".into()
                        }
                        Ok(()) => format!("m-bootloader installed and verified; backup {backup}"),
                        Err(error) if backup.is_empty() => error.to_string(),
                        Err(error) => format!("{error}; backup {backup}"),
                    });
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }

    /// Write a stored full-flash backup back to the unit. Only the backup chosen
    /// from the row's own list reaches this, so model, chip key and flash match.
    pub fn restore_backup(&self, path: &str, key: &str) {
        let Some(item) = self
            .backups
            .lock()
            .unwrap()
            .iter()
            .find(|item| item.key == key)
            .cloned()
        else {
            return;
        };
        let store = self.store.clone();
        let path = path.to_string();
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            if row.busy {
                return;
            }
            row.busy = true;
            row.busy_label = Some(format!("Restoring {}…", item.label));
            row.outcome = None;
        }
        generation.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("fm1-recovery-restore".into())
            .spawn(move || {
                let result = store
                    .fetch_blob(&item.key)
                    .map_err(fm1_recovery::Error::Io)
                    .and_then(|bytes| {
                        fm1_recovery::restore_flash(&path, &bytes, &mut |_| {}, &mut |_, _| {})
                    });
                if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
                    row.busy = false;
                    row.busy_label = None;
                    row.outcome = Some(match result {
                        Ok(()) => format!("restored {}; power-cycle the unit", item.label),
                        Err(error) => error.to_string(),
                    });
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }
}

fn belongs_to(item: &BackupItem, probe: &Probe) -> bool {
    item.model == probe.model && item.chip_key == probe.chip_key && item.flash == probe.flash
}

fn load_backups(store: &Store) -> Vec<BackupItem> {
    let mut out: Vec<BackupItem> = store
        .list_objects(Some("recovery/*"))
        .into_iter()
        .filter_map(|key| store.fetch_object::<BackupItem>(&key).ok())
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

fn save_backup(
    store: &Store,
    backups: &Mutex<Vec<BackupItem>>,
    probe: &Probe,
    flash: &[u8],
) -> Result<String, String> {
    if flash.len() != FLASH_SIZE {
        return Err(format!(
            "the flash read {} bytes, not the FM-1's {FLASH_SIZE}",
            flash.len()
        ));
    }
    let created = now_ms();
    let stamp = timestamp(created);
    let key = format!("{BACKUP_PREFIX}{stamp}.bin");
    let item = BackupItem {
        key: key.clone(),
        created,
        label: format!("{stamp} UTC · {} · {}", probe.model, probe.flash),
        model: probe.model.clone(),
        chip_key: probe.chip_key.clone(),
        flash: probe.flash.clone(),
        bytes: flash.len(),
    };
    store.store_blob(&key, flash)?;
    let meta = format!("{BACKUP_PREFIX}{stamp}.json");
    if let Err(error) = store.store_object(&meta, &item) {
        let _ = store.delete_blob(&key);
        return Err(error);
    }
    let mut list = backups.lock().unwrap();
    list.push(item.clone());
    list.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(item.label)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// A filesystem-safe UTC timestamp, `YYYY-MM-DD_HH-MM-SS-mmm`.
fn timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}-{:03}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60,
        ms % 1000
    )
}

/// Howard Hinnant's civil-from-days algorithm; no chrono dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Probe {
        Probe {
            model: "FM-1".into(),
            firmware: "FM-1_900".into(),
            app: "OK".into(),
            bootloader: "stock".into(),
            chip_key: "0x980F".into(),
            flash: "0xB36014".into(),
            certain: true,
        }
    }

    #[test]
    fn backups_are_stored_and_filtered_by_unit() {
        let dir = std::env::temp_dir().join(format!(
            "fm1-grid-recovery-backups-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let store = Store::in_dir(&dir);
        let backups = Mutex::new(load_backups(&store));
        let flash = vec![0x5A; FLASH_SIZE];
        save_backup(&store, &backups, &probe(), &flash).unwrap();
        let listed = backups.lock().unwrap().clone();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].model, "FM-1");
        assert!(listed[0].label.ends_with("FM-1 · 0xB36014"));
        assert_eq!(store.fetch_blob(&listed[0].key).unwrap(), flash);
        let mut other = probe();
        other.chip_key = "0x1234".into();
        assert!(!belongs_to(&listed[0], &other));
        assert!(belongs_to(&listed[0], &probe()));
        assert!(store
            .fetch_object::<BackupItem>(&format!(
                "{BACKUP_PREFIX}{}.json",
                &listed[0].key[BACKUP_PREFIX.len()..listed[0].key.len() - 4]
            ))
            .is_ok());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timestamp_is_sortable_and_has_a_date() {
        assert!(timestamp(0).starts_with("1970-01-01"));
        assert!(timestamp(0).contains("_00-00-00-000"));
    }
}
