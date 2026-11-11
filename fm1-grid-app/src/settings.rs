//! The Settings tab: gpui-kit's Settings component — one General page with an
//! Appearance group (the theme family, the auto/light/dark override and the accent
//! swatches the computed families use). Every pick applies at once and lands in
//! the session record on change.

use crate::app::{Appearance, GridApp};
use gpui_kit::component::h_flex;
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

impl GridApp {
    /// The Settings tab: a searchable settings page with the sidebar the
    /// component provides.
    pub(crate) fn settings_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app = cx.entity().clone();

        let theme_options: Vec<(SharedString, SharedString)> = crate::theme::theme_families()
            .iter()
            .map(|family| (family.id.into(), family.label.into()))
            .collect();
        let theme_field = SettingField::dropdown(
            theme_options,
            {
                let app = app.clone();
                move |cx: &App| app.read(cx).theme_id.clone().into()
            },
            {
                let app = app.clone();
                move |id: SharedString, cx: &mut App| {
                    app.update(cx, |this, cx| this.set_theme_family(id.as_ref(), cx));
                }
            },
        )
        .default_value(crate::theme::DEFAULT_FAMILY);

        let appearance_app = app.clone();
        let appearance_field = SettingField::dropdown(
            vec![
                ("auto".into(), "Auto".into()),
                ("light".into(), "Light".into()),
                ("dark".into(), "Dark".into()),
            ],
            {
                let app = appearance_app.clone();
                move |cx: &App| {
                    match app.read(cx).appearance {
                        Appearance::Auto => "auto",
                        Appearance::Light => "light",
                        Appearance::Dark => "dark",
                    }
                    .into()
                }
            },
            move |id: SharedString, cx: &mut App| {
                let appearance = match id.as_ref() {
                    "light" => Appearance::Light,
                    "dark" => Appearance::Dark,
                    _ => Appearance::Auto,
                };
                appearance_app.update(cx, |this, cx| this.set_appearance(appearance, cx));
            },
        )
        .default_value("auto");

        // The accent palette as one row of swatches: the macOS colours plus the
        // surviving Windows 11 hues (see influenza.rs). The field is disabled unless
        // the picked family actually consumes an accent.
        let accent_active = matches!(
            self.theme_id.as_str(),
            crate::theme::INFLUENZA
                | crate::theme::MELTYSAND
                | crate::theme::VISUALIA
                | crate::theme::MAC_PLATINUM
                | crate::theme::VERY_HIGH_CONTRAST
                | crate::theme::JAPANESE_STATIONERY
        );
        let accent_field = SettingField::render({
            let app = app.clone();
            move |options, _window, cx| {
                let current = app.read(cx).accent.clone();
                let enabled = !options.is_disabled();
                let mut row = h_flex().w_full().flex_wrap().items_center().gap_1();
                for accent in crate::themes::accents_by_hue() {
                    let selected = accent.id == current;
                    let app = app.clone();
                    let id = accent.id;
                    let label = accent.label;
                    // Every circle carries a subtle border; the selected one adds a
                    // primary ring outside it, with a small gap so the colour reads.
                    row = row.child(
                        div()
                            .id(format!("accent-swatch-{id}"))
                            .test_support()
                            .size(px(24.0))
                            .rounded_full()
                            .border_2()
                            .border_color(if selected {
                                cx.theme().primary
                            } else {
                                gpui_kit::transparent_black()
                            })
                            .p(px(2.0))
                            .child(
                                div()
                                    .size_full()
                                    .rounded_full()
                                    .bg(crate::themes::color(accent))
                                    .border_1()
                                    .border_color(cx.theme().border),
                            )
                            .when(enabled, |el| {
                                el.cursor_pointer()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(label).build(window, cx)
                                    })
                                    .on_click(move |_, _, cx| {
                                        app.update(cx, |this, cx| this.set_accent(id, cx));
                                    })
                            }),
                    );
                }
                row
            }
        });

        let accent_description = if accent_active {
            "Color to drive the UI accents"
        } else {
            "This theme doesn't do accents"
        };

        Settings::new("fm1-settings").page(SettingPage::new("General").resettable(true).group(
            SettingGroup::new().title("Appearance").items(vec![
                    SettingItem::new("Theme", theme_field)
                        .description(
                            "Choose your rainbow. Micromanage your button radii.",
                        )
                        .keywords(["colors", "scheme"]),
                    SettingItem::new("Mode", appearance_field)
                        .description("Follow the OS light/dark setting, or pin one.")
                        .keywords(["dark", "light"]),
                    SettingItem::new("Accent color", accent_field)
                        .disabled(!accent_active)
                        .description(accent_description)
                        .keywords(["colour", "fluent", "windows", "macos"]),
                ]),
        ))
    }
}
