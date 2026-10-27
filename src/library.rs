//! The firmware library's data layer: what's installed (builtins, imported files,
//! downloaded hub packages) and what the online listing offers. The listing merges
//! two sources fetched together: the Firmware Hub catalog (the source of truth) and
//! the fm1-editor site's firmware page, which is scraped (it publishes no feed) and
//! is additive only. The store object `library` is the list of non-builtin entries;
//! hub packages live in the blob store under `hub/<file>`, packages from absolute
//! URLs under `ext/<file>-<hash>`. Network work (catalog refresh, downloads) runs
//! on background threads; `list()` picks up finished downloads.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::builtin::BUILTINS;
use crate::store::Store;

const HUB_BASE: &str = "https://thefirmwarehub.com";
/// The secondary listing: the fm1-editor site's firmware page, scraped for entries
/// the hub lacks. Additive only — the hub catalog stays the source of truth.
const EDITOR_PAGE: &str = "https://fm1-editor.com/firmware/";
/// The store object holding every non-builtin entry.
const LIBRARY_OBJECT: &str = "library";

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum FirmwareSource {
    Builtin,
    File { path: String },
    /// `path` is the package's path on the hub ("fw/<file>") or, for entries scraped
    /// from the fm1-editor page, the package's absolute URL; `blob` is its key in
    /// the blob store (see `blob_key`), set once the download completes.
    Hub { path: String, blob: Option<String> },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FirmwareDevice {
    Fm1,
    Smk37Pro,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Status {
    #[default]
    Available,
    Downloading,
    Missing,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Firmware {
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
pub enum HubStatus {
    /// Not fetching right now.
    Idle,
    Loading,
    Failed(String),
}

pub struct Library {
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
    pub fn load(store: &Store) -> Library {
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
    pub fn list_clone(&mut self) -> Vec<Firmware> {
        self.list().to_vec()
    }

    /// The percent of a download in flight, for the UI's progress display.
    pub fn download_percent(&self, name: &str) -> Option<u32> {
        self.downloads.get(name).map(|d| d.percent.load(Ordering::Relaxed))
    }

    /// The hub refresh's status (the Library tab's status item reads it).
    pub fn hub_status(&self) -> HubStatus {
        self.hub_status.lock().unwrap().clone()
    }

    /// The online listing's size (0 until the first successful fetch).
    #[cfg(feature = "gui")]
    pub(crate) fn hub_len(&self) -> usize {
        self.hub.lock().unwrap().len()
    }

    /// Fetch today's hub catalog and the fm1-editor page on a background thread and
    /// swap the merged listing in, at most once an hour (app load and tab opens call
    /// this; within the hour it's a no-op). A hub failure keeps the old (or empty)
    /// list and shows in the status item.
    pub fn refresh_hub(&mut self) {
        self.refresh_hub_inner(false);
    }

    /// The reload/retry button's fetch: always goes.
    pub fn reload_hub(&mut self) {
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
        let day = today_utc();
        self.refresh_hub_from(
            format!("{HUB_BASE}/catalog.json?{day}"),
            format!("{EDITOR_PAGE}?{day}"),
        );
    }

    /// The workhorse, with the URLs injectable so tests stay offline (a port that's
    /// closed is a deterministic failure). Both sources are fetched at the same
    /// time. The hub decides the outcome: its failure keeps the old listing, while
    /// an editor-page failure only costs the additive entries (and shows in the
    /// status). Every fetch — cadence or forced — counts for the cadence, so the
    /// timestamp is set here, the single funnel.
    fn refresh_hub_from(&mut self, url: String, editor_url: String) {
        *self.last_refresh.lock().unwrap() = Some(Instant::now());
        *self.hub_status.lock().unwrap() = HubStatus::Loading;
        let hub = Arc::clone(&self.hub);
        let status = Arc::clone(&self.hub_status);
        std::thread::spawn(move || {
            let (listing, editor) = std::thread::scope(|scope| {
                let editor = scope
                    .spawn(|| fetch_text(&editor_url).and_then(|body| parse_editor_page(&body)));
                let listing = fetch_text(&url).and_then(|body| parse_catalog(&body));
                (listing, editor.join())
            });
            let next = match listing {
                Err(e) => HubStatus::Failed(e),
                Ok(listing) => {
                    let (listing, outcome) = match editor {
                        Ok(Ok(extra)) => (merge_listings(listing, extra), HubStatus::Idle),
                        Ok(Err(e)) => (listing, HubStatus::Failed(e)),
                        Err(_) => (
                            listing,
                            HubStatus::Failed("editor page thread panicked".to_string()),
                        ),
                    };
                    *hub.lock().unwrap() = listing;
                    outcome
                }
            };
            *status.lock().unwrap() = next;
        });
    }

    /// Download a hub package into the blob store on a background thread. On
    /// completion the entry joins the persisted list (folded in by the next
    /// `list()`).
    pub fn start_download(&mut self, name: &str) {
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
    #[cfg(any(feature = "gui", test))]
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
    #[cfg(any(feature = "gui", test))]
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
    #[cfg(feature = "gui")]
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

/// The blob-store key for a package path. Hub paths keep their file names; absolute
/// URLs (fm1-editor entries) get a hash of the whole URL, since their last segments
/// collide ("…/releases/latest" is common) or carry queries.
fn blob_key(hub_path: &str) -> String {
    if hub_path.starts_with("http") {
        let file = hub_path
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|file| !file.is_empty())
            .unwrap_or("package");
        format!("ext/{}-{:016x}", file, fnv1a(hub_path))
    } else {
        format!("hub/{}", hub_path.rsplit('/').next().unwrap_or(hub_path))
    }
}

/// FNV-1a 64-bit: for blob keys only, not a checksum.
fn fnv1a(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The package's download URL; absolute URLs pass through untouched.
fn hub_download_url(hub_path: &str) -> String {
    if hub_path.starts_with("http") {
        hub_path.to_string()
    } else {
        format!("{HUB_BASE}/{hub_path}")
    }
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

/// The slice from the end of `from` to the next `to` after it.
fn between<'a>(hay: &'a str, from: &str, to: &str) -> Option<&'a str> {
    let start = hay.find(from)? + from.len();
    let end = hay[start..].find(to)? + start;
    Some(&hay[start..end])
}

/// The few entities the editor page emits; `&amp;` last so it can't double-decode.
fn unescape(text: &str) -> String {
    text.replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Tags stripped, entities resolved, whitespace collapsed to single spaces.
fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = match rest[open..].find('>') {
            Some(close) => &rest[open + close + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    unescape(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The status line's version: "Latest 1.4" and "Beta · Latest 1.4" carry one;
/// "No numbered releases" doesn't, and an empty version never wins a conflict.
fn status_version(status: &str) -> String {
    status
        .split_once("Latest ")
        .map(|(_, version)| version.trim().to_string())
        .unwrap_or_default()
}

/// An editor entry's download: a direct package link when one is offered, else the
/// install/download page, the release page, the source repository, or the first
/// external link, in that order.
fn entry_url(links: &[(String, String)]) -> Option<String> {
    let by_label = |prefix: &str| {
        links
            .iter()
            .find(|(_, label)| label.starts_with(prefix))
            .map(|(url, _)| url.clone())
    };
    links
        .iter()
        .map(|(url, _)| url.clone())
        .find(|url| {
            let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
            path.ends_with(".fwsc") || path.ends_with(".zip")
        })
        .or_else(|| by_label("Install"))
        .or_else(|| by_label("Download"))
        .or_else(|| by_label("Release"))
        .or_else(|| by_label("Source"))
        .or_else(|| links.first().map(|(url, _)| url.clone()))
}

/// Parse the fm1-editor firmware page into hub-style offers. The page is a static,
/// hand-maintained listing (its repo confirms no feed exists) with one `<article>`
/// per entry: the kicker is the author, `data-categories` the tags, the status line
/// the version, the blurb paragraphs the description. Entries without a name or any
/// external link are skipped. Separated from the fetch so tests stay offline.
fn parse_editor_page(html: &str) -> Result<Vec<Firmware>, String> {
    let mut entries = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<article") {
        rest = &rest[start + "<article".len()..];
        // Closing tags are matched by their prefix only: the page wraps them across
        // lines ("</span\n>"), and the content slice ends at the marker either way.
        let end = rest.find("</article");
        let chunk = &rest[..end.unwrap_or(rest.len())];
        rest = &rest[end.unwrap_or(rest.len())..];

        let Some(name) = between(chunk, "<h2 class=\"firmware-title\"", "</h2")
            .and_then(|inner| inner.find('>').map(|i| &inner[i + 1..]))
            .map(text_of)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let byline = between(chunk, "<p class=\"synthwave-kicker\">", "</p")
            .map(text_of)
            .unwrap_or_default();
        let version = between(chunk, "<p class=\"firmware-status\">", "</p")
            .and_then(|inner| between(inner, "<span>", "</span"))
            .map(|status| status_version(&text_of(status)))
            .unwrap_or_default();
        let tags = between(chunk, "data-categories=\"", "\"")
            .map(|kinds| kinds.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        let mut blurbs = Vec::new();
        let mut section = chunk;
        while let Some(open) = section.find("<p class=\"firmware-blurb\">") {
            let from = open + "<p class=\"firmware-blurb\">".len();
            let Some(close) = section[from..].find("</p") else { break };
            blurbs.push(text_of(&section[from..from + close]));
            let past = section[from + close..]
                .find('>')
                .map_or(section.len(), |i| from + close + i + 1);
            section = &section[past..];
        }
        let mut links = Vec::new();
        let mut section = chunk;
        // The page writes anchors across lines: `<a\n class=…` and `…</a\n>`.
        while let Some(anchor) = section.find("<a") {
            section = &section[anchor + 2..];
            if !section.starts_with(|c: char| c.is_ascii_whitespace()) {
                continue;
            }
            let Some(tag_end) = section.find('>') else { break };
            if let Some(href) = between(&section[..tag_end], "href=\"", "\"") {
                if href.starts_with("http") {
                    let label = section[tag_end + 1..]
                        .find("</a")
                        .map(|i| text_of(&section[tag_end + 1..tag_end + 1 + i]))
                        .unwrap_or_default();
                    links.push((href.to_string(), label));
                }
            }
            section = &section[tag_end + 1..];
        }
        let Some(url) = entry_url(&links) else { continue };

        entries.push(Firmware {
            device: if name.to_ascii_uppercase().contains("SMK") {
                FirmwareDevice::Smk37Pro
            } else {
                FirmwareDevice::Fm1
            },
            name,
            byline,
            version,
            description: blurbs.join("\n\n"),
            tags,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: url,
                blob: None,
            },
        });
    }
    if entries.is_empty() {
        return Err("no firmware entries found on the editor page".to_string());
    }
    Ok(entries)
}

/// Fold the editor listing into the hub's. The hub is the source of truth: its
/// entries keep their name, author and description. A name both sources carry
/// (compared case-insensitively) gets the union of their tags, and the version and
/// download URL of whichever source is newer. Editor-only names append at the end.
fn merge_listings(mut hub: Vec<Firmware>, editor: Vec<Firmware>) -> Vec<Firmware> {
    let mut extra = Vec::new();
    for offer in editor {
        let mut matched = false;
        for entry in hub.iter_mut() {
            if !entry.name.trim().eq_ignore_ascii_case(offer.name.trim()) {
                continue;
            }
            matched = true;
            for tag in &offer.tags {
                if !entry.tags.iter().any(|have| have.eq_ignore_ascii_case(tag)) {
                    entry.tags.push(tag.clone());
                }
            }
            if version_newer(&offer.version, &entry.version) {
                entry.version = offer.version.clone();
                entry.source = offer.source.clone();
            }
        }
        if !matched {
            extra.push(offer);
        }
    }
    hub.extend(extra);
    hub
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
    fn editor_page_parses() {
        // The page's real markup shape: one <article> per entry.
        let html = r##"
        <article class="firmware-entry synthwave-panel" id="karaoke"
                 aria-labelledby="firmware-karaoke" data-categories="grooveboxes games"
                 data-updated="2026-10-19" data-stars="4">
          <div class="firmware-body">
            <p class="synthwave-kicker">seajaysec</p>
            <h2 class="firmware-title" id="firmware-karaoke">Karaoke &amp; Robots</h2>
            <p class="firmware-status">
              <span>Beta · Latest 0.10.0</span
              ><span class="firmware-updated"
                >Updated <time datetime="2026-10-19">19 Oct 2026</time></span
              >
            </p>
            <p class="firmware-blurb">
              Sings songs you
              bring.
            </p>
            <p class="firmware-blurb">A <a href="#official-open">linked</a> second paragraph.</p>
            <ul class="firmware-links">
              <li>
                <a
                  class="firmware-anchor"
                  href="#karaoke"
                  >#</a
                >
              </li>
              <li>
                <a
                  class="hero-action firmware-link"
                  href="https://karaoke.example.com"
                  rel="noopener"
                  target="_blank"
                  >Install ↗</a
                >
              </li>
              <li>
                <a
                  class="hero-action firmware-link"
                  href="https://github.com/seajaysec/karaoke"
                  rel="noopener"
                  target="_blank"
                  >Source ↗</a
                >
              </li>
            </ul>
          </div>
        </article>
        <article class="firmware-entry" id="doom" data-categories="games" data-updated="2026-10-22">
          <div class="firmware-body">
            <p class="synthwave-kicker">id</p>
            <h2 class="firmware-title" id="firmware-doom">Doom FM-1 / SMK</h2>
            <p class="firmware-status"><span>No numbered releases</span></p>
            <p class="firmware-blurb">It runs.</p>
            <ul class="firmware-links">
              <li>
                <a
                  href="https://doom.example.com/play"
                  >Try it in a browser ↗</a
                >
              </li>
              <li>
                <a
                  href="https://files.example.com/doom.zip?raw=1"
                  >Download ↗</a
                >
              </li>
            </ul>
          </div>
        </article>
        <article class="firmware-entry" id="linkless" data-categories="tools">
          <div class="firmware-body">
            <h2 class="firmware-title" id="firmware-linkless">No Links</h2>
            <p class="firmware-status"><span>Latest 1.0</span></p>
          </div>
        </article>
        <article class="not-an-entry" data-stars="1">
          <p>No title, skipped.</p>
        </article>
        "##;
        let entries = parse_editor_page(html).unwrap();
        assert_eq!(entries.len(), 2);
        let karaoke = &entries[0];
        assert_eq!(karaoke.name, "Karaoke & Robots");
        assert_eq!(karaoke.byline, "seajaysec");
        assert_eq!(karaoke.version, "0.10.0");
        assert_eq!(karaoke.tags, vec!["grooveboxes", "games"]);
        assert_eq!(karaoke.description, "Sings songs you bring.\n\nA linked second paragraph.");
        assert_eq!(karaoke.device, FirmwareDevice::Fm1);
        match &karaoke.source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, "https://karaoke.example.com");
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
        let doom = &entries[1];
        assert_eq!(doom.name, "Doom FM-1 / SMK");
        assert_eq!(doom.device, FirmwareDevice::Smk37Pro);
        // "No numbered releases" carries no version, and a direct package link beats
        // every page link.
        assert_eq!(doom.version, "");
        match &doom.source {
            FirmwareSource::Hub { path, .. } => {
                assert_eq!(path, "https://files.example.com/doom.zip?raw=1")
            }
            _ => panic!("hub source expected"),
        }
        // An entry with no external link and a non-entry article are both skipped.
        assert!(entries.iter().all(|f| f.name != "No Links"));
        assert!(parse_editor_page("<p>nothing here</p>").is_err());
    }

    #[test]
    fn merge_combines_tags_and_prefers_the_newer_side() {
        let hub = vec![
            hub_offer("zp12", "2.5", "zp12-2.5.fwsc"),
            hub_offer("Rainbow Mode", "1.1.5-rb", "rainbow.fwsc"),
        ];
        // The editor has a newer zp12: its version and URL win, its tags join in.
        let mut newer = hub_offer("zp12", "3.0", "ignored.fwsc");
        newer.tags = vec!["kind".to_string(), "drums".to_string()];
        newer.byline = "editor author".to_string();
        newer.source = FirmwareSource::Hub {
            path: "https://example.com/zp12-3.0.fwsc".to_string(),
            blob: None,
        };
        // Same Rainbow Mode, spelled differently and older: tags merge, the hub's
        // version and URL stay.
        let mut older = hub_offer("rainbow mode", "1.0", "ignored.fwsc");
        older.tags = vec!["ambient".to_string()];
        older.source = FirmwareSource::Hub {
            path: "https://example.com/rainbow.fwsc".to_string(),
            blob: None,
        };
        // An editor-only entry appends.
        let only = hub_offer("editor-only", "0.1", "only.fwsc");
        let merged = merge_listings(hub, vec![newer, older, only]);
        assert_eq!(merged.len(), 3);
        let zp12 = merged.iter().find(|f| f.name == "zp12").unwrap();
        assert_eq!(zp12.version, "3.0");
        assert_eq!(zp12.tags, vec!["kind", "drums"]);
        // The hub stays the source of truth for the rest.
        assert_eq!(zp12.byline, "hub author");
        match &zp12.source {
            FirmwareSource::Hub { path, .. } => {
                assert_eq!(path, "https://example.com/zp12-3.0.fwsc")
            }
            _ => panic!("hub source expected"),
        }
        let rainbow = merged.iter().find(|f| f.name == "Rainbow Mode").unwrap();
        assert_eq!(rainbow.version, "1.1.5-rb");
        assert_eq!(rainbow.tags, vec!["kind", "ambient"]);
        match &rainbow.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/rainbow.fwsc"),
            _ => panic!("hub source expected"),
        }
        assert_eq!(merged[2].name, "editor-only");
    }

    #[test]
    fn absolute_urls_key_uniquely_and_download_as_given() {
        // Hub-relative paths behave exactly as before.
        assert_eq!(blob_key("fw/x.fwsc"), "hub/x.fwsc");
        assert_eq!(
            hub_download_url("fw/x.fwsc"),
            "https://thefirmwarehub.com/fw/x.fwsc"
        );
        // Absolute URLs pass through to the fetch and key on the whole URL: two
        // "latest" release pages must not share a blob.
        let a = "https://github.com/a/proj/releases/latest";
        let b = "https://github.com/b/proj/releases/latest";
        assert_eq!(hub_download_url(a), a);
        assert!(blob_key(a).starts_with("ext/latest-"));
        assert_ne!(blob_key(a), blob_key(b));
        // Queries stay out of the file name.
        assert!(blob_key("https://x.test/fw/p.fwsc?raw=1").starts_with("ext/p.fwsc-"));
    }

    #[test]
    fn hub_refresh_keeps_an_hourly_cadence() {
        let (store, dir) = temp_store("cadence");
        let mut library = Library::load(&store);
        library.refresh_hub_from(
            "http://127.0.0.1:1/catalog.json".to_string(),
            "http://127.0.0.1:1/firmware/".to_string(),
        );
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
        library.refresh_hub_from(
            "http://127.0.0.1:1/catalog.json".to_string(),
            "http://127.0.0.1:1/firmware/".to_string(),
        );
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

