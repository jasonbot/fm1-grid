//! Everything that turns an emulator's RGB565 framebuffer into pixels on screen: the
//! colour decode, the dominant-colour pickup for the ambient background, the
//! NTSC-distorted hypersaturated fisheye snapshot of the selected pane, and the bezel +
//! image view both the grid cards and the sidebar draw. Pure functions; no app state.

use crate::emulator::{LCD_H, LCD_W};
use crate::theme::paint_inset;
use eframe::egui;
use egui::{Color32, ColorImage, CornerRadius, Rect, TextureHandle, TextureOptions};

/// The dark frame around every LCD.
pub const BEZEL: Color32 = Color32::from_rgb(54, 54, 54);

/// Rec.709 luminance of linear-ish 0..255 channels.
pub fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Decode one RGB565 framebuffer pixel, expanding each channel back to 8 bits.
pub fn rgb565(px: u16) -> Color32 {
    let r = ((px >> 11) & 0x1f) as u8;
    let g = ((px >> 5) & 0x3f) as u8;
    let b = (px & 0x1f) as u8;
    Color32::from_rgb((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
}

/// A blank (black) LCD image, for power-off and startup.
pub fn blank() -> ColorImage {
    ColorImage::new([LCD_W, LCD_H], vec![Color32::BLACK; LCD_W * LCD_H])
}

/// The dominant colour of a framebuffer: the most common 4-bits-per-channel bucket.
/// An all-zero (powered-off) framebuffer yields black.
pub fn dominant(fb: &[u16]) -> Color32 {
    let mut counts = [0u32; 4096];
    for &px in fb {
        let key = ((((px >> 11) & 0x1f) >> 1) << 8
            | (((px >> 5) & 0x3f) >> 2) << 4
            | ((px & 0x1f) >> 1)) as usize;
        counts[key] += 1;
    }
    let mut best = 0usize;
    for (key, &count) in counts.iter().enumerate() {
        if count > counts[best] {
            best = key;
        }
    }
    if counts[best] == 0 {
        return Color32::BLACK;
    }
    let chan = |bits: u32| ((bits << 4) | bits) as u8;
    Color32::from_rgb(
        chan((best >> 8) as u32 & 0xf),
        chan((best >> 4) as u32 & 0xf),
        chan(best as u32 & 0xf),
    )
}

/// NTSC-style composite artefacts, applied before the lens: sharp luma with the chroma
/// smeared horizontally (colour bleed) and sampled with a per-line alternating phase
/// offset (the NTSC subcarrier flips phase every line; this fakes the resulting
/// dot-crawl / cross-colour combing on vertical edges).
///
/// `frame` makes the distortion dynamic: it seeds a tiny xorshift, and per line the
/// chroma phase offset wanders (mostly the alternating ±1, sometimes 0 or doubled) and
/// roughly every twelfth line gets a 1 px horizontal sync wobble, so two snapshots of
/// the very same pixels are never quite identical.
fn ntsc(fb: &[u16], frame: u64) -> Vec<Color32> {
    let src: Vec<Color32> = fb.iter().map(|&px| rgb565(px)).collect();
    let lum_of = |c: Color32| luminance(c.r() as f32, c.g() as f32, c.b() as f32);
    let mut rng = frame.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut out = Vec::with_capacity(src.len());
    for y in 0..LCD_H {
        let r = rand();
        let base = if y % 2 == 0 { 1isize } else { -1 };
        let off = match r % 8 {
            0 => 0,
            1 => base * 2,
            _ => base,
        };
        // H-sync hunt: about one line in twelve shifts sideways a pixel.
        let shift = if r % 12 == 5 { ((r >> 8) % 3) as isize - 1 } else { 0 };
        for x in 0..LCD_W {
            let xw = (x as isize + shift).clamp(0, LCD_W as isize - 1) as usize;
            let here = src[y * LCD_W + xw];
            let tap = |d: isize| {
                src[y * LCD_W + (xw as isize + off + d).clamp(0, LCD_W as isize - 1) as usize]
            };
            let (t0, t1, t2) = (tap(-1), tap(0), tap(1));
            let y_here = lum_of(here);
            // Luma stays the pixel's own; chroma (channel - luma) is the taps' average.
            let mix = |chan: fn(Color32) -> u8| {
                let avg = [t0, t1, t2]
                    .iter()
                    .map(|t| chan(*t) as f32 - lum_of(*t))
                    .sum::<f32>()
                    / 3.0;
                (y_here + avg).clamp(0.0, 255.0) as u8
            };
            out.push(Color32::from_rgb(
                mix(|c| c.r()),
                mix(|c| c.g()),
                mix(|c| c.b()),
            ));
        }
    }
    out
}

/// The grid card's picture of the selected pane: the screen NTSC-distorted, then
/// hypersaturated with a very slight fisheye bulge. The fisheye magnifies the centre
/// ~8%, keeps the edge of the inscribed circle put and clamps the corners; the
/// hypersaturation pushes every channel away from the pixel's luminance and clamps, so
/// colours pop hard while black stays black and white stays white. `frame` varies the
/// NTSC pass, so every snapshot distorts a little differently.
pub fn ghost(fb: &[u16], frame: u64) -> Vec<Color32> {
    let screen = ntsc(fb, frame);
    let c = LCD_W as f32 / 2.0;
    (0..LCD_W * LCD_H)
        .map(|i| {
            let u = (i % LCD_W) as f32 + 0.5 - c;
            let v = (i / LCD_W) as f32 + 0.5 - c;
            let r2 = (u * u + v * v) / (c * c);
            let f = 1.0 - 0.08 * (1.0 - r2);
            let sample = |d: f32| ((d * f + c).round() as i32).clamp(0, LCD_W as i32 - 1) as usize;
            let source = screen[sample(v) * LCD_W + sample(u)];
            let (r, g, b) = (source.r() as f32, source.g() as f32, source.b() as f32);
            let lum = luminance(r, g, b);
            let hyper = |v: f32| (lum + (v - lum) * 3.0).clamp(0.0, 255.0) as u8;
            Color32::from_rgb(hyper(r), hyper(g), hyper(b))
        })
        .collect()
}

/// Draw an LCD at `bezel`: the dark inset frame plus the texture inside it. The caller
/// allocates the rect (grid cell or sidebar); this only paints.
pub fn view_at(ui: &mut egui::Ui, bezel: Rect, texture: &TextureHandle) {
    paint_inset(ui.painter(), bezel, CornerRadius::same(6), BEZEL);
    let image_rect = bezel.shrink(3.0);
    ui.put(
        image_rect,
        egui::Image::new(texture).fit_to_exact_size(image_rect.size()),
    );
}

/// Upload helper shared by the per-frame live textures and the 3 Hz ghost.
pub fn set_lcd(texture: &mut TextureHandle, pixels: Vec<Color32>) {
    texture.set(
        ColorImage::new([LCD_W, LCD_H], pixels),
        TextureOptions::NEAREST,
    );
}

/// Blank (power-off / startup) a texture.
pub fn blank_to(texture: &mut TextureHandle) {
    texture.set(blank(), TextureOptions::NEAREST);
}
