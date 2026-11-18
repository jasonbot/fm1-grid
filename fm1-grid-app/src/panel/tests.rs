// No `use super::*` — the parent's gpui glob import would drag in GPUI's `test`
// attribute macro, which re-expands the built-in #[test] (see library_tab.rs).
use super::paint::{led_bottom, panel_colors, strip_colors, strip_height};
use super::WHITE_KEY_LIP;
use gpui_kit::component::Theme;

#[test]
fn led_sits_four_pixels_above_the_lip() {
    assert_eq!(led_bottom(10.0, WHITE_KEY_LIP), 16.0);
    assert_eq!(led_bottom(5.0, WHITE_KEY_LIP), 11.0);
    assert_eq!(led_bottom(6.0, 0.0), 10.0);
    assert_eq!(led_bottom(3.0, 0.0), 7.0);
}

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

    // A dark, saturated background: same invariants, hue carries over.
    theme.background = gpui_kit::hsla(0.6, 0.3, 0.08, 1.0);
    let colors = panel_colors(&theme, None);
    assert!(colors.bed.l <= 0.16);
    assert!(
        (colors.bed.h - 0.6).abs() < 1e-6,
        "the bed keeps the theme's hue"
    );
    assert!(
        (colors.white.h - 0.6).abs() < 1e-6,
        "the keys keep the theme's hue"
    );
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
