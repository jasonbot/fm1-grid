//! The Settings tab: gpui-kit's Settings component — one General page with an
//! Appearance group (the theme family and the auto/light/dark override). Both
//! apply at once and land in the session record on change; both declare their
//! defaults, so the page's Reset button restores them.

use crate::app::{Appearance, GridApp};
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
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

        let appearance_field = SettingField::dropdown(
            vec![
                ("auto".into(), "Auto".into()),
                ("light".into(), "Light".into()),
                ("dark".into(), "Dark".into()),
            ],
            {
                let app = app.clone();
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
                app.update(cx, |this, cx| this.set_appearance(appearance, cx));
            },
        )
        .default_value("auto");

        Settings::new("fm1-settings").page(
            SettingPage::new("General").resettable(true).group(
                SettingGroup::new().title("Appearance").items(vec![
                    SettingItem::new("Theme", theme_field)
                        .description("The colour scheme family: gpui-kit's sets plus the bundled FM-1 ones.")
                        .keywords(["colors", "scheme"]),
                    SettingItem::new("Mode", appearance_field)
                        .description("Follow the OS light/dark setting, or pin one.")
                        .keywords(["dark", "light"]),
                ]),
            ),
        )
    }
}
