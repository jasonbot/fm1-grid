//! Full-flash backups in the on-disk blob store: one `.bin` plus a `.json`
//! record per backup, so both apps can list and restore the same set.

use crate::{Probe, FLASH_SIZE};
use fm1_base::store::Store;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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
}

/// A full-flash image carries settings and the chip-key record, so a backup from
/// another unit is not offered.
pub fn belongs_to(item: &BackupItem, probe: &Probe) -> bool {
    item.model == probe.model && item.chip_key == probe.chip_key && item.flash == probe.flash
}

pub fn load_backups(store: &Store) -> Vec<BackupItem> {
    let mut out: Vec<BackupItem> = store
        .list_objects(Some("recovery/*"))
        .into_iter()
        .filter_map(|key| store.fetch_object::<BackupItem>(&key).ok())
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn save_backup(store: &Store, probe: &Probe, flash: &[u8]) -> Result<BackupItem, String> {
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
    };
    store.store_blob(&key, flash)?;
    let meta = format!("{BACKUP_PREFIX}{stamp}.json");
    if let Err(error) = store.store_object(&meta, &item) {
        let _ = store.delete_blob(&key);
        return Err(error);
    }
    Ok(item)
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
            "fm1-recovery-backups-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let store = Store::in_dir(&dir);
        let flash = vec![0x5A; FLASH_SIZE];
        let saved = save_backup(&store, &probe(), &flash).unwrap();
        let listed = load_backups(&store);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, saved.key);
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
