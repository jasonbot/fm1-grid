//! jakehandy.com/h1: one hosted FM-1 firmware. The guide names the current
//! release; the package is the GitHub release asset for that version.

use super::{between, text_of};
use crate::library::{Firmware, FirmwareDevice, FirmwareSource, Status};

pub(crate) const SITE: &str = "https://jakehandy.com/h1";
const REPO: &str = "https://github.com/jakehandy/h1";

pub(crate) fn fetch_listing(site_url: &str) -> Result<Vec<Firmware>, String> {
    let page = crate::download_heuristics::fetch_text(site_url)?;
    parse_listing(&page)
}

fn parse_listing(page: &str) -> Result<Vec<Firmware>, String> {
    let version = between(
        page,
        "Guide matches h1's current build, <a href=\"https://github.com/jakehandy/h1/releases/tag/v",
        "\"",
    )
    .map(|text| text.split_whitespace().next().unwrap_or(text))
    .filter(|version| !version.is_empty())
    .ok_or_else(|| "h1 page has no current release".to_string())?;
    let description = between(page, "<meta name=\"description\" content=\"", "\"")
        .map(text_of)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "h1 for the M-VAVE FM-1.".to_string());
    Ok(vec![Firmware {
        name: "h1".to_string(),
        byline: "Jake Handy".to_string(),
        version: version.to_string(),
        description,
        tags: vec!["synth".to_string(), "source:jakehandy".to_string()],
        device: FirmwareDevice::Fm1,
        website: Some(SITE.to_string()),
        source_url: Some(REPO.to_string()),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Hub {
            path: format!("{REPO}/releases/download/v{version}/h1-{version}.fwsc"),
            blob: None,
        },
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h1_listing_parses() {
        let page = r#"<meta name="description" content="h1 is free M-VAVE FM-1 firmware." />
Guide matches h1's current build, <a href="https://github.com/jakehandy/h1/releases/tag/v0.2.1">v0.2.1 (5b697e5)</a>, 18 November 2026."#;
        let listing = parse_listing(page).unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "h1");
        assert_eq!(listing[0].version, "0.2.1");
        assert_eq!(listing[0].byline, "Jake Handy");
        assert_eq!(listing[0].tags, ["synth", "source:jakehandy"]);
        assert_eq!(listing[0].device, FirmwareDevice::Fm1);
        match &listing[0].source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(
                    path,
                    "https://github.com/jakehandy/h1/releases/download/v0.2.1/h1-0.2.1.fwsc"
                );
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
    }
}
