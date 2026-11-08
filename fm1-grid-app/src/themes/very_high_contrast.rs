//! Very High Contrast: maximum legibility and crisp square controls, computed
//! from the chosen accent like the other runtime families.
//!
//! The surfaces are pure white or black, borders are the opposite ink, and the
//! accent drives primary controls, links and selection. Radii are zero and
//! shadows are off; the UI font is a monospace cascade shared by desktop
//! platforms.

use super::colors;
use super::{color, hex, opt, Accent, Rgb};
use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};

/// The picker id of the computed family (mirrors `theme::VERY_HIGH_CONTRAST`).
pub(crate) const FAMILY: &str = "very-high-contrast";

const FONT: &str = "JetBrains Mono, Cascadia Mono, SF Mono, Menlo, Consolas, \
                    DejaVu Sans Mono, Noto Sans Mono, Courier New, monospace";
/// Push the chosen accent toward full saturation for this family.
const ACCENT_SATURATION_BOOST: f32 = 1.5;

fn saturated(accent: &Accent) -> Rgb {
    let color = color(accent);
    let hsla = gpui_kit::Hsla {
        s: (color.s * ACCENT_SATURATION_BOOST).min(1.0),
        ..color
    };
    Rgb::parse(&hex(hsla))
}

pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = saturated(accent);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Very High Contrast {} ({})",
            if dark { "Dark" } else { "Light" },
            accent.label
        )
        .into(),
        mode,
        font_size: Some(16.0),
        font_family: Some(FONT.into()),
        mono_font_family: Some(FONT.into()),
        mono_font_size: Some(14.0),
        radius: Some(0),
        radius_lg: Some(0),
        shadow: Some(false),
        colors: if dark { dark_tokens(base) } else { light_tokens(base) },
        ..Default::default()
    }
}

fn light_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.hex();
    let accent_text = a.contrast();
    let hover = a.darken(0.12).hex();
    let active = a.darken(0.24).hex();
    colors! {
        background: opt("#ffffff"),
        foreground: opt("#000000"),
        border: opt("#000000"),
        muted: opt("#f0f0f0"),
        muted_foreground: opt("#000000"),
        secondary: opt("#ffffff"),
        secondary_hover: opt("#e6e6e6"),
        secondary_active: opt("#cccccc"),
        secondary_foreground: opt("#000000"),
        accent: opt(a.alpha(0x66)),
        accent_foreground: opt(accent_text),
        primary: opt(accent.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        button: opt("#ffffff"),
        button_hover: opt("#e6e6e6"),
        button_active: opt("#cccccc"),
        button_foreground: opt("#000000"),
        button_primary: opt(accent.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#f0f0f0"),
        button_secondary_hover: opt("#e0e0e0"),
        button_secondary_active: opt("#d0d0d0"),
        button_secondary_foreground: opt("#000000"),
        button_danger: opt("#cc0000"),
        button_danger_hover: opt("#aa0000"),
        button_danger_active: opt("#880000"),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt("#006600"),
        button_success_hover: opt("#004d00"),
        button_success_active: opt("#003300"),
        button_success_foreground: opt("#ffffff"),
        button_warning: opt("#8a5a00"),
        button_warning_hover: opt("#6b4500"),
        button_warning_active: opt("#4d3100"),
        button_warning_foreground: opt("#ffffff"),
        button_info: opt(accent.clone()),
        button_info_hover: opt(hover.clone()),
        button_info_active: opt(active.clone()),
        button_info_foreground: opt(accent_text),
        danger: opt("#cc0000"),
        danger_hover: opt("#aa0000"),
        danger_active: opt("#880000"),
        danger_foreground: opt("#ffffff"),
        success: opt("#006600"),
        success_hover: opt("#004d00"),
        success_active: opt("#003300"),
        success_foreground: opt("#ffffff"),
        warning: opt("#8a5a00"),
        warning_hover: opt("#6b4500"),
        warning_active: opt("#4d3100"),
        warning_foreground: opt("#ffffff"),
        info: opt(accent.clone()),
        info_hover: opt(hover),
        info_active: opt(active),
        info_foreground: opt(accent_text),
        link: opt(accent.clone()),
        link_hover: opt(a.darken(0.12).hex()),
        link_active: opt(a.darken(0.24).hex()),
        list: opt("#ffffff"),
        list_hover: opt("#e6e6e6"),
        list_active: opt(a.alpha(0x66)),
        list_active_border: opt(accent.clone()),
        list_even: opt("#f5f5f5"),
        list_head: opt("#e6e6e6"),
        popover: opt("#ffffff"),
        popover_foreground: opt("#000000"),
        input: opt("#000000"),
        selection: opt(a.alpha(0x66)),
        sidebar: opt("#ffffff"),
        sidebar_border: opt("#000000"),
        sidebar_foreground: opt("#000000"),
        sidebar_accent: opt(a.alpha(0x66)),
        sidebar_accent_foreground: opt("#000000"),
        sidebar_primary: opt(accent.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#ffffff"),
        title_bar_border: opt("#000000"),
        status_bar: opt("#ffffff"),
        status_bar_border: opt("#000000"),
        window_border: opt("#000000"),
        tab: opt("#00000000"),
        tab_active: opt("#ffffff"),
        tab_active_foreground: opt("#000000"),
        tab_foreground: opt("#000000"),
        tab_bar: opt("#e6e6e6"),
        tab_bar_segmented: opt("#d0d0d0"),
        switch: opt("#000000"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#000000"),
        slider_thumb: opt(accent.clone()),
        progress_bar: opt(accent.clone()),
        scrollbar: opt("#ffffff00"),
        scrollbar_thumb: opt("#000000"),
        scrollbar_thumb_hover: opt("#333333"),
        skeleton: opt("#e6e6e6"),
        table: opt("#ffffff"),
        table_hover: opt("#e6e6e6"),
        table_active: opt(a.alpha(0x66)),
        table_active_border: opt(accent),
        table_even: opt("#f5f5f5"),
        table_head: opt("#e6e6e6"),
        table_head_foreground: opt("#000000"),
        table_row_border: opt("#000000"),
    }
}

fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.lighten(0.12);
    let accent_hex = accent.hex();
    let accent_text = accent.contrast();
    let hover = accent.lighten(0.12).hex();
    let active = accent.darken(0.12).hex();
    colors! {
        background: opt("#000000"),
        foreground: opt("#ffffff"),
        border: opt("#ffffff"),
        muted: opt("#1a1a1a"),
        muted_foreground: opt("#ffffff"),
        secondary: opt("#000000"),
        secondary_hover: opt("#1a1a1a"),
        secondary_active: opt("#333333"),
        secondary_foreground: opt("#ffffff"),
        accent: opt(accent.alpha(0x66)),
        accent_foreground: opt(accent_text),
        primary: opt(accent_hex.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        button: opt("#000000"),
        button_hover: opt("#1a1a1a"),
        button_active: opt("#333333"),
        button_foreground: opt("#ffffff"),
        button_primary: opt(accent_hex.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#1a1a1a"),
        button_secondary_hover: opt("#333333"),
        button_secondary_active: opt("#4d4d4d"),
        button_secondary_foreground: opt("#ffffff"),
        button_danger: opt("#ff6666"),
        button_danger_hover: opt("#ff8888"),
        button_danger_active: opt("#ff4444"),
        button_danger_foreground: opt("#000000"),
        button_success: opt("#66ff66"),
        button_success_hover: opt("#99ff99"),
        button_success_active: opt("#33cc33"),
        button_success_foreground: opt("#000000"),
        button_warning: opt("#ffcc00"),
        button_warning_hover: opt("#ffe066"),
        button_warning_active: opt("#ccaa00"),
        button_warning_foreground: opt("#000000"),
        button_info: opt(accent_hex.clone()),
        button_info_hover: opt(hover.clone()),
        button_info_active: opt(active.clone()),
        button_info_foreground: opt(accent_text),
        danger: opt("#ff6666"),
        danger_hover: opt("#ff8888"),
        danger_active: opt("#ff4444"),
        danger_foreground: opt("#000000"),
        success: opt("#66ff66"),
        success_hover: opt("#99ff99"),
        success_active: opt("#33cc33"),
        success_foreground: opt("#000000"),
        warning: opt("#ffcc00"),
        warning_hover: opt("#ffe066"),
        warning_active: opt("#ccaa00"),
        warning_foreground: opt("#000000"),
        info: opt(accent_hex.clone()),
        info_hover: opt(hover),
        info_active: opt(active),
        info_foreground: opt(accent_text),
        link: opt(accent_hex.clone()),
        link_hover: opt(accent.lighten(0.12).hex()),
        link_active: opt(accent.darken(0.12).hex()),
        list: opt("#000000"),
        list_hover: opt("#1a1a1a"),
        list_active: opt(accent.alpha(0x66)),
        list_active_border: opt(accent_hex.clone()),
        list_even: opt("#0d0d0d"),
        list_head: opt("#1a1a1a"),
        popover: opt("#000000"),
        popover_foreground: opt("#ffffff"),
        input: opt("#ffffff"),
        selection: opt(accent.alpha(0x66)),
        sidebar: opt("#000000"),
        sidebar_border: opt("#ffffff"),
        sidebar_foreground: opt("#ffffff"),
        sidebar_accent: opt(accent.alpha(0x66)),
        sidebar_accent_foreground: opt("#ffffff"),
        sidebar_primary: opt(accent_hex.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#000000"),
        title_bar_border: opt("#ffffff"),
        status_bar: opt("#000000"),
        status_bar_border: opt("#ffffff"),
        window_border: opt("#ffffff"),
        tab: opt("#00000000"),
        tab_active: opt("#000000"),
        tab_active_foreground: opt("#ffffff"),
        tab_foreground: opt("#ffffff"),
        tab_bar: opt("#1a1a1a"),
        tab_bar_segmented: opt("#333333"),
        switch: opt("#ffffff"),
        switch_thumb: opt("#000000"),
        slider_bar: opt("#ffffff"),
        slider_thumb: opt(accent_hex.clone()),
        progress_bar: opt(accent_hex.clone()),
        scrollbar: opt("#00000000"),
        scrollbar_thumb: opt("#ffffff"),
        scrollbar_thumb_hover: opt("#cccccc"),
        skeleton: opt("#333333"),
        table: opt("#000000"),
        table_hover: opt("#1a1a1a"),
        table_active: opt(accent.alpha(0x66)),
        table_active_border: opt(accent_hex),
        table_even: opt("#0d0d0d"),
        table_head: opt("#1a1a1a"),
        table_head_foreground: opt("#ffffff"),
        table_row_border: opt("#ffffff"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::accent;

    #[test]
    fn high_contrast_has_square_crisp_surfaces_and_a_monospace_ui() {
        let red = accent("red");
        let light = theme(ThemeMode::Light, red);
        assert_eq!(light.name.as_ref(), "Very High Contrast Light (Red)");
        assert_eq!(light.radius, Some(0));
        assert_eq!(light.radius_lg, Some(0));
        assert_eq!(light.shadow, Some(false));
        assert_eq!(light.colors.background.as_deref(), Some("#ffffff"));
        assert_eq!(light.colors.foreground.as_deref(), Some("#000000"));
        assert_eq!(light.colors.border.as_deref(), Some("#000000"));
        assert!(light.font_family.as_deref().unwrap().contains("monospace"));
    }

    #[test]
    fn high_contrast_dark_is_black_on_white() {
        let blue = accent("blue");
        let dark = theme(ThemeMode::Dark, blue);
        assert_eq!(dark.colors.background.as_deref(), Some("#000000"));
        assert_eq!(dark.colors.foreground.as_deref(), Some("#ffffff"));
        assert_eq!(dark.colors.border.as_deref(), Some("#ffffff"));
        assert_eq!(dark.colors.popover.as_deref(), Some("#000000"));
        assert_eq!(dark.colors.secondary.as_deref(), Some("#000000"));
        assert_eq!(dark.radius, Some(0));
    }
}
