// Mark overlay layout. Renders a glyph at each set mark's screen position, styled exactly
// like the grid cell letters. Shares the glyph rasterization (overlay_glyph) and the
// per-platform compositor (overlay_surface) with the grid overlay; this file only decides
// where each mark glyph goes.

#[cfg(target_os = "linux")]
use super::overlay_glyph::blit_label_argb_u32;
#[cfg(target_os = "windows")]
use super::overlay_glyph::blit_label_bgra_u32;
#[cfg(target_os = "macos")]
use super::overlay_glyph::blit_label_bgra_u8;
use super::overlay_surface::OverlaySurface;
use crate::state::{MonitorInfo, Point};
use winit::window::Window;

// A mark to draw: its key label glyph and its position in virtual-desktop coordinates.
// main.rs builds this list from SharedState.marks so the overlay state is self-contained
// and cheap to compare for change detection.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkGlyph {
    pub label: char,
    pub position: Point,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkOverlayState {
    pub visible: bool,
    pub monitor: MonitorInfo,
    pub marks: Vec<MarkGlyph>,
}

pub struct MarkSurface {
    surface: OverlaySurface,
    // Bumped whenever the rendered content could differ, so the surface rebuilds its cache.
    version: u64,
    last_marks: Vec<MarkGlyph>,
    last_monitor: Option<MonitorInfo>,
}

impl MarkSurface {
    pub fn new() -> Self {
        Self {
            surface: OverlaySurface::new(),
            version: 0,
            last_marks: Vec::new(),
            last_monitor: None,
        }
    }

    pub fn update(&mut self, window: &Window, state: &MarkOverlayState) {
        // Content depends on the mark glyphs and the monitor (which sets the coordinate
        // mapping and surface size). Bump the version when either changes.
        if self.last_marks != state.marks || self.last_monitor != Some(state.monitor) {
            self.last_marks = state.marks.clone();
            self.last_monitor = Some(state.monitor);
            self.version = self.version.wrapping_add(1);
        }

        let marks = &state.marks;
        let monitor = state.monitor;
        self.surface.update(
            window,
            &monitor,
            state.visible,
            self.version,
            |pixels, w, h| fill_marks(pixels, w, h, &monitor, marks),
        );
    }
}

impl Default for MarkSurface {
    fn default() -> Self {
        Self::new()
    }
}

// Convert a mark's virtual-desktop position to monitor-local physical pixel coordinates,
// or None if the mark does not lie on this monitor. Uses the same physical-size convention
// as overlay_surface::monitor_size_physical (scale applied on macOS only).
fn mark_pixel(monitor: &MonitorInfo, position: Point) -> Option<(usize, usize)> {
    if !monitor.contains(position) {
        return None;
    }
    let local_x = position.x - monitor.origin.x;
    let local_y = position.y - monitor.origin.y;

    #[cfg(target_os = "macos")]
    let (px, py) = (
        local_x * monitor.scale_factor,
        local_y * monitor.scale_factor,
    );
    #[cfg(not(target_os = "macos"))]
    let (px, py) = (local_x, local_y);

    Some((px.round().max(0.0) as usize, py.round().max(0.0) as usize))
}

#[cfg(target_os = "windows")]
fn fill_marks(pixels: &mut [u32], w: usize, h: usize, monitor: &MonitorInfo, marks: &[MarkGlyph]) {
    for mark in marks {
        if let Some((cx, cy)) = mark_pixel(monitor, mark.position) {
            blit_label_bgra_u32(pixels, w, h, cx, cy, mark.label);
        }
    }
}

#[cfg(target_os = "macos")]
fn fill_marks(pixels: &mut [u8], w: usize, h: usize, monitor: &MonitorInfo, marks: &[MarkGlyph]) {
    pixels.fill(0);
    for mark in marks {
        if let Some((cx, cy)) = mark_pixel(monitor, mark.position) {
            blit_label_bgra_u8(pixels, w, h, cx, cy, mark.label);
        }
    }
}

#[cfg(target_os = "linux")]
fn fill_marks(pixels: &mut [u32], w: usize, h: usize, monitor: &MonitorInfo, marks: &[MarkGlyph]) {
    pixels.fill(0);
    for mark in marks {
        if let Some((cx, cy)) = mark_pixel(monitor, mark.position) {
            blit_label_argb_u32(pixels, w, h, cx, cy, mark.label);
        }
    }
}
