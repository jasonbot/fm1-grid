//! The miniature FM-1 front panel at the bottom of the Emulator tab: a working control
//! surface whose keys, buttons and knob ends are clickable, whose knobs also turn with
//! the mouse wheel, and whose LEDs follow both the computer keyboard and the emulated
//! firmware's own LED state. The layout is fixed geometry from the FM-1 control map
//! ([`fm1_performer::keys`]), drawn with absolutely positioned elements; the panel's rendered
//! width is recorded during prepaint so the geometry tracks the window.

use crate::app::GridApp;
use fm1_performer::keys::{
    BLACK_KEYS, FUNCTION_ROW_1, FUNCTION_ROW_2, PANEL_KEYS, WHITE_KEYS, panel_key_hint,
};
use crate::theme::mix_hsla;
use gpui_kit::base::{ElementExt, v_flex};
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

const KEY_HEIGHT: f32 = 120.0;
/// What the compact keyboard sheds when the window is too short for the full stack.
const KEY_COMPACT_LOSS: f32 = 32.0;
const KEY_MARGIN: f32 = 6.0;
/// The shaded depth strip at the bottom of each key.
const WHITE_KEY_DEPTH: f32 = 10.0;
/// The membrane window near the top of a key.
const KEY_LED_INSET: f32 = 0.36;
const KEY_LED_TOP: f32 = 1.0;
const KEY_LED_HEIGHT: f32 = 45.0;
const KEY_LED_RADIUS: f32 = 6.0;
/// The unpressed window sits a little higher; pressing sinks it.
const KEY_LED_DEPTH: f32 = 3.0;
const KEY_LED_FILL_ALPHA: f32 = 0.16;
/// The lit lip just above a white key's shade strip.
const WHITE_KEY_LIP: f32 = 2.0;
const BLACK_KEY_DEPTH: f32 = 6.0;
/// The panel's height above the key bed: header, control rows and margins.
const PANEL_CONTROLS_H: f32 = 156.0;
const GROUP_HEIGHT: f32 = 24.0;
/// The panel never grows past the card grid's width.
const PANEL_MAX_W: f32 = 1040.0;
/// The smallest the panel lays out at; a narrower window clips the right edge.
const PANEL_MIN_W: f32 = 320.0;

/// The panel's colorway, derived from the active theme each render: the bed keeps
/// the theme background's hue but stays dark (a light theme still gets a dark
/// panel), the keys and controls are the same hue family stepped out in lightness,
/// and the lit/accent colours are the theme's own (primary for held keys, warning
/// for amber LEDs, success for the play LED).
struct PanelColors {
    /// Dark theme: reverse the key shading.
    dark: bool,
    bed: Hsla,
    button: Hsla,
    button_hover: Hsla,
    button_edge: Hsla,
    silk: Hsla,
    silk_dim: Hsla,
    led_off: Hsla,
    amber: Hsla,
    green: Hsla,
    lit: Hsla,
    lit_ink: Hsla,
    lit_label: Hsla,
    lit_hint: Hsla,
    white: Hsla,
    white_edge: Hsla,
    white_ink: Hsla,
    white_note: Hsla,
    black: Hsla,
    black_edge: Hsla,
    black_ink: Hsla,
}

/// A colour at a new lightness and a share of its saturation, hue kept.
fn relight(color: Hsla, l: f32, s_scale: f32) -> Hsla {
    Hsla {
        h: color.h,
        s: (color.s * s_scale).clamp(0.0, 1.0),
        l,
        a: 1.0,
    }
}

fn panel_colors(
    theme: &gpui_kit::component::Theme,
    visualia: Option<crate::themes::visualia::PanelPalette>,
) -> PanelColors {
    let bg = theme.background;
    let mut colors = PanelColors {
        dark: theme.is_dark(),
        bed: Hsla {
            l: bg.l.min(0.16),
            s: (bg.s * 0.85).clamp(0.0, 1.0),
            ..bg
        },
        button: relight(bg, 0.26, 0.7),
        button_hover: relight(bg, 0.31, 0.7),
        button_edge: relight(bg, 0.12, 0.7),
        silk: relight(bg, 0.80, 0.3),
        silk_dim: relight(bg, 0.55, 0.35),
        led_off: relight(bg, 0.20, 0.5),
        amber: theme.warning,
        green: theme.success,
        lit: theme.primary,
        lit_ink: theme.primary_foreground,
        lit_label: relight(bg, 0.13, 0.6),
        lit_hint: relight(bg, 0.26, 0.5),
        white: relight(bg, 0.93, 0.4),
        white_edge: relight(bg, 0.60, 0.4),
        white_ink: relight(bg, 0.16, 0.6),
        white_note: relight(bg, 0.42, 0.5),
        black: relight(bg, 0.24, 0.7),
        black_edge: relight(bg, 0.10, 0.7),
        black_ink: relight(bg, 0.80, 0.4),
    };
    if let Some(visualia) = visualia {
        colors.bed = visualia.bed;
        colors.button = visualia.button;
        colors.button_hover = visualia.button_hover;
        colors.button_edge = visualia.border;
    }
    colors
}

/// What a three-part rotary group turns: the MASTER pot or one of the FM-1's encoders.
#[derive(Clone, Copy)]
enum KnobTarget {
    Master,
    Enc(u8),
}

/// The two gradient ends for a key's bottom strip, top then bottom. In a light
/// theme the top is darker; a dark theme reverses the light direction.
fn strip_colors(fill: Hsla, dark: bool) -> (Hsla, Hsla) {
    let darker = Hsla { l: (fill.l - 0.07).max(0.0), ..fill };
    let lighter = Hsla { l: (fill.l + 0.07).min(1.0), ..fill };
    if dark {
        (lighter, darker)
    } else {
        (darker, lighter)
    }
}

/// The small membrane window near the top of a key. Up, it is a shade off the
/// key, like an unpowered LED behind the membrane; pressed, it glows with the
/// accent and sinks a couple of pixels. Dark themes reverse the shade direction.
fn led_window(fill: Hsla, glow: Hsla, border: Hsla, pressed: bool, dark: bool) -> AnyElement {
    let top = if pressed { KEY_LED_TOP + KEY_LED_DEPTH } else { KEY_LED_TOP };
    let window = div()
        .absolute()
        .left(relative(KEY_LED_INSET))
        .right(relative(KEY_LED_INSET))
        .top(px(top))
        .h(px(KEY_LED_HEIGHT))
        .rounded(px(KEY_LED_RADIUS))
        .overflow_hidden();
    if !pressed {
        return window
            .bg(strip_colors(fill, dark).0)
            .border_1()
            .border_color(border)
            .into_any_element();
    }
    window
        .bg(Hsla { a: KEY_LED_FILL_ALPHA, ..glow })
        .border_1()
        .border_color(border)
        .into_any_element()
}

/// The strip's drawn height in each state.
fn strip_height(height: f32, pressed: bool) -> f32 {
    if pressed { height / 2.0 } else { height }
}

/// The key's bottom strip. Pressed, it is half as high and solid at the top shade;
/// otherwise it is the full depth with a top-to-bottom gradient. Dark themes reverse
/// the light direction.
fn key_strip(fill: Hsla, pressed: bool, height: f32, dark: bool) -> AnyElement {
    let (top, bottom) = strip_colors(fill, dark);
    let height = strip_height(height, pressed);
    let strip = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(height));
    if pressed {
        strip.bg(top).into_any_element()
    } else {
        strip
            .bg(linear_gradient(
                180.,
                linear_color_stop(top, 0.),
                linear_color_stop(bottom, 1.),
            ))
            .into_any_element()
    }
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
        let left = 14.0f32;
        let right = w - 14.0;
        let controls_top = 24.0f32;
        let group_width = 120.0f32;
        let group_gap = 6.0f32;
        let row_gap = 4.0f32;
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
        let left_columns = left + 2.0 * group_width + group_gap;
        let max_function_width = (right - left_columns - 12.0).max(0.0);
        let function_width = (w * 0.52).clamp(360.0, 520.0).min(max_function_width);
        let function_left = right - function_width;
        let button_gap = 4.0f32;
        let button_width = (function_width - button_gap * 5.0) / 6.0;
        let knob_gap = 6.0f32;
        let knob_width = (function_width - knob_gap * 3.0) / 4.0;

        let app = cx.entity().clone();
        let mut panel = div()
            .id("fm1-panel")
            .test_support()
            .relative()
            .w_full()
            .max_w(px(PANEL_MAX_W))
            .h(px(panel_h))
            .rounded(px(8.0))
            .overflow_hidden()
            .bg(colors.bed)
            .on_prepaint(move |bounds, _, cx| {
                let width: f32 = bounds.size.width.into();
                app.update(cx, |this, cx| {
                    if (this.panel_width - width).abs() > 0.5 {
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
                    .top(px(7.0))
                    .text_size(px(9.5))
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
            panel = panel.child(self.turn_group(cx, x, y, group_width, *salt, label, *target, &colors));
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

        let second_band = controls_top + GROUP_HEIGHT * 2.0 + row_gap + 8.0;

        // OCT- and OCT+ sit under the two encoder columns.
        for (index, (label, id, hint)) in [("OCT−", 0u8, "Z"), ("OCT+", 1, "X")]
            .iter()
            .enumerate()
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
                let tint = if *id == 12 { colors.green } else { colors.amber };
                panel = panel.child(self.panel_button(
                    cx, x, y, button_width, *id, label, hint, tint, &mono, &colors,
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
                .gap(px(1.0))
                .children(WHITE_KEYS.iter().map(|&id| self.white_key(cx, id, &colors))),
        );
        let black_height = key_height * 0.62;
        for &(id, after) in BLACK_KEYS.iter() {
            let center_x = key_left + (after as f32 + 1.0) * white_width;
            let width = white_width * 0.62;
            panel = panel.child(self.black_key(cx, id, center_x - width / 2.0, key_top, width, black_height, &colors));
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
        let arrow = 22.0f32;
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
                .text_size(px(13.0))
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
            .rounded(px(3.0))
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
                    ScrollDelta::Pixels(point) => f32::from(point.y) / 40.0,
                };
                let steps = this.wheel_detents(salt, detents);
                if steps != 0 {
                    this.turn_target(target, steps);
                    cx.notify();
                }
            }))
            .child(end("turn-down", "−", -1))
            .child(div().w(px(1.0)).h_full().bg(edge))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .truncate()
                    .text_size(px(9.5))
                    .text_color(silk)
                    .child(label.to_string()),
            )
            .child(div().w(px(1.0)).h_full().bg(edge))
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
        let lit =
            self.held[id as usize] || self.leds[id as usize] > 0.04 || self.panel_down[id as usize];
        let fill = if lit {
            mix_hsla(colors.button, tint, 0.45)
        } else {
            colors.button
        };
        let ink = if lit { colors.lit_label } else { colors.silk };
        let hint_color = if lit { colors.lit_hint } else { colors.silk_dim };
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
            .rounded(px(4.0))
            .overflow_hidden()
            .bg(fill)
            .border_1()
            .border_color(if lit { tint } else { colors.button_edge })
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px(px(6.0))
            .cursor_pointer()
            .hover(move |style| {
                if lit {
                    style
                } else {
                    style.bg(button_hover)
                }
            })
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
                    .w(px(7.0))
                    .h(px(7.0))
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
                    .text_size(px(9.5))
                    .text_color(ink)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(px(8.5))
                    .font_family(mono)
                    .text_color(hint_color)
                    .child(hint.to_string()),
            )
    }

    /// A white piano key: the theme's light step at rest, the theme's accent while
    /// held; hint in the middle, note name at the bottom.
    fn white_key(&self, cx: &mut Context<Self>, id: u8, colors: &PanelColors) -> impl IntoElement {
        let pressed = self.held[id as usize] || self.panel_down[id as usize];
        let lit = pressed || self.leds[id as usize] > 0.04;
        let note = PANEL_KEYS
            .iter()
            .find(|(key, _, _)| *key == id)
            .map(|(_, note, _)| *note)
            .unwrap_or("");
        let hint = panel_key_hint(id);
        let (key_fill, ink, note_ink) = if lit && !pressed {
            (colors.lit, colors.lit_ink, colors.lit_ink)
        } else {
            (colors.white, colors.white_ink, colors.white_note)
        };
        let depth = strip_height(WHITE_KEY_DEPTH, pressed);
        let lip = strip_colors(key_fill, colors.dark).1;
        div()
            .id(("panel-key", id as usize))
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .rounded(px(1.0))
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
            .child(led_window(key_fill, colors.lit, colors.white_edge, pressed, colors.dark))
            .child(
                v_flex()
                    .size_full()
                    .pb(px(WHITE_KEY_DEPTH + WHITE_KEY_LIP))
                    .items_center()
                    .child(
                        div().flex_1().flex().items_center().child(
                            div()
                                .text_size(px(16.0))
                                .text_color(ink)
                                .child(hint),
                        ),
                    )
                    .child(
                        div()
                            .pb(px(2.0))
                            .text_size(px(10.0))
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
        let lit = pressed || self.leds[id as usize] > 0.04;
        let hint = panel_key_hint(id);
        let (fill, ink) = if lit && !pressed {
            (colors.lit, colors.lit_ink)
        } else {
            (colors.black, colors.black_ink)
        };
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(height))
            .id(("panel-key", id as usize))
            .rounded(px(1.0))
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
                    .child(led_window(fill, colors.lit, colors.black_edge, pressed, colors.dark))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(ink)
                            .child(hint),
                    )
                    .child(key_strip(fill, pressed, BLACK_KEY_DEPTH, colors.dark)),
            )
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*` — the parent's gpui glob import would drag in GPUI's `test`
    // attribute macro, which re-expands the built-in #[test] (see library_tab.rs).
    use super::{panel_colors, strip_colors, strip_height};
    use gpui_kit::component::Theme;

    #[test]
    fn pressed_strip_is_half_height() {
        assert_eq!(strip_height(10.0, false), 10.0);
        assert_eq!(strip_height(10.0, true), 5.0);
    }

    #[test]
    fn key_strip_gradient_brackets_the_key_color() {
        let theme = Theme::default();
        let colors = panel_colors(&theme, None);
        for fill in [colors.white, colors.black, colors.lit] {
            let (top, bottom) = strip_colors(fill, false);
            assert!(top.l <= fill.l, "light theme: the top is not darker");
            assert!(bottom.l >= fill.l, "light theme: the bottom is not lighter");
            assert_eq!(top.h, fill.h);
            assert_eq!(bottom.h, fill.h);
            let (top, bottom) = strip_colors(fill, true);
            assert!(top.l >= fill.l, "dark theme: the top is not lighter");
            assert!(bottom.l <= fill.l, "dark theme: the bottom is not darker");
        }
        for fill in [colors.white, colors.black] {
            let (top, bottom) = strip_colors(fill, false);
            assert!(top.l < fill.l && bottom.l > fill.l);
            let (top, bottom) = strip_colors(fill, true);
            assert!(top.l > fill.l && bottom.l < fill.l);
        }
    }

    /// The key colorway follows the theme background's hue with the bed always dark
    /// (light themes too), the black keys strictly between bed and white, and a held
    /// key on the theme's accent with its on-accent ink.
    #[test]
    fn keys_follow_theme_with_dark_bed() {
        let mut theme = Theme::default();

        // A light background: the bed must stay dark, the keys light.
        theme.background = gpui_kit::hsla(0.08, 0.4, 0.96, 1.0);
        let colors = panel_colors(&theme, None);
        assert!(colors.bed.l <= 0.16, "light theme: bed stays dark");
        assert!(colors.white.l >= 0.9, "white keys read light");
        assert!(colors.black.l > colors.bed.l && colors.black.l < colors.white.l);
        assert_eq!(colors.lit, theme.primary, "held keys use the theme accent");
        assert_eq!(colors.lit_ink, theme.primary_foreground);

        // A dark, saturated background: same invariants, hue carries over.
        theme.background = gpui_kit::hsla(0.6, 0.3, 0.08, 1.0);
        let colors = panel_colors(&theme, None);
        assert!(colors.bed.l <= 0.16);
        assert!((colors.bed.h - 0.6).abs() < 1e-6, "the bed keeps the theme's hue");
        assert!((colors.white.h - 0.6).abs() < 1e-6, "the keys keep the theme's hue");
    }

    /// Visualia swaps the bed, buttons and their edges for the accent-derived
    /// palette while the white keys keep the shared background derivation.
    #[test]
    fn visualia_panel_uses_the_accent_palette() {
        let theme = Theme::default();
        for dark in [false, true] {
            let palette = crate::themes::visualia::panel(crate::themes::accent("red"), dark);
            let colors = panel_colors(&theme, Some(palette));
            assert_eq!(colors.bed.h, palette.bed.h);
            assert_eq!(colors.bed.l, palette.bed.l);
            assert_eq!(colors.button.l, palette.button.l);
            assert_eq!(colors.button_edge.l, palette.border.l);
            assert!(colors.white.l >= 0.9, "white keys keep their derivation");
        }
    }
}
