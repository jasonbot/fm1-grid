//! The panel's colorway and the small painted parts (LED windows, key strips).

use crate::theme::mix_hsla;
use gpui_kit::component::Theme;
use gpui_kit::*;

/// The LED window sits this far above the key's lip.
pub(super) const KEY_LED_LIP_GAP: f32 = 4.0;
pub(super) const KEY_LED_RADIUS: f32 = 6.0;
/// The strip's lightness offset from the key face.
const STRIP_SHADE_DELTA: f32 = 0.07;
/// How much the LED border mixes toward the key face.
const LED_BORDER_MIX: f32 = 0.5;
const KEY_LED_FILL_ALPHA: f32 = 0.5;

/// The panel's colorway, derived from the active theme each render: the bed keeps
/// the theme background's hue but stays dark (a light theme still gets a dark
/// panel), the keys and controls are the same hue family stepped out in lightness,
/// and the lit/accent colours are the theme's own (primary for held keys, warning
/// for amber LEDs, success for the play LED).
pub(super) struct PanelColors {
    /// Dark theme: reverse the key shading.
    pub(super) dark: bool,
    pub(super) bed: Hsla,
    pub(super) button: Hsla,
    pub(super) button_hover: Hsla,
    pub(super) button_edge: Hsla,
    pub(super) silk: Hsla,
    pub(super) silk_dim: Hsla,
    pub(super) led_off: Hsla,
    pub(super) amber: Hsla,
    pub(super) green: Hsla,
    pub(super) lit: Hsla,
    pub(super) lit_label: Hsla,
    pub(super) lit_hint: Hsla,
    pub(super) white: Hsla,
    pub(super) white_edge: Hsla,
    pub(super) white_ink: Hsla,
    pub(super) white_note: Hsla,
    pub(super) black: Hsla,
    pub(super) black_edge: Hsla,
    pub(super) black_ink: Hsla,
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

pub(super) fn panel_colors(
    theme: &Theme,
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

/// The two gradient ends for a key's bottom strip, top then bottom. In a light
/// theme the top is darker; a dark theme reverses the light direction.
pub(super) fn strip_colors(fill: Hsla, dark: bool) -> (Hsla, Hsla) {
    let darker = Hsla {
        l: (fill.l - STRIP_SHADE_DELTA).max(0.0),
        ..fill
    };
    let lighter = Hsla {
        l: (fill.l + STRIP_SHADE_DELTA).min(1.0),
        ..fill
    };
    if dark {
        (lighter, darker)
    } else {
        (darker, lighter)
    }
}

/// The small membrane window near the top of a key. Up, it is a shade off the
/// key, like an unpowered LED behind the membrane; pressed, it glows with the
/// accent and sinks a couple of pixels. Dark themes reverse the shade direction.
pub(super) fn led_window(
    fill: Hsla,
    glow: Hsla,
    border: Hsla,
    lit: bool,
    dark: bool,
    top: f32,
    height: f32,
    insets: (f32, f32),
) -> AnyElement {
    let border = mix_hsla(border, fill, LED_BORDER_MIX);
    let window = div()
        .absolute()
        .left(relative(insets.0))
        .right(relative(insets.1))
        .top(px(top))
        .h(px(height))
        .rounded(px(KEY_LED_RADIUS))
        .overflow_hidden();
    if !lit {
        return window
            .bg(strip_colors(fill, dark).0)
            .border_1()
            .border_color(border)
            .into_any_element();
    }
    window
        .bg(Hsla {
            a: KEY_LED_FILL_ALPHA,
            ..glow
        })
        .border_1()
        .border_color(border)
        .into_any_element()
}

/// The LED window's bottom edge: the strip height, the lip above it, and the
/// gap to the LED.
pub(super) fn led_bottom(strip_height: f32, lip: f32) -> f32 {
    strip_height + lip + KEY_LED_LIP_GAP
}

/// The strip's drawn height in each state.
pub(super) fn strip_height(height: f32, pressed: bool) -> f32 {
    if pressed {
        height / 2.0
    } else {
        height
    }
}

/// The key's bottom strip. Pressed, it is half as high and solid at the top shade;
/// otherwise it is the full depth with a top-to-bottom gradient. Dark themes reverse
/// the light direction.
pub(super) fn key_strip(fill: Hsla, pressed: bool, height: f32, dark: bool) -> AnyElement {
    let (top, bottom) = strip_colors(fill, dark);
    let height = strip_height(height, pressed);
    let strip = div().absolute().left_0().right_0().bottom_0().h(px(height));
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
