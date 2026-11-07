//! The `.fwsc` package layout: the identity markers interleaved into the first
//! blocks, and the image left after removing them.

/// At least twenty 48-byte blocks; byte 47 of each is a marker. Stock SMK-37 v016
/// packages interleave 36.
const PKG_BLOCKS: usize = 20;
const PKG_BLOCK: usize = 0x30;
const PKG_KEEP: usize = 0x2F;

/// The number of interleaved blocks. FM-1 and most custom builds use 20; M-VAVE's
/// stock SMK-37 v016 uses 36 (14 identity characters plus 22 `0x7D` markers). The
/// first byte after the interleave is not a marker, and every stock FM-1 package
/// carries `0xFF` there, so counting while the marker is `0x7D` finds the real end.
fn pkg_blocks(fwsc: &[u8]) -> usize {
    let mut blocks = PKG_BLOCKS;
    while blocks * PKG_BLOCK + PKG_KEEP < fwsc.len()
        && fwsc[blocks * PKG_BLOCK + PKG_KEEP] == 0x7D
    {
        blocks += 1;
    }
    blocks
}

/// The package identity ("FM-1_916"): the marker byte after each interleaved block.
/// None when the file is not an FM-1/SMK package.
pub fn product_of(fwsc: &[u8]) -> Option<String> {
    if fwsc.len() < PKG_BLOCKS * PKG_BLOCK {
        return None;
    }
    let mut s = String::new();
    for i in 0..pkg_blocks(fwsc) {
        let m = fwsc[i * PKG_BLOCK + PKG_KEEP];
        if m != 0x7D {
            s.push((m.wrapping_sub(i as u8).wrapping_sub(1)) as char);
        }
    }
    (!s.is_empty()).then_some(s)
}

/// The image the device pulls during an update: the package without its marker bytes.
pub fn logical_image(fwsc: &[u8]) -> Vec<u8> {
    let blocks = pkg_blocks(fwsc);
    let mut out = Vec::with_capacity(fwsc.len().saturating_sub(blocks));
    for i in 0..blocks {
        out.extend_from_slice(&fwsc[i * PKG_BLOCK..i * PKG_BLOCK + PKG_KEEP]);
    }
    out.extend_from_slice(&fwsc[blocks * PKG_BLOCK..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(identity: &str, blocks: usize) -> Vec<u8> {
        let mut fwsc = vec![0xAA; (blocks + 1) * PKG_BLOCK + 64];
        for i in 0..blocks {
            let marker = i.checked_sub(identity.len()).map_or_else(
                || identity.as_bytes()[i].wrapping_add(i as u8).wrapping_add(1),
                |_| 0x7D,
            );
            fwsc[i * PKG_BLOCK + PKG_KEEP] = marker;
        }
        fwsc[blocks * PKG_BLOCK + PKG_KEEP] = 0xFF;
        fwsc
    }

    #[test]
    fn reads_twenty_and_thirty_six_block_files() {
        let fm1 = package("FM-1_916", 20);
        assert_eq!(pkg_blocks(&fm1), 20);
        assert_eq!(product_of(&fm1).as_deref(), Some("FM-1_916"));
        assert_eq!(logical_image(&fm1).len(), fm1.len() - 20);

        let smk = package("SMK-37 Pro_016", 36);
        assert_eq!(pkg_blocks(&smk), 36);
        assert_eq!(product_of(&smk).as_deref(), Some("SMK-37 Pro_016"));
        assert_eq!(logical_image(&smk).len(), smk.len() - 36);
    }
}
