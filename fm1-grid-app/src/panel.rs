//! The miniature FM-1 front panel at the bottom of the Emulator tab: a working control
//! surface whose keys, buttons and knob ends are clickable, whose knobs also turn with
//! the mouse wheel, and whose LEDs follow both the computer keyboard and the emulated
//! firmware's own LED state. The layout is fixed geometry from the FM-1 control map
//! ([`fm1_performer::keys`]), drawn with absolutely positioned elements; the panel's rendered
//! width is recorded during prepaint so the geometry tracks the window.

use crate::app::GridApp;
use crate::theme::mix_hsla;
use fm1_performer::keys::{
    panel_key_hint, BLACK_KEYS, FUNCTION_ROW_1, FUNCTION_ROW_2, PANEL_KEYS, WHITE_KEYS,
};
use gpui_kit::base::ElementExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

const KEY_HEIGHT: f32 = 120.0;
/// What the compact keyboard sheds when the window is too short for the full stack.
const KEY_COMPACT_LOSS: f32 = 32.0;
const KEY_MARGIN: f32 = 6.0;
/// The shaded depth strip at the bottom of each key.
const WHITE_KEY_DEPTH: f32 = 10.0;
/// The membrane window near the top of a key.
const KEY_LED_INSET: f32 = 0.36;
const BLACK_KEY_WIDTH_FRAC: f32 = 0.62;
const BLACK_KEY_HEIGHT_FRAC: f32 = 0.62;
const WHITE_LED_HEIGHT: f32 = 19.5;
const BLACK_LED_HEIGHT: f32 = 18.5;
const KEY_LABEL_TOP: f32 = 0.0;
const KEY_LABEL_GAP: f32 = 12.0;
const WHITE_HINT_SIZE: f32 = 16.0;
const BLACK_HINT_SIZE: f32 = 14.0;
const KEY_NOTE_SIZE: f32 = 10.0;
/// Key corner rounding.
const KEY_RADIUS: f32 = 1.0;
/// Firmware LED level that counts as lit.
const LED_LIT_THRESHOLD: f32 = 0.04;
/// The lit lip just above a white key's shade strip.
const WHITE_KEY_LIP: f32 = 2.0;
const BLACK_KEY_DEPTH: f32 = 6.0;
/// The panel's height above the key bed: header, control rows and margins.
const PANEL_CONTROLS_H: f32 = 156.0;
const GROUP_HEIGHT: f32 = 24.0;
/// Panel content margins and control geometry.
const PANEL_SIDE_MARGIN: f32 = 14.0;
const PANEL_CONTROLS_TOP: f32 = 24.0;
const PANEL_HEADER_TOP: f32 = 7.0;
const PANEL_HEADER_SIZE: f32 = 9.5;
const GROUP_WIDTH: f32 = 120.0;
const GROUP_GAP: f32 = 6.0;
const CONTROL_ROW_GAP: f32 = 4.0;
const CONTROL_GROUP_COLUMNS: f32 = 2.0;
const CONTROL_GROUP_ROWS: f32 = 2.0;
const SECOND_BAND_GAP: f32 = 8.0;
const FUNCTION_SIDE_GAP: f32 = 12.0;
const FUNCTION_SHARE: f32 = 0.52;
const FUNCTION_MIN_W: f32 = 360.0;
const FUNCTION_MAX_W: f32 = 520.0;
const FUNCTION_BUTTON_GAP: f32 = 4.0;
const FUNCTION_BUTTONS: f32 = 6.0;
const KNOB_GAP: f32 = 6.0;
const KNOBS: f32 = 4.0;
const KEY_ROW_GAP: f32 = 1.0;
const PANEL_RADIUS: f32 = 8.0;
const PANEL_WIDTH_EPSILON: f32 = 0.5;
const KNOB_ARROW_W: f32 = 22.0;
const KNOB_RADIUS: f32 = 3.0;
const KNOB_END_SIZE: f32 = 13.0;
const KNOB_LABEL_SIZE: f32 = 9.5;
const KNOB_DIVIDER_W: f32 = 1.0;
const WHEEL_PIXELS_PER_DETENT: f32 = 40.0;
const BUTTON_RADIUS: f32 = 4.0;
const BUTTON_GAP: f32 = 6.0;
const BUTTON_PAD_X: f32 = 6.0;
const BUTTON_LED_SIZE: f32 = 7.0;
const BUTTON_LABEL_SIZE: f32 = 9.5;
const BUTTON_HINT_SIZE: f32 = 8.5;
const BUTTON_TINT_MIX: f32 = 0.45;
/// The panel never grows past the card grid's width.
const PANEL_MAX_W: f32 = 1040.0;
/// The smallest the panel lays out at; a narrower window clips the right edge.
const PANEL_MIN_W: f32 = 320.0;

mod paint;
use paint::{
    key_strip, led_bottom, led_window, panel_colors, strip_colors, strip_height, PanelColors,
};

/// What a three-part rotary group turns: the MASTER pot or one of the FM-1's encoders.
#[derive(Clone, Copy)]
enum KnobTarget {
    Master,
    Enc(u8),
}

impl GridApp {
    /// A miniature FM-1 front panel. It is a working control surface: note keys and
    /// buttons can be clicked, the knobs are turned with the -/+ ends or the mouse
    /// wheel, and both the computer keyboard and the emulated firmware's LEDs light
    /// the indicators. `compact` shortens the keyboard by [`KEY_COMPACT_LOSS`] when
    /// the window is too short for the full-height stack.
    pub fn control_display(&mut self, cx: &mut Context<Self>, compact: bool) -> impl IntoElement {
        let key_height = if compact {
            KEY_HEIGHT - KEY_COMPACT_LOSS
        } else {
            KEY_HEIGHT
        };
        let panel_h = PANEL_CONTROLS_H + key_height;
        // The last measured width drives the absolute geometry; the element itself
        // stays full-width, so narrowing and widening the window both take effect.
        let w = self.panel_width.clamp(PANEL_MIN_W, PANEL_MAX_W);
        let left = PANEL_SIDE_MARGIN;
        let right = w - PANEL_SIDE_MARGIN;
        let controls_top = PANEL_CONTROLS_TOP;
        let group_width = GROUP_WIDTH;
        let group_gap = GROUP_GAP;
        let row_gap = CONTROL_ROW_GAP;
        let mono = cx.theme().mono_font_family.clone();
        let visualia = (self.theme_id == crate::theme::VISUALIA).then(|| {
            crate::themes::visualia::panel(
                crate::themes::accent(&self.accent),
                cx.theme().is_dark(),
            )
        });
        let colors = panel_colors(cx.theme(), visualia);

        // The right-hand groups (the four knobs and the function buttons) span one
        // common width, so the knobs sit directly above the button grid. Cap it to
        // what the two left group columns leave, or narrow windows overlap them.
        let left_columns = left + CONTROL_GROUP_COLUMNS * group_width + group_gap;
        let max_function_width = (right - left_columns - FUNCTION_SIDE_GAP).max(0.0);
        let function_width = (w * FUNCTION_SHARE)
            .clamp(FUNCTION_MIN_W, FUNCTION_MAX_W)
            .min(max_function_width);
        let function_left = right - function_width;
        let button_gap = FUNCTION_BUTTON_GAP;
        let button_width =
            (function_width - button_gap * (FUNCTION_BUTTONS - 1.0)) / FUNCTION_BUTTONS;
        let knob_gap = KNOB_GAP;
        let knob_width = (function_width - knob_gap * (KNOBS - 1.0)) / KNOBS;

        let app = cx.entity().clone();
        let mut panel = div()
            .id("fm1-panel")
            .test_support()
            .relative()
            .w_full()
            .max_w(px(PANEL_MAX_W))
            .h(px(panel_h))
            .rounded(px(PANEL_RADIUS))
            .overflow_hidden()
            .bg(colors.bed)
            .on_prepaint(move |bounds, _, cx| {
                let width: f32 = bounds.size.width.into();
                app.update(cx, |this, cx| {
                    if (this.panel_width - width).abs() > PANEL_WIDTH_EPSILON {
                        this.panel_width = width;
                        cx.notify();
                    }
                });
            })
            // The header line.
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .right(px(left))
                    .top(px(PANEL_HEADER_TOP))
                    .text_size(px(PANEL_HEADER_SIZE))
                    .text_color(colors.silk_dim)
                    .child("FM-1 PANEL"),
            );

        // Top left: MASTER / SELECT over PRESETS / ALGORITHM.
        let top_left = [
            ("MASTER", 0u32, KnobTarget::Master),
            ("SELECT", 1, KnobTarget::Enc(0)),
            ("PRESETS", 2, KnobTarget::Enc(6)),
            ("ALGORITHM", 3, KnobTarget::Enc(1)),
        ];
        for (index, (label, salt, target)) in top_left.iter().enumerate() {
            let column = (index % 2) as f32;
            let row = (index / 2) as f32;
            let x = left + column * (group_width + group_gap);
            let y = controls_top + row * (GROUP_HEIGHT + row_gap);
            panel =
                panel.child(self.turn_group(cx, x, y, group_width, *salt, label, *target, &colors));
        }

        // Top right: KNOB 1..4.
        for index in 0..4u32 {
            let x = function_left + index as f32 * (knob_width + knob_gap);
            panel = panel.child(self.turn_group(
                cx,
                x,
                controls_top,
                knob_width,
                10 + index,
                &format!("KNOB {}", index + 1),
                KnobTarget::Enc(2 + index as u8),
                &colors,
            ));
        }

        let second_band =
            controls_top + GROUP_HEIGHT * CONTROL_GROUP_ROWS + row_gap + SECOND_BAND_GAP;

        // OCT- and OCT+ sit under the two encoder columns.
        for (index, (label, id, hint)) in [("OCT−", 0u8, "Z"), ("OCT+", 1, "X")].iter().enumerate()
        {
            let x = left + index as f32 * (group_width + group_gap);
            panel = panel.child(self.panel_button(
                cx,
                x,
                second_band,
                group_width,
                *id,
                label,
                hint,
                colors.amber,
                &mono,
                &colors,
            ));
        }

        // The two rows of six function buttons, with their computer-key hints and
        // indicators. PLAY's LED takes the theme's success colour, the rest warning.
        for (row_index, row) in [FUNCTION_ROW_1, FUNCTION_ROW_2].iter().enumerate() {
            for (column, (label, id, hint)) in row.iter().enumerate() {
                let x = function_left + column as f32 * (button_width + button_gap);
                let y = second_band + row_index as f32 * (GROUP_HEIGHT + row_gap);
                let tint = if *id == 12 {
                    colors.green
                } else {
                    colors.amber
                };
                panel = panel.child(self.panel_button(
                    cx,
                    x,
                    y,
                    button_width,
                    *id,
                    label,
                    hint,
                    tint,
                    &mono,
                    &colors,
                ));
            }
        }

        // The 27 pill keys: a row of white keys, then the black keys over them.
        let key_left = left;
        let key_right = right;
        let key_bottom = panel_h - KEY_MARGIN;
        let key_top = key_bottom - key_height;
        let white_width = (key_right - key_left) / WHITE_KEYS.len() as f32;
        panel = panel.child(
            div()
                .absolute()
                .left(px(key_left))
                .top(px(key_top))
                .w(px(key_right - key_left))
                .h(px(key_height))
                .flex()
                .flex_row()
                .gap(px(KEY_ROW_GAP))
                .children(
                    WHITE_KEYS
                        .iter()
                        .map(|&id| self.white_key(cx, id, key_height, &colors)),
                ),
        );
        let black_height = key_height * BLACK_KEY_HEIGHT_FRAC;
        for &(id, after) in BLACK_KEYS.iter() {
            let center_x = key_left + (after as f32 + 1.0) * white_width;
            let width = white_width * BLACK_KEY_WIDTH_FRAC;
            panel = panel.child(self.black_key(
                cx,
                id,
                center_x - width / 2.0,
                key_top,
                width,
                black_height,
                &colors,
            ));
        }

        div().w_full().flex().justify_center().child(panel)
    }

    /// Accumulate wheel detents per knob group: a wheel mouse reports whole notches, a
    /// touchpad reports points (~40 per detent), so the fractions carry over between
    /// events. Returns the whole detents to turn by.
    fn wheel_detents(&mut self, group: u32, detents: f32) -> i32 {
        if self.wheel_acc.0 != group {
            self.wheel_acc = (group, 0.0);
        }
        self.wheel_acc.1 += detents;
        let whole = self.wheel_acc.1.trunc() as i32;
        if whole != 0 {
            self.wheel_acc.1 -= whole as f32;
        }
        whole
    }

    fn turn_target(&mut self, target: KnobTarget, clicks: i32) {
        match target {
            KnobTarget::Master => self.adjust_master(clicks),
            KnobTarget::Enc(enc) => self.turn_encoder(enc, clicks),
        }
    }

    /// A three-part rotary group: an inert label in the middle and clickable - / +
    /// ends. The knob's mouse-wheel input: while the pointer is over the group, each
    /// wheel click turns it one detent — up clockwise, down counter-clockwise.
    #[allow(clippy::too_many_arguments)]
    fn turn_group(
        &self,
        cx: &mut Context<Self>,
        x: f32,
        y: f32,
        width: f32,
        salt: u32,
        label: &str,
        target: KnobTarget,
        colors: &PanelColors,
    ) -> impl IntoElement {
        let arrow = KNOB_ARROW_W;
        let (silk, hover, button, edge) = (
            colors.silk,
            colors.button_hover,
            colors.button,
            colors.button_edge,
        );
        let end = |salt_end: &'static str, glyph: &'static str, clicks: i32| {
            div()
                .id((salt_end, salt as usize))
                .w(px(arrow))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(KNOB_END_SIZE))
                .text_color(silk)
                .hover(move |style| style.bg(hover))
                .child(glyph)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.turn_target(target, clicks);
                        cx.notify();
                    }),
                )
        };
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(GROUP_HEIGHT))
            .rounded(px(KNOB_RADIUS))
            .bg(button)
            .border_1()
            .border_color(edge)
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                let detents = match event.delta {
                    ScrollDelta::Lines(point) => point.y,
                    ScrollDelta::Pixels(point) => f32::from(point.y) / WHEEL_PIXELS_PER_DETENT,
                };
                let steps = this.wheel_detents(salt, detents);
                if steps != 0 {
                    this.turn_target(target, steps);
                    cx.notify();
                }
            }))
            .child(end("turn-down", "−", -1))
            .child(div().w(px(KNOB_DIVIDER_W)).h_full().bg(edge))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .truncate()
                    .text_size(px(KNOB_LABEL_SIZE))
                    .text_color(silk)
                    .child(label.to_string()),
            )
            .child(div().w(px(KNOB_DIVIDER_W)).h_full().bg(edge))
            .child(end("turn-up", "+", 1))
    }

    /// A clickable front-panel button with its LED and computer-key hint. Lit follows
    /// the keyboard, the mouse and the firmware's own LED state.
    #[allow(clippy::too_many_arguments)]
    fn panel_button(
        &self,
        cx: &mut Context<Self>,
        x: f32,
        y: f32,
        width: f32,
        id: u8,
        label: &str,
        hint: &str,
        tint: Hsla,
        mono: &SharedString,
        colors: &PanelColors,
    ) -> impl IntoElement {
        let lit = self.held[id as usize]
            || self.leds[id as usize] > LED_LIT_THRESHOLD
            || self.panel_down[id as usize];
        let fill = if lit {
            mix_hsla(colors.button, tint, BUTTON_TINT_MIX)
        } else {
            colors.button
        };
        let ink = if lit { colors.lit_label } else { colors.silk };
        let hint_color = if lit {
            colors.lit_hint
        } else {
            colors.silk_dim
        };
        let button_hover = colors.button_hover;
        let mono = mono.clone();
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(GROUP_HEIGHT))
            .id(("panel-btn", id as usize))
            .test_support()
            .rounded(px(BUTTON_RADIUS))
            .overflow_hidden()
            .bg(fill)
            .border_1()
            .border_color(if lit { tint } else { colors.button_edge })
            .flex()
            .flex_row()
            .items_center()
            .gap(px(BUTTON_GAP))
            .px(px(BUTTON_PAD_X))
            .cursor_pointer()
            .hover(
                move |style| {
                    if lit {
                        style
                    } else {
                        style.bg(button_hover)
                    }
                },
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.panel_down[id as usize] = true;
                    this.sync_held();
                    cx.notify();
                }),
            )
            // The LED: the theme's tint while lit, a dark step of the bed otherwise.
            .child(
                div()
                    .w(px(BUTTON_LED_SIZE))
                    .h(px(BUTTON_LED_SIZE))
                    .rounded_full()
                    .flex_none()
                    .bg(if lit { tint } else { colors.led_off })
                    .border_1()
                    .border_color(colors.button_edge),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(BUTTON_LABEL_SIZE))
                    .text_color(ink)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(px(BUTTON_HINT_SIZE))
                    .font_family(mono)
                    .text_color(hint_color)
                    .child(hint.to_string()),
            )
    }

    /// A white piano key: the theme's light step at rest, the theme's accent while
    /// held; hint below the LED window, note name at the bottom.
    fn white_key(
        &self,
        cx: &mut Context<Self>,
        id: u8,
        height: f32,
        colors: &PanelColors,
    ) -> impl IntoElement {
        let pressed = self.held[id as usize] || self.panel_down[id as usize];
        let lit = pressed || self.leds[id as usize] > LED_LIT_THRESHOLD;
        let note = PANEL_KEYS
            .iter()
            .find(|(key, _, _)| *key == id)
            .map(|(_, note, _)| *note)
            .unwrap_or("");
        let hint = panel_key_hint(id);
        let (key_fill, ink, note_ink) = (colors.white, colors.white_ink, colors.white_note);
        let depth = strip_height(WHITE_KEY_DEPTH, pressed);
        let led_height = WHITE_LED_HEIGHT;
        let led_top = height - led_bottom(depth, WHITE_KEY_LIP) - led_height;
        let note_top = KEY_LABEL_TOP + WHITE_HINT_SIZE + KEY_LABEL_GAP;
        let lip = strip_colors(key_fill, colors.dark).1;
        div()
            .id(("panel-key", id as usize))
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .rounded(px(KEY_RADIUS))
            .bg(key_fill)
            .border_1()
            .border_color(colors.white_edge)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.panel_down[id as usize] = true;
                    this.sync_held();
                    cx.notify();
                }),
            )
            .child(led_window(
                key_fill,
                colors.lit,
                colors.white_edge,
                lit,
                colors.dark,
                led_top,
                led_height,
                (KEY_LED_INSET, KEY_LED_INSET),
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(KEY_LABEL_TOP))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(WHITE_HINT_SIZE))
                            .text_color(ink)
                            .child(hint),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(note_top))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(KEY_NOTE_SIZE))
                            .text_color(note_ink)
                            .child(note),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(depth))
                    .h(px(WHITE_KEY_LIP))
                    .bg(lip),
            )
            .child(key_strip(key_fill, pressed, WHITE_KEY_DEPTH, colors.dark))
    }

    /// A black piano key, overlapping the white row: the theme's dark steps, the
    /// accent while held.
    #[allow(clippy::too_many_arguments)]
    fn black_key(
        &self,
        cx: &mut Context<Self>,
        id: u8,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        colors: &PanelColors,
    ) -> impl IntoElement {
        let pressed = self.held[id as usize] || self.panel_down[id as usize];
        let lit = pressed || self.leds[id as usize] > LED_LIT_THRESHOLD;
        let hint = panel_key_hint(id);
        let (fill, ink) = (colors.black, colors.black_ink);
        let led_height = BLACK_LED_HEIGHT;
        let led_top = height - led_bottom(strip_height(BLACK_KEY_DEPTH, pressed), 0.0) - led_height;
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(height))
            .id(("panel-key", id as usize))
            .rounded(px(KEY_RADIUS))
            .bg(fill)
            .border_1()
            .border_color(colors.black_edge)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.panel_down[id as usize] = true;
                    this.sync_held();
                    // A black key sits on top of a white one: the press must not fall
                    // through to the key beneath.
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            // Keep the strip under one wrapper: GPUI's flex row mislays the black
            // keys when an absolute child is added to the key directly.
            .child(
                div()
                    .size_full()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(led_window(
                        fill,
                        colors.lit,
                        colors.black_edge,
                        lit,
                        colors.dark,
                        led_top,
                        led_height,
                        (KEY_LED_INSET, KEY_LED_INSET),
                    ))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(KEY_LABEL_TOP))
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(BLACK_HINT_SIZE))
                                    .text_color(ink)
                                    .child(hint),
                            ),
                    )
                    .child(key_strip(fill, pressed, BLACK_KEY_DEPTH, colors.dark)),
            )
    }
}

#[cfg(test)]
mod tests;
