//! The computed accent families — Influenza (Fluent UI), Meltysand (macOS
//! Tahoe/Liquid Glass), Visualia, Mac Platinum, Very High Contrast and Japanese
//! Stationery — plus the accent palette and colour maths they share. None of
//! these are JSON-backed: `GridApp::apply_theme` builds both light and dark
//! `ThemeConfig`s at runtime.
//!
//! The accent palette merges the accent choices of Windows 11 and macOS. Windows
//! 11 ships the 48-swatch user palette (the Windows 11 25H2 list); macOS ships
//! blue/purple/pink/red/orange/yellow/green/graphite. Where the two sets overlap
//! the macOS value is canonical (macOS Big Sur's `controlAccentColor` values:
//! #007AFF, #A550A7, #F74F9E, #FF5257, #F7821B, #FFC600, #62BA46, #8C8C8C); the
//! remaining Windows hues stay as distinct entries. Earthy and dark Windows
//! variants that sit within a hue/lightness step of a kept colour are folded
//! away rather than offered twice.

use gpui_kit::{Hsla, Rgba, SharedString};

pub(crate) mod influenza;
pub(crate) mod japanese_stationery;
pub(crate) mod mac_platinum;
pub(crate) mod very_high_contrast;
pub(crate) mod meltysand;
pub(crate) mod visualia;

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
pub(crate) use colors;

pub(crate) fn opt(value: impl Into<SharedString>) -> Option<SharedString> {
    Some(value.into())
}

/// An sRGB triple with the few operations the theme tokens need.
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

/// An opaque colour from HSL components.
pub(crate) fn hsl(h: f32, s: f32, l: f32) -> Hsla {
    Hsla { h: h.rem_euclid(1.0), s: s.clamp(0.0, 1.0), l: l.clamp(0.0, 1.0), a: 1.0 }
}

/// `#rrggbb` for a colour (alpha ignored).
pub(crate) fn hex(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(rgba.r), channel(rgba.g), channel(rgba.b))
}

/// `#rrggbbaa` for a colour with the given alpha.
pub(crate) fn alpha_hex(color: Hsla, alpha: f32) -> String {
    let rgba = Rgba::from(color);
    let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(alpha)
    )
}

/// Rotate a hue a quarter-step toward purple (the Visualia dark background).
pub(crate) fn toward_purple(h: f32) -> f32 {
    const PURPLE: f32 = 0.78;
    let mut delta = (PURPLE - h).rem_euclid(1.0);
    if delta > 0.5 {
        delta -= 1.0;
    }
    (h + delta * 0.25).rem_euclid(1.0)
}

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

/// The accent palette in spectrum order: red through yellow, green, cyan,
/// blue and violet; achromatic Graphite comes last.
pub(crate) fn accents_by_hue() -> Vec<&'static Accent> {
    let mut accents: Vec<_> = ACCENTS.iter().collect();
    accents.sort_by(|a, b| {
        let (a, b) = (color(a), color(b));
        let a_grey = a.s <= f32::EPSILON;
        let b_grey = b.s <= f32::EPSILON;
        a_grey
            .cmp(&b_grey)
            .then_with(|| a.h.total_cmp(&b.h))
            .then_with(|| a.l.total_cmp(&b.l))
    });
    accents
}

/// An accent by picker id; an unknown or stale session value falls back to Blue.
pub(crate) fn accent(id: &str) -> &'static Accent {
    ACCENTS
        .iter()
        .find(|accent| accent.id == id)
        .unwrap_or(&ACCENTS[0])
}

/// The parsed accent colour, for swatches and the panel.
pub(crate) fn color(accent: &Accent) -> Hsla {
    let rgb = Rgb::parse(accent.hex);
    // Through gpui's parser so the palette and the theme tokens agree.
    gpui_kit::component::try_parse_color(&rgb.hex()).unwrap_or_default()
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
    fn accents_are_ordered_by_hue() {
        let ordered = accents_by_hue();
        let hues: Vec<f32> = ordered
            .iter()
            .filter(|accent| color(accent).s > f32::EPSILON)
            .map(|accent| color(accent).h)
            .collect();
        assert!(hues.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(ordered.last().is_some_and(|accent| color(accent).s <= f32::EPSILON));
    }

    #[test]
    fn unknown_accent_falls_back_to_blue() {
        assert_eq!(accent("nope").id, "blue");
    }

    #[test]
    fn toward_purple_takes_the_short_way() {
        // Red (0.0) is closer to purple going backwards through 1.0.
        assert!(toward_purple(0.0) > 0.9);
        // Blue is already close; a quarter-step moves it toward 0.78.
        assert!(toward_purple(0.58) > 0.58 && toward_purple(0.58) < 0.78);
    }
}
