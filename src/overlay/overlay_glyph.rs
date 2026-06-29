// Shared glyph rasterization for overlays. The grid overlay and the mark overlay both
// draw 8×8 font glyphs (with an outline) onto premultiplied CPU pixel buffers; the only
// difference is *where* the glyphs go. This module owns the "how to draw a glyph" part;
// each overlay owns the "where" part.

use crate::config::{
    OVERLAY_LETTER_ALPHA, OVERLAY_LETTER_BRIGHTNESS, OVERLAY_LETTER_OUTLINE_ALPHA,
    OVERLAY_LETTER_OUTLINE_BRIGHTNESS, OVERLAY_LETTER_OUTLINE_THICKNESS, OVERLAY_LETTER_SIZE,
};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use rdev::Key;

// The character drawn for a key in an overlay (grid cells and marks).
pub fn key_label(key: Key) -> Option<char> {
    match key {
        Key::KeyA => Some('A'),
        Key::KeyB => Some('B'),
        Key::KeyC => Some('C'),
        Key::KeyD => Some('D'),
        Key::KeyE => Some('E'),
        Key::KeyF => Some('F'),
        Key::KeyG => Some('G'),
        Key::KeyH => Some('H'),
        Key::KeyI => Some('I'),
        Key::KeyJ => Some('J'),
        Key::KeyK => Some('K'),
        Key::KeyL => Some('L'),
        Key::KeyM => Some('M'),
        Key::KeyN => Some('N'),
        Key::KeyO => Some('O'),
        Key::KeyP => Some('P'),
        Key::KeyQ => Some('Q'),
        Key::KeyR => Some('R'),
        Key::KeyS => Some('S'),
        Key::KeyT => Some('T'),
        Key::KeyU => Some('U'),
        Key::KeyV => Some('V'),
        Key::KeyW => Some('W'),
        Key::KeyX => Some('X'),
        Key::KeyY => Some('Y'),
        Key::KeyZ => Some('Z'),
        Key::Num0 => Some('0'),
        Key::Num1 => Some('1'),
        Key::Num2 => Some('2'),
        Key::Num3 => Some('3'),
        Key::Num4 => Some('4'),
        Key::Num5 => Some('5'),
        Key::Num6 => Some('6'),
        Key::Num7 => Some('7'),
        Key::Num8 => Some('8'),
        Key::Num9 => Some('9'),
        _ => None,
    }
}

// True if the glyph pixel at scaled-block offset (gx, gy) is lit. Block is 8*s pixels square.
pub fn glyph_px_lit(glyph: [u8; 8], s: isize, gx: isize, gy: isize) -> bool {
    if gx < 0 || gy < 0 {
        return false;
    }
    let (col, row) = (gx / s, gy / s);
    row < 8 && col < 8 && (glyph[row as usize] >> col) & 1 == 1
}

// True if (gx, gy) is an outline pixel: not lit itself, but within OUTLINE_THICKNESS px of a lit pixel.
pub fn glyph_px_outline(glyph: [u8; 8], s: isize, gx: isize, gy: isize) -> bool {
    let t = OVERLAY_LETTER_OUTLINE_THICKNESS as isize;
    if t == 0 || glyph_px_lit(glyph, s, gx, gy) {
        return false;
    }
    for ny in gy - t..=gy + t {
        for nx in gx - t..=gx + t {
            if glyph_px_lit(glyph, s, nx, ny) {
                return true;
            }
        }
    }
    false
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u32 BGRA premultiplied pixel buffer.
// pixel format: little-endian u32 where bytes are B G R A.
#[cfg(target_os = "windows")]
pub fn blit_label_bgra_u32(pixels: &mut [u32], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (OVERLAY_LETTER_BRIGHTNESS as u32 * OVERLAY_LETTER_ALPHA as u32) / 255;
    let pixel: u32 = pm | (pm << 8) | (pm << 16) | ((OVERLAY_LETTER_ALPHA as u32) << 24);
    let opm =
        (OVERLAY_LETTER_OUTLINE_BRIGHTNESS as u32 * OVERLAY_LETTER_OUTLINE_ALPHA as u32) / 255;
    let outline: u32 =
        opm | (opm << 8) | (opm << 16) | ((OVERLAY_LETTER_OUTLINE_ALPHA as u32) << 24);
    let s = OVERLAY_LETTER_SIZE.max(1);
    let t = OVERLAY_LETTER_OUTLINE_THICKNESS;
    let ox = cx.saturating_sub(4 * s + t);
    let oy = cy.saturating_sub(4 * s + t);
    for gy in 0..8 * s + 2 * t {
        for gx in 0..8 * s + 2 * t {
            let (bx, by) = (gx as isize - t as isize, gy as isize - t as isize);
            let color = if glyph_px_lit(glyph, s as isize, bx, by) {
                pixel
            } else if glyph_px_outline(glyph, s as isize, bx, by) {
                outline
            } else {
                continue;
            };
            let (px, py) = (ox + gx, oy + gy);
            if px < w && py < h {
                pixels[py * w + px] = color;
            }
        }
    }
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u8×4 BGRA premultiplied pixel buffer.
// pixel format: bytes B G R A (macOS CGBitmapContext kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst).
#[cfg(target_os = "macos")]
pub fn blit_label_bgra_u8(pixels: &mut [u8], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (OVERLAY_LETTER_BRIGHTNESS as u32 * OVERLAY_LETTER_ALPHA as u32 / 255) as u8;
    let opm = (OVERLAY_LETTER_OUTLINE_BRIGHTNESS as u32 * OVERLAY_LETTER_OUTLINE_ALPHA as u32 / 255)
        as u8;
    let s = OVERLAY_LETTER_SIZE.max(1);
    let t = OVERLAY_LETTER_OUTLINE_THICKNESS;
    let ox = cx.saturating_sub(4 * s + t);
    let oy = cy.saturating_sub(4 * s + t);
    for gy in 0..8 * s + 2 * t {
        for gx in 0..8 * s + 2 * t {
            let (bx, by) = (gx as isize - t as isize, gy as isize - t as isize);
            let (g, a) = if glyph_px_lit(glyph, s as isize, bx, by) {
                (pm, OVERLAY_LETTER_ALPHA)
            } else if glyph_px_outline(glyph, s as isize, bx, by) {
                (opm, OVERLAY_LETTER_OUTLINE_ALPHA)
            } else {
                continue;
            };
            let (px, py) = (ox + gx, oy + gy);
            if px < w && py < h {
                let i = (py * w + px) * 4;
                pixels[i] = g;
                pixels[i + 1] = g;
                pixels[i + 2] = g;
                pixels[i + 3] = a;
            }
        }
    }
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u32 ARGB premultiplied pixel buffer.
// pixel format: native-endian u32 = 0xAARRGGBB.
#[cfg(target_os = "linux")]
pub fn blit_label_argb_u32(pixels: &mut [u32], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (OVERLAY_LETTER_BRIGHTNESS as u32 * OVERLAY_LETTER_ALPHA as u32) / 255;
    let pixel: u32 = ((OVERLAY_LETTER_ALPHA as u32) << 24) | (pm << 16) | (pm << 8) | pm;
    let opm =
        (OVERLAY_LETTER_OUTLINE_BRIGHTNESS as u32 * OVERLAY_LETTER_OUTLINE_ALPHA as u32) / 255;
    let outline: u32 =
        ((OVERLAY_LETTER_OUTLINE_ALPHA as u32) << 24) | (opm << 16) | (opm << 8) | opm;
    let s = OVERLAY_LETTER_SIZE.max(1);
    let t = OVERLAY_LETTER_OUTLINE_THICKNESS;
    let ox = cx.saturating_sub(4 * s + t);
    let oy = cy.saturating_sub(4 * s + t);
    for gy in 0..8 * s + 2 * t {
        for gx in 0..8 * s + 2 * t {
            let (bx, by) = (gx as isize - t as isize, gy as isize - t as isize);
            let color = if glyph_px_lit(glyph, s as isize, bx, by) {
                pixel
            } else if glyph_px_outline(glyph, s as isize, bx, by) {
                outline
            } else {
                continue;
            };
            let (px, py) = (ox + gx, oy + gy);
            if px < w && py < h {
                pixels[py * w + px] = color;
            }
        }
    }
}
