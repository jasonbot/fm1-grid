//! The look. App chrome follows the gpui-kit theme (`cx.theme()` tokens at the call
//! site); this module keeps only the fixed colours of the FM-1 hardware the window
//! depicts — the panel body, silk-screen, keycaps, LEDs and LCD greens — plus the
//! small colour helpers the computed grid background and the panel need. The one
//! computed colour in the app is the grid background (see `grid.rs`); everything else
//! is a theme token.

/// The FM-1's front panel and keycaps.
pub(crate) const PANEL_BODY: u32 = 0x36383c;
pub(crate) const PANEL_BUTTON: u32 = 0x404349;
pub(crate) const PANEL_BUTTON_HOVER: u32 = 0x4e5158;
pub(crate) const PANEL_BUTTON_EDGE: u32 = 0x1c1c1e;
pub(crate) const SILK: u32 = 0xc9c6bd;
pub(crate) const SILK_DIM: u32 = 0x807e78;
pub(crate) const AMBER: u32 = 0xf2a33a;
pub(crate) const GREEN: u32 = 0x62d36e;
pub(crate) const KEY_WHITE: u32 = 0xebe8de;
pub(crate) const KEY_BLACK: u32 = 0x26262a;

/// Dark ink on a lit (amber/green) control; from the machine's own palette.
pub(crate) const LIT_INK: u32 = 0x221a0a;
pub(crate) const LIT_HINT: u32 = 0x3c3018;

/// Ink on the ivory keys, and the key's edge.
pub(crate) const KEY_INK: u32 = 0x2d2b28;
pub(crate) const KEY_NOTE: u32 = 0x605b52;
pub(crate) const KEY_EDGE: u32 = 0x706b60;
pub(crate) const KEY_BLACK_EDGE: u32 = 0x121214;
pub(crate) const KEY_BLACK_INK: u32 = 0xd6d3ca;

/// Linear per-channel mix of two 0xRRGGBB colours.
pub(crate) fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xFF) as f32;
        let y = ((b >> shift) & 0xFF) as f32;
        (x + (y - x) * t).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Halfway between two theme colours, per channel in sRGB: the health ramp's orange
/// sits between the theme's `warning` and `danger`.
pub(crate) fn halfway(a: gpui_kit::Hsla, b: gpui_kit::Hsla) -> gpui_kit::Hsla {
    let (a, b) = (gpui_kit::Rgba::from(a), gpui_kit::Rgba::from(b));
    gpui_kit::Rgba {
        r: (a.r + b.r) * 0.5,
        g: (a.g + b.g) * 0.5,
        b: (a.b + b.b) * 0.5,
        a: (a.a + b.a) * 0.5,
    }
    .into()
}

/// Layout constants shared across the window.
///
/// The sidebar split's initial/min/max widths: the screen at 2x, 1x and 4x plus its
/// bezel and padding.
pub(crate) const SIDEBAR_INITIAL: f32 = 512.0;
pub(crate) const SIDEBAR_MIN: f32 = 272.0;
pub(crate) const SIDEBAR_MAX: f32 = 992.0;
/// The firmware comboboxes' dropdown width (sidebar and devices pane).
pub(crate) const COMBO_MENU_W: f32 = 260.0;
