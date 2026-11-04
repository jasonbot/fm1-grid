//! The runtime "Influenza" theme: a Fluent-UI-flavoured light/dark pair computed
//! from the chosen accent colour. Nothing here is JSON-backed; `apply_theme`
//! builds both `ThemeConfig`s and swaps them into the registry slots.
//!
//! The palette merges the accent choices of Windows 11 and macOS. Windows 11
//! ships the 48-swatch user palette (see the Windows 11 25H2 list), macOS ships
//! blue/purple/pink/red/orange/yellow/green/graphite. Where the two sets overlap
//! the macOS value is canonical (macOS Big Sur's `controlAccentColor` values:
//! #007AFF, #A550A7, #F74F9E, #FF5257, #F7821B, #FFC600, #62BA46, #8C8C8C); the
//! remaining Windows hues stay as distinct entries. Earthy and dark Windows
//! variants that sit within a hue/lightness step of a kept colour are folded
//! away rather than offered twice.
//!
//! Token values follow the Windows 11 / WinUI 3 resources: SolidBackgroundFill-
//! ColorBase for surfaces, TextFillColorPrimary/Secondary, DividerStrokeColor,
//! CardBackgroundFillColor and the SystemFillColor status pair (see the WinUI
//! light/dark tables used while building this). Corner radii are Fluent's 4 px
//! controls and 8 px surfaces.

use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::SharedString;

/// The picker id of the computed family (mirrors `theme::INFLUENZA`).
pub(crate) const FAMILY: &str = "influenza";

/// One selectable accent. `hex` is the canonical value named in the module docs.
pub(crate) struct Accent {
    pub id: &'static str,
    pub label: &'static str,
    pub hex: &'static str,
}

/// The merged accent palette. macOS entries first (they win the overlaps), then
/// the Windows-only hues that survive de-duplication, brightest to darkest where
/// a family has more than one survivor.
pub(crate) const ACCENTS: &[Accent] = &[
    // macOS accent colours (System Settings ▸ Appearance).
    Accent { id: "blue", label: "Blue", hex: "#007AFF" },
    Accent { id: "purple", label: "Purple", hex: "#A550A7" },
    Accent { id: "pink", label: "Pink", hex: "#F74F9E" },
    Accent { id: "red", label: "Red", hex: "#FF5257" },
    Accent { id: "orange", label: "Orange", hex: "#F7821B" },
    Accent { id: "yellow", label: "Yellow", hex: "#FFC600" },
    Accent { id: "green", label: "Green", hex: "#62BA46" },
    Accent { id: "graphite", label: "Graphite", hex: "#8C8C8C" },
    // Windows 11 hues with no macOS counterpart (or a clearly different value).
    Accent { id: "teal", label: "Teal", hex: "#00B7C3" },     // Seafoam
    Accent { id: "indigo", label: "Indigo", hex: "#6B69D6" }, // Purple Shadow Dark
    Accent { id: "violet", label: "Violet", hex: "#B146C2" }, // Violet Red Light
    Accent { id: "magenta", label: "Magenta", hex: "#9A0089" }, // Orchid
    Accent { id: "rose", label: "Rose", hex: "#EA005E" },     // Rose Bright
    Accent { id: "rust", label: "Rust", hex: "#DA3B01" },     // Rust
    Accent { id: "forest", label: "Forest", hex: "#107C10" }, // Green
    Accent { id: "olive", label: "Olive", hex: "#498205" },   // Meadow Green
    Accent { id: "turf", label: "Turf", hex: "#00CC6A" },     // Turf Green
    Accent { id: "steel", label: "Steel", hex: "#2D7D9A" },   // Cool Blue
    Accent { id: "khaki", label: "Khaki", hex: "#7E735F" },   // Camouflage
    Accent { id: "moss", label: "Moss", hex: "#486860" },     // Moss
];

/// An accent by picker id; an unknown or stale session value falls back to Blue.
pub(crate) fn accent(id: &str) -> &'static Accent {
    ACCENTS
        .iter()
        .find(|accent| accent.id == id)
        .unwrap_or(&ACCENTS[0])
}

/// The parsed accent colour, for swatches outside the theme.
pub(crate) fn color(accent: &Accent) -> gpui_kit::Hsla {
    gpui_kit::component::try_parse_color(accent.hex).unwrap_or_default()
}

/// The theme pair's light or dark side.
pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = Rgb::parse(accent.hex);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Influenza {} ({})",
            if dark { "Dark" } else { "Light" },
            accent.label
        )
        .into(),
        mode,
        // Fluent controls use 4 px corners; flyouts and cards use 8.
        radius: Some(4),
        radius_lg: Some(8),
        shadow: Some(true),
        colors: if dark { dark_tokens(base) } else { light_tokens(base) },
        ..Default::default()
    }
}

pub(crate) fn opt(value: impl Into<SharedString>) -> Option<SharedString> {
    Some(value.into())
}

/// Build a `ThemeConfigColors` from public fields, leaving the crate-private
/// base-colour slots at their defaults. (The struct has private fields, so a
/// struct literal cannot use `..Default::default()` from this crate.)
macro_rules! colors {
    ($($field:ident: $value:expr),* $(,)?) => {{
        let mut colors = ThemeConfigColors::default();
        $( colors.$field = $value; )*
        colors
    }};
}

/// The light Fluent table.
fn light_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.hex();
    let accent_text = a.contrast();
    let primary_hover = a.darken(0.08).hex();
    let primary_active = a.darken(0.18).hex();
    colors! {
        // Surfaces: SolidBackgroundFillColorBase / CardBackgroundFillColor.
        background: opt("#f3f3f3"),
        foreground: opt("#1b1b1b"),
        border: opt("#e0e0e0"),
        muted: opt("#ebebeb"),
        muted_foreground: opt("#616161"),
        // Accent hover wash and accent text.
        accent: opt(a.alpha(0x33)),
        accent_foreground: opt(accent_text),
        primary: opt(accent.clone()),
        primary_hover: opt(primary_hover.clone()),
        primary_active: opt(primary_active.clone()),
        primary_foreground: opt(accent_text),
        // Controls: neutral fills with a hairline stroke, accent for the primary.
        button: opt("#fdfdfd"),
        button_hover: opt("#f6f6f6"),
        button_active: opt("#ededed"),
        button_foreground: opt("#1b1b1b"),
        button_primary: opt(accent.clone()),
        button_primary_hover: opt(primary_hover.clone()),
        button_primary_active: opt(primary_active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#ededed"),
        button_secondary_hover: opt("#e6e6e6"),
        button_secondary_active: opt("#dddddd"),
        button_secondary_foreground: opt("#1b1b1b"),
        button_danger: opt("#c42b1c"),
        button_danger_hover: opt("#b02a1c"),
        button_danger_active: opt("#9e2115"),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt("#0f7b0f"),
        button_success_hover: opt("#0c6b0c"),
        button_success_active: opt("#0a5c0a"),
        button_success_foreground: opt("#ffffff"),
        button_warning: opt("#9d5d00"),
        button_warning_hover: opt("#8a5200"),
        button_warning_active: opt("#784700"),
        button_warning_foreground: opt("#ffffff"),
        button_info: opt("#0078d4"),
        button_info_hover: opt("#106ebe"),
        button_info_active: opt("#005a9e"),
        button_info_foreground: opt("#ffffff"),
        // Status fills: SystemFillColor{Success,Critical,Caution}.
        danger: opt("#c42b1c"),
        danger_hover: opt("#b02a1c"),
        danger_active: opt("#9e2115"),
        danger_foreground: opt("#ffffff"),
        success: opt("#0f7b0f"),
        success_hover: opt("#0c6b0c"),
        success_active: opt("#0a5c0a"),
        success_foreground: opt("#ffffff"),
        warning: opt("#9d5d00"),
        warning_hover: opt("#8a5200"),
        warning_active: opt("#784700"),
        warning_foreground: opt("#ffffff"),
        info: opt("#0078d4"),
        info_hover: opt("#106ebe"),
        info_active: opt("#005a9e"),
        info_foreground: opt("#ffffff"),
        link: opt(accent.clone()),
        link_hover: opt(a.darken(0.12).hex()),
        link_active: opt(a.darken(0.25).hex()),
        // Lists and flyouts: LayerFill / CardBackground.
        list: opt("#ffffff"),
        list_hover: opt("#f6f6f6"),
        list_active: opt(a.alpha(0x33)),
        list_active_border: opt(accent.clone()),
        list_even: opt("#fafafa"),
        list_head: opt("#f3f3f3"),
        popover: opt("#ffffff"),
        popover_foreground: opt("#1b1b1b"),
        input: opt("#c8c8c8"),
        selection: opt(a.alpha(0x4d)),
        // Chrome.
        sidebar: opt("#f3f3f3"),
        sidebar_border: opt("#e0e0e0"),
        sidebar_foreground: opt("#1b1b1b"),
        sidebar_accent: opt(a.alpha(0x26)),
        sidebar_accent_foreground: opt("#1b1b1b"),
        sidebar_primary: opt(accent.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#f3f3f3"),
        title_bar_border: opt("#e0e0e0"),
        status_bar: opt("#f3f3f3"),
        status_bar_border: opt("#e0e0e0"),
        window_border: opt("#e0e0e0"),
        // Tabs, switches, sliders, scrollbars.
        tab: opt("#00000000"),
        tab_active: opt("#ffffff"),
        tab_active_foreground: opt("#1b1b1b"),
        tab_foreground: opt("#616161"),
        tab_bar: opt("#f3f3f3"),
        tab_bar_segmented: opt("#ebebeb"),
        switch: opt("#a6a6a6"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#e0e0e0"),
        slider_thumb: opt(accent.clone()),
        progress_bar: opt(accent.clone()),
        scrollbar: opt("#f3f3f300"),
        scrollbar_thumb: opt("#c7c7c7"),
        scrollbar_thumb_hover: opt("#a6a6a6"),
        skeleton: opt("#e6e6e6"),
        // Tables reuse the list neutrals.
        table: opt("#ffffff"),
        table_hover: opt("#f6f6f6"),
        table_active: opt(a.alpha(0x33)),
        table_active_border: opt(accent),
        table_even: opt("#fafafa"),
        table_head: opt("#f3f3f3"),
        table_head_foreground: opt("#616161"),
        table_row_border: opt("#e8e8e8"),
    }
}

/// The dark Fluent table. The accent is lifted for dark surfaces (Windows' dark
/// theme uses the SystemAccentColor light steps for text and fills).
fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.lighten(0.18);
    let accent_hex = accent.hex();
    let accent_text = accent.contrast();
    let hover = accent.lighten(0.08).hex();
    let active = accent.darken(0.10).hex();
    colors! {
        background: opt("#202020"),
        foreground: opt("#ffffff"),
        border: opt("#3a3a3a"),
        muted: opt("#2d2d2d"),
        muted_foreground: opt("#c5c5c5"),
        accent: opt(accent.alpha(0x40)),
        accent_foreground: opt(accent_text),
        primary: opt(accent_hex.clone()),
        primary_hover: opt(hover.clone()),
        primary_active: opt(active.clone()),
        primary_foreground: opt(accent_text),
        button: opt("#2d2d2d"),
        button_hover: opt("#333333"),
        button_active: opt("#292929"),
        button_foreground: opt("#ffffff"),
        button_primary: opt(accent_hex.clone()),
        button_primary_hover: opt(hover.clone()),
        button_primary_active: opt(active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt("#333333"),
        button_secondary_hover: opt("#3a3a3a"),
        button_secondary_active: opt("#2f2f2f"),
        button_secondary_foreground: opt("#ffffff"),
        button_danger: opt("#ff99a4"),
        button_danger_hover: opt("#ffb3bc"),
        button_danger_active: opt("#e8808a"),
        button_danger_foreground: opt("#000000"),
        button_success: opt("#6ccb5f"),
        button_success_hover: opt("#84d878"),
        button_success_active: opt("#5bb54f"),
        button_success_foreground: opt("#000000"),
        button_warning: opt("#fce100"),
        button_warning_hover: opt("#ffe94d"),
        button_warning_active: opt("#e6cf00"),
        button_warning_foreground: opt("#000000"),
        button_info: opt("#60cdff"),
        button_info_hover: opt("#7fd8ff"),
        button_info_active: opt("#4ab8e6"),
        button_info_foreground: opt("#000000"),
        danger: opt("#ff99a4"),
        danger_hover: opt("#ffb3bc"),
        danger_active: opt("#e8808a"),
        danger_foreground: opt("#000000"),
        success: opt("#6ccb5f"),
        success_hover: opt("#84d878"),
        success_active: opt("#5bb54f"),
        success_foreground: opt("#000000"),
        warning: opt("#fce100"),
        warning_hover: opt("#ffe94d"),
        warning_active: opt("#e6cf00"),
        warning_foreground: opt("#000000"),
        info: opt("#60cdff"),
        info_hover: opt("#7fd8ff"),
        info_active: opt("#4ab8e6"),
        info_foreground: opt("#000000"),
        link: opt(accent_hex.clone()),
        link_hover: opt(accent.lighten(0.12).hex()),
        link_active: opt(accent.darken(0.15).hex()),
        list: opt("#272727"),
        list_hover: opt("#2f2f2f"),
        list_active: opt(accent.alpha(0x40)),
        list_active_border: opt(accent_hex.clone()),
        list_even: opt("#232323"),
        list_head: opt("#2a2a2a"),
        popover: opt("#2b2b2b"),
        popover_foreground: opt("#ffffff"),
        input: opt("#5a5a5a"),
        selection: opt(accent.alpha(0x59)),
        sidebar: opt("#202020"),
        sidebar_border: opt("#3a3a3a"),
        sidebar_foreground: opt("#ffffff"),
        sidebar_accent: opt(accent.alpha(0x40)),
        sidebar_accent_foreground: opt("#ffffff"),
        sidebar_primary: opt(accent_hex.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt("#202020"),
        title_bar_border: opt("#3a3a3a"),
        status_bar: opt("#202020"),
        status_bar_border: opt("#3a3a3a"),
        window_border: opt("#3a3a3a"),
        tab: opt("#00000000"),
        tab_active: opt("#2a2a2a"),
        tab_active_foreground: opt("#ffffff"),
        tab_foreground: opt("#c5c5c5"),
        tab_bar: opt("#202020"),
        tab_bar_segmented: opt("#2a2a2a"),
        switch: opt("#4a4a4a"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#3a3a3a"),
        slider_thumb: opt(accent_hex.clone()),
        progress_bar: opt(accent_hex),
        scrollbar: opt("#20202000"),
        scrollbar_thumb: opt("#4a4a4a"),
        scrollbar_thumb_hover: opt("#5c5c5c"),
        skeleton: opt("#2f2f2f"),
        table: opt("#272727"),
        table_hover: opt("#2f2f2f"),
        table_active: opt(accent.alpha(0x40)),
        table_active_border: opt(accent.hex()),
        table_even: opt("#232323"),
        table_head: opt("#2a2a2a"),
        table_head_foreground: opt("#c5c5c5"),
        table_row_border: opt("#353535"),
    }
}

/// An sRGB triple with the few operations the theme needs.
#[derive(Clone, Copy)]
pub(crate) struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    pub(crate) fn parse(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        Self { r: channel(0), g: channel(2), b: channel(4) }
    }

    pub(crate) fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// `#rrggbbaa` for translucent washes over the surface.
    pub(crate) fn alpha(self, a: u8) -> String {
        format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, a)
    }

    pub(crate) fn blend(self, other: Self, t: f32) -> Self {
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
        }
    }

    pub(crate) fn lighten(self, t: f32) -> Self {
        self.blend(Self { r: 255, g: 255, b: 255 }, t)
    }

    pub(crate) fn darken(self, t: f32) -> Self {
        self.blend(Self { r: 0, g: 0, b: 0 }, t)
    }

    /// WCAG relative luminance, for picking readable text on the accent.
    pub(crate) fn luminance(self) -> f32 {
        let channel = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// Black or white, whichever has more contrast on this fill. The threshold sits
    /// at the point Fluent flips: blues/reds/purples get white text, oranges,
    /// greens and yellows get black.
    pub(crate) fn contrast(self) -> &'static str {
        if self.luminance() > 0.30 {
            "#000000"
        } else {
            "#ffffff"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_are_unique_and_parse() {
        let mut ids = std::collections::HashSet::new();
        for accent in ACCENTS {
            assert!(ids.insert(accent.id), "duplicate accent id {}", accent.id);
            assert_eq!(Rgb::parse(accent.hex).hex(), accent.hex.to_lowercase());
        }
    }

    #[test]
    fn theme_names_the_accent_and_mode() {
        let red = accent("red");
        let light = theme(ThemeMode::Light, red);
        let dark = theme(ThemeMode::Dark, red);
        assert_eq!(light.name.as_ref(), "Influenza Light (Red)");
        assert_eq!(dark.name.as_ref(), "Influenza Dark (Red)");
        assert_eq!(light.mode, ThemeMode::Light);
        assert_eq!(dark.mode, ThemeMode::Dark);
        // The accent reaches the primary and accent slots, lifted for dark.
        assert_eq!(light.colors.primary.as_deref(), Some("#ff5257"));
        assert_eq!(dark.colors.primary.as_deref(), Some("#ff7175"));
        // Fluent corner radii.
        assert_eq!(light.radius, Some(4));
        assert_eq!(light.radius_lg, Some(8));
    }

    #[test]
    fn contrast_picks_black_on_light_accents_and_white_on_dark() {
        assert_eq!(Rgb::parse("#FFC600").contrast(), "#000000");
        assert_eq!(Rgb::parse("#007AFF").contrast(), "#ffffff");
        assert_eq!(Rgb::parse("#A550A7").contrast(), "#ffffff");
    }

    #[test]
    fn unknown_accent_falls_back_to_blue() {
        assert_eq!(accent("nope").id, "blue");
    }
}
