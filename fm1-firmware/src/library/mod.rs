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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::builtin::BUILTINS;
use crate::download_heuristics::fetch_package;
use crate::sources::{editor, hub, mutant, reverbose};
use fm1_base::store::Store;
use crate::filter::DeviceFilter;

mod merge;
mod model;
mod persist;
mod util;
#[cfg(test)]
mod tests;

pub use model::*;
use merge::*;
use persist::*;
use util::*;

/// The store object holding every non-builtin entry.
const LIBRARY_OBJECT: &str = "library";

/// How often `list()` re-stats files/blobs for the Missing status. External deletes
/// surface within this; everything app-driven rebuilds immediately.
const STAT_CADENCE: Duration = Duration::from_millis(500);

pub struct Library {
    store: Store,
    builtins: Vec<Firmware>,
    /// The persisted non-builtin entries (files and downloaded hub packages).
    stored: Vec<Firmware>,
    /// Entries removed since the last write; `persist` keeps them out of the disk
    /// copy while merging another app's additions.
    removed: Vec<Firmware>,
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
    devices: Option<DeviceFilter>,
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
            removed: Vec::new(),
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
    pub fn set_device_filter(&mut self, devices: Option<DeviceFilter>) {
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
            let shown = devices.is_none_or(|filter| filter.shows(firmware));
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
        let mut gone = Vec::new();
        self.stored.retain(|f| {
            if f.name != name {
                return true;
            }
            if let FirmwareSource::Hub { path, .. } = &f.source {
                self.store.delete_blob(&blob_key(path)).ok();
            }
            gone.push(f.clone());
            false
        });
        if self.stored.len() != before {
            self.removed.extend(gone);
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
        // The GUI and the TUI share this store. Re-read before writing so an entry
        // the other app added since this one loaded is not erased. Our copy wins for
        // entries we both know.
        let merged = match self.store.fetch_object::<Vec<Firmware>>(LIBRARY_OBJECT) {
            Ok(mut disk) => {
                disk.retain(|f| !self.removed.iter().any(|gone| same_firmware(gone, f)));
                for entry in self.stored.drain(..) {
                    disk.retain(|f| !same_firmware(f, &entry));
                    disk.push(entry);
                }
                disk
            }
            Err(_) => std::mem::take(&mut self.stored),
        };
        self.stored = merged;
        self.removed.clear();
        if let Err(e) = self.store.store_object(LIBRARY_OBJECT, &self.stored) {
            eprintln!("fm1-performer: {e}");
        }
        self.view_dirty = true;
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
