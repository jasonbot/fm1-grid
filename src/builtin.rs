//! Firmware images embedded in the binary.
//!
//! These are unmodified `.fwsc` update packages mirrored by
//! [The FM-1 Firmware Hub](https://thefirmwarehub.com/), plus M-VAVE's official package that
//! ships with the FM1 Open development kit. The SHA-256 hashes match the hub's catalogue
//! (checked 2026-10-22); see `firmware/README.md` for sources and licences.

pub struct BuiltinFirmware {
    pub name: &'static str,
    pub version: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub bytes: &'static [u8],
}

impl BuiltinFirmware {
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

pub const BUILTINS: &[BuiltinFirmware] = &[
    BuiltinFirmware {
        name: "SLOOP",
        version: "2.8",
        file: "sloop-2.8.fwsc",
        sha256: "fb69b4311790eb7ec09498be16ae2b71bd681840e221c0d8cfdb26fb5f9a2928",
        bytes: include_bytes!("../firmware/sloop-2.8.fwsc"),
    },
    BuiltinFirmware {
        name: "Felucca",
        version: "1.6.1",
        file: "felucca-1.6.1.fwsc",
        sha256: "dff3a38f7e5b9833e15aba322558c9485e9e422e3ff813c616c91b3584a2d909",
        bytes: include_bytes!("../firmware/felucca-1.6.1.fwsc"),
    },
    BuiltinFirmware {
        name: "FM1 Open",
        version: "V15",
        file: "FM-1_015.fwsc",
        sha256: "db1642b2b6fa5c2cccb11ffd13878068bb28601678d3644049f99dc40e7edb8a",
        bytes: include_bytes!("../firmware/FM-1_015.fwsc"),
    },
    BuiltinFirmware {
        name: "ChoralRoot",
        version: "0.26.1",
        file: "choralroot-0.26.1.fwsc",
        sha256: "7fec6a729c2d05d1f19e8b6ab3e2101affb8d9b11e4835cb412b11625289ace2",
        bytes: include_bytes!("../firmware/choralroot-0.26.1.fwsc"),
    },
];
