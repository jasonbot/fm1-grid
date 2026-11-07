//! mutant.cloud's catalog (mutant.cloud/catalog.json): the Firmware Hub's JSON
//! format, one row per installable package. Its separate `firmware.json` also lists
//! tools (editors, emulators, installers); the catalog carries firmwares only, so
//! nothing here needs a kind filter. Additive to the hub.

use crate::library::Firmware;
use crate::sources::hub;

/// The catalog itself.
pub(crate) const CATALOG: &str = "https://mutant.cloud/catalog.json";

/// Fetch the catalog and parse it with the hub's format, tagged `source:mutant`.
pub(crate) fn fetch_listing(url: &str) -> Result<Vec<Firmware>, String> {
    let body = crate::download_heuristics::fetch_text(url)?;
    hub::parse_catalog_with(&body, "source:mutant")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{FirmwareDevice, FirmwareSource};

    #[test]
    fn mutant_catalog_parses_as_firmware() {
        // The real catalog's shape: absolute package and link URLs, explicit
        // devices; `kind` may be one string or a list.
        let json = r#"{"hub": "https://mutant.cloud", "firmwares": [
            {"name": "NACHTFAHRT", "author": "FartyPants", "version": "0.9.9",
             "what": "Berlin School sequencer.", "kind": "groove", "product": "FM-1_900",
             "pkg": "https://github.com/o/r/releases/download/v0.9.9/nachtfahrt.fwsc",
             "site": "https://o.github.io/r/", "source": "https://github.com/o/r",
             "devices": ["fm1"]},
            {"name": "SMK Mod", "version": "022", "kind": ["other"],
             "product": "MKE-P37_900", "pkg": "https://example.test/smk.fwsc",
             "devices": ["smk37"]}
        ]}"#;
        let listing = hub::parse_catalog_with(json, "source:mutant").unwrap();
        assert_eq!(listing.len(), 2);
        let entry = &listing[0];
        assert_eq!(entry.tags, vec!["groove", "source:mutant"]);
        assert_eq!(entry.device, FirmwareDevice::Fm1);
        assert_eq!(entry.website.as_deref(), Some("https://o.github.io/r/"));
        match &entry.source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(
                    path,
                    "https://github.com/o/r/releases/download/v0.9.9/nachtfahrt.fwsc"
                );
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
        // The explicit device list beats the product string; the product names the
        // model, which becomes a tag and a name suffix so the model rows stay apart.
        assert_eq!(listing[1].device, FirmwareDevice::Smk37);
        assert_eq!(listing[1].name, "SMK Mod — MKE-P37");
        assert_eq!(listing[1].tags, vec!["other", "MKE-P37", "source:mutant"]);
    }
}
