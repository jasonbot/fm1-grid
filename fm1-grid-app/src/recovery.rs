//! Download-mode units found through the chip's raw USB recovery path.
//!
//! The MIDI scanner cannot see a unit sitting in the chip's ROM loader; this
//! thread checks the SCSI/USB download-mode list, reads each unit's flash once
//! and keeps the model, firmware and bootloader text for the Devices pane.

use fm1_recovery::Probe;
use std::path::PathBuf;
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
    pub outcome: Option<String>,
}

pub struct RecoveryState {
    rows: Arc<Mutex<Vec<RecoveryRow>>>,
    generation: Arc<AtomicU64>,
    backup_dir: PathBuf,
}

impl RecoveryState {
    pub fn start() -> Self {
        let rows: Arc<Mutex<Vec<RecoveryRow>>> = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let backup_dir = fm1_recovery::backup_dir();
        let state = Self {
            rows: rows.clone(),
            generation: generation.clone(),
            backup_dir,
        };
        let backup = state.backup_dir.clone();
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
                        outcome: None,
                    };
                    // Reading a unit is ~1 MiB and must not hold the UI lock.
                    match fm1_recovery::probe(&path, backup.clone(), &mut |_| {}, &mut |_, _| {}) {
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

    pub fn changed(&self, seen: &mut u64) -> bool {
        let generation = self.generation.load(Ordering::Relaxed);
        let changed = generation != *seen;
        *seen = generation;
        changed
    }

    /// Write m-bootloader on the chip, in the background. `apply` takes its own
    /// full-flash backup before touching the boot area.
    pub fn install_bootloader(&self, path: &str) {
        let backup = self.backup_dir.clone();
        let path = path.to_string();
        let rows = self.rows.clone();
        let generation = self.generation.clone();
        if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
            if row.busy {
                return;
            }
            row.busy = true;
            row.outcome = None;
        }
        generation.fetch_add(1, Ordering::Relaxed);
        std::thread::Builder::new()
            .name("fm1-recovery-install".into())
            .spawn(move || {
                let result = fm1_recovery::install_bootloader(
                    &path,
                    backup,
                    &mut |_| {},
                    &mut |_, _| {},
                );
                if let Some(row) = rows.lock().unwrap().iter_mut().find(|row| row.path == path) {
                    row.busy = false;
                    row.outcome = Some(match result {
                        Ok(()) => "m-bootloader installed and the boot area verified".into(),
                        Err(error) => error.to_string(),
                    });
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }
}
