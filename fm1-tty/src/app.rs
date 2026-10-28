//! fm1-tty's state: the shared library/hardware/clock pieces from `fm1-performer` plus the
//! TUI's own selection, focus, popup and note-merge state. Drawing lives in `ui.rs`,
//! the event loop in `main.rs`.

use fm1_performer::clock::Clock;
use fm1_performer::firmware_picker::{pick_of, FirmwarePick};
use fm1_performer::hardware::{DeviceRow, FlashState, Hardware};
use fm1_performer::library::{Firmware, FirmwareSource, HubStatus, Library, Status};
use fm1_performer::midi_keys::MidiKeys;
use fm1_performer::routing::{Port, Routing};
use fm1_performer::store::Store;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// MIDI note number of the panel's lowest key (the mirror of the midi_keys mapping:
/// FM-1 key id 14 plays F3).
const MIDI_F3: u8 = 53;
/// The FM-1's note key ids run F3..G5.
const KEY_F3: u8 = 14;
const KEY_G5: u8 = 40;
/// Note-on velocity the virtual keyboard sends.
const VELOCITY: u8 = 100;
/// How long a computer-key note lasts on terminals that report no key releases.
const MOMENTARY: Duration = Duration::from_millis(250);
/// How long a transient footer message stays up.
const STATUS_TTL: Duration = Duration::from_secs(5);

/// Which section has the computer keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Library,
    Keys,
}

/// A modal over the main screen; while one is up it gets every key.
#[derive(Clone)]
pub enum Popup {
    /// The MIDI-keyboard selector: the highlighted row (0 is "no keyboard").
    MidiPorts(usize),
    /// The flash target picker: (selected row, target unit names, what to flash).
    FlashTarget(usize, Vec<String>, FirmwarePick),
}

/// Mouse hit regions, refilled by every draw. Plain data; `ui.rs` writes, `main.rs`
/// reads.
#[derive(Default)]
pub struct Regions {
    /// The piano: (rect, key id), black keys first (they overlap the whites).
    pub keys: Vec<(Rect, u8)>,
    /// The library list's inner area; the clicked row is `lib_offset + y − top`.
    pub lib_rows: Rect,
    pub lib_offset: usize,
    /// The "MIDI in" field: opens the keyboard selector.
    pub midi_field: Rect,
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
    /// The library view from the last tick, in draw order.
    pub firmwares: Vec<Firmware>,
    /// The live FM-1 units from the last tick.
    pub devices: Vec<DeviceRow>,
    /// The selected library row.
    pub sel: usize,
    pub focus: Focus,
    pub popup: Option<Popup>,
    /// Held note keys, all sources merged (what the piano paints).
    pub held: [bool; 41],
    /// Note keys this app has sent note-ons for (mouse + computer keys only; the
    /// physical keyboard's bytes reach the units through the MIDI graph instead).
    sent: [bool; 41],
    mouse_key: Option<u8>,
    kb_keys: HashSet<u8>,
    /// Momentary note-offs for terminals without release events.
    momentary: Vec<(u8, Instant)>,
    /// Whether the terminal reports key releases (kitty protocol).
    pub key_release: bool,
    /// The MIDI keyboard port the user picked (the poller may still be opening it).
    want_midi: Option<String>,
    /// Graph links this app created: (system source, device) port names.
    linked: Vec<(String, String)>,
    /// A dismissed flash board stays down until the next flash starts.
    flash_dismissed: bool,
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

impl TuiApp {
    /// Start the shared machinery: clock, MIDI graph, USB scanner, keyboard poller,
    /// store and library (the GUI's store directory, so downloads are shared). The
    /// hub catalog starts fetching in the background.
    pub fn new(key_release: bool) -> Self {
        // No emulator panes here; the hardware module's USB targets carry the clock.
        let clock = Arc::new(Clock::start(1));
        let routing = Routing::start();
        let midi_keys = MidiKeys::start();
        let hardware = Hardware::start(clock.clone(), routing.clone());
        let store = match Store::new() {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("fm1-tty: no on-disk store: {e}");
                None
            }
        };
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
            devices: Vec::new(),
            sel: 0,
            focus: Focus::Library,
            popup: None,
            held: [false; 41],
            sent: [false; 41],
            mouse_key: None,
            kb_keys: HashSet::new(),
            momentary: Vec::new(),
            key_release,
            want_midi: None,
            linked: Vec::new(),
            flash_dismissed: false,
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

    /// The per-frame update: fold in finished downloads, track the devices, expire
    /// momentary notes, send note changes, and keep the keyboard's graph links
    /// matched to the units that are actually plugged in.
    pub fn tick(&mut self) {
        self.firmwares = self
            .library
            .as_mut()
            .map(|library| library.list_clone())
            .unwrap_or_default();
        self.sel = if self.firmwares.is_empty() {
            0
        } else {
            self.sel.min(self.firmwares.len() - 1)
        };
        self.devices = self.hardware.devices();

        let now = Instant::now();
        let (expired, pending): (Vec<_>, Vec<_>) =
            self.momentary.drain(..).partition(|(_, at)| *at <= now);
        self.momentary = pending;
        for (id, _) in expired {
            self.kb_keys.remove(&id);
        }

        self.sync_notes();
        self.reconcile_links();
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

    /// Send note on/off for changed local (mouse/computer) keys to every connected
    /// unit, and merge every source into the painted key state.
    fn sync_notes(&mut self) {
        let mut local = [false; 41];
        for &id in &self.kb_keys {
            local[id as usize] = true;
        }
        if let Some(id) = self.mouse_key {
            local[id as usize] = true;
        }
        let midi = self.midi_keys.held();
        for id in KEY_F3..=KEY_G5 {
            let (now, was) = (local[id as usize], self.sent[id as usize]);
            if now != was {
                let note = id - KEY_F3 + MIDI_F3;
                let message = if now {
                    [0x90, note, VELOCITY]
                } else {
                    [0x80, note, 0]
                };
                for device in &self.devices {
                    self.hardware.send(&device.name, &message);
                }
                self.sent[id as usize] = now;
            }
            self.held[id as usize] = local[id as usize] | midi[id as usize];
        }
    }

    /// The selected physical keyboard plays every connected unit: keep one graph link
    /// (system port → device) per live unit, adding and dropping as they come and go.
    /// Only links this app created are ever touched.
    fn reconcile_links(&mut self) {
        // Forget links the graph already pruned (their device went away).
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

    // ---- keyboard input -------------------------------------------------------

    /// A note key went down (computer key or mouse).
    pub fn note_down(&mut self, id: u8) {
        if !(KEY_F3..=KEY_G5).contains(&id) {
            return;
        }
        if self.kb_keys.insert(id) && !self.key_release {
            // No release events on this terminal: ring the note briefly.
            self.momentary.push((id, Instant::now() + MOMENTARY));
        }
    }

    /// A note key went up (terminals with release events only).
    pub fn note_up(&mut self, id: u8) {
        self.kb_keys.remove(&id);
        self.momentary.retain(|(held, _)| *held != id);
    }

    /// The mouse pressed (or dragged onto) a piano key; `None` on release.
    pub fn mouse_note(&mut self, id: Option<u8>) {
        self.mouse_key = id;
    }

    /// Whether the mouse is currently holding a piano key (drags only retarget then).
    pub fn mouse_holding(&self) -> bool {
        self.mouse_key.is_some()
    }

    /// Lift everything: on quit, so no note sticks on a unit.
    pub fn release_all(&mut self) {
        self.kb_keys.clear();
        self.momentary.clear();
        self.mouse_key = None;
        self.sync_notes();
    }

    // ---- library ---------------------------------------------------------------

    /// Move the library selection (negative up).
    pub fn scroll(&mut self, delta: isize) {
        if self.firmwares.is_empty() {
            return;
        }
        let last = self.firmwares.len() - 1;
        self.sel = (self.sel.saturating_add_signed(delta)).min(last);
    }

    /// The selected firmware, when there is one.
    fn selected(&self) -> Option<&Firmware> {
        self.firmwares.get(self.sel)
    }

    /// Download the selected hub offer; a no-op with a hint otherwise.
    pub fn download_selected(&mut self) {
        let Some(firmware) = self.selected() else {
            return;
        };
        let downloadable = firmware.status == Status::Available
            && matches!(firmware.source, FirmwareSource::Hub { ref blob, .. } if blob.is_none());
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

    /// Re-fetch the hub catalog now.
    pub fn reload_hub(&mut self) {
        if let Some(library) = &mut self.library {
            library.reload_hub();
        }
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

    /// Flash the selected firmware: pick the target when several units are plugged
    /// in, flash directly when there is exactly one.
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
        let targets: Vec<String> = self
            .devices
            .iter()
            .filter(|device| device.flash.is_none())
            .map(|device| device.name.clone())
            .collect();
        match targets.len() {
            0 => self.say("no live FM-1 on USB".to_string()),
            1 => self.start_flash(&targets[0], &pick),
            _ => self.popup = Some(Popup::FlashTarget(0, targets, pick)),
        }
    }

    /// Flash the package to the named unit; failures land on the device's row.
    pub fn start_flash(&mut self, unit: &str, pick: &FirmwarePick) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                self.hardware.flash(unit, bytes);
                self.flash_dismissed = false;
            }
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

    /// Choose the physical MIDI keyboard (None for off). The notes light the piano at
    /// once; the graph links land on the next ticks once the port is open.
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

    /// Dismiss a finished (failed) flash board.
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

impl Drop for TuiApp {
    /// Lift every note this app is holding, so nothing sticks on a unit. (The clock's
    /// own Drop sends MIDI Stop.)
    fn drop(&mut self) {
        self.release_all();
    }
}
