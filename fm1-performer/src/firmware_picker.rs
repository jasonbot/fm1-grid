//! What picking a firmware means: [`FirmwarePick`] names the install/flash source (a
//! builtin index, a file, a downloaded hub blob) and [`pick_of`] maps a library entry
//! to one. Zip-wrapped downloads are unwrapped on the way in. The gpui-kit combobox
//! delegate that presents these lives in the fm1-grid crate.

use crate::builtin::BUILTINS;
use crate::library::FirmwareSource;
use std::path::PathBuf;

/// What a pick installs (or flashes).
#[derive(Clone, PartialEq)]
pub enum FirmwarePick {
    Builtin(usize),
    File(PathBuf),
    /// A downloaded hub package, by its blob-store key.
    HubBlob(String),
}

impl FirmwarePick {
    /// The package bytes: the builtin table, the filesystem, or the blob store.
    pub fn bytes(&self, store: Option<&crate::store::Store>) -> Result<Vec<u8>, String> {
        match self {
            FirmwarePick::Builtin(index) => Ok(BUILTINS[*index].bytes.to_vec()),
            FirmwarePick::File(path) => {
                std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
            }
            FirmwarePick::HubBlob(key) => store
                .ok_or("no on-disk store".to_string())
                .and_then(|store| store.fetch_blob(key))
                .and_then(unwrap_fwsc),
        }
    }
}

/// Some download URLs wrap the package in a zip; installing pulls the first .fwsc
/// member out. Anything that isn't a zip passes through untouched — except a web
/// page: a listing that linked an HTML page instead of a package used to download
/// the page itself, and booting it does nothing. Fail those loudly.
fn unwrap_fwsc(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(b"PK\x03\x04") {
        if crate::download_heuristics::looks_like_web_page(&bytes[..bytes.len().min(512)]) {
            return Err(
                "the download is a web page, not a firmware package — download it again"
                    .to_string(),
            );
        }
        return Ok(bytes);
    }
    zip_first_fwsc(&bytes)
}

/// The first `.fwsc` member of a zip, found through the central directory (local
/// headers may carry zeroed sizes with the data in a trailing descriptor, so the
/// central directory is the only reliable index). Methods: stored and deflated.
fn zip_first_fwsc(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let u16_at = |at: usize| -> Result<u16, String> {
        let field = bytes
            .get(at..at + 2)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        Ok(u16::from_le_bytes([field[0], field[1]]))
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let field = bytes
            .get(at..at + 4)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
    };
    // The end-of-central-directory record sits within the trailing 64 KiB + 22.
    let scan_from = bytes.len().saturating_sub(22 + 65_535);
    let eocd = bytes[scan_from..]
        .windows(22)
        .rposition(|window| window[..4] == *b"PK\x05\x06")
        .map(|i| scan_from + i)
        .ok_or_else(|| "zip: no end of central directory".to_string())?;
    let entries = u16_at(eocd + 10)? as usize;
    let mut directory = u32_at(eocd + 16)? as usize;
    for _ in 0..entries {
        if bytes.get(directory..directory + 4) != Some(b"PK\x01\x02") {
            return Err("zip: bad central directory".to_string());
        }
        let method = u16_at(directory + 10)?;
        let packed = u32_at(directory + 20)? as usize;
        let unpacked = u32_at(directory + 24)? as usize;
        let name_len = u16_at(directory + 28)? as usize;
        let extra_len = u16_at(directory + 30)? as usize;
        let comment_len = u16_at(directory + 32)? as usize;
        let local = u32_at(directory + 42)? as usize;
        let name = bytes
            .get(directory + 46..directory + 46 + name_len)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        let name = String::from_utf8_lossy(name).into_owned();
        if name.to_ascii_lowercase().ends_with(".fwsc") {
            if bytes.get(local..local + 4) != Some(b"PK\x03\x04") {
                return Err(format!("zip: {name} has a bad local header"));
            }
            let local_name = u16_at(local + 26)? as usize;
            let local_extra = u16_at(local + 28)? as usize;
            let data = bytes
                .get(local + 30 + local_name + local_extra..local + 30 + local_name + local_extra + packed)
                .ok_or_else(|| "zip: truncated archive".to_string())?;
            let member = match method {
                0 => data.to_vec(),
                8 => {
                    use std::io::Read;
                    let mut out = Vec::with_capacity(unpacked);
                    flate2::read::DeflateDecoder::new(data)
                        .read_to_end(&mut out)
                        .map_err(|e| format!("zip: inflate {name}: {e}"))?;
                    out
                }
                other => return Err(format!("zip: {name} uses unsupported method {other}")),
            };
            if member.len() != unpacked {
                return Err(format!(
                    "zip: {name} unpacked to {} bytes, expected {unpacked}",
                    member.len()
                ));
            }
            return Ok(member);
        }
        directory += 46 + name_len + extra_len + comment_len;
    }
    Err("zip: no .fwsc member".to_string())
}

/// The install/flash target for a library entry: None when the package isn't at hand
/// (a hub offer not downloaded yet, or a file that's gone).
pub fn pick_of(firmware: &crate::library::Firmware) -> Option<FirmwarePick> {
    match &firmware.source {
        FirmwareSource::Builtin => BUILTINS
            .iter()
            .position(|b| b.name == firmware.name && b.version == firmware.version)
            .map(FirmwarePick::Builtin),
        FirmwareSource::File { path } => Some(FirmwarePick::File(PathBuf::from(path))),
        FirmwareSource::Hub { blob: Some(key), .. } => Some(FirmwarePick::HubBlob(key.clone())),
        FirmwareSource::Hub { blob: None, .. } => None,
    }
}

#[cfg(test)]
mod zip_tests {
    use super::*;

    /// A minimal zip with the given members (name, method, plain content); CRCs are
    /// zero — the reader takes the central directory's sizes, not its checksums.
    fn zip_with(members: &[(&str, u16, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, method, content) in members {
            let stored = match method {
                0 => content.to_vec(),
                8 => {
                    let mut encoder = flate2::write::DeflateEncoder::new(
                        Vec::new(),
                        flate2::Compression::fast(),
                    );
                    encoder.write_all(content).unwrap();
                    encoder.finish().unwrap()
                }
                other => panic!("test zip: method {other}"),
            };
            let offset = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc
            out.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0; 2]); // extra length
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&stored);

            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]); // time, date, crc
            central.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 8]); // extra, comment, disk, attributes
            central.extend_from_slice(&[0; 4]); // external attributes
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0; 4]); // disk numbers
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&[0; 2]); // comment length
        out
    }

    #[test]
    fn zip_yields_its_fwsc_member_stored_or_deflated() {
        // The .fwsc is picked by name, not position: a readme comes first.
        let zip = zip_with(&[
            ("README.txt", 0, b"read me first"),
            ("firmware/pack.fwsc", 0, b"the package bytes"),
        ]);
        assert_eq!(unwrap_fwsc(zip).unwrap(), b"the package bytes");
        let zip = zip_with(&[("pack.fwsc", 8, b"the deflated package bytes")]);
        assert_eq!(unwrap_fwsc(zip).unwrap(), b"the deflated package bytes");
    }

    #[test]
    fn non_zip_bytes_pass_through_and_zip_without_fwsc_fails() {
        assert_eq!(unwrap_fwsc(b"raw firmware".to_vec()).unwrap(), b"raw firmware");
        let zip = zip_with(&[("notes.txt", 0, b"hi")]);
        assert_eq!(unwrap_fwsc(zip).unwrap_err(), "zip: no .fwsc member");
        assert!(unwrap_fwsc(b"PK\x03\x04 too short".to_vec()).is_err());
    }

    #[test]
    fn hub_blob_pick_unpacks_the_fwsc() {
        let dir = std::env::temp_dir().join(format!("fm1-grid-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = crate::store::Store::in_dir(&dir);
        let zip = zip_with(&[("pack.fwsc", 8, b"flashable bytes")]);
        store.store_blob("hub/pack.zip", &zip).unwrap();
        let pick = FirmwarePick::HubBlob("hub/pack.zip".to_string());
        assert_eq!(pick.bytes(Some(&store)).unwrap(), b"flashable bytes");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A listing that linked an HTML page used to store the page as the "package";
    /// booting it did nothing. Those blobs fail loudly now.
    #[test]
    fn html_blobs_are_rejected() {
        assert!(unwrap_fwsc(b"<!DOCTYPE html><html><body>page</body></html>".to_vec()).is_err());
        assert!(unwrap_fwsc(b"<html><body>page</body></html>".to_vec()).is_err());
        assert!(unwrap_fwsc(b"  \n<!doctype html>".to_vec()).is_err());
        // Real packages and other binaries still pass through.
        assert_eq!(unwrap_fwsc(b"fw-binary-bytes".to_vec()).unwrap(), b"fw-binary-bytes");
    }
}
