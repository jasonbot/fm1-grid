//! shrub.audio: one hosted firmware for the FM-1. The page's build.json names the
//! firmware version; the page metadata carries its description. The package is a
//! normal .fwsc at a fixed URL and downloads lazily like a hub offer.

use super::{between, text_of};
use crate::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
use serde::Deserialize;

/// The project page.
pub(crate) const SITE: &str = "https://shrub.audio/";
/// The build record next to the installer.
pub(crate) const BUILD: &str = "https://shrub.audio/editor/build.json";
/// The single hosted package.
pub(crate) const PACKAGE: &str = "https://shrub.audio/editor/firmware/SHRUB.fwsc";

#[derive(Deserialize)]
struct Build {
    firmware: String,
}

pub(crate) fn fetch_listing(site_url: &str, build_url: &str) -> Result<Vec<Firmware>, String> {
    let page = crate::download_heuristics::fetch_text(site_url)?;
    let build = crate::download_heuristics::fetch_text(build_url)?;
    parse_listing(&page, &build)
}

fn parse_listing(page: &str, build: &str) -> Result<Vec<Firmware>, String> {
    let build: Build = serde_json::from_str(build).map_err(|e| format!("shrub build: {e}"))?;
    let description = between(
        page,
        "<meta name=\"description\" content=\"",
        "\"",
    )
    .map(text_of)
    .filter(|text| !text.is_empty())
    .unwrap_or_else(|| "A stripped-down Serum for the M-VAVE FM-1.".to_string());
    Ok(vec![Firmware {
        name: "SHRUB".to_string(),
        byline: "Xfer Records".to_string(),
        version: build.firmware,
        description,
        tags: vec!["synth".to_string(), "source:shrub".to_string()],
        device: FirmwareDevice::Fm1,
        website: Some(SITE.to_string()),
        source_url: None,
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Hub {
            path: PACKAGE.to_string(),
            blob: None,
        },
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrub_listing_parses() {
        let page = r#"<meta name="description" content="SHRUB is a stripped-down Serum for the M-VAVE FM-1." />
"#;
        let build = r#"{"date":"2026-11-13","commit":"36822d2","dirty":false,"firmware":"1.00"}"#;
        let listing = parse_listing(page, build).unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "SHRUB");
        assert_eq!(listing[0].version, "1.00");
        assert_eq!(listing[0].byline, "Xfer Records");
        assert_eq!(
            listing[0].description,
            "SHRUB is a stripped-down Serum for the M-VAVE FM-1."
        );
        assert_eq!(listing[0].tags, ["synth", "source:shrub"]);
        assert_eq!(listing[0].device, FirmwareDevice::Fm1);
        match &listing[0].source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, PACKAGE);
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
    }
}
