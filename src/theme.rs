//! The Adwaita-inspired look: palette, egui style, fonts and the small custom widgets
//! (switch, stepper, cards, LEDs) shared across the window. No app logic lives here.

use crate::lcd::luminance;
use eframe::egui;
use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextStyle, Vec2,
};

// GNOME Adwaita (light): a near-white window, white cards with hairline borders, the
// #3584e4 accent, rounded corners and no bevels. The grid area's background instead takes
// the dominant colour of the selected pane's screen.
pub const CARD_BG: Color32 = Color32::WHITE;
pub const FAINT_BG: Color32 = Color32::from_rgb(240, 239, 237);
pub const BUTTON_BG: Color32 = Color32::from_rgb(231, 229, 226);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(223, 221, 218);
pub const BUTTON_ACTIVE: Color32 = Color32::from_rgb(213, 211, 208);
pub const BORDER: Color32 = Color32::from_black_alpha(22);
/// Softer than BORDER: the separator line keeps BORDER; big panes get this lighter edge.
pub const BORDER_SOFT: Color32 = Color32::from_black_alpha(14);
/// Opaque edge for the window-level panels (sidebar, transport bar). Slightly lighter than
/// the separator hairline; opaque because egui 0.33 paints panel frame strokes as if the
/// alpha were ignored.
pub const PANEL_EDGE: Color32 = Color32::from_rgb(0xee, 0xed, 0xec);
pub const INK: Color32 = Color32::from_rgb(36, 36, 36);
pub const DIM: Color32 = Color32::from_rgb(112, 110, 116);
pub const ACCENT: Color32 = Color32::from_rgb(53, 132, 228);
pub const RED: Color32 = Color32::from_rgb(192, 28, 40);
pub const LCD_BG: Color32 = Color32::from_rgb(24, 32, 26);
pub const LCD_ON: Color32 = Color32::from_rgb(126, 235, 146);
pub const LCD_OFF: Color32 = Color32::from_rgb(74, 100, 80);
pub const LED_OFF: Color32 = Color32::from_rgb(122, 130, 122);
pub const LED_ON: Color32 = Color32::from_rgb(52, 190, 96);

// The miniature front-panel display above the metronome.
pub const PANEL_BODY: Color32 = Color32::from_rgb(54, 56, 60);
pub const PANEL_BUTTON: Color32 = Color32::from_rgb(64, 67, 73);
pub const PANEL_BUTTON_EDGE: Color32 = Color32::from_rgb(28, 28, 30);
pub const SILK: Color32 = Color32::from_rgb(201, 198, 189);
pub const SILK_DIM: Color32 = Color32::from_rgb(128, 126, 120);
pub const AMBER: Color32 = Color32::from_rgb(242, 163, 58);
pub const GREEN: Color32 = Color32::from_rgb(98, 211, 110);
pub const KEY_WHITE: Color32 = Color32::from_rgb(235, 232, 222);
pub const KEY_BLACK: Color32 = Color32::from_rgb(38, 38, 42);

pub fn apply_adwaita_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = CARD_BG;
    visuals.window_fill = CARD_BG;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.extreme_bg_color = CARD_BG;
    visuals.faint_bg_color = FAINT_BG;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.weak_text_alpha = 0.72;
    visuals.button_frame = true;
    visuals.striped = false;
    visuals.interact_cursor = None;
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(50),
    };
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(45),
    };

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = CARD_BG;
    widgets.noninteractive.weak_bg_fill = CARD_BG;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.noninteractive.corner_radius = CornerRadius::same(6);

    widgets.inactive.bg_fill = BUTTON_BG;
    widgets.inactive.weak_bg_fill = BUTTON_BG;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(22));
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.inactive.corner_radius = CornerRadius::same(6);

    widgets.hovered.bg_fill = BUTTON_HOVER;
    widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(28));
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.hovered.corner_radius = CornerRadius::same(6);

    widgets.active.bg_fill = BUTTON_ACTIVE;
    widgets.active.weak_bg_fill = BUTTON_ACTIVE;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(28));
    widgets.active.fg_stroke = Stroke::new(1.0_f32, INK);
    widgets.active.corner_radius = CornerRadius::same(6);

    widgets.open = widgets.hovered.clone();

    style.text_styles = [
        (TextStyle::Small, FontId::new(10.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(9.0, 3.0);
    style.spacing.interact_size = Vec2::new(26.0, 22.0);
    style.spacing.menu_margin = Margin::same(6);
    style.animation_time = 0.08;

    style.visuals = visuals;
    ctx.set_style(style);

    load_system_sans(ctx);
}

/// Use the system default sans-serif (via fontconfig) for proportional text, if available.
fn load_system_sans(ctx: &egui::Context) {
    let path = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|file| file.trim().to_string())
        .filter(|file| !file.is_empty());
    let Some(path) = path else { return };
    let Ok(bytes) = std::fs::read(path) else { return };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("system-sans".into(), egui::FontData::from_owned(bytes).into());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "system-sans".into());
    ctx.set_fonts(fonts);
}

/// A flat Adwaita surface: fill plus a hairline border. No bevels, no hard outlines.
pub fn paint_card(painter: &egui::Painter, rect: Rect, radius: CornerRadius, fill: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
}

/// A flat sunken surface (LCD bezel, device panel body): fill plus a soft hairline border.
pub fn paint_inset(painter: &egui::Painter, rect: Rect, radius: CornerRadius, fill: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0_f32, BORDER_SOFT), StrokeKind::Inside);
}

pub fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
    )
}

/// The device sidebar's background: the ambient grid colour, slightly desaturated and
/// strongly lightened so dark text and controls stay readable on it. Black becomes a
/// light grey; saturated colours become pale pastels.
pub fn ambient_tint(c: Color32) -> Color32 {
    let lum = luminance(c.r() as f32, c.g() as f32, c.b() as f32) as u8;
    let gray = Color32::from_rgb(lum, lum, lum);
    // Keep ~60% of the saturation, then mix 85% towards white.
    mix_color(mix_color(c, gray, 0.4), Color32::WHITE, 0.85)
}

/// The selection halo: the ambient colour's hue rotated towards the accent blue (away
/// from it when the ambient is already really blue, so the ring keeps contrasting with
/// the tinted background), at the accent's saturation and value so it stays vivid.
/// Dark or grey ambients have no usable hue: plain accent then.
pub fn halo_color(ambient: Color32) -> Color32 {
    let accent = egui::ecolor::HsvaGamma::from(ACCENT);
    let hsv = egui::ecolor::HsvaGamma::from(ambient);
    if hsv.s < 0.12 || hsv.v < 0.25 {
        return ACCENT;
    }
    // Signed shortest hue distance from the ambient to the accent blue.
    let d = (accent.h - hsv.h + 0.5).rem_euclid(1.0) - 0.5;
    let h = if d.abs() < 0.09 {
        // Already really blue: rotate away from the blue (either way if it is spot on).
        let dir = if d.abs() < 0.01 { 1.0 } else { -d.signum() };
        (hsv.h + dir * 0.12).rem_euclid(1.0)
    } else {
        // Two thirds of the way towards the blue.
        (hsv.h + d * 0.66).rem_euclid(1.0)
    };
    egui::ecolor::HsvaGamma {
        h,
        s: accent.s.max(hsv.s),
        v: accent.v,
        a: 1.0,
    }
    .into()
}

/// A fixed-size cell that centers its contents both ways. egui's plain horizontal layout
/// centers each widget against the row height so far, so widgets added before the tallest
/// one end up anchored to the top; fixed cells avoid that.
pub fn centered_cell<R>(
    ui: &mut egui::Ui,
    size: Vec2,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.allocate_ui_with_layout(
        size,
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        add,
    )
    .inner
}

/// An Adwaita-style switch. Returns the response; toggles `on` when clicked.
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(46.0, 26.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    let painter = ui.painter();
    let track = match (*on, response.hovered()) {
        (true, true) => Color32::from_rgb(43, 120, 214),
        (true, false) => ACCENT,
        (false, true) => Color32::from_rgb(199, 196, 191),
        (false, false) => Color32::from_rgb(211, 208, 203),
    };
    painter.rect_filled(rect, CornerRadius::same(13), track);

    let knob_r = 9.0;
    let knob_x = if *on {
        rect.right() - 4.0 - knob_r
    } else {
        rect.left() + 4.0 + knob_r
    };
    let center = Pos2::new(knob_x, rect.center().y);
    painter.circle_filled(
        center + Vec2::new(0.0, 1.0),
        knob_r,
        Color32::from_black_alpha(28),
    );
    painter.circle_filled(center, knob_r, Color32::WHITE);

    response
}

/// The two-segment tempo control: `[ − ][ + ]`, one flat segmented control.
/// Returns `(down, up)` responses.
pub fn tempo_stepper(ui: &mut egui::Ui) -> (egui::Response, egui::Response) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(58.0, 26.0), Sense::hover());
    let middle = rect.center().x;
    let down_rect = Rect::from_min_max(rect.min, Pos2::new(middle, rect.max.y));
    let up_rect = Rect::from_min_max(Pos2::new(middle, rect.min.y), rect.max);

    let down = ui.interact(down_rect, ui.id().with("tempo-down"), Sense::click());
    let up = ui.interact(up_rect, ui.id().with("tempo-up"), Sense::click());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), BUTTON_BG);

    let left_corners = CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 };
    let right_corners = CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 };
    for (response, half, corners) in [
        (&down, down_rect, left_corners),
        (&up, up_rect, right_corners),
    ] {
        if response.is_pointer_button_down_on() {
            painter.rect_filled(half.shrink(1.0), corners, BUTTON_ACTIVE);
        } else if response.hovered() {
            painter.rect_filled(half.shrink(1.0), corners, BUTTON_HOVER);
        }
    }

    // One hairline around both halves, then the divider: a single segmented control.
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            Pos2::new(middle, rect.top() + 4.0),
            Pos2::new(middle, rect.bottom() - 4.0),
        ],
        Stroke::new(1.0_f32, BORDER),
    );

    painter.text(
        down_rect.center(),
        Align2::CENTER_CENTER,
        "−",
        FontId::proportional(16.0),
        INK,
    );
    painter.text(
        up_rect.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(16.0),
        INK,
    );

    (down, up)
}

pub fn paint_led(painter: &egui::Painter, rect: Rect, lit: bool) {
    let center = rect.center();
    painter.circle_filled(center, 8.0, Color32::from_rgb(96, 96, 96));
    painter.circle_stroke(center, 8.0, Stroke::new(1.0_f32, Color32::from_rgb(48, 48, 48)));
    painter.circle_filled(center, 6.0, if lit { LED_ON } else { LED_OFF });
    painter.circle_stroke(center, 6.0, Stroke::new(1.0_f32, Color32::from_rgb(64, 64, 64)));
    if lit {
        painter.circle_filled(
            Pos2::new(center.x - 1.8, center.y - 1.8),
            2.0,
            Color32::from_rgb(214, 255, 220),
        );
    }
}
