//! Raw USB recovery for FM-1 and SMK-37 download mode.

mod inspect;
mod loader;
mod transport;

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub use transport::find_devices;

#[derive(Debug)]
pub enum Error {
    Transport(transport::TransportError),
    Refused(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "{error}"),
            Self::Refused(message) | Self::Io(message) => f.write_str(message),
        }
    }
}

impl From<transport::TransportError> for Error {
    fn from(error: transport::TransportError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub model: String,
    pub firmware: String,
    pub app: String,
    pub bootloader: String,
    pub chip_key: String,
    pub flash: String,
    pub certain: bool,
}

fn read_flash(command: &mut loader::Loader, progress: &mut dyn FnMut(usize, usize)) -> Result<Vec<u8>, Error> {
    let flash = command.read_all(&mut |done| progress(done, inspect::FLASH_SIZE))?;
    let chip_crc = command.crc32(0, inspect::FLASH_SIZE)?;
    if !crc32_matches(&flash, chip_crc) {
        return Err(Error::Refused("the flash read did not match the chip checksum".into()));
    }
    Ok(flash)
}

fn crc32_matches(bytes: &[u8], expected: u32) -> bool {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 != 0 { 0xEDB8_8320 ^ (value >> 1) } else { value >> 1 };
        }
        *slot = value;
    }
    let mut crc = !0u32;
    for &byte in bytes {
        crc = table[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc == expected
}

pub fn probe(path: &str, backup_dir: PathBuf, log: &mut dyn FnMut(String), progress: &mut dyn FnMut(usize, usize)) -> Result<Probe, Error> {
    let _ = backup_dir;
    let mut loader = loader::Loader::open(path, log)?;
    let flash = read_flash(&mut loader, progress)?;
    let analysis = inspect::analyze(&flash, loader.flash_id(), loader.chip_key());
    Ok(Probe {
        model: analysis.model,
        firmware: analysis.firmware,
        app: analysis.app,
        bootloader: analysis.bootloader,
        chip_key: format!("0x{:04X}", loader.chip_key()),
        flash: format!("0x{:06X}", loader.flash_id()),
        certain: analysis.model_known,
    })
}

/// Install the bundled open bootloader after taking a full-flash backup and
/// verifying the write.
pub fn install_bootloader(path: &str, backup_dir: PathBuf, log: &mut dyn FnMut(String), progress: &mut dyn FnMut(usize, usize)) -> Result<(), Error> {
    let mut loader = loader::Loader::open(path, log)?;
    let flash = read_flash(&mut loader, progress)?;
    let analysis = inspect::analyze(&flash, loader.flash_id(), loader.chip_key());
    let bootloader: &[u8] = match analysis.board_key {
        Some("fm1") => include_bytes!("../data/bootloader-fm1.boot"),
        Some("smk37") => include_bytes!("../data/bootloader-smk37.boot"),
        _ => return Err(Error::Refused("the unit's model could not be identified".into())),
    };
    let area = inspect::build_boot_area(&flash, bootloader).map_err(Error::Refused)?;
    let backup = write_backup(&backup_dir, &flash).map_err(Error::Io)?;
    log(format!("backup saved: {}", backup.display()));
    loader.erase(0, inspect::BOOT_AREA)?;
    loader.write(0, &area, &mut |done| progress(done, inspect::BOOT_AREA))?;
    let verify = loader.read(0, inspect::BOOT_AREA, &mut |_| {})?;
    if verify != area {
        return Err(Error::Refused(
            "the boot area did not verify; do not power off, reconnect and retry".into(),
        ));
    }
    Ok(())
}

fn write_backup(dir: &Path, flash: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("fm1-flash-{seconds}.bin"));
    std::fs::write(&path, flash).map_err(|error| error.to_string())?;
    let hash = Sha256::digest(flash);
    let hash: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    std::fs::write(
        path.with_extension("json"),
        format!(
            "{{\"file\":\"{}\",\"sha256\":\"{hash}\",\"bytes\":{}}}\n",
            path.file_name().unwrap_or_default().to_string_lossy(),
            flash.len()
        ),
    )
    .map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn backup_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    let documents = home.join("Documents");
    (if documents.is_dir() { documents } else { home })
        .join("FM1-Grid")
        .join("recovery")
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_directory_is_under_the_user_documents() {
        assert!(super::backup_dir().ends_with("recovery"));
    }
}
