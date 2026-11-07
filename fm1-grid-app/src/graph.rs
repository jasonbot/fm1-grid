//! The MIDI graph editor: one always-on section per source endpoint ("INPUT" rows —
//! system MIDI outs, live FM-1 units), listing the sinks it feeds with an (X) pill
//! to unroute each, and an always-there Connect dropdown to route another. Running
//! panes and system ports are sinks; the live units piggyback on the hardware
//! module's open ports. All graph state lives in [`fm1_performer::routing`]; this
//! only draws it and posts commands.

use crate::app::GridApp;
use fm1_performer::hardware::device_type;
use fm1_performer::routing::Port;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};
use std::collections::HashMap;
use std::time::Instant;

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

        let kinds: HashMap<String, &'static str> = self
            .hardware
            .devices()
            .iter()
            .filter_map(|row| {
                row.identity.as_deref().and_then(device_type).map(|kind| (row.name.clone(), kind))
            })
            .collect();
        let app = cx.entity().clone();
        let mut sections: Vec<AnyElement> = Vec::new();
        for (row_ix, source) in endpoints
            .iter()
            .filter(|endpoint| endpoint.source)
            .enumerate()
        {
            let source = source.port.clone();
            let mut heading = h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .id(("graph-source", row_ix))
                        .test_support()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(ink)
                        .child(label(&source)),
                );
            let kind = match &source {
                Port::Device(name) => kinds.get(name).copied(),
                _ => None,
            };
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
            let mut trailing = h_flex().items_center().gap_2();
            if let Some(kind) = kind {
                trailing = trailing.child(crate::app::type_pill(kind));
            }
            trailing = trailing.child(
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
            heading = heading.child(trailing);
            let mut section = v_flex()
                .w_full()
                .gap_1()
                .child(heading);
            // The sinks this source feeds, each with its (X) unroute pill. A row
            // that just appeared snaps in on the library's spring; rows that just
            // vanished linger as ghosts and fade out.
            for (sink_ix, (_, sink)) in
                links.iter().filter(|(s, _)| *s == source).enumerate()
            {
                let sink = sink.clone();
                let sink_label = label(&sink);
                let (nudge, fade) =
                    match self.graph_added.get(&(source.clone(), sink.clone())) {
                        Some(at) => {
                            let t = at.elapsed().as_secs_f32();
                            ((1.0 - crate::library_tab::snap_in(t)) * 6.0, (t / 0.12).min(1.0))
                        }
                        None => (0.0, 1.0),
                    };
                section = section.child(self.graph_sink_row(
                    source.clone(),
                    sink,
                    sink_label,
                    row_ix,
                    sink_ix,
                    nudge,
                    fade,
                    cx,
                ));
            }
            let mut ghosts: Vec<(Port, Instant)> = self
                .graph_removed
                .iter()
                .filter(|((src, _), _)| *src == source)
                .map(|((_, sink), at)| (sink.clone(), *at))
                .collect();
            ghosts.sort_by_key(|(_, at)| *at);
            for (ghost_ix, (sink, at)) in ghosts.into_iter().enumerate() {
                let t = at.elapsed().as_secs_f32();
                let fade = (1.0 - t / crate::app::GRAPH_ANIM.as_secs_f32()).max(0.0);
                let sink_label = label(&sink);
                section = section.child(self.graph_sink_row(
                    source.clone(),
                    sink,
                    sink_label,
                    row_ix,
                    4096 + ghost_ix,
                    0.0,
                    fade,
                    cx,
                ));
            }

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

    /// One connection row: the unroute pill, then the sink's label. `nudge` lifts
    /// the row on the snap-in spring; `fade` is its opacity (a removed row ghosts
    /// out before the tick drops it).
    #[allow(clippy::too_many_arguments)]
    fn graph_sink_row(
        &mut self,
        source: Port,
        sink: Port,
        sink_label: String,
        row_ix: usize,
        id: usize,
        nudge: f32,
        fade: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (ink, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.border)
        };
        // A pane whose firmware advertises USB-MIDI has MIDI out on real hardware;
        // in the emulator only its MIDI IN jack exists.
        let tip = match &sink {
            Port::Pane(slot) if self.slots[*slot].usb_midi => {
                format!(
                    "Remove this connection. {} advertises USB-MIDI in/out on real \
                     hardware; the emulator bridges its MIDI IN only, so its MIDI \
                     out stays silent.",
                    sink_label
                )
            }
            _ => "Remove this connection".to_string(),
        };
        let src = source;
        // The unroute X sits first, beside the sink it removes; the rule above each
        // row separates connections and gives them height.
        h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .px_2()
            .py_2()
            .border_t_1()
            .border_color(border)
            .pt(px(8.0 + nudge))
            .opacity(fade)
            .child(
                Button::new(("sink-x", row_ix * 1024 + id))
                    .icon(Icon::new(Lucide::X))
                    .accessibility_label("unroute")
                    .tooltip(tip)
                    .small()
                    .rounded(px(999.0))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.routing.disconnect(src.clone(), sink.clone());
                        this.refocus(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(ink)
                    .child(sink_label),
            )
            .into_any_element()
    }
}
