//! Everything that turns an emulator's RGB565 framebuffer into pixels on screen: the
//! colour decode, the dominant-colour pickup for the computed grid background, the
//! NTSC-distorted fisheye snapshot of the selected pane, the hqx upscale of the
//! sidebar's picture, and the desaturated vignetted look of the other running panes.
//! Pure functions; no app state. Pixels are `[r, g, b]` bytes; [`frame_image`] packs
//! them into the gpui `RenderImage` the views paint.

use fm1_performer::emulator::{LCD_H, LCD_W};
use gpui_kit::RenderImage;
use image::{Frame, ImageBuffer, Rgba};
use std::sync::Arc;

/// An opaque 8-bit pixel.
pub(crate) type Rgb = [u8; 3];

/// The dark frame around every LCD.
pub(crate) const BEZEL: u32 = 0x363636;

/// Rec.709 luminance of linear-ish 0..255 channels.
pub fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Decode one RGB565 framebuffer pixel, expanding each channel back to 8 bits.
pub fn rgb565(px: u16) -> Rgb {
    let r = ((px >> 11) & 0x1f) as u8;
    let g = ((px >> 5) & 0x3f) as u8;
    let b = (px & 0x1f) as u8;
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// A blank (black) LCD image, for power-off and startup.
pub(crate) fn blank() -> Vec<Rgb> {
    vec![[0, 0, 0]; LCD_W * LCD_H]
}

/// The dominant colour of a framebuffer: the most common 4-bits-per-channel bucket.
/// An all-zero (powered-off) framebuffer yields black.
pub fn dominant(fb: &[u16]) -> Rgb {
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
        return [0, 0, 0];
    }
    let chan = |bits: u32| ((bits << 4) | bits) as u8;
    [
        chan((best >> 8) as u32 & 0xf),
        chan((best >> 4) as u32 & 0xf),
        chan(best as u32 & 0xf),
    ]
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
fn ntsc(fb: &[u16], frame: u64) -> Vec<Rgb> {
    let src: Vec<Rgb> = fb.iter().map(|&px| rgb565(px)).collect();
    let lum_of = |c: Rgb| luminance(c[0] as f32, c[1] as f32, c[2] as f32);
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
            let mix = |chan: fn(&Rgb) -> u8| {
                let avg = [t0, t1, t2]
                    .iter()
                    .map(|t| chan(t) as f32 - lum_of(*t))
                    .sum::<f32>()
                    / 3.0;
                (y_here + avg).clamp(0.0, 255.0) as u8
            };
            out.push([mix(|c| c[0]), mix(|c| c[1]), mix(|c| c[2])]);
        }
    }
    out
}

/// The grid card's picture of the selected pane: the screen NTSC-distorted, then given
/// a very slight fisheye bulge. The fisheye magnifies the centre ~8%, keeps the edge of
/// the inscribed circle put and clamps the corners. `frame` varies the NTSC pass, so
/// every snapshot distorts a little differently.
pub(crate) fn ghost(fb: &[u16], frame: u64) -> Vec<Rgb> {
    let screen = ntsc(fb, frame);
    let c = LCD_W as f32 / 2.0;
    (0..LCD_W * LCD_H)
        .map(|i| {
            let u = (i % LCD_W) as f32 + 0.5 - c;
            let v = (i / LCD_W) as f32 + 0.5 - c;
            let r2 = (u * u + v * v) / (c * c);
            let f = 1.0 - 0.08 * (1.0 - r2);
            let sample = |d: f32| ((d * f + c).round() as i32).clamp(0, LCD_W as i32 - 1) as usize;
            screen[sample(v) * LCD_W + sample(u)]
        })
        .collect()
}

/// The grid card's picture of a running pane that is not selected: the faithful screen
/// slightly desaturated (channels pulled 30% towards their luminance) and vignetted —
/// the edges and corners darkened, the inverse of a lens flare's glow — so the
/// selection pops and the other panes recede while staying legible.
pub(crate) fn recede(fb: &[u16]) -> Vec<Rgb> {
    let c = LCD_W as f32 / 2.0;
    fb.iter()
        .enumerate()
        .map(|(i, &px)| {
            let source = rgb565(px);
            let u = (i % LCD_W) as f32 + 0.5 - c;
            let v = (i / LCD_W) as f32 + 0.5 - c;
            let r2 = (u * u + v * v) / (c * c);
            let shade = 1.0 - 0.3 * r2;
            let (r, g, b) = (source[0] as f32, source[1] as f32, source[2] as f32);
            let lum = luminance(r, g, b);
            let recessed = |v: f32| ((lum + (v - lum) * 0.7) * shade).clamp(0.0, 255.0) as u8;
            [recessed(r), recessed(g), recessed(b)]
        })
        .collect()
}

/// The sidebar's picture of the selected pane's screen: the faithful pixels run through
/// the hqx filter at an exact 2x or 4x (`scale`), smoothing jagged edges while keeping
/// lines crisp. Any other scale passes the pixels through untouched (1x needs no
/// filter, and fractional scales never reach here).
pub(crate) fn upscaled(pixels: &[Rgb], scale: usize) -> Vec<Rgb> {
    if scale != 2 && scale != 4 {
        return pixels.to_vec();
    }
    // hqx works on 0xAARRGGBB words; the LCD is opaque, so alpha is always full.
    let src: Vec<u32> = pixels
        .iter()
        .map(|c| 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();
    let mut dest = vec![0u32; LCD_W * LCD_H * scale * scale];
    match scale {
        2 => hqx::hq2x(&src, &mut dest, LCD_W as u32, LCD_H as u32),
        _ => hqx::hq4x(&src, &mut dest, LCD_W as u32, LCD_H as u32),
    }
    dest.iter()
        .map(|&v| {
            [
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ]
        })
        .collect()
}

/// Pack RGB pixels into the image a gpui `img()` element paints. The renderer's image
/// atlas is BGRA8 and uploads `RenderImage` buffers unchanged, so the bytes go in BGRA
/// order (swapping this back to RGBA trades red for blue on every screen).
pub(crate) fn frame_image(pixels: &[Rgb], width: usize, height: usize) -> Arc<RenderImage> {
    let mut bgra = Vec::with_capacity(width * height * 4);
    for &[r, g, b] in pixels {
        bgra.extend_from_slice(&[b, g, r, 255]);
    }
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width as u32, height as u32, bgra)
        .expect("pixel count matches dimensions");
    Arc::new(RenderImage::new([Frame::new(buffer)]))
}

/// A blank (power-off / startup) image.
pub(crate) fn blank_image() -> Arc<RenderImage> {
    frame_image(&blank(), LCD_W, LCD_H)
}
