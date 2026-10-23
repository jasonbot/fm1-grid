//! The transport bar at the bottom of the window: the miniature front panel and the
//! MIDI-clock metronome row.

use crate::app::GridApp;
use crate::clock::{MAX_BPM, MIN_BPM};
use crate::routing::{label_of, Port};
use crate::theme::{
    centered_cell, paint_led, tempo_stepper, toggle_switch, ACCENT, BORDER, CARD_BG, DIM, FAINT_BG,
    INK, LCD_BG, LCD_OFF, LCD_ON, PANEL_EDGE, RED,
};
use eframe::egui;
use egui::{
    Align, Color32, FontId, Layout, Margin, Pos2, RichText, Sense, Stroke, TextStyle, Vec2,
};

impl GridApp {
    /// The transport sits in a flat white bar at the bottom of the screen.
    pub fn transport(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("transport").show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(CARD_BG)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0_f32, PANEL_EDGE)),
            )
            .show(ctx, |ui| {
                self.control_display(ui);
                ui.add_space(6.0);
                let (separator, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 2.0),
                    Sense::hover(),
                );
                ui.painter().line_segment(
                    [
                        Pos2::new(separator.left(), separator.center().y),
                        Pos2::new(separator.right(), separator.center().y),
                    ],
                    Stroke::new(1.0_f32, BORDER),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(FAINT_BG)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            // A fixed-height row: every element sits in a cell that centers
                            // it against the full height, so nothing anchors to the top.
                            const ROW_H: f32 = 36.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(ui.available_width(), ROW_H),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    let mut on = self.clock.enabled();
                                    let switch =
                                        centered_cell(ui, Vec2::new(46.0, ROW_H), |ui| {
                                            toggle_switch(ui, &mut on)
                                        })
                                        .on_hover_text("Start or stop the MIDI clock");
                                    if switch.clicked() {
                                        self.clock.set_enabled(on);
                                    }

                                    ui.add_space(4.0);
                                    let running = self.clock.enabled();
                                    // Fixed width: RUNNING and STOPPED have the same number of
                                    // characters in a monospace font, so the tempo display
                                    // never shifts by a pixel when the clock starts.
                                    centered_cell(ui, Vec2::new(108.0, ROW_H), |ui| {
                                        ui.vertical(|ui| {
                                            ui.set_width(108.0);
                                            ui.label(
                                                RichText::new("MIDI CLOCK")
                                                    .strong()
                                                    .size(11.0)
                                                    .color(INK),
                                            );
                                            ui.label(
                                                RichText::new(if running {
                                                    "RUNNING · 24 PPQN"
                                                } else {
                                                    "STOPPED · 24 PPQN"
                                                })
                                                .monospace()
                                                .size(9.0)
                                                .color(if running { ACCENT } else { DIM }),
                                            );
                                        });
                                    });

                                    ui.add_space(14.0);
                                    centered_cell(ui, Vec2::new(86.0, ROW_H), |ui| {
                                        egui::Frame::new()
                                            .fill(LCD_BG)
                                            .stroke(Stroke::new(
                                                1.0_f32,
                                                Color32::from_rgb(28, 40, 31),
                                            ))
                                            .corner_radius(3)
                                            .inner_margin(Margin::symmetric(9, 2))
                                            .show(ui, |ui| {
                                                // The readout is also a DragValue: click and
                                                // type a tempo, or drag it. Its box is a fixed
                                                // size, so the running/stopped colour change
                                                // cannot move the row.
                                                {
                                                    let style = ui.style_mut();
                                                    style.drag_value_text_style =
                                                        TextStyle::Monospace;
                                                    style.text_styles.insert(
                                                        TextStyle::Monospace,
                                                        FontId::monospace(24.0),
                                                    );
                                                    // The DragValue's edit field is sized from
                                                    // interact_size; make it wide enough for 3
                                                    // digits.
                                                    style.spacing.interact_size =
                                                        Vec2::new(66.0, 30.0);
                                                    style.visuals.override_text_color = Some(
                                                        if running { LCD_ON } else { LCD_OFF },
                                                    );
                                                    style.visuals.widgets.inactive.bg_fill =
                                                        Color32::TRANSPARENT;
                                                    style.visuals.widgets.inactive
                                                        .weak_bg_fill = Color32::TRANSPARENT;
                                                    style.visuals.widgets.inactive.bg_stroke =
                                                        Stroke::NONE;
                                                    style.visuals.widgets.hovered.bg_fill =
                                                        Color32::from_rgb(32, 44, 34);
                                                    style.visuals.widgets.hovered.weak_bg_fill =
                                                        Color32::from_rgb(32, 44, 34);
                                                    style.visuals.widgets.hovered.bg_stroke =
                                                        Stroke::new(
                                                            1.0_f32,
                                                            Color32::from_rgb(60, 90, 60),
                                                        );
                                                    style.visuals.widgets.active.bg_fill =
                                                        Color32::from_rgb(40, 52, 42);
                                                    style.visuals.widgets.active.weak_bg_fill =
                                                        Color32::from_rgb(40, 52, 42);
                                                    style.visuals.widgets.active.bg_stroke =
                                                        Stroke::new(1.0_f32, LCD_ON);
                                                }

                                                let mut bpm = self.clock.bpm();
                                                let _ = ui.add_sized(
                                                    [66.0, 30.0],
                                                    egui::DragValue::new(&mut bpm)
                                                        .range(MIN_BPM..=MAX_BPM)
                                                        .speed(0.5)
                                                        .custom_formatter(|n, _| {
                                                            format!(
                                                                "{:03}",
                                                                n.round().clamp(0.0, 999.0)
                                                                    as u32
                                                            )
                                                        }),
                                                );
                                                if bpm != self.clock.bpm() {
                                                    self.clock.set_bpm(bpm);
                                                }
                                            });
                                    });
                                    centered_cell(ui, Vec2::new(24.0, ROW_H), |ui| {
                                        ui.label(RichText::new("BPM").size(9.0).color(DIM));
                                    });

                                    ui.add_space(2.0);
                                    let (down, up) =
                                        centered_cell(ui, Vec2::new(58.0, ROW_H), tempo_stepper);
                                    if down.clicked() {
                                        self.clock.adjust_bpm(-1);
                                    }
                                    if up.clicked() {
                                        self.clock.adjust_bpm(1);
                                    }

                                    ui.add_space(12.0);
                                    let lit = self.clock.beat_light();
                                    centered_cell(ui, Vec2::new(52.0, ROW_H), |ui| {
                                        ui.horizontal(|ui| {
                                            let (led, _) = ui.allocate_exact_size(
                                                Vec2::splat(18.0),
                                                Sense::hover(),
                                            );
                                            paint_led(ui.painter(), led, lit);
                                            ui.label(
                                                RichText::new("BEAT")
                                                    .size(9.0)
                                                    .color(if lit { ACCENT } else { DIM }),
                                            );
                                        });
                                    });

                                    // The physical MIDI keyboard whose notes play the
                                    // on-screen virtual keyboard; the port list comes
                                    // from the graph's poller.
                                    ui.add_space(16.0);
                                    let selected = self.midi_keys.selected();
                                    let keyboards: Vec<(String, String)> = self
                                        .routing
                                        .endpoints()
                                        .into_iter()
                                        .filter_map(|endpoint| match endpoint.port {
                                            Port::System(name) if endpoint.source => {
                                                Some((name, endpoint.label))
                                            }
                                            _ => None,
                                        })
                                        .collect();
                                    let mut pick: Option<Option<String>> = None;
                                    centered_cell(ui, Vec2::new(230.0, ROW_H), |ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("MIDI KEYS")
                                                    .strong()
                                                    .size(11.0)
                                                    .color(INK),
                                            );
                                            let current =
                                                selected.as_ref().map_or_else(
                                                    || "no keyboard".to_string(),
                                                    |name| {
                                                        keyboards
                                                            .iter()
                                                            .find(|(n, _)| n == name)
                                                            .map_or_else(
                                                                || label_of(name),
                                                                |(_, label)| label.clone(),
                                                            )
                                                    },
                                                );
                                            let combo =
                                                egui::ComboBox::from_id_salt("midi-keys-port")
                                                    .selected_text(
                                                        RichText::new(current).size(10.0),
                                                    )
                                                    .width(170.0)
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(
                                                            &mut pick,
                                                            Some(None),
                                                            "no keyboard",
                                                        );
                                                        for (name, label) in &keyboards {
                                                            ui.selectable_value(
                                                                &mut pick,
                                                                Some(Some(name.clone())),
                                                                label,
                                                            );
                                                        }
                                                    });
                                            // A focused menu would otherwise eat Space
                                            // (PLAY) on the active emulator.
                                            let response = combo.response;
                                            if response.clicked() {
                                                response.surrender_focus();
                                            }
                                            response
                                        })
                                        .inner
                                    })
                                    .on_hover_text(
                                        "A physical MIDI keyboard device whose notes play \
                                         the virtual keyboard (F3..G5)",
                                    );
                                    if let Some(choice) = pick {
                                        self.midi_keys.select(choice);
                                    }
                                },
                            );
                        });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(err) = &self.audio_error {
                            ui.label(
                                RichText::new(format!("audio off: {err}")).size(9.0).color(RED),
                            );
                        }
                        ui.label(
                            RichText::new(format!("{}–{} BPM · MIDI 0xF8", MIN_BPM, MAX_BPM))
                                .size(9.0)
                                .color(DIM),
                        );
                    });
                });
            });
    }
}
