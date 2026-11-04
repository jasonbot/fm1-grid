//! The firmware library's data layer: what's installed (builtins, imported files,
//! downloaded hub packages) and what the online listing offers. The listing merges
//! five sources fetched together: the Firmware Hub catalog (the source of truth),
//! the fm1-editor site's firmware page and Reverbose's FM-1 and SMK-37 lists, all
//! scraped (none publishes a feed), and mutant.cloud's catalog, all additive only.
//! Only mutant.cloud publishes SMK-37 packages; the hub's SMK-37 page links to the
//! makers' own installers. The store object `library` is the list of non-builtin
//! entries; hub packages live in the blob store under `hub/<file>`, packages from
//! absolute URLs under `ext/<file>-<hash>`. Network work (catalog refresh,
//! downloads) runs on background threads; `list()` picks up finished downloads.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::builtin::{BUILTINS, BUNDLED_INDEX};
use crate::download_heuristics::{fetch_package, looks_like_web_page};
use crate::sources::{editor, hub, mutant, reverbose};
use crate::store::Store;

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
    /// Any SMK-37 model (Pro, Elite, MKE-P37); the row's variant tag names which.
    /// The stored key stays "smk37pro" for old library records.
    #[serde(rename = "smk37pro")]
    Smk37,
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
    /// An installed entry (builtin or downloaded hub package) older than the hub's
    /// version of the same name. Computed, not stored.
    #[serde(skip)]
    pub upgradable: bool,
    /// Computed on every `list()`, never stored.
    #[serde(skip)]
    pub status: Status,
    /// The project's web page, when the listing source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// The source code's home, when the listing source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
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
    /// Names whose last download failed; [`Library::next_download`] skips them so a
    /// broken offer can't jam a Download All sweep. A forced hub refresh, an
    /// explicit [`Library::start_download`] or a fresh toggle-on clears the mark.
    download_failed: HashSet<String>,
    /// The Download All toggle, mirrored from the app (session state lives there).
    download_all: bool,
    /// The Library tab's device filter, mirrored from the app: (FM-1, SMK-37).
    /// None allows everything (the TUI, which has no filter).
    devices: Option<(bool, bool)>,
    /// A hub swap was observed by `list()` but no sweep has run since.
    swap_pending: bool,
    /// What the last `list()` returned. Rebuilt only when its inputs move — the UI
    /// calls `list()` every frame.
    view: Vec<Firmware>,
    /// Set when the view's inputs change (import, remove, a download starting or
    /// folding in); the next `list()` rebuilds.
    view_dirty: bool,
    /// Bumped whenever the returned view changes (a rebuild, or a status re-stat
    /// that flipped an entry between Available and Missing). The Library tab caches
    /// its flattened list against it.
    view_generation: u64,
    /// Set by the hub refresh thread when it swaps the listing in.
    hub_swapped: Arc<AtomicBool>,
    /// The last on-disk existence sweep (file/blob stats), throttled so the per-frame
    /// `list()` doesn't stat every entry every frame.
    last_stat: Instant,
    /// Bumped whenever the installed list changes (import, remove, a download folded
    /// in); the app's pickers rebuild when it moves.
    generation: u64,
}

/// How often `list()` re-stats files/blobs for the Missing status. External deletes
/// surface within this; everything app-driven rebuilds immediately.
const STAT_CADENCE: Duration = Duration::from_millis(500);

impl Library {
    pub fn load(store: &Store) -> Library {
        let mut stored: Vec<Firmware> = store.fetch_object(LIBRARY_OBJECT).unwrap_or_default();
        // Entries stored before origin tags existed get one from their source.
        for firmware in &mut stored {
            if !firmware.tags.iter().any(|tag| tag.starts_with("source:")) {
                let tag = match &firmware.source {
                    // Pre-tag downloads of fm1-editor offers were stored as Hub
                    // entries whose path is the absolute page URL; only the hub's
                    // own entries carry relative paths.
                    FirmwareSource::Hub { path, .. } if path.starts_with("http") => {
                        "source:fm1-editor"
                    }
                    FirmwareSource::Hub { .. } => "source:hub",
                    FirmwareSource::File { .. } => "source:file",
                    FirmwareSource::Builtin => "source:builtin",
                };
                firmware.tags.push(tag.to_string());
            }
        }
        Library {
            store: store.clone(),
            builtins: BUILTINS.iter().map(builtin_entry).collect(),
            stored,
            hub: Arc::new(Mutex::new(index_listing())),
            hub_status: Arc::new(Mutex::new(HubStatus::Idle)),
            last_refresh: Arc::new(Mutex::new(None)),
            downloads: HashMap::new(),
            download_failed: HashSet::new(),
            download_all: false,
            devices: None,
            swap_pending: false,
            view: Vec::new(),
            view_dirty: true,
            view_generation: 0,
            hub_swapped: Arc::new(AtomicBool::new(false)),
            last_stat: Instant::now(),
            generation: 0,
        }
    }

    /// Installed entries first (builtins, then imported/downloaded), then the
    /// hub-only offers. Finished downloads are folded in here.
    ///
    /// The view is cached: rebuilding means cloning every entry, and the old
    /// per-frame rebuild also re-read every downloaded blob just to check presence.
    /// A rebuild happens only when the inputs move (import, remove, download start
    /// or finish, hub swap); on-disk presence is re-statted on [`STAT_CADENCE`].
    pub fn list(&mut self) -> &[Firmware] {
        let finished = self.collect_downloads();
        let swapped = self.hub_swapped.swap(false, Ordering::Relaxed);
        self.swap_pending |= swapped;
        if finished || swapped || self.view_dirty {
            self.rebuild_view();
        } else if self.last_stat.elapsed() >= STAT_CADENCE {
            self.refresh_statuses();
        }
        &self.view
    }

    /// Rebuild the cached view from the installed entries and the hub listing.
    fn rebuild_view(&mut self) {
        self.last_stat = Instant::now();
        let hub = self.hub.lock().unwrap().clone();
        let mut view: Vec<Firmware> = Vec::new();
        for firmware in self.builtins.iter_mut().chain(&mut self.stored) {
            // Hub entries never shadow an installed entry; they flag it upgradable.
            // Builtins and installed hub packages both qualify — the listing moving
            // ahead of an installed version is an upgrade either way.
            firmware.upgradable = !matches!(firmware.source, FirmwareSource::File { .. })
                && hub.iter().any(|offer| {
                    same_firmware(firmware, offer) && version_newer(&offer.version, &firmware.version)
                });
            firmware.status = entry_status(firmware, &self.downloads, &self.store);
            view.push(firmware.clone());
        }
        // Hub offers not already installed under the same name.
        for offer in hub {
            if view.iter().any(|f| same_firmware(f, &offer)) {
                continue;
            }
            let mut offer = offer;
            offer.status = entry_status(&offer, &self.downloads, &self.store);
            view.push(offer);
        }
        self.view = view;
        self.view_dirty = false;
        self.view_generation = self.view_generation.wrapping_add(1);
    }

    /// Re-check on-disk presence in place (a file or blob can vanish outside the
    /// app). Names and order don't move, so the view keeps its entries.
    fn refresh_statuses(&mut self) {
        self.last_stat = Instant::now();
        let mut changed = false;
        for firmware in &mut self.view {
            let status = entry_status(firmware, &self.downloads, &self.store);
            changed |= status != firmware.status;
            firmware.status = status;
        }
        if changed {
            self.view_generation = self.view_generation.wrapping_add(1);
        }
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
    pub fn hub_len(&self) -> usize {
        self.hub.lock().unwrap().len()
    }

    /// Fetch today's hub catalog and every additive listing on a background thread
    /// and swap the merged listing in, at most once an hour (app load and tab opens
    /// call this; within the hour it's a no-op). A hub failure keeps the old (or
    /// empty) list and shows in the status item.
    pub fn refresh_hub(&mut self) {
        self.refresh_hub_inner(false);
    }

    /// The reload/retry button's fetch: always goes. A forced refresh also clears
    /// the failed-download marks — the listing may carry fixed URLs.
    pub fn reload_hub(&mut self) {
        self.download_failed.clear();
        self.refresh_hub_inner(true);
    }

    /// Mirror the Library tab's device filter: Download All only walks the device
    /// families shown. `None` allows everything. A filter change while Download All
    /// runs starts the next allowed offer at once.
    pub fn set_device_filter(&mut self, devices: Option<(bool, bool)>) {
        self.devices = devices;
        if self.download_all && self.downloads.is_empty() {
            self.sweep_downloads();
        }
    }

    /// The Download All toggle. Turning it on sweeps at once; after that a sweep
    /// runs only when the app's [`Library::pump_downloads`] sees a download finish
    /// or a fresh hub listing swap in. A fresh toggle-on clears the failed marks,
    /// so earlier failures get one retry per arming.
    pub fn set_download_all(&mut self, on: bool) {
        if on && !self.download_all {
            self.download_failed.clear();
        }
        self.download_all = on;
        if on {
            self.sweep_downloads();
        }
    }

    /// The app tick's Download All driver. Cheap while idle (one pass over the
    /// in-flight map plus two flags); the actual sweep runs only when a download
    /// has an outcome to fold in or a hub swap is waiting.
    pub fn pump_downloads(&mut self) {
        if !self.download_all {
            return;
        }
        let finished = self
            .downloads
            .values()
            .any(|d| d.outcome.lock().unwrap().is_some());
        if finished || self.swap_pending || self.hub_swapped.load(Ordering::Relaxed) {
            self.sweep_downloads();
        }
    }

    /// One Download All step: fold in what finished (a failure marks its name and
    /// the walk moves past it), then start the next downloadable. One in flight at
    /// a time; the next step is kicked by [`Library::pump_downloads`].
    fn sweep_downloads(&mut self) {
        self.swap_pending = false;
        // A single attempt: if `start_download` no-ops (the offer vanished under a
        // hub swap mid-sweep), the walk stalls until the next trigger rather than
        // spin on the same name.
        if let Some(name) = self.next_download() {
            self.start_download(&name);
        }
    }

    /// A Download All sweep's next target: the first entry in `list()` order that
    /// can be downloaded — a hub offer whose package isn't at hand (or whose stored
    /// blob went missing) — or upgraded: a builtin or installed hub package the
    /// hub carries a newer version of, where the upgrade lands the same download
    /// under the entry's name. One
    /// at a time: None while any download is in flight, when nothing is left, or
    /// once a failed name (marked by [`Library::collect_downloads`]) is all that
    /// remains. An upgrade counts as done once a downloaded same-firmware entry is
    /// at least the hub's version, so a completed upgrade isn't fetched again.
    pub fn next_download(&mut self) -> Option<String> {
        self.list();
        if !self.downloads.is_empty() {
            return None;
        }
        let hub = self.hub.lock().unwrap().clone();
        let view = &self.view;
        let failed = &self.download_failed;
        let devices = self.devices;
        view.iter().find_map(|firmware| {
            if failed.contains(&firmware.name) {
                return None;
            }
            let shown = devices.is_none_or(|(fm1, smk37)| match firmware.device {
                FirmwareDevice::Fm1 => fm1,
                FirmwareDevice::Smk37 => smk37,
            });
            if !shown {
                return None;
            }
            match &firmware.source {
                FirmwareSource::Builtin if firmware.upgradable => {
                    // The flag was set by the fuzzy merge; resolve the offer (and
                    // the covering download) with the same predicate — a variant
                    // spelling never exact-matches.
                    let offer = hub.iter().find(|o| same_firmware(firmware, o))?;
                    let done = view.iter().any(|other| {
                        same_firmware(firmware, other)
                            && matches!(other.source, FirmwareSource::Hub { blob: Some(_), .. })
                            && other.status == Status::Available
                            && !version_newer(&offer.version, &other.version)
                    });
                    (!done).then(|| firmware.name.clone())
                }
                FirmwareSource::Hub { blob: None, .. } => Some(firmware.name.clone()),
                FirmwareSource::Hub { .. } if firmware.status == Status::Missing => {
                    Some(firmware.name.clone())
                }
                // An installed hub package the listing moved ahead of: the upgrade
                // lands under the same name and replaces the entry, so a satisfied
                // upgrade (equal versions) never re-arms this arm.
                FirmwareSource::Hub { blob: Some(_), .. } if firmware.upgradable => {
                    Some(firmware.name.clone())
                }
                _ => None,
            }
        })
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
            format!("{}?{day}", hub::CATALOG),
            format!("{}?{day}", editor::PAGE),
            format!("{}?{day}", mutant::CATALOG),
            format!("{}?{day}", reverbose::PAGE),
            format!("{}?{day}", reverbose::SMK37_PAGE),
        );
    }

    /// The workhorse, with the URLs injectable so tests stay offline (a port that's
    /// closed is a deterministic failure). All sources are fetched at the same
    /// time. The hub decides the outcome: its failure keeps the old listing, while
    /// an additive source's failure only costs its entries (and shows in the
    /// status). Every fetch — cadence or forced — counts for the cadence, so the
    /// timestamp is set here, the single funnel.
    fn refresh_hub_from(
        &mut self,
        url: String,
        editor_url: String,
        mutant_url: String,
        reverbose_url: String,
        reverbose_smk_url: String,
    ) {
        *self.last_refresh.lock().unwrap() = Some(Instant::now());
        *self.hub_status.lock().unwrap() = HubStatus::Loading;
        let hub = Arc::clone(&self.hub);
        let status = Arc::clone(&self.hub_status);
        let swapped = Arc::clone(&self.hub_swapped);
        std::thread::spawn(move || {
            let (listing, editor_page, mutant_page, reverbose_page, reverbose_smk_page) =
                std::thread::scope(|scope| {
                    let editor_page = scope.spawn(|| editor::fetch_listing(&editor_url));
                    let mutant_page = scope.spawn(|| mutant::fetch_listing(&mutant_url));
                    let reverbose_page = scope.spawn(|| {
                        reverbose::fetch_listing(&reverbose_url, FirmwareDevice::Fm1)
                    });
                    let reverbose_smk_page = scope.spawn(|| {
                        reverbose::fetch_listing(&reverbose_smk_url, FirmwareDevice::Smk37)
                    });
                    let listing = hub::fetch_listing(&url);
                    (
                        listing,
                        editor_page.join(),
                        mutant_page.join(),
                        reverbose_page.join(),
                        reverbose_smk_page.join(),
                    )
                });
            // A panicked scrape thread reads as that source's failure.
            let editor_page =
                editor_page.unwrap_or_else(|_| Err("editor page thread panicked".to_string()));
            let mutant_page =
                mutant_page.unwrap_or_else(|_| Err("mutant catalog thread panicked".to_string()));
            let reverbose_page = reverbose_page
                .unwrap_or_else(|_| Err("reverbose page thread panicked".to_string()));
            let reverbose_smk_page = reverbose_smk_page
                .unwrap_or_else(|_| Err("reverbose SMK-37 page thread panicked".to_string()));
            let next = match listing {
                Err(e) => HubStatus::Failed(e),
                Ok(listing) => {
                    // The additive sources fold in one after another; the first
                    // failure is the one the status reports.
                    let mut outcome = HubStatus::Idle;
                    let mut listing = listing;
                    for extra in [editor_page, mutant_page, reverbose_page, reverbose_smk_page] {
                        match extra {
                            Ok(extra) => listing = merge_listings(listing, extra),
                            Err(e) if matches!(outcome, HubStatus::Idle) => {
                                outcome = HubStatus::Failed(e)
                            }
                            Err(_) => {}
                        }
                    }
                    *hub.lock().unwrap() = merge_listings(listing, index_listing());
                    swapped.store(true, Ordering::Relaxed);
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
        self.download_failed.remove(name);
        if self.downloads.contains_key(name) {
            return;
        }
        let entry = {
            let hub = self.hub.lock().unwrap();
            hub.iter().find(|f| f.name == name).cloned().or_else(|| {
                // The caller passes a view row's name; a stored row whose spelling
                // is a variant of the hub's (the fuzzy merge flags it upgradable)
                // resolves to its offer by the same predicate. The download still
                // keys on the row's name, so the row shows the progress.
                let row = self.view.iter().find(|f| f.name == name)?;
                hub.iter().find(|offer| same_firmware(row, offer)).cloned()
            })
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
        self.view_dirty = true;
        let store = self.store.clone();
        std::thread::spawn(move || {
            let result = fetch_package(&url, &download.percent)
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
    pub fn import_file(
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
            tags: vec!["source:file".to_string()],
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::File { path: path.to_string() },
        });
        self.view_dirty = true;
        self.persist();
    }

    /// Forget an entry. Builtins stay. Files drop only the entry (the file stays on
    /// disk); hub entries drop the entry and the downloaded blob.
    pub fn remove(&mut self, name: &str) {
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
            self.view_dirty = true;
            self.persist();
        }
    }

    /// Fold finished downloads into the stored list. True when any download left the
    /// in-flight set: a success folds the entry in and persists; a failure frees the
    /// row back to a hub offer and marks the name so [`Library::next_download`]
    /// skips it — the view rebuilds either way.
    fn collect_downloads(&mut self) -> bool {
        let finished: Vec<String> = self
            .downloads
            .iter()
            .filter(|(_, d)| d.outcome.lock().unwrap().is_some())
            .map(|(name, _)| name.clone())
            .collect();
        if finished.is_empty() {
            return false;
        }
        let mut changed = false;
        for name in finished {
            let download = self.downloads.remove(&name).unwrap();
            let outcome = download.outcome.lock().unwrap().take();
            match outcome {
                Some(Ok(entry)) => {
                    // Replace by firmware, not by exact name: the row that armed
                    // this download can be a variant spelling of the hub's name.
                    self.stored.retain(|f| !same_firmware(f, &entry));
                    self.stored.push(entry);
                    changed = true;
                }
                Some(Err(_)) => {
                    self.download_failed.insert(name);
                }
                None => {}
            }
        }
        if changed {
            self.persist();
        }
        true
    }

    fn persist(&mut self) {
        self.store.store_object(LIBRARY_OBJECT, &self.stored).ok();
        self.generation = self.generation.wrapping_add(1);
    }

    /// The installed list's change counter (see the field).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The returned view's change counter (see the field): a cache key for UI that
    /// derives rows from it.
    pub fn view_generation(&self) -> u64 {
        self.view_generation
    }
}

/// An entry's presence status. Downloading beats presence; files and downloaded hub
/// blobs are statted (never read in full); builtins are always here; hub-only offers
/// (no blob yet) are Available unless downloading. A blob that's a web page counts
/// as Missing so the row offers the download again.
fn entry_status(
    firmware: &Firmware,
    downloads: &HashMap<String, Arc<Download>>,
    store: &Store,
) -> Status {
    match &firmware.source {
        FirmwareSource::Builtin => Status::Available,
        _ if downloads.contains_key(&firmware.name) => Status::Downloading,
        FirmwareSource::File { path } => {
            if std::path::Path::new(path).is_file() {
                Status::Available
            } else {
                Status::Missing
            }
        }
        FirmwareSource::Hub { path, blob: Some(_) } => {
            let usable = store
                .blob_head(&blob_key(path), 512)
                .map(|head| !looks_like_web_page(&head))
                .unwrap_or(false);
            if usable {
                Status::Available
            } else {
                Status::Missing
            }
        }
        FirmwareSource::Hub { blob: None, .. } => Status::Available,
    }
}

/// The binary's listing-only entries as catalog offers: metadata and the recorded
/// download URL, no bytes. They seed the hub at load and merge into every online
/// refresh, so a package that may not be redistributed still appears (and downloads
/// on request) without being baked into the app.
fn index_listing() -> Vec<Firmware> {
    BUNDLED_INDEX.iter().map(index_entry).collect()
}

fn index_entry(b: &crate::builtin::BuiltinFirmware) -> Firmware {
    /// An empty string in the table means the link doesn't exist.
    fn link(text: &str) -> Option<String> {
        (!text.is_empty()).then(|| text.to_string())
    }
    let mut tags: Vec<String> = b
        .tags
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();
    // The origin tag names the listing the offer came from: the binary's index.
    tags.push("source:index".to_string());
    Firmware {
        name: b.name.to_string(),
        byline: b.byline.to_string(),
        version: b.version.to_string(),
        description: b.description.to_string(),
        tags,
        device: crate::sources::parse_device(b.device),
        website: link(b.website),
        source_url: link(b.source),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Hub {
            path: b.url.to_string(),
            blob: None,
        },
    }
}

fn builtin_entry(b: &crate::builtin::BuiltinFirmware) -> Firmware {
    /// An empty string in the table means the link doesn't exist.
    fn link(text: &str) -> Option<String> {
        (!text.is_empty()).then(|| text.to_string())
    }
    let mut tags: Vec<String> = b.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
    // The origin tag names the listing the entry's package came from; a merge
    // swaps it when a newer listing supersedes the download.
    tags.push("source:builtin".to_string());
    Firmware {
        name: b.name.to_string(),
        byline: b.byline.to_string(),
        version: b.version.to_string(),
        description: b.description.to_string(),
        tags,
        device: crate::sources::parse_device(b.device),
        website: link(b.website),
        source_url: link(b.source),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Builtin,
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
        format!("{}/{hub_path}", hub::BASE)
    }
}

/// A name's identity key: lowercase alphanumerics only. Sources spell the same
/// firmware "PurpleMonkey", "Purple Monkey" and "PurpleMonkey FM-1"; case,
/// spaces and punctuation are spelling, not identity.
fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Device tokens listings tack onto firmware names, as `name_key` suffixes,
/// longest first ("SMK-37 Pro" before "SMK37", so no "pro" stump remains).
const DEVICE_SUFFIXES: [&str; 3] = ["smk37pro", "smk37", "fm1"];

/// `key` minus one trailing device token ("purplemonkeyfm1" -> "purplemonkey").
fn without_device(key: &str) -> &str {
    for suffix in DEVICE_SUFFIXES {
        if let Some(base) = key.strip_suffix(suffix) {
            if !base.is_empty() {
                return base;
            }
        }
    }
    key
}

/// Whether two entries name the same firmware. Equal name keys always match — but
/// only for the same device: most projects ship an FM-1 and an SMK-37 build under
/// one name, and merging those would lose one build and risk handing the other's
/// row the wrong package. Keys differing only by a trailing device token match only
/// when the authors agree — "OPX Solo" is not "OPX", and two projects can share a
/// stem.
fn same_firmware(a: &Firmware, b: &Firmware) -> bool {
    if a.device != b.device {
        return false;
    }
    let (ka, kb) = (name_key(&a.name), name_key(&b.name));
    if ka == kb {
        return true;
    }
    !a.byline.trim().is_empty()
        && a.byline.trim().eq_ignore_ascii_case(b.byline.trim())
        && without_device(&ka) == without_device(&kb)
}

/// Fold an additive listing into the hub's. The hub is the source of truth: its
/// entries keep their name, author and description. A name both sources carry
/// (per [`same_firmware`]) gets the union of their non-origin tags,
/// website/source links fill gaps (the newer side's win), and the version and
/// download URL of whichever source is newer — a superseding source's `source:`
/// origin tag replaces the entry's. Names only the other source carries append at
/// the end.
fn merge_listings(mut hub: Vec<Firmware>, editor: Vec<Firmware>) -> Vec<Firmware> {
    let mut extra = Vec::new();
    for offer in editor {
        let mut matched = false;
        for entry in hub.iter_mut() {
            if !same_firmware(entry, &offer) {
                continue;
            }
            matched = true;
            if version_newer(&offer.version, &entry.version) {
                entry.version = offer.version.clone();
                entry.source = offer.source.clone();
                // The newer side's links describe the version the URL now fetches.
                if offer.website.is_some() {
                    entry.website = offer.website.clone();
                }
                if offer.source_url.is_some() {
                    entry.source_url = offer.source_url.clone();
                }
                // The download now comes from the offer's listing: its origin tag
                // replaces the entry's.
                entry.tags.retain(|tag| !tag.starts_with("source:"));
                entry
                    .tags
                    .extend(offer.tags.iter().filter(|tag| tag.starts_with("source:")).cloned());
            }
            for tag in &offer.tags {
                // Origin tags never union in: the entry keeps its own listing's tag
                // unless the offer superseded it above.
                if tag.starts_with("source:") {
                    continue;
                }
                if !entry.tags.iter().any(|have| have.eq_ignore_ascii_case(tag)) {
                    entry.tags.push(tag.clone());
                }
            }
            // Either way, links the entry lacks fill in from the offer.
            if entry.website.is_none() {
                entry.website = offer.website.clone();
            }
            if entry.source_url.is_none() {
                entry.source_url = offer.source_url.clone();
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
            tags: vec!["kind".to_string(), "source:hub".to_string()],
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
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
    fn installed_hub_entry_is_upgradable() {
        let (store, dir) = temp_store("hub-upgrade");
        let mut library = Library::load(&store);
        // Installed at 1.0 (blob stored); the listing then moves to 2.0.
        let key = blob_key("fw/pkg.fwsc");
        store.store_blob(&key, b"package").unwrap();
        let mut installed = hub_offer("pkg", "1.0", "pkg.fwsc");
        installed.source = FirmwareSource::Hub {
            path: "fw/pkg.fwsc".to_string(),
            blob: Some(key),
        };
        library.stored.push(installed);
        *library.hub.lock().unwrap() = vec![hub_offer("pkg", "2.0", "pkg-2.fwsc")];
        library.view_dirty = true;

        let view = library.list();
        let entry = view.iter().find(|f| f.name == "pkg").unwrap();
        assert!(entry.upgradable, "an installed hub package flags upgradable");
        // The offer hides behind the install: one row, and the sweep offers it.
        assert_eq!(view.iter().filter(|f| f.name == "pkg").count(), 1);
        assert_eq!(library.next_download().as_deref(), Some("pkg"));

        // Caught up: the same version neither flags nor sweeps.
        *library.hub.lock().unwrap() = vec![hub_offer("pkg", "1.0", "pkg.fwsc")];
        library.view_dirty = true;
        let view = library.list();
        assert!(!view.iter().find(|f| f.name == "pkg").unwrap().upgradable);
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn download_all_follows_the_device_filter() {
        let (store, dir) = temp_store("device-filter");
        let mut library = Library::load(&store);
        let mut smk = hub_offer("smk-thing", "1.0", "smk.fwsc");
        smk.device = FirmwareDevice::Smk37;
        *library.hub.lock().unwrap() = vec![smk, hub_offer("fm1-thing", "1.0", "fm1.fwsc")];
        library.view_dirty = true;

        library.set_device_filter(Some((true, false)));
        assert_eq!(library.next_download().as_deref(), Some("fm1-thing"));
        library.set_device_filter(Some((true, true)));
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        library.set_device_filter(Some((false, true)));
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        // None means no UI filter: everything is offered.
        library.set_device_filter(None);
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn next_download_walks_and_finishes() {
        let (store, dir) = temp_store("next");
        let builtin = BUILTINS.first().expect("no builtins");
        let mut library = Library::load(&store);
        // The load seeds the binary's index-only offers; this test owns the listing.
        library.hub.lock().unwrap().clear();

        // No hub listing: nothing to fetch.
        assert_eq!(library.next_download(), None);

        // An upgradable builtin comes before a hub-only offer (list order: builtins
        // first); the upgrade downloads under the builtin's name.
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        library.view_dirty = true;
        assert_eq!(library.next_download().as_deref(), Some(builtin.name));

        // One at a time: an in-flight download parks the pump.
        library.downloads.insert(
            "hub-only".to_string(),
            Arc::new(Download {
                percent: AtomicU32::new(0),
                outcome: Mutex::new(None),
            }),
        );
        assert_eq!(library.next_download(), None);
        library.downloads.clear();

        // A failed name is skipped, not retried forever.
        library.download_failed.insert(builtin.name.to_string());
        assert_eq!(library.next_download().as_deref(), Some("hub-only"));

        // Once the hub's newer package sits downloaded under the builtin's name,
        // the upgrade is satisfied — only the offer remains.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        let key = blob_key("fw/newer.fwsc");
        store.store_blob(&key, b"package").unwrap();
        let mut done = hub_offer(builtin.name, "999.0", "newer.fwsc");
        done.source = FirmwareSource::Hub {
            path: "fw/newer.fwsc".to_string(),
            blob: Some(key),
        };
        library.stored.push(done);
        library.view_dirty = true;
        assert_eq!(library.next_download().as_deref(), Some("hub-only"));

        // Everything at hand: nothing left.
        library.download_failed.insert("hub-only".to_string());
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Download All is event-driven: a sweep runs on toggle-on, on a hub swap, and
    /// when a download finishes — never on a poll. A failed item is marked and the
    /// walk moves past it; with nothing left the pump stays parked.
    #[test]
    fn download_all_sweeps_on_events() {
        let (store, dir) = temp_store("all");
        let mut library = Library::load(&store);
        // The load seeds the binary's index-only offers; this test owns the listing.
        library.hub.lock().unwrap().clear();
        let offer = |name: &str| Firmware {
            source: FirmwareSource::Hub {
                // A closed port fails fast and deterministically, offline.
                path: format!("http://127.0.0.1:1/fw/{name}.fwsc"),
                blob: None,
            },
            ..hub_offer(name, "1.0", "ignored.fwsc")
        };

        // Toggle-on with an empty listing sweeps without finding anything, and a
        // pump with nothing moving is a no-op.
        library.set_download_all(true);
        assert!(library.downloads.is_empty());
        library.pump_downloads();
        assert!(library.downloads.is_empty());

        // A hub swap is a trigger: two closed-port offers, both doomed to fail fast.
        *library.hub.lock().unwrap() = vec![offer("fail-one"), offer("fail-two")];
        library.hub_swapped.store(true, Ordering::Relaxed);
        library.pump_downloads();
        assert!(
            library.downloads.contains_key("fail-one"),
            "the swap should kick the first download"
        );

        // A finished download kicks the next step: fail-one errors out, the pump
        // marks it and the walk moves down the list to fail-two.
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            library.pump_downloads();
            if library.downloads.contains_key("fail-two") {
                break;
            }
        }
        assert!(library.download_failed.contains("fail-one"));
        assert!(
            library.downloads.contains_key("fail-two"),
            "the walk should move past the failure"
        );

        // fail-two fails too; the listing is exhausted and the pump parks.
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            library.pump_downloads();
            if library.downloads.is_empty() {
                break;
            }
        }
        assert!(library.download_failed.contains("fail-two"));
        assert!(library.downloads.is_empty());
        library.pump_downloads();
        assert!(library.downloads.is_empty(), "nothing left: no retry loop");

        // A fresh toggle-on clears the marks and retries once.
        library.set_download_all(false);
        library.set_download_all(true);
        assert!(library.downloads.contains_key("fail-one"));
        library.set_download_all(false);
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
    fn html_downloads_fail_without_storing_a_blob() {
        let (store, dir) = temp_store("html");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            // Two requests: the .fwsc URL's 404-style page, then the install page.
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept() else { break };
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf);
                let body = b"<!DOCTYPE html><html><body>page</body></html>";
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).ok();
                socket.write_all(body).ok();
            }
        });
        let mut library = Library::load(&store);
        let offer = |name: &str, url: String| Firmware {
            source: FirmwareSource::Hub {
                path: url,
                blob: None,
            },
            ..hub_offer(name, "1.0", "ignored.fwsc")
        };
        let base = format!("http://127.0.0.1:{port}");
        // A package-looking URL that serves HTML, and an install page naming no
        // package: both fail loudly instead of storing a page that reads Missing
        // forever.
        *library.hub.lock().unwrap() = vec![
            offer("error-page", format!("{base}/dl/x.fwsc")),
            offer("install-page", format!("{base}/install/")),
        ];
        for name in ["error-page", "install-page"] {
            library.start_download(name);
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(50));
                if !library.downloads.contains_key(name) {
                    break;
                }
                library.collect_downloads();
            }
            library.collect_downloads();
            assert!(
                library.download_failed.contains(name),
                "{name} must be marked failed"
            );
        }
        assert!(store.list_blobs(None).unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn merge_combines_tags_and_prefers_the_newer_side() {
        let hub = vec![
            hub_offer("zp12", "2.5", "zp12-2.5.fwsc"),
            hub_offer("Rainbow Mode", "1.1.5-rb", "rainbow.fwsc"),
        ];
        // The editor has a newer zp12: its version and URL win, its tags join in,
        // and its origin tag replaces the hub's.
        let mut newer = hub_offer("zp12", "3.0", "ignored.fwsc");
        newer.tags = vec!["kind".to_string(), "drums".to_string(), "source:fm1-editor".to_string()];
        newer.byline = "editor author".to_string();
        newer.website = Some("https://zp12.example.com".to_string());
        newer.source_url = Some("https://github.com/x/zp12".to_string());
        newer.source = FirmwareSource::Hub {
            path: "https://example.com/zp12-3.0.fwsc".to_string(),
            blob: None,
        };
        // Same Rainbow Mode, spelled differently and older: tags merge, the hub's
        // version, URL and origin tag stay; the offer's links fill the gaps.
        let mut older = hub_offer("rainbow mode", "1.0", "ignored.fwsc");
        older.tags = vec!["ambient".to_string(), "source:fm1-editor".to_string()];
        older.website = Some("https://rainbow.example.com".to_string());
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
        assert_eq!(zp12.tags, vec!["kind", "source:fm1-editor", "drums"]);
        assert_eq!(zp12.website.as_deref(), Some("https://zp12.example.com"));
        assert_eq!(zp12.source_url.as_deref(), Some("https://github.com/x/zp12"));
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
        assert_eq!(rainbow.tags, vec!["kind", "source:hub", "ambient"]);
        assert_eq!(rainbow.website.as_deref(), Some("https://rainbow.example.com"));
        assert_eq!(rainbow.source_url, None);
        match &rainbow.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/rainbow.fwsc"),
            _ => panic!("hub source expected"),
        }
        assert_eq!(merged[2].name, "editor-only");
    }

    #[test]
    fn merge_deduplicates_name_spelling_variants() {
        // The live triplicate: hub "PurpleMonkey", fm1-editor "PurpleMonkey FM-1",
        // Reverbose "Purple Monkey", all by Quixotic7. Editor first, then
        // Reverbose — the refresh's fold order.
        let hub = vec![hub_offer("PurpleMonkey", "0.11", "purplemonkey.fwsc")];
        let mut editor = hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc");
        editor.tags = vec!["games".to_string(), "source:fm1-editor".to_string()];
        let mut reverbose = hub_offer("Purple Monkey", "", "ignored.fwsc");
        reverbose.tags = vec!["source:reverbose".to_string()];
        let merged = merge_listings(merge_listings(hub, vec![editor]), vec![reverbose]);
        assert_eq!(merged.len(), 1);
        let entry = &merged[0];
        // The hub's name, version, download and origin tag survive; the equal
        // version and the empty one never supersede.
        assert_eq!(entry.name, "PurpleMonkey");
        assert_eq!(entry.version, "0.11");
        assert_eq!(entry.tags, vec!["kind", "source:hub", "games"]);
        match &entry.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/purplemonkey.fwsc"),
            _ => panic!("hub source expected"),
        }
    }

    #[test]
    fn merge_name_variants_respect_distinct_firmwares() {
        let hub = vec![
            hub_offer("OPX", "0.2.0", "opx.fwsc"),
            hub_offer("X0X", "1.1.0", "x0x.fwsc"),
            hub_offer("Tuna", "1.0", "tuna.fwsc"),
        ];
        // "OPX Solo" grows the name by a word, "X0X-RMX" by a suffix: distinct
        // firmwares by the same author, and neither is a device token.
        let solo = hub_offer("OPX Solo", "0.2.0", "ignored.fwsc");
        let rmx = hub_offer("X0X-RMX", "1.1.0", "ignored.fwsc");
        // A device-suffix spelling from a different author is someone else's
        // firmware, not a variant.
        let mut other = hub_offer("Tuna FM-1", "1.0", "ignored.fwsc");
        other.byline = "another author".to_string();
        let merged = merge_listings(hub, vec![solo, rmx, other]);
        assert_eq!(merged.len(), 6);
    }

    #[test]
    fn same_name_on_two_devices_stays_two_rows() {
        // OberVave ships an FM-1 and an SMK-37 package under one name; merging them
        // would lose the SMK-37 build (and a newer one could hand the FM-1 row the
        // SMK package).
        let hub = vec![hub_offer("OberVave", "3.3.2", "obervave.fwsc")];
        let mut smk = hub_offer("OberVave", "9.0", "obervave-9-smk37.fwsc");
        smk.device = FirmwareDevice::Smk37;
        smk.tags = vec!["synth".to_string(), "source:mutant".to_string()];
        let merged = merge_listings(hub, vec![smk]);
        assert_eq!(merged.len(), 2);
        let fm1 = merged
            .iter()
            .find(|f| f.device == FirmwareDevice::Fm1)
            .unwrap();
        assert_eq!(fm1.version, "3.3.2");
        match &fm1.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/obervave.fwsc"),
            _ => panic!("hub source expected"),
        }
        let smk = merged
            .iter()
            .find(|f| f.device == FirmwareDevice::Smk37)
            .unwrap();
        assert_eq!(smk.version, "9.0");
    }

    #[test]
    fn installed_variant_spelling_shadows_the_offer() {
        let (store, dir) = temp_store("shadow");
        let mut library = Library::load(&store);
        // Downloaded when the editor listing called it "PurpleMonkey FM-1"; the
        // stored entry keeps the byline it was scraped with.
        let installed = hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc");
        library.stored.push(installed);
        // The merged listing now carries the hub's spelling.
        *library.hub.lock().unwrap() = vec![hub_offer("PurpleMonkey", "0.11", "pm.fwsc")];
        let names: Vec<&str> = library
            .list()
            .iter()
            .map(|f| f.name.as_str())
            .filter(|name| name.to_lowercase().contains("purple"))
            .collect();
        assert_eq!(names, vec!["PurpleMonkey FM-1"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Entries stored before origin tags existed get one from their source at load;
    /// an absolute Hub path means a pre-tag fm1-editor download (the hub's own
    /// entries carry relative paths).
    #[test]
    fn legacy_entries_gain_origin_tags() {
        let (store, dir) = temp_store("origin-tags");
        let mut hub_row = hub_offer("Old Hub Row", "1.0", "x.fwsc");
        hub_row.tags.retain(|tag| !tag.starts_with("source:"));
        hub_row.source = FirmwareSource::Hub {
            path: "fw/x.fwsc".to_string(),
            blob: Some("hub/x.fwsc".to_string()),
        };
        let mut editor_row = hub_offer("Old Editor Row", "1.0", "y.fwsc");
        editor_row.tags.retain(|tag| !tag.starts_with("source:"));
        editor_row.source = FirmwareSource::Hub {
            path: "https://fm1-editor.com/firmware/y".to_string(),
            blob: Some("ext/y".to_string()),
        };
        store
            .store_object(LIBRARY_OBJECT, &vec![hub_row, editor_row])
            .unwrap();
        let library = Library::load(&store);
        let tag_of = |name: &str| {
            library
                .stored
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .tags
                .iter()
                .find(|tag| tag.starts_with("source:"))
                .cloned()
        };
        assert_eq!(tag_of("Old Hub Row").as_deref(), Some("source:hub"));
        assert_eq!(tag_of("Old Editor Row").as_deref(), Some("source:fm1-editor"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// The variant-spelled row must still download: the version bump flags it
    /// upgradable, Download All targets it by its own name, `start_download`
    /// resolves the hub's offer (an exact-name lookup would miss), and completion
    /// replaces the variant row with the hub-spelled one.
    #[test]
    fn variant_spelling_upgrades_through_the_offer() {
        let (store, dir) = temp_store("variant-upgrade");
        let mut library = Library::load(&store);
        library.stored.push(hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc"));
        // A dead URL: start_download's fetch thread fails fast and offline (its
        // error lands in an Arc this test replaces before collecting).
        let mut offer = hub_offer("PurpleMonkey", "0.12", "pm.fwsc");
        offer.source = FirmwareSource::Hub {
            path: "http://127.0.0.1:1/pm.fwsc".to_string(),
            blob: None,
        };
        *library.hub.lock().unwrap() = vec![offer];

        let row = library
            .list()
            .iter()
            .find(|f| f.name == "PurpleMonkey FM-1")
            .unwrap()
            .clone();
        assert!(row.upgradable, "hub's 0.12 moves ahead of the stored 0.11");
        assert_eq!(library.next_download().as_deref(), Some("PurpleMonkey FM-1"));

        // The row's own name arms the download; the offer resolves fuzzily.
        library.start_download("PurpleMonkey FM-1");
        assert!(library.downloads.contains_key("PurpleMonkey FM-1"));
        assert_eq!(entry_status(&row, &library.downloads, &store), Status::Downloading);

        // Completion folds the package in under the hub's name, replacing the
        // variant row (the blob is stored for real so the status stats Available).
        let path = "http://127.0.0.1:1/pm.fwsc";
        store.store_blob(&blob_key(path), b"\x00\x01 firmware").unwrap();
        library.downloads.insert(
            "PurpleMonkey FM-1".to_string(),
            Arc::new(Download {
                percent: AtomicU32::new(100),
                outcome: Mutex::new(Some(Ok(Firmware {
                    status: Status::Available,
                    source: FirmwareSource::Hub {
                        path: path.to_string(),
                        blob: Some(blob_key(path)),
                    },
                    ..hub_offer("PurpleMonkey", "0.12", "pm.fwsc")
                }))),
            }),
        );
        assert!(library.collect_downloads());
        let names: Vec<&str> = library
            .list()
            .iter()
            .map(|f| f.name.as_str())
            .filter(|name| name.to_lowercase().contains("purple"))
            .collect();
        assert_eq!(names, vec!["PurpleMonkey"], "the variant row is replaced");
        // Versions equal: nothing re-arms, Download All parks.
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
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
            "http://127.0.0.1:1/mutant.json".to_string(),
            "http://127.0.0.1:1/h/m-vave/fm-1".to_string(),
            "http://127.0.0.1:1/h/m-vave/smk-37".to_string(),
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
            "http://127.0.0.1:1/mutant.json".to_string(),
            "http://127.0.0.1:1/h/m-vave/fm-1".to_string(),
            "http://127.0.0.1:1/h/m-vave/smk-37".to_string(),
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

    /// Listing-only entries ship metadata and a URL, never bytes; the baked table
    /// leaves them out and the hub carries them as downloadable offers from load.
    #[test]
    fn index_only_entries_are_offers_not_builtins() {
        let (store, dir) = temp_store("indexonly");
        let library = Library::load(&store);
        let hub = library.hub.lock().unwrap();
        assert_eq!(
            hub.len(),
            BUNDLED_INDEX.len(),
            "the seeded hub is exactly the index-only listing"
        );
        for b in BUNDLED_INDEX {
            assert!(b.bytes.is_empty(), "{} must not be embedded", b.name);
            assert!(!BUILTINS.iter().any(|baked| baked.name == b.name));
            let offer = hub
                .iter()
                .find(|firmware| firmware.name == b.name)
                .expect("index-only entry in the seeded hub");
            assert!(matches!(
                offer.source,
                FirmwareSource::Hub { ref path, blob: None } if path == b.url
            ));
            assert!(offer.tags.iter().any(|tag| tag == "source:index"));
        }
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
