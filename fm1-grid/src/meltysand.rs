//! The runtime "Meltysand" theme: macOS Tahoe (26) styling computed from the
//! chosen accent colour. Like Influenza it is not JSON-backed.
//!
//! Apple's current desktop look is Liquid Glass: translucent, lightly tinted
//! chrome over content, capsule/very-round controls, layered surfaces and a
//! restrained accent. GPUI cannot refract the wallpaper, so the approximation
//! uses Apple's semantic colours and the glass impression where the token set
//! allows it:
//!
//! * semantic labels: #1D1D1F light / #F2F2F7 dark, secondary #6E6E73 / #98989D
//! * system grey surfaces: #F2F2F4 / #1C1C1E, separators #E3E3E6 / #38383A
//! * controls and sidebars carry alpha, the way Liquid Glass floats over content
//! * Apple system status colours: red #FF3B30 / #FF453A, green #34C759 /
//!   #30D158, orange #FF9F0A, blue #0A84FF (dark)
//! * Tahoe's window corner radius (26 px) for large surfaces, a rounded 12 px
//!   for controls (macOS renders many as capsules)
//!
//! The accent palette is shared with Influenza (`influenza::ACCENTS`).

use crate::influenza::{opt, Accent, Rgb};
use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};

/// The picker id of the computed family (mirrors `theme::MELTYSAND`).
pub(crate) const FAMILY: &str = "meltysand";

/// Build a `ThemeConfigColors` from public fields; the base-colour slots are
/// crate-private and stay at their defaults.
macro_rules! colors {
    ($($field:ident: $value:expr),* $(,)?) => {{
        let mut colors = ThemeConfigColors::default();
        $( colors.$field = $value; )*
        colors
    }};
}

/// The theme pair's light or dark side.
pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = Rgb::parse(accent.hex);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Meltysand {} ({})",
            if dark { "Dark" } else { "Light" },
            accent.label
        )
        .into(),
        mode,
        // Tahoe's window rounding (26 px) with a rounded control scale.
        radius: Some(12),
        radius_lg: Some(26),
        shadow: Some(true),
        colors: if dark { dark_tokens(base) } else { light_tokens(base) },
        ..Default::default()
    }
}

/// The light Liquid Glass table.
fn light_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.hex();
    let accent_text = a.contrast();
    let hover = a.darken(0.06).hex();
    let active = a.darken(0.14).hex();
    colors! {
        background: opt("#f2f2f4"),
        foreground: opt("#1d1d1f"),
        border: opt("#e3e3e6"),
        muted: opt("#e9e9ee"),
        muted_foreground: opt("#6e6e73"),
        accent: opt(a.alpha(0x26)),
        accent_foreground: opt(accent_text),
        primary: opt(accent.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        // Controls are glass: the fill carries alpha over the window material.
        button: opt("#ffffffb8"),
        button_hover: opt("#ffffffd9"),
        button_active: opt("#f2f2f7"),
        button_foreground: opt("#1d1d1f"),
        button_primary: opt(accent.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#e9e9ee"),
        button_secondary_hover: opt("#e1e1e7"),
        button_secondary_active: opt("#d8d8de"),
        button_secondary_foreground: opt("#1d1d1f"),
        button_danger: opt("#ff3b30"),
        button_danger_hover: opt("#ff544a"),
        button_danger_active: opt("#e0352b"),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt("#34c759"),
        button_success_hover: opt("#4bd46e"),
        button_success_active: opt("#2eb84f"),
        button_success_foreground: opt("#000000"),
        button_warning: opt("#ff9f0a"),
        button_warning_hover: opt("#ffb340"),
        button_warning_active: opt("#e68f00"),
        button_warning_foreground: opt("#000000"),
        button_info: opt("#007aff"),
        button_info_hover: opt("#3395ff"),
        button_info_active: opt("#0062cc"),
        button_info_foreground: opt("#ffffff"),
        // Apple's semantic status colours.
        danger: opt("#ff3b30"),
        danger_hover: opt("#ff544a"),
        danger_active: opt("#e0352b"),
        danger_foreground: opt("#ffffff"),
        success: opt("#34c759"),
        success_hover: opt("#4bd46e"),
        success_active: opt("#2eb84f"),
        success_foreground: opt("#000000"),
        warning: opt("#ff9f0a"),
        warning_hover: opt("#ffb340"),
        warning_active: opt("#e68f00"),
        warning_foreground: opt("#000000"),
        info: opt("#007aff"),
        info_hover: opt("#3395ff"),
        info_active: opt("#0062cc"),
        info_foreground: opt("#ffffff"),
        link: opt(accent.clone()),
        link_hover: opt(a.darken(0.10).hex()),
        link_active: opt(a.darken(0.22).hex()),
        // Content surfaces: raised cards are white, popovers the system grey.
        list: opt("#ffffff"),
        list_hover: opt("#f2f2f7"),
        list_active: opt(a.alpha(0x26)),
        list_active_border: opt(accent.clone()),
        list_even: opt("#fafafc"),
        list_head: opt("#f2f2f4"),
        popover: opt("#f2f2f7"),
        popover_foreground: opt("#1d1d1f"),
        input: opt("#c6c6c8"),
        selection: opt(a.alpha(0x40)),
        // Liquid Glass chrome: translucent greys over the content.
        sidebar: opt("#f2f2f4cc"),
        sidebar_border: opt("#e3e3e6"),
        sidebar_foreground: opt("#1d1d1f"),
        sidebar_accent: opt(a.alpha(0x26)),
        sidebar_accent_foreground: opt("#1d1d1f"),
        sidebar_primary: opt(accent.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#f2f2f4e6"),
        title_bar_border: opt("#e3e3e6"),
        status_bar: opt("#f2f2f4"),
        status_bar_border: opt("#e3e3e6"),
        window_border: opt("#d8d8dc"),
        tab: opt("#00000000"),
        tab_active: opt("#ffffff"),
        tab_active_foreground: opt("#1d1d1f"),
        tab_foreground: opt("#6e6e73"),
        tab_bar: opt("#f2f2f4"),
        tab_bar_segmented: opt("#e9e9ee"),
        switch: opt("#d1d1d6"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#d1d1d6"),
        slider_thumb: opt(accent.clone()),
        progress_bar: opt(accent.clone()),
        scrollbar: opt("#f2f2f400"),
        scrollbar_thumb: opt("#c6c6c8"),
        scrollbar_thumb_hover: opt("#aeaeb2"),
        skeleton: opt("#e5e5ea"),
        table: opt("#ffffff"),
        table_hover: opt("#f2f2f7"),
        table_active: opt(a.alpha(0x26)),
        table_active_border: opt(accent),
        table_even: opt("#fafafc"),
        table_head: opt("#f2f2f4"),
        table_head_foreground: opt("#6e6e73"),
        table_row_border: opt("#e5e5ea"),
    }
}

/// The dark Liquid Glass table; the accent is lifted the way macOS lifts its
/// system colours in dark mode.
fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.lighten(0.12);
    let accent_hex = accent.hex();
    let accent_text = accent.contrast();
    let hover = accent.lighten(0.08).hex();
    let active = accent.darken(0.12).hex();
    colors! {
        background: opt("#1c1c1e"),
        foreground: opt("#f2f2f7"),
        border: opt("#38383a"),
        muted: opt("#2c2c2e"),
        muted_foreground: opt("#98989d"),
        accent: opt(accent.alpha(0x40)),
        accent_foreground: opt(accent_text),
        primary: opt(accent_hex.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        // Glass controls tint white over dark content.
        button: opt("#ffffff1a"),
        button_hover: opt("#ffffff29"),
        button_active: opt("#ffffff12"),
        button_foreground: opt("#ffffff"),
        button_primary: opt(accent_hex.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#2c2c2e"),
        button_secondary_hover: opt("#3a3a3c"),
        button_secondary_active: opt("#242426"),
        button_secondary_foreground: opt("#f2f2f7"),
        button_danger: opt("#ff453a"),
        button_danger_hover: opt("#ff6961"),
        button_danger_active: opt("#d93a31"),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt("#30d158"),
        button_success_hover: opt("#4adb6f"),
        button_success_active: opt("#28b84b"),
        button_success_foreground: opt("#000000"),
        button_warning: opt("#ffd60a"),
        button_warning_hover: opt("#ffe33d"),
        button_warning_active: opt("#e6c009"),
        button_warning_foreground: opt("#000000"),
        button_info: opt("#0a84ff"),
        button_info_hover: opt("#3d9dff"),
        button_info_active: opt("#006edb"),
        button_info_foreground: opt("#ffffff"),
        danger: opt("#ff453a"),
        danger_hover: opt("#ff6961"),
        danger_active: opt("#d93a31"),
        danger_foreground: opt("#ffffff"),
        success: opt("#30d158"),
        success_hover: opt("#4adb6f"),
        success_active: opt("#28b84b"),
        success_foreground: opt("#000000"),
        warning: opt("#ffd60a"),
        warning_hover: opt("#ffe33d"),
        warning_active: opt("#e6c009"),
        warning_foreground: opt("#000000"),
        info: opt("#0a84ff"),
        info_hover: opt("#3d9dff"),
        info_active: opt("#006edb"),
        info_foreground: opt("#ffffff"),
        link: opt(accent_hex.clone()),
        link_hover: opt(accent.lighten(0.10).hex()),
        link_active: opt(accent.darken(0.15).hex()),
        list: opt("#1c1c1e"),
        list_hover: opt("#2c2c2e"),
        list_active: opt(accent.alpha(0x40)),
        list_active_border: opt(accent_hex.clone()),
        list_even: opt("#202022"),
        list_head: opt("#242426"),
        popover: opt("#2c2c2e"),
        popover_foreground: opt("#f2f2f7"),
        input: opt("#48484a"),
        selection: opt(accent.alpha(0x59)),
        sidebar: opt("#1c1c1ecc"),
        sidebar_border: opt("#38383a"),
        sidebar_foreground: opt("#f2f2f7"),
        sidebar_accent: opt(accent.alpha(0x40)),
        sidebar_accent_foreground: opt("#ffffff"),
        sidebar_primary: opt(accent_hex.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#1c1c1ee6"),
        title_bar_border: opt("#38383a"),
        status_bar: opt("#1c1c1e"),
        status_bar_border: opt("#38383a"),
        window_border: opt("#3a3a3c"),
        tab: opt("#00000000"),
        tab_active: opt("#2c2c2e"),
        tab_active_foreground: opt("#f2f2f7"),
        tab_foreground: opt("#98989d"),
        tab_bar: opt("#1c1c1e"),
        tab_bar_segmented: opt("#2c2c2e"),
        switch: opt("#3a3a3c"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#3a3a3c"),
        slider_thumb: opt(accent_hex.clone()),
        progress_bar: opt(accent_hex),
        scrollbar: opt("#1c1c1e00"),
        scrollbar_thumb: opt("#48484a"),
        scrollbar_thumb_hover: opt("#636366"),
        skeleton: opt("#2c2c2e"),
        table: opt("#1c1c1e"),
        table_hover: opt("#2c2c2e"),
        table_active: opt(accent.alpha(0x40)),
        table_active_border: opt(accent.hex()),
        table_even: opt("#202022"),
        table_head: opt("#242426"),
        table_head_foreground: opt("#98989d"),
        table_row_border: opt("#2f2f31"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::influenza::accent;

    #[test]
    fn names_the_theme_and_uses_tahoe_rounding() {
        let blue = accent("blue");
        let light = theme(ThemeMode::Light, blue);
        let dark = theme(ThemeMode::Dark, blue);
        assert_eq!(light.name.as_ref(), "Meltysand Light (Blue)");
        assert_eq!(dark.name.as_ref(), "Meltysand Dark (Blue)");
        assert_eq!(light.radius, Some(12));
        assert_eq!(light.radius_lg, Some(26));
        // Accent reaches the primary slot unchanged in light, lifted in dark.
        assert_eq!(light.colors.primary.as_deref(), Some("#007aff"));
        assert_eq!(dark.colors.primary.as_deref(), Some("#1f8aff"));
    }

    #[test]
    fn glass_surfaces_carry_alpha() {
        let light = theme(ThemeMode::Light, accent("blue"));
        assert_eq!(light.colors.button.as_deref(), Some("#ffffffb8"));
        assert_eq!(light.colors.sidebar.as_deref(), Some("#f2f2f4cc"));
        let dark = theme(ThemeMode::Dark, accent("blue"));
        assert_eq!(dark.colors.button.as_deref(), Some("#ffffff1a"));
        assert_eq!(dark.colors.sidebar.as_deref(), Some("#1c1c1ecc"));
    }
}
