//! The window's core: pane state and lifecycle (firmware loading, power), keyboard and
//! encoder routing, the per-frame LCD upload, and the eframe app itself.
//!
//! The UI areas are separate modules: [`crate::grid`] (the 3x3 card grid),
//! [`crate::sidebar`] (device sidebar), [`crate::graph`] (the MIDI graph editor in the
//! sidebar), [`crate::panel`] (the miniature front panel) and [`crate::transport`] (the
//! bottom bar). The look lives in [`crate::theme`], LCD pixel work in [`crate::lcd`],
//! and the FM-1 control maps in [`crate::keys`].

use crate::audio::{start_audio, AudioRing};
use crate::builtin::BUILTINS;
use crate::clock::Clock;
use crate::emulator::EmulatorHandle;
use crate::hardware::Hardware;
use crate::keys::{BUTTONS, PIANO};
use crate::midi_keys::MidiKeys;
use crate::routing::Routing;
use crate::theme::apply_adwaita_style;
use crate::{lcd, MAX_SLOTS};
use eframe::egui;
use egui::{Color32, TextureOptions};
use fm1_soc::Action;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) enum Source {
    Builtin(usize),
    File(PathBuf),
}

pub(crate) struct Slot {
    pub handle: Option<EmulatorHandle>,
    pub source: Option<Source>,
    pub label: Option<String>,
    pub error: Option<String>,
}

impl Default for Slot {
    fn default() -> Self {
        Self { handle: None, source: None, label: None, error: None }
    }
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
    pub(crate) pending_source: Option<crate::routing::Port>,
    /// The live device whose flash selector is open (the devices pane UI state).
    pub(crate) flash_pick: Option<String>,
    rings: [Arc<AudioRing>; MAX_SLOTS],
    /// Kept alive for as long as the app wants sound.
    _audio: Option<cpal::Stream>,
    pub(crate) audio_error: Option<String>,
    pub(crate) textures: [egui::TextureHandle; MAX_SLOTS],
    /// Snapshot of the selected pane's screen, hypersaturated with a slight fisheye
    /// bulge and refreshed once a second; this is what its grid card shows, while the
    /// sidebar shows the live picture at 4x.
    pub(crate) ghost: egui::TextureHandle,
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
    /// Dominant colour of the selected pane's screen, recomputed once a second (and on
    /// pane switches); the grid background eases towards it over 500 ms.
    pub(crate) dominant: Color32,
    pub(crate) dominant_at: f64,
    /// When the selected pane's grid-card snapshot was last refreshed (3 Hz).
    pub(crate) ghost_at: f64,
    /// Snapshot counter, seeding the ghost's dynamic NTSC distortion.
    ghost_frame: u64,
    pub(crate) dominant_slot: usize,
    held_slot: Option<usize>,
}

impl GridApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: &[PathBuf]) -> Self {
        apply_adwaita_style(&cc.egui_ctx);
        let core_cpus = crate::affinity::core_cpus();

        let rings: [Arc<AudioRing>; MAX_SLOTS] =
            std::array::from_fn(|_| Arc::new(AudioRing::new()));
        let (audio, audio_error) = match start_audio(rings.clone()) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e)),
        };

        let textures = std::array::from_fn(|i| {
            cc.egui_ctx
                .load_texture(format!("lcd-{i}"), lcd::blank(), TextureOptions::NEAREST)
        });
        let ghost = cc
            .egui_ctx
            .load_texture("lcd-ghost", lcd::blank(), TextureOptions::NEAREST);

        let clock = Arc::new(Clock::start(MAX_SLOTS));
        let routing = Routing::start();
        let midi_keys = MidiKeys::start();
        let hardware = Hardware::start(clock.clone(), routing.clone());

        let mut app = Self {
            slots: std::array::from_fn(|_| Slot::default()),
            core_cpus,
            active: 0,
            clock,
            hardware,
            routing,
            midi_keys,
            pending_source: None,
            flash_pick: None,
            rings,
            _audio: audio,
            audio_error,
            textures,
            ghost,
            held: [false; 41],
            panel_down: [false; 41],
            leds: [0.0; 41],
            master: [900; MAX_SLOTS],
            powered: [false; MAX_SLOTS],
            dominant: Color32::BLACK,
            dominant_at: 0.0,
            ghost_at: 0.0,
            ghost_frame: 0,
            dominant_slot: usize::MAX,
            held_slot: None,
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
                        handle.send(Action::Key { id: id as u8, down: false });
                    }
                }
            }
        }
        self.held = [false; 41];
        self.panel_down = [false; 41];
    }

    pub(crate) fn load_builtin(&mut self, slot: usize, index: usize) {
        let Some(firmware) = BUILTINS.get(index) else { return };
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

    pub(crate) fn pick_file(&mut self, slot: usize) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose an FM-1 firmware")
            .add_filter("FM-1 firmware", &["fwsc", "bin", "xip"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        self.load_file(slot, path);
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

        match EmulatorHandle::start(
            label,
            bytes,
            package,
            self.rings[slot].clone(),
            self._audio.is_some(),
            self.core_cpus.get(slot).copied(),
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
                crate::routing::pane_label(slot, self.slots[slot].label.as_ref()),
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
                // Nothing to run yet; the switch stays off until a firmware is chosen.
                None => self.powered[slot] = false,
            }
        } else {
            if self.held_slot == Some(slot) {
                self.release_held_keys();
            }
            self.clock.set_target(slot, None);
            self.slots[slot].handle = None;
            self.sync_pane(slot);
            self.rings[slot].reset();
            if slot == self.active {
                self.leds = [0.0; 41];
            }
            lcd::blank_to(&mut self.textures[slot]);
            if slot == self.active {
                self.refresh_ghost();
            }
        }
    }

    /// Reconcile the emulator's key state with the computer keyboard and the mouse-held
    /// controls on the front panel. Called before the UI and again after panel interactions,
    /// so a mouse press and release are both delivered on the frame they happen.
    pub(crate) fn sync_held(&mut self, ctx: &egui::Context) {
        if self.held_slot.is_some_and(|slot| slot != self.active) {
            self.release_held_keys();
        }

        let mut want = self.panel_down;
        // Notes held on the selected physical MIDI keyboard light the same note keys
        // (the panel writes panel_down every frame, so this merges on top).
        for (id, down) in self.midi_keys.held().into_iter().enumerate() {
            want[id] |= down;
        }
        // A focused text field (the tempo entry, a firmware menu) owns the keyboard:
        // typing a number must not also trigger the FM-1's function buttons.
        if !ctx.wants_keyboard_input() {
            ctx.input(|input| {
                for (key, id) in PIANO {
                    if input.key_down(key) {
                        want[id as usize] = true;
                    }
                }
                for (key, id) in BUTTONS {
                    if input.key_down(key) {
                        want[id as usize] = true;
                    }
                }
            });
        }

        let Some(handle) = self.slots[self.active].handle.as_ref() else {
            self.held = [false; 41];
            self.held_slot = None;
            return;
        };

        for (id, &down) in want.iter().enumerate() {
            if down != self.held[id] {
                handle.send(Action::Key { id: id as u8, down });
                self.held[id] = down;
            }
        }
        self.held_slot = want.iter().any(|&down| down).then_some(self.active);
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

    /// Turn the three top-left encoders with the arrow and comma/period keys: ←/→ turn
    /// PRESETS, ↑/↓ turn ALGORITHM (up minus, down plus), and ,/. turn SELECT.
    fn encoder_keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (left, right, down, up, comma, period) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
                input.key_pressed(egui::Key::ArrowDown),
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::Comma),
                input.key_pressed(egui::Key::Period),
            )
        });
        if left {
            self.turn_encoder(6, -1);
        }
        if right {
            self.turn_encoder(6, 1);
        }
        if down {
            self.turn_encoder(1, 1);
        }
        if up {
            self.turn_encoder(1, -1);
        }
        if comma {
            self.turn_encoder(0, -1);
        }
        if period {
            self.turn_encoder(0, 1);
        }
    }

    fn upload_lcds(&mut self) {
        for slot in 0..MAX_SLOTS {
            let Some(handle) = &self.slots[slot].handle else { continue };
            let Ok(fb) = handle.lcd.lock() else { continue };
            let pixels = fb.iter().map(|&px| lcd::rgb565(px)).collect::<Vec<_>>();
            lcd::set_lcd(&mut self.textures[slot], pixels);
        }

        // LEDs are scanned by the firmware in short pulses; decay them slowly so the panel
        // does not flicker at 60 Hz.
        let fresh = self.slots[self.active]
            .handle
            .as_ref()
            .map(|handle| handle.leds())
            .unwrap_or([0.0; 41]);
        for (shown, &now) in self.leds.iter_mut().zip(fresh.iter()) {
            *shown = now.max(*shown * 0.8);
        }
    }

    /// The dominant colour of the selected pane's screen. A pane that is off shows black,
    /// and so its dominant colour is black too.
    pub(crate) fn dominant_lcd_color(&self) -> Color32 {
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => lcd::dominant(&fb),
            None => Color32::BLACK,
        }
    }

    /// Refresh the snapshot the selected pane shows on its grid card: its screen
    /// NTSC-distorted, then hypersaturated with a very slight fisheye bulge, taken three
    /// times a second (and on pane switches). The live, faithful picture is the 4x view
    /// in the sidebar. A pane that is off shows black.
    pub(crate) fn refresh_ghost(&mut self) {
        self.ghost_frame = self.ghost_frame.wrapping_add(1);
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => {
                let pixels = lcd::ghost(&fb, self.ghost_frame);
                lcd::set_lcd(&mut self.ghost, pixels);
            }
            None => lcd::blank_to(&mut self.ghost),
        }
    }
}

impl eframe::App for GridApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_held(ctx);
        self.encoder_keys(ctx);
        self.upload_lcds();
        // Bottom panel first, then the central grid takes what is left.
        self.transport(ctx);
        self.grid(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
