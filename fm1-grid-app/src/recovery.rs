//! Download-mode units found through the chip's raw USB recovery path.
//!
//! The MIDI scanner cannot see a unit sitting in the chip's ROM loader; this
//! thread checks the SCSI/USB download-mode list, reads each unit's flash once
//! and keeps the model, firmware and bootloader text for the Devices pane. Full
//! flash backups live in the app's blob store so they can be listed and restored
//! from the same pane.

use fm1_performer::store::Store;
use fm1_recovery::backups::{belongs_to, load_backups, BackupItem};
use fm1_recovery::Probe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
                // Probing new units takes seconds; keep the busy/outcome state the
                // task threads wrote to the live list meanwhile.
                let mut live = rows.lock().unwrap();
                for row in current.iter_mut() {
                    let Some(existing) = live.iter().find(|existing| existing.path == row.path)
                    else {
                        continue;
                    };
                    row.busy = existing.busy;
                    row.busy_label = existing.busy_label.clone();
                    row.outcome = existing.outcome.clone();
                    if row.probe.is_none() {
                        row.probe = existing.probe.clone();
                    }
                    if row.error.is_none() {
                        row.error = existing.error.clone();
                    }
                }
                *live = current;
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
        let task_path = path.to_string();
        self.run(path, "Backing up flash…", move || {
            let flash = fm1_recovery::backup_flash(&task_path, &mut |_| {}, &mut |_, _| {})
                .map_err(|error| error.to_string())?;
            let label = save_backup(&store, &backups, &probe, &flash)?;
            Ok(format!("backup saved: {label}"))
        });
    }

    /// Write m-bootloader on the chip, in the background. The install path takes a
    /// full-flash backup first and stores it as a datestamped blob.
    pub fn install_bootloader(&self, path: &str, probe: Probe) {
        let store = self.store.clone();
        let backups = self.backups.clone();
        let task_path = path.to_string();
        self.run(path, "Installing m-bootloader…", move || {
            let mut backup = String::new();
            let result = fm1_recovery::install_bootloader(
                &task_path,
                &mut |flash| save_backup(&store, &backups, &probe, flash),
                &mut |line| {
                    if let Some(path) = line.strip_prefix("backup saved: ") {
                        backup = path.to_string();
                    }
                },
                &mut |_, _| {},
            );
            match result {
                Ok(()) if backup.is_empty() => {
                    Ok("m-bootloader installed and the boot area verified".into())
                }
                Ok(()) => Ok(format!(
                    "m-bootloader installed and verified; backup {backup}"
                )),
                Err(error) if backup.is_empty() => Err(error.to_string()),
                Err(error) => Err(format!("{error}; backup {backup}")),
            }
        });
    }

    /// Install a library `.fwsc` package over the unit's application area. The
    /// boot area and the unit's data outside the package stay as they are.
    pub fn flash_firmware(&self, path: &str, bytes: Vec<u8>, probe: Probe) {
        let store = self.store.clone();
        let backups = self.backups.clone();
        let task_path = path.to_string();
        self.run(path, "Flashing firmware…", move || {
            let mut backup = String::new();
            let result = fm1_recovery::install_firmware(
                &task_path,
                &bytes,
                &mut |flash| save_backup(&store, &backups, &probe, flash),
                &mut |line| {
                    if let Some(path) = line.strip_prefix("backup saved: ") {
                        backup = path.to_string();
                    }
                },
                &mut |_, _| {},
            );
            match result {
                Ok(()) if backup.is_empty() => Ok("firmware is already installed".into()),
                Ok(()) => Ok(format!(
                    "firmware flashed; power-cycle the unit; backup {backup}"
                )),
                Err(error) if backup.is_empty() => Err(error.to_string()),
                Err(error) => Err(format!("{error}; backup {backup}")),
            }
        });
    }

    /// Put a message on one recovery row.
    pub fn report(&self, path: &str, message: String) {
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            row.outcome = Some(message);
        }
        generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Write a stored full-flash backup back to the unit. Only backups belonging to
    /// the row's own unit are offered.
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
        let task_path = path.to_string();
        self.run(path, &format!("Restoring {}…", item.label), move || {
            let bytes = store
                .fetch_blob(&item.key)
                .map_err(fm1_recovery::Error::Io)
                .map_err(|error| error.to_string())?;
            fm1_recovery::restore_flash(&task_path, &bytes, &mut |_| {}, &mut |_, _| {})
                .map_err(|error| error.to_string())?;
            Ok(format!("restored {}; power-cycle the unit", item.label))
        });
    }

    fn run(
        &self,
        path: &str,
        busy_label: &str,
        task: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        let path = path.to_string();
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            if row.busy {
                return;
            }
            row.busy = true;
            row.busy_label = Some(busy_label.to_string());
            row.outcome = None;
        } else {
            return;
        }
        generation.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("fm1-recovery-task".into())
            .spawn(move || {
                let outcome = match task() {
                    Ok(message) => message,
                    Err(error) => error,
                };
                if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
                    row.busy = false;
                    row.busy_label = None;
                    row.outcome = Some(outcome);
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }
}

/// Store a backup and refresh the GUI's in-memory list.
fn save_backup(
    store: &Store,
    backups: &Mutex<Vec<BackupItem>>,
    probe: &Probe,
    flash: &[u8],
) -> Result<String, String> {
    let item = fm1_recovery::backups::save_backup(store, probe, flash)?;
    let mut list = backups.lock().unwrap();
    list.push(item.clone());
    list.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(item.label)
}

