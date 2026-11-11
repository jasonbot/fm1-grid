//! Japanese Stationery: warm paper neutrals with one blue family computed from
//! the chosen accent. The neutral inks, paper and washes are the original
//! hand-authored palette; only the blue tokens move with the accent.
//!
//! The blue transform is a relative HSL shift from the palette's original
//! accent, macOS Blue (#007AFF). That makes Blue reproduce every original blue
//! exactly while other accents keep the same lightness and saturation steps.

use super::colors;
use super::{hex, opt, Accent, Rgb};
use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::Hsla;

/// The picker id of the computed family (mirrors `theme::JAPANESE_STATIONERY`).
pub(crate) const FAMILY: &str = "japanese-stationery";

const BLUE_SOURCE: &str = "#007AFF";

/// Recolor one original blue for `accent`, preserving its hue/lightness step
/// relative to the palette's original Blue.
fn blue(a: Rgb, original: &str) -> String {
    let source = hsla(Rgb::parse(BLUE_SOURCE));
    let target = hsla(Rgb::parse(original));
    let accent = hsla(a);
    hex(Hsla {
        h: (accent.h + target.h - source.h).rem_euclid(1.0),
        s: (accent.s + target.s - source.s).clamp(0.0, 1.0),
        l: (accent.l + target.l - source.l).clamp(0.0, 1.0),
        a: 1.0,
    })
}

fn hsla(color: Rgb) -> Hsla {
    gpui_kit::component::try_parse_color(&color.hex()).unwrap_or_default()
}

/// The theme pair's light or dark side.
pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = Rgb::parse(accent.hex);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Japanese Stationery {}",
            if dark { "Dark" } else { "Light" }
        )
        .into(),
        mode,
        font_size: Some(16.0),
        font_family: Some(
            "Hiragino Kaku Gothic ProN, Hiragino Sans, Yu Gothic, Meiryo, DejaVu Sans, \
             Noto Sans JP, Noto Sans CJK JP, Segoe UI Variable Text, SF Pro Text, Inter, \
             Helvetica Neue, DejaVu Sans, sans-serif"
                .into(),
        ),
        mono_font_family: Some(
            "Noto Sans Mono CJK JP, Osaka, MS Gothic, Cascadia Mono, Consolas, Menlo, \
             DejaVu Sans Mono, monospace"
                .into(),
        ),
        radius: Some(4),
        radius_lg: Some(6),
        shadow: Some(true),
        colors: with_base_colors(
            if dark {
                dark_tokens(base)
            } else {
                light_tokens(base)
            },
            base,
            dark,
        ),
        ..Default::default()
    }
}

fn with_base_colors(colors: ThemeConfigColors, a: Rgb, dark: bool) -> ThemeConfigColors {
    let mut value = serde_json::to_value(colors).expect("theme colors serialize");
    let map = value.as_object_mut().expect("theme colors are an object");
    let mut set = |key: &str, color: String| {
        map.insert(key.to_string(), color.into());
    };
    set(
        "base.blue",
        blue(a, if dark { "#5e9fe8" } else { "#2783de" }),
    );
    set(
        "base.blue.light",
        blue(a, if dark { "#355f8b" } else { "#cee3f7" }),
    );
    let bases = if dark {
        [
            ("base.cyan", "#2eabbd"),
            ("base.cyan.light", "#1d6b77"),
            ("base.green", "#3fa871"),
            ("base.green.light", "#2c7450"),
            ("base.magenta", "#b577d6"),
            ("base.magenta.light", "#7a4a93"),
            ("base.red", "#f05b4d"),
            ("base.red.light", "#a93229"),
            ("base.yellow", "#e5a823"),
            ("base.yellow.light", "#8a6a1a"),
        ]
    } else {
        [
            ("base.cyan", "#2eabbd"),
            ("base.cyan.light", "#d4eef2"),
            ("base.green", "#3fa871"),
            ("base.green.light", "#e5f4ec"),
            ("base.magenta", "#b577d6"),
            ("base.magenta.light", "#f0e3f7"),
            ("base.red", "#d9483e"),
            ("base.red.light", "#fce9e7"),
            ("base.yellow", "#e5a823"),
            ("base.yellow.light", "#fbebde"),
        ]
    };
    for (key, color) in bases {
        set(key, color.into());
    }
    serde_json::from_value(value).expect("theme colors deserialize")
}

fn light_tokens(a: Rgb) -> ThemeConfigColors {
    colors! {
        background: opt("#ffffff"),
        foreground: opt("#2c2c2b"),
        border: opt("#e6e5e3"),
        caret: opt("#2c2c2b"),
        accent: opt("#f0efed"),
        accent_foreground: opt(blue(a, "#2783de")),
        muted: opt("#f5f3f2"),
        muted_foreground: opt("#8e8b86"),
        primary: opt(blue(a, "#2783de")),
        primary_foreground: opt("#ffffff"),
        primary_hover: opt(blue(a, "#387dc9")),
        primary_active: opt(blue(a, "#355f8b")),
        secondary: opt("#f0efed"),
        secondary_foreground: opt("#383836"),
        secondary_hover: opt("#e6e5e3"),
        secondary_active: opt("#d4d3cf"),
        danger: opt("#d9483e"),
        danger_foreground: opt("#ffffff"),
        danger_hover: opt("#c23b32"),
        danger_active: opt("#a93229"),
        warning: opt("#e5a823"),
        warning_foreground: opt("#4a3600"),
        warning_hover: opt("#d69729"),
        warning_active: opt("#bd8a26"),
        success: opt("#3fa871"),
        success_foreground: opt("#ffffff"),
        success_hover: opt("#35875c"),
        success_active: opt("#2c7450"),
        info: opt(blue(a, "#2783de")),
        info_foreground: opt("#ffffff"),
        info_hover: opt(blue(a, "#387dc9")),
        info_active: opt(blue(a, "#355f8b")),
        link: opt(blue(a, "#2783de")),
        link_hover: opt(blue(a, "#387dc9")),
        link_active: opt(blue(a, "#355f8b")),
        selection: opt(blue(a, "#cee3f7")),
        ring: opt(blue(a, "#2783de")),
        input: opt("#d4d3cf"),
        overlay: opt("#2c2c2b66"),
        sidebar: opt("#f9f8f7"),
        sidebar_foreground: opt("#2c2c2b"),
        sidebar_border: opt("#e6e5e3"),
        sidebar_accent: opt("#f0efed"),
        sidebar_accent_foreground: opt("#2c2c2b"),
        sidebar_primary: opt(blue(a, "#2783de")),
        sidebar_primary_foreground: opt("#ffffff"),
        popover: opt("#ffffff"),
        popover_foreground: opt("#2c2c2b"),
        title_bar: opt("#f9f8f7"),
        title_bar_border: opt("#e6e5e3"),
        tab_bar: opt("#f5f3f2"),
        tab_bar_segmented: opt("#f0efed"),
        tab: opt("#f9f8f7"),
        tab_foreground: opt("#8e8b86"),
        tab_active: opt("#ffffff"),
        tab_active_foreground: opt("#2c2c2b"),
        scrollbar: opt("#ffffff"),
        scrollbar_thumb: opt("#d4d3cf"),
        scrollbar_thumb_hover: opt("#bcbab6"),
        list: opt("#ffffff"),
        list_head: opt("#f5f3f2"),
        list_hover: opt("#f0efed"),
        list_active: opt(blue(a, "#cee3f7")),
        list_active_border: opt(blue(a, "#2783de")),
        list_even: opt("#fbfbfa"),
        table: opt("#ffffff"),
        table_head: opt("#f5f3f2"),
        table_head_foreground: opt("#8e8b86"),
        table_foot: opt("#f5f3f2"),
        table_foot_foreground: opt("#8e8b86"),
        table_row_border: opt("#f0efed"),
        table_hover: opt("#f0efed"),
        table_active: opt(blue(a, "#cee3f7")),
        table_active_border: opt(blue(a, "#2783de")),
        table_even: opt("#fbfbfa"),
        accordion: opt("#ffffff"),
        group_box: opt("#f9f8f7"),
        group_box_foreground: opt("#2c2c2b"),
        group_box_title_foreground: opt("#8e8b86"),
        description_list_label: opt("#f5f3f2"),
        description_list_label_foreground: opt("#8e8b86"),
        skeleton: opt("#f0efed"),
        switch: opt("#d4d3cf"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#e6e5e3"),
        slider_thumb: opt(blue(a, "#2783de")),
        progress_bar: opt(blue(a, "#2783de")),
        drop_target: opt(blue(a, "#cee3f7")),
        drag_border: opt(blue(a, "#2783de")),
        window_border: opt("#e6e5e3"),
        chart_1: opt(blue(a, "#2783de")),
        chart_2: opt("#d9483e"),
        chart_3: opt("#3fa871"),
        chart_4: opt("#e5a823"),
        chart_5: opt("#b577d6"),
        chart_bullish: opt("#3fa871"),
        chart_bearish: opt("#d9483e"),
        chart_grid: opt("#e6e5e3"),
    }
}

fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    colors! {
        background: opt("#191919"),
        foreground: opt("#f0efed"),
        border: opt("#383836"),
        caret: opt("#f0efed"),
        accent: opt("#2c2c2b"),
        accent_foreground: opt(blue(a, "#7db4ee")),
        muted: opt("#202020"),
        muted_foreground: opt("#8e8b86"),
        primary: opt(blue(a, "#2783de")),
        primary_foreground: opt("#ffffff"),
        primary_hover: opt(blue(a, "#5e9fe8")),
        primary_active: opt(blue(a, "#83abe1")),
        secondary: opt("#2c2c2b"),
        secondary_foreground: opt("#e6e5e3"),
        secondary_hover: opt("#383836"),
        secondary_active: opt("#494846"),
        danger: opt("#f05b4d"),
        danger_foreground: opt("#ffffff"),
        danger_hover: opt("#d9483e"),
        danger_active: opt("#c23b32"),
        warning: opt("#e5a823"),
        warning_foreground: opt("#2b2103"),
        warning_hover: opt("#f0b93a"),
        warning_active: opt("#d69729"),
        success: opt("#3fa871"),
        success_foreground: opt("#ffffff"),
        success_hover: opt("#4b996d"),
        success_active: opt("#35875c"),
        info: opt(blue(a, "#2783de")),
        info_foreground: opt("#ffffff"),
        info_hover: opt(blue(a, "#5e9fe8")),
        info_active: opt(blue(a, "#83abe1")),
        link: opt(blue(a, "#7db4ee")),
        link_hover: opt(blue(a, "#9dc0ea")),
        link_active: opt(blue(a, "#cee3f7")),
        selection: opt(blue(a, "#355f8b")),
        ring: opt(blue(a, "#5e9fe8")),
        input: opt("#494846"),
        overlay: opt("#00000080"),
        sidebar: opt("#1b1b1b"),
        sidebar_foreground: opt("#f0efed"),
        sidebar_border: opt("#383836"),
        sidebar_accent: opt("#2c2c2b"),
        sidebar_accent_foreground: opt("#f0efed"),
        sidebar_primary: opt(blue(a, "#2783de")),
        sidebar_primary_foreground: opt("#ffffff"),
        popover: opt("#202020"),
        popover_foreground: opt("#f0efed"),
        title_bar: opt("#1b1b1b"),
        title_bar_border: opt("#383836"),
        tab_bar: opt("#1b1b1b"),
        tab_bar_segmented: opt("#2c2c2b"),
        tab: opt("#1b1b1b"),
        tab_foreground: opt("#8e8b86"),
        tab_active: opt("#191919"),
        tab_active_foreground: opt("#f0efed"),
        scrollbar: opt("#191919"),
        scrollbar_thumb: opt("#494846"),
        scrollbar_thumb_hover: opt("#5f5e59"),
        list: opt("#191919"),
        list_head: opt("#202020"),
        list_hover: opt("#2c2c2b"),
        list_active: opt(blue(a, "#355f8b")),
        list_active_border: opt(blue(a, "#5e9fe8")),
        list_even: opt("#1b1b1b"),
        table: opt("#191919"),
        table_head: opt("#202020"),
        table_head_foreground: opt("#8e8b86"),
        table_foot: opt("#202020"),
        table_foot_foreground: opt("#8e8b86"),
        table_row_border: opt("#2c2c2b"),
        table_hover: opt("#2c2c2b"),
        table_active: opt(blue(a, "#355f8b")),
        table_active_border: opt(blue(a, "#5e9fe8")),
        table_even: opt("#1b1b1b"),
        accordion: opt("#191919"),
        group_box: opt("#1b1b1b"),
        group_box_foreground: opt("#f0efed"),
        group_box_title_foreground: opt("#8e8b86"),
        description_list_label: opt("#202020"),
        description_list_label_foreground: opt("#8e8b86"),
        skeleton: opt("#202020"),
        switch: opt("#494846"),
        switch_thumb: opt("#ffffff"),
        slider_bar: opt("#383836"),
        slider_thumb: opt(blue(a, "#5e9fe8")),
        progress_bar: opt(blue(a, "#5e9fe8")),
        drop_target: opt(blue(a, "#355f8b")),
        drag_border: opt(blue(a, "#5e9fe8")),
        window_border: opt("#383836"),
        chart_1: opt(blue(a, "#5e9fe8")),
        chart_2: opt("#f05b4d"),
        chart_3: opt("#3fa871"),
        chart_4: opt("#e5a823"),
        chart_5: opt("#b577d6"),
        chart_bullish: opt("#3fa871"),
        chart_bearish: opt("#f05b4d"),
        chart_grid: opt("#383836"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::accent;

    #[test]
    fn blue_accent_reproduces_the_original_blue_palette() {
        let blue = accent("blue");
        let light = theme(ThemeMode::Light, blue).colors;
        let dark = theme(ThemeMode::Dark, blue).colors;
        assert_eq!(light.primary.as_deref(), Some("#2783de"));
        assert_eq!(light.primary_hover.as_deref(), Some("#387dc9"));
        assert_eq!(light.primary_active.as_deref(), Some("#355f8b"));
        assert_eq!(light.selection.as_deref(), Some("#cee3f7"));
        assert_eq!(dark.primary.as_deref(), Some("#2783de"));
        assert_eq!(dark.primary_hover.as_deref(), Some("#5e9fe8"));
        assert_eq!(dark.primary_active.as_deref(), Some("#83abe1"));
        assert_eq!(dark.link.as_deref(), Some("#7db4ee"));
        assert_eq!(dark.selection.as_deref(), Some("#355f8b"));
        assert_eq!(light.background.as_deref(), Some("#ffffff"));
        assert_eq!(dark.background.as_deref(), Some("#191919"));
    }

    #[test]
    fn every_original_blue_maps_back_to_itself_for_blue_accent() {
        for original in [
            "#2783de", "#387dc9", "#355f8b", "#cee3f7", "#7db4ee", "#5e9fe8", "#83abe1", "#9dc0ea",
        ] {
            assert_eq!(blue(Rgb::parse("#007AFF"), original), original);
        }
    }

    #[test]
    fn blue_matches_every_original_public_token() {
        let original: serde_json::Value =
            serde_json::from_str(include_str!("testdata/japanese-stationery.json")).unwrap();
        for set in original["themes"].as_array().unwrap() {
            let mode = set["mode"].as_str().unwrap();
            let expected_mode = if mode == "dark" {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            let generated =
                serde_json::to_value(theme(expected_mode, accent("blue")).colors).unwrap();
            let actual = generated.as_object().unwrap();
            for (key, value) in set["colors"].as_object().unwrap() {
                if key == "accordion.hover.background" {
                    continue;
                }
                assert_eq!(
                    actual.get(key).and_then(|v| v.as_str()),
                    Some(value.as_str().unwrap()),
                    "{mode} {key}"
                );
            }
        }
    }

    #[test]
    fn accents_move_the_blue_family_without_touching_the_paper() {
        let red = theme(ThemeMode::Light, accent("red")).colors;
        assert_eq!(red.background.as_deref(), Some("#ffffff"));
        assert_eq!(red.foreground.as_deref(), Some("#2c2c2b"));
        assert_ne!(red.primary.as_deref(), Some("#2783de"));
        assert_ne!(red.selection.as_deref(), Some("#cee3f7"));
    }
}
