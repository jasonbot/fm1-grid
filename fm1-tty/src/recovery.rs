//! Download-mode units found through the chip's raw USB recovery path, plus a
//! background firmware flash into one. The TUI's MIDI path cannot reach a unit
//! whose firmware no longer accepts an update; this can.

use fm1_performer::store::Store;
use fm1_recovery::Probe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct Unit {
    pub path: String,
    pub description: String,
    pub probe: Option<Probe>,
    pub error: Option<String>,
    pub busy: bool,
    pub outcome: Option<String>,
}

pub struct Recovery {
    units: Arc<Mutex<Vec<Unit>>>,
}

impl Recovery {
    pub fn start() -> Self {
        let units: Arc<Mutex<Vec<Unit>>> = Arc::new(Mutex::new(Vec::new()));
        let scan = units.clone();
        std::thread::Builder::new()
            .name("fm1-tty-recovery".into())
            .spawn(move || loop {
                let found = fm1_recovery::find_devices();
                let old = scan.lock().unwrap().clone();
                let mut current = Vec::with_capacity(found.len());
                for (path, description) in found {
                    if let Some(mut unit) = old.iter().find(|unit| unit.path == path).cloned() {
                        unit.description = description;
                        current.push(unit);
                        continue;
                    }
                    let mut unit = Unit {
                        path: path.clone(),
                        description,
                        probe: None,
                        error: None,
                        busy: false,
                        outcome: None,
                    };
                    match fm1_recovery::probe(&path, &mut |_| {}, &mut |_, _| {}) {
                        Ok(probe) => unit.probe = Some(probe),
                        Err(error) => unit.error = Some(error.to_string()),
                    }
                    current.push(unit);
                }
                // Probing new units takes seconds; keep the busy/outcome state the
                // flash threads wrote to the live list meanwhile.
                let mut live = scan.lock().unwrap();
                for unit in current.iter_mut() {
                    let Some(existing) = live.iter().find(|existing| existing.path == unit.path)
                    else {
                        continue;
                    };
                    unit.busy = existing.busy;
                    unit.outcome = existing.outcome.clone();
                    if unit.probe.is_none() {
                        unit.probe = existing.probe.clone();
                    }
                    if unit.error.is_none() {
                        unit.error = existing.error.clone();
                    }
                }
                *live = current;
                // The UI clones this list every frame; never sleep holding the lock.
                drop(live);
                std::thread::sleep(Duration::from_secs(2));
            })
            .expect("spawn recovery scanner");
        Self { units }
    }

    pub fn units(&self) -> Vec<Unit> {
        self.units.lock().unwrap().clone()
    }

    /// Flash a library package into the unit's application area. A full-flash
    /// backup is written to the store before the first write.
    pub fn flash(&self, path: &str, bytes: Vec<u8>, store: Option<Store>) {
        let units = self.units.clone();
        let path = path.to_string();
        let probe = {
            let mut units = units.lock().unwrap();
            let Some(unit) = units.iter_mut().find(|unit| unit.path == path) else {
                return;
            };
            if unit.busy {
                return;
            }
            unit.busy = true;
            unit.outcome = None;
            unit.probe.clone()
        };
        std::thread::Builder::new()
            .name("fm1-tty-recovery-flash".into())
            .spawn(move || {
                let result = flash_one(&path, probe.as_ref(), &bytes, store);
                if let Some(unit) = units.lock().unwrap().iter_mut().find(|unit| unit.path == path) {
                    unit.busy = false;
                    unit.outcome = Some(match result {
                        Ok(message) => message,
                        Err(error) => error,
                    });
                }
            })
            .ok();
    }
}

fn flash_one(
    path: &str,
    probe: Option<&Probe>,
    bytes: &[u8],
    store: Option<Store>,
) -> Result<String, String> {
    let store = store.ok_or("no on-disk store for a full-flash backup")?;
    let probe = probe.ok_or("the unit has not been identified yet")?;
    let mut backup = String::new();
    fm1_recovery::install_firmware(
        path,
        bytes,
        &mut |flash| {
            let item = fm1_recovery::backups::save_backup(&store, probe, flash)?;
            backup = item.label.clone();
            Ok(item.label)
        },
        &mut |_| {},
        &mut |_, _| {},
    )
    .map_err(|error| error.to_string())?;
    if backup.is_empty() {
        Ok("firmware is already installed".into())
    } else {
        Ok(format!("firmware flashed; power-cycle the unit; backup {backup}"))
    }
}
