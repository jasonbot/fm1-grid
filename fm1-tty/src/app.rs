//! fm1-tty's state: the shared library/hardware/clock pieces from `fm1-performer` plus the
//! TUI's own selection, focus, filter, popup and MIDI-graph state. Drawing lives in
//! `ui.rs`, the event loop in `main.rs`.

use fm1_performer::clock::Clock;
use fm1_performer::firmware_picker::{pick_of, FirmwarePick};
use fm1_performer::hardware::{DeviceRow, FlashState, Hardware};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, HubStatus, Library, Status};
use fm1_performer::midi_keys::MidiKeys;
use fm1_performer::routing::{Endpoint, Port, Routing};
use fm1_performer::store::Store;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a transient footer message stays up.
const STATUS_TTL: Duration = Duration::from_secs(5);

/// Which section has the computer keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Library,
    Graph,
    /// The library's filter field; printable keys edit the needle.
    Filter,
}

/// A modal over the main screen; while one is up it gets every key.
#[derive(Clone)]
pub enum Popup {
    /// The MIDI-keyboard selector: the highlighted row (0 is "no keyboard").
    MidiPorts(usize),
    /// The flash target picker: (selected row, (target name, is rescue target) rows,
    /// what to flash).
    FlashTarget(usize, Vec<(String, bool)>, FirmwarePick),
    /// The library's device filter: the highlighted row (0 FM-1, 1 SMK-37).
    DeviceFilter(usize),
    /// The MIDI graph's connect menu: the source and the highlighted sink candidate.
    GraphSinks(Port, usize),
}

/// Which device families the library lists, mirroring the GUI's device dropdown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DeviceFilter {
    pub fm1: bool,
    pub smk37: bool,
}

impl Default for DeviceFilter {
    fn default() -> Self {
        // FM-1 only, like the GUI.
        Self { fm1: true, smk37: false }
    }
}

impl DeviceFilter {
    fn shows(self, device: FirmwareDevice) -> bool {
        match device {
            FirmwareDevice::Fm1 => self.fm1,
            FirmwareDevice::Smk37 => self.smk37,
        }
    }
}

/// A row of the MIDI-graph panel, in draw order. Sources are always shown, linked or
/// not; each source's links follow, then its always-there connect action.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GraphRow {
    Source(Port),
    Link(Port, Port),
    Connect(Port),
}

/// Mouse hit regions, refilled by every draw. Plain data; `ui.rs` writes, `main.rs`
/// reads.
#[derive(Default)]
pub struct Regions {
    /// The library list's inner area; each item is two lines, so the clicked item is
    /// `lib_offset + (y − top) / 2`.
    pub lib_rows: Rect,
    pub lib_offset: usize,
    /// The filter field and the device-filter field above the list.
    pub lib_filter: Rect,
    pub lib_devices: Rect,
    /// The "MIDI in" field: opens the keyboard selector.
    pub midi_field: Rect,
    /// The graph rows: (rect, row index into `TuiApp::graph_rows`).
    pub graph_rows: Vec<(Rect, usize)>,
    /// The graph's scrolling list area (wheel events).
    pub graph_list: Rect,
    /// The clock switch: starts/stops the metronome.
    pub metro_toggle: Rect,
    pub bpm_minus: Rect,
    pub bpm_plus: Rect,
    /// The open popup's rows: (rect, item index).
    pub popup_items: Vec<(Rect, usize)>,
}

pub struct TuiApp {
    pub clock: Arc<Clock>,
    routing: Routing,
    pub hardware: Hardware,
    midi_keys: MidiKeys,
    store: Option<Store>,
    library: Option<Library>,
    /// The full library view from the last tick, in library order.
    pub firmwares: Vec<Firmware>,
    /// The rows the filter and device selector leave: indices into `firmwares`, in
    /// draw order. `sel` indexes this.
    pub view: Vec<usize>,
    /// The live FM-1 units from the last tick.
    pub devices: Vec<DeviceRow>,
    /// The selected row in `view`.
    pub sel: usize,
    pub focus: Focus,
    pub popup: Option<Popup>,
    /// The library filter needle; space-separated tokens must all match.
    pub filter: String,
    pub device_filter: DeviceFilter,
    /// The MIDI graph from the last tick: every endpoint and link.
    pub endpoints: Vec<Endpoint>,
    pub links: Vec<(Port, Port)>,
    /// The highlighted graph row (an index into `graph_rows`).
    pub graph_sel: usize,
    /// The graph list's scroll state (the widget owns the offset).
    pub graph_state: ListState,
    /// The MIDI keyboard port the user picked (the poller may still be opening it).
    want_midi: Option<String>,
    /// Graph links this app created for the picked keyboard: (system source, device)
    /// port names.
    linked: Vec<(String, String)>,
    /// The (keyboard, device list) the auto-links were last reconciled for: a manual
    /// graph edit sticks until one of those moves.
    links_key: Option<(Option<String>, Vec<String>)>,
    /// A dismissed flash board stays down until the next flash starts.
    flash_dismissed: bool,
    /// The Download All toggle: sweeps run on toggle-on, on a hub swap and when a
    /// download finishes (the library's pump); never on a poll. Not persisted — the
    /// TUI has no session record.
    pub download_all: bool,
    /// The rescue toggle: offer every system MIDI port as a flash target too, for
    /// units too bricked to answer to their name. `rescue_ports` is the last sweep.
    pub rescue: bool,
    pub rescue_ports: Vec<String>,
    /// The rescue port sweep's cadence (the hardware scanner's own poll rate).
    rescue_at: Instant,
    /// A transient footer message and when it was set.
    status: Option<(String, Instant)>,
    /// Mouse hit regions from the last draw.
    pub regions: Regions,
    /// The library list's scroll state (selection is `sel`; the widget owns offset).
    pub list_state: ListState,
    pub quit: bool,
    /// OSC bookkeeping: last tick's rows by name, for the add/remove/complete diff.
    osc_prev: HashMap<String, OscRow>,
    /// Set once the baseline is seeded — the library's startup contents aren't news.
    osc_seeded: bool,
    /// The percent last pushed to the terminal's progress bar; None = bar hidden.
    osc_percent: Option<u32>,
    /// A failed download's error bar clears this long after.
    osc_error_until: Option<Instant>,
}

/// What the OSC tracker remembers about a library row.
#[derive(Clone, Copy)]
struct OscRow {
    /// In the library proper (builtin, imported file, downloaded package) — bare
    /// hub offers are the catalog, not the library.
    installed: bool,
    /// A download was in flight.
    downloading: bool,
}

/// Whether the entry is part of the library proper: builtins, imported files and
/// downloaded hub packages. A hub offer without its blob is the catalog, not the
/// library.
fn in_library(firmware: &Firmware) -> bool {
    match &firmware.source {
        FirmwareSource::Hub { blob, .. } => blob.is_some(),
        _ => true,
    }
}

/// Whether the entry can leave the library: installed, at hand and not a builtin.
/// Mirrors the GUI's Remove button.
fn deletable(firmware: &Firmware) -> bool {
    in_library(firmware)
        && firmware.status == Status::Available
        && !matches!(firmware.source, FirmwareSource::Builtin)
}

/// The filter box's match, as in the GUI: every space-separated token of the needle
/// must appear as a case-insensitive substring of name, version, tags, byline or
/// description.
fn matches_filter(firmware: &Firmware, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    let haystack = format!(
        "{} {} {} {} {}",
        firmware.name,
        firmware.version,
        firmware.tags.join(" "),
        firmware.byline,
        firmware.description
    )
    .to_lowercase();
    needle
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

/// Flatten the graph into draw rows: every source, its links (sorted by sink label so
/// the row indices hold still), then its connect action. The cache keeps `main.rs`
/// acting on the same rows the last frame drew.
fn graph_rows(endpoints: &[Endpoint], links: &[(Port, Port)]) -> Vec<GraphRow> {
    let label = |port: &Port| -> String {
        endpoints
            .iter()
            .find(|endpoint| &endpoint.port == port)
            .map_or_else(|| "gone".to_string(), |endpoint| endpoint.label.clone())
    };
    let mut sources: Vec<&Endpoint> = endpoints.iter().filter(|endpoint| endpoint.source).collect();
    sources.sort_by_key(|endpoint| endpoint.label.clone());
    let mut rows = Vec::new();
    for source in sources {
        rows.push(GraphRow::Source(source.port.clone()));
        let mut sinks: Vec<&(Port, Port)> = links
            .iter()
            .filter(|(from, _)| from == &source.port)
            .collect();
        sinks.sort_by_key(|(_, sink)| label(sink));
        for (_, sink) in sinks {
            rows.push(GraphRow::Link(source.port.clone(), sink.clone()));
        }
        rows.push(GraphRow::Connect(source.port.clone()));
    }
    rows
}

impl TuiApp {
    /// Start the shared machinery: clock, MIDI graph, USB scanner, keyboard poller,
    /// store and library (the GUI's store directory, so downloads are shared). The
    /// hub catalog starts fetching in the background.
    pub fn new() -> Self {
        // No emulator panes here; the hardware module's USB targets carry the clock.
        let clock = Arc::new(Clock::start(1));
        let routing = Routing::start();
        let midi_keys = MidiKeys::start();
        let store = match Store::new() {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("fm1-tty: no on-disk store: {e}");
                None
            }
        };
        // The store comes first: the scanner loads its remembered probe names from it.
        let hardware = Hardware::start(clock.clone(), routing.clone(), store.clone());
        let mut library = store.as_ref().map(Library::load);
        if let Some(library) = &mut library {
            library.refresh_hub();
        }
        Self {
            clock,
            routing,
            hardware,
            midi_keys,
            store,
            library,
            firmwares: Vec::new(),
            view: Vec::new(),
            devices: Vec::new(),
            sel: 0,
            focus: Focus::Library,
            popup: None,
            filter: String::new(),
            device_filter: DeviceFilter::default(),
            endpoints: Vec::new(),
            links: Vec::new(),
            graph_sel: 0,
            graph_state: ListState::default(),
            want_midi: None,
            linked: Vec::new(),
            links_key: None,
            flash_dismissed: false,
            download_all: false,
            rescue: false,
            rescue_ports: Vec::new(),
            rescue_at: Instant::now() - Duration::from_secs(1),
            status: None,
            regions: Regions::default(),
            list_state: ListState::default().with_selected(Some(0)),
            quit: false,
            osc_prev: HashMap::new(),
            osc_seeded: false,
            osc_percent: None,
            osc_error_until: None,
        }
    }

    /// The per-frame update: fold in finished downloads, track the devices, refresh
    /// the filter view and the graph snapshot, and keep the picked keyboard's graph
    /// links matched to the units that are actually plugged in.
    pub fn tick(&mut self) {
        if let Some(library) = &mut self.library {
            library.pump_downloads();
        }
        self.firmwares = self
            .library
            .as_mut()
            .map(|library| library.list_clone())
            .unwrap_or_default();
        self.rebuild_view();
        self.devices = self.hardware.devices();
        // The rescue target list, swept at the scanner's own cadence.
        if self.rescue_at.elapsed() >= Duration::from_millis(500) {
            self.rescue_at = Instant::now();
            self.rescue_ports = self.hardware.midi_ports();
        }

        self.reconcile_links();
        self.endpoints = self.routing.endpoints();
        self.links = self.routing.links();
        let rows = self.graph_rows().len();
        self.graph_sel = if rows == 0 { 0 } else { self.graph_sel.min(rows - 1) };

        if self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > STATUS_TTL)
        {
            self.status = None;
        }
        self.osc_track();
    }

    /// Terminal-side signals for the library: OSC 9;4 progress while downloads run
    /// (cleared when they finish), and an OSC 9 desktop notification when an item
    /// joins or leaves the library — a completed download, or an add/remove made by
    /// the GUI app against this shared store. The first call only seeds the diff
    /// baseline; the startup contents aren't news.
    fn osc_track(&mut self) {
        // Progress: the downloads in flight (usually one), averaged.
        let percents: Vec<u32> = self
            .firmwares
            .iter()
            .filter(|firmware| firmware.status == Status::Downloading)
            .filter_map(|firmware| self.download_percent(&firmware.name))
            .collect();
        if percents.is_empty() {
            if self.osc_percent.take().is_some() {
                crate::osc::progress_clear();
            }
        } else {
            let percent = percents.iter().sum::<u32>() / percents.len() as u32;
            if self.osc_percent != Some(percent) {
                self.osc_percent = Some(percent);
                crate::osc::progress(percent);
            }
        }
        // A failed download's red bar hides again after a beat.
        if self
            .osc_error_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.osc_error_until = None;
            if self.osc_percent.is_none() {
                crate::osc::progress_clear();
            }
        }

        // The add/remove/complete diff against last tick's rows.
        let current: HashMap<String, OscRow> = self
            .firmwares
            .iter()
            .map(|firmware| {
                (
                    firmware.name.clone(),
                    OscRow {
                        installed: in_library(firmware),
                        downloading: firmware.status == Status::Downloading,
                    },
                )
            })
            .collect();
        let prev = std::mem::replace(&mut self.osc_prev, current);
        if !self.osc_seeded {
            self.osc_seeded = true;
            return;
        }
        for firmware in &self.firmwares {
            let installed = in_library(firmware);
            match prev.get(&firmware.name) {
                // A download that just ended (it was in flight last tick, it isn't
                // now — in-flight ticks are Downloading and must not fire). Success
                // folds the package in: installed and Available. Anything else
                // failed: back to a bare offer (the download errored) or installed
                // but Missing (the "package" was a web page).
                Some(row) if row.downloading && firmware.status != Status::Downloading => {
                    if installed && firmware.status == Status::Available {
                        crate::osc::notify(&format!("{} added to the library", firmware.name));
                    } else {
                        crate::osc::progress_error();
                        self.osc_percent = None;
                        self.osc_error_until = Some(Instant::now() + Duration::from_secs(2));
                        crate::osc::notify(&format!("{} download failed", firmware.name));
                    }
                }
                // A newly appeared installed row (the GUI app added it).
                None if installed => {
                    crate::osc::notify(&format!("{} added to the library", firmware.name));
                }
                _ => {}
            }
        }
        // A vanished installed row (the GUI app removed it). Vanished bare offers
        // are just catalog churn from a refresh.
        for (name, row) in &prev {
            if row.installed && !self.firmwares.iter().any(|f| f.name == *name) {
                crate::osc::notify(&format!("{name} removed from the library"));
            }
        }
    }

    /// The selected physical keyboard plays every connected unit: keep one graph link
    /// (system port → device) per live unit, adding and dropping as they come and go.
    /// Only links this app created are ever touched, and only when the pick or the
    /// device list moves — a manual edit in the graph pane sticks otherwise.
    fn reconcile_links(&mut self) {
        let mut devices: Vec<String> = self.devices.iter().map(|device| device.name.clone()).collect();
        devices.sort();
        let key = (self.midi_keys.selected(), devices);
        if self.links_key.as_ref() == Some(&key) {
            return;
        }
        self.links_key = Some(key);
        let live = self.routing.links();
        self.linked.retain(|(source, sink)| {
            live.contains(&(Port::System(source.clone()), Port::Device(sink.clone())))
        });
        let want: HashSet<(String, String)> = match self.midi_keys.selected() {
            Some(port) => self
                .devices
                .iter()
                .map(|device| (port.clone(), device.name.clone()))
                .collect(),
            None => HashSet::new(),
        };
        for (source, sink) in &want {
            if !self.linked.contains(&(source.clone(), sink.clone())) {
                self.routing
                    .connect(Port::System(source.clone()), Port::Device(sink.clone()));
            }
        }
        let extra: Vec<(String, String)> = self
            .linked
            .iter()
            .filter(|link| !want.contains(link))
            .cloned()
            .collect();
        for (source, sink) in extra {
            self.routing
                .disconnect(Port::System(source), Port::Device(sink));
        }
        self.linked = want.into_iter().collect();
    }

    // ---- library filter + selection --------------------------------------------

    /// Rebuild the visible rows from the full library, the filter needle and the
    /// device selector, and keep `sel` inside the result.
    fn rebuild_view(&mut self) {
        let needle = self.filter.trim();
        self.view = self
            .firmwares
            .iter()
            .enumerate()
            .filter(|(_, firmware)| {
                self.device_filter.shows(firmware.device) && matches_filter(firmware, needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.sel = match self.view.len() {
            0 => 0,
            len => self.sel.min(len - 1),
        };
    }

    /// Append a character to the filter needle (the filter field has the keyboard).
    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_view();
    }

    /// Remove the last character of the filter needle.
    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.rebuild_view();
    }

    /// Clear the filter needle.
    pub fn filter_clear(&mut self) {
        self.filter.clear();
        self.rebuild_view();
    }

    /// The rows the filter leaves, in draw order.
    pub fn visible(&self) -> impl Iterator<Item = &Firmware> {
        self.view
            .iter()
            .filter_map(|&index| self.firmwares.get(index))
    }

    /// The device selector's label, as in the GUI.
    pub fn device_filter_label(&self) -> String {
        match (self.device_filter.fm1, self.device_filter.smk37) {
            (true, true) => "FM-1 + SMK-37",
            (true, false) => "FM-1",
            (false, true) => "SMK-37",
            (false, false) => "no devices",
        }
        .to_string()
    }

    /// Toggle one row of the device selector: 0 is FM-1, 1 is SMK-37.
    pub fn toggle_device_row(&mut self, row: usize) {
        match row {
            0 => self.device_filter.fm1 = !self.device_filter.fm1,
            1 => self.device_filter.smk37 = !self.device_filter.smk37,
            _ => return,
        }
        self.rebuild_view();
    }

    /// Move the library selection (negative up).
    pub fn scroll(&mut self, delta: isize) {
        if self.view.is_empty() {
            self.sel = 0;
            return;
        }
        let last = self.view.len() - 1;
        self.sel = (self.sel.saturating_add_signed(delta)).min(last);
    }

    /// The selected firmware, when there is one.
    fn selected(&self) -> Option<&Firmware> {
        self.view
            .get(self.sel)
            .and_then(|&index| self.firmwares.get(index))
    }

    // ---- MIDI graph --------------------------------------------------------------

    /// The graph's draw rows from the last tick's endpoint/link snapshot.
    pub fn graph_rows(&self) -> Vec<GraphRow> {
        graph_rows(&self.endpoints, &self.links)
    }

    /// An endpoint's display label ("gone" if it disappeared since the snapshot).
    pub fn graph_source_label(&self, port: &Port) -> String {
        self.endpoints
            .iter()
            .find(|endpoint| &endpoint.port == port)
            .map_or_else(|| "gone".to_string(), |endpoint| endpoint.label.clone())
    }

    /// The sinks a source could still feed: every sink endpoint it has no link to.
    pub fn graph_sinks(&self, source: &Port) -> Vec<(Port, String)> {
        self.endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.sink
                    && &endpoint.port != source
                    && !self
                        .links
                        .iter()
                        .any(|(from, sink)| from == source && sink == &endpoint.port)
            })
            .map(|endpoint| (endpoint.port.clone(), endpoint.label.clone()))
            .collect()
    }

    /// Move the graph highlight (negative up).
    pub fn graph_move(&mut self, delta: isize) {
        let len = self.graph_rows().len();
        if len == 0 {
            self.graph_sel = 0;
            return;
        }
        self.graph_sel = self
            .graph_sel
            .saturating_add_signed(delta)
            .min(len - 1);
    }

    /// Do what a graph row offers: a link unroutes, a connect row opens the sink
    /// picker, a source row just selects.
    pub fn graph_activate(&mut self, index: usize) {
        self.graph_sel = index;
        match self.graph_rows().get(index).cloned() {
            Some(GraphRow::Link(source, sink)) => self.routing.disconnect(source, sink),
            Some(GraphRow::Connect(source)) => {
                self.popup = Some(Popup::GraphSinks(source, 0));
            }
            Some(GraphRow::Source(_)) | None => {}
        }
    }

    /// Connect the open graph popup's highlighted sink to its source.
    pub fn confirm_graph_sink(&mut self, row: usize) {
        let Some(Popup::GraphSinks(source, _)) = self.popup.clone() else {
            return;
        };
        if let Some((sink, _)) = self.graph_sinks(&source).get(row).cloned() {
            self.routing.connect(source, sink);
        }
        self.popup = None;
    }

    // ---- library ---------------------------------------------------------------

    /// Download the selected hub offer, or upgrade the selected entry when the
    /// hub carries a newer version (the newer package lands under the same name).
    /// A no-op with a hint otherwise.
    pub fn download_selected(&mut self) {
        let Some(firmware) = self.selected() else {
            return;
        };
        let downloadable = firmware.status == Status::Available
            && (matches!(firmware.source, FirmwareSource::Hub { ref blob, .. } if blob.is_none())
                || firmware.upgradable);
        if !downloadable {
            self.say(match firmware.status {
                Status::Downloading => "already downloading".to_string(),
                _ => format!("{} is already installed", firmware.name),
            });
            return;
        }
        let name = firmware.name.clone();
        if let Some(library) = &mut self.library {
            library.start_download(&name);
        }
    }

    /// Delete the selected library entry (the `x` key); a no-op with a hint
    /// otherwise. A hub entry's downloaded package goes with it; an imported file
    /// stays on disk (see `Library::remove`).
    pub fn delete_selected(&mut self) {
        let Some(firmware) = self.selected() else {
            return;
        };
        if !deletable(firmware) {
            self.say(format!("{} can't be deleted", firmware.name));
            return;
        }
        let name = firmware.name.clone();
        if let Some(library) = &mut self.library {
            library.remove(&name);
            self.say(format!("{name} deleted"));
        }
    }

    /// Whether the selected entry takes an upgrade, for the footer's `d` hint.
    pub fn selected_upgradable(&self) -> bool {
        self.selected().is_some_and(|firmware| firmware.upgradable)
    }

    /// Whether the selected entry can be deleted, for the footer's `x` hint.
    pub fn selected_deletable(&self) -> bool {
        self.selected().is_some_and(deletable)
    }

    /// Re-fetch the hub catalog now.
    pub fn reload_hub(&mut self) {
        if let Some(library) = &mut self.library {
            library.reload_hub();
        }
    }

    /// The Download All toggle (the `a` key). Turning it on sweeps at once and
    /// retries what failed last time; a finished download or a hub swap kicks the
    /// next step from the tick's pump.
    pub fn toggle_download_all(&mut self) {
        self.download_all = !self.download_all;
        if let Some(library) = &mut self.library {
            library.set_download_all(self.download_all);
        }
        self.say(if self.download_all {
            "downloading everything, one at a time".to_string()
        } else {
            "download all off".to_string()
        });
    }

    /// The rescue toggle (shift-R): every system MIDI port becomes a flash target,
    /// for units too bricked to answer to their name.
    pub fn toggle_rescue(&mut self) {
        self.rescue = !self.rescue;
        self.say(if self.rescue {
            "rescue on: every MIDI port is a flash target".to_string()
        } else {
            "rescue off".to_string()
        });
    }

    /// The rescue warning: no FM-1 answered, but something MIDI is out there.
    pub fn rescue_warn(&self) -> bool {
        self.devices.is_empty() && !self.rescue_ports.is_empty()
    }

    /// The percent of a download in flight, for the list's pill.
    pub fn download_percent(&self, name: &str) -> Option<u32> {
        self.library
            .as_ref()
            .and_then(|library| library.download_percent(name))
    }

    /// Where the hub refresh stands, for the library block's title.
    pub fn hub_status(&self) -> Option<HubStatus> {
        self.library.as_ref().map(|library| library.hub_status())
    }

    /// Flash the selected firmware: pick the target when several are around, flash
    /// directly when there is exactly one. Targets are the live units; with the
    /// rescue toggle on, every system MIDI port joins them, flagged `rescue`.
    pub fn flash_selected(&mut self) {
        let Some(firmware) = self.selected().cloned() else {
            return;
        };
        if firmware.status != Status::Available {
            self.say(format!("{} is not at hand", firmware.name));
            return;
        }
        let Some(pick) = pick_of(&firmware) else {
            self.say(format!("download {} first", firmware.name));
            return;
        };
        let mut targets: Vec<(String, bool)> = self
            .devices
            .iter()
            .filter(|device| device.flash.is_none())
            .map(|device| (device.name.clone(), false))
            .collect();
        if self.rescue {
            targets.extend(self.rescue_ports.iter().map(|port| (port.clone(), true)));
        }
        match targets.len() {
            0 => self.say(if self.rescue {
                "no MIDI port in sight".to_string()
            } else if self.rescue_warn() {
                "no live FM-1 on USB — R offers every MIDI port as a rescue target"
                    .to_string()
            } else {
                "no live FM-1 on USB".to_string()
            }),
            1 => {
                let (unit, rescue) = targets[0].clone();
                self.start_flash(&unit, rescue, &pick);
            }
            _ => self.popup = Some(Popup::FlashTarget(0, targets, pick)),
        }
    }

    /// Flash the package to the named target; a rescue target is a bare MIDI port no
    /// FM-1 answered from, flashed through `Hardware::rescue_flash` with failures on
    /// the pane-level board. A live unit's failures land on its row.
    pub fn start_flash(&mut self, unit: &str, rescue: bool, pick: &FirmwarePick) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                if rescue {
                    self.hardware.rescue_flash(unit, bytes);
                } else {
                    self.hardware.flash(unit, bytes);
                }
                self.flash_dismissed = false;
            }
            Err(e) if rescue => self.hardware.report_board(e),
            Err(e) => self.hardware.report(unit, e),
        }
        self.popup = None;
    }

    // ---- MIDI keyboard ---------------------------------------------------------

    /// The system MIDI sources for the selector: (port name, label).
    pub fn midi_ports(&self) -> Vec<(String, String)> {
        self.routing
            .endpoints()
            .into_iter()
            .filter_map(|endpoint| match endpoint.port {
                Port::System(name) if endpoint.source => Some((name, endpoint.label)),
                _ => None,
            })
            .collect()
    }

    /// The selector's display name for the current pick.
    pub fn midi_label(&self) -> String {
        match &self.want_midi {
            None => "none".to_string(),
            Some(name) => fm1_performer::routing::label_of(name),
        }
    }

    /// Choose the physical MIDI keyboard (None for off). Its notes are routed to
    /// every connected unit; the graph links land on the next ticks once the port is
    /// open.
    pub fn select_midi(&mut self, port: Option<String>) {
        self.want_midi = port.clone();
        self.midi_keys.select(port);
    }

    // ---- metronome ---------------------------------------------------------------

    /// Start or stop the MIDI clock.
    pub fn toggle_clock(&self) {
        self.clock.set_enabled(!self.clock.enabled());
    }

    /// Nudge the tempo (clamped by the clock).
    pub fn bump_bpm(&self, delta: i32) {
        self.clock.set_bpm(self.clock.bpm().saturating_add_signed(delta));
    }

    // ---- flash board + status ----------------------------------------------------

    /// The pane-level flash state for the modal, unless the user dismissed it.
    pub fn flash_board(&self) -> Option<FlashState> {
        if self.flash_dismissed {
            None
        } else {
            self.hardware.flash_state()
        }
    }

    /// Dismiss the flash board; a running flash continues on its own thread.
    pub fn dismiss_flash(&mut self) {
        self.flash_dismissed = true;
    }

    /// Show a transient footer message.
    pub fn say(&mut self, message: String) {
        self.status = Some((message, Instant::now()));
    }

    /// The current transient message, if fresh.
    pub fn status(&self) -> Option<&str> {
        self.status.as_ref().map(|(message, _)| message.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm1_performer::library::FirmwareDevice;

    fn firmware(source: FirmwareSource, status: Status) -> Firmware {
        Firmware {
            name: "pkg".to_string(),
            byline: String::new(),
            version: "1.0".to_string(),
            description: String::new(),
            tags: Vec::new(),
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
            upgradable: false,
            status,
            source,
        }
    }

    #[test]
    fn deletable_mirrors_the_remove_button() {
        let file = || FirmwareSource::File { path: "f.fwsc".to_string() };
        let installed = || FirmwareSource::Hub {
            path: "fw/f.fwsc".to_string(),
            blob: Some("key".to_string()),
        };
        let offer = || FirmwareSource::Hub {
            path: "fw/f.fwsc".to_string(),
            blob: None,
        };
        // Builtins stay, whatever their state.
        assert!(!deletable(&firmware(FirmwareSource::Builtin, Status::Available)));
        // Installed and at hand: imported files and downloaded hub packages.
        assert!(deletable(&firmware(file(), Status::Available)));
        assert!(deletable(&firmware(installed(), Status::Available)));
        // A bare hub offer is the catalog, not the library.
        assert!(!deletable(&firmware(offer(), Status::Available)));
        // Not at hand: downloading or missing.
        assert!(!deletable(&firmware(file(), Status::Downloading)));
        assert!(!deletable(&firmware(installed(), Status::Missing)));
    }

    /// The filter is space-separated tokens, each a case-insensitive substring of the
    /// row's searchable text; every token must hit.
    #[test]
    fn filter_matches_all_tokens_as_substrings() {
        let mut felucca = firmware(FirmwareSource::Builtin, Status::Available);
        felucca.name = "Felucca".to_string();
        felucca.version = "1.2".to_string();
        felucca.tags = vec!["groove".to_string(), "dev".to_string()];
        felucca.byline = "author".to_string();
        felucca.description = "A live GROOVEBOX".to_string();
        assert!(matches_filter(&felucca, ""));
        assert!(matches_filter(&felucca, "fel gro"));
        assert!(matches_filter(&felucca, "GROOVE Dev 1.2"));
        assert!(matches_filter(&felucca, "author"));
        assert!(matches_filter(&felucca, "AUTHOR"));
        assert!(matches_filter(&felucca, "a LIVE groovebox"));
        assert!(!matches_filter(&felucca, "fel zzz"));
    }

    /// The device selector is a hard filter, FM-1 only by default like the GUI.
    #[test]
    fn device_filter_defaults_to_fm1_only() {
        let filter = DeviceFilter::default();
        assert!(filter.shows(FirmwareDevice::Fm1));
        assert!(!filter.shows(FirmwareDevice::Smk37));
    }

    /// Every source row gets its links and a connect row; sources and sinks sort by
    /// label so the row indices hold still.
    #[test]
    fn graph_rows_list_links_and_connect_actions() {
        let keyboard = Port::System("kb".to_string());
        let unit = Port::Device("fm1".to_string());
        let out = Port::System("out".to_string());
        let endpoints = vec![
            Endpoint {
                port: out.clone(),
                label: "Out".to_string(),
                source: false,
                sink: true,
            },
            Endpoint {
                port: unit.clone(),
                label: "Unit".to_string(),
                source: true,
                sink: true,
            },
            Endpoint {
                port: keyboard.clone(),
                label: "Keyboard".to_string(),
                source: true,
                sink: false,
            },
        ];
        let links = vec![(keyboard.clone(), unit.clone())];
        let rows = graph_rows(&endpoints, &links);
        assert_eq!(
            rows,
            vec![
                GraphRow::Source(keyboard.clone()),
                GraphRow::Link(keyboard.clone(), unit.clone()),
                GraphRow::Connect(keyboard),
                GraphRow::Source(unit.clone()),
                GraphRow::Connect(unit),
            ]
        );
    }
}
