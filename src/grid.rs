//! The central area: the 3×3 grid of display-only pane cards on the left with the
//! devices pane for the live USB units under it, the ambient background colour and the
//! 3 Hz snapshot tick, and the hand-off to the device sidebar that takes all remaining
//! space on the right.

use crate::app::GridApp;
use crate::lcd;
use crate::theme::{ambient_tint, halo_color, paint_card};
use eframe::egui;
use egui::{
    Align, Color32, CornerRadius, Layout, Margin, Pos2, Rect, Sense, Stroke, StrokeKind, UiBuilder,
    Vec2,
};

/// Minimum width the grid sizing reserves for the device sidebar on the right; the
/// sidebar itself is greedy and takes everything the grid does not use.
const SIDEBAR_WIDTH: f32 = 240.0;

impl GridApp {
    pub fn grid(&mut self, ctx: &egui::Context) {
        // The grid background takes the dominant colour of the selected pane's screen,
        // recalculated once a second (and right after a pane switch) and eased into over
        // 500 ms whenever it changes. The selected pane's grid-card snapshot (its screen
        // NTSC-distorted and slightly bulged) refreshes every frame, in realtime.
        let now = ctx.input(|i| i.time);
        let switched = self.dominant_slot != self.active;
        self.refresh_ghost();
        if now - self.dominant_at >= 1.0 || switched {
            self.dominant_at = now;
            self.dominant_slot = self.active;
            self.dominant = self.dominant_lcd_color();
        }
        let bg_channel = |salt: &str, target: u8| {
            ctx.animate_value_with_time(egui::Id::new(("grid-bg", salt)), target as f32, 0.5)
                as u8
        };
        let bg = Color32::from_rgb(
            bg_channel("r", self.dominant.r()),
            bg_channel("g", self.dominant.g()),
            bg_channel("b", self.dominant.b()),
        );

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(Margin::same(8)))
            .show(ctx, |ui| {
                let gap = ui.spacing().item_spacing;
                let avail_w = ui.available_width();
                let avail_h = ui.available_height();

                // Nine display-only monitors at 1x in a 3x3 grid on the left, with
                // tight fixed gaps so the sidebar gets as much room as possible in
                // normal use; the device sidebar greedily takes all space to its
                // right. The sizing reserves at least SIDEBAR_WIDTH for the sidebar.
                // The selected pane's live screen shows in the sidebar; its grid card
                // shows the NTSC-distorted realtime snapshot. Under the grid
                // sits the devices pane for the live USB units, as wide as the grid, so
                // the sidebar keeps its clearance on the right.
                const GRID_GAP: f32 = 3.0;
                let device_rows = self.hardware.devices();
                let devices_h = 50.0
                    + device_rows
                        .iter()
                        .map(|row| 31.0 + if row.outcome.is_some() { 19.0 } else { 0.0 })
                        .sum::<f32>()
                        .max(36.0)
                    + 24.0
                    + if self.hardware.flash_state().is_some() { 22.0 } else { 0.0 };
                let s = ((avail_w - SIDEBAR_WIDTH - 2.0 * gap.x - 2.0 * GRID_GAP) / 3.0 - 22.0)
                    .min(
                        (avail_h - devices_h - 2.0 * gap.y - 2.0 * GRID_GAP) / 3.0 - 20.0,
                    )
                    .min(240.0)
                    .max(40.0);
                let width = s + 22.0;
                let card_height = s + 20.0;
                let grid_w = width * 3.0 + GRID_GAP * 2.0;
                let grid_h = card_height * 3.0 + GRID_GAP * 2.0;
                let column_h = grid_h + gap.y * 2.0 + devices_h;
                let grid_left = ui.max_rect().left();
                let grid_top = ui.max_rect().top() + ((avail_h - column_h) / 2.0).max(0.0);

                // Cards are laid out with explicit rects: add_space around a horizontal
                // row does not advance the cursor reliably (rows end up overlapping).
                // The cards take the same ambient tint as the panes around them; the
                // selection halo is that colour's hue rotated towards blue.
                let card_bg = ambient_tint(bg);
                let halo = halo_color(bg);
                for row in 0..3 {
                    for col in 0..3 {
                        let slot = row * 3 + col;
                        let cell = Rect::from_min_size(
                            Pos2::new(
                                grid_left + col as f32 * (width + GRID_GAP),
                                grid_top + row as f32 * (card_height + GRID_GAP),
                            ),
                            Vec2::new(width, card_height),
                        );
                        // Explicit rect + painter clip: the card never paints outside its
                        // cell even if the window is too small for the whole grid.
                        ui.scope_builder(
                            UiBuilder::new()
                                .id_salt(("slot-cell", slot))
                                .max_rect(cell)
                                .layout(Layout::top_down(Align::Center)),
                            |ui| {
                                ui.set_clip_rect(cell);
                                ui.set_width(width);
                                self.slot_ui(ui, slot, card_bg, halo);
                            },
                        );
                    }
                }

                // The devices pane: as wide as the grid, pinned to the bottom of the
                // central area so its bottom line shares the sidebar's clearance; the
                // contents are top-aligned inside it.
                let devices_rect = Rect::from_min_size(
                    Pos2::new(grid_left, ui.max_rect().bottom() - devices_h),
                    Vec2::new(grid_w, devices_h),
                );
                self.devices_pane(ui, devices_rect, &device_rows, bg);

                // The device sidebar: everything right of the grid, full height.
                let sidebar_rect = Rect::from_min_max(
                    Pos2::new(grid_left + grid_w + gap.x * 2.0, ui.max_rect().top()),
                    ui.max_rect().right_bottom(),
                );
                if sidebar_rect.width() >= 80.0 {
                    // The sidebar's MIDI graph scroll region is capped at 1.5x the
                    // grid's screen size.
                    self.device_sidebar(ui, sidebar_rect, bg, s);
                }

                // Reserve the full area so the panel keeps its size.
                ui.allocate_rect(
                    Rect::from_min_size(ui.max_rect().left_top(), Vec2::new(avail_w, avail_h)),
                    Sense::hover(),
                );
            });
    }

    fn slot_ui(&mut self, ui: &mut egui::Ui, slot: usize, card_bg: Color32, halo: Color32) {
        let card_rect = ui.max_rect().shrink2(Vec2::splat(2.0));

        // Register the card before its children so buttons and the dropdown stay on top;
        // the pointer test also catches clicks on child widgets, i.e. anywhere in the box.
        let card = ui.interact(
            card_rect,
            ui.id().with(("slot-card", slot)),
            Sense::click(),
        );
        let clicked_inside = ui.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|pos| card_rect.contains(pos))
        });
        // Do not steal a click from an open menu that happens to overlap this card.
        let on_card_layer = ui
            .input(|i| i.pointer.interact_pos())
            .and_then(|pos| ui.ctx().layer_id_at(pos))
            .is_none_or(|layer| layer == ui.layer_id());
        if card.clicked() || (clicked_inside && on_card_layer) {
            self.set_active(slot);
        }

        let active = self.active == slot;
        let painter = ui.painter();
        paint_card(painter, card_rect, CornerRadius::same(10), card_bg);

        // A constant-width outline plus an inset selection ring: selecting never changes
        // the card's padding or size, so nothing jumps.
        if active {
            painter.rect_stroke(
                card_rect.shrink(1.5),
                CornerRadius::same(9),
                Stroke::new(2.0_f32, halo),
                StrokeKind::Inside,
            );
        }

        let mut content = ui.new_child(
            UiBuilder::new()
                .id_salt(("slot-content", slot))
                .max_rect(card_rect.shrink2(Vec2::new(6.0, 5.0)))
                .layout(Layout::top_down(Align::Center)),
        );
        content.set_width(content.available_width());

        // The bezel (lcd_side + 6) always fits the content rect exactly.
        let lcd_side = (content.available_width() - 6.0).min(240.0);
        let (bezel, _) =
            content.allocate_exact_size(Vec2::splat(lcd_side + 6.0), Sense::hover());

        // The selected pane shows the NTSC-distorted realtime snapshot on the
        // grid; its live screen is the 4x view in the sidebar. Every other pane shows
        // its live screen.
        let texture = if active {
            &self.ghost
        } else {
            &self.textures[slot]
        };
        lcd::view_at(&mut content, bezel, texture);
    }
}
