//! The window's core: pane state and lifecycle (firmware loading, power), keyboard and
//! encoder routing, the per-tick LCD upload, and the gpui view itself.
//!
//! The UI areas are separate modules: [`crate::grid`] (the 2x2 card grid),
//! [`crate::sidebar`] (device sidebar), [`crate::graph`] (the MIDI graph editor in the
//! sidebar), [`crate::panel`] (the miniature front panel) and [`crate::transport`] (the
//! bottom bar). The look lives in [`crate::theme`] plus the gpui-kit theme tokens, LCD
//! pixel work in [`crate::lcd`], and the FM-1 control maps in [`fm1_performer::keys`].
//!
//! egui used to poll the keyboard once per frame; gpui delivers key events instead, so
//! the held computer keys are tracked in [`GridApp::kb_down`] from `on_key_down` /
//! `on_key_up` and merged with the panel mouse and the physical MIDI keyboard in
//! [`GridApp::sync_held`]. The computer keyboard and panel mouse play only while the
//! Emulator tab shows the panel; the MIDI keyboard forwards on any tab. A timer task
//! ticks every 16 ms to upload LCD frames, decay LEDs, ease the grid background and
//! repaint.

use crate::audio::start_audio;
use crate::firmware_picker::FirmwareDelegate;
use crate::lcd;
use fm1_performer::MAX_SLOTS;
use fm1_performer::audio::AudioRing;
use fm1_performer::builtin::BUILTINS;
use fm1_performer::clock::Clock;
use fm1_performer::emulator::{EmulatorHandle, LCD_H, LCD_W};
use fm1_performer::hardware::Hardware;
use fm1_performer::keys::{BUTTONS, PIANO};
use fm1_performer::midi_keys::MidiKeys;
use fm1_performer::routing::Routing;
use fm1_soc::Action;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{VirtualListScrollHandle, resizable_panel};
use gpui_kit::component::combobox::{ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::resizable::{ResizableState, h_resizable};
use gpui_kit::component::{ActiveTheme, Icon, Theme, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) enum Source {
    Builtin(usize),
    File(PathBuf),
    /// A downloaded hub package, by blob-store key (the label is its display name).
    Blob {
        key: String,
        label: String,
    },
}

#[derive(Default)]
pub(crate) struct Slot {
    pub handle: Option<EmulatorHandle>,
    pub source: Option<Source>,
    pub label: Option<String>,
    pub error: Option<String>,
    /// The firmware advertises USB-MIDI on real hardware (the emulator cannot bridge
    /// it; the MIDI graph shows it for context).
    pub usb_midi: bool,
    /// Pinned: the pane's device storage persists across power-offs and runs (the
    /// flash snapshot lives in the store).
    pub pinned: bool,
}

/// A pinned slot's record in the store, `slot-N`.
#[derive(serde::Serialize, serde::Deserialize)]
struct PinnedSlot {
    firmware: String,
    source: PinnedSource,
}

/// An emulator's audio health, from its five-second dropout window: the colour of
/// the grid card's status dot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Green,
    Yellow,
    Orange,
    Red,
}

impl Health {
    /// The ramp, in drops per window. Audio callbacks run every few milliseconds, so
    /// [`RED_AT`] drops is already several percent of them running dry.
    fn of(drops_5s: u64) -> Self {
        const ORANGE_AT: u64 = 6;
        const RED_AT: u64 = 26;
        match drops_5s {
            0 => Self::Green,
            1..ORANGE_AT => Self::Yellow,
            ORANGE_AT..RED_AT => Self::Orange,
            _ => Self::Red,
        }
    }
}

/// The dot's painted state: it appears on any non-green colour and lingers
/// [`DOT_LINGER`] after recovering to green, then hides until the colour changes.
#[derive(Clone, Copy)]
pub(crate) struct SlotHealth {
    pub color: Health,
    since: Instant,
    pub shown: bool,
}

/// How long a recovered (green) dot stays visible before hiding.
pub(crate) const DOT_LINGER: Duration = Duration::from_secs(1);

impl SlotHealth {
    /// Fold in the latest window count; true when the painted state changed.
    pub(crate) fn track(&mut self, drops_5s: u64, running: bool) -> bool {
        let color = if running {
            Health::of(drops_5s)
        } else {
            Health::Green
        };
        if color != self.color {
            *self = Self {
                color,
                since: Instant::now(),
                shown: running,
            };
            return true;
        }
        let shown = running && (color != Health::Green || self.since.elapsed() < DOT_LINGER);
        std::mem::replace(&mut self.shown, shown) != shown
    }
}

impl Default for SlotHealth {
    fn default() -> Self {
        Self {
            color: Health::Green,
            since: Instant::now(),
            shown: false,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PinnedSource {
    Builtin { index: usize },
    File { path: PathBuf },
    Blob { key: String, label: String },
}

/// The Library tab's add-file form: three text fields and the chosen file.
pub(crate) struct LibForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub description: Entity<InputState>,
    pub file: Option<PathBuf>,
    /// The list's filter box.
    pub filter: Entity<InputState>,
}

pub struct GridApp {
    pub(crate) slots: [Slot; MAX_SLOTS],
    /// One logical CPU per physical core, used to pin emulator threads on Linux.
    core_cpus: Vec<usize>,
    pub(crate) active: usize,
    pub(crate) clock: Arc<Clock>,
    /// The live FM-1 units on USB: discovery, firmware detection, flashing and clock
    /// routing, all on background threads.
    pub(crate) hardware: Hardware,
    /// The MIDI graph: routes system MIDI ports, running panes and live units to each
    /// other, on its own poller thread.
    pub(crate) routing: Routing,
    /// A physical MIDI keyboard driving the on-screen virtual keyboard, on its own
    /// poller thread; the transport bar's selector picks the port.
    pub(crate) midi_keys: MidiKeys,
    /// The graph source waiting for its first sink (the graph UI's pending row).
    pub(crate) pending_source: Option<fm1_performer::routing::Port>,
    /// Per-device flash arming (the row's "Flash" toggle) and its combobox, and the
    /// was-flashing marker that re-safes the toggle when a flash succeeds.
    pub(crate) flash_armed: std::collections::HashMap<String, bool>,
    pub(crate) flash_combos:
        std::collections::HashMap<String, Entity<ComboboxState<FirmwareDelegate>>>,
    flash_was: std::collections::HashMap<String, bool>,
    rings: [Arc<AudioRing>; MAX_SLOTS],
    /// Kept alive for as long as the app wants sound.
    _audio: Option<cpal::Stream>,
    pub(crate) audio_error: Option<String>,
    /// The per-slot live pictures, rebuilt on tick while a pane runs; `None` when off.
    /// The selected slot's frame is the sidebar's hqx-upscaled picture.
    pub(crate) frames: [Option<Arc<RenderImage>>; MAX_SLOTS],
    /// The framebuffer each slot's frame was built from; a pane whose screen has not
    /// changed costs a memcmp per tick instead of a full decode + upload.
    last_fb: [Option<Box<[u16]>>; MAX_SLOTS],
    /// The selected pane's screen, NTSC-distorted with a slight fisheye bulge and
    /// refreshed every tick; this is what its grid card shows.
    pub(crate) ghost: Arc<RenderImage>,
    /// Whether the ghost currently holds a live (powered) snapshot.
    ghost_live: bool,
    /// FM-1 keys/buttons currently held on the computer keyboard (event-driven; only
    /// set while the Emulator tab shows the panel).
    kb_down: [bool; 41],
    /// Keys/buttons currently sent as down to the active emulator (keyboard or panel mouse).
    pub(crate) held: [bool; 41],
    /// Front-panel keys/buttons currently held down with the mouse.
    pub(crate) panel_down: [bool; 41],
    /// Smoothed front-panel LED brightness for the active slot.
    pub(crate) leds: [f32; 41],
    /// The MASTER pot position for each slot (0..1023).
    master: [u16; MAX_SLOTS],
    /// Whether each slot's emulator is switched on. Off by default; a slot with no power
    /// has a loaded firmware but no running emulator thread.
    pub(crate) powered: [bool; MAX_SLOTS],
    /// The computed grid background: the dominant colour of the selected pane's screen,
    /// resampled once a second (and on pane switches) and eased towards over 500 ms.
    pub(crate) bg: [f32; 3],
    bg_from: [f32; 3],
    bg_target: [f32; 3],
    bg_at: Instant,
    bg_sampled_at: Instant,
    dominant_slot: usize,
    /// Snapshot counter, seeding the ghost's dynamic NTSC distortion.
    ghost_frame: u64,
    /// The sidebar screen's exact upscale factor (1, 2 or 4), decided by the sidebar
    /// panel's rendered size last frame and applied by `upload_lcds` with the hqx
    /// filter.
    pub(crate) sidebar_scale: usize,
    /// The sidebar panel's rendered size, recorded during prepaint (one frame of lag,
    /// the same trade the egui layout made with the graph height).
    pub(crate) sidebar_size: (f32, f32),
    /// The grid area's rendered size, recorded the same way; the cards snap between
    /// 1x and 0.5x screens to fit it.
    pub(crate) grid_size: (f32, f32),
    /// The grid cards' screen scale: exactly 1.0 or 0.5.
    pub(crate) grid_scale: f32,
    /// The splits' bound states, so the quit handler can read the panel widths, and
    /// the widths restored from the session record (the panels' initial sizes).
    pub(crate) emulator_split: Entity<ResizableState>,
    pub(crate) devices_split: Entity<ResizableState>,
    pub(crate) sidebar_width: f32,
    pub(crate) devices_width: Option<f32>,
    /// Per-slot audio health behind the grid cards' status dot.
    pub(crate) health: [SlotHealth; MAX_SLOTS],
    /// The main area's tab: 0 = Emulator (grid + sidebar split), 1 = Devices (USB
    /// devices + MIDI graph split).
    pub(crate) active_tab: usize,
    /// The front panel's rendered width, recorded during prepaint.
    pub(crate) panel_width: f32,
    /// Mouse-wheel detent accumulator for the panel knobs: (group, fractional detents).
    pub(crate) wheel_acc: (u32, f32),
    /// The beat light's state last tick, so its flips repaint without a constant poll.
    last_beat: bool,
    /// Ticks since start, driving the half-second external-state sweep.
    tick_count: u64,
    held_slot: Option<usize>,
    /// The keyboard focus for playing the active pane: the root view's handle.
    pub(crate) focus: FocusHandle,
    /// One searchable firmware combobox per slot (the sidebar shows the active one).
    pub(crate) firmware_combos: [Entity<ComboboxState<FirmwareDelegate>>; MAX_SLOTS],
    /// The tempo readout's text field.
    pub(crate) tempo_input: Entity<InputState>,
    /// On-disk state (pinned slots' firmware records and flash snapshots). None if the
    /// platform has no data directory.
    pub(crate) store: Option<fm1_performer::store::Store>,
    /// The firmware library (the Library tab's data; pickers read it too).
    pub(crate) library: fm1_performer::library::Library,
    /// The Library tab's add-file form.
    pub(crate) lib_form: LibForm,
    /// The library list's scroll state (shared with its scrollbar, PgUp/PgDn, and
    /// scroll-to-item for freshly downloaded firmware).
    pub(crate) lib_scroll: VirtualListScrollHandle,
    /// The library list's viewport height, for page scrolling.
    pub(crate) lib_view_h: f32,
    /// A firmware whose download was just kicked off: when it lands in the Installed
    /// section, the list scrolls to it (its section changes on completion).
    pub(crate) lib_scroll_to: Option<String>,
    /// The library generation the firmware pickers were last built from.
    combos_at_generation: u64,
    _tick: Task<()>,
    /// Event subscriptions: the firmware comboboxes, the tempo input, the flash picker.
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl GridApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, initial: &[PathBuf]) -> Self {
        let core_cpus = fm1_performer::affinity::core_cpus();

        let rings: [Arc<AudioRing>; MAX_SLOTS] =
            std::array::from_fn(|_| Arc::new(AudioRing::new()));
        let (audio, audio_error) = match start_audio(rings.clone()) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e)),
        };

        let clock = Arc::new(Clock::start(MAX_SLOTS));
        let routing = Routing::start();
        let midi_keys = MidiKeys::start();
        let hardware = Hardware::start(clock.clone(), routing.clone());
        let store = match fm1_performer::store::Store::new() {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("fm1-grid: no on-disk store: {e}");
                None
            }
        };
        // The previous session's layout and tempo; the tempo field below initialises
        // from the clock, so the restored bpm lands on it first.
        let session = crate::session::Session::load(store.as_ref());
        clock.set_bpm(session.bpm);
        let emulator_split = cx.new(|_| ResizableState::default());
        let devices_split = cx.new(|_| ResizableState::default());

        let focus = cx.focus_handle().tab_stop(true);
        focus.focus(window, cx);

        // Focus lost (a text field took it, or the window blurred): the keyboard no
        // longer plays the pane, so lift every computer-held key — the same thing
        // egui's `wants_keyboard_input` guard did, and the only chance to avoid stuck
        // notes when the window blurs with keys down.
        let weak = cx.weak_entity();
        let focus_out = window.on_focus_out(&focus, cx, move |_, _, cx| {
            weak.update(cx, |this: &mut Self, cx| {
                if this.kb_down.iter().any(|&down| down) {
                    this.kb_down = [false; 41];
                    this.sync_held();
                    cx.notify();
                }
            })
            .ok();
        });
        let mut subscriptions = vec![focus_out];

        // The chrome follows the OS light/dark setting: take the current appearance
        // (init() forced Light), then track live changes while the window is up.
        Theme::sync_system_appearance(Some(window), cx);
        subscriptions.push(window.observe_window_appearance(|window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        }));

        // One firmware combobox per slot; picking an entry loads it into that slot.
        // The delegates start empty; refresh_firmware_combos fills them from the
        // library once it (and any pins) are loaded.
        let firmware_combos: [Entity<ComboboxState<FirmwareDelegate>>; MAX_SLOTS] =
            std::array::from_fn(|slot| {
                let state = cx.new(|cx| {
                    ComboboxState::new(FirmwareDelegate::default(), vec![], window, cx)
                        .searchable(true)
                });
                // Change fires on the click that selects; Confirm fires when the
                // popup closes — handling both would install twice, so only Change.
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event, window, cx| {
                        if let ComboboxEvent::Change(values) = event {
                            if let Some(pick) = values.last() {
                                this.install_pick(slot, pick.clone());
                            }
                            this.focus.focus(window, cx);
                            cx.notify();
                        }
                    },
                ));
                // A dismissed popup returns the keyboard to the pane.
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    |this, _, _: &DismissEvent, window, cx| {
                        this.focus.focus(window, cx);
                    },
                ));
                state
            });

        // The Library tab's add-file form, and the list's filter box (its Change just
        // repaints — the list re-reads the text).
        let lib_form = LibForm {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Name")),
            version: cx.new(|cx| InputState::new(window, cx).placeholder("Version")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("Description")),
            file: None,
            filter: cx.new(|cx| InputState::new(window, cx).placeholder("Filter firmwares…")),
        };
        subscriptions.push(
            cx.subscribe_in(&lib_form.filter, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        );

        // The tempo field is free-form while it is focused — an invalid or
        // out-of-range value only paints the error state — and the sanitise-and-apply
        // pass runs on blur or Enter. The steppers edit the text without taking
        // focus, so their Change events apply immediately.
        let tempo_input = cx.new(|cx| InputState::new(window, cx));
        tempo_input.update(cx, |state, cx| {
            state.set_value(format!("{:03}", clock.bpm()), window, cx);
        });
        subscriptions.push(cx.subscribe_in(
            &tempo_input,
            window,
            |this, state, event, window, cx| {
                match event {
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        this.apply_tempo(state, window, cx);
                    }
                    InputEvent::Change if !state.read(cx).focus_handle(cx).is_focused(window) => {
                        this.apply_tempo(state, window, cx);
                    }
                    _ => {}
                }
                cx.notify();
            },
        ));

        // The frame pump: 16 ms, replacing egui's request_repaint_after. spawn_in
        // carries the window — the library-generation sweep rebuilds pickers, and
        // that needs one.
        let tick = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break; // the window is gone
                }
            }
        });

        // The library needs a store even when the platform has none (a throwaway one).
        let fallback_store;
        let store_ref = match &store {
            Some(store) => store,
            None => {
                fallback_store =
                    fm1_performer::store::Store::in_dir(&std::env::temp_dir().join("fm1-grid-nostore"));
                &fallback_store
            }
        };
        // The hub catalog comes in at app load; later refreshes keep an hourly cadence.
        let mut library = fm1_performer::library::Library::load(store_ref);
        library.refresh_hub();

        let mut app = Self {
            slots: std::array::from_fn(|_| Slot::default()),
            core_cpus,
            active: session.slot,
            clock,
            hardware,
            routing,
            midi_keys,
            pending_source: None,
            flash_armed: Default::default(),
            flash_combos: Default::default(),
            flash_was: Default::default(),
            rings,
            _audio: audio,
            audio_error,
            frames: std::array::from_fn(|_| None),
            last_fb: std::array::from_fn(|_| None),
            ghost: lcd::blank_image(),
            ghost_live: false,
            kb_down: [false; 41],
            held: [false; 41],
            panel_down: [false; 41],
            leds: [0.0; 41],
            master: [900; MAX_SLOTS],
            powered: [false; MAX_SLOTS],
            bg: [0.0; 3],
            bg_from: [0.0; 3],
            bg_target: [0.0; 3],
            bg_at: Instant::now(),
            bg_sampled_at: Instant::now() - Duration::from_secs(1),
            dominant_slot: usize::MAX,
            ghost_frame: 0,
            sidebar_scale: 1,
            sidebar_size: (512.0, 700.0),
            grid_size: (crate::grid::GRID_W, crate::grid::GRID_W),
            grid_scale: 1.0,
            emulator_split,
            devices_split,
            sidebar_width: session.sidebar_width,
            devices_width: session.devices_width,
            health: [SlotHealth::default(); MAX_SLOTS],
            active_tab: session.tab,
            panel_width: 1040.0,
            wheel_acc: (u32::MAX, 0.0),
            last_beat: false,
            tick_count: 0,
            held_slot: None,
            focus,
            firmware_combos,
            tempo_input,
            store,
            library,
            lib_form,
            lib_scroll: VirtualListScrollHandle::new(),
            lib_view_h: 400.0,
            lib_scroll_to: None,
            combos_at_generation: 0,
            _tick: tick,
            _subscriptions: subscriptions,
        };

        if initial.is_empty() {
            // Load the embedded firmwares into the first panes (powered off, like all).
            for slot in 0..BUILTINS.len().min(MAX_SLOTS) {
                app.load_builtin(slot, slot);
            }
        } else {
            for (slot, path) in initial.iter().take(MAX_SLOTS).enumerate() {
                app.load_file(slot, path.clone());
            }
        }
        // Pinned slots come back: their firmware record is in the store. CLI firmware
        // arguments win over a stored pin (the explicit now beats the remembered).
        // Read every record first (the store borrows the app immutably; applying them
        // mutates it).
        let records: Vec<(usize, PinnedSlot)> = match &app.store {
            Some(store) => (initial.len()..MAX_SLOTS)
                .filter_map(|slot| {
                    store
                        .fetch_object::<PinnedSlot>(&format!("slot-{slot}"))
                        .ok()
                        .map(|record| (slot, record))
                })
                .collect(),
            None => Vec::new(),
        };
        for (slot, record) in records {
            let restored = match &record.source {
                PinnedSource::Builtin { index } if *index < BUILTINS.len() => {
                    app.load_builtin(slot, *index);
                    true
                }
                PinnedSource::File { path } if path.is_file() => {
                    app.load_file(slot, path.clone());
                    true
                }
                PinnedSource::Blob { key, .. }
                    if app
                        .store
                        .as_ref()
                        .is_some_and(|store| store.fetch_blob(key).is_ok()) =>
                {
                    app.load_blob(slot, key);
                    true
                }
                _ => false,
            };
            if restored {
                app.slots[slot].pinned = true;
            }
        }

        // The pickers' lists and selections come from the library.
        app.combos_at_generation = app.library.generation();
        app.refresh_firmware_combos(window, cx);

        app
    }

    pub(crate) fn set_active(&mut self, slot: usize) {
        if slot >= MAX_SLOTS {
            return;
        }
        if self.active != slot {
            self.release_held_keys();
            self.active = slot;
            self.leds = [0.0; 41];
        }
    }

    fn release_held_keys(&mut self) {
        if let Some(slot) = self.held_slot.take() {
            if let Some(handle) = &self.slots[slot].handle {
                for (id, down) in self.held.iter().enumerate() {
                    if *down {
                        handle.send(Action::Key {
                            id: id as u8,
                            down: false,
                        });
                    }
                }
            }
        }
        self.held = [false; 41];
        self.panel_down = [false; 41];
        self.kb_down = [false; 41];
    }

    pub(crate) fn load_builtin(&mut self, slot: usize, index: usize) {
        let Some(firmware) = BUILTINS.get(index) else {
            return;
        };
        let label = firmware.label();
        let bytes = firmware.bytes.to_vec();
        self.install(slot, label, bytes, true, Source::Builtin(index));
    }

    fn load_file(&mut self, slot: usize, path: PathBuf) {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.slots[slot].error = Some(format!("{}: {e}", path.display()));
                return;
            }
        };
        let package = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fwsc"));
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.install(slot, label, bytes, package, Source::File(path));
    }

    /// What a picker selected, installed into the slot.
    pub(crate) fn install_pick(&mut self, slot: usize, pick: fm1_performer::firmware_picker::FirmwarePick) {
        use fm1_performer::firmware_picker::FirmwarePick;
        match pick {
            FirmwarePick::Builtin(index) => self.load_builtin(slot, index),
            FirmwarePick::File(path) => self.load_file(slot, path),
            FirmwarePick::HubBlob(key) => self.load_blob(slot, &key),
        }
    }

    /// A downloaded hub package from the blob store into a slot. Goes through the
    /// pick so zip-wrapped packages are unwrapped (and web-page blobs rejected)
    /// exactly like the USB flash path.
    fn load_blob(&mut self, slot: usize, key: &str) {
        let Some(store) = &self.store else { return };
        let pick = fm1_performer::firmware_picker::FirmwarePick::HubBlob(key.to_string());
        match pick.bytes(Some(store)) {
            Ok(bytes) => {
                let label = key.rsplit('/').next().unwrap_or(key).to_string();
                self.install(
                    slot,
                    label.clone(),
                    bytes,
                    true,
                    Source::Blob {
                        key: key.to_string(),
                        label,
                    },
                );
            }
            Err(e) => self.slots[slot].error = Some(e),
        }
    }

    /// Rebuild every slot's firmware combobox from the library and re-select the
    /// slot's current firmware (a download, import or removal changed the list).
    pub(crate) fn refresh_firmware_combos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for slot in 0..MAX_SLOTS {
            let delegate = crate::firmware_picker::FirmwareDelegate::from_app(self);
            let selected = match &self.slots[slot].source {
                Some(Source::Builtin(index)) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::Builtin(*index))
                }
                Some(Source::File(path)) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::File(path.clone()))
                }
                Some(Source::Blob { key, .. }) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::HubBlob(key.clone()))
                }
                None => None,
            };
            self.firmware_combos[slot].update(cx, |state, cx| {
                state.set_items(delegate, window, cx);
                if let Some(selected) = &selected {
                    state.set_selected_values(std::slice::from_ref(selected), window, cx);
                }
            });
        }
        // The devices' flash comboboxes list the same library (no selection to keep:
        // each flash is a fresh choice).
        let flash_combos: Vec<_> = self.flash_combos.values().cloned().collect();
        for combo in flash_combos {
            let delegate = crate::firmware_picker::FirmwareDelegate::from_app(self);
            combo.update(cx, |state, cx| {
                state.set_items(delegate, window, cx);
            });
        }
    }

    fn install(
        &mut self,
        slot: usize,
        label: String,
        bytes: Vec<u8>,
        package: bool,
        source: Source,
    ) {
        self.release_held_keys();
        // Unplug the clock before the emulator thread goes away.
        self.clock.set_target(slot, None);
        self.slots[slot].handle = None;
        self.sync_pane(slot);
        self.slots[slot].usb_midi = fm1_performer::usbmidi::advertised(&bytes);
        self.slots[slot].source = Some(source);
        self.slots[slot].label = Some(label.clone());
        self.slots[slot].error = None;
        self.master[slot] = 900;
        if slot == self.active {
            self.leds = [0.0; 41];
        }
        self.rings[slot].reset();

        // A powered-off slot keeps its firmware selection but runs nothing.
        if !self.powered[slot] {
            return;
        }

        // A pinned slot's device storage comes back with the boot.
        let resume_flash = if self.slots[slot].pinned {
            self.store
                .as_ref()
                .and_then(|store| store.fetch_blob(&format!("slot-{slot}.flash")).ok())
        } else {
            None
        };

        match EmulatorHandle::start(
            label,
            bytes,
            package,
            self.rings[slot].clone(),
            self._audio.is_some(),
            self.core_cpus.get(slot).copied(),
            resume_flash,
        ) {
            Ok(handle) => {
                self.clock.set_target(slot, Some(handle.tx.clone()));
                self.slots[slot].handle = Some(handle);
            }
            Err(e) => self.slots[slot].error = Some(e),
        }
        self.sync_pane(slot);
    }

    /// Keep the pane's MIDI-graph registration in step with its emulator handle: a
    /// running pane is a graph sink (its MIDI IN jack); anything else leaves the graph.
    fn sync_pane(&self, slot: usize) {
        let sink = self.slots[slot].handle.as_ref().map(|handle| {
            (
                fm1_performer::routing::pane_label(slot, self.slots[slot].label.as_ref()),
                handle.tx.clone(),
            )
        });
        self.routing.set_pane(slot, sink);
    }

    /// Switch a slot's emulator on or off. Powering on restarts whatever firmware the
    /// slot is set to; powering off stops its thread, silences it and blanks its screen.
    pub(crate) fn set_power(&mut self, slot: usize, on: bool) {
        if self.powered[slot] == on {
            return;
        }
        self.powered[slot] = on;
        if on {
            match self.slots[slot].source.clone() {
                Some(Source::Builtin(index)) => self.load_builtin(slot, index),
                Some(Source::File(path)) => self.load_file(slot, path),
                Some(Source::Blob { key, .. }) => self.load_blob(slot, &key),
                // Nothing to run yet; the switch stays off until a firmware is chosen.
                None => self.powered[slot] = false,
            }
        } else {
            if self.held_slot == Some(slot) {
                self.release_held_keys();
            }
            self.clock.set_target(slot, None);
            // A pinned slot's flash goes to the store before the thread dies.
            if self.slots[slot].pinned {
                self.save_pinned_slot(slot);
            }
            self.slots[slot].handle = None;
            self.sync_pane(slot);
            self.rings[slot].reset();
            if slot == self.active {
                self.leds = [0.0; 41];
            }
            self.frames[slot] = None;
        }
    }

    /// The sidebar's pin toggle. It only moves while the pane's emulator runs — a
    /// stopped pane keeps its state (and its pin) as is.
    pub(crate) fn set_pinned(&mut self, slot: usize, on: bool) {
        if self.slots[slot].handle.is_none() {
            return;
        }
        self.slots[slot].pinned = on;
    }

    /// Snapshot and store a pinned slot's running emulator (the power-off and quit
    /// paths share this). Takes and stops the handle either way.
    fn save_pinned_slot(&mut self, slot: usize) {
        let mut handle = self.slots[slot].handle.take();
        if let (Some(store), Some(handle)) = (&self.store, handle.as_mut()) {
            if let Some(flash) = handle.stop_with_snapshot() {
                let source = match &self.slots[slot].source {
                    Some(Source::Builtin(index)) => PinnedSource::Builtin { index: *index },
                    Some(Source::File(path)) => PinnedSource::File { path: path.clone() },
                    Some(Source::Blob { key, label }) => PinnedSource::Blob {
                        key: key.clone(),
                        label: label.clone(),
                    },
                    // A pinned slot always has a source; be quiet anyway.
                    None => return,
                };
                let record = PinnedSlot {
                    firmware: self.slots[slot].label.clone().unwrap_or_default(),
                    source,
                };
                if let Err(e) = store.store_blob(&format!("slot-{slot}.flash"), &flash) {
                    eprintln!("fm1-grid: {e}");
                }
                if let Err(e) = store.store_object(&format!("slot-{slot}"), &record) {
                    eprintln!("fm1-grid: {e}");
                }
            }
        }
    }

    /// The app is going away (the quit hook in main.rs): snapshot and store every
    /// pinned slot that still runs. Pinned slots already off saved at power-off.
    pub fn save_pinned(&mut self) {
        for slot in 0..MAX_SLOTS {
            if self.slots[slot].pinned && self.slots[slot].handle.is_some() {
                self.save_pinned_slot(slot);
            }
        }
    }

    /// Reconcile the emulator's key state with the computer keyboard (event-tracked),
    /// the mouse-held controls on the front panel and the physical MIDI keyboard.
    /// Returns whether the emulated key state changed (the panel's LEDs follow it).
    pub(crate) fn sync_held(&mut self) -> bool {
        if self.held_slot.is_some_and(|slot| slot != self.active) {
            self.release_held_keys();
        }

        let mut want = self.panel_down;
        // Notes held on the selected physical MIDI keyboard light the same note keys.
        for (id, down) in self.midi_keys.held().into_iter().enumerate() {
            want[id] |= down;
        }
        for (id, down) in self.kb_down.iter().enumerate() {
            want[id] |= down;
        }

        let before = self.held;
        let Some(handle) = self.slots[self.active].handle.as_ref() else {
            self.held = [false; 41];
            self.held_slot = None;
            return before != self.held;
        };

        for (id, &down) in want.iter().enumerate() {
            if down != self.held[id] {
                handle.send(Action::Key { id: id as u8, down });
                self.held[id] = down;
            }
        }
        self.held_slot = want.iter().any(|&down| down).then_some(self.active);
        before != self.held
    }

    /// PgUp/PgDn on the Library tab: scroll the list by ~a viewport. (The offset runs
    /// negative going down; clamp to the content.)
    fn scroll_library(&mut self, pages: f32) {
        let current: f32 = self.lib_scroll.offset().y.into();
        let max: f32 = f32::from(self.lib_scroll.max_offset().y).abs();
        let next: f32 = (current - pages * self.lib_view_h * 0.9).clamp(-max, 0.0);
        self.lib_scroll.set_offset(point(px(0.0), px(next)));
    }

    pub(crate) fn turn_encoder(&self, enc: u8, clicks: i32) {
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Enc { enc, clicks });
        }
    }

    pub(crate) fn adjust_master(&mut self, clicks: i32) {
        let value = (self.master[self.active] as i32 + clicks * 16).clamp(0, 1023) as u16;
        self.master[self.active] = value;
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Adc { ch: 4, value });
        }
    }

    /// Computer key pressed while the root view has focus: an encoder turn for the
    /// arrow and comma/period keys, an FM-1 key/button down for the rest — but only
    /// on the Emulator tab, the only place the front panel shows; the Library tab
    /// keeps PgUp/PgDn for its list. Modifier chords (Ctrl/Alt/Super) are left
    /// alone; Shift is allowed so Caps Lock and shifted letters still play.
    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let modifiers = &stroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        match stroke.key.as_str() {
            // The Library tab pages its list.
            "pageup" if self.active_tab == 2 => self.scroll_library(-1.0),
            "pagedown" if self.active_tab == 2 => self.scroll_library(1.0),
            // Everything below plays the panel's pane; with the panel hidden the
            // keys stay inert.
            _ if self.active_tab != 0 => return,
            "left" => self.turn_encoder(6, -1),
            "right" => self.turn_encoder(6, 1),
            "down" => self.turn_encoder(1, 1),
            "up" => self.turn_encoder(1, -1),
            "," => self.turn_encoder(0, -1),
            "." => self.turn_encoder(0, 1),
            key => {
                let key = key.to_ascii_lowercase();
                let Some((_, id)) = PIANO
                    .iter()
                    .chain(BUTTONS.iter())
                    .find(|(name, _)| *name == key)
                else {
                    return;
                };
                self.kb_down[*id as usize] = true;
                self.sync_held();
            }
        }
        cx.notify();
    }

    /// Key releases only matter where presses are played (the Emulator tab); with the
    /// panel hidden `kb_down` is already empty, so this is a cheap early-out.
    fn key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab != 0 {
            return;
        }
        let key = event.keystroke.key.to_ascii_lowercase();
        if let Some((_, id)) = PIANO
            .iter()
            .chain(BUTTONS.iter())
            .find(|(name, _)| *name == key)
        {
            if self.kb_down[*id as usize] {
                self.kb_down[*id as usize] = false;
                self.sync_held();
                cx.notify();
            }
        }
    }

    /// A mouse release anywhere lifts the mouse-held panel keys/buttons. The panel
    /// shows only on the Emulator tab, so only there can any be held.
    fn mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab == 0 && self.panel_down.iter().any(|&down| down) {
            self.panel_down = [false; 41];
            self.sync_held();
            cx.notify();
        }
    }

    /// Sanitise the tempo field onto the metronome: parse, clamp into the clock's
    /// range, reformat. Text that is not a number snaps back to the live tempo.
    fn apply_tempo(
        &mut self,
        state: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = state.read(cx).value().to_string();
        let bpm = match text.trim().parse::<u32>() {
            Ok(bpm) => bpm.clamp(fm1_performer::clock::MIN_BPM, fm1_performer::clock::MAX_BPM),
            Err(_) => self.clock.bpm(),
        };
        self.clock.set_bpm(bpm);
        let formatted = format!("{bpm:03}");
        if text != formatted {
            state.update(cx, |state, cx| state.set_value(formatted, window, cx));
        }
    }

    /// The 16 ms pump: merge inputs, upload LCD frames, refresh the ghost, decay the
    /// LEDs and ease the grid background. Replaces egui's per-frame `update()` work —
    /// with one retained-mode difference: nothing repaints unless something changed
    /// (plus a half-second sweep for state arriving on background threads), so an idle
    /// window costs a few memcmps instead of 60 renders a second.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The pickers rebuild when the library's installed list moved (a download
        // landed, an import, a removal).
        let generation = self.library.generation();
        if generation != self.combos_at_generation {
            self.combos_at_generation = generation;
            self.refresh_firmware_combos(window, cx);
        }

        let mut dirty = self.sync_held();
        dirty |= self.upload_lcds();
        dirty |= self.refresh_ghost();
        dirty |= self.ease_background();
        // The metronome's beat light flips without any UI input.
        let beat = self.clock.beat_light();
        dirty |= std::mem::replace(&mut self.last_beat, beat) != beat;
        // The cards' health dots follow each emulator's five-second dropout window.
        for slot in 0..MAX_SLOTS {
            let (running, drops) = match &self.slots[slot].handle {
                Some(handle) => (true, handle.drops_5s()),
                None => (false, 0),
            };
            dirty |= self.health[slot].track(drops, running);
        }
        // The cards snap between 1x and 0.5x screens to fit the split panel.
        let scale =
            if self.grid_size.0 >= crate::grid::GRID_W && self.grid_size.1 >= crate::grid::GRID_W {
                1.0
            } else {
                0.5
            };
        dirty |= std::mem::replace(&mut self.grid_scale, scale) != scale;
        // Flash progress and hotplugged USB devices arrive on the hardware module's
        // threads; sweep them up twice a second.
        self.tick_count = self.tick_count.wrapping_add(1);
        let sweeping = self.tick_count.is_multiple_of(30);
        dirty |= self.hardware.flash_state().is_some() || sweeping;
        if sweeping {
            // Every connected unit gets a flash combobox (built here, where a window
            // lives); gone units drop theirs. A flash that just succeeded re-safes the
            // row's toggle.
            let rows = self.hardware.devices();
            let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
            self.flash_combos.retain(|name, _| names.contains(name));
            for row in rows {
                if !self.flash_combos.contains_key(&row.name) {
                    self.open_flash_picker(&row.name.clone(), window, cx);
                    dirty = true;
                }
                let was = self.flash_was.insert(row.name.clone(), row.flash.is_some());
                if was == Some(true)
                    && row.flash.is_none()
                    && row
                        .outcome
                        .as_deref()
                        .is_some_and(|outcome| outcome.starts_with("flashed:"))
                {
                    self.flash_armed.insert(row.name.clone(), false);
                    dirty = true;
                }
            }
        }
        if dirty {
            cx.notify();
        }
    }

    /// Upload the live LCD pictures. A pane whose framebuffer is unchanged since the
    /// last upload is skipped after a single compare — powered panes with a static
    /// screen cost nothing.
    fn upload_lcds(&mut self) -> bool {
        let mut changed = false;
        for slot in 0..MAX_SLOTS {
            let Some(handle) = &self.slots[slot].handle else {
                if self.last_fb[slot].take().is_some() || self.frames[slot].take().is_some() {
                    changed = true;
                }
                continue;
            };
            let Ok(fb) = handle.lcd.lock() else { continue };
            if self.last_fb[slot].as_deref() == Some(fb.as_slice()) {
                continue;
            }
            self.last_fb[slot] = Some(fb.as_slice().into());
            // The selected pane's frame feeds the sidebar, hqx-upscaled to the exact
            // factor its layout chose last frame; the other running panes' grid cards
            // show the recessed look at 1x.
            self.frames[slot] = Some(if slot == self.active {
                let faithful: Vec<lcd::Rgb> = fb.iter().map(|&px| lcd::rgb565(px)).collect();
                let scale = if matches!(self.sidebar_scale, 2 | 4) {
                    self.sidebar_scale
                } else {
                    1
                };
                let pixels = lcd::upscaled(&faithful, scale);
                lcd::frame_image(&pixels, LCD_W * scale, LCD_H * scale)
            } else {
                lcd::frame_image(&lcd::recede(&fb), LCD_W, LCD_H)
            });
            changed = true;
        }

        // LEDs are scanned by the firmware in short pulses; decay them slowly so the panel
        // does not flicker at 60 Hz.
        let lit_before = self.lit_bits();
        let fresh = self.slots[self.active]
            .handle
            .as_ref()
            .map(|handle| handle.leds())
            .unwrap_or([0.0; 41]);
        for (shown, &now) in self.leds.iter_mut().zip(fresh.iter()) {
            *shown = now.max(*shown * 0.8);
        }
        changed || lit_before != self.lit_bits()
    }

    /// The panel's lit-LED set as a bitmap (the rendering threshold), for change
    /// detection across ticks.
    fn lit_bits(&self) -> u64 {
        self.leds
            .iter()
            .enumerate()
            .fold(0u64, |bits, (id, &v)| bits | ((v > 0.04) as u64) << id)
    }

    /// The dominant colour of the selected pane's screen. A pane that is off shows black,
    /// and so its dominant colour is black too.
    fn dominant_lcd_color(&self) -> lcd::Rgb {
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => lcd::dominant(&fb),
            None => [0, 0, 0],
        }
    }

    /// Resample the selected pane's dominant colour once a second (and right after a
    /// pane switch) and ease the background towards it over 500 ms whenever it changes.
    /// Reports whether the background moved (easing in progress).
    fn ease_background(&mut self) -> bool {
        let switched = self.dominant_slot != self.active;
        if self.bg_sampled_at.elapsed() >= Duration::from_secs(1) || switched {
            self.bg_sampled_at = Instant::now();
            self.dominant_slot = self.active;
            let [r, g, b] = self.dominant_lcd_color();
            let target = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
            if target != self.bg_target {
                self.bg_from = self.bg;
                self.bg_target = target;
                self.bg_at = Instant::now();
            }
        }
        let t = (self.bg_at.elapsed().as_secs_f32() / 0.5).min(1.0);
        if t >= 1.0 && self.bg == self.bg_target {
            return false;
        }
        for ch in 0..3 {
            self.bg[ch] = self.bg_from[ch] + (self.bg_target[ch] - self.bg_from[ch]) * t;
        }
        true
    }

    /// Refresh the snapshot the selected pane shows on its grid card: its screen
    /// NTSC-distorted with a very slight fisheye bulge, taken every tick while the pane
    /// runs (the dynamic distortion is the point — it always reports changed). A pane
    /// that is off shows black, blanked once.
    fn refresh_ghost(&mut self) -> bool {
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => {
                self.ghost_frame = self.ghost_frame.wrapping_add(1);
                let pixels = lcd::ghost(&fb, self.ghost_frame);
                self.ghost = lcd::frame_image(&pixels, LCD_W, LCD_H);
                self.ghost_live = true;
                true
            }
            None => {
                if self.ghost_live {
                    self.ghost = lcd::blank_image();
                    self.ghost_live = false;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Refocus the root view after a button/switch/menu interaction, so the computer
    /// keyboard keeps playing the active pane (egui surrendered widget focus for the
    /// same reason).
    pub(crate) fn refocus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }
}

/// The window height the full stack needs (title bar, tabs, full panel, 1x grid);
/// below it the front panel's keyboard sheds its compact allowance.
const PANEL_COMPACT_BELOW: f32 = 950.0;

impl GridApp {
    /// The Emulator tab: the 2x2 grid and the selected pane's sidebar in a draggable
    /// split, with the miniature front panel tucked at the bottom — the only tab that
    /// shows it. The sidebar's range holds the screen at 1x to 4x; the grid panel's
    /// minimum keeps the 0.5x cards whole (the cards snap down when it shrinks).
    fn emulator_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().height < px(PANEL_COMPACT_BELOW);
        v_flex()
            .size_full()
            .child(
                div().flex_1().min_h_0().w_full().child(
                    h_resizable("emulator-split")
                        .with_state(&self.emulator_split)
                        .child(
                            // gpui-base rescales panels with the container only when
                            // every panel has a size; an unsized grid panel would pin
                            // the sidebar to its initial width (and its screen would
                            // never reach 4x).
                            resizable_panel()
                                .size(px(1152.0 - self.sidebar_width))
                                .size_range(px(crate::grid::GRID_MIN_W)..Pixels::MAX)
                                .child(self.grid_area(cx)),
                        )
                        .child(
                            resizable_panel()
                                .size(px(self.sidebar_width))
                                .size_range(
                                    px(crate::theme::SIDEBAR_MIN)..px(crate::theme::SIDEBAR_MAX),
                                )
                                .child(self.device_sidebar(window, cx)),
                        ),
                ),
            )
            .child(self.control_display(cx, compact))
    }

    /// The left icon rail: the app tabs as icon-only buttons stacked vertically.
    /// Mirrors the old segmented TabBar's states — the selected button keeps the
    /// tab_active_foreground ink on the background token with a small shadow, the
    /// rest use tab_foreground and pick up a tab_active wash on hover. Icons run
    /// at size_5 (20px), a notch up from the 16px the large tabs used.
    fn tab_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (rail_bg, ink, dim, wash, border, chip) = {
            let theme = cx.theme();
            (
                theme.tab_bar,
                theme.tab_active_foreground,
                theme.tab_foreground,
                theme.tab_active,
                theme.border,
                theme.tokens.background,
            )
        };
        const TABS: [(Lucide, usize); 3] = [
            (Lucide::Monitor, 0),
            (Lucide::KeyboardMusic, 1),
            (Lucide::Library, 2),
        ];
        v_flex()
            .w(px(48.0))
            .h_full()
            .flex_shrink_0()
            .bg(rail_bg)
            .border_r_1()
            .border_color(border)
            .pt_2()
            .items_center()
            .gap_2()
            .children(TABS.iter().map(|&(icon, ix)| {
                let selected = ix == self.active_tab;
                div()
                    .id(("tab", ix))
                    .test_support()
                    .size(px(36.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_color(if selected { ink } else { dim })
                    .when(selected, |this| this.bg(chip).shadow_xs())
                    .when(!selected, |this| this.hover(|this| this.bg(wash)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.active_tab == 0 && ix != 0 {
                            // The panel hides with the tab: lift computer- and
                            // mouse-held keys so nothing sticks while it's gone.
                            // Physical-MIDI notes survive — sync_held re-merges
                            // them into the wanted state.
                            this.kb_down = [false; 41];
                            this.panel_down = [false; 41];
                            this.sync_held();
                        }
                        this.active_tab = ix;
                        if ix == 2 {
                            // Opening the library refreshes the online listing.
                            this.library.refresh_hub();
                        }
                        this.refocus(window, cx);
                        cx.notify();
                    }))
                    .child(Icon::new(icon).size_5())
            }))
    }

    /// The Devices tab: the live USB units and the MIDI graph in an even split.
    fn devices_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        div().size_full().bg(background).child(
            h_resizable("devices-split")
                .with_state(&self.devices_split)
                .child(
                    match self.devices_width {
                        Some(width) => resizable_panel().size(px(width)),
                        None => resizable_panel(),
                    }
                    .child(div().size_full().p_2().child(self.devices_pane(window, cx))),
                )
                .child(resizable_panel().child(self.graph_section(cx))),
        )
    }
}

impl Render for GridApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (background, dim) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            // The in-app title bar: draws the window controls where the compositor
            // doesn't supply decorations (GNOME), and owns window dragging.
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(dim)
                        .child("FM-1 GRID"),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.tab_rail(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(match self.active_tab {
                                0 => self.emulator_tab(window, cx).into_any_element(),
                                1 => self.devices_tab(window, cx).into_any_element(),
                                _ => self.library_tab(window, cx).into_any_element(),
                            }),
                    ),
            )
            .child(self.transport(window, cx))
    }
}
