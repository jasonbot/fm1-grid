//! The devices pane under the grid: one row per live FM-1 unit on USB — its clock
//! switch, its port labels, and the flash control with its progress board. All device
//! state lives in [`fm1_performer::hardware`]; this only draws it and posts commands.
//! When no FM-1 is found at all, the pane offers every system MIDI port as a flash
//! target instead (the "MIDI RESCUE: …" rows), for units too bricked to answer to
//! their name; those flash through `Hardware::rescue_flash`.

use crate::app::GridApp;
use crate::firmware_picker::FirmwareDelegate;
use fm1_performer::hardware::DeviceRow;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// The flash progress bar's size.
const PROGRESS_W: f32 = 180.0;
const PROGRESS_H: f32 = 12.0;
/// The flash outcome report's indent, aligning it under the row's text.
const OUTCOME_INDENT: f32 = 52.0;
/// The flash firmware combobox's width in the row.
const FLASH_COMBO_W: f32 = 200.0;

impl GridApp {
    /// The Devices tab's left half: the live USB units. Bare like the graph half —
    /// the split's divider is the only frame.
    pub(crate) fn devices_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (dim, danger, accent) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.danger, theme.primary)
        };
        let rows = self.hardware.devices();
        let board = self.hardware.flash_state();
        let error = self.hardware.error.clone();

        // overflow_hidden: narrow splits must clip, never bleed into the graph half.
        let mut pane = v_flex().size_full().overflow_hidden().gap(px(2.0))
            .child(
                h_flex()
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

        if rows.is_empty() {
            pane = pane.child(
                div()
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child("No FM-1 connected — plug one in and it shows up here."),
            );
            // MIDI rescue: a unit bricked out of its FM-1 name never gets a row, so
            // every system MIDI port is offered as a flash target instead. Not while
            // a flash is running — that empties the rows too (the unit reboots).
            let flashing = board.as_ref().is_some_and(|state| !state.error);
            if !flashing && !self.rescue_ports.is_empty() {
                pane = pane.child(
                    div()
                        .text_size(px(9.5))
                        .text_color(dim)
                        .child(
                            "A UNIT STUCK IN ITS UPDATE LOADER MAY NOT ANSWER TO ITS \
                             NAME — EVERY MIDI PORT BELOW IS FLASHABLE:",
                        ),
                );
                for (ix, name) in self.rescue_ports.clone().into_iter().enumerate() {
                    pane = pane.child(self.rescue_row(ix, &name, cx));
                }
            }
        }

        for (ix, row) in rows.into_iter().enumerate() {
            pane = pane.child(self.device_row(ix, row, cx));
        }

        // The flash board: the state of the running flash even while the device's row
        // is gone (it reboots through the loader), or the last failure.
        if let Some(board) = board {
            pane = pane.child(
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
            );
        }

        pane
    }

    /// One live unit: clock switch, port labels, and the flash control on the right.
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
        let mut line = h_flex()
            .w_full()
            .items_center()
            .flex_wrap()
            .gap_2()
            .child(
                div()
                    .id(("dev-port", ix))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink)
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
            })
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(ink)
                    .child(row.display.clone()),
            );

        // The flash control on the right: the "Flash" toggle arms the always-present
        // firmware combobox. While a flash runs, the row shows the flash state.
        let armed = self.flash_armed.get(&row.name).copied().unwrap_or(false);
        let right: Option<AnyElement> = if row.flash.is_some() {
            None
        } else {
            let combo = self.flash_combos.get(&row.name).cloned();
            Some(self.flash_control(("dev-flash", ix), &row.name, armed, combo, cx))
        };
        if let Some(right) = right {
            line = line.child(div().ml_auto().child(right));
        }

        // The flash outcome report sits on a second, indented line.
        let mut out = v_flex().w_full().child(line.into_any_element());
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

    /// One rescue target: a system MIDI port no FM-1 answered from, captioned
    /// "MIDI RESCUE: <port>", with the same flash control a live unit's row has.
    fn rescue_row(&mut self, ix: usize, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let ink = cx.theme().foreground;
        let armed = self.flash_armed.get(name).copied().unwrap_or(false);
        let combo = self.rescue_combos.get(name).cloned();
        h_flex()
            .w_full()
            .items_center()
            .flex_wrap()
            .gap_2()
            .child(
                div()
                    .id(("rescue-port", ix))
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
            .child(
                div().ml_auto().child(
                    self.flash_control(("rescue-flash", ix), name, armed, combo, cx),
                ),
            )
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
