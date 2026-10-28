//! The MIDI graph editor: one row per connected source ("out → in in in"), edited with
//! chips and dropdown menus. System ports, running panes and the live FM-1 units are
//! all endpoints; the live units piggyback on the hardware module's open ports. All
//! graph state lives in [`fm1_performer::routing`]; this only draws it and posts commands. It
//! sits at the bottom of the device sidebar, scrolling when the links outgrow the
//! space the sidebar leaves.

use crate::app::GridApp;
use fm1_performer::routing::Port;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};

impl GridApp {
    /// The MIDI GRAPH section: the header, then the rows in a scroll region that
    /// takes the height the sidebar leaves it.
    pub(crate) fn graph_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (ink, dim) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground)
        };
        let endpoints = self.routing.endpoints();
        let links = self.routing.links();
        let label = |port: &Port| -> String {
            endpoints
                .iter()
                .find(|endpoint| &endpoint.port == port)
                .map_or_else(|| "gone".to_string(), |endpoint| endpoint.label.clone())
        };

        let mut sources: Vec<Port> = Vec::new();
        for (source, _) in &links {
            if !sources.contains(source) {
                sources.push(source.clone());
            }
        }

        let app = cx.entity().clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (row_ix, source) in sources.iter().enumerate() {
            let mut row = h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(ink)
                        .child(label(source)),
                )
                .child(div().text_size(px(11.0)).text_color(dim).child("→"));

            for (sink_ix, (src, sink)) in
                links.iter().filter(|(s, _)| s == source).enumerate()
            {
                let (src, sink) = (src.clone(), sink.clone());
                // A pane whose firmware advertises USB-MIDI has MIDI out on real
                // hardware; in the emulator only its MIDI IN jack exists.
                let tip = match &sink {
                    Port::Pane(slot) if self.slots[*slot].usb_midi => {
                        format!(
                            "Remove this connection. {} advertises USB-MIDI in/out on real \
                             hardware; the emulator bridges its MIDI IN only, so its MIDI \
                             out stays silent.",
                            label(&sink)
                        )
                    }
                    _ => "Remove this connection".to_string(),
                };
                row = row.child(
                    Button::new(("sink-chip", row_ix * 1024 + sink_ix))
                        .label(format!("{} ×", label(&sink)))
                        .compact()
                        .small()
                        .tooltip(tip)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.routing.disconnect(src.clone(), sink.clone());
                            this.refocus(window, cx);
                            cx.notify();
                        })),
                );
            }

            // The row's "+": every sink this source is not already connected to.
            let candidates: Vec<(Port, String)> = endpoints
                .iter()
                .filter(|endpoint| {
                    endpoint.sink
                        && &endpoint.port != source
                        && !links
                            .iter()
                            .any(|(s, d)| s == source && d == &endpoint.port)
                })
                .map(|endpoint| (endpoint.port.clone(), endpoint.label.clone()))
                .collect();
            let app_menu = app.clone();
            let source_menu = source.clone();
            row = row.child(
                Button::new(("add-sink", row_ix))
                    .label("+")
                    .compact()
                    .small()
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        if candidates.is_empty() {
                            menu = menu.item(PopupMenuItem::new("no other sinks").disabled(true));
                        }
                        for (port, label) in &candidates {
                            let app = app_menu.clone();
                            let source = source_menu.clone();
                            let port = port.clone();
                            let label = label.clone();
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    let source = source.clone();
                                    let port = port.clone();
                                    app.update(cx, |this, cx| {
                                        this.routing.connect(source, port);
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                        menu
                    }),
            );
            rows.push(row.into_any_element());
        }

        // The add row: pick a source, then its first sink.
        let source_candidates: Vec<(Port, String)> = endpoints
            .iter()
            .filter(|endpoint| endpoint.source)
            .map(|endpoint| (endpoint.port.clone(), endpoint.label.clone()))
            .collect();
        let app_menu = app.clone();
        let mut add_row = h_flex().w_full().flex_wrap().items_center().gap_1().child(
            Button::new("graph-add-source")
                .label(if self.pending_source.is_some() {
                    "connecting…"
                } else {
                    "+ connect a MIDI out…"
                })
                .compact()
                .small()
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    if source_candidates.is_empty() {
                        menu = menu.item(PopupMenuItem::new("no MIDI outs in sight").disabled(true));
                    }
                    for (port, label) in &source_candidates {
                        let app = app_menu.clone();
                        let port = port.clone();
                        let label = label.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(
                            move |_, window, cx| {
                                let port = port.clone();
                                app.update(cx, |this, cx| {
                                    this.pending_source = Some(port);
                                    this.focus.focus(window, cx);
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    menu
                }),
        );
        if let Some(pending) = self.pending_source.clone() {
            add_row = add_row
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(ink)
                        .child(label(&pending)),
                )
                .child(div().text_size(px(11.0)).text_color(dim).child("→"));
            let sink_candidates: Vec<(Port, String)> = endpoints
                .iter()
                .filter(|endpoint| endpoint.sink && endpoint.port != pending)
                .map(|endpoint| (endpoint.port.clone(), endpoint.label.clone()))
                .collect();
            let app_menu = app.clone();
            let pending_menu = pending.clone();
            add_row = add_row.child(
                Button::new("graph-pending-sink")
                    .label("to…")
                    .compact()
                    .small()
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for (port, label) in &sink_candidates {
                            let app = app_menu.clone();
                            let pending = pending_menu.clone();
                            let port = port.clone();
                            let label = label.clone();
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    let pending = pending.clone();
                                    let port = port.clone();
                                    app.update(cx, |this, cx| {
                                        this.routing.connect(pending, port);
                                        this.pending_source = None;
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                        menu
                    }),
            );
            add_row = add_row.child(
                Button::new("graph-pending-cancel")
                    .label("×")
                    .compact()
                    .small()
                    .tooltip("Cancel this connection")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pending_source = None;
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            );
        }
        rows.push(add_row.into_any_element());

        // The graph's honest footnote: a running pane whose firmware carries a USB
        // MIDIStreaming interface would show up as a bidirectional USB-MIDI device on
        // real hardware; the emulated SoC can't enumerate USB, so only its TRS MIDI IN
        // jack exists here.
        let usb_note = self
            .slots
            .iter()
            .any(|slot| slot.handle.is_some() && slot.usb_midi);

        v_flex()
            .size_full()
            .p_2()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(dim)
                    .child("MIDI GRAPH"),
            )
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(dim)
                    .child("ONE OUT FEEDS ANY NUMBER OF INS"),
            )
            .when(usb_note, |el| {
                el.child(
                    div()
                        .id("graph-usb-note")
                        .test_support()
                        .text_size(px(9.0))
                        .text_color(dim)
                        .child(
                            "RUNNING PANES ADVERTISE USB-MIDI IN/OUT ON HARDWARE — THE \
                             EMULATOR BRIDGES MIDI IN ONLY",
                        ),
                )
            })
            .child(
                div()
                    .id("midi-graph-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().w_full().gap(px(4.0)).children(rows)),
            )
    }
}
