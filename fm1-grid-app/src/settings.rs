//! The Settings tab: gpui-kit's Settings component — one General page with an
//! Appearance group (the theme family, the auto/light/dark override and the accent
//! swatches the computed families use). Every pick applies at once and lands in
//! the session record on change.

use crate::app::{Appearance, GridApp};
use fm1_performer::library::HubFeeds;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::ActiveTheme;
use gpui_kit::assets::IconName as Lucide;
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

        let source_item = |label: &'static str,
                           url: &'static str,
                           description: &'static str,
                           get: fn(HubFeeds) -> bool,
                           set: fn(&mut HubFeeds, bool),
                           default: bool| {
            let get_app = app.clone();
            let set_app = app.clone();
            let reset_app = app.clone();
            let dirty_app = app.clone();
            SettingItem::render(move |_options, _window, cx| {
                let checked = get(get_app.read(cx).library.feeds());
                let set_app = set_app.clone();
                let id = label.to_string();
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .max_w_3_5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id(format!("feed-link-{label}"))
                                            .test_support()
                                            .cursor_pointer()
                                            .text_color(cx.theme().muted_foreground)
                                            .on_click(move |_, _, cx| cx.open_url(url))
                                            .child(Icon::new(Lucide::Globe).xsmall()),
                                    )
                                    .child(div().text_sm().child(label)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(description),
                            ),
                    )
                    .child(
                        Checkbox::new(format!("feed-check-{label}"))
                            .accessibility_label(id)
                            .checked(checked)
                            .on_click(move |checked: &bool, _, cx: &mut App| {
                                set_app.update(cx, |this, cx| {
                                    let mut feeds = this.library.feeds();
                                    set(&mut feeds, *checked);
                                    this.set_hub_feeds(feeds, cx);
                                });
                            }),
                    )
            })
            .keywords([label, description, "catalog", "online"])
            .on_reset(
                move |cx: &App| get(dirty_app.read(cx).library.feeds()) != default,
                move |_, cx: &mut App| {
                    reset_app.update(cx, |this, cx| {
                        let mut feeds = this.library.feeds();
                        set(&mut feeds, default);
                        this.set_hub_feeds(feeds, cx);
                    });
                },
            )
        };
        let hub_item = source_item(
            "Firmware Hub",
            "https://thefirmwarehub.com/",
            "The FM-1 Firmware Hub catalog; the main online listing.",
            |feeds| feeds.hub,
            |feeds, on| feeds.hub = on,
            true,
        );
        let editor_item = source_item(
            "fm1-editor",
            "https://fm1-editor.com/firmware/",
            "The fm1-editor firmware page.",
            |feeds| feeds.editor,
            |feeds, on| feeds.editor = on,
            true,
        );
        let mutant_item = source_item(
            "mutant.cloud",
            "https://mutant.cloud/",
            "mutant.cloud's catalog, including SMK-37 offers.",
            |feeds| feeds.mutant,
            |feeds, on| feeds.mutant = on,
            false,
        );
        let reverbose_item = source_item(
            "Reverbose",
            "https://reverbose.com/h/m-vave/fm-1",
            "Reverbose's FM-1 and SMK-37 firmware pages.",
            |feeds| feeds.reverbose,
            |feeds, on| feeds.reverbose = on,
            false,
        );
        let source_group = SettingGroup::new()
            .title("Firmware sources")
            .items(vec![hub_item, editor_item, mutant_item, reverbose_item]);

        let appearance_group = SettingGroup::new().title("Appearance").items(vec![
            SettingItem::new("Theme", theme_field)
                .description("Choose your rainbow. Micromanage your button radii.")
                .keywords(["colors", "scheme"]),
            SettingItem::new("Mode", appearance_field)
                .description("Follow the OS light/dark setting, or pin one.")
                .keywords(["dark", "light"]),
            SettingItem::new("Accent color", accent_field)
                .disabled(!accent_active)
                .description(accent_description)
                .keywords(["colour", "fluent", "windows", "macos"]),
        ]);

        Settings::new("fm1-settings").page(
            SettingPage::new("General")
                .resettable(true)
                .group(appearance_group)
                .group(source_group),
        )
    }

    pub(crate) fn set_hub_feeds(&mut self, feeds: HubFeeds, cx: &mut Context<Self>) {
        if self.library.feeds() == feeds {
            return;
        }
        self.library.set_feeds(feeds);
        self.feeds_dirty = true;
        self.save_session(cx);
        cx.notify();
    }
}
