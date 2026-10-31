//! The Firmware Hub's JSON catalog: the listing's source of truth. Every other
//! source only folds into what it offers.

use serde::Deserialize;

use super::parse_device;
use crate::library::{Firmware, FirmwareSource, Status};

/// The hub's host; package paths in the catalog are relative to it.
pub(crate) const BASE: &str = "https://thefirmwarehub.com";
/// The catalog itself.
pub(crate) const CATALOG: &str = "https://thefirmwarehub.com/catalog.json";

/// One catalog entry, with the hub's real field names; unknown fields are ignored,
/// missing ones default.
#[derive(Deserialize)]
struct CatalogEntry {
    name: String,
    #[serde(default, rename = "author")]
    byline: String,
    #[serde(default)]
    version: String,
    #[serde(default, rename = "what")]
    description: String,
    /// The hub's kind is one string or a list, depending on the entry.
    #[serde(default)]
    kind: Kind,
    #[serde(default)]
    vintage: bool,
    #[serde(default, rename = "product")]
    device: String,
    /// The package path on the hub ("fw/<file>", M-VAVE's under "fw/mvave/").
    pkg: String,
    #[serde(default)]
    #[allow(dead_code)]
    sha256: Option<String>,
    /// The project's web page ("site") and source tree link; missing on some entries.
    #[serde(default, rename = "site")]
    website: Option<String>,
    #[serde(default, rename = "source")]
    source_url: Option<String>,
}

/// The hub's `kind` field: one string for most entries, a list for some.
#[derive(Deserialize)]
#[serde(untagged)]
enum Kind {
    One(String),
    Many(Vec<String>),
}

impl Default for Kind {
    fn default() -> Self {
        Kind::One(String::new())
    }
}

/// Parse the hub catalog into hub-source entries. Accepts either a bare array or an
/// object with a `firmwares` array. Separated from the fetch so tests stay offline.
pub(crate) fn parse_catalog(json: &str) -> Result<Vec<Firmware>, String> {
    #[derive(Deserialize)]
    struct Catalog {
        firmwares: Vec<CatalogEntry>,
    }
    let entries: Vec<CatalogEntry> = serde_json::from_str(json)
        .or_else(|_| serde_json::from_str::<Catalog>(json).map(|c| c.firmwares))
        .map_err(|e| format!("parse catalog: {e}"))?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let mut tags = match entry.kind {
                Kind::One(kind) if !kind.is_empty() => vec![kind],
                Kind::One(_) => Vec::new(),
                Kind::Many(kinds) => kinds,
            };
            if entry.vintage {
                tags.push("vintage".to_string());
            }
            tags.push("source:hub".to_string());
            Firmware {
                name: entry.name,
                byline: entry.byline,
                version: entry.version,
                description: entry.description,
                tags,
                device: parse_device(&entry.device),
                website: entry.website,
                source_url: entry.source_url,
                upgradable: false,
                status: Status::Available,
                source: FirmwareSource::Hub {
                    path: entry.pkg,
                    blob: None,
                },
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::FirmwareDevice;

    #[test]
    fn catalog_parses() {
        // The real catalog's field names (author/what/kind/vintage/product/pkg, plus
        // the site/source links).
        let json = r#"{"firmwares": [
            {"name": "fm1-open", "version": "2.1", "pkg": "fw/mvave/fm1-open.fwsc",
             "sha256": "abc123", "author": "M-VAVE", "product": "FM-1_975",
             "what": "dev kit firmware", "kind": "official",
             "site": "https://open.m-vave.com", "source": "https://github.com/m-vave/fm1_open"},
            {"name": "smk-os", "version": "0.9", "pkg": "fw/smk-os.fwsc",
             "product": "SMK-37 Pro", "kind": "synth", "vintage": true}
        ]}"#;
        let listing = parse_catalog(json).unwrap();
        assert_eq!(listing.len(), 2);
        assert_eq!(listing[0].name, "fm1-open");
        assert_eq!(listing[0].device, FirmwareDevice::Fm1);
        assert_eq!(listing[0].byline, "M-VAVE");
        assert_eq!(listing[0].description, "dev kit firmware");
        assert_eq!(listing[0].tags, vec!["official", "source:hub"]);
        assert_eq!(
            listing[0].website.as_deref(),
            Some("https://open.m-vave.com")
        );
        assert_eq!(
            listing[0].source_url.as_deref(),
            Some("https://github.com/m-vave/fm1_open")
        );
        match &listing[0].source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, "fw/mvave/fm1-open.fwsc");
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
        // Links are optional: absent ones stay None.
        assert_eq!(listing[1].website, None);
        assert_eq!(listing[1].source_url, None);
        assert_eq!(listing[1].device, FirmwareDevice::Smk37Pro);
        assert_eq!(listing[1].tags, vec!["synth", "vintage", "source:hub"]);
        // A bare array works too.
        let listing = parse_catalog(r#"[{"name": "x", "pkg": "fw/x.fwsc"}]"#).unwrap();
        assert_eq!(listing.len(), 1);
        assert!(parse_catalog("not json").is_err());
    }
}
