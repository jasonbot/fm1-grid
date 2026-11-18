//! M-VAVE `.fwsc` packages: the UFW container and its `flash.bin` image.

/// One UFW file entry: type, data CRC16, offset, size and name.
struct Entry {
    kind: u16,
    crc: u16,
    offset: usize,
    size: usize,
    name: String,
}

pub struct Package {
    pub identity: String,
    pub flash: Vec<u8>,
}

impl Package {
    /// Parse the identity markers and the UFW container, then return the
    /// `flash.bin` image (encrypted for the chip key it was made with).
    pub fn parse(raw: &[u8]) -> Result<Self, String> {
        let bad = || "not a firmware package (.fwsc)".to_string();
        if raw.len() < 0x1000 {
            return Err(bad());
        }
        let mut blocks = 0;
        while blocks < 20 || (blocks * 0x30 + 0x2F < raw.len() && raw[blocks * 0x30 + 0x2F] == 0x7D)
        {
            blocks += 1;
            if (blocks + 1) * 0x30 > raw.len() {
                return Err(bad());
            }
        }
        let mut identity = String::new();
        for i in 0..blocks {
            let marker = raw[i * 0x30 + 0x2F];
            if marker == 0x7D {
                break;
            }
            identity.push(marker.wrapping_sub(i as u8 + 1) as char);
        }
        let mut image = Vec::with_capacity(raw.len());
        for i in 0..blocks {
            image.extend_from_slice(&raw[i * 0x30..i * 0x30 + 0x2F]);
        }
        image.extend_from_slice(&raw[blocks * 0x30..]);

        let head = super::inspect::decrypt(&image[..0x40], 0xFFFF);
        if u16le(&head, 0) != super::inspect::crc16(&head[2..]) {
            return Err("not a firmware package, or a damaged one (header CRC)".into());
        }
        let count = u16le(&head, 8) as usize;
        if 0x40 + 0x50 * count > image.len() {
            return Err("damaged firmware package (file list)".into());
        }
        let list = &image[0x40..0x40 + 0x50 * count];
        if u16le(&head, 2) != super::inspect::crc16(list) {
            return Err("damaged firmware package (file list CRC)".into());
        }
        let mut flash = None;
        for i in 0..count {
            let entry = entry(&list[i * 0x50..(i + 1) * 0x50]);
            if entry.offset + entry.size > image.len() {
                return Err("damaged firmware package (file outside the image)".into());
            }
            if entry.name == "flash.bin" && entry.kind == 0 {
                let data = image[entry.offset..entry.offset + entry.size].to_vec();
                if super::inspect::crc16(&data) != entry.crc {
                    return Err("damaged firmware package (flash.bin CRC)".into());
                }
                flash = Some(data);
            }
        }
        let Some(flash) = flash else {
            return Err("the package has no flash image (flash.bin)".into());
        };
        if flash.len() <= super::inspect::BOOT_AREA || flash.len() % 0x1000 != 0 {
            return Err(format!("unexpected flash image size {}", flash.len()));
        }
        Ok(Self { identity, flash })
    }

    /// The model name without its version (`FM-1`, `SMK-37 Pro`, ...).
    pub fn model(&self) -> &str {
        self.identity
            .rsplit_once('_')
            .map_or(self.identity.as_str(), |(model, _)| model)
    }
}

fn entry(raw: &[u8]) -> Entry {
    let plain = super::inspect::decrypt(raw, 0xFFFF);
    let name = plain[0x40..]
        .iter()
        .position(|&byte| byte == 0)
        .map_or_else(|| &plain[0x40..], |end| &plain[0x40..0x40 + end]);
    Entry {
        kind: u16le(&plain, 0),
        crc: u16le(&plain, 4),
        offset: u32le(&plain, 8) as usize,
        size: u32le(&plain, 12) as usize,
        name: String::from_utf8_lossy(name).into_owned(),
    }
}

fn u16le(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
