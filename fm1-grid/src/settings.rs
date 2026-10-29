//! The Settings tab: the theme family picker (gpui-kit's bundled sets) and the
//! auto/light/dark appearance override. Both apply at once and land in the session
//! record on change.

use crate::app::{Appearance, GridApp};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, Toggle, ToggleGroup, ToggleVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::*;

impl GridApp {
    /// The Settings tab: one labelled row per setting, left-aligned in the pane.
    pub(crate) fn settings_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dim = cx.theme().muted_foreground;

        // The theme family picker: the current family's label on the button, a check
        // marking it in the menu.
        let current_id = self.theme_id.clone();
        let current_label = crate::theme::family(&self.theme_id).label;
        let app = cx.entity().clone();
        let theme_picker = Button::new("settings-theme")
            .label(current_label)
            .icon(Icon::new(Lucide::Palette))
            .outline()
            .flex_1()
            .min_w_0()
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.scrollable(true).max_h(px(400.0));
                for family in crate::theme::theme_families() {
                    let app = app.clone();
                    let checked = family.id == current_id;
                    menu = menu.item(PopupMenuItem::new(family.label).checked(checked).on_click(
                        move |_, window, cx| {
                            app.update(cx, |this, cx| this.set_theme_family(family.id, window, cx));
                        },
                    ));
                }
                menu
            });

        // The appearance override as a segmented radio: the group's callback carries
        // the checks with the clicked bit flipped; the diff against the rendered
        // state names the click. Clicking the active one is a no-op.
        let current = self.appearance as usize;
        let app = cx.entity().clone();
        let appearance_picker = ToggleGroup::new("settings-appearance")
            .segmented()
            .outline()
            .child(
                Toggle::new(("settings-appearance", 0usize))
                    .label("Auto")
                    .icon(Icon::new(Lucide::SunMoon))
                    .tooltip("Follow the OS light/dark setting")
                    .checked(current == 0),
            )
            .child(
                Toggle::new(("settings-appearance", 1usize))
                    .label("Light")
                    .icon(Icon::new(Lucide::Sun))
                    .tooltip("Always light")
                    .checked(current == 1),
            )
            .child(
                Toggle::new(("settings-appearance", 2usize))
                    .label("Dark")
                    .icon(Icon::new(Lucide::Moon))
                    .tooltip("Always dark")
                    .checked(current == 2),
            )
            .on_click(move |states, window, cx| {
                let Some(ix) = states
                    .iter()
                    .enumerate()
                    .position(|(ix, on)| *on != (ix == current))
                else {
                    return;
                };
                if ix == current {
                    return;
                }
                let appearance = match ix {
                    1 => Appearance::Light,
                    2 => Appearance::Dark,
                    _ => Appearance::Auto,
                };
                app.update(cx, |this, cx| this.set_appearance(appearance, window, cx));
            });

        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(dim)
                    .child("SETTINGS"),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(theme_picker)
                    .child(appearance_picker),
            )
    }
}
