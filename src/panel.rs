//! The miniature FM-1 front panel in the transport bar: a working control surface whose
//! keys, buttons and knob ends are clickable, and whose LEDs follow both the computer
//! keyboard and the emulated firmware's own LED state.

use crate::app::GridApp;
use crate::keys::{
    panel_key_hint, BLACK_KEYS, FUNCTION_ROW_1, FUNCTION_ROW_2, PANEL_KEYS, WHITE_KEYS,
};
use crate::theme::{
    mix_color, paint_inset, AMBER, GREEN, KEY_BLACK, KEY_WHITE, PANEL_BODY, PANEL_BUTTON,
    PANEL_BUTTON_EDGE, SILK, SILK_DIM,
};
use eframe::egui;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

const KEY_HEIGHT: f32 = 120.0;
const KEY_MARGIN: f32 = 6.0;
const PANEL_HEIGHT: f32 = 196.0 + (KEY_HEIGHT - 40.0);

impl GridApp {
    /// A miniature FM-1 front panel. It is a working control surface: note keys and
    /// buttons can be clicked, the knobs are turned with the -/+ ends, and both the
    /// computer keyboard and the emulated firmware's LEDs light the indicators.
    pub fn control_display(&mut self, ui: &mut egui::Ui) {
        // Keep the panel roughly as wide as the card grid.
        let panel_max_width = 1040.0;
        let available = ui.available_width();
        let width = available.min(panel_max_width);
        let (row, _) =
            ui.allocate_exact_size(Vec2::new(available, PANEL_HEIGHT), Sense::hover());
        let panel = Rect::from_center_size(row.center(), Vec2::new(width, PANEL_HEIGHT));
        let painter = ui.painter().clone();
        paint_inset(&painter, panel, CornerRadius::same(8), PANEL_BODY);

        let left = panel.left() + 14.0;
        let right = panel.right() - 14.0;
        let controls_top = panel.top() + 24.0;
        let group_width = 120.0;
        let group_height = 24.0;
        let group_gap = 6.0;
        let row_gap = 4.0;

        painter.text(
            Pos2::new(left, panel.top() + 7.0),
            Align2::LEFT_TOP,
            "FM-1 PANEL",
            FontId::proportional(9.5),
            SILK_DIM,
        );
        painter.text(
            Pos2::new(right, panel.top() + 7.0),
            Align2::RIGHT_TOP,
            format!("CLICKABLE · KEYBOARD LIGHTS · PANE {}", self.active + 1),
            FontId::proportional(9.5),
            SILK_DIM,
        );

        // The right-hand groups (the four knobs and the function buttons) span one
        // common width, so the knobs sit directly above the button grid.
        let function_width = (panel.width() * 0.52).clamp(360.0, 520.0);
        let function_left = right - function_width;
        let button_gap = 4.0;
        let button_width = (function_width - button_gap * 5.0) / 6.0;
        let knob_gap = 6.0;
        let knob_width = (function_width - knob_gap * 3.0) / 4.0;

        // Top left: MASTER / SELECT over PRESETS / ALGORITHM. The labels are inert; the
        // tiny - and + ends turn the control one detent.
        let top_left = [
            ("MASTER", 0u32, None),
            ("SELECT", 1, Some(0u8)),
            ("PRESETS", 2, Some(6)),
            ("ALGORITHM", 3, Some(1)),
        ];
        for (index, (label, salt, encoder)) in top_left.iter().enumerate() {
            let column = index % 2;
            let row_index = index / 2;
            let rect = Rect::from_min_size(
                Pos2::new(
                    left + column as f32 * (group_width + group_gap),
                    controls_top + row_index as f32 * (group_height + row_gap),
                ),
                Vec2::new(group_width, group_height),
            );
            let (down, up) = panel_turn_group(ui, rect, *salt, label);
            let is_master = encoder.is_none();
            let down = down.on_hover_text(if is_master {
                "Lower master"
            } else {
                "Turn counter-clockwise"
            });
            let up = up.on_hover_text(if is_master {
                "Raise master"
            } else {
                "Turn clockwise"
            });
            if down.clicked() {
                if let Some(encoder) = encoder {
                    self.turn_encoder(*encoder, -1);
                } else {
                    self.adjust_master(-1);
                }
            }
            if up.clicked() {
                if let Some(encoder) = encoder {
                    self.turn_encoder(*encoder, 1);
                } else {
                    self.adjust_master(1);
                }
            }
        }

        // Top right: KNOB 1..4.
        for index in 0..4 {
            let rect = Rect::from_min_size(
                Pos2::new(
                    function_left + index as f32 * (knob_width + knob_gap),
                    controls_top,
                ),
                Vec2::new(knob_width, group_height),
            );
            let (down, up) =
                panel_turn_group(ui, rect, 10 + index as u32, &format!("KNOB {}", index + 1));
            if down.on_hover_text("Turn counter-clockwise").clicked() {
                self.turn_encoder(2 + index as u8, -1);
            }
            if up.on_hover_text("Turn clockwise").clicked() {
                self.turn_encoder(2 + index as u8, 1);
            }
        }

        let second_band = controls_top + group_height * 2.0 + row_gap + 8.0;

        // OCT- and OCT+ sit under the two encoder columns.
        for (index, (label, id, hint)) in [("OCT−", 0u8, "Z"), ("OCT+", 1, "X")]
            .iter()
            .enumerate()
        {
            let rect = Rect::from_min_size(
                Pos2::new(left + index as f32 * (group_width + group_gap), second_band),
                Vec2::new(group_width, group_height),
            );
            let (response, down) = panel_button_ui(
                ui,
                rect,
                *id,
                label,
                hint,
                self.held[*id as usize],
                self.leds[*id as usize] > 0.04,
                AMBER,
            );
            self.panel_down[*id as usize] = down;
            response.on_hover_text(format!("{label} ({hint})"));
        }

        // The two rows of six function buttons, with their computer-key hints and
        // indicators. PLAY uses the same green as the machine's play/pause LED.
        for (row_index, row) in [FUNCTION_ROW_1, FUNCTION_ROW_2].iter().enumerate() {
            for (column, (label, id, hint)) in row.iter().enumerate() {
                let rect = Rect::from_min_size(
                    Pos2::new(
                        function_left + column as f32 * (button_width + button_gap),
                        second_band + row_index as f32 * (group_height + row_gap),
                    ),
                    Vec2::new(button_width, group_height),
                );
                let tint = if *id == 12 { GREEN } else { AMBER };
                let (response, down) = panel_button_ui(
                    ui,
                    rect,
                    *id,
                    label,
                    hint,
                    self.held[*id as usize],
                    self.leds[*id as usize] > 0.04,
                    tint,
                );
                self.panel_down[*id as usize] = down;
                if *id == 12 {
                    response.on_hover_text("Play/Pause (Space)");
                } else {
                    response.on_hover_text(format!("{label} ({hint})"));
                }
            }
        }

        // The 27 pill keys: white keys first, then the black keys over them.
        let key_left = left;
        let key_right = right;
        let key_bottom = panel.bottom() - KEY_MARGIN;
        let key_top = key_bottom - KEY_HEIGHT;
        let white_width = (key_right - key_left) / WHITE_KEYS.len() as f32;
        for (index, id) in WHITE_KEYS.iter().enumerate() {
            let white_left = key_left + index as f32 * white_width;
            let rect = Rect::from_min_max(
                Pos2::new(white_left + 0.5, key_top),
                Pos2::new(white_left + white_width - 0.5, key_bottom),
            );
            let note = PANEL_KEYS
                .iter()
                .find(|(key, _, _)| key == id)
                .map(|(_, note, _)| *note)
                .unwrap_or("");
            let (response, down) = panel_key_ui(
                ui,
                rect,
                *id,
                true,
                note,
                panel_key_hint(*id),
                self.held[*id as usize],
                self.leds[*id as usize] > 0.04,
            );
            self.panel_down[*id as usize] = down;
            let _ = response;
        }
        let black_height = (key_bottom - key_top) * 0.62;
        for (id, after) in BLACK_KEYS {
            let center_x = key_left + (after as f32 + 1.0) * white_width;
            let rect = Rect::from_center_size(
                Pos2::new(center_x, key_top + black_height / 2.0),
                Vec2::new(white_width * 0.62, black_height),
            );
            let (response, down) = panel_key_ui(
                ui,
                rect,
                id,
                false,
                "",
                panel_key_hint(id),
                self.held[id as usize],
                self.leds[id as usize] > 0.04,
            );
            self.panel_down[id as usize] = down;
            let _ = response;
        }

        // Mouse changes were collected above; deliver them on this same frame.
        self.sync_held(ui.ctx());
    }
}

/// A three-part rotary group: an inert label in the middle and clickable - / + ends.
fn panel_turn_group(
    ui: &mut egui::Ui,
    rect: Rect,
    salt: u32,
    label: &str,
) -> (egui::Response, egui::Response) {
    let arrow = 22.0;
    let down_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + arrow, rect.bottom()));
    let up_rect = Rect::from_min_max(Pos2::new(rect.right() - arrow, rect.top()), rect.max);
    let down = ui.interact(
        down_rect,
        ui.id().with(("panel-turn-down", salt)),
        Sense::click(),
    );
    let up = ui.interact(
        up_rect,
        ui.id().with(("panel-turn-up", salt)),
        Sense::click(),
    );

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), PANEL_BUTTON);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        StrokeKind::Inside,
    );

    let left_corners = CornerRadius { nw: 3, sw: 3, ne: 0, se: 0 };
    let right_corners = CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 };
    for (response, half, corners) in [
        (&down, down_rect, left_corners),
        (&up, up_rect, right_corners),
    ] {
        if response.is_pointer_button_down_on() {
            painter.rect_filled(
                half.shrink(1.0),
                corners,
                mix_color(PANEL_BUTTON, AMBER, 0.3),
            );
        } else if response.hovered() {
            painter.rect_filled(half.shrink(1.0), corners, Color32::from_rgb(78, 81, 88));
        }
    }

    for x in [rect.left() + arrow, rect.right() - arrow] {
        painter.line_segment(
            [
                Pos2::new(x, rect.top() + 3.0),
                Pos2::new(x, rect.bottom() - 3.0),
            ],
            Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        );
    }

    painter.text(
        down_rect.center(),
        Align2::CENTER_CENTER,
        "−",
        FontId::proportional(15.0),
        SILK,
    );
    painter.text(
        up_rect.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(15.0),
        SILK,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        SILK,
    );
    (
        down.on_hover_cursor(egui::CursorIcon::PointingHand),
        up.on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

/// A clickable front-panel button. Returns its response and whether the mouse is holding it.
#[allow(clippy::too_many_arguments)]
fn panel_button_ui(
    ui: &mut egui::Ui,
    rect: Rect,
    id: u8,
    label: &str,
    hint: &str,
    held: bool,
    led: bool,
    tint: Color32,
) -> (egui::Response, bool) {
    let response = ui.interact(rect, ui.id().with(("panel-btn", id)), Sense::click());
    let mut down = response.is_pointer_button_down_on();
    // A very fast click can begin and end between two frames; keep it down for one frame.
    if response.clicked() {
        down = true;
    }
    paint_panel_button(
        ui.painter(),
        rect,
        label,
        hint,
        down || held || led,
        tint,
    );
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        down,
    )
}

/// A clickable note key. Returns its response and whether the mouse is holding it.
#[allow(clippy::too_many_arguments)]
fn panel_key_ui(
    ui: &mut egui::Ui,
    rect: Rect,
    id: u8,
    white: bool,
    note: &str,
    hint: &str,
    held: bool,
    led: bool,
) -> (egui::Response, bool) {
    let response = ui.interact(rect, ui.id().with(("panel-key", id)), Sense::click());
    let mut down = response.is_pointer_button_down_on();
    if response.clicked() {
        down = true;
    }
    paint_panel_key(
        ui.painter(),
        rect,
        white,
        note,
        hint,
        down || held || led,
    );
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        down,
    )
}

/// A keycap on the miniature FM-1 panel. `lit` is the pressed/hold state; the small
/// indicator beside the label uses the button's own colour (green for PLAY, amber
/// otherwise).
fn paint_panel_button(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    hint: &str,
    lit: bool,
    tint: Color32,
) {
    let fill = if lit {
        mix_color(PANEL_BUTTON, tint, 0.45)
    } else {
        PANEL_BUTTON
    };
    let ink = if lit {
        Color32::from_rgb(34, 26, 10)
    } else {
        SILK
    };
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, PANEL_BUTTON_EDGE),
        StrokeKind::Inside,
    );

    let led_center = Pos2::new(rect.left() + 9.0, rect.center().y);
    painter.circle_filled(led_center, 4.0, PANEL_BUTTON_EDGE);
    painter.circle_filled(
        led_center,
        3.0,
        if lit { tint } else { Color32::from_rgb(48, 51, 56) },
    );
    if lit {
        painter.circle_filled(
            Pos2::new(led_center.x - 0.8, led_center.y - 0.9),
            1.0,
            Color32::from_rgb(255, 245, 220),
        );
    }

    painter.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(9.5),
        ink,
    );
    painter.text(
        Pos2::new(rect.right() - 6.0, rect.center().y),
        Align2::RIGHT_CENTER,
        hint,
        FontId::monospace(8.5),
        if lit {
            Color32::from_rgb(60, 48, 24)
        } else {
            SILK_DIM
        },
    );
    if lit {
        painter.rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(4),
            Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 80),
            ),
            StrokeKind::Middle,
        );
    }
}

/// One note key on the miniature panel: ivory/black at rest, amber while held.
fn paint_panel_key(
    painter: &egui::Painter,
    rect: Rect,
    white: bool,
    note: &str,
    hint: &str,
    lit: bool,
) {
    if white {
        painter.rect_filled(
            rect,
            CornerRadius::same(1),
            if lit { AMBER } else { KEY_WHITE },
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, Color32::from_rgb(112, 107, 96)),
            StrokeKind::Inside,
        );
        let ink = if lit {
            Color32::from_rgb(44, 27, 4)
        } else {
            Color32::from_rgb(45, 43, 40)
        };
        if !hint.is_empty() {
            painter.text(
                Pos2::new(rect.center().x, rect.top() + rect.height() * 0.55),
                Align2::CENTER_CENTER,
                hint,
                FontId::proportional(16.0),
                ink,
            );
        }
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_CENTER,
            note,
            FontId::proportional(10.0),
            if lit {
                ink
            } else {
                Color32::from_rgb(96, 91, 82)
            },
        );
    } else {
        painter.rect_filled(
            rect,
            CornerRadius::same(1),
            if lit { AMBER } else { KEY_BLACK },
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, Color32::from_rgb(18, 18, 20)),
            StrokeKind::Inside,
        );
        let ink = if lit {
            Color32::from_rgb(44, 27, 4)
        } else {
            Color32::from_rgb(214, 211, 202)
        };
        if !hint.is_empty() {
            painter.text(
                Pos2::new(rect.center().x, rect.center().y),
                Align2::CENTER_CENTER,
                hint,
                FontId::proportional(14.0),
                ink,
            );
        }
    }
}
