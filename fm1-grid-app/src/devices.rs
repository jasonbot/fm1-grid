//! The Devices section of the Devices tab: one row per live FM-1 unit on USB — its
//! name, its port labels, and the flash control on its own row. All device state
//! lives in [`fm1_performer::hardware`]; this only draws it and posts commands. The
//! header's Rescue toggle offers every system MIDI port as a flash target too, for
//! units too bricked to answer to their name; those flash through
//! `Hardware::rescue_flash`. The rows scroll (wheel, scrollbar, PgUp/PgDn); the
//! header and the flash board stay pinned.

use crate::app::GridApp;
use crate::firmware_picker::FirmwareDelegate;
use fm1_performer::hardware::DeviceRow;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::button::{Toggle, ToggleVariants};
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};

/// The flash progress bar's size.
const PROGRESS_W: f32 = 180.0;
const PROGRESS_H: f32 = 12.0;
/// The flash outcome report's indent, aligning it under the row's text.
const OUTCOME_INDENT: f32 = 52.0;
/// The flash firmware combobox's width in the row.
const FLASH_COMBO_W: f32 = 200.0;

impl GridApp {
    /// The Devices section: a pinned header (title, Rescue toggle, status), the
    /// scrollable rows, and the pinned flash board.
    pub(crate) fn devices_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (dim, danger, accent, warning) = {
            let theme = cx.theme();
            (
                theme.muted_foreground,
                theme.danger,
                theme.primary,
                theme.warning,
            )
        };
        let rows = self.hardware.devices();
        let board = self.hardware.flash_state();
        let error = self.hardware.error.clone();

        // The Rescue toggle: every system MIDI port becomes a flash target. The
        // warning ring says no FM-1 answered but something MIDI is out there.
        let rescue_on = self.rescue_on;
        let rescue_warn = self.rescue_warn;
        let app = cx.entity().clone();
        let header = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(dim)
                    .child("USB DEVICES"),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Toggle::new("rescue-toggle")
                            .label("Rescue")
                            .icon(Icon::new(Lucide::LifeBuoy))
                            .small()
                            .outline()
                            .checked(rescue_on)
                            .tooltip(
                                "Also offer every system MIDI port as a flash target — for \
                                 a unit too bricked to answer to its name",
                            )
                            .when(rescue_warn, |toggle| {
                                toggle.border_2().border_color(warning)
                            })
                            .on_click(move |on, window, cx| {
                                app.update(cx, |this, cx| {
                                    this.rescue_on = *on;
                                    this.refocus(window, cx);
                                    cx.notify();
                                });
                            }),
                    )
                    .child(match error {
                        Some(error) => div()
                            .text_size(px(9.5))
                            .text_color(danger)
                            .child(error),
                        None => div()
                            .text_size(px(9.0))
                            .text_color(dim)
                            .child("POLLS THE MIDI PORTS EVERY ½ S"),
                    }),
            );

        // The scrollable rows. Built before the container: the rows borrow `self`
        // mutably, the container borrows the scroll handle. Right padding clears
        // the overlay scrollbar (16 px, gpui-base's WIDTH).
        let mut content = v_flex().w_full().p_2().pr(px(16.0)).gap_2();
        if rows.is_empty() {
            content = content.child(
                div()
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child("No FM-1 connected — plug one in and it shows up here."),
            );
            if rescue_warn && !rescue_on {
                content = content.child(
                    div()
                        .text_size(px(9.5))
                        .text_color(dim)
                        .child(
                            "A unit stuck in its update loader never answers to its name \
                             — the Rescue toggle above offers every MIDI port as a flash \
                             target.",
                        ),
                );
            }
        }
        if rescue_on {
            if self.rescue_ports.is_empty() {
                content = content.child(
                    div()
                        .text_size(px(9.5))
                        .text_color(dim)
                        .child("Rescue is on, but no MIDI port is in sight."),
                );
            }
            for (ix, name) in self.rescue_ports.clone().into_iter().enumerate() {
                content = content.child(self.rescue_row(ix, &name, cx));
            }
        }
        for (ix, row) in rows.into_iter().enumerate() {
            content = content.child(self.device_row(ix, row, cx));
        }

        let app = cx.entity().clone();
        let mut pane = v_flex()
            .size_full()
            .overflow_hidden()
            .child(div().w_full().p_2().pb_0().child(header))
            .child(
                div()
                    .id("devices-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar(&self.devices_scroll)
                    // PgUp/PgDn scroll by the viewport; record it.
                    .on_prepaint(move |bounds, _, cx| {
                        let height: f32 = bounds.size.height.into();
                        app.update(cx, |this, _| this.devices_view_h = height);
                    })
                    .child(
                        div()
                            .id("devices-rows")
                            .w_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.devices_scroll)
                            .child(content),
                    ),
            );

        // The flash board, pinned under the list: the state of the running flash even
        // while the device's row is gone (it reboots through the loader), or the last
        // failure.
        if let Some(board) = board {
            pane = pane.child(
                div().w_full().px_2().pb_2().child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(if board.error { danger } else { accent })
                                .child(board.step.clone()),
                        )
                        .when(!board.error, |el| {
                            el.when_some(board.progress, |el, progress| {
                                el.child(
                                    div().w(px(PROGRESS_W)).h(px(PROGRESS_H)).child(
                                        Progress::new("flash-progress").value(progress * 100.0),
                                    ),
                                )
                            })
                        }),
                ),
            );
        }

        pane
    }

    /// One live unit: the name row (what it runs, then the dim port facts), the flash
    /// control on its own row, and the flash outcome report under it.
    fn device_row(&mut self, ix: usize, row: DeviceRow, cx: &mut Context<Self>) -> AnyElement {
        let (ink, dim, danger, accent) = {
            let theme = cx.theme();
            (
                theme.foreground,
                theme.muted_foreground,
                theme.danger,
                theme.primary,
            )
        };
        let name = h_flex()
            .w_full()
            .items_center()
            .flex_wrap()
            .gap_2()
            .child(
                div()
                    .id(("dev-name", ix))
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink)
                    .child(row.display.clone()),
            )
            .child(
                div()
                    .id(("dev-port", ix))
                    .text_size(px(9.5))
                    .text_color(dim)
                    .child(row.port.clone())
                    .tooltip({
                        let name = row.name.clone();
                        move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                    }),
            )
            .when_some(row.number, |el, number| {
                el.child(
                    div()
                        .id(("dev-client", ix))
                        .text_size(px(9.5))
                        .text_color(dim)
                        .child(format!("client {number}"))
                        .tooltip(|window, cx| {
                            Tooltip::new("The ALSA client number of the device")
                                .build(window, cx)
                        }),
                )
            })
            .when_some(row.usb.clone(), |el, usb| {
                el.child(
                    div()
                        .id(("dev-usb", ix))
                        .text_size(px(9.5))
                        .text_color(dim)
                        .child(format!("USB {usb}"))
                        .tooltip(|window, cx| {
                            Tooltip::new("The USB bus-port path").build(window, cx)
                        }),
                )
            });

        // The flash control's own row: the "Flash" toggle arms the always-present
        // firmware combobox. While a flash runs, the row hides and the pinned board
        // carries the state.
        let mut out = v_flex()
            .w_full()
            .child(name.into_any_element());
        if row.flash.is_none() {
            let armed = self.flash_armed.get(&row.name).copied().unwrap_or(false);
            let combo = self.flash_combos.get(&row.name).cloned();
            out = out.child(
                div()
                    .w_full()
                    .child(self.flash_control(("dev-flash", ix), &row.name, armed, combo, cx)),
            );
        }

        if let Some(outcome) = &row.outcome {
            let color = if outcome.starts_with("flashed:") {
                ink
            } else if outcome.starts_with("step 1 done") {
                accent
            } else {
                danger
            };
            out = out.child(
                div()
                    .pl(px(OUTCOME_INDENT))
                    .text_size(px(10.0))
                    .text_color(color)
                    .child(outcome.clone()),
            );
        }
        out.into_any_element()
    }

    /// One rescue target: a system MIDI port offered while the Rescue toggle is on,
    /// captioned "MIDI RESCUE: <port>", with the same flash control a live unit's
    /// row has.
    fn rescue_row(&mut self, ix: usize, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let ink = cx.theme().foreground;
        let armed = self.flash_armed.get(name).copied().unwrap_or(false);
        let combo = self.rescue_combos.get(name).cloned();
        v_flex()
            .w_full()
            .child(
                div()
                    .id(("rescue-port", ix))
                    .test_support()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink)
                    .child(format!("MIDI RESCUE: {name}"))
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            "No FM-1 answered to this port's name — flash it only if it \
                             is a unit stuck in its update loader",
                        )
                        .build(window, cx)
                    }),
            )
            .child(self.flash_control(("rescue-flash", ix), name, armed, combo, cx))
            .into_any_element()
    }

    /// The flash control every flashable row shares — a live unit's or a rescue
    /// port's: the "Flash" toggle arms the always-present firmware combobox. The
    /// combobox's subscription does the flashing; the sweep builds the combobox.
    fn flash_control(
        &self,
        switch: impl Into<ElementId>,
        name: &str,
        armed: bool,
        combo: Option<Entity<ComboboxState<FirmwareDelegate>>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = name.to_string();
        h_flex()
            .items_center()
            .gap_2()
            .child(
                Switch::new(switch)
                    .label("Flash")
                    .checked(armed)
                    .tooltip("Arm flashing, then pick a firmware to flash this unit")
                    .on_change(cx.listener(move |this, &on, window, cx| {
                        this.flash_armed.insert(name.clone(), on);
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            )
            .child(match combo {
                Some(state) => div()
                    .w(px(FLASH_COMBO_W))
                    .child(
                        Combobox::new(&state)
                            .placeholder("flash firmware…")
                            .search_placeholder("Filter…")
                            .menu_width(px(crate::theme::COMBO_MENU_W))
                            .disabled(!armed)
                            .empty(|_, cx| {
                                div()
                                    .p_2()
                                    .text_size(px(10.0))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No firmware matches")
                            }),
                    )
                    .into_any_element(),
                // The sweep builds the combobox the moment a target appears; hold its
                // space until then so the row doesn't jump.
                None => div()
                    .w(px(FLASH_COMBO_W))
                    .h(px(28.0))
                    .into_any_element(),
            })
            .into_any_element()
    }

    /// Build a row's flash combobox with its subscriptions: a pick flashes the device.
    /// Called from the sweep (which has the window) when a device first appears;
    /// dropped when it goes. With `rescue`, the row belongs to a MIDI port no FM-1
    /// answered from: the pick goes to `Hardware::rescue_flash`, and problems land on
    /// the pane-level flash board (a rescue target has no row of its own).
    /// shortcut: each combo leaves two inert Subscriptions in `_subscriptions` after
    /// its device goes away; worth pruning only if hotplug churn ever runs hot.
    pub(crate) fn open_flash_picker(&mut self, device: &str, rescue: bool, window: &mut Window, cx: &mut Context<Self>) {
        let delegate = FirmwareDelegate::from_app(self);
        let state = cx.new(|cx| {
            ComboboxState::new(delegate, vec![], window, cx).searchable(true)
        });
        let picked = device.to_string();
        self._subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, _, event, _window, cx| {
                if let ComboboxEvent::Change(values) = event {
                    if let Some(pick) = values.last() {
                        match (rescue, pick.bytes(this.store.as_ref())) {
                            (true, Ok(bytes)) => this.hardware.rescue_flash(&picked, bytes),
                            (false, Ok(bytes)) => this.hardware.flash(&picked, bytes),
                            (true, Err(e)) => this.hardware.report_board(e),
                            (false, Err(e)) => this.hardware.report(&picked, e),
                        }
                    }
                    cx.notify();
                }
            },
        ));
        self._subscriptions.push(cx.subscribe_in(
            &state,
            window,
            |this, _, _: &DismissEvent, window, cx| {
                this.focus.focus(window, cx);
            },
        ));
        if rescue {
            self.rescue_combos.insert(device.to_string(), state);
        } else {
            self.flash_combos.insert(device.to_string(), state);
        }
    }
}
