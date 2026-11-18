//! Download-mode units found through the chip's raw USB recovery path, plus a
//! background firmware flash into one. The TUI's MIDI path cannot reach a unit
//! whose firmware no longer accepts an update; this can.

use fm1_performer::store::Store;
use fm1_recovery::Probe;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
                *scan.lock().unwrap() = current;
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
        if let Some(unit) = units.lock().unwrap().iter_mut().find(|unit| unit.path == path) {
            if unit.busy {
                return;
            }
            unit.busy = true;
            unit.outcome = None;
        }
        std::thread::Builder::new()
            .name("fm1-tty-recovery-flash".into())
            .spawn(move || {
                let result = flash_one(&path, &bytes, store);
                if let Some(unit) = units.lock().unwrap().iter_mut().find(|unit| unit.path == path) {
                    unit.busy = false;
                    unit.outcome = Some(match result {
                        Ok(()) => "firmware flashed; power-cycle the unit".into(),
                        Err(error) => error,
                    });
                }
            })
            .ok();
    }
}

fn flash_one(path: &str, bytes: &[u8], store: Option<Store>) -> Result<(), String> {
    let store = store.ok_or("no on-disk store for a full-flash backup")?;
    let mut backup = String::new();
    fm1_recovery::install_firmware(
        path,
        bytes,
        &mut |flash| {
            let key = format!(
                "recovery/tty-{}.bin",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            store.store_blob(&key, flash)?;
            backup = key.clone();
            Ok(key)
        },
        &mut |_| {},
        &mut |_, _| {},
    )
    .map_err(|error| error.to_string())?;
    let _ = backup;
    Ok(())
}
