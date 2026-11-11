//! Mac Platinum: the classic beveled Mac OS look, computed from the chosen
//! accent colour. Both light and dark sides are built at runtime, like
//! Influenza and Visualia.
//!
//! The neutrals and bevels stay in the Platinum family: pale grey surfaces,
//! hard one-pixel borders, white-to-grey button faces, and dark title/status
//! bars. The accent drives the primary controls, selection, focus ring and
//! links.

use super::colors;
use super::{opt, Accent, Rgb};
use gpui_kit::component::{ThemeConfig, ThemeConfigColors, ThemeMode};

/// The picker id of the computed family (mirrors `theme::MAC_PLATINUM`).
pub(crate) const FAMILY: &str = "mac-platinum";

fn gradient(top: Rgb, bottom: Rgb) -> String {
    format!(
        "linear-gradient(180deg, {} 25%, {} 75%)",
        top.hex(),
        bottom.hex()
    )
}

fn horizontal_gradient(left: Rgb, right: Rgb) -> String {
    format!(
        "linear-gradient(90deg, {} 25%, {} 75%)",
        left.hex(),
        right.hex()
    )
}

/// The theme pair's light or dark side.
pub(crate) fn theme(mode: ThemeMode, accent: &Accent) -> ThemeConfig {
    let base = Rgb::parse(accent.hex);
    let dark = mode.is_dark();
    ThemeConfig {
        name: format!(
            "Mac Platinum {} ({})",
            if dark { "Dark" } else { "Light" },
            accent.label
        )
        .into(),
        mode,
        font_family: Some(
            "Chicago, Charcoal, Geneva, Helvetica Neue, Helvetica, Arial, Noto Sans, \
             DejaVu Sans, sans-serif"
                .into(),
        ),
        mono_font_family: Some("Monaco, Menlo, Courier New, DejaVu Sans Mono, monospace".into()),
        radius: Some(4),
        radius_lg: Some(8),
        shadow: Some(true),
        colors: if dark {
            dark_tokens(base)
        } else {
            light_tokens(base)
        },
        ..Default::default()
    }
}

fn light_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.hex();
    let accent_text = a.contrast();
    let primary = gradient(a.lighten(0.12), a);
    let primary_hover = gradient(a.lighten(0.22), a.lighten(0.07));
    let primary_active = gradient(a, a.darken(0.18));
    let button = gradient(Rgb::parse("#ffffff"), Rgb::parse("#dddddd"));
    let button_hover = gradient(Rgb::parse("#ffffff"), Rgb::parse("#e6e6e6"));
    let button_active = gradient(Rgb::parse("#dddddd"), Rgb::parse("#c6c6c6"));
    let secondary = gradient(Rgb::parse("#f1f1f1"), Rgb::parse("#dbdee0"));
    let secondary_hover = gradient(Rgb::parse("#f6f6f6"), Rgb::parse("#e2e5e7"));
    let secondary_active = gradient(Rgb::parse("#dbdee0"), Rgb::parse("#c6c6c6"));
    let danger = gradient(Rgb::parse("#ef1f1f"), Rgb::parse("#cc241f"));
    let danger_hover = gradient(Rgb::parse("#ff4444"), Rgb::parse("#ef1f1f"));
    let danger_active = gradient(Rgb::parse("#cc241f"), Rgb::parse("#a31814"));
    let success = gradient(Rgb::parse("#3e8e2b"), Rgb::parse("#2f7020"));
    let success_hover = gradient(Rgb::parse("#4aa336"), Rgb::parse("#3e8e2b"));
    let success_active = gradient(Rgb::parse("#2f7020"), Rgb::parse("#265a1a"));
    let warning = gradient(Rgb::parse("#f4ca25"), Rgb::parse("#dab10b"));
    let warning_hover = gradient(Rgb::parse("#fad542"), Rgb::parse("#f4ca25"));
    let warning_active = gradient(Rgb::parse("#dab10b"), Rgb::parse("#b8950a"));
    let info = gradient(Rgb::parse("#026ec4"), Rgb::parse("#025a9e"));
    let info_hover = gradient(Rgb::parse("#2593e0"), Rgb::parse("#026ec4"));
    let info_active = gradient(Rgb::parse("#025a9e"), Rgb::parse("#024a82"));
    colors! {
        background: opt("#f6f6f6"),
        foreground: opt("#1f1f1f"),
        border: opt("#a6a6a6"),
        muted: opt("#d6d6d6"),
        muted_foreground: opt("#6e6e6e"),
        accent: opt(a.alpha(0x22)),
        accent_foreground: opt(accent_text),
        primary: opt(primary.clone()),
        primary_hover: opt(primary_hover.clone()),
        primary_active: opt(primary_active.clone()),
        primary_foreground: opt(accent_text),
        button: opt(button.clone()),
        button_hover: opt(button_hover.clone()),
        button_active: opt(button_active.clone()),
        button_foreground: opt("#1f1f1f"),
        button_primary: opt(primary.clone()),
        button_primary_hover: opt(primary_hover.clone()),
        button_primary_active: opt(primary_active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt(secondary.clone()),
        button_secondary_hover: opt(secondary_hover.clone()),
        button_secondary_active: opt(secondary_active.clone()),
        button_secondary_foreground: opt("#1f1f1f"),
        button_danger: opt(danger.clone()),
        button_danger_hover: opt(danger_hover.clone()),
        button_danger_active: opt(danger_active.clone()),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt(success.clone()),
        button_success_hover: opt(success_hover.clone()),
        button_success_active: opt(success_active.clone()),
        button_success_foreground: opt("#ffffff"),
        button_warning: opt(warning.clone()),
        button_warning_hover: opt(warning_hover.clone()),
        button_warning_active: opt(warning_active.clone()),
        button_warning_foreground: opt("#3a2f00"),
        button_info: opt(info.clone()),
        button_info_hover: opt(info_hover.clone()),
        button_info_active: opt(info_active.clone()),
        button_info_foreground: opt("#ffffff"),
        danger: opt(danger),
        danger_hover: opt(danger_hover),
        danger_active: opt(danger_active),
        danger_foreground: opt("#ffffff"),
        success: opt(success),
        success_hover: opt(success_hover),
        success_active: opt(success_active),
        success_foreground: opt("#ffffff"),
        warning: opt(warning),
        warning_hover: opt(warning_hover),
        warning_active: opt(warning_active),
        warning_foreground: opt("#3a2f00"),
        info: opt(info),
        info_hover: opt(info_hover),
        info_active: opt(info_active),
        info_foreground: opt("#ffffff"),
        link: opt(accent.clone()),
        link_hover: opt(a.darken(0.10).hex()),
        link_active: opt(a.darken(0.22).hex()),
        list: opt("#ffffff"),
        list_hover: opt("#e8e8f0"),
        list_active: opt(a.alpha(0x26)),
        list_active_border: opt(accent.clone()),
        list_even: opt("#efefef"),
        list_head: opt("#f6f6f6"),
        popover: opt("#ffffff"),
        popover_foreground: opt("#1f1f1f"),
        input: opt("#a6a6a6"),
        selection: opt(a.alpha(0x55)),
        sidebar: opt("#eeeeee"),
        sidebar_border: opt("#a6a6a6"),
        sidebar_foreground: opt("#1f1f1f"),
        sidebar_accent: opt(a.alpha(0x22)),
        sidebar_accent_foreground: opt("#1f1f1f"),
        sidebar_primary: opt(accent.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt(gradient(Rgb::parse("#ffffff"), Rgb::parse("#dddddd"))),
        title_bar_border: opt("#4c4c4c"),
        status_bar: opt(gradient(Rgb::parse("#eeeeee"), Rgb::parse("#dddddd"))),
        status_bar_border: opt("#a6a6a6"),
        window_border: opt("#a6a6a6"),
        tab: opt("#00000000"),
        tab_active: opt("#f6f6f6"),
        tab_active_foreground: opt("#1f1f1f"),
        tab_foreground: opt("#6e6e6e"),
        tab_bar: opt("#dddddd"),
        tab_bar_segmented: opt("#d6d6d6"),
        switch: opt(gradient(Rgb::parse("#dbdee0"), Rgb::parse("#c6c6c6"))),
        switch_thumb: opt(gradient(Rgb::parse("#ffffff"), Rgb::parse("#eeeeee"))),
        slider_bar: opt("#4c4c4c"),
        slider_thumb: opt(button.clone()),
        progress_bar: opt(primary.clone()),
        scrollbar: opt("#f6f6f600"),
        scrollbar_thumb: opt(horizontal_gradient(Rgb::parse("#eeeeee"), Rgb::parse("#c6c6c6"))),
        scrollbar_thumb_hover: opt(horizontal_gradient(Rgb::parse("#f6f6f6"), Rgb::parse("#b0b0b0"))),
        skeleton: opt("#e6e6e6"),
        table: opt("#ffffff"),
        table_hover: opt("#e8e8f0"),
        table_active: opt(a.alpha(0x26)),
        table_active_border: opt(accent),
        table_even: opt("#efefef"),
        table_head: opt("#f6f6f6"),
        table_head_foreground: opt("#6e6e6e"),
        table_row_border: opt("#a6a6a6"),
    }
}

fn dark_tokens(a: Rgb) -> ThemeConfigColors {
    let accent = a.lighten(0.12);
    let accent_hex = accent.hex();
    let accent_text = accent.contrast();
    let primary = gradient(accent.lighten(0.12), accent);
    let primary_hover = gradient(accent.lighten(0.22), accent.lighten(0.10));
    let primary_active = gradient(accent, accent.darken(0.16));
    let button = gradient(Rgb::parse("#0f0f0f"), Rgb::parse("#363636"));
    let button_hover = gradient(Rgb::parse("#0f0f0f"), Rgb::parse("#2c2c2c"));
    let button_active = gradient(Rgb::parse("#363636"), Rgb::parse("#4c4c4c"));
    let secondary = gradient(Rgb::parse("#212121"), Rgb::parse("#313639"));
    let secondary_hover = gradient(Rgb::parse("#1b1b1b"), Rgb::parse("#2a2f32"));
    let secondary_active = gradient(Rgb::parse("#313639"), Rgb::parse("#4c4c4c"));
    let danger = gradient(Rgb::parse("#f24949"), Rgb::parse("#e34843"));
    let danger_hover = gradient(Rgb::parse("#ff5c5c"), Rgb::parse("#f24949"));
    let danger_active = gradient(Rgb::parse("#e34843"), Rgb::parse("#e42823"));
    let success = gradient(Rgb::parse("#5bc642"), Rgb::parse("#4eba35"));
    let success_hover = gradient(Rgb::parse("#6ac755"), Rgb::parse("#5bc642"));
    let success_active = gradient(Rgb::parse("#4eba35"), Rgb::parse("#49ad32"));
    let warning = gradient(Rgb::parse("#f6d34c"), Rgb::parse("#f4cd2e"));
    let warning_hover = gradient(Rgb::parse("#fbdb5c"), Rgb::parse("#f6d34c"));
    let warning_active = gradient(Rgb::parse("#f4cd2e"), Rgb::parse("#f3c71b"));
    let info = gradient(Rgb::parse("#1395fd"), Rgb::parse("#038cf6"));
    let info_hover = gradient(Rgb::parse("#50a8e6"), Rgb::parse("#1395fd"));
    let info_active = gradient(Rgb::parse("#038cf6"), Rgb::parse("#0482e5"));
    colors! {
        background: opt("#1b1b1b"),
        foreground: opt("#e0e0e0"),
        border: opt("#6a6a6a"),
        muted: opt("#2a2a2a"),
        muted_foreground: opt("#9c9c9c"),
        accent: opt(accent.alpha(0x22)),
        accent_foreground: opt(accent_text),
        primary: opt(primary.clone()),
        primary_hover: opt(primary_hover.clone()),
        primary_active: opt(primary_active.clone()),
        primary_foreground: opt(accent_text),
        button: opt(button.clone()),
        button_hover: opt(button_hover.clone()),
        button_active: opt(button_active.clone()),
        button_foreground: opt("#e0e0e0"),
        button_primary: opt(primary.clone()),
        button_primary_hover: opt(primary_hover.clone()),
        button_primary_active: opt(primary_active.clone()),
        button_primary_foreground: opt(accent_text),
        button_secondary: opt(secondary.clone()),
        button_secondary_hover: opt(secondary_hover.clone()),
        button_secondary_active: opt(secondary_active.clone()),
        button_secondary_foreground: opt("#e0e0e0"),
        button_danger: opt(danger.clone()),
        button_danger_hover: opt(danger_hover.clone()),
        button_danger_active: opt(danger_active.clone()),
        button_danger_foreground: opt("#ffffff"),
        button_success: opt(success.clone()),
        button_success_hover: opt(success_hover.clone()),
        button_success_active: opt(success_active.clone()),
        button_success_foreground: opt("#ffffff"),
        button_warning: opt(warning.clone()),
        button_warning_hover: opt(warning_hover.clone()),
        button_warning_active: opt(warning_active.clone()),
        button_warning_foreground: opt("#3a2f00"),
        button_info: opt(info.clone()),
        button_info_hover: opt(info_hover.clone()),
        button_info_active: opt(info_active.clone()),
        button_info_foreground: opt("#ffffff"),
        danger: opt(danger),
        danger_hover: opt(danger_hover),
        danger_active: opt(danger_active),
        danger_foreground: opt("#ffffff"),
        success: opt(success),
        success_hover: opt(success_hover),
        success_active: opt(success_active),
        success_foreground: opt("#ffffff"),
        warning: opt(warning),
        warning_hover: opt(warning_hover),
        warning_active: opt(warning_active),
        warning_foreground: opt("#3a2f00"),
        info: opt(info),
        info_hover: opt(info_hover),
        info_active: opt(info_active),
        info_foreground: opt("#ffffff"),
        link: opt(accent_hex.clone()),
        link_hover: opt(accent.lighten(0.12).hex()),
        link_active: opt(accent.darken(0.15).hex()),
        list: opt("#1f1f1f"),
        list_hover: opt("#1e1e2e"),
        list_active: opt(accent.alpha(0x26)),
        list_active_border: opt(accent_hex.clone()),
        list_even: opt("#232323"),
        list_head: opt("#242424"),
        popover: opt("#1f1f1f"),
        popover_foreground: opt("#e0e0e0"),
        input: opt("#6a6a6a"),
        selection: opt(accent.alpha(0x55)),
        sidebar: opt("#242424"),
        sidebar_border: opt("#6a6a6a"),
        sidebar_foreground: opt("#e0e0e0"),
        sidebar_accent: opt(accent.alpha(0x22)),
        sidebar_accent_foreground: opt("#e0e0e0"),
        sidebar_primary: opt(accent_hex.clone()),
        sidebar_primary_foreground: opt(accent_text),
        title_bar: opt(gradient(Rgb::parse("#0f0f0f"), Rgb::parse("#363636"))),
        title_bar_border: opt("#bababa"),
        status_bar: opt(gradient(Rgb::parse("#242424"), Rgb::parse("#363636"))),
        status_bar_border: opt("#6a6a6a"),
        window_border: opt("#6a6a6a"),
        tab: opt("#00000000"),
        tab_active: opt("#1b1b1b"),
        tab_active_foreground: opt("#e0e0e0"),
        tab_foreground: opt("#9c9c9c"),
        tab_bar: opt("#363636"),
        tab_bar_segmented: opt("#2a2a2a"),
        switch: opt(gradient(Rgb::parse("#313639"), Rgb::parse("#4c4c4c"))),
        switch_thumb: opt(gradient(Rgb::parse("#0f0f0f"), Rgb::parse("#242424"))),
        slider_bar: opt("#bababa"),
        slider_thumb: opt(button.clone()),
        progress_bar: opt(primary.clone()),
        scrollbar: opt("#f6f6f600"),
        scrollbar_thumb: opt(horizontal_gradient(Rgb::parse("#242424"), Rgb::parse("#4c4c4c"))),
        scrollbar_thumb_hover: opt(horizontal_gradient(Rgb::parse("#1b1b1b"), Rgb::parse("#616161"))),
        skeleton: opt("#2a2a2a"),
        table: opt("#1f1f1f"),
        table_hover: opt("#1e1e2e"),
        table_active: opt(accent.alpha(0x26)),
        table_active_border: opt(accent_hex),
        table_even: opt("#232323"),
        table_head: opt("#242424"),
        table_head_foreground: opt("#9c9c9c"),
        table_row_border: opt("#6a6a6a"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::accent;

    #[test]
    fn platinum_uses_the_classic_light_surfaces_and_the_accent() {
        let red = accent("red");
        let light = theme(ThemeMode::Light, red);
        assert_eq!(light.name.as_ref(), "Mac Platinum Light (Red)");
        assert_eq!(light.colors.background.as_deref(), Some("#f6f6f6"));
        assert_eq!(light.colors.primary_foreground.as_deref(), Some("#ffffff"));
        assert!(light.colors.primary.as_deref().unwrap().contains("#ff5257"));
    }

    #[test]
    fn platinum_dark_keeps_the_platinum_chrome() {
        let blue = accent("blue");
        let dark = theme(ThemeMode::Dark, blue);
        assert_eq!(dark.name.as_ref(), "Mac Platinum Dark (Blue)");
        assert_eq!(dark.colors.background.as_deref(), Some("#1b1b1b"));
        assert_eq!(dark.colors.title_bar_border.as_deref(), Some("#bababa"));
    }
}
