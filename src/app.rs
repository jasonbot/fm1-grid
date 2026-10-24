//! The window's core: pane state and lifecycle (firmware loading, power), keyboard and
//! encoder routing, the per-tick LCD upload, and the gpui view itself.
//!
//! The UI areas are separate modules: [`crate::grid`] (the 2x2 card grid),
//! [`crate::sidebar`] (device sidebar), [`crate::graph`] (the MIDI graph editor in the
//! sidebar), [`crate::panel`] (the miniature front panel) and [`crate::transport`] (the
//! bottom bar). The look lives in [`crate::theme`] plus the gpui-kit theme tokens, LCD
//! pixel work in [`crate::lcd`], and the FM-1 control maps in [`crate::keys`].
//!
//! egui used to poll the keyboard once per frame; gpui delivers key events instead, so
//! the held computer keys are tracked in [`GridApp::kb_down`] from `on_key_down` /
//! `on_key_up` and merged with the panel mouse and the physical MIDI keyboard in
//! [`GridApp::sync_held`]. A timer task ticks every 16 ms to upload LCD frames, decay
//! LEDs, ease the grid background and repaint.

use crate::audio::{AudioRing, start_audio};
use crate::builtin::BUILTINS;
use crate::clock::Clock;
use crate::emulator::{EmulatorHandle, LCD_H, LCD_W};
use crate::firmware_picker::FirmwareDelegate;
use crate::hardware::Hardware;
use crate::keys::{BUTTONS, PIANO};
use crate::midi_keys::MidiKeys;
use crate::routing::Routing;
use crate::{MAX_SLOTS, lcd};
use fm1_soc::Action;
use gpui_kit::component::combobox::{ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// The firmware advertises USB-MIDI on real hardware (the emulator cannot bridge
    /// it; the MIDI graph shows it for context).
    pub usb_midi: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            handle: None,
            source: None,
            label: None,
            error: None,
            usb_midi: false,
        }
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
    /// The flash selector's combobox state, created when the picker opens.
    pub(crate) flash_combo: Option<Entity<ComboboxState<FirmwareDelegate>>>,
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
    /// FM-1 keys/buttons currently held on the computer keyboard (event-driven).
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
    /// The sidebar screen's exact upscale factor (1, 2 or 4), decided by the sidebar's
    /// rendered width last frame and applied by `upload_lcds` with the hqx filter.
    pub(crate) sidebar_scale: usize,
    /// The sidebar's rendered width, recorded during prepaint (one frame of lag, the
    /// same trade the egui layout made with the graph height).
    pub(crate) sidebar_width: f32,
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
    _tick: Task<()>,
    /// Event subscriptions: the firmware comboboxes, the tempo input, the flash picker.
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl GridApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, initial: &[PathBuf]) -> Self {
        let core_cpus = crate::affinity::core_cpus();

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

        // One firmware combobox per slot; picking an entry loads it into that slot.
        let firmware_combos: [Entity<ComboboxState<FirmwareDelegate>>; MAX_SLOTS] =
            std::array::from_fn(|slot| {
                let state = cx.new(|cx| {
                    ComboboxState::new(FirmwareDelegate::new(), vec![], window, cx)
                        .searchable(true)
                });
                // Change fires on the click that selects; Confirm fires when the
                // popup closes — handling both would install twice, so only Change.
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event, window, cx| {
                        if let ComboboxEvent::Change(values) = event {
                            match values.last() {
                                Some(&Some(index)) => this.load_builtin(slot, index),
                                Some(&None) => this.pick_file(slot),
                                None => {}
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

        // The tempo readout edits the metronome's BPM directly.
        let tempo_input = cx.new(|cx| InputState::new(window, cx));
        tempo_input.update(cx, |state, cx| {
            state.set_value(format!("{:03}", clock.bpm()), window, cx);
        });
        subscriptions.push(cx.subscribe_in(
            &tempo_input,
            window,
            |this, state, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    if let Ok(bpm) = text.trim().parse::<u32>() {
                        let clamped = bpm.clamp(crate::clock::MIN_BPM, crate::clock::MAX_BPM);
                        if clamped != this.clock.bpm() {
                            this.clock.set_bpm(clamped);
                        }
                        if clamped != bpm {
                            state.update(cx, |state, cx| {
                                state.set_value(format!("{clamped:03}"), window, cx)
                            });
                        }
                    }
                    cx.notify();
                }
            },
        ));

        // The frame pump: 16 ms, replacing egui's request_repaint_after.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break; // the window is gone
                }
            }
        });

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
            flash_combo: None,
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
            sidebar_width: 320.0,
            panel_width: 1040.0,
            wheel_acc: (u32::MAX, 0.0),
            last_beat: false,
            tick_count: 0,
            held_slot: None,
            focus,
            firmware_combos,
            tempo_input,
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
        // Reflect the CLI/builtin firmware choices in the comboboxes.
        for slot in 0..MAX_SLOTS {
            if let Some(Source::Builtin(index)) = app.slots[slot].source {
                app.firmware_combos[slot].update(cx, |state, cx| {
                    state.set_selected_values(&[Some(index)], window, cx);
                });
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
        self.kb_down = [false; 41];
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
        self.slots[slot].usb_midi = crate::usbmidi::advertised(&bytes);
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
            self.frames[slot] = None;
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
    /// arrow and comma/period keys, an FM-1 key/button down for the rest. Modifier
    /// chords (Ctrl/Alt/Super) are left alone; Shift is allowed so Caps Lock and
    /// shifted letters still play.
    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let modifiers = &stroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        match stroke.key.as_str() {
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

    fn key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
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

    /// A mouse release anywhere lifts the mouse-held panel keys/buttons.
    fn mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_down.iter().any(|&down| down) {
            self.panel_down = [false; 41];
            self.sync_held();
            cx.notify();
        }
    }

    /// The 16 ms pump: merge inputs, upload LCD frames, refresh the ghost, decay the
    /// LEDs and ease the grid background. Replaces egui's per-frame `update()` work —
    /// with one retained-mode difference: nothing repaints unless something changed
    /// (plus a half-second sweep for state arriving on background threads), so an idle
    /// window costs a few memcmps instead of 60 renders a second.
    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut dirty = self.sync_held();
        dirty |= self.upload_lcds();
        dirty |= self.refresh_ghost();
        dirty |= self.ease_background();
        // The metronome's beat light flips without any UI input.
        let beat = self.clock.beat_light();
        dirty |= std::mem::replace(&mut self.last_beat, beat) != beat;
        // Flash progress and hotplugged USB devices arrive on the hardware module's
        // threads; sweep them up twice a second.
        self.tick_count = self.tick_count.wrapping_add(1);
        dirty |= self.hardware.flash_state().is_some() || self.tick_count % 30 == 0;
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
            .fold(0u64, |bits, (id, &v)| {
                bits | ((v > 0.04) as u64) << id
            })
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
            let target = [
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ];
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
            .child(self.central(window, cx))
            .child(self.transport(window, cx))
    }
}
