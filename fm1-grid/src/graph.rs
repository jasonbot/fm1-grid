//! The MIDI graph editor: one always-on section per source endpoint ("INPUT" rows —
//! system MIDI outs, live FM-1 units), listing the sinks it feeds with an (X) pill
//! to unroute each, and an always-there Connect dropdown to route another. Running
//! panes and system ports are sinks; the live units piggyback on the hardware
//! module's open ports. All graph state lives in [`fm1_performer::routing`]; this
//! only draws it and posts commands.

use crate::app::GridApp;
use fm1_performer::routing::Port;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};

impl GridApp {
    /// The MIDI Graph section: every source endpoint gets a section, linked or not.
    /// The sections scroll (wheel, scrollbar, PgUp/PgDn); the heading lines stay
    /// pinned.
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

        let app = cx.entity().clone();
        let mut sections: Vec<AnyElement> = Vec::new();
        for (row_ix, source) in endpoints
            .iter()
            .filter(|endpoint| endpoint.source)
            .enumerate()
        {
            let source = source.port.clone();
            let mut section = v_flex()
                .w_full()
                .gap_1()
                .child(
                    div()
                        .id(("graph-source", row_ix))
                        .test_support()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(ink)
                        .child(label(&source)),
                );

            // The sinks this source feeds, each with its (X) unroute pill.
            for (sink_ix, (_, sink)) in
                links.iter().filter(|(s, _)| *s == source).enumerate()
            {
                let sink = sink.clone();
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
                let src = source.clone();
                section = section.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(ink)
                                .child(label(&sink)),
                        )
                        .child(
                            Button::new(("sink-x", row_ix * 1024 + sink_ix))
                                .icon(Icon::new(Lucide::X))
                                .accessibility_label("unroute")
                                .tooltip(tip)
                                .compact()
                                .xsmall()
                                .rounded(px(999.0))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.routing.disconnect(src.clone(), sink.clone());
                                    this.refocus(window, cx);
                                    cx.notify();
                                })),
                        ),
                );
            }

            // The always-there router dropdown: every sink this source is not
            // already connected to.
            let candidates: Vec<(Port, String)> = endpoints
                .iter()
                .filter(|endpoint| {
                    endpoint.sink
                        && endpoint.port != source
                        && !links
                            .iter()
                            .any(|(s, d)| *s == source && d == &endpoint.port)
                })
                .map(|endpoint| (endpoint.port.clone(), endpoint.label.clone()))
                .collect();
            let app_menu = app.clone();
            let source_menu = source.clone();
            section = section.child(
                Button::new(("graph-connect", row_ix))
                    .label("Connect")
                    .icon(Icon::new(Lucide::ChevronDown))
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
            sections.push(section.into_any_element());
        }
        if sections.is_empty() {
            sections.push(
                div()
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child("No MIDI outs in sight — plug something in or power a pane.")
                    .into_any_element(),
            );
        }

        // The graph's honest footnote: a running pane whose firmware carries a USB
        // MIDIStreaming interface would show up as a bidirectional USB-MIDI device on
        // real hardware; the emulated SoC can't enumerate USB, so only its TRS MIDI IN
        // jack exists here.
        let usb_note = self
            .slots
            .iter()
            .any(|slot| slot.handle.is_some() && slot.usb_midi);

        let app = cx.entity().clone();
        v_flex()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .p_2()
                    .pb_0()
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
                    }),
            )
            .child(
                div()
                    .id("graph-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar(&self.graph_scroll)
                    // PgUp/PgDn scroll by the viewport; record it.
                    .on_prepaint(move |bounds, _, cx| {
                        let height: f32 = bounds.size.height.into();
                        app.update(cx, |this, _| this.graph_view_h = height);
                    })
                    .child(
                        div()
                            .id("graph-sections")
                            .w_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.graph_scroll)
                            .child(
                            // Right padding clears the overlay scrollbar.
                            v_flex().w_full().p_2().pr(px(16.0)).gap_3().children(sections),
                        ),
                    ),
            )
    }
}
