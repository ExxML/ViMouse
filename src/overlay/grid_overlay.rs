// Grid overlay layout. Renders the 5×3 jump grid (lines and/or cell letters) onto a
// shared OverlaySurface. The "how to draw a glyph" and "how to composite the buffer"
// concerns live in overlay_glyph and overlay_surface respectively; this file only knows
// where the grid lines and cell letters go.

#[cfg(target_os = "linux")]
use super::overlay_glyph::blit_label_argb_u32;
#[cfg(target_os = "windows")]
use super::overlay_glyph::blit_label_bgra_u32;
#[cfg(target_os = "macos")]
use super::overlay_glyph::blit_label_bgra_u8;
use super::overlay_glyph::key_label;
use super::overlay_surface::OverlaySurface;
use crate::config::{GRID_ALPHA, GRID_BRIGHTNESS, GRID_THICKNESS, JUMP_GRID};
use crate::state::MonitorInfo;
use winit::window::Window;

const GRID_COLS: usize = JUMP_GRID[0].len();
const GRID_ROWS: usize = JUMP_GRID.len();

#[derive(Clone, Debug, PartialEq)]
pub struct GridOverlayState {
    pub visible: bool,
    pub show_letters: bool,
    pub monitor: MonitorInfo,
}

pub struct GridSurface {
    surface: OverlaySurface,
}

impl GridSurface {
    pub fn new(_window: &Window, _initial_monitor: &MonitorInfo) -> Self {
        Self {
            surface: OverlaySurface::new(),
        }
    }

    // prime() is kept for API compatibility but is now a no-op (initialization is lazy).
    pub fn prime(&mut self, _window: &Window, _monitor: &MonitorInfo) {}

    pub fn update(&mut self, window: &Window, state: &GridOverlayState) {
        let show_letters = state.show_letters;
        // Grid content only varies with the letters toggle (size is tracked by the surface).
        let version = show_letters as u64;
        self.surface.update(
            window,
            &state.monitor,
            state.visible,
            version,
            move |pixels, w, h| fill_grid(pixels, w, h, show_letters),
        );
    }
}

fn axis_line_centers(length: usize, cells: usize) -> impl Iterator<Item = usize> {
    std::iter::once(0).chain(
        (1..cells)
            .map(move |i| i * length / cells)
            .chain(std::iter::once(length.saturating_sub(1))),
    )
}

// On macOS the CALayer compositor does not render physical pixel row 0; inset top/bottom by 1px.
#[cfg(target_os = "macos")]
fn axis_line_centers_y(length: usize, cells: usize) -> impl Iterator<Item = usize> {
    std::iter::once(1).chain(
        (1..cells)
            .map(move |i| i * length / cells)
            .chain(std::iter::once(length.saturating_sub(1))),
    )
}

fn line_range(center: usize, length: usize) -> std::ops::Range<usize> {
    let start = center
        .saturating_sub(GRID_THICKNESS / 2)
        .min(length.saturating_sub(GRID_THICKNESS));
    let end = (start + GRID_THICKNESS).min(length);
    start..end
}

// Center pixel of each grid cell, for placing cell letters.
fn cell_center(col: usize, row: usize, w: usize, h: usize) -> (usize, usize) {
    let cx = (col * w / GRID_COLS) + (w / GRID_COLS / 2);
    let cy = (row * h / GRID_ROWS) + (h / GRID_ROWS / 2);
    (cx, cy)
}

// Fill pre-multiplied BGRA pixels for UpdateLayeredWindow (DIB memory layout: B G R A bytes).
// Pre-multiplied: R,G,B are multiplied by A/255.
#[cfg(target_os = "windows")]
fn fill_grid(pixels: &mut [u32], w: usize, h: usize, show_letters: bool) {
    if !show_letters {
        let pm = (GRID_BRIGHTNESS as u32 * GRID_ALPHA as u32) / 255;
        // Memory bytes: B=pm, G=pm, R=pm, A=GRID_ALPHA  →  little-endian u32
        let line_pixel: u32 = pm | (pm << 8) | (pm << 16) | ((GRID_ALPHA as u32) << 24);

        for x_center in axis_line_centers(w, GRID_COLS) {
            for y in 0..h {
                for x in line_range(x_center, w) {
                    pixels[y * w + x] = line_pixel;
                }
            }
        }

        for y_center in axis_line_centers(h, GRID_ROWS) {
            for y in line_range(y_center, h) {
                for x in 0..w {
                    pixels[y * w + x] = line_pixel;
                }
            }
        }
    } else {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let (cx, cy) = cell_center(col, row, w, h);
                    blit_label_bgra_u32(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}

// Pre-multiplied BGRA for macOS CGBitmapContext (kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst).
// In memory: B G R A per pixel; as little-endian u32 = 0xAARRGGBB.
#[cfg(target_os = "macos")]
fn fill_grid(pixels: &mut [u8], w: usize, h: usize, show_letters: bool) {
    pixels.fill(0);
    if !show_letters {
        let pm = (GRID_BRIGHTNESS as u32 * GRID_ALPHA as u32 / 255) as u8;
        // Memory layout per pixel: [B, G, R, A]
        for x_center in axis_line_centers(w, GRID_COLS) {
            for y in 0..h {
                for x in line_range(x_center, w) {
                    let i = (y * w + x) * 4;
                    pixels[i] = pm;
                    pixels[i + 1] = pm;
                    pixels[i + 2] = pm;
                    pixels[i + 3] = GRID_ALPHA;
                }
            }
        }
        for y_center in axis_line_centers_y(h, GRID_ROWS) {
            for y in line_range(y_center, h) {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    pixels[i] = pm;
                    pixels[i + 1] = pm;
                    pixels[i + 2] = pm;
                    pixels[i + 3] = GRID_ALPHA;
                }
            }
        }
    } else {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let (cx, cy) = cell_center(col, row, w, h);
                    blit_label_bgra_u8(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}

// Pre-multiplied ARGB for Linux XRender (native-endian u32 = 0xAARRGGBB).
#[cfg(target_os = "linux")]
fn fill_grid(pixels: &mut [u32], w: usize, h: usize, show_letters: bool) {
    pixels.fill(0);
    if !show_letters {
        let pm = (GRID_BRIGHTNESS as u32 * GRID_ALPHA as u32) / 255;
        let pixel: u32 = ((GRID_ALPHA as u32) << 24) | (pm << 16) | (pm << 8) | pm;
        for x_center in axis_line_centers(w, GRID_COLS) {
            for y in 0..h {
                for x in line_range(x_center, w) {
                    pixels[y * w + x] = pixel;
                }
            }
        }
        for y_center in axis_line_centers(h, GRID_ROWS) {
            for y in line_range(y_center, h) {
                for x in 0..w {
                    pixels[y * w + x] = pixel;
                }
            }
        }
    } else {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let (cx, cy) = cell_center(col, row, w, h);
                    blit_label_argb_u32(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}
