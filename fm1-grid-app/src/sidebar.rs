//! The device sidebar: the selected pane's controls (power toggle, firmware
//! selector, pin) across the top, its screen centred in the spare height, and the
//! slot number pill at the bottom right. The status line lives in the transport's
//! metronome row.
//! Theme colours throughout; the only computed colour in the window is the grid
//! background behind it.

use crate::app::{GridApp, Source};
use crate::lcd;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants, Toggle, ToggleVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::component::combobox::Combobox;
use gpui_kit::*;

/// Exact pixel multiples the screen can show at, largest first (4x, 2x, 1x).
const SCREEN_SIDES: [f32; 3] = [960.0, 480.0, 240.0];
/// Horizontal chrome around the screen: the panel's padding and the bezel.
const FIT_W_CHROME: f32 = 22.0;
/// Vertical chrome: the controls row, the slot-pill row, the gaps and the padding.
const FIT_H_CHROME: f32 = 92.0;
/// Smallest screen side the layout falls back to (fractional fit).
const SCREEN_MIN: f32 = 80.0;
/// The firmware combobox renders at the component's medium input height (32 px); the
/// pinned label matches it, so pinning never changes the row's height.
const COMBO_H: f32 = 32.0;

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
        let surface = cx.theme().popover;

        // The screen shows at an exact pixel multiple — 4x, 2x or 1x (960/480/240),
        // the largest that fits the width the split gives the panel and the height the
        // attributes row leaves. Computed from last frame's measured content size (one
        // frame of lag, as with the graph height in the egui app).
        let (panel_w, panel_h) = self.sidebar_size;
        let fit_w = panel_w - FIT_W_CHROME;
        let fit_h = panel_h - FIT_H_CHROME;
        let fit = fit_w.min(fit_h).clamp(SCREEN_MIN, SCREEN_SIDES[0]);
        let side = SCREEN_SIDES
            .into_iter()
            .find(|&exact| exact <= fit)
            .unwrap_or(fit);
        self.sidebar_scale = (side / SCREEN_SIDES[2]).round() as usize;

        // Record the rendered size for next frame's scale choice.
        let app = cx.entity().clone();
        v_flex()
            .id("device-sidebar")
            .size_full()
            .bg(surface)
            .p_2()
            .gap_2()
            .on_prepaint(move |bounds, _, cx| {
                let size = (bounds.size.width.into(), bounds.size.height.into());
                app.update(cx, |this, _| {
                    if this.sidebar_size != size {
                        this.sidebar_size = size;
                    }
                });
            })
            // The controls pin to the top at their natural size; the screen fills
            // the spare height and centres itself in it. The slot pill sits at the
            // bottom right on its own row.
            .child(self.pane_attributes(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.sidebar_screen(side)),
            )
            .child(
                h_flex().w_full().justify_end().child(
                    div()
                        .id("slot-pill")
                        .test_support()
                        .rounded_full()
                        .px(px(10.0))
                        .py(px(2.0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(cx.theme().muted)
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}", self.active + 1)),
                ),
            )
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

    /// The selected pane's attributes — slot pill, power toggle and firmware selector
    /// across the top of the sidebar, the selector growing to fill the row's width.
    /// The cards are display-only, so this is where firmware switching lives.
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
        let combo_label = label.clone();
        let combo = Combobox::new(&self.firmware_combos[slot])
            .placeholder("Choose firmware…")
            .search_placeholder("Filter…")
            .menu_width(px(crate::theme::COMBO_MENU_W))
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
                            .child(combo_label.clone()),
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

        // A pinned pane keeps its device storage across power-offs and runs; the
        // selector collapses to the pinned firmware's name. The toggle only moves
        // while the pane runs — a stopped pane keeps its state as is.
        let pinned = self.slots[slot].pinned;
        let running = self.slots[slot].handle.is_some();
        let app = cx.entity().clone();
        h_flex()
            .id("pane-attrs")
            .test_support()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                Toggle::new("pane-power")
                    .icon(Icon::new(Lucide::Power))
                    .large()
                    .outline()
                    .checked(on)
                    .tooltip(if on { "Power off" } else { "Power on" })
                    .on_click(move |on, window, cx| {
                        app.update(cx, |this, cx| {
                            this.set_power(slot, *on);
                            this.refocus(window, cx);
                            cx.notify();
                        });
                    }),
            )
            .child(if pinned {
                div()
                    .id(("firmware", slot))
                    .flex_1()
                    .min_w_0()
                    .h(px(COMBO_H))
                    .flex()
                    .items_center()
                    .truncate()
                    .text_size(px(11.0))
                    .child(label)
                    .into_any_element()
            } else {
                combo_host.child(combo).into_any_element()
            })
            .child(
                Button::new(("pin", slot))
                    .icon(Icon::new(Lucide::Pin))
                    .tooltip(if running {
                        "Keep this pane's device storage across runs"
                    } else {
                        "Start the pane to pin or unpin it"
                    })
                    .when(pinned, |button| button.primary())
                    .disabled(!running)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_pinned(slot, !pinned);
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            )
    }
}
