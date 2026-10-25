//! The device sidebar: the selected pane's screen, its power switch, firmware selector
//! and status, and the MIDI graph editor at the bottom. Theme colours throughout; the
//! only computed colour in the window is the grid background behind it.

use crate::app::{GridApp, Source};
use crate::lcd;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::component::combobox::Combobox;
use gpui_kit::component::switch::Switch;
use gpui_kit::*;

/// Exact pixel multiples the screen can show at, largest first (4x, 2x, 1x).
const SCREEN_SIDES: [f32; 3] = [960.0, 480.0, 240.0];
/// Horizontal chrome around the screen: the card's padding and the bezel.
const FIT_W_CHROME: f32 = 26.0;
/// Vertical chrome: title, attributes, status, note, gaps and padding.
const FIT_H_CHROME: f32 = 164.0;
/// Smallest screen side the layout falls back to (fractional fit).
const SCREEN_MIN: f32 = 80.0;

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

        // The screen shows at an exact pixel multiple — 4x, 2x or 1x (960/480/240),
        // the largest that fits the width the split gives the panel and the height the
        // title/attributes/status/note leave. Computed from last frame's measured
        // content size (one frame of lag, as with the graph height in the egui app).
        let (panel_w, panel_h) = self.sidebar_size;
        let fit_w = panel_w - FIT_W_CHROME;
        let fit_h = panel_h - FIT_H_CHROME;
        let fit = fit_w.min(fit_h).clamp(SCREEN_MIN, SCREEN_SIDES[0]);
        let side = SCREEN_SIDES
            .into_iter()
            .find(|&exact| exact <= fit)
            .unwrap_or(fit);
        self.sidebar_scale = (side / SCREEN_SIDES[2]).round() as usize;

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

        // Record the rendered size for next frame's scale choice.
        let app = cx.entity().clone();
        v_flex()
            .id("device-sidebar")
            .size_full()
            .rounded(px(10.0))
            .bg(surface)
            .border_1()
            .border_color(border)
            .p_3()
            .gap_3()
            .on_prepaint(move |bounds, _, cx| {
                let size = (bounds.size.width.into(), bounds.size.height.into());
                app.update(cx, |this, _| {
                    if this.sidebar_size != size {
                        this.sidebar_size = size;
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
            .child(if pinned {
                div()
                    .id(("firmware", slot))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.0))
                    .child(label)
                    .into_any_element()
            } else {
                combo_host.child(combo).into_any_element()
            })
            .child(
                Button::new(("pin", slot))
                    .xsmall()
                    .label("pinned")
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
