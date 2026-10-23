//! The emulator grid window: LCDs, firmware choices, keyboard routing and the metronome.

use crate::audio::{start_audio, AudioRing};
use crate::builtin::BUILTINS;
use crate::clock::{Clock, MAX_BPM, MIN_BPM};
use crate::emulator::{EmulatorHandle, LCD_H, LCD_W};
use crate::MAX_SLOTS;
use eframe::egui;
use egui::{
    Align, Align2, Color32, ColorImage, CornerRadius, FontFamily, FontId, Layout, Margin, Pos2,
    Rect, RichText, Sense, Stroke, StrokeKind, TextStyle, TextureOptions, UiBuilder, Vec2,
};
use fm1_soc::Action;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// Mac OS 9 "Platinum" palette: light greys, 1 px dark outlines, a white bevel on top/left
// and a shadow on bottom/right, with the classic selection blue.
const PLATINUM: Color32 = Color32::from_rgb(214, 214, 214);
const DESKTOP: Color32 = Color32::from_rgb(188, 188, 188);
const PANEL: Color32 = Color32::from_rgb(230, 230, 230);
const CARD: Color32 = Color32::from_rgb(224, 224, 224);
const CARD_ACTIVE: Color32 = Color32::from_rgb(240, 240, 240);
const HOVER: Color32 = Color32::from_rgb(247, 247, 247);
const PRESSED: Color32 = Color32::from_rgb(197, 197, 197);
const OUTLINE: Color32 = Color32::from_rgb(84, 84, 84);
const SHADOW: Color32 = Color32::from_rgb(156, 156, 156);
const HIGHLIGHT: Color32 = Color32::from_rgb(255, 255, 255);
const INK: Color32 = Color32::from_rgb(24, 24, 24);
const DIM: Color32 = Color32::from_rgb(92, 92, 92);
const ACCENT: Color32 = Color32::from_rgb(42, 84, 168);
const RED: Color32 = Color32::from_rgb(160, 36, 36);
const LCD_BG: Color32 = Color32::from_rgb(24, 32, 26);
const LCD_ON: Color32 = Color32::from_rgb(126, 235, 146);
const LCD_OFF: Color32 = Color32::from_rgb(74, 100, 80);
const LED_OFF: Color32 = Color32::from_rgb(122, 130, 122);
const LED_ON: Color32 = Color32::from_rgb(52, 190, 96);

// The miniature front-panel display above the metronome.
const PANEL_BODY: Color32 = Color32::from_rgb(54, 56, 60);
const PANEL_BUTTON: Color32 = Color32::from_rgb(64, 67, 73);
const PANEL_BUTTON_EDGE: Color32 = Color32::from_rgb(28, 28, 30);
const SILK: Color32 = Color32::from_rgb(201, 198, 189);
const SILK_DIM: Color32 = Color32::from_rgb(128, 126, 120);
const AMBER: Color32 = Color32::from_rgb(242, 163, 58);
const GREEN: Color32 = Color32::from_rgb(98, 211, 110);
const KEY_WHITE: Color32 = Color32::from_rgb(235, 232, 222);
const KEY_BLACK: Color32 = Color32::from_rgb(38, 38, 42);

/// The computer-keyboard layout shared with fm1-live: C4..F5 on the piano keys.
const PIANO: [(egui::Key, u8); 18] = [
    (egui::Key::A, 21),
    (egui::Key::W, 22),
    (egui::Key::S, 23),
    (egui::Key::E, 24),
    (egui::Key::D, 25),
    (egui::Key::F, 26),
    (egui::Key::T, 27),
    (egui::Key::G, 28),
    (egui::Key::Y, 29),
    (egui::Key::H, 30),
    (egui::Key::U, 31),
    (egui::Key::J, 32),
    (egui::Key::K, 33),
    (egui::Key::O, 34),
    (egui::Key::L, 35),
    (egui::Key::P, 36),
    (egui::Key::Semicolon, 37),
    (egui::Key::Quote, 38),
];

/// The FM-1's octave and function buttons.
const BUTTONS: [(egui::Key, u8); 14] = [
    (egui::Key::Z, 0),
    (egui::Key::X, 1),
    (egui::Key::Num1, 2),
    (egui::Key::Num2, 3),
    (egui::Key::Num3, 4),
    (egui::Key::Num4, 5),
    (egui::Key::Num5, 6),
    (egui::Key::Num6, 7),
    (egui::Key::Num7, 8),
    (egui::Key::Num8, 9),
    (egui::Key::Num9, 10),
    (egui::Key::Num0, 11),
    (egui::Key::Space, 12),
    (egui::Key::R, 13),
];

/// The FM-1's two rows of six function buttons: (label, key id, computer key).
const FUNCTION_ROW_1: [(&str, u8, &str); 6] = [
    ("FX", 2, "1"),
    ("SCL", 3, "2"),
    ("ENV", 4, "3"),
    ("LFO", 5, "4"),
    ("EDIT", 6, "5"),
    ("GLO", 7, "6"),
];
const FUNCTION_ROW_2: [(&str, u8, &str); 6] = [
    ("HOME", 8, "7"),
    ("SAVE", 9, "8"),
    ("ARP", 10, "9"),
    ("SEQ", 11, "0"),
    ("PLAY", 12, "SPACE"),
    ("REC", 13, "R"),
];

/// The FM-1's 27 note keys, F3..G5: (key id, note name, black?).
const PANEL_KEYS: [(u8, &str, bool); 27] = [
    (14, "F3", false),
    (15, "F#3", true),
    (16, "G3", false),
    (17, "G#3", true),
    (18, "A3", false),
    (19, "A#3", true),
    (20, "B3", false),
    (21, "C4", false),
    (22, "C#4", true),
    (23, "D4", false),
    (24, "D#4", true),
    (25, "E4", false),
    (26, "F4", false),
    (27, "F#4", true),
    (28, "G4", false),
    (29, "G#4", true),
    (30, "A4", false),
    (31, "A#4", true),
    (32, "B4", false),
    (33, "C5", false),
    (34, "C#5", true),
    (35, "D5", false),
    (36, "D#5", true),
    (37, "E5", false),
    (38, "F5", false),
    (39, "F#5", true),
    (40, "G5", false),
];

/// White keys across the panel, left to right.
const WHITE_KEYS: [u8; 16] = [14, 16, 18, 20, 21, 23, 25, 26, 28, 30, 32, 33, 35, 37, 38, 40];

/// Black keys: (key id, index of the white key they sit after).
const BLACK_KEYS: [(u8, usize); 11] = [
    (15, 0),
    (17, 1),
    (19, 2),
    (22, 4),
    (24, 5),
    (27, 7),
    (29, 8),
    (31, 9),
    (34, 11),
    (36, 12),
    (39, 14),
];

#[derive(Clone)]
enum Source {
    Builtin(usize),
    File(PathBuf),
}

struct Slot {
    handle: Option<EmulatorHandle>,
    source: Option<Source>,
    label: Option<String>,
    error: Option<String>,
}

impl Default for Slot {
    fn default() -> Self {
        Self { handle: None, source: None, label: None, error: None }
    }
}

enum Choice {
    Builtin(usize),
    Other,
}

pub struct GridApp {
    slots: [Slot; MAX_SLOTS],
    /// Number of slots actually shown and running (1..=MAX_SLOTS).
    slot_count: usize,
    active: usize,
    clock: Clock,
    rings: [Arc<AudioRing>; MAX_SLOTS],
    /// Kept alive for as long as the app wants sound.
    _audio: Option<cpal::Stream>,
    audio_error: Option<String>,
    textures: [egui::TextureHandle; MAX_SLOTS],
    /// Keys/buttons currently sent as down to the active emulator (keyboard or panel mouse).
    held: [bool; 41],
    /// Front-panel keys/buttons currently held down with the mouse.
    panel_down: [bool; 41],
    /// Smoothed front-panel LED brightness for the active slot.
    leds: [f32; 41],
    /// The MASTER pot position for each slot (0..1023).
    master: [u16; MAX_SLOTS],
    held_slot: Option<usize>,
}

impl GridApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: &[PathBuf],
        slot_count: usize,
    ) -> Self {
        apply_platinum_style(&cc.egui_ctx);
        let slot_count = slot_count.clamp(1, MAX_SLOTS);

        let rings: [Arc<AudioRing>; MAX_SLOTS] =
            std::array::from_fn(|_| Arc::new(AudioRing::new()));
        let (audio, audio_error) = match start_audio(rings.clone()) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e)),
        };

        let blank = ColorImage::new([LCD_W, LCD_H], vec![Color32::BLACK; LCD_W * LCD_H]);
        let textures = std::array::from_fn(|i| {
            cc.egui_ctx
                .load_texture(format!("lcd-{i}"), blank.clone(), TextureOptions::NEAREST)
        });

        let mut app = Self {
            slots: std::array::from_fn(|_| Slot::default()),
            slot_count,
            active: 0,
            clock: Clock::start(slot_count),
            rings,
            _audio: audio,
            audio_error,
            textures,
            held: [false; 41],
            panel_down: [false; 41],
            leds: [0.0; 41],
            master: [900; MAX_SLOTS],
            held_slot: None,
        };

        if initial.is_empty() {
            // Start with the first N embedded firmwares side by side.
            for slot in 0..BUILTINS.len().min(slot_count) {
                app.load_builtin(slot, slot);
            }
        } else {
            for (slot, path) in initial.iter().take(slot_count).enumerate() {
                app.load_file(slot, path.clone());
            }
        }

        app
    }

    fn set_active(&mut self, slot: usize) {
        if slot >= self.slot_count {
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

    fn load_builtin(&mut self, slot: usize, index: usize) {
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

    fn pick_file(&mut self, slot: usize) {
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
        self.slots[slot].source = Some(source);
        self.slots[slot].label = Some(label.clone());
        self.slots[slot].error = None;
        self.master[slot] = 900;
        if slot == self.active {
            self.leds = [0.0; 41];
        }
        self.rings[slot].reset();

        match EmulatorHandle::start(
            label,
            bytes,
            package,
            self.rings[slot].clone(),
            self._audio.is_some(),
        ) {
            Ok(handle) => {
                self.clock.set_target(slot, Some(handle.tx.clone()));
                self.slots[slot].handle = Some(handle);
            }
            Err(e) => self.slots[slot].error = Some(e),
        }
    }

    /// Reconcile the emulator's key state with the computer keyboard and the mouse-held
    /// controls on the front panel. Called before the UI and again after panel interactions,
    /// so a mouse press and release are both delivered on the frame they happen.
    fn sync_held(&mut self, ctx: &egui::Context) {
        if self.held_slot.is_some_and(|slot| slot != self.active) {
            self.release_held_keys();
        }

        let mut want = self.panel_down;
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

    fn turn_encoder(&self, enc: u8, clicks: i32) {
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Enc { enc, clicks });
        }
    }

    fn adjust_master(&mut self, clicks: i32) {
        let value = (self.master[self.active] as i32 + clicks * 16).clamp(0, 1023) as u16;
        self.master[self.active] = value;
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Adc { ch: 4, value });
        }
    }

    /// Turn the three top-left encoders with the arrow and comma/period keys, matching
    /// fm1-live's computer-keyboard layout.
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
            self.turn_encoder(0, -1);
        }
        if right {
            self.turn_encoder(0, 1);
        }
        if down {
            self.turn_encoder(1, -1);
        }
        if up {
            self.turn_encoder(1, 1);
        }
        if comma {
            self.turn_encoder(6, -1);
        }
        if period {
            self.turn_encoder(6, 1);
        }
    }

    fn upload_lcds(&mut self) {
        for slot in 0..self.slot_count {
            let Some(handle) = &self.slots[slot].handle else { continue };
            let Ok(fb) = handle.lcd.lock() else { continue };
            let pixels = fb
                .iter()
                .map(|&px| {
                    let r = ((px >> 11) & 0x1f) as u8;
                    let g = ((px >> 5) & 0x3f) as u8;
                    let b = (px & 0x1f) as u8;
                    Color32::from_rgb((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
                })
                .collect::<Vec<_>>();
            self.textures[slot].set(
                ColorImage::new([LCD_W, LCD_H], pixels),
                TextureOptions::NEAREST,
            );
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

    /// A miniature FM-1 front panel. It is a working control surface: note keys and
    /// buttons can be clicked, the knobs are turned with the -/+ ends, and both the
    /// computer keyboard and the emulated firmware's LEDs light the indicators.
    fn control_display(&mut self, ui: &mut egui::Ui) {
        // Keep the panel roughly as wide as the card row when extra slots are enabled.
        let panel_max_width = 1040.0 + self.slot_count.saturating_sub(2) as f32 * 250.0;
        let width = ui.available_width().min(panel_max_width);
        // The keys are 120 px tall (three times the old 40 px), so the panel grows by 80 px.
        let key_height = 120.0;
        let key_margin = 6.0;
        let panel_height = 196.0 + (key_height - 40.0);
        let (row, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), panel_height),
            Sense::hover(),
        );
        let panel = Rect::from_center_size(row.center(), Vec2::new(width, panel_height));
        let painter = ui.painter().clone();
        paint_recessed(&painter, panel, CornerRadius::same(4), PANEL_BODY);

        let left = panel.left() + 14.0;
        let right = panel.right() - 14.0;
        let controls_top = panel.top() + 24.0;
        let group_width = 120.0;
        let group_height = 24.0;
        let group_gap = 6.0;
        let row_gap = 4.0;

        painter.text(
            Pos2::new(left, panel.top() + 7.0),
            Align2::LEFT_TOP,
            "FM-1 PANEL",
            FontId::proportional(9.5),
            SILK_DIM,
        );
        painter.text(
            Pos2::new(right, panel.top() + 7.0),
            Align2::RIGHT_TOP,
            format!("CLICKABLE · KEYBOARD LIGHTS · SLOT {}", self.active + 1),
            FontId::proportional(9.5),
            SILK_DIM,
        );

        // The right-hand groups (the four knobs and the function buttons) span one
        // common width, so the knobs sit directly above the button grid.
        let function_width = (panel.width() * 0.52).clamp(360.0, 520.0);
        let function_left = right - function_width;
        let button_gap = 4.0;
        let button_width = (function_width - button_gap * 5.0) / 6.0;
        let knob_gap = 6.0;
        let knob_width = (function_width - knob_gap * 3.0) / 4.0;

        // Top left: MASTER / SELECT over PRESETS / ALGORITHM. The labels are inert; the
        // tiny - and + ends turn the control one detent.
        let top_left = [
            ("MASTER", 0u32, None),
            ("SELECT", 1, Some(0u8)),
            ("PRESETS", 2, Some(6)),
            ("ALGORITHM", 3, Some(1)),
        ];
        for (index, (label, salt, encoder)) in top_left.iter().enumerate() {
            let column = index % 2;
            let row_index = index / 2;
            let rect = Rect::from_min_size(
                Pos2::new(
                    left + column as f32 * (group_width + group_gap),
                    controls_top + row_index as f32 * (group_height + row_gap),
                ),
                Vec2::new(group_width, group_height),
            );
            let (down, up) = panel_turn_group(ui, rect, *salt, label);
            let is_master = encoder.is_none();
            let down = down.on_hover_text(if is_master {
                "Lower master"
            } else {
                "Turn counter-clockwise"
            });
            let up = up.on_hover_text(if is_master {
                "Raise master"
            } else {
                "Turn clockwise"
            });
            if down.clicked() {
                if let Some(encoder) = encoder {
                    self.turn_encoder(*encoder, -1);
                } else {
                    self.adjust_master(-1);
                }
            }
            if up.clicked() {
                if let Some(encoder) = encoder {
                    self.turn_encoder(*encoder, 1);
                } else {
                    self.adjust_master(1);
                }
            }
        }

        // Top right: KNOB 1..4.
        for index in 0..4 {
            let rect = Rect::from_min_size(
                Pos2::new(
                    function_left + index as f32 * (knob_width + knob_gap),
                    controls_top,
                ),
                Vec2::new(knob_width, group_height),
            );
            let (down, up) = panel_turn_group(ui, rect, 10 + index as u32, &format!("KNOB {}", index + 1));
            if down
                .on_hover_text("Turn counter-clockwise")
                .clicked()
            {
                self.turn_encoder(2 + index as u8, -1);
            }
            if up.on_hover_text("Turn clockwise").clicked() {
                self.turn_encoder(2 + index as u8, 1);
            }
        }

        let second_band = controls_top + group_height * 2.0 + row_gap + 8.0;

        // OCT- and OCT+ sit under the two encoder columns.
        for (index, (label, id, hint)) in [("OCT−", 0u8, "Z"), ("OCT+", 1, "X")]
            .iter()
            .enumerate()
        {
            let rect = Rect::from_min_size(
                Pos2::new(left + index as f32 * (group_width + group_gap), second_band),
                Vec2::new(group_width, group_height),
            );
            let (response, down) = panel_button_ui(
                ui,
                rect,
                *id,
                label,
                hint,
                self.held[*id as usize],
                self.leds[*id as usize] > 0.04,
                AMBER,
            );
            self.panel_down[*id as usize] = down;
            response.on_hover_text(format!("{label} ({hint})"));
        }

        // The two rows of six function buttons, with their computer-key hints and
        // indicators. PLAY uses the same green as the machine's play/pause LED.
        for (row_index, row) in [FUNCTION_ROW_1, FUNCTION_ROW_2].iter().enumerate() {
            for (column, (label, id, hint)) in row.iter().enumerate() {
                let rect = Rect::from_min_size(
                    Pos2::new(
                        function_left + column as f32 * (button_width + button_gap),
                        second_band + row_index as f32 * (group_height + row_gap),
                    ),
                    Vec2::new(button_width, group_height),
                );
                let tint = if *id == 12 { GREEN } else { AMBER };
                let (response, down) = panel_button_ui(
                    ui,
                    rect,
                    *id,
                    label,
                    hint,
                    self.held[*id as usize],
                    self.leds[*id as usize] > 0.04,
                    tint,
                );
                self.panel_down[*id as usize] = down;
                if *id == 12 {
                    response.on_hover_text("Play/Pause (Space)");
                } else {
                    response.on_hover_text(format!("{label} ({hint})"));
                }
            }
        }

        // The 27 pill keys: white keys first, then the black keys over them.
        let key_left = left;
        let key_right = right;
        let key_bottom = panel.bottom() - key_margin;
        let key_top = key_bottom - key_height;
        let white_width = (key_right - key_left) / WHITE_KEYS.len() as f32;
        for (index, id) in WHITE_KEYS.iter().enumerate() {
            let white_left = key_left + index as f32 * white_width;
            let rect = Rect::from_min_max(
                Pos2::new(white_left + 0.5, key_top),
                Pos2::new(white_left + white_width - 0.5, key_bottom),
            );
            let note = PANEL_KEYS
                .iter()
                .find(|(key, _, _)| key == id)
                .map(|(_, note, _)| *note)
                .unwrap_or("");
            let (response, down) = panel_key_ui(
                ui,
                rect,
                *id,
                true,
                note,
                panel_key_hint(*id),
                self.held[*id as usize],
                self.leds[*id as usize] > 0.04,
            );
            self.panel_down[*id as usize] = down;
            let _ = response;
        }
        let black_height = (key_bottom - key_top) * 0.62;
        for (id, after) in BLACK_KEYS {
            let center_x = key_left + (after as f32 + 1.0) * white_width;
            let rect = Rect::from_center_size(
                Pos2::new(center_x, key_top + black_height / 2.0),
                Vec2::new(white_width * 0.62, black_height),
            );
            let (response, down) = panel_key_ui(
                ui,
                rect,
                id,
                false,
                "",
                panel_key_hint(id),
                self.held[id as usize],
                self.leds[id as usize] > 0.04,
            );
            self.panel_down[id as usize] = down;
            let _ = response;
        }

        // Mouse changes were collected above; deliver them on this same frame.
        self.sync_held(ui.ctx());
    }

    /// The transport sits in a Platinum window at the bottom of the screen.
    fn transport(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("transport")
            .frame(
                egui::Frame::new()
                    .fill(PLATINUM)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0_f32, OUTLINE)),
            )
            .show(ctx, |ui| {
                self.control_display(ui);
                ui.add_space(6.0);
                let (separator, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 2.0),
                    Sense::hover(),
                );
                ui.painter().line_segment(
                    [
                        Pos2::new(separator.left(), separator.center().y),
                        Pos2::new(separator.right(), separator.center().y),
                    ],
                    Stroke::new(1.0_f32, SHADOW),
                );
                ui.painter().line_segment(
                    [
                        Pos2::new(separator.left(), separator.center().y + 1.0),
                        Pos2::new(separator.right(), separator.center().y + 1.0),
                    ],
                    Stroke::new(1.0_f32, HIGHLIGHT),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(PANEL)
                        .stroke(Stroke::new(1.0_f32, OUTLINE))
                        .corner_radius(4)
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut on = self.clock.enabled();
                                let switch = toggle_switch(ui, &mut on)
                                    .on_hover_text("Start or stop the MIDI clock");
                                if switch.clicked() {
                                    self.clock.set_enabled(on);
                                }

                                ui.add_space(4.0);
                                let running = self.clock.enabled();
                                // Fixed width: RUNNING and STOPPED have the same number of
                                // characters in a monospace font, so the tempo display never
                                // shifts by a pixel when the clock starts.
                                ui.vertical(|ui| {
                                    ui.set_width(108.0);
                                    ui.label(
                                        RichText::new("MIDI CLOCK")
                                            .strong()
                                            .size(11.0)
                                            .color(INK),
                                    );
                                    ui.label(
                                        RichText::new(if running {
                                            "RUNNING · 24 PPQN"
                                        } else {
                                            "STOPPED · 24 PPQN"
                                        })
                                        .monospace()
                                        .size(9.0)
                                        .color(if running { ACCENT } else { DIM }),
                                    );
                                });

                                ui.add_space(14.0);
                                egui::Frame::new()
                                    .fill(LCD_BG)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 40, 31)))
                                    .corner_radius(3)
                                    .inner_margin(Margin::symmetric(9, 2))
                                    .show(ui, |ui| {
                                        // The readout is also a DragValue: click and type a
                                        // tempo, or drag it. Its box is a fixed size, so the
                                        // running/stopped colour change cannot move the row.
                                        {
                                            let style = ui.style_mut();
                                            style.drag_value_text_style =
                                                TextStyle::Monospace;
                                            style.text_styles.insert(
                                                TextStyle::Monospace,
                                                FontId::monospace(24.0),
                                            );
                                            // The DragValue's edit field is sized from
                                            // interact_size; make it wide enough for 3 digits.
                                            style.spacing.interact_size =
                                                Vec2::new(66.0, 30.0);
                                            style.visuals.override_text_color =
                                                Some(if running { LCD_ON } else { LCD_OFF });
                                            style.visuals.widgets.inactive.bg_fill =
                                                Color32::TRANSPARENT;
                                            style.visuals.widgets.inactive.weak_bg_fill =
                                                Color32::TRANSPARENT;
                                            style.visuals.widgets.inactive.bg_stroke =
                                                Stroke::NONE;
                                            style.visuals.widgets.hovered.bg_fill =
                                                Color32::from_rgb(32, 44, 34);
                                            style.visuals.widgets.hovered.weak_bg_fill =
                                                Color32::from_rgb(32, 44, 34);
                                            style.visuals.widgets.hovered.bg_stroke =
                                                Stroke::new(
                                                    1.0_f32,
                                                    Color32::from_rgb(60, 90, 60),
                                                );
                                            style.visuals.widgets.active.bg_fill =
                                                Color32::from_rgb(40, 52, 42);
                                            style.visuals.widgets.active.weak_bg_fill =
                                                Color32::from_rgb(40, 52, 42);
                                            style.visuals.widgets.active.bg_stroke =
                                                Stroke::new(1.0_f32, LCD_ON);
                                        }

                                        let mut bpm = self.clock.bpm();
                                        let _ = ui.add_sized(
                                            [66.0, 30.0],
                                            egui::DragValue::new(&mut bpm)
                                                .range(MIN_BPM..=MAX_BPM)
                                                .speed(0.5)
                                                .custom_formatter(|n, _| {
                                                    format!(
                                                        "{:03}",
                                                        n.round().clamp(0.0, 999.0) as u32
                                                    )
                                                }),
                                        );
                                        if bpm != self.clock.bpm() {
                                            self.clock.set_bpm(bpm);
                                        }
                                    });
                                ui.label(RichText::new("BPM").size(9.0).color(DIM));

                                ui.add_space(2.0);
                                let (down, up) = tempo_stepper(ui);
                                if down.clicked() {
                                    self.clock.adjust_bpm(-1);
                                }
                                if up.clicked() {
                                    self.clock.adjust_bpm(1);
                                }

                                ui.add_space(12.0);
                                let lit = self.clock.beat_light();
                                let (led, _) =
                                    ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                                paint_led(ui.painter(), led, lit);
                                ui.label(
                                    RichText::new("BEAT")
                                        .size(9.0)
                                        .color(if lit { ACCENT } else { DIM }),
                                );
                            });
                        });

                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("FM-1 GRID").strong().size(11.0).color(INK));
                        ui.label(
                            RichText::new("CLICK A SLOT TO ROUTE THE KEYBOARD")
                                .size(9.0)
                                .color(DIM),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(err) = &self.audio_error {
                            ui.label(
                                RichText::new(format!("audio off: {err}")).size(9.0).color(RED),
                            );
                        }
                        ui.label(
                            RichText::new(format!("{}–{} BPM · MIDI 0xF8", MIN_BPM, MAX_BPM))
                                .size(9.0)
                                .color(DIM),
                        );
                    });
                });
            });
    }

    fn grid(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(DESKTOP).inner_margin(Margin::same(8)))
            .show(ctx, |ui| {
                let gap = ui.spacing().item_spacing.x;
                let slot_count = self.slot_count as f32;
                let width = ((ui.available_width() - gap * (slot_count - 1.0)) / slot_count)
                    .min(380.0)
                    .max(150.0);
                // Cards hug their contents instead of stretching to the full window height.
                let card_height = (width + 88.0).min(350.0).min(ui.available_height());
                let top_gap = ((ui.available_height() - card_height) / 2.0).max(0.0);
                let row_width = width * slot_count + gap * (slot_count - 1.0);
                let left_gap = ((ui.available_width() - row_width) / 2.0).max(0.0);
                ui.add_space(top_gap);
                ui.horizontal_top(|ui| {
                    ui.add_space(left_gap);
                    for slot in 0..self.slot_count {
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, card_height),
                            Layout::top_down(Align::Center),
                            |ui| {
                                ui.set_width(width);
                                self.slot_ui(ui, slot);
                            },
                        );
                    }
                });
            });
    }

    fn slot_ui(&mut self, ui: &mut egui::Ui, slot: usize) {
        let card_rect = ui.max_rect().shrink2(Vec2::splat(2.0));

        // Register the card before its children so buttons and the dropdown stay on top;
        // the pointer test also catches clicks on child widgets, i.e. anywhere in the box.
        let card = ui.interact(
            card_rect,
            ui.id().with(("slot-card", slot)),
            Sense::click(),
        );
        let clicked_inside = ui.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|pos| card_rect.contains(pos))
        });
        // Do not steal a click from an open menu that happens to overlap this card.
        let on_card_layer = ui
            .input(|i| i.pointer.interact_pos())
            .and_then(|pos| ui.ctx().layer_id_at(pos))
            .is_none_or(|layer| layer == ui.layer_id());
        if card.clicked() || (clicked_inside && on_card_layer) {
            self.set_active(slot);
        }

        let active = self.active == slot;
        let fill = if active { CARD_ACTIVE } else { CARD };
        let painter = ui.painter();
        paint_raised(painter, card_rect, CornerRadius::same(3), fill);

        // A plain Platinum title bar, with the slot name drawn over it.
        let header = Rect::from_min_size(
            Pos2::new(card_rect.left() + 2.0, card_rect.top() + 2.0),
            Vec2::new(card_rect.width() - 4.0, 23.0),
        );
        painter.rect_filled(
            header,
            CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 },
            Color32::from_rgb(207, 207, 207),
        );
        painter.line_segment(
            [
                Pos2::new(header.left(), header.bottom()),
                Pos2::new(header.right(), header.bottom()),
            ],
            Stroke::new(1.0_f32, SHADOW),
        );

        // A constant-width outline plus an inset selection ring: selecting never changes
        // the card's padding or size, so nothing jumps.
        if active {
            painter.rect_stroke(
                card_rect.shrink(1.5),
                CornerRadius::same(2),
                Stroke::new(2.0_f32, ACCENT),
                StrokeKind::Inside,
            );
        }

        let mut content = ui.new_child(
            UiBuilder::new()
                .id_salt(("slot-content", slot))
                .max_rect(card_rect.shrink2(Vec2::new(11.0, 9.0)))
                .layout(Layout::top_down(Align::Center)),
        );
        content.set_width(content.available_width());

        content.horizontal(|ui| {
            ui.label(
                RichText::new(format!("SLOT {}", slot + 1))
                    .strong()
                    .size(11.0)
                    .color(INK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if active {
                    ui.label(
                        RichText::new("KEYBOARD")
                            .strong()
                            .size(9.0)
                            .color(ACCENT),
                    );
                }
            });
        });

        let lcd_side = content.available_width().min(240.0);
        let (bezel, _) =
            content.allocate_exact_size(Vec2::splat(lcd_side + 6.0), Sense::hover());
        paint_recessed(
            content.painter(),
            bezel,
            CornerRadius::same(3),
            Color32::from_rgb(54, 54, 54),
        );
        content.put(
            bezel.shrink(3.0),
            egui::Image::new(&self.textures[slot]).fit_to_exact_size(bezel.shrink(3.0).size()),
        );

        let name = self.slots[slot]
            .label
            .clone()
            .unwrap_or_else(|| "no firmware loaded".to_string());
        let path_tip = match &self.slots[slot].source {
            Some(Source::File(path)) => Some(path.display().to_string()),
            _ => None,
        };
        let name_response =
            content.add(egui::Label::new(RichText::new(name).strong().size(12.0)).truncate());
        if let Some(path) = path_tip {
            name_response.on_hover_text(path);
        }

        let status = match (&self.slots[slot].error, &self.slots[slot].handle) {
            (Some(error), _) => error.clone(),
            (None, Some(handle)) => handle.status(),
            (None, None) => String::new(),
        };
        let status_color = if self.slots[slot].error.is_some()
            || self
                .slots[slot]
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_running())
        {
            RED
        } else {
            DIM
        };
        content.add(egui::Label::new(
            RichText::new(status).size(10.0).color(status_color),
        ).truncate());

        let current = self.slots[slot]
            .label
            .clone()
            .unwrap_or_else(|| "Choose firmware…".to_string());
        let selected_builtin = match self.slots[slot].source {
            Some(Source::Builtin(index)) => Some(index),
            _ => None,
        };
        let selected_file = matches!(self.slots[slot].source, Some(Source::File(_)));

        let mut choice = None;
        let combo = egui::ComboBox::from_id_salt(("firmware", slot))
            .selected_text(RichText::new(current).size(11.0))
            .width(content.available_width())
            .show_ui(&mut content, |ui| {
                for (index, firmware) in BUILTINS.iter().enumerate() {
                    let tip = format!("{}\nSHA-256 {}", firmware.file, firmware.sha256);
                    if ui
                        .selectable_label(selected_builtin == Some(index), firmware.label())
                        .on_hover_text(tip)
                        .clicked()
                    {
                        choice = Some(Choice::Builtin(index));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.selectable_label(selected_file, "Other…").clicked() {
                    choice = Some(Choice::Other);
                    ui.close();
                }
            });

        // A focused menu would otherwise eat Space (PLAY) on the active emulator.
        if combo.response.clicked() {
            combo.response.surrender_focus();
        }

        match choice {
            Some(Choice::Builtin(index)) => self.load_builtin(slot, index),
            Some(Choice::Other) => self.pick_file(slot),
            None => {}
        }
    }
}

impl eframe::App for GridApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_held(ctx);
        self.encoder_keys(ctx);
        self.upload_lcds();
        // Bottom panel first, then the central grid takes the rest of the window.
        self.transport(ctx);
        self.grid(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn apply_platinum_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = PLATINUM;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(3);
    visuals.extreme_bg_color = Color32::from_rgb(250, 250, 250);
    visuals.faint_bg_color = Color32::from_rgb(233, 233, 233);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.weak_text_alpha = 0.72;
    visuals.button_frame = true;
    visuals.striped = false;
    visuals.interact_cursor = None;
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [2, 3],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(70),
    };

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PLATINUM;
    widgets.noninteractive.weak_bg_fill = PLATINUM;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, SHADOW);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.noninteractive.corner_radius = CornerRadius::same(2);

    widgets.inactive.bg_fill = PANEL;
    widgets.inactive.weak_bg_fill = PANEL;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.inactive.corner_radius = CornerRadius::same(3);

    widgets.hovered.bg_fill = HOVER;
    widgets.hovered.weak_bg_fill = HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, INK);
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.hovered.corner_radius = CornerRadius::same(3);

    widgets.active.bg_fill = PRESSED;
    widgets.active.weak_bg_fill = PRESSED;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, INK);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.active.corner_radius = CornerRadius::same(3);

    widgets.open = widgets.hovered.clone();

    style.text_styles = [
        (TextStyle::Small, FontId::new(10.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(5.0, 4.0);
    style.spacing.button_padding = Vec2::new(7.0, 2.0);
    style.spacing.interact_size = Vec2::new(26.0, 20.0);
    style.spacing.menu_margin = Margin::same(4);
    style.animation_time = 0.05;

    style.visuals = visuals;
    ctx.set_style(style);
}

/// A raised Platinum surface: fill, dark 1 px outline, white top/left bevel and grey shadow.
fn paint_raised(painter: &egui::Painter, rect: Rect, radius: CornerRadius, fill: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0_f32, OUTLINE), StrokeKind::Inside);
    let r = radius.nw as f32;
    let top = rect.top() + 1.5;
    let left = rect.left() + 1.5;
    let bottom = rect.bottom() - 1.5;
    let right = rect.right() - 1.5;
    painter.line_segment(
        [Pos2::new(rect.left() + r, top), Pos2::new(rect.right() - r, top)],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
    painter.line_segment(
        [Pos2::new(left, rect.top() + r), Pos2::new(left, rect.bottom() - r)],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
    painter.line_segment(
        [Pos2::new(rect.left() + r, bottom), Pos2::new(rect.right() - r, bottom)],
        Stroke::new(1.0_f32, SHADOW),
    );
    painter.line_segment(
        [Pos2::new(right, rect.top() + r), Pos2::new(right, rect.bottom() - r)],
        Stroke::new(1.0_f32, SHADOW),
    );
}

/// A sunken Platinum surface: fill, dark outline and a shadow on the top/left inside edge.
fn paint_recessed(painter: &egui::Painter, rect: Rect, radius: CornerRadius, fill: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0_f32, OUTLINE), StrokeKind::Inside);
    let r = radius.nw as f32;
    let top = rect.top() + 1.0;
    let left = rect.left() + 1.0;
    let bottom = rect.bottom() - 1.0;
    let right = rect.right() - 1.0;
    painter.line_segment(
        [Pos2::new(rect.left() + r, top), Pos2::new(rect.right() - r, top)],
        Stroke::new(1.0_f32, Color32::from_rgb(36, 36, 36)),
    );
    painter.line_segment(
        [Pos2::new(left, rect.top() + r), Pos2::new(left, rect.bottom() - r)],
        Stroke::new(1.0_f32, Color32::from_rgb(36, 36, 36)),
    );
    painter.line_segment(
        [Pos2::new(rect.left() + r, bottom), Pos2::new(rect.right() - r, bottom)],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
    painter.line_segment(
        [Pos2::new(right, rect.top() + r), Pos2::new(right, rect.bottom() - r)],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
}

fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
    )
}

/// A three-part rotary group: an inert label in the middle and clickable - / + ends.
fn panel_turn_group(
    ui: &mut egui::Ui,
    rect: Rect,
    salt: u32,
    label: &str,
) -> (egui::Response, egui::Response) {
    let arrow = 22.0;
    let down_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + arrow, rect.bottom()));
    let up_rect = Rect::from_min_max(Pos2::new(rect.right() - arrow, rect.top()), rect.max);
    let down = ui.interact(
        down_rect,
        ui.id().with(("panel-turn-down", salt)),
        Sense::click(),
    );
    let up = ui.interact(
        up_rect,
        ui.id().with(("panel-turn-up", salt)),
        Sense::click(),
    );

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), PANEL_BUTTON);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        StrokeKind::Inside,
    );

    let left_corners = CornerRadius { nw: 3, sw: 3, ne: 0, se: 0 };
    let right_corners = CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 };
    for (response, half, corners) in [
        (&down, down_rect, left_corners),
        (&up, up_rect, right_corners),
    ] {
        if response.is_pointer_button_down_on() {
            painter.rect_filled(
                half.shrink(1.0),
                corners,
                mix_color(PANEL_BUTTON, AMBER, 0.3),
            );
        } else if response.hovered() {
            painter.rect_filled(half.shrink(1.0), corners, Color32::from_rgb(78, 81, 88));
        }
    }

    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.top() + 1.5),
            Pos2::new(rect.right() - 3.0, rect.top() + 1.5),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(88, 91, 98)),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.bottom() - 1.5),
            Pos2::new(rect.right() - 3.0, rect.bottom() - 1.5),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(28, 28, 30)),
    );
    for x in [rect.left() + arrow, rect.right() - arrow] {
        painter.line_segment(
            [
                Pos2::new(x, rect.top() + 3.0),
                Pos2::new(x, rect.bottom() - 3.0),
            ],
            Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        );
    }

    painter.text(
        down_rect.center(),
        Align2::CENTER_CENTER,
        "−",
        FontId::proportional(15.0),
        SILK,
    );
    painter.text(
        up_rect.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(15.0),
        SILK,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        SILK,
    );
    (
        down.on_hover_cursor(egui::CursorIcon::PointingHand),
        up.on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

/// A clickable front-panel button. Returns its response and whether the mouse is holding it.
#[allow(clippy::too_many_arguments)]
fn panel_button_ui(
    ui: &mut egui::Ui,
    rect: Rect,
    id: u8,
    label: &str,
    hint: &str,
    held: bool,
    led: bool,
    tint: Color32,
) -> (egui::Response, bool) {
    let response = ui.interact(rect, ui.id().with(("panel-btn", id)), Sense::click());
    let mut down = response.is_pointer_button_down_on();
    // A very fast click can begin and end between two frames; keep it down for one frame.
    if response.clicked() {
        down = true;
    }
    paint_panel_button(
        ui.painter(),
        rect,
        label,
        hint,
        down || held || led,
        tint,
    );
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        down,
    )
}

/// A clickable note key. Returns its response and whether the mouse is holding it.
fn panel_key_ui(
    ui: &mut egui::Ui,
    rect: Rect,
    id: u8,
    white: bool,
    note: &str,
    hint: &str,
    held: bool,
    led: bool,
) -> (egui::Response, bool) {
    let response = ui.interact(rect, ui.id().with(("panel-key", id)), Sense::click());
    let mut down = response.is_pointer_button_down_on();
    if response.clicked() {
        down = true;
    }
    paint_panel_key(
        ui.painter(),
        rect,
        white,
        note,
        hint,
        down || held || led,
    );
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        down,
    )
}

/// A keycap on the miniature FM-1 panel. `lit` is the pressed/hold state; the small
/// indicator beside the label uses the button's own colour (green for PLAY, amber
/// otherwise).
fn paint_panel_button(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    hint: &str,
    lit: bool,
    tint: Color32,
) {
    let fill = if lit {
        mix_color(PANEL_BUTTON, tint, 0.45)
    } else {
        PANEL_BUTTON
    };
    let ink = if lit {
        Color32::from_rgb(34, 26, 10)
    } else {
        SILK
    };
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.top() + 1.5),
            Pos2::new(rect.right() - 3.0, rect.top() + 1.5),
        ],
        Stroke::new(1.0_f32, if lit { tint } else { Color32::from_rgb(88, 91, 98) }),
    );

    let led_center = Pos2::new(rect.left() + 9.0, rect.center().y);
    painter.circle_filled(led_center, 4.0, PANEL_BUTTON_EDGE);
    painter.circle_filled(
        led_center,
        3.0,
        if lit { tint } else { Color32::from_rgb(48, 51, 56) },
    );
    if lit {
        painter.circle_filled(
            Pos2::new(led_center.x - 0.8, led_center.y - 0.9),
            1.0,
            Color32::from_rgb(255, 245, 220),
        );
    }

    painter.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(9.5),
        ink,
    );
    painter.text(
        Pos2::new(rect.right() - 6.0, rect.center().y),
        Align2::RIGHT_CENTER,
        hint,
        FontId::monospace(8.5),
        if lit {
            Color32::from_rgb(60, 48, 24)
        } else {
            SILK_DIM
        },
    );
    if lit {
        painter.rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(4),
            Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 80),
            ),
            StrokeKind::Middle,
        );
    }
}

/// One note key on the miniature panel: ivory/black at rest, amber while held.
fn paint_panel_key(
    painter: &egui::Painter,
    rect: Rect,
    white: bool,
    note: &str,
    hint: &str,
    lit: bool,
) {
    if white {
        painter.rect_filled(
            rect,
            CornerRadius::same(1),
            if lit { AMBER } else { KEY_WHITE },
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, Color32::from_rgb(112, 107, 96)),
            StrokeKind::Inside,
        );
        let ink = if lit {
            Color32::from_rgb(44, 27, 4)
        } else {
            Color32::from_rgb(45, 43, 40)
        };
        if !hint.is_empty() {
            painter.text(
                Pos2::new(rect.center().x, rect.top() + rect.height() * 0.55),
                Align2::CENTER_CENTER,
                hint,
                FontId::proportional(16.0),
                ink,
            );
        }
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_CENTER,
            note,
            FontId::proportional(10.0),
            if lit {
                ink
            } else {
                Color32::from_rgb(96, 91, 82)
            },
        );
    } else {
        painter.rect_filled(
            rect,
            CornerRadius::same(1),
            if lit { AMBER } else { KEY_BLACK },
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, Color32::from_rgb(18, 18, 20)),
            StrokeKind::Inside,
        );
        let ink = if lit {
            Color32::from_rgb(44, 27, 4)
        } else {
            Color32::from_rgb(214, 211, 202)
        };
        if !hint.is_empty() {
            painter.text(
                Pos2::new(rect.center().x, rect.center().y),
                Align2::CENTER_CENTER,
                hint,
                FontId::proportional(14.0),
                ink,
            );
        }
    }
}

/// The computer key that plays a panel key, if it has one.
fn panel_key_hint(id: u8) -> &'static str {
    match id {
        21 => "A",
        22 => "W",
        23 => "S",
        24 => "E",
        25 => "D",
        26 => "F",
        27 => "T",
        28 => "G",
        29 => "Y",
        30 => "H",
        31 => "U",
        32 => "J",
        33 => "K",
        34 => "O",
        35 => "L",
        36 => "P",
        37 => ";",
        38 => "'",
        _ => "",
    }
}

/// A Platinum rocker switch. Returns the response; toggles `on` when clicked.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(48.0, 24.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    let painter = ui.painter();
    paint_recessed(
        painter,
        rect,
        CornerRadius::same(4),
        Color32::from_rgb(178, 178, 178),
    );
    if *on {
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(3),
            Color32::from_rgb(201, 213, 234),
        );
    }

    let knob_width = 18.0;
    let knob_x = if *on {
        rect.right() - knob_width - 3.0
    } else {
        rect.left() + 3.0
    };
    let knob = Rect::from_min_size(
        Pos2::new(knob_x, rect.top() + 3.0),
        Vec2::new(knob_width, rect.height() - 6.0),
    );
    paint_raised(
        painter,
        knob,
        CornerRadius::same(3),
        if *on {
            Color32::from_rgb(243, 246, 251)
        } else {
            Color32::from_rgb(234, 234, 234)
        },
    );

    response
}

/// The two-segment tempo control: `[ − ][ + ]`, one grouped Platinum button.
/// Returns `(down, up)` responses.
fn tempo_stepper(ui: &mut egui::Ui) -> (egui::Response, egui::Response) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(58.0, 26.0), Sense::hover());
    let middle = rect.center().x;
    let down_rect = Rect::from_min_max(rect.min, Pos2::new(middle, rect.max.y));
    let up_rect = Rect::from_min_max(Pos2::new(middle, rect.min.y), rect.max);

    let down = ui.interact(down_rect, ui.id().with("tempo-down"), Sense::click());
    let up = ui.interact(up_rect, ui.id().with("tempo-up"), Sense::click());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), PANEL);

    let left_corners = CornerRadius { nw: 3, sw: 3, ne: 0, se: 0 };
    let right_corners = CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 };
    for (response, half, corners) in [
        (&down, down_rect, left_corners),
        (&up, up_rect, right_corners),
    ] {
        if response.is_pointer_button_down_on() {
            painter.rect_filled(half.shrink(1.0), corners, PRESSED);
        } else if response.hovered() {
            painter.rect_filled(half.shrink(1.0), corners, HOVER);
        }
    }

    // One outline around both halves, then the divider: a single segmented control.
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, OUTLINE),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.top() + 1.5),
            Pos2::new(rect.right() - 3.0, rect.top() + 1.5),
        ],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 1.5, rect.top() + 3.0),
            Pos2::new(rect.left() + 1.5, rect.bottom() - 3.0),
        ],
        Stroke::new(1.0_f32, HIGHLIGHT),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.bottom() - 1.5),
            Pos2::new(rect.right() - 3.0, rect.bottom() - 1.5),
        ],
        Stroke::new(1.0_f32, SHADOW),
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 1.5, rect.top() + 3.0),
            Pos2::new(rect.right() - 1.5, rect.bottom() - 3.0),
        ],
        Stroke::new(1.0_f32, SHADOW),
    );
    painter.line_segment(
        [
            Pos2::new(middle, rect.top() + 2.0),
            Pos2::new(middle, rect.bottom() - 2.0),
        ],
        Stroke::new(1.0_f32, OUTLINE),
    );

    painter.text(
        down_rect.center(),
        Align2::CENTER_CENTER,
        "−",
        FontId::proportional(16.0),
        INK,
    );
    painter.text(
        up_rect.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(16.0),
        INK,
    );

    (down, up)
}

fn paint_led(painter: &egui::Painter, rect: Rect, lit: bool) {
    let center = rect.center();
    painter.circle_filled(center, 8.0, Color32::from_rgb(96, 96, 96));
    painter.circle_stroke(center, 8.0, Stroke::new(1.0_f32, Color32::from_rgb(48, 48, 48)));
    painter.circle_filled(center, 6.0, if lit { LED_ON } else { LED_OFF });
    painter.circle_stroke(center, 6.0, Stroke::new(1.0_f32, OUTLINE));
    if lit {
        painter.circle_filled(
            Pos2::new(center.x - 1.8, center.y - 1.8),
            2.0,
            Color32::from_rgb(214, 255, 220),
        );
    }
}
