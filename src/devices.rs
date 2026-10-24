//! The devices pane under the grid: one row per live FM-1 unit on USB — its clock
//! switch, its port labels, and the flash control with its progress board. All device
//! state lives in [`crate::hardware`]; this only draws it and posts commands.

use crate::app::GridApp;
use crate::builtin::BUILTINS;
use crate::firmware_picker::FirmwareDelegate;
use crate::hardware::DeviceRow;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

impl GridApp {
    /// The devices pane, as wide as the grid, under it. Theme colours like the sidebar.
    pub(crate) fn devices_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (surface, border, dim, danger, accent) = {
            let theme = cx.theme();
            (
                theme.popover,
                theme.border,
                theme.muted_foreground,
                theme.danger,
                theme.primary,
            )
        };
        let rows = self.hardware.devices();
        let board = self.hardware.flash_state();
        let error = self.hardware.error.clone();

        let mut pane = v_flex()
            .w_full()
            .rounded(px(10.0))
            .bg(surface)
            .border_1()
            .border_color(border)
            .p(px(10.0))
            .gap(px(2.0))
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
                                div().w(px(180.0)).h(px(12.0)).child(
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
        let name = row.name.clone();

        let mut line = h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                Switch::new(("dev-clock", ix))
                    .checked(row.clock)
                    .tooltip("Send the metronome's MIDI clock to this device")
                    .on_change(cx.listener(move |this, &on, window, cx| {
                        this.hardware.set_clock(&name, on);
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            )
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

        // The flash control on the right: a button that gives way to the searchable
        // firmware combobox and a dismiss button, or nothing while a flash runs.
        let right: Option<AnyElement> = if row.flash.is_some() {
            None
        } else if self.flash_pick.as_deref() == Some(row.name.as_str()) {
            let combo = self.flash_combo.clone();
            Some(
                h_flex()
                    .items_center()
                    .gap_1()
                    .when_some(combo, |el, state| {
                        el.child(
                            div().w(px(200.0)).child(
                                Combobox::new(&state)
                                    .placeholder("flash firmware…")
                                    .search_placeholder("Filter…")
                                    .menu_width(px(260.0)),
                            ),
                        )
                    })
                    .child(
                        Button::new(("flash-dismiss", ix))
                            .label("×")
                            .compact()
                            .small()
                            .tooltip("Dismiss")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.flash_pick = None;
                                this.flash_combo = None;
                                this.refocus(window, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            )
        } else {
            let name = row.name.clone();
            Some(
                Button::new(("flash", ix))
                    .label("Flash")
                    .compact()
                    .small()
                    .tooltip("Flash a firmware to this device")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_flash_picker(&name, window, cx);
                    }))
                    .into_any_element(),
            )
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
                    .pl(px(52.0))
                    .text_size(px(10.0))
                    .text_color(color)
                    .child(outcome.clone()),
            );
        }
        out.into_any_element()
    }

    /// Open the row's firmware combobox: a fresh state with its pick and dismiss
    /// subscriptions. Picking flashes the device; dismissing (Escape, click away)
    /// closes the row's selector.
    /// shortcut: each open leaves two inert Subscriptions in `_subscriptions` (their
    /// source entity is dropped on close); worth pruning only if this ever runs hot.
    fn open_flash_picker(&mut self, device: &str, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| {
            ComboboxState::new(FirmwareDelegate::new(), vec![], window, cx).searchable(true)
        });
        let picked = device.to_string();
        self._subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, _, event, window, cx| {
                if let ComboboxEvent::Change(values) = event {
                    match values.last() {
                        Some(&Some(index)) => {
                            this.hardware
                                .flash(&picked, BUILTINS[index].bytes.to_vec());
                        }
                        Some(&None) => this.flash_file(&picked),
                        None => {}
                    }
                    this.flash_pick = None;
                    this.flash_combo = None;
                    this.focus.focus(window, cx);
                    cx.notify();
                }
            },
        ));
        self._subscriptions.push(cx.subscribe_in(
            &state,
            window,
            |this, _, _: &DismissEvent, _, cx| {
                this.flash_pick = None;
                this.flash_combo = None;
                cx.notify();
            },
        ));
        self.flash_pick = Some(device.to_string());
        self.flash_combo = Some(state);
        cx.notify();
    }

    /// Pick a package and hand it to the hardware module; the flash itself runs on its
    /// own thread and reports through the device's row.
    fn flash_file(&mut self, name: &str) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose an FM-1 firmware package")
            .add_filter("FM-1 package", &["fwsc"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => self.hardware.flash(name, bytes),
            Err(e) => self.hardware.report(name, format!("{}: {e}", path.display())),
        }
    }
}
