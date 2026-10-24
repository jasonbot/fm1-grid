//! The device sidebar: the selected pane's screen, its power switch, firmware selector
//! and status, and the MIDI graph editor at the bottom. Theme colours throughout; the
//! only computed colour in the window is the grid background behind it.

use crate::app::{GridApp, Source};
use crate::lcd;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::component::combobox::Combobox;
use gpui_kit::component::switch::Switch;
use gpui_kit::*;

impl GridApp {
    /// The sidebar takes a fixed share of the window's width (the grid keeps the rest)
    /// and its full height. The selected screen shows at an exact pixel multiple —
    /// 4x, 2x or 1x (960/480/240), the largest that fits the width the sidebar had
    /// last frame — so every LCD pixel maps to a clean block of screen pixels (the
    /// image arrives hqx-upscaled to match).
    pub fn device_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let slot = self.active;
        let (surface, border, ink, dim, danger) = {
            let theme = cx.theme();
            (
                theme.popover,
                theme.border,
                theme.foreground,
                theme.muted_foreground,
                theme.danger,
            )
        };

        // The exact-multiple fit, from the width measured during last frame's prepaint
        // (the same one-frame lag the egui layout had with the graph's height).
        let inner_w = (self.sidebar_width - 26.0).max(80.0);
        let fit = inner_w.min(960.0);
        let side = [960.0, 480.0, 240.0]
            .into_iter()
            .find(|&exact| exact <= fit)
            .unwrap_or(fit);
        self.sidebar_scale = (side / 240.0).round() as usize;

        let name = self.slots[slot]
            .label
            .clone()
            .unwrap_or_else(|| "No firmware".to_string());

        let status = match (&self.slots[slot].error, &self.slots[slot].handle) {
            (Some(error), _) => error.clone(),
            (None, Some(handle)) => handle.status(),
            (None, None) => "Off".to_string(),
        };
        let status_bad = self.slots[slot].error.is_some()
            || self.slots[slot]
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_running());

        let note = match &self.slots[slot].label {
            Some(label) => format!("No custom behavior for {label}"),
            None => format!("No firmware loaded in pane {}", slot + 1),
        };

        // Record the rendered width for next frame's scale choice.
        let app = cx.entity().clone();
        v_flex()
            .id("device-sidebar")
            .w(DefiniteLength::Fraction(0.32))
            .min_w(px(280.0))
            .max_w(px(620.0))
            .h_full()
            .rounded(px(10.0))
            .bg(surface)
            .border_1()
            .border_color(border)
            .p_3()
            .gap_3()
            .on_prepaint(move |bounds, _, cx| {
                let width: f32 = bounds.size.width.into();
                app.update(cx, |this, _| {
                    if (this.sidebar_width - width).abs() > 0.5 {
                        this.sidebar_width = width;
                    }
                });
            })
            .child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink)
                    .child(format!("Device {}: {}", slot + 1, name)),
            )
            // The screen, centred horizontally.
            .child(h_flex().w_full().justify_center().child(self.sidebar_screen(side)))
            .child(self.pane_attributes(cx))
            .child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(px(10.0))
                    .text_color(if status_bad { danger } else { dim })
                    .child(status),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child(note),
            )
            .child(self.graph_section(cx))
    }

    /// The selected pane's screen in its dark bezel, at the exact upscale the sidebar's
    /// width allows.
    fn sidebar_screen(&self, side: f32) -> impl IntoElement {
        let picture = self.frames[self.active].clone();
        div()
            .rounded(px(6.0))
            .bg(rgb(lcd::BEZEL))
            .p(px(3.0))
            .child(match picture {
                Some(image) => img(image)
                    .w(px(side))
                    .h(px(side))
                    .into_any_element(),
                None => div()
                    .w(px(side))
                    .h(px(side))
                    .bg(rgb(0x000000))
                    .into_any_element(),
            })
    }

    /// The selected pane's attributes — power switch and firmware selector — under the
    /// screen. The cards are display-only, so this is where firmware switching lives.
    fn pane_attributes(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let slot = self.active;
        let on = self.powered[slot];
        let label = self.slots[slot]
            .label
            .clone()
            .unwrap_or_else(|| "Choose firmware…".to_string());
        let path_tip = match &self.slots[slot].source {
            Some(Source::File(path)) => Some(path.display().to_string()),
            _ => None,
        };

        // Firmware can only be changed while the pane is powered off.
        let combo = Combobox::new(&self.firmware_combos[slot])
            .placeholder("Choose firmware…")
            .search_placeholder("Filter…")
            .menu_width(px(260.0))
            .disabled(on)
            .empty(|_, cx| {
                div()
                    .p_2()
                    .text_size(px(10.0))
                    .text_color(cx.theme().muted_foreground)
                    .child("No firmware matches")
            })
            .render_trigger(move |_, _, _| {
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.0))
                            .child(label.clone()),
                    )
                    .child(Icon::new(IconName::ChevronDown).xsmall())
            });

        let combo_host = div().id(("firmware", slot)).flex_1().min_w_0();
        let combo_host = match path_tip {
            Some(tip) => combo_host.tooltip(move |window, cx| {
                Tooltip::new(tip.clone()).build(window, cx)
            }),
            None => combo_host,
        };

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                Switch::new("pane-power")
                    .checked(on)
                    .on_change(cx.listener(move |this, &on, window, cx| {
                        this.set_power(slot, on);
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            )
            .child(combo_host.child(combo))
    }
}
