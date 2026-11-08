//! Flash-layout parsing used to name the board, firmware and bootloader, and
//! to build a replacement boot area.

pub const FLASH_SIZE: usize = 0x100000;
pub const BOOT_AREA: usize = 0x4000;
pub const BOOT_AT: usize = 0xA0;
const CONFIG_AT: usize = 0x3FA0;
const CONFIG_KEY: usize = 34;
const CONFIG_SIZE: usize = BOOT_AREA - CONFIG_AT;
const CONFIG_SETTINGS: [u8; CONFIG_SIZE - CONFIG_KEY] = *b"\x04OSC\0OSC0\
\x04OSC_FREQ\0\x00\x36\x6E\x01\
\x04SYS_CLK\0\x00\x2A\x75\x15\
\x04HSB_DIV\0\x01\0\0\0\
\x04LSB_DIV\0\x02\0\0\0";

pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn decrypt(bytes: &[u8], mut key: u16) -> Vec<u8> {
    bytes
        .iter()
        .map(|&byte| {
            let plain = byte ^ key as u8;
            key = (key << 1) ^ if key & 0x8000 != 0 { 0x1021 } else { 0 };
            plain
        })
        .collect()
}

fn encrypt(bytes: &[u8], key: u16) -> Vec<u8> {
    decrypt(bytes, key)
}

fn u16le(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn cstr(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    bytes[..end].iter().map(|&byte| byte as char).collect()
}

#[derive(Clone)]
struct Entry {
    raw: [u8; 32],
    data_crc: u16,
    addr: u32,
    len: u32,
    name: String,
    valid: bool,
}

fn entry(raw: &[u8]) -> Entry {
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&raw[..32]);
    Entry {
        raw: bytes,
        data_crc: u16le(&bytes, 2),
        addr: u32le(&bytes, 4),
        len: u32le(&bytes, 8),
        name: cstr(&bytes[16..32]),
        valid: crc16(&bytes[2..]) == u16le(&bytes, 0),
    }
}

fn header_ok(flash: &[u8]) -> bool {
    if flash.len() < 32 {
        return false;
    }
    let head = decrypt(&flash[..32], 0xFFFF);
    crc16(&head[2..]) == u16le(&head, 0)
}

fn boot_table(flash: &[u8]) -> Vec<Entry> {
    if flash.len() < BOOT_AREA || !header_ok(flash) {
        return vec![];
    }
    let mut entries = vec![];
    for at in (0x20..BOOT_AT).step_by(0x20) {
        let parsed = entry(&decrypt(&flash[at..at + 32], 0xFFFF));
        if !parsed.valid {
            break;
        }
        entries.push(parsed);
    }
    entries
}

fn app_plain(flash: &[u8], offset: usize, length: usize, key: u16) -> Vec<u8> {
    let mut output = Vec::with_capacity(length + 64);
    let mut at = offset - offset % 32;
    while at < offset + length {
        let start = BOOT_AREA + at;
        if start + 32 > flash.len() {
            break;
        }
        output.extend(decrypt(&flash[start..start + 32], key ^ ((at >> 2) as u16)));
        at += 32;
    }
    let skip = offset % 32;
    output.into_iter().skip(skip).take(length).collect()
}

fn app_table(flash: &[u8], key: u16) -> Vec<Entry> {
    let mut entries = vec![];
    for index in 0..16 {
        let raw = app_plain(flash, index * 32, 32, key);
        if raw.len() < 32 {
            break;
        }
        let parsed = entry(&raw);
        if !parsed.valid {
            break;
        }
        entries.push(parsed);
    }
    entries
}

fn stored_bootloader(flash: &[u8]) -> Option<Vec<u8>> {
    let table = boot_table(flash);
    let first = table.first()?;
    if first.name != "uboot.boot"
        || first.addr as usize != BOOT_AT
        || BOOT_AT + first.len as usize > flash.len()
    {
        return None;
    }
    let stored = &flash[BOOT_AT..BOOT_AT + first.len as usize];
    let mut boot = decrypt(&stored[..16], 0xFFFF);
    boot.extend(decrypt(&stored[16..], 0xFFFF));
    (crc16(&boot) == first.data_crc).then_some(boot)
}

fn bootloader_identity(boot: &[u8]) -> Option<(String, String)> {
    if boot.len() < 16 {
        return None;
    }
    let bank = u16le(boot, 0);
    let size = u16le(boot, 2) as usize;
    let offset = u32le(boot, 8) as usize;
    if bank != 1 || crc16(&boot[..14]) != u16le(boot, 14) || offset + size > boot.len() {
        return None;
    }
    let payload = &boot[offset..offset + size];
    if crc16(payload) != u16le(boot, 12)
        || payload.len() < 0x70
        || &payload[0x40..0x48] != b"AC79BLID"
    {
        return None;
    }
    Some((cstr(&payload[0x48..0x50]), cstr(&payload[0x50..0x70])))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn identity_texts(bytes: &[u8]) -> Vec<(String, String)> {
    const MODELS: [&[u8]; 5] = [
        b"FM-1",
        b"MiniFM",
        b"SMK-37 Pro",
        b"SMK-37 Elite",
        b"SMK-37+VA",
    ];
    let mut found = vec![];
    for model in MODELS {
        let mut at = 0;
        while at + model.len() + 4 <= bytes.len() {
            if &bytes[at..at + model.len()] == model && bytes[at + model.len()] == b'_' {
                let digits = &bytes[at + model.len() + 1..at + model.len() + 4];
                if digits.iter().all(u8::is_ascii_digit) {
                    found.push((
                        String::from_utf8_lossy(model).into_owned(),
                        String::from_utf8_lossy(digits).into_owned(),
                    ));
                }
            }
            at += 1;
        }
    }
    found
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub model: String,
    pub model_known: bool,
    pub firmware: String,
    pub app: String,
    pub bootloader: String,
    pub board_key: Option<&'static str>,
}

pub fn analyze(flash: &[u8], flash_id: u32, chip_key: u16) -> Analysis {
    let table = app_table(flash, chip_key);
    let boot = stored_bootloader(flash);
    let identity = boot.as_deref().and_then(bootloader_identity);
    let board_key = match identity.as_ref().map(|(tag, _)| tag.as_str()) {
        Some("fm1") => Some("fm1"),
        Some("smk37") => Some("smk37"),
        _ if matches!(flash_id, 0x856014 | 0xB36014) => Some("fm1"),
        _ if flash_id == 0xEB6014 => Some("smk37"),
        _ => None,
    };
    let model = match board_key {
        Some("fm1") => "FM-1",
        Some("smk37") => "SMK-37 Pro / Elite",
        _ => "unknown",
    }
    .to_string();
    let bootloader = match identity {
        Some((tag, version)) => format!("open bootloader {version} ({tag})"),
        None if boot.is_some() => "stock".to_string(),
        None => "unreadable".to_string(),
    };
    let app_entry = table.iter().find(|item| item.name == "app.bin");
    let app = match app_entry {
        None => "none installed".to_string(),
        Some(item) => {
            let text = app_plain(flash, item.addr as usize, item.len as usize, chip_key);
            if crc16(&text) == item.data_crc {
                "OK".to_string()
            } else {
                "checksum failed".to_string()
            }
        }
    };
    let firmware = app_entry.map_or_else(
        || "none".to_string(),
        |item| {
            let text = app_plain(flash, item.addr as usize, item.len as usize, chip_key);
            let baud = table
                .iter()
                .any(|entry| entry.name == "VM" && entry.addr == 0xD9000);
            if let Some(version) = after_version(&text, b"SLOOP ") {
                format!("SLOOP {version}")
            } else if contains(&text, b"Felucca") {
                "Felucca".to_string()
            } else {
                identity_texts(&text).last().map_or_else(
                    || "unknown".to_string(),
                    |(name, version)| {
                        if baud || name == "SMK-37+VA" {
                            format!("{name} {version} (Baud Girl)")
                        } else {
                            format!("{name} {version}")
                        }
                    },
                )
            }
        },
    );
    Analysis {
        model,
        model_known: board_key.is_some(),
        firmware,
        app,
        bootloader,
        board_key,
    }
}

fn after_version(bytes: &[u8], prefix: &[u8]) -> Option<String> {
    let mut at = 0;
    while at + prefix.len() < bytes.len() {
        if &bytes[at..at + prefix.len()] == prefix {
            let start = at + prefix.len();
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                end += 1;
            }
            if end > start && bytes[start].is_ascii_digit() {
                return Some(
                    String::from_utf8_lossy(&bytes[start..end])
                        .trim_end_matches('.')
                        .to_string(),
                );
            }
        }
        at += 1;
    }
    None
}

/// Build a boot area that holds `bootloader` and the chip-key record the
/// firmware installers need. The bootloader is an m-bootloader BANKCB file.
pub fn build_boot_area(flash: &[u8], bootloader: &[u8]) -> Result<Vec<u8>, String> {
    if flash.len() < BOOT_AREA {
        return Err("the boot area is incomplete".into());
    }
    let Some((tag, _)) = bootloader_identity(bootloader) else {
        return Err("not a valid open bootloader".into());
    };
    if bootloader.len() > CONFIG_AT - BOOT_AT {
        return Err("the bootloader is too large for the boot area".into());
    }
    let table = boot_table(flash);
    if table.len() < 3 || table[0].name != "uboot.boot" || table[0].addr as usize != BOOT_AT {
        return Err("the boot area has no usable file table".into());
    }
    let (config_at, config_len) = (table[1].addr as usize, table[1].len as usize);
    if table[1].name != "isd_config.ini"
        || config_at < 0x38D0
        || config_at > BOOT_AREA
        || config_len < CONFIG_KEY
        || config_len > BOOT_AREA - config_at
    {
        return Err("the boot configuration is not safely contained".into());
    }
    if table[2..]
        .iter()
        .any(|entry| (entry.addr as usize) < BOOT_AREA)
    {
        return Err("other boot-area data would be overwritten".into());
    }
    let key = &flash[config_at..config_at + CONFIG_KEY];
    if crc16(&key[..32]) != u16le(key, 32) {
        return Err("the unit's chip-key record is damaged".into());
    }
    let mut area = vec![0xFF; BOOT_AREA];
    area[..BOOT_AT].copy_from_slice(&flash[..BOOT_AT]);
    let mut entry0 = table[0].raw;
    put16(&mut entry0, 2, crc16(bootloader));
    put32(&mut entry0, 8, bootloader.len() as u32);
    let head_crc = crc16(&entry0[2..]);
    put16(&mut entry0, 0, head_crc);
    area[0x20..0x40].copy_from_slice(&encrypt(&entry0, 0xFFFF));

    let mut config = key.to_vec();
    config.extend_from_slice(&CONFIG_SETTINGS);
    let mut entry1 = table[1].raw;
    put16(&mut entry1, 2, crc16(&config));
    put32(&mut entry1, 4, CONFIG_AT as u32);
    put32(&mut entry1, 8, CONFIG_SIZE as u32);
    let head_crc = crc16(&entry1[2..]);
    put16(&mut entry1, 0, head_crc);
    area[0x40..0x60].copy_from_slice(&encrypt(&entry1, 0xFFFF));

    area[BOOT_AT..BOOT_AT + 16].copy_from_slice(&encrypt(&bootloader[..16], 0xFFFF));
    area[BOOT_AT + 16..BOOT_AT + bootloader.len()]
        .copy_from_slice(&encrypt(&bootloader[16..], 0xFFFF));
    area[CONFIG_AT..].copy_from_slice(&config);
    let _ = tag;
    Ok(area)
}
