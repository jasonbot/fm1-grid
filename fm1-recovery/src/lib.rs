//! Raw USB recovery for FM-1 and SMK-37 download mode.

mod inspect;
mod loader;
mod package;
mod transport;

pub mod backups;

pub use transport::find_devices;

/// The FM-1's full flash size. A backup must be exactly this long.
pub const FLASH_SIZE: usize = inspect::FLASH_SIZE;

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

fn read_flash(
    command: &mut loader::Loader,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<u8>, Error> {
    let flash = command.read_all(&mut |done| progress(done, inspect::FLASH_SIZE))?;
    let chip_crc = command.crc32(0, inspect::FLASH_SIZE)?;
    if !crc32_matches(&flash, chip_crc) {
        return Err(Error::Refused(
            "the flash read did not match the chip checksum".into(),
        ));
    }
    Ok(flash)
}

fn crc32_matches(bytes: &[u8], expected: u32) -> bool {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
        }
        *slot = value;
    }
    let mut crc = !0u32;
    for &byte in bytes {
        crc = table[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc == expected
}

pub fn probe(
    path: &str,
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Probe, Error> {
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
pub fn install_bootloader(
    path: &str,
    save_backup: &mut dyn FnMut(&[u8]) -> Result<String, String>,
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), Error> {
    let mut loader = loader::Loader::open(path, log)?;
    let flash = read_flash(&mut loader, progress)?;
    let analysis = inspect::analyze(&flash, loader.flash_id(), loader.chip_key());
    let bootloader: &[u8] = match analysis.board_key {
        Some("fm1") => include_bytes!("../data/bootloader-fm1.boot"),
        Some("smk37") => include_bytes!("../data/bootloader-smk37.boot"),
        _ => {
            return Err(Error::Refused(
                "the unit's model could not be identified".into(),
            ))
        }
    };
    let area = inspect::build_boot_area(&flash, bootloader).map_err(Error::Refused)?;
    let backup = save_backup(&flash).map_err(Error::Io)?;
    log(format!("backup saved: {backup}"));
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

/// Read the whole flash, for a datestamped backup.
pub fn backup_flash(
    path: &str,
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<u8>, Error> {
    let mut loader = loader::Loader::open(path, log)?;
    read_flash(&mut loader, progress)
}

/// Write a full-flash backup back to a download-mode unit, then verify the write
/// against the chip's own CRC32. The caller supplies the bytes; the backup is not
/// rewritten, so restoring is safe to retry.
pub fn install_firmware(
    path: &str,
    raw: &[u8],
    save_backup: &mut dyn FnMut(&[u8]) -> Result<String, String>,
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), Error> {
    let package = package::Package::parse(raw).map_err(Error::Refused)?;
    if package.flash.len() > FLASH_SIZE {
        return Err(Error::Refused(
            "the package is larger than the flash".into(),
        ));
    }
    let mut loader = loader::Loader::open(path, log)?;
    let current = read_flash(&mut loader, progress)?;
    let analysis = inspect::analyze(&current, loader.flash_id(), loader.chip_key());
    let expected = analysis.model.as_str();
    let package_model = package.model();
    let model_fits = match expected {
        "FM-1" => package_model == "FM-1" || package_model == "MiniFM",
        "SMK-37 Pro / Elite" => package_model.starts_with("SMK-37"),
        _ => false,
    };
    if !model_fits {
        return Err(Error::Refused(format!(
            "the package is for {package_model}, the unit is {expected}"
        )));
    }
    let pkg_analysis = inspect::analyze(&package.flash, loader.flash_id(), loader.chip_key());
    if pkg_analysis.app != "OK" {
        return Err(Error::Refused(
            "the package's firmware does not decrypt with this unit's chip key".into(),
        ));
    }
    let end = package.flash.len();
    if end <= inspect::BOOT_AREA {
        return Err(Error::Refused(
            "the package has no application image".into(),
        ));
    }
    let mut target = current.clone();
    target[inspect::BOOT_AREA..end].copy_from_slice(&package.flash[inspect::BOOT_AREA..end]);
    let mut runs = Vec::new();
    for at in (inspect::BOOT_AREA..end).step_by(0x1000) {
        if current[at..at + 0x1000] != target[at..at + 0x1000] {
            match runs.last_mut() {
                Some((_, run_end)) if *run_end == at => *run_end = at + 0x1000,
                _ => runs.push((at, at + 0x1000)),
            }
        }
    }
    if runs.is_empty() {
        log("the firmware is already installed".into());
        return Ok(());
    }
    let backup = save_backup(&current).map_err(Error::Io)?;
    log(format!("backup saved: {backup}"));
    for (start, run_end) in runs {
        loader.erase(start, run_end - start)?;
        loader.write(start, &target[start..run_end], &mut |done| {
            progress(done, end - inspect::BOOT_AREA)
        })?;
    }
    log("checking the written flash".into());
    let actual = loader.crc32(0, FLASH_SIZE)?;
    if !crc32_matches(&target, actual) {
        return Err(Error::Refused(
            "the written firmware did not match its checksum".into(),
        ));
    }
    Ok(())
}

pub fn restore_flash(
    path: &str,
    bytes: &[u8],
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), Error> {
    if bytes.len() != FLASH_SIZE {
        return Err(Error::Refused(format!(
            "the backup is {} bytes, not the FM-1's {FLASH_SIZE} bytes",
            bytes.len()
        )));
    }
    let mut loader = loader::Loader::open(path, log)?;
    log("erasing the unit's flash".into());
    loader.erase(0, FLASH_SIZE)?;
    log("writing the backup".into());
    loader.write(0, bytes, &mut |done| progress(done, FLASH_SIZE))?;
    log("checking the written flash".into());
    let actual = loader.crc32(0, FLASH_SIZE)?;
    if !crc32_matches(bytes, actual) {
        return Err(Error::Refused(
            "the written flash did not match the backup's CRC32".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_rejects_a_backup_of_the_wrong_size() {
        let mut log = |_| {};
        let mut progress = |_, _| {};
        let error = restore_flash("no-device", &[], &mut log, &mut progress).unwrap_err();
        assert!(error.to_string().contains("not the FM-1"));
    }
}
