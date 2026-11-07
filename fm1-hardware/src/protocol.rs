pub(crate) const HS_QUERY: [u8; 10] = [0xF0, 0x00, 0x32, 0x45, 0x00, 0x00, 0x00, 0x40, 0x7F, 0xF7];
/// The upgrade command: the device starts pulling the image (fm1ota.js UPGRADE).
pub(crate) const UPGRADE: [u8; 6] = [0xF0, 0x22, 0x24, 0x35, 0x7F, 0xF7];
/// The largest data packet a device may ask for.
pub(crate) const MAXDATA: usize = 512;
/// The "I am done" addresses: step 1 (staging the loader) and step 2 (writing).
pub(crate) const FINISH_STAGE: u32 = 0xE000_0000;
pub(crate) const FINISH_WRITE: u32 = 0xF000_0000;

pub(crate) fn pack7(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 8 / 7 + 1);
    let (mut acc, mut nb) = (0u32, 0u32);
    for &b in data {
        acc |= (b as u32) << nb;
        nb += 8;
        while nb >= 7 {
            out.push((acc & 0x7F) as u8);
            acc >>= 7;
            nb -= 7;
        }
    }
    if nb > 0 {
        out.push((acc & 0x7F) as u8);
    }
    out
}

pub(crate) fn unpack7(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 7 / 8 + 1);
    let (mut acc, mut nb) = (0u32, 0u32);
    for &b in data {
        acc |= ((b & 0x7F) as u32) << nb;
        nb += 7;
        while nb >= 8 {
            out.push((acc & 0xFF) as u8);
            acc >>= 8;
            nb -= 8;
        }
    }
    out
}

/// A data packet answering a read request: header `00 59 30`, the total length, the
/// request's flag, the address, the data length, the data and a checksum, 7-bit packed
/// into a SysEx.
pub(crate) fn response(addr: u32, data: &[u8], fl: u8) -> Vec<u8> {
    let len = data.len();
    let mut body = Vec::with_capacity(15 + len);
    body.extend_from_slice(&[
        0x00,
        0x59,
        0x30,
        ((len + 8) & 0xFF) as u8,
        (((len + 8) >> 8) & 0xFF) as u8,
        0,
        fl,
        (addr & 0xFF) as u8,
        ((addr >> 8) & 0xFF) as u8,
        ((addr >> 16) & 0xFF) as u8,
        ((addr >> 24) & 0xFF) as u8,
        (len & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
        0,
    ]);
    body.extend_from_slice(data);
    let sum: u32 = body[6..].iter().map(|&b| b as u32).sum();
    body.push((!sum & 0xFF) as u8);
    let mut sysex = Vec::with_capacity(body.len() + 2);
    sysex.push(0xF0);
    sysex.extend_from_slice(&pack7(&body));
    sysex.push(0xF7);
    sysex
}

/// The identity an answered handshake reports, from its SysEx packet.
pub(crate) fn parse_identity(pkt: &[u8]) -> Option<String> {
    if pkt.first() != Some(&0xF0) || pkt.last() != Some(&0xF7) {
        return None;
    }
    let d = unpack7(&pkt[1..pkt.len() - 1]);
    if d.len() != 34 || d[0] != 0 || d[1] != 0x59 || d[2] != 0x11 {
        return None;
    }
    let text: String = d[6..33]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    // "MODEL_VERSION", like "FM-1_916" or "ota-FM-1_904".
    let valid = text
        .split_once('_')
        .is_some_and(|(model, version)| !model.is_empty() && !version.is_empty());
    valid.then_some(text)
}

/// One read request from the device during an update.
pub(crate) struct Request {
    pub(crate) fl: u8,
    pub(crate) addr: u32,
    pub(crate) len: u32,
}

pub(crate) fn parse_request(pkt: &[u8]) -> Option<Request> {
    if pkt.first() != Some(&0xF0) || pkt.last() != Some(&0xF7) {
        return None;
    }
    let u = unpack7(&pkt[1..pkt.len() - 1]);
    if u.len() != 15 || u[0] != 0 || u[1] != 0x59 || u[2] != 0x30 {
        return None;
    }
    let sum: u32 = u[6..14].iter().map(|&b| b as u32).sum();
    if (!sum & 0xFF) as u8 != u[14] {
        return None;
    }
    Some(Request {
        fl: u[6],
        addr: u[7] as u32 | (u[8] as u32) << 8 | (u[9] as u32) << 16 | (u[10] as u32) << 24,
        len: u[11] as u32 | (u[12] as u32) << 8 | (u[13] as u32) << 16,
    })
}
