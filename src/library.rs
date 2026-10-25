//! The firmware library's data layer: what's installed (builtins, imported files,
//! downloaded hub packages) and what the hub offers. The store object `library` is
//! the list of non-builtin entries; hub packages live in the blob store under
//! `hub/<file>`. Network work (catalog refresh, downloads) runs on background
//! threads; `list()` picks up finished downloads.

// The UI (wave 3b) wires this in; nothing constructs a Library yet.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::builtin::BUILTINS;
use crate::store::Store;

const HUB_BASE: &str = "https://thefirmwarehub.com";
/// The store object holding every non-builtin entry.
const LIBRARY_OBJECT: &str = "library";

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "source", rename_all = "lowercase")]
pub(crate) enum FirmwareSource {
    Builtin,
    File { path: String },
    /// `path` is the package's path on the hub ("fw/<file>"); `blob` is its key in the
    /// blob store (`hub/<file>`), set once the download completes.
    Hub { path: String, blob: Option<String> },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FirmwareDevice {
    Fm1,
    Smk37Pro,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub(crate) enum Status {
    #[default]
    Available,
    Downloading,
    Missing,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Firmware {
    pub name: String,
    pub byline: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub device: FirmwareDevice,
    /// A builtin older than the hub's version of the same name. Computed, not stored.
    #[serde(skip)]
    pub upgradable: bool,
    /// Computed on every `list()`, never stored.
    #[serde(skip)]
    pub status: Status,
    pub source: FirmwareSource,
}

/// A download in flight: the percent the UI shows, and the outcome the next `list()`
/// picks up.
struct Download {
    percent: AtomicU32,
    outcome: Mutex<Option<Result<Firmware, String>>>,
}

/// Where a hub refresh stands (the Library tab's status item reads it).
#[derive(Clone)]
pub(crate) enum HubStatus {
    /// Not fetching right now.
    Idle,
    Loading,
    Failed(String),
}

pub(crate) struct Library {
    store: Store,
    builtins: Vec<Firmware>,
    /// The persisted non-builtin entries (files and downloaded hub packages).
    stored: Vec<Firmware>,
    /// The online hub listing; a refresh thread swaps it in.
    hub: Arc<Mutex<Vec<Firmware>>>,
    /// The refresh's status; the thread updates it.
    hub_status: Arc<Mutex<HubStatus>>,
    /// When the last fetch started — automatic refreshes keep an hourly cadence.
    last_refresh: Arc<Mutex<Option<Instant>>>,
    downloads: HashMap<String, Arc<Download>>,
    /// What the last `list()` returned.
    view: Vec<Firmware>,
    /// Bumped whenever the installed list changes (import, remove, a download folded
    /// in); the app's pickers rebuild when it moves.
    generation: u64,
}

impl Library {
    pub(crate) fn load(store: &Store) -> Library {
        let stored: Vec<Firmware> = store.fetch_object(LIBRARY_OBJECT).unwrap_or_default();
        Library {
            store: store.clone(),
            builtins: BUILTINS.iter().map(builtin_entry).collect(),
            stored,
            hub: Arc::new(Mutex::new(Vec::new())),
            hub_status: Arc::new(Mutex::new(HubStatus::Idle)),
            last_refresh: Arc::new(Mutex::new(None)),
            downloads: HashMap::new(),
            view: Vec::new(),
            generation: 0,
        }
    }

    /// Installed entries first (builtins, then imported/downloaded), then the
    /// hub-only offers. Finished downloads are folded in here.
    pub(crate) fn list(&mut self) -> &[Firmware] {
        self.collect_downloads();
        let hub = self.hub.lock().unwrap().clone();
        let mut view: Vec<Firmware> = Vec::new();
        for firmware in self.builtins.iter_mut().chain(&mut self.stored) {
            firmware.upgradable = false;
            firmware.status = match &firmware.source {
                FirmwareSource::Builtin => {
                    // Hub entries never shadow a builtin; they flag it upgradable.
                    firmware.upgradable = hub.iter().any(|offer| {
                        offer.name == firmware.name
                            && version_newer(&offer.version, &firmware.version)
                    });
                    Status::Available
                }
                FirmwareSource::File { path } => {
                    if std::path::Path::new(path).is_file() {
                        Status::Available
                    } else {
                        Status::Missing
                    }
                }
                FirmwareSource::Hub { path, .. } => {
                    if self.downloads.contains_key(&firmware.name) {
                        Status::Downloading
                    } else if self.store.fetch_blob(&blob_key(path)).is_ok() {
                        Status::Available
                    } else {
                        Status::Missing
                    }
                }
            };
            view.push(firmware.clone());
        }
        // Hub offers not already installed under the same name.
        for offer in hub {
            if view.iter().any(|f| f.name == offer.name) {
                continue;
            }
            let mut offer = offer;
            offer.status = if self.downloads.contains_key(&offer.name) {
                Status::Downloading
            } else {
                Status::Available
            };
            view.push(offer);
        }
        self.view = view;
        &self.view
    }

    /// An owned copy of the current view, for UI code that borrows the app elsewhere.
    pub(crate) fn list_clone(&mut self) -> Vec<Firmware> {
        self.list().to_vec()
    }

    /// The percent of a download in flight, for the UI's progress display.
    pub(crate) fn download_percent(&self, name: &str) -> Option<u32> {
        self.downloads.get(name).map(|d| d.percent.load(Ordering::Relaxed))
    }

    /// The hub refresh's status (the Library tab's status item reads it).
    pub(crate) fn hub_status(&self) -> HubStatus {
        self.hub_status.lock().unwrap().clone()
    }

    /// The online listing's size (0 until the first successful fetch).
    pub(crate) fn hub_len(&self) -> usize {
        self.hub.lock().unwrap().len()
    }

    /// Fetch today's hub catalog on a background thread and swap the listing in, at
    /// most once an hour (app load and tab opens call this; within the hour it's a
    /// no-op). A failure keeps the old (or empty) list and shows in the status item.
    pub(crate) fn refresh_hub(&mut self) {
        self.refresh_hub_inner(false);
    }

    /// The reload/retry button's fetch: always goes.
    pub(crate) fn reload_hub(&mut self) {
        self.refresh_hub_inner(true);
    }

    fn refresh_hub_inner(&mut self, force: bool) {
        if !force
            && self
                .last_refresh
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < Duration::from_secs(3600))
        {
            return;
        }
        self.refresh_hub_from(format!("{HUB_BASE}/catalog.json?{}", today_utc()));
    }

    /// The workhorse, with the URL injectable so tests stay offline (a port that's
    /// closed is a deterministic failure). Every fetch — cadence or forced — counts
    /// for the cadence, so the timestamp is set here, the single funnel.
    fn refresh_hub_from(&mut self, url: String) {
        *self.last_refresh.lock().unwrap() = Some(Instant::now());
        *self.hub_status.lock().unwrap() = HubStatus::Loading;
        let hub = Arc::clone(&self.hub);
        let status = Arc::clone(&self.hub_status);
        std::thread::spawn(move || {
            let next = match fetch_text(&url).and_then(|body| parse_catalog(&body)) {
                Ok(listing) => {
                    *hub.lock().unwrap() = listing;
                    HubStatus::Idle
                }
                Err(e) => HubStatus::Failed(e),
            };
            *status.lock().unwrap() = next;
        });
    }

    /// Download a hub package into the blob store on a background thread. On
    /// completion the entry joins the persisted list (folded in by the next
    /// `list()`).
    pub(crate) fn start_download(&mut self, name: &str) {
        if self.downloads.contains_key(name) {
            return;
        }
        let entry = {
            let hub = self.hub.lock().unwrap();
            hub.iter().find(|f| f.name == name).cloned()
        };
        let Some(entry) = entry else { return };
        let FirmwareSource::Hub { path, .. } = entry.source.clone() else {
            return;
        };
        let key = blob_key(&path);
        let url = hub_download_url(&path);
        let download = Arc::new(Download {
            percent: AtomicU32::new(0),
            outcome: Mutex::new(None),
        });
        self.downloads.insert(name.to_string(), Arc::clone(&download));
        let store = self.store.clone();
        std::thread::spawn(move || {
            let result = download_package(&url, &download.percent)
                .and_then(|bytes| store.store_blob(&key, &bytes));
            let outcome = result.map(|()| Firmware {
                status: Status::Available,
                source: FirmwareSource::Hub {
                    path,
                    blob: Some(key),
                },
                ..entry
            });
            *download.outcome.lock().unwrap() = Some(outcome);
        });
    }

    /// Add a firmware file from disk.
    pub(crate) fn import_file(
        &mut self,
        path: &str,
        name: &str,
        version: &str,
        description: &str,
    ) {
        self.stored.retain(|f| f.name != name);
        self.stored.push(Firmware {
            name: name.to_string(),
            byline: String::new(),
            version: version.to_string(),
            description: description.to_string(),
            tags: Vec::new(),
            device: FirmwareDevice::Fm1,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::File { path: path.to_string() },
        });
        self.persist();
    }

    /// Forget an entry. Builtins stay. Files drop only the entry (the file stays on
    /// disk); hub entries drop the entry and the downloaded blob.
    pub(crate) fn remove(&mut self, name: &str) {
        if self.builtins.iter().any(|f| f.name == name) {
            return;
        }
        let before = self.stored.len();
        self.stored.retain(|f| {
            if f.name != name {
                return true;
            }
            if let FirmwareSource::Hub { path, .. } = &f.source {
                self.store.delete_blob(&blob_key(path)).ok();
            }
            false
        });
        if self.stored.len() != before {
            self.persist();
        }
    }

    /// Fold finished downloads into the stored list.
    fn collect_downloads(&mut self) {
        let finished: Vec<String> = self
            .downloads
            .iter()
            .filter(|(_, d)| d.outcome.lock().unwrap().is_some())
            .map(|(name, _)| name.clone())
            .collect();
        let mut changed = false;
        for name in finished {
            let download = self.downloads.remove(&name).unwrap();
            let outcome = download.outcome.lock().unwrap().take();
            if let Some(Ok(entry)) = outcome {
                self.stored.retain(|f| f.name != entry.name);
                self.stored.push(entry);
                changed = true;
            }
        }
        if changed {
            self.persist();
        }
    }

    fn persist(&mut self) {
        self.store.store_object(LIBRARY_OBJECT, &self.stored).ok();
        self.generation = self.generation.wrapping_add(1);
    }

    /// The installed list's change counter (see the field).
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

fn builtin_entry(b: &crate::builtin::BuiltinFirmware) -> Firmware {
    Firmware {
        name: b.name.to_string(),
        byline: b.byline.to_string(),
        version: b.version.to_string(),
        description: b.description.to_string(),
        tags: b.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect(),
        device: parse_device(b.device),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Builtin,
    }
}

/// The hub's device strings ("fm-1", "smk37-pro"); unknown/empty falls back to Fm1.
fn parse_device(product: &str) -> FirmwareDevice {
    if product.to_ascii_uppercase().starts_with("SMK") {
        FirmwareDevice::Smk37Pro
    } else {
        FirmwareDevice::Fm1
    }
}

/// Piecewise numeric version compare: split on non-digits ("1.10b2" -> [1, 10, 2]).
/// True when `candidate` is newer than `current`.
fn version_newer(candidate: &str, current: &str) -> bool {
    fn pieces(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|piece| !piece.is_empty())
            .map(|piece| piece.parse().unwrap_or(0))
            .collect()
    }
    let candidate = pieces(candidate);
    let current = pieces(current);
    for i in 0..candidate.len().max(current.len()) {
        let a = candidate.get(i).copied().unwrap_or(0);
        let b = current.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Days since the Unix epoch -> (year, month, day), Howard Hinnant's
/// civil_from_days. Valid for the whole epoch range the app cares about.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Today UTC, YYYY-MM-DD, from the system clock (no chrono).
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

fn fetch_text(url: &str) -> Result<String, String> {
    ureq::get(url)
        .call()
        .map_err(|e| format!("fetch {url}: {e}"))?
        .into_string()
        .map_err(|e| format!("read {url}: {e}"))
}

/// Download a package, reporting percent (of Content-Length) as it goes.
fn download_package(url: &str, percent: &AtomicU32) -> Result<Vec<u8>, String> {
    let response = ureq::get(url).call().map_err(|e| format!("fetch {url}: {e}"))?;
    let total: Option<u64> = response
        .header("content-length")
        .and_then(|h| h.parse().ok())
        .filter(|&t| t > 0);
    let mut body = Vec::new();
    let mut reader = response.into_reader();
    let mut chunk = [0u8; 65_536];
    loop {
        use std::io::Read;
        let read = reader.read(&mut chunk).map_err(|e| format!("read {url}: {e}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if let Some(total) = total {
            percent.store((body.len() as u64 * 100 / total).min(99) as u32, Ordering::Relaxed);
        }
    }
    percent.store(100, Ordering::Relaxed);
    Ok(body)
}

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

/// The blob-store key for a hub package path.
fn blob_key(hub_path: &str) -> String {
    format!("hub/{}", hub_path.rsplit('/').next().unwrap_or(hub_path))
}

/// The package's download URL.
fn hub_download_url(hub_path: &str) -> String {
    format!("{HUB_BASE}/{hub_path}")
}

/// Parse the hub catalog into hub-source entries. Accepts either a bare array or an
/// object with a `firmwares` array. Separated from the fetch so tests stay offline.
fn parse_catalog(json: &str) -> Result<Vec<Firmware>, String> {
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
            Firmware {
                name: entry.name,
                byline: entry.byline,
                version: entry.version,
                description: entry.description,
                tags,
                device: parse_device(&entry.device),
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

    fn temp_store(tag: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "fm1-grid-library-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::in_dir(&dir), dir)
    }

    fn hub_offer(name: &str, version: &str, file: &str) -> Firmware {
        Firmware {
            name: name.to_string(),
            byline: "hub author".to_string(),
            version: version.to_string(),
            description: "a hub firmware".to_string(),
            tags: vec!["kind".to_string()],
            device: FirmwareDevice::Fm1,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: format!("fw/{file}"),
                blob: None,
            },
        }
    }

    #[test]
    fn store_round_trip() {
        let (store, dir) = temp_store("roundtrip");
        let mut library = Library::load(&store);
        let file = dir.join("imported.fwsc");
        std::fs::write(&file, b"firmware bytes").unwrap();
        library.import_file(file.to_str().unwrap(), "imported", "1.2", "mine");
        let reloaded = Library::load(&store);
        let stored = &reloaded.stored;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "imported");
        assert_eq!(stored[0].version, "1.2");
        assert!(matches!(stored[0].source, FirmwareSource::File { .. }));
        // The computed status must not have been persisted.
        let json = std::fs::read_to_string(dir.join("objects/library.json")).unwrap();
        assert!(!json.contains("status"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_file_is_missing() {
        let (store, dir) = temp_store("missing");
        let mut library = Library::load(&store);
        library.import_file("/no/such/file.fwsc", "ghost", "1.0", "gone");
        let entry = library.list().iter().find(|f| f.name == "ghost").unwrap().clone();
        assert_eq!(entry.status, Status::Missing);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hub_never_shadows_builtin_and_flags_upgradable() {
        let (store, dir) = temp_store("shadow");
        let mut library = Library::load(&store);
        let builtin = BUILTINS.first().expect("no builtins");
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        let view = library.list();
        // The builtin appears once, under its own name, flagged upgradable.
        let matches: Vec<_> = view.iter().filter(|f| f.name == builtin.name).collect();
        assert_eq!(matches.len(), 1);
        assert!(matches!(matches[0].source, FirmwareSource::Builtin));
        assert!(matches[0].upgradable);
        // Same version is not an upgrade.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![hub_offer(builtin.name, builtin.version, "same.fwsc")];
        let view = library.list();
        let matches: Vec<_> = view.iter().filter(|f| f.name == builtin.name).collect();
        assert_eq!(matches.len(), 1);
        assert!(!matches[0].upgradable);
        // A hub-only name is listed after the installed entries.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![hub_offer("hub-only", "1.0", "hubonly.fwsc")];
        let view = library.list();
        let position = view.iter().position(|f| f.name == "hub-only").unwrap();
        assert_eq!(position, view.len() - 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn version_compare_is_piecewise_numeric() {
        assert!(version_newer("1.10", "1.9"));
        assert!(!version_newer("1.9", "1.10"));
        assert!(!version_newer("1.10", "1.10"));
        assert!(version_newer("2.0", "1.10"));
        assert!(version_newer("1.2.1", "1.2"));
        assert!(!version_newer("1.2", "1.2.1"));
    }

    #[test]
    fn remove_rules() {
        let (store, dir) = temp_store("remove");
        let mut library = Library::load(&store);
        // Builtins can't be removed.
        let builtin_name = BUILTINS.first().unwrap().name.to_string();
        library.remove(&builtin_name);
        assert!(library.list().iter().any(|f| f.name == builtin_name));
        // A File entry is dropped; the file itself stays.
        let file = dir.join("keep.fwsc");
        std::fs::write(&file, b"bytes").unwrap();
        library.import_file(file.to_str().unwrap(), "imported", "1.0", "");
        library.remove("imported");
        assert!(!library.list().iter().any(|f| f.name == "imported"));
        assert!(file.is_file());
        assert!(Library::load(&store).stored.is_empty());
        // A Hub entry is dropped along with its blob.
        let mut entry = hub_offer("downloaded", "1.0", "dl.fwsc");
        if let FirmwareSource::Hub { path, blob } = &mut entry.source {
            let key = blob_key(path);
            store.store_blob(&key, b"package").unwrap();
            *blob = Some(key);
        }
        library.stored.push(entry);
        library.persist();
        library.remove("downloaded");
        assert!(!library.list().iter().any(|f| f.name == "downloaded"));
        assert!(store.list_blobs(None).unwrap().is_empty());
        assert!(Library::load(&store).stored.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn catalog_parses() {
        // The real catalog's field names (author/what/kind/vintage/product/pkg).
        let json = r#"{"firmwares": [
            {"name": "fm1-open", "version": "2.1", "pkg": "fw/mvave/fm1-open.fwsc",
             "sha256": "abc123", "author": "M-VAVE", "product": "FM-1_975",
             "what": "dev kit firmware", "kind": "official"},
            {"name": "smk-os", "version": "0.9", "pkg": "fw/smk-os.fwsc",
             "product": "SMK-37 Pro", "kind": "synth", "vintage": true}
        ]}"#;
        let listing = parse_catalog(json).unwrap();
        assert_eq!(listing.len(), 2);
        assert_eq!(listing[0].name, "fm1-open");
        assert_eq!(listing[0].device, FirmwareDevice::Fm1);
        assert_eq!(listing[0].byline, "M-VAVE");
        assert_eq!(listing[0].description, "dev kit firmware");
        assert_eq!(listing[0].tags, vec!["official"]);
        match &listing[0].source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, "fw/mvave/fm1-open.fwsc");
                assert_eq!(blob, &None);
                assert_eq!(
                    hub_download_url(path),
                    "https://thefirmwarehub.com/fw/mvave/fm1-open.fwsc"
                );
                assert_eq!(blob_key(path), "hub/fm1-open.fwsc");
            }
            _ => panic!("hub source expected"),
        }
        assert_eq!(listing[1].device, FirmwareDevice::Smk37Pro);
        assert_eq!(listing[1].tags, vec!["synth", "vintage"]);
        // A bare array works too.
        let listing = parse_catalog(r#"[{"name": "x", "pkg": "fw/x.fwsc"}]"#).unwrap();
        assert_eq!(listing.len(), 1);
        assert!(parse_catalog("not json").is_err());
    }

    #[test]
    fn hub_refresh_keeps_an_hourly_cadence() {
        let (store, dir) = temp_store("cadence");
        let mut library = Library::load(&store);
        library.refresh_hub_from("http://127.0.0.1:1/catalog.json".to_string());
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(50));
            if matches!(library.hub_status(), HubStatus::Failed(_)) {
                break;
            }
        }
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        // Within the hour, an automatic refresh is a no-op — the status must not
        // flick back to Loading (and nothing goes on the network).
        library.refresh_hub();
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        // The button always goes.
        library.reload_hub();
        assert!(matches!(library.hub_status(), HubStatus::Loading));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hub_status_reports_loading_then_failure() {
        let (store, dir) = temp_store("hubstatus");
        let mut library = Library::load(&store);
        // A closed port fails fast and deterministically.
        library.refresh_hub_from("http://127.0.0.1:1/catalog.json".to_string());
        assert!(matches!(library.hub_status(), HubStatus::Loading));
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if matches!(library.hub_status(), HubStatus::Failed(_)) {
                break;
            }
        }
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_751), (2026, 10, 25));
        // Round-trip sanity across a span including leap years.
        for days in [-719_162, -1, 0, 1, 18_282, 20_768, 29_303] {
            let (y, m, d) = civil_from_days(days);
            assert!((1..=12).contains(&m) && (1..=31).contains(&d) && y >= 1);
        }
    }
}

