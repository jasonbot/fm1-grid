//! The hosted remote feed: the online listing (the Hub, every additive feed and
//! the binary's index-only metadata) with each entry resolved to the direct `.fwsc`
//! package URL a client without the download heuristics can fetch.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use super::*;

/// How many entries resolve at once. Resolution is network-bound (a release or
/// install page costs a request or two), so a handful of threads keeps the walk
/// short without hammering any one host.
const RESOLVE_WORKERS: usize = 8;

/// One firmware in the hosted feed. The fields a minimal client needs to list and
/// download it; `url` is always a direct `.fwsc` package.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RemoteFirmware {
    pub name: String,
    pub version: String,
    pub byline: String,
    pub description: String,
    pub device: FirmwareDevice,
    /// The listing's tags without the `source:` origin bookkeeping; the display
    /// tags (kind, chip support, SMK-37 model) stay.
    pub tags: Vec<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

/// Fetch every online feed, merge them like the library does, and resolve each
/// entry's listing link to a direct `.fwsc` URL. Entries whose package can't be
/// found are dropped (and named on stderr), as are feed failures other than the
/// hub's: a partial feed is only worth publishing with a warning.
pub fn fetch_remotes() -> Result<Vec<RemoteFirmware>, String> {
    fetch_remotes_from(
        &FeedUrls::today(),
        HubFeeds {
            hub: true,
            editor: true,
            mutant: true,
            reverbose: true,
            shrub: true,
            jakehandy: true,
        },
    )
}

/// [`fetch_remotes`] with injectable URLs and feeds, for tests.
fn fetch_remotes_from(urls: &FeedUrls, feeds: HubFeeds) -> Result<Vec<RemoteFirmware>, String> {
    let (listing, outcome) = fetch_sources(urls, feeds)?;
    if let HubStatus::Failed(e) = outcome {
        eprintln!("fm1-remotes: warning: {e}");
    }
    let listing = merge_listings(listing, index_listing());
    Ok(resolve_listing(
        listing,
        &crate::download_heuristics::resolve_package_url,
    ))
}

/// Resolve every entry's link and keep the ones that end at a `.fwsc` package. The
/// output keeps listing order; skipped entries name their reason on stderr.
fn resolve_listing(
    listing: Vec<Firmware>,
    resolve: &(impl Fn(&str) -> Result<String, String> + Sync),
) -> Vec<RemoteFirmware> {
    let next = AtomicUsize::new(0);
    let resolved: Mutex<Vec<Option<RemoteFirmware>>> =
        Mutex::new(listing.iter().map(|_| None).collect());
    let workers = RESOLVE_WORKERS.min(listing.len()).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(firmware) = listing.get(index) else {
                    break;
                };
                match remote_entry(firmware, resolve) {
                    Ok(remote) => resolved.lock().unwrap()[index] = Some(remote),
                    Err(e) => eprintln!("fm1-remotes: skipped {}: {e}", firmware.name),
                }
            });
        }
    });
    resolved
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .collect()
}

/// One listing entry as a hosted row, with its link resolved. Only online entries
/// (`Hub` sources: hub-relative paths, absolute package URLs and listing pages)
/// qualify; a direct link that doesn't end at `.fwsc` is not downloadable by the
/// feed's client.
fn remote_entry(
    firmware: &Firmware,
    resolve: &(impl Fn(&str) -> Result<String, String> + Sync),
) -> Result<RemoteFirmware, String> {
    let FirmwareSource::Hub { path, .. } = &firmware.source else {
        return Err("not an online listing entry".to_string());
    };
    let url = resolve(&hub_download_url(path))?;
    if !is_fwsc(&url) {
        return Err(format!("{url} is not a .fwsc package"));
    }
    Ok(RemoteFirmware {
        name: firmware.name.clone(),
        version: firmware.version.clone(),
        byline: firmware.byline.clone(),
        description: firmware.description.clone(),
        device: firmware.device,
        tags: firmware
            .tags
            .iter()
            .filter(|tag| !tag.starts_with("source:"))
            .cloned()
            .collect(),
        url,
        website: firmware.website.clone(),
        source_url: firmware.source_url.clone(),
    })
}

/// Whether a URL names a `.fwsc` file, ignoring its query and fragment.
fn is_fwsc(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase()
        .ends_with(".fwsc")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing_entry(name: &str, path: &str) -> Firmware {
        Firmware {
            name: name.to_string(),
            byline: "an author".to_string(),
            version: "1.2".to_string(),
            description: "a firmware".to_string(),
            tags: vec![
                "groove".to_string(),
                "new chip".to_string(),
                "source:hub".to_string(),
            ],
            device: FirmwareDevice::Fm1,
            website: Some("https://project.test".to_string()),
            source_url: Some("https://github.com/o/r".to_string()),
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: path.to_string(),
                blob: None,
            },
        }
    }

    #[test]
    fn rows_carry_the_listing_metadata_and_strip_origin_tags() {
        let entry = listing_entry("SLOOP", "fw/sloop-2.8.5.fwsc");
        let remote = remote_entry(&entry, &|url| Ok(url.to_string())).unwrap();
        assert_eq!(remote.name, "SLOOP");
        assert_eq!(remote.version, "1.2");
        assert_eq!(remote.byline, "an author");
        assert_eq!(remote.description, "a firmware");
        assert_eq!(remote.device, FirmwareDevice::Fm1);
        assert_eq!(remote.tags, vec!["groove", "new chip"]);
        assert_eq!(remote.url, "https://thefirmwarehub.com/fw/sloop-2.8.5.fwsc");
        assert_eq!(remote.website.as_deref(), Some("https://project.test"));
        assert_eq!(remote.source_url.as_deref(), Some("https://github.com/o/r"));
    }

    #[test]
    fn only_direct_fwsc_links_survive() {
        let entry = listing_entry("X", "https://example.test/release");
        // A page resolves to a package: kept, with the resolved URL.
        let remote = remote_entry(&entry, &|_| Ok("https://cdn.test/x.fwsc".to_string())).unwrap();
        assert_eq!(remote.url, "https://cdn.test/x.fwsc");
        // A zip is a package the app unwraps but not a .fwsc the feed can hand out.
        assert!(remote_entry(&entry, &|_| Ok("https://cdn.test/x.zip".to_string())).is_err());
        // A page that names no package is skipped.
        assert!(remote_entry(&entry, &|_| Err("names no package".to_string())).is_err());
        // Local entries never appear in the hosted feed.
        let file = Firmware {
            source: FirmwareSource::File {
                path: "/tmp/x.fwsc".to_string(),
            },
            ..listing_entry("X", "fw/x.fwsc")
        };
        assert!(remote_entry(&file, &|url| Ok(url.to_string())).is_err());

        assert!(is_fwsc("https://x.test/a.fwsc"));
        assert!(is_fwsc("https://x.test/a.FWSC?raw=1"));
        assert!(!is_fwsc("https://x.test/a.fwsc.zip"));
        assert!(!is_fwsc("https://x.test/a.zip#x.fwsc"));
    }

    #[test]
    fn hub_failure_is_fatal() {
        // A closed port fails fast and deterministically; without the hub's
        // listing there is no feed to publish.
        let closed = || "http://127.0.0.1:1/x".to_string();
        let urls = FeedUrls {
            hub: closed(),
            editor: closed(),
            mutant: closed(),
            reverbose: closed(),
            reverbose_smk: closed(),
            shrub: closed(),
            shrub_build: closed(),
            jakehandy: closed(),
        };
        let feeds = HubFeeds {
            hub: true,
            editor: false,
            mutant: false,
            reverbose: false,
            shrub: false,
            jakehandy: false,
        };
        assert!(fetch_remotes_from(&urls, feeds).is_err());
    }

    #[test]
    fn resolution_keeps_listing_order_and_drops_failures() {
        let listing = vec![
            listing_entry("A", "fw/a.fwsc"),
            listing_entry("B", "fw/b.fwsc"),
            listing_entry("C", "fw/c.fwsc"),
        ];
        let resolved = resolve_listing(listing, &|url: &str| {
            if url.ends_with("b.fwsc") {
                Err("gone".to_string())
            } else {
                Ok(url.to_string())
            }
        });
        let names: Vec<&str> = resolved.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["A", "C"]);
    }

    /// fm1-card's catalog parser reads `url` as the package path and
    /// `byline`/`description` as author/what. A rename would silently leave every
    /// synced card without a download URL, so pin the wire shape.
    #[test]
    fn hosted_feed_matches_the_card_manifest_keys() {
        let feed = vec![RemoteFirmware {
            name: "Sloop".into(),
            version: "2.8.5".into(),
            byline: "isod89".into(),
            description: "A groovebox".into(),
            device: FirmwareDevice::Fm1,
            tags: vec!["synth".into(), "new chip".into()],
            url: "https://cdn.test/sloop-2.8.5.fwsc".into(),
            website: None,
            source_url: None,
        }];
        let value = serde_json::to_value(&feed).unwrap();
        let entry = &value[0];
        for key in ["name", "version", "byline", "description", "device", "tags", "url"] {
            assert!(entry.get(key).is_some(), "{key} is part of the card manifest");
        }
        assert_eq!(entry["url"], "https://cdn.test/sloop-2.8.5.fwsc");
        assert_eq!(entry["device"], "fm1");
        assert_eq!(entry["tags"][1], "new chip");
    }
}
