//! Whether a firmware image advertises USB-MIDI.
//!
//! The FM-1's TRS jack is MIDI-in only, and fm1-soc models it that way (UART1 RX;
//! there is no TX pin). On real hardware the firmwares talk MIDI both ways over USB-C
//! instead: all the hub firmwares carry a USB MIDIStreaming interface descriptor (one
//! bidirectional endpoint pair), while M-VAVE's stock package carries none. The
//! emulated SoC stubs the USB controller at register level — nothing enumerates — so in
//! fm1-grid that advertisement never reaches a host. This module reads it out of the
//! image so the MIDI graph can present a pane's real capability instead of pretending
//! the pane is MIDI-in by nature.

/// True when the image holds a USB interface descriptor with class Audio (0x01) and
/// subclass MIDIStreaming (0x03): `bLength=9, bDescriptorType=INTERFACE, ...,
/// bInterfaceClass=1, bInterfaceSubClass=3`. The image is an update payload, not a
/// descriptor table, so this is a byte-level search — cheap enough at install time and
/// exact enough (the 9-byte pattern has no false positives in practice).
#[cfg(any(feature = "gui", test))]
pub(crate) fn advertised(image: &[u8]) -> bool {
    let mut i = 0;
    while i + 9 <= image.len() {
        if image[i] == 9
            && image[i + 1] == 4
            && image[i + 5] == 0x01
            && image[i + 6] == 0x03
        {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_midistreaming_interface() {
        // A bare interface descriptor anywhere in the image is enough.
        let mut image = vec![0u8; 64];
        image[10..19].copy_from_slice(&[9, 4, 0, 0, 2, 0x01, 0x03, 0, 0]);
        assert!(advertised(&image));
        assert!(!advertised(&[]));
        // Class Audio but subclass AudioControl (0x01) is not MIDI.
        image[16] = 0x01;
        assert!(!advertised(&image));
    }

    #[test]
    fn bundled_firmwares() {
        // Known cases: hub firmwares advertise USB-MIDI in/out; M-VAVE's own packages
        // don't. The embedded list changes as packages come and go, so assert only
        // these (skipping any that isn't currently bundled).
        let cases = [
            ("sloop-2.8.1.fwsc", true),
            ("felucca-1.6.1.fwsc", true),
            ("FM-1_015.fwsc", false),
            ("FM-1+VA-102.fwsc", false),
            ("FM1-Open-V14.fwsc", false),
        ];
        for (file, expected) in cases {
            if let Some(firmware) = crate::builtin::BUILTINS.iter().find(|f| f.file == file) {
                assert_eq!(
                    advertised(firmware.bytes),
                    expected,
                    "{} changed its USB-MIDI advertisement",
                    firmware.file
                );
            }
        }
    }
}
