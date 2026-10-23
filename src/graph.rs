//! The MIDI graph editor: one row per connected source ("out → in in in"), edited with
//! chips and dropdowns. System ports, running panes and the live FM-1 units are all
//! endpoints; the live units piggyback on the hardware module's open ports. All graph
//! state lives in [`crate::routing`]; this only draws it and posts commands. It sits at
//! the bottom of the device sidebar, under everything else there.

use crate::app::GridApp;
use crate::routing::Port;
use crate::theme::{DIM, INK};
use eframe::egui;
use egui::{Button, CornerRadius, RichText};

impl GridApp {
    /// The MIDI GRAPH section, top-down: the header (title and subtitle stacked — the
    /// sidebar can be too narrow for both on one line), then the rows in a scroll
    /// region capped at `max_h` (the sidebar passes 1.5x the grid's screen size) that
    /// scrolls when it grows past that. The sidebar anchors the whole section to the
    /// bottom of its column using the height measured here last frame.
    pub(crate) fn graph_section(&mut self, ui: &mut egui::Ui, max_h: f32) {
        ui.label(RichText::new("MIDI GRAPH").strong().size(10.5).color(DIM));
        ui.label(RichText::new("ONE OUT FEEDS ANY NUMBER OF INS").size(9.0).color(DIM));
        ui.add_space(2.0);

        let endpoints = self.routing.endpoints();
        let links = self.routing.links();
        let label = |port: &Port| -> String {
            endpoints
                .iter()
                .find(|endpoint| &endpoint.port == port)
                .map_or_else(|| "gone".to_string(), |endpoint| endpoint.label.clone())
        };

        let height = max_h.min(ui.available_height().max(60.0));
        egui::ScrollArea::vertical()
            .id_salt("midi-graph-scroll")
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let mut sources: Vec<Port> = Vec::new();
                for (source, _) in &links {
                    if !sources.contains(source) {
                        sources.push(source.clone());
                    }
                }
                for source in &sources {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label(source)).size(11.0).color(INK));
                        ui.label(RichText::new("→").size(11.0).color(DIM));
                        for (src, sink) in links.iter().filter(|(s, _)| s == source) {
                            let chip = ui.add(
                                Button::new(RichText::new(format!("{} ×", label(sink))).size(10.0))
                                    .corner_radius(CornerRadius::same(6)),
                            );
                            if chip.on_hover_text("Remove this connection").clicked() {
                                self.routing.disconnect(src.clone(), sink.clone());
                            }
                        }
                        // The row's "+": every sink this source is not already connected to.
                        let mut chosen: Option<Port> = None;
                        egui::ComboBox::from_id_salt(ui.id().with(("add-sink", source.clone())))
                            .selected_text("+")
                            .show_ui(ui, |ui| {
                                let mut any = false;
                                for endpoint in endpoints.iter().filter(|endpoint| {
                                    endpoint.sink
                                        && &endpoint.port != source
                                        && !links
                                            .iter()
                                            .any(|(s, d)| s == source && d == &endpoint.port)
                                }) {
                                    any = true;
                                    ui.selectable_value(
                                        &mut chosen,
                                        Some(endpoint.port.clone()),
                                        &endpoint.label,
                                    );
                                }
                                if !any {
                                    ui.label(RichText::new("no other sinks").weak());
                                }
                            });
                        if let Some(sink) = chosen {
                            self.routing.connect(source.clone(), sink);
                        }
                    });
                }

                // The add row: pick a source, then its first sink.
                ui.horizontal(|ui| {
                    let mut add: Option<Port> = None;
                    egui::ComboBox::from_id_salt("graph-add-source")
                        .selected_text(if self.pending_source.is_some() {
                            "connecting…"
                        } else {
                            "+ connect a MIDI out…"
                        })
                        .show_ui(ui, |ui| {
                            let mut any = false;
                            for endpoint in endpoints.iter().filter(|endpoint| endpoint.source) {
                                any = true;
                                ui.selectable_value(
                                    &mut add,
                                    Some(endpoint.port.clone()),
                                    &endpoint.label,
                                );
                            }
                            if !any {
                                ui.label(RichText::new("no MIDI outs in sight").weak());
                            }
                        });
                    if let Some(source) = add {
                        self.pending_source = Some(source);
                    }
                    if let Some(pending) = self.pending_source.clone() {
                        ui.label(RichText::new(label(&pending)).size(11.0).color(INK));
                        ui.label(RichText::new("→").size(11.0).color(DIM));
                        let mut chosen: Option<Port> = None;
                        egui::ComboBox::from_id_salt("graph-pending-sink")
                            .selected_text("to…")
                            .show_ui(ui, |ui| {
                                for endpoint in endpoints
                                    .iter()
                                    .filter(|endpoint| endpoint.sink && endpoint.port != pending)
                                {
                                    ui.selectable_value(
                                        &mut chosen,
                                        Some(endpoint.port.clone()),
                                        &endpoint.label,
                                    );
                                }
                            });
                        if let Some(sink) = chosen {
                            self.routing.connect(pending, sink);
                            self.pending_source = None;
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Cancel this connection")
                            .clicked()
                        {
                            self.pending_source = None;
                        }
                    }
                });
            });
    }
}
