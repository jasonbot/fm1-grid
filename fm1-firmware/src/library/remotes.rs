//! The hosted remote feed: the online listing (the Hub, every additive feed and
//! the binary's index-only metadata) with each entry resolved to the direct `.fwsc`
//! package URL a client without the download heuristics can fetch.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use fm1_base::store::Store;

use super::merge::name_key;
use super::*;

/// How many entries resolve at once. Resolution is network-bound (a release or
/// install page costs a request or two), so a handful of threads keeps the walk
/// short without hammering any one host.
const RESOLVE_WORKERS: usize = 8;

/// Failed resolutions retried per garden pass. Rate limiting is the usual cause, so
/// a full retry of every blank would recreate the load that hit the limit.
const PENDING_RETRIES_PER_PASS: usize = 8;

/// The store object holding the card-bound catalog and its unresolved blanks.
const REMOTE_CATALOG_OBJECT: &str = "remote-catalog";

/// The persisted card catalog: ready-to-send rows plus the listing entries whose
/// package link could not be resolved yet.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RemoteCatalog {
    pub entries: Vec<RemoteFirmware>,
    pub pending: Vec<PendingRemote>,
    /// Unix seconds of the last garden pass.
    pub updated: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingRemote {
    pub firmware: Firmware,
    /// Unix seconds of the last resolution attempt.
    pub tried: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GardenReport {
    pub entries: usize,
    pub pending: usize,
    pub attempted: usize,
}

/// One firmware in the hosted feed. The fields a minimal client needs to list and
/// download it; `url` is always a direct `.fwsc` package.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
    resolve_each(listing, resolve)
        .into_iter()
        .filter_map(|(firmware, outcome)| match outcome {
            Ok(remote) => Some(remote),
            Err(e) => {
                eprintln!("fm1-remotes: skipped {}: {e}", firmware.name);
                None
            }
        })
        .collect()
}

/// Resolve a batch concurrently, keeping each entry and its own outcome so the
/// caller can tell which links failed.
fn resolve_each(
    listing: Vec<Firmware>,
    resolve: &(impl Fn(&str) -> Result<String, String> + Sync),
) -> Vec<(Firmware, Result<RemoteFirmware, String>)> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<(Firmware, Result<RemoteFirmware, String>)>>> =
        Mutex::new(listing.iter().map(|_| None).collect());
    let workers = RESOLVE_WORKERS.min(listing.len()).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(firmware) = listing.get(index).cloned() else {
                    break;
                };
                let outcome = remote_entry(&firmware, resolve);
                results.lock().unwrap()[index] = Some((firmware, outcome));
            });
        }
    });
    results
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
    remote_row(firmware, resolve(&hub_download_url(path))?)
}

/// A hosted row with a known package URL; the display fields always come from the
/// current listing.
fn remote_row(firmware: &Firmware, url: String) -> Result<RemoteFirmware, String> {
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

/// The identity the catalog keeps across passes: a version bump or a different
/// device/variant is a new row, a respelling is not.
fn catalog_key(
    name: &str,
    version: &str,
    device: FirmwareDevice,
    variant: Option<&str>,
) -> String {
    format!(
        "{}|{version}|{device:?}|{}",
        name_key(name),
        variant.unwrap_or("")
    )
}

fn firmware_key(firmware: &Firmware) -> String {
    catalog_key(
        &firmware.name,
        &firmware.version,
        firmware.device,
        firmware.variant(),
    )
}

fn remote_key(row: &RemoteFirmware) -> String {
    let variant = row.tags.iter().find_map(|tag| match tag.as_str() {
        "SMK-37 Pro" | "SMK-37 Elite" | "MKE-P37" => Some(tag.as_str()),
        _ => None,
    });
    catalog_key(&row.name, &row.version, row.device, variant)
}

/// The persisted catalog, or an empty one before the first pass.
pub fn load_remote_catalog(store: &Store) -> RemoteCatalog {
    store.fetch_object(REMOTE_CATALOG_OBJECT).unwrap_or_default()
}

/// Bring the persisted card catalog up to date from the library's online listing.
/// Resolved URLs are reused; only new rows and a bounded batch of earlier failures
/// hit the network. `complete` is false when an additive feed failed, so rows the
/// listing does not carry this time are kept instead of dropped.
pub fn garden_remote_catalog(
    store: &Store,
    listing: Vec<Firmware>,
    complete: bool,
) -> Result<GardenReport, String> {
    garden_from(
        store,
        listing,
        complete,
        &crate::download_heuristics::resolve_package_url,
    )
}

fn garden_from(
    store: &Store,
    listing: Vec<Firmware>,
    complete: bool,
    resolve: &(impl Fn(&str) -> Result<String, String> + Sync),
) -> Result<GardenReport, String> {
    // The GUI and the TUI share the store; one writer at a time.
    let _lock = store.lock_file(REMOTE_CATALOG_OBJECT).ok().map(|lock| {
        let _ = lock.lock();
        lock
    });
    if listing.is_empty() {
        // No successful fetch yet: an empty listing must not erase the catalog.
        let cached = load_remote_catalog(store);
        return Ok(GardenReport {
            entries: cached.entries.len(),
            pending: cached.pending.len(),
            attempted: 0,
        });
    }
    let cached = load_remote_catalog(store);
    let now = now_seconds();
    let mut rows: HashMap<String, RemoteFirmware> = cached
        .entries
        .into_iter()
        .map(|row| (remote_key(&row), row))
        .collect();
    let mut pending: HashMap<String, PendingRemote> = cached
        .pending
        .into_iter()
        .map(|row| (firmware_key(&row.firmware), row))
        .collect();

    // Retry the least recently attempted first, a batch at a time: a rate-limit
    // streak must not turn every pass into the full walk that caused it.
    let mut retry_order: Vec<String> = pending.keys().cloned().collect();
    retry_order.sort_by_key(|key| pending.get(key).map_or(0, |row| row.tried));
    let retry: HashSet<String> = retry_order
        .into_iter()
        .take(PENDING_RETRIES_PER_PASS)
        .collect();

    let mut resolve_now: Vec<(String, Firmware)> = Vec::new();
    for firmware in &listing {
        if !matches!(firmware.source, FirmwareSource::Hub { .. }) {
            continue;
        }
        let key = firmware_key(firmware);
        if rows.contains_key(&key) {
            continue;
        }
        match pending.get(&key) {
            Some(_) if !retry.contains(&key) => {}
            _ => resolve_now.push((key, firmware.clone())),
        }
    }
    for (key, _) in &resolve_now {
        pending.remove(key);
    }

    let keys: Vec<String> = resolve_now.iter().map(|(key, _)| key.clone()).collect();
    let outcomes = resolve_each(
        resolve_now
            .into_iter()
            .map(|(_, firmware)| firmware)
            .collect(),
        resolve,
    );
    let attempted = keys.len();
    for (key, (firmware, outcome)) in keys.into_iter().zip(outcomes) {
        match outcome {
            Ok(row) => {
                rows.insert(key, row);
            }
            Err(_) => {
                pending.insert(
                    key,
                    PendingRemote {
                        firmware,
                        tried: now,
                    },
                );
            }
        }
    }

    let mut entries = Vec::new();
    let mut pending_rows = Vec::new();
    for firmware in &listing {
        if !matches!(firmware.source, FirmwareSource::Hub { .. }) {
            continue;
        }
        let key = firmware_key(firmware);
        if let Some(cached) = rows.remove(&key) {
            // Rebuild from the current listing so display metadata stays fresh.
            if let Ok(row) = remote_row(firmware, cached.url) {
                entries.push(row);
            }
        } else if let Some(row) = pending.remove(&key) {
            pending_rows.push(row);
        }
    }
    if !complete {
        // A failed additive feed is not a deleted firmware: keep the rows it would
        // have carried.
        let mut extra_rows: Vec<RemoteFirmware> = rows.into_values().collect();
        extra_rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
        entries.extend(extra_rows);
        let mut extra_pending: Vec<PendingRemote> = pending.into_values().collect();
        extra_pending.sort_by(|a, b| {
            a.firmware
                .name
                .cmp(&b.firmware.name)
                .then(a.firmware.version.cmp(&b.firmware.version))
        });
        pending_rows.extend(extra_pending);
    }

    let report = GardenReport {
        entries: entries.len(),
        pending: pending_rows.len(),
        attempted,
    };
    store.store_object(
        REMOTE_CATALOG_OBJECT,
        &RemoteCatalog {
            entries,
            pending: pending_rows,
            updated: now,
        },
    )?;
    Ok(report)
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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

    fn temp_store(name: &str) -> (Store, std::path::PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "fm1-remotes-{name}-{}-{nanos}",
            std::process::id()
        ));
        (Store::in_dir(&dir), dir)
    }

    #[test]
    fn garden_reuses_resolved_urls_and_keeps_failures_pending() {
        let (store, dir) = temp_store("garden");
        let listing = vec![
            listing_entry("A", "fw/a.fwsc"),
            listing_entry("B", "fw/b.fwsc"),
        ];
        let calls = Mutex::new(Vec::new());
        let flaky = |url: &str| {
            calls.lock().unwrap().push(url.to_string());
            if url.contains("b.fwsc") {
                Err("rate limited".to_string())
            } else {
                Ok(url.to_string())
            }
        };
        let report = garden_from(&store, listing.clone(), true, &flaky).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.pending, 1);
        let catalog = load_remote_catalog(&store);
        assert_eq!(catalog.entries[0].name, "A");
        assert_eq!(catalog.pending[0].firmware.name, "B");

        calls.lock().unwrap().clear();
        let ok = |url: &str| {
            calls.lock().unwrap().push(url.to_string());
            Ok(url.to_string())
        };
        let report = garden_from(&store, listing, true, &ok).unwrap();
        assert_eq!(report.entries, 2);
        assert_eq!(report.pending, 0);
        assert_eq!(report.attempted, 1, "only the blank is resolved again");
        let calls = calls.lock().unwrap();
        assert!(calls.iter().any(|url| url.contains("b.fwsc")));
        assert!(!calls.iter().any(|url| url.contains("a.fwsc")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn garden_retries_only_a_bounded_batch_of_pending_rows() {
        let (store, dir) = temp_store("garden-batch");
        let listing: Vec<Firmware> = (0..PENDING_RETRIES_PER_PASS + 4)
            .map(|ix| listing_entry(&format!("F{ix}"), &format!("fw/f{ix}.fwsc")))
            .collect();
        let failed = |_: &str| Err("rate limited".to_string());
        let report = garden_from(&store, listing.clone(), true, &failed).unwrap();
        assert_eq!(report.pending, PENDING_RETRIES_PER_PASS + 4);

        let ok = |url: &str| Ok(url.to_string());
        let report = garden_from(&store, listing.clone(), true, &ok).unwrap();
        assert_eq!(report.attempted, PENDING_RETRIES_PER_PASS);
        assert_eq!(report.entries, PENDING_RETRIES_PER_PASS);
        assert_eq!(report.pending, 4);

        // A new row is attempted even when the retry budget is spent.
        let mut with_new = listing;
        with_new.push(listing_entry("NEW", "fw/new.fwsc"));
        let report = garden_from(&store, with_new, true, &ok).unwrap();
        assert_eq!(report.pending, 0);
        assert_eq!(report.entries, PENDING_RETRIES_PER_PASS + 5);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_empty_listing_never_erases_the_catalog() {
        let (store, dir) = temp_store("garden-empty");
        let listing = vec![listing_entry("A", "fw/a.fwsc")];
        let ok = |url: &str| Ok(url.to_string());
        garden_from(&store, listing, true, &ok).unwrap();
        let report = garden_from(&store, Vec::new(), true, &ok).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(load_remote_catalog(&store).entries.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_partial_listing_keeps_the_rows_it_does_not_carry() {
        let (store, dir) = temp_store("garden-partial");
        let both = vec![
            listing_entry("A", "fw/a.fwsc"),
            listing_entry("B", "fw/b.fwsc"),
        ];
        let ok = |url: &str| Ok(url.to_string());
        garden_from(&store, both, true, &ok).unwrap();
        let only_a = vec![listing_entry("A", "fw/a.fwsc")];
        let report = garden_from(&store, only_a.clone(), false, &ok).unwrap();
        assert_eq!(report.entries, 2, "B stays while a feed is down");
        let report = garden_from(&store, only_a, true, &ok).unwrap();
        assert_eq!(report.entries, 1, "a complete listing drops B");
        std::fs::remove_dir_all(dir).ok();
    }

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
