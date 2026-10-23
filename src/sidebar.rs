//! The device sidebar: everything right of the grid, full height. From the top: the
//! heading, the selected screen live at 4x, the pane's attributes, the placeholder
//! for firmware-specific controls, then the MIDI graph ([`crate::graph`]).

use crate::app::{GridApp, Source};
use crate::builtin::BUILTINS;
use crate::lcd;
use crate::theme::{ambient_tint, paint_card, toggle_switch, DIM, INK, RED};
use eframe::egui;
use egui::{Align, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Sense, UiBuilder, Vec2};

enum Choice {
    Builtin(usize),
    Other,
}

impl GridApp {
    /// The device sidebar: greedy for all space right of the grid, full height. From the
    /// top: the heading, the selected screen live at 4x, the pane's attributes, the
    /// placeholder for firmware-specific controls (a registry of those will dispatch here
    /// once one exists), then the MIDI graph in a scroll region capped at 1.5x the grid's
    /// screen size. Its background is the ambient grid colour, slightly desaturated and
    /// strongly lightened so dark text and controls stay readable.
    pub fn device_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        ambient: Color32,
        grid_screen: f32,
    ) {
        let slot = self.active;
        paint_card(ui.painter(), rect, CornerRadius::same(10), ambient_tint(ambient));

        // Contents align from the top, never centered in the leftover height.
        let mut content = ui.new_child(
            UiBuilder::new()
                .id_salt("device-sidebar")
                .max_rect(rect.shrink2(Vec2::splat(16.0)))
                .layout(Layout::top_down(Align::Min)),
        );
        content.set_width(content.available_width());
        content.set_clip_rect(rect);

        let name = self.slots[slot]
            .label
            .clone()
            .unwrap_or_else(|| "No firmware".to_string());
        content.label(
            RichText::new(format!("Device {}: {}", slot + 1, name))
                .strong()
                .size(20.0)
                .color(INK),
        );
        content.add_space(12.0);

        // The selected screen at 4x (960 px), clamped to the sidebar's width and to
        // what its height leaves for the attributes, placeholder and MIDI graph below,
        // centered horizontally in the sidebar.
        let graph_max = 1.5 * grid_screen;
        let side = 960.0_f32
            .min(content.available_width() - 6.0)
            .min((content.available_height() - 140.0 - graph_max).max(80.0));
        let (bezel, _) = content.allocate_exact_size(Vec2::splat(side + 6.0), Sense::hover());
        let bezel = bezel.translate(Vec2::new(
            rect.left() + (rect.width() - bezel.width()) / 2.0 - bezel.left(),
            0.0,
        ));
        let texture = &self.textures[slot];
        lcd::view_at(&mut content, bezel, texture);
        content.add_space(12.0);

        self.pane_attributes(&mut content);
        content.add_space(12.0);

        let text = match &self.slots[slot].label {
            Some(label) => format!("No custom behavior for {label}"),
            None => format!("No firmware loaded in pane {}", slot + 1),
        };
        content.add(egui::Label::new(RichText::new(text).size(11.0).color(DIM)).wrap());

        // The MIDI graph, pinned to the bottom of the column: a top-down child anchored
        // so the section's bottom edge meets the sidebar's. Its height is what it
        // measured last frame (the first frame uses a rough estimate), so the block
        // hugs the content exactly.
        let graph_id = egui::Id::new("midi-graph-height");
        let max = content.max_rect();
        let spare = (max.bottom() - content.min_rect().bottom() - 12.0).max(60.0);
        let graph_h = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(graph_id))
            .unwrap_or(96.0)
            .min(spare);
        let graph_rect =
            Rect::from_min_max(Pos2::new(max.left(), max.bottom() - graph_h), max.right_bottom());
        let mut graph_ui = ui.new_child(
            UiBuilder::new()
                .id_salt("sidebar-graph")
                .max_rect(graph_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        graph_ui.set_width(graph_ui.available_width());
        graph_ui.set_clip_rect(rect);
        self.graph_section(&mut graph_ui, graph_max);
        ui.ctx()
            .data_mut(|data| data.insert_temp(graph_id, graph_ui.min_rect().height()));
    }

    /// The selected pane's attributes — power switch, firmware selector and status — at
    /// the top of the device sidebar. The cards are display-only, so this is where
    /// firmware switching lives.
    fn pane_attributes(&mut self, ui: &mut egui::Ui) {
        let slot = self.active;

        let mut choice = None;
        ui.horizontal(|ui| {
            let mut on = self.powered[slot];
            let response = toggle_switch(ui, &mut on).on_hover_text(if on {
                "Power this pane off"
            } else {
                "Power this pane on"
            });
            if response.changed() {
                self.set_power(slot, on);
            }

            let current = self.slots[slot]
                .label
                .clone()
                .unwrap_or_else(|| "Choose firmware…".to_string());
            let path_tip = match &self.slots[slot].source {
                Some(Source::File(path)) => Some(path.display().to_string()),
                _ => None,
            };
            let selected_builtin = match self.slots[slot].source {
                Some(Source::Builtin(index)) => Some(index),
                _ => None,
            };
            let selected_file = matches!(self.slots[slot].source, Some(Source::File(_)));

            let combo_width = (ui.available_width() - 8.0).clamp(120.0, 220.0);
            // Firmware can only be changed while the pane is powered off.
            let response = ui
                .add_enabled_ui(!self.powered[slot], |ui| {
                    let combo = egui::ComboBox::from_id_salt(("firmware", slot))
                        .selected_text(RichText::new(current).size(11.0))
                        .width(combo_width)
                        .show_ui(ui, |ui| {
                            for (index, firmware) in BUILTINS.iter().enumerate() {
                                let tip =
                                    format!("{}\nSHA-256 {}", firmware.file, firmware.sha256);
                                if ui
                                    .selectable_label(
                                        selected_builtin == Some(index),
                                        firmware.label(),
                                    )
                                    .on_hover_text(tip)
                                    .clicked()
                                {
                                    choice = Some(Choice::Builtin(index));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.selectable_label(selected_file, "Other…").clicked() {
                                choice = Some(Choice::Other);
                                ui.close();
                            }
                        });

                    // A focused menu would otherwise eat Space (PLAY) on the active
                    // emulator.
                    let response = match path_tip {
                        Some(path) => combo.response.on_hover_text(path),
                        None => combo.response,
                    };
                    if response.clicked() {
                        response.surrender_focus();
                    }
                    response
                })
                .inner;
            response.on_disabled_hover_text("Power the pane off to change its firmware");
        });

        let status = match (&self.slots[slot].error, &self.slots[slot].handle) {
            (Some(error), _) => error.clone(),
            (None, Some(handle)) => handle.status(),
            (None, None) => "Off".to_string(),
        };
        let status_color = if self.slots[slot].error.is_some()
            || self
                .slots[slot]
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_running())
        {
            RED
        } else {
            DIM
        };
        ui.label(RichText::new(status).size(10.0).color(status_color));

        match choice {
            Some(Choice::Builtin(index)) => self.load_builtin(slot, index),
            Some(Choice::Other) => self.pick_file(slot),
            None => {}
        }
    }
}
