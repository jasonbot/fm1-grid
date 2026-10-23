//! The devices pane under the grid: one row per live FM-1 on USB, with its firmware,
//! its MIDI-clock switch and its Flash button. All device state lives in
//! [`crate::hardware`]; this only draws it and posts commands. The MIDI graph editor
//! lives in [`crate::graph`], at the bottom of the device sidebar.

use crate::app::GridApp;
use crate::builtin::BUILTINS;
use crate::hardware::DeviceRow;
use crate::theme::{ambient_tint, paint_card, toggle_switch, ACCENT, DIM, INK, RED};
use eframe::egui;
use egui::{Align, Color32, CornerRadius, Layout, Rect, RichText, UiBuilder, Vec2};

/// What the row's flash selector picked.
enum FlashChoice {
    Builtin(usize),
    File,
}

impl GridApp {
    /// The devices pane, in the rect the grid gives it (as wide as the grid, under it).
    /// Same ambient-tinted background as the sidebar.
    pub(crate) fn devices_pane(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        rows: &[DeviceRow],
        ambient: Color32,
    ) {
        // No clip on the shared ui here: it would persist and clip away everything
        // painted after the pane (the sidebar). The child below clips itself.
        paint_card(ui.painter(), rect, CornerRadius::same(10), ambient_tint(ambient));

        let mut content = ui.new_child(
            UiBuilder::new()
                .id_salt("devices-pane")
                .max_rect(rect.shrink2(Vec2::splat(10.0)))
                .layout(Layout::top_down(Align::Min)),
        );
        content.set_width(content.available_width());
        content.set_clip_rect(rect);

        content.horizontal(|ui| {
            ui.label(RichText::new("USB DEVICES").strong().size(10.5).color(DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(error) = &self.hardware.error {
                    ui.label(RichText::new(error).size(9.5).color(RED));
                } else {
                    ui.label(
                        RichText::new("POLLS THE MIDI PORTS EVERY ½ S")
                            .size(9.0)
                            .color(DIM),
                    );
                }
            });
        });
        content.add_space(2.0);

        if rows.is_empty() {
            content.label(
                RichText::new("No FM-1 connected — plug one in and it shows up here.")
                    .size(11.0)
                    .color(DIM),
            );
        }

        for row in rows {
            content.horizontal(|ui| {
                let mut on = row.clock;
                let switch = toggle_switch(ui, &mut on)
                    .on_hover_text("Send the metronome's MIDI clock to this device");
                if switch.changed() {
                    self.hardware.set_clock(&row.name, on);
                }
                ui.label(RichText::new(&row.port).strong().size(12.0).color(INK))
                    .on_hover_text(&row.name);
                if let Some(number) = row.number {
                    ui.label(RichText::new(format!("client {number}")).size(9.5).color(DIM))
                        .on_hover_text("The ALSA client number of the device");
                }
                if let Some(usb) = &row.usb {
                    ui.label(RichText::new(format!("USB {usb}")).size(9.5).color(DIM))
                        .on_hover_text("The USB bus-port path");
                }
                ui.label(RichText::new(&row.display).size(11.5).color(INK));

                let picked = ui
                    .with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if row.flash.is_some() {
                            return None; // flashing: the flash board below shows what it does
                        }
                        if self.flash_pick.as_ref() != Some(&row.name) {
                            if ui
                                .button("Flash")
                                .on_hover_text("Flash a firmware to this device")
                                .clicked()
                            {
                                self.flash_pick = Some(row.name.clone());
                            }
                            return None;
                        }
                        // The selector: the button gave way to the firmware dropdown
                        // and a little dismiss button (right-to-left: the × ends up
                        // right of the dropdown, next to it).
                        if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                            self.flash_pick = None;
                        }
                        let mut choice = None;
                        egui::ComboBox::from_id_salt(("flash", row.name.clone()))
                            .selected_text(RichText::new("flash firmware…").size(11.0))
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (index, firmware) in BUILTINS.iter().enumerate() {
                                    let tip =
                                        format!("{}\nSHA-256 {}", firmware.file, firmware.sha256);
                                    if ui
                                        .selectable_label(false, firmware.label())
                                        .on_hover_text(tip)
                                        .clicked()
                                    {
                                        choice = Some(FlashChoice::Builtin(index));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.selectable_label(false, "Other…").clicked() {
                                    choice = Some(FlashChoice::File);
                                    ui.close();
                                }
                            });
                        choice
                    })
                    .inner;
                match picked {
                    Some(FlashChoice::Builtin(index)) => {
                        self.flash_pick = None;
                        self.hardware.flash(&row.name, BUILTINS[index].bytes.to_vec());
                    }
                    Some(FlashChoice::File) => {
                        self.flash_pick = None;
                        self.flash_device(row);
                    }
                    None => {}
                }
            });
            if let Some(outcome) = &row.outcome {
                let color = if outcome.starts_with("flashed:") {
                    INK
                } else if outcome.starts_with("step 1 done") {
                    ACCENT
                } else {
                    RED
                };
                content.horizontal(|ui| {
                    ui.add_space(52.0);
                    ui.label(RichText::new(outcome).size(10.0).color(color));
                });
            }
        }

        // The flash board: the state of the running flash even while the device's row
        // is gone (it reboots through the loader), or the last failure.
        if let Some(board) = self.hardware.flash_state() {
            content.horizontal(|ui| {
                let color = if board.error { RED } else { ACCENT };
                ui.label(RichText::new(&board.step).size(10.5).color(color));
                if !board.error {
                    if let Some(progress) = board.progress {
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(180.0)
                                .desired_height(12.0)
                                .text(format!("{:.0}%", progress * 100.0)),
                        );
                    }
                }
            });
        }
    }

    /// Pick a package and hand it to the hardware module; the flash itself runs on its
    /// own thread and reports through the device's row.
    fn flash_device(&mut self, row: &DeviceRow) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose an FM-1 firmware package")
            .add_filter("FM-1 package", &["fwsc"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => self.hardware.flash(&row.name, bytes),
            Err(e) => self.hardware.report(&row.name, format!("{}: {e}", path.display())),
        }
    }
}
