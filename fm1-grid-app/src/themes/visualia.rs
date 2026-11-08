//! Visualia: an accent-driven theme with a cream light side and a near-black
//! purple-tinted dark side. Like Influenza and Meltysand it is computed at
//! runtime from the accent and the light/dark mode.
//!
//! The front panel gets its own treatment (see [`panel`]): the key bed is a
//! desaturated, heavily darkened accent, the buttons a lighter and even more
//! desaturated step, and the panel borders follow the theme's tinted grey. The
//! white/black keys keep the shared panel derivation (a light/dark step of the
//! theme background), so they still look like piano keys in either mode.

use super::colors;
use super::{alpha_hex, color, hex, hsl, opt, toward_purple, Accent, Rgb};
use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::Hsla;

/// The picker id of the computed family (mirrors `theme::VISUALIA`).
pub(crate) const FAMILY: &str = "visualia";

/// The front-panel colours Visualia overrides, derived from the accent.
#[derive(Clone, Copy)]
pub(crate) struct PanelPalette {
    /// The key bed: desaturated, heavily darkened accent.
    pub bed: Hsla,
    /// The front-panel buttons: a lighter, more desaturated accent step.
    pub button: Hsla,
    pub button_hover: Hsla,
    /// Panel edges: tinted grey in light mode, light desaturated grey in dark.
    pub border: Hsla,
}

/// The theme pair's light or dark side.
pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = Rgb::parse(accent.hex);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Visualia {} ({})",
            if dark { "Dark" } else { "Light" },
            accent.label
        )
        .into(),
        mode,
        font_family: Some(
            "Inter, Segoe UI, Helvetica Neue, Noto Sans, DejaVu Sans, sans-serif".into(),
        ),
        mono_font_family: Some(
            "JetBrains Mono, Cascadia Mono, Consolas, Menlo, DejaVu Sans Mono, monospace".into(),
        ),
        radius: Some(8),
        radius_lg: Some(16),
        shadow: Some(true),
        colors: if dark {
            dark_tokens(base)
        } else {
            light_tokens(base)
        },
        ..Default::default()
    }
}

/// The cream light side: a fixed cream canvas, accent-tinted grey borders and
/// the usual neutral controls warmed to match.
fn light_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.hex();
    let accent_text = a.contrast();
    let hover = a.darken(0.08).hex();
    let active = a.darken(0.18).hex();
    // Dark grey pulled slightly toward the accent's hue.
    let border = hex(hsl(color_hue(a), 0.10, 0.28));
    let border_soft = hex(hsl(color_hue(a), 0.08, 0.42));
    colors! {
        background: opt("#fbf9f7"),
        foreground: opt("#241f17"),
        border: opt(border.clone()),
        muted: opt("#efe8d5"),
        muted_foreground: opt("#6b6254"),
        accent: opt(a.alpha(0x99)),
        accent_foreground: opt(accent_text),
        primary: opt(accent.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        button: opt("#fffaf0"),
        button_hover: opt("#f9f2e2"),
        button_active: opt("#f1e8d5"),
        button_foreground: opt("#241f17"),
        button_primary: opt(accent.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#eee6d2"),
        button_secondary_hover: opt("#e6dcc6"),
        button_secondary_active: opt("#ddd2ba"),
        button_secondary_foreground: opt("#241f17"),
        button_danger: opt("#b3261e"),
        button_danger_hover: opt("#9e211a"),
        button_danger_active: opt("#871a14"),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt("#3f6f21"),
        button_success_hover: opt("#365f1c"),
        button_success_active: opt("#2c4f17"),
        button_success_foreground: opt("#ffffff"),
        button_warning: opt("#8a5a00"),
        button_warning_hover: opt("#754c00"),
        button_warning_active: opt("#603e00"),
        button_warning_foreground: opt("#ffffff"),
        button_info: opt(accent.clone()),
        button_info_hover: opt(hover.clone()),
        button_info_active: opt(active.clone()),
        button_info_foreground: opt(accent_text),
        danger: opt("#b3261e"),
        danger_hover: opt("#9e211a"),
        danger_active: opt("#871a14"),
        danger_foreground: opt("#ffffff"),
        success: opt("#3f6f21"),
        success_hover: opt("#365f1c"),
        success_active: opt("#2c4f17"),
        success_foreground: opt("#ffffff"),
        warning: opt("#8a5a00"),
        warning_hover: opt("#754c00"),
        warning_active: opt("#603e00"),
        warning_foreground: opt("#ffffff"),
        info: opt(accent.clone()),
        info_hover: opt(hover.clone()),
        info_active: opt(active.clone()),
        info_foreground: opt(accent_text),
        link: opt(accent.clone()),
        link_hover: opt(a.darken(0.10).hex()),
        link_active: opt(a.darken(0.22).hex()),
        list: opt("#fbf6e8"),
        list_hover: opt("#f7f0df"),
        list_active: opt(a.alpha(0x33)),
        list_active_border: opt(accent.clone()),
        list_even: opt("#f5eedb"),
        list_head: opt("#f1e8d3"),
        popover: opt("#f6f4ed"),
        popover_foreground: opt("#241f17"),
        input: opt(border_soft),
        selection: opt(a.alpha(0x40)),
        sidebar: opt("#fbf9f7"),
        sidebar_border: opt(border.clone()),
        sidebar_foreground: opt("#241f17"),
        sidebar_accent: opt(a.alpha(0x26)),
        sidebar_accent_foreground: opt("#241f17"),
        sidebar_primary: opt(accent.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#fbf9f7"),
        title_bar_border: opt(border.clone()),
        status_bar: opt("#fbf9f7"),
        status_bar_border: opt(border),
        window_border: opt(hex(hsl(color_hue(a), 0.08, 0.32))),
        tab: opt("#00000000"),
        tab_active: opt("#fffaf0"),
        tab_active_foreground: opt("#241f17"),
        tab_foreground: opt("#6b6254"),
        tab_bar: opt("#fbf9f7"),
        tab_bar_segmented: opt("#efe8d5"),
        switch: opt("#c9beaa"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#ddd2bd"),
        slider_thumb: opt(accent.clone()),
        progress_bar: opt(accent.clone()),
        scrollbar: opt("#fbf9f700"),
        scrollbar_thumb: opt("#cbbfa8"),
        scrollbar_thumb_hover: opt("#b3a58b"),
        skeleton: opt("#ece3cf"),
        table: opt("#fbf6e8"),
        table_hover: opt("#f7f0df"),
        table_active: opt(a.alpha(0x33)),
        table_active_border: opt(accent),
        table_even: opt("#f5eedb"),
        table_head: opt("#f1e8d3"),
        table_head_foreground: opt("#6b6254"),
        table_row_border: opt("#e6dcc6"),
    }
}

/// The dark side: a nearly black, desaturated accent hue pulled toward purple,
/// with medium-light desaturated borders.
fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = color_of(a);
    let bg_hue = toward_purple(accent.h);
    let background = hsl(bg_hue, accent.s * 0.20, 0.055);
    let border = hsl(bg_hue, 0.06, 0.42);
    let lifted = hsl(
        accent.h,
        (accent.s * 0.90).clamp(0.0, 1.0),
        (accent.l + 0.15).min(0.92),
    );
    let lifted_hex = hex(lifted);
    let accent_text = a.lighten(0.15).contrast();
    let hover = hsl(lifted.h, lifted.s, (lifted.l + 0.06).min(1.0));
    let active = hsl(lifted.h, lifted.s, (lifted.l - 0.08).max(0.0));
    colors! {
        background: opt(hex(background)),
        foreground: opt("#ece7f2"),
        border: opt(hex(border)),
        muted: opt(hex(hsl(bg_hue, 0.10, 0.10))),
        muted_foreground: opt("#a8a2b2"),
        accent: opt(alpha_hex(accent, 0.40)),
        accent_foreground: opt(accent_text),
        primary: opt(lifted_hex.clone()),
        primary_hover: opt(hex(hover)),
        primary_active: opt(hex(active)),
        primary_foreground: opt(accent_text),
        button: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        button_hover: opt(hex(hsl(bg_hue, 0.10, 0.16))),
        button_active: opt(hex(hsl(bg_hue, 0.10, 0.10))),
        button_foreground: opt("#ece7f2"),
        button_primary: opt(lifted_hex.clone()),
        button_primary_hover: opt(hex(hover)),
        button_primary_active: opt(hex(active)),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt(hex(hsl(bg_hue, 0.10, 0.16))),
        button_secondary_hover: opt(hex(hsl(bg_hue, 0.10, 0.20))),
        button_secondary_active: opt(hex(hsl(bg_hue, 0.10, 0.13))),
        button_secondary_foreground: opt("#ece7f2"),
        button_danger: opt("#ff8a80"),
        button_danger_hover: opt("#ffa59d"),
        button_danger_active: opt("#e56f66"),
        button_danger_foreground: opt("#000000"),
        button_success: opt("#8bd17c"),
        button_success_hover: opt("#a3de96"),
        button_success_active: opt("#74ba66"),
        button_success_foreground: opt("#000000"),
        button_warning: opt("#ffd479"),
        button_warning_hover: opt("#ffe3a3"),
        button_warning_active: opt("#e6bc63"),
        button_warning_foreground: opt("#000000"),
        button_info: opt("#8ab4ff"),
        button_info_hover: opt("#a9c8ff"),
        button_info_active: opt("#709be6"),
        button_info_foreground: opt("#000000"),
        danger: opt("#ff8a80"),
        danger_hover: opt("#ffa59d"),
        danger_active: opt("#e56f66"),
        danger_foreground: opt("#000000"),
        success: opt("#8bd17c"),
        success_hover: opt("#a3de96"),
        success_active: opt("#74ba66"),
        success_foreground: opt("#000000"),
        warning: opt("#ffd479"),
        warning_hover: opt("#ffe3a3"),
        warning_active: opt("#e6bc63"),
        warning_foreground: opt("#000000"),
        info: opt("#8ab4ff"),
        info_hover: opt("#a9c8ff"),
        info_active: opt("#709be6"),
        info_foreground: opt("#000000"),
        link: opt(lifted_hex.clone()),
        link_hover: opt(hex(hsl(lifted.h, lifted.s, (lifted.l + 0.10).min(1.0)))),
        link_active: opt(hex(hsl(lifted.h, lifted.s, (lifted.l - 0.12).max(0.0)))),
        list: opt(hex(hsl(bg_hue, 0.10, 0.08))),
        list_hover: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        list_active: opt(alpha_hex(accent, 0.45)),
        list_active_border: opt(lifted_hex.clone()),
        list_even: opt(hex(hsl(bg_hue, 0.10, 0.07))),
        list_head: opt(hex(hsl(bg_hue, 0.10, 0.10))),
        popover: opt(hex(hsl(bg_hue, 0.10, 0.10))),
        popover_foreground: opt("#ece7f2"),
        input: opt(hex(hsl(bg_hue, 0.06, 0.34))),
        selection: opt(alpha_hex(accent, 0.45)),
        sidebar: opt(hex(background)),
        sidebar_border: opt(hex(border)),
        sidebar_foreground: opt("#ece7f2"),
        sidebar_accent: opt(alpha_hex(accent, 0.45)),
        sidebar_accent_foreground: opt("#ece7f2"),
        sidebar_primary: opt(lifted_hex.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt(hex(background)),
        title_bar_border: opt(hex(border)),
        status_bar: opt(hex(background)),
        status_bar_border: opt(hex(border)),
        window_border: opt(hex(hsl(bg_hue, 0.06, 0.30))),
        tab: opt("#00000000"),
        tab_active: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        tab_active_foreground: opt("#ece7f2"),
        tab_foreground: opt("#a8a2b2"),
        tab_bar: opt(hex(background)),
        tab_bar_segmented: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        switch: opt(hex(hsl(bg_hue, 0.08, 0.28))),
        switch_thumb: opt("#ece7f2"),
        slider_bar: opt(hex(hsl(bg_hue, 0.08, 0.30))),
        slider_thumb: opt(lifted_hex.clone()),
        progress_bar: opt(lifted_hex),
        scrollbar: opt("#20202000"),
        scrollbar_thumb: opt(hex(hsl(bg_hue, 0.08, 0.28))),
        scrollbar_thumb_hover: opt(hex(hsl(bg_hue, 0.08, 0.38))),
        skeleton: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        table: opt(hex(hsl(bg_hue, 0.10, 0.08))),
        table_hover: opt(hex(hsl(bg_hue, 0.10, 0.12))),
        table_active: opt(alpha_hex(accent, 0.45)),
        table_active_border: opt(hex(lifted)),
        table_even: opt(hex(hsl(bg_hue, 0.10, 0.07))),
        table_head: opt(hex(hsl(bg_hue, 0.10, 0.10))),
        table_head_foreground: opt("#a8a2b2"),
        table_row_border: opt(hex(hsl(bg_hue, 0.08, 0.18))),
    }
}

/// The accent's hue, for the tinted grey light-mode borders.
fn color_hue(a: Rgb) -> f32 {
    // Reconstruct the hue from sRGB via gpui's parser.
    gpui_kit::component::try_parse_color(&a.hex())
        .map(|color| color.h)
        .unwrap_or(0.58)
}

fn color_of(a: Rgb) -> Hsla {
    gpui_kit::component::try_parse_color(&a.hex()).unwrap_or_default()
}

/// The front-panel palette for Visualia. `dark` selects the mirrored dark
/// treatment; the key bed is always the desaturated, darkened accent.
pub(crate) fn panel(accent: &Accent, dark: bool) -> PanelPalette {
    let a = if dark {
        Hsla {
            h: toward_purple(color(accent).h),
            ..color(accent)
        }
    } else {
        color(accent)
    };
    if dark {
        PanelPalette {
            bed: hsl(a.h, a.s * 0.25, 0.05),
            button: hsl(a.h, a.s * 0.18, 0.15),
            button_hover: hsl(a.h, a.s * 0.18, 0.20),
            border: hsl(a.h, 0.06, 0.42),
        }
    } else {
        PanelPalette {
            bed: hsl(a.h, a.s * 0.30, 0.10),
            button: hsl(a.h, a.s * 0.22, 0.22),
            button_hover: hsl(a.h, a.s * 0.22, 0.27),
            border: hsl(a.h, 0.10, 0.28),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::accent;

    #[test]
    fn light_is_cream_with_accent_tinted_borders() {
        let red = accent("red");
        let light = theme(ThemeMode::Light, red);
        assert_eq!(light.name.as_ref(), "Visualia Light (Red)");
        assert_eq!(light.colors.background.as_deref(), Some("#fbf9f7"));
        // The border is a dark, low-saturation tint of the accent hue.
        let border =
            gpui_kit::component::try_parse_color(light.colors.border.as_deref().unwrap()).unwrap();
        assert!(border.l < 0.35 && border.s < 0.15);
        // The menu/metronome surface is a desaturated cream, not the yellow one.
        let popover =
            gpui_kit::component::try_parse_color(light.colors.popover.as_deref().unwrap()).unwrap();
        assert!(popover.s < 0.35, "popover is desaturated: {popover:?}");
    }

    #[test]
    fn dark_background_is_near_black_and_purple_shifted() {
        let blue = accent("blue");
        let dark = theme(ThemeMode::Dark, blue);
        assert_eq!(dark.name.as_ref(), "Visualia Dark (Blue)");
        let bg = gpui_kit::component::try_parse_color(dark.colors.background.as_deref().unwrap())
            .unwrap();
        assert!(bg.l < 0.10, "background is nearly black: {bg:?}");
        assert!(bg.s < 0.25, "background is desaturated: {bg:?}");
        let border =
            gpui_kit::component::try_parse_color(dark.colors.border.as_deref().unwrap()).unwrap();
        assert!(
            border.l > 0.35 && border.s < 0.12,
            "medium light desaturated border: {border:?}"
        );
    }

    #[test]
    fn panel_bed_and_buttons_follow_the_accent() {
        for dark in [false, true] {
            let accent = accent("red");
            let palette = panel(accent, dark);
            assert!(palette.bed.l < 0.12);
            assert!(palette.button.l > palette.bed.l);
            assert!(
                palette.button.s < palette.bed.s,
                "buttons are more desaturated"
            );
            assert!(palette.border.l > palette.button.l);
        }
    }
}
