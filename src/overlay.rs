use crate::config::{OverlayPos, OVERLAY_CORNER, OVERLAY_SIZE_MONITOR_FRACTION};
use crate::state::{Mode, MonitorInfo, Shared};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use pixels::{Error, Pixels, SurfaceTexture};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder, WindowLevel};

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayState {
    pub mode: Mode,
    pub monitor: MonitorInfo,
}

pub fn create_window(event_loop: &EventLoop<()>) -> Window {
    WindowBuilder::new()
        .with_title("ViMouse")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1, 1))
        .build(event_loop)
        .expect("failed to create overlay window")
}

pub fn create_pixels(window: &Window) -> Pixels {
    let window_size = window.inner_size();
    let surface = SurfaceTexture::new(window_size.width, window_size.height, window);
    Pixels::new(window_size.width, window_size.height, surface).expect("pixels init failed")
}

// Snapshot just the overlay-relevant state so the UI code stays simple.
pub fn current_overlay(shared: &Shared) -> OverlayState {
    let state = shared.lock().expect("shared state poisoned");
    OverlayState {
        mode: state.mode,
        monitor: state
            .monitors
            .get(state.selected_monitor)
            .copied()
            .expect("selected monitor out of bounds"),
    }
}

// Overlay painting is intentionally tiny: move the window, draw the square, present it.
pub fn paint_overlay(
    window: &Window,
    pixels: &mut Pixels,
    overlay: &OverlayState,
) -> Result<(), Error> {
    let overlay_size = sync_overlay_size(window, pixels, &overlay.monitor)?;
    position_overlay(window, &overlay.monitor);
    draw_overlay(pixels.frame_mut(), overlay.mode, overlay_size as usize);
    pixels.render()
}

fn overlay_size_for_monitor(monitor: MonitorInfo) -> u32 {
    (monitor.width.min(monitor.height) * OVERLAY_SIZE_MONITOR_FRACTION)
        .round()
        .max(1.0) as u32
}

fn sync_overlay_size(
    window: &Window,
    pixels: &mut Pixels,
    monitor: &MonitorInfo,
) -> Result<u32, Error> {
    let overlay_size = overlay_size_for_monitor(*monitor);
    let inner_size = overlay_inner_size(monitor, overlay_size);

    if window.inner_size() != inner_size {
        set_overlay_inner_size(window, monitor, overlay_size);
        pixels.resize_buffer(inner_size.width, inner_size.height)?;
        pixels.resize_surface(inner_size.width, inner_size.height)?;
    }

    Ok(inner_size.width)
}

#[cfg(target_os = "macos")]
fn overlay_inner_size(monitor: &MonitorInfo, overlay_size: u32) -> PhysicalSize<u32> {
    let physical_size = (overlay_size as f64 * monitor.scale_factor)
        .round()
        .max(1.0) as u32;
    PhysicalSize::new(physical_size, physical_size)
}

#[cfg(not(target_os = "macos"))]
fn overlay_inner_size(_monitor: &MonitorInfo, overlay_size: u32) -> PhysicalSize<u32> {
    PhysicalSize::new(overlay_size, overlay_size)
}

#[cfg(target_os = "macos")]
fn set_overlay_inner_size(window: &Window, _monitor: &MonitorInfo, overlay_size: u32) {
    let overlay_size = overlay_size as f64;
    window.set_inner_size(LogicalSize::new(overlay_size, overlay_size));
}

#[cfg(not(target_os = "macos"))]
fn set_overlay_inner_size(window: &Window, _monitor: &MonitorInfo, overlay_size: u32) {
    window.set_inner_size(PhysicalSize::new(overlay_size, overlay_size));
}

#[cfg(target_os = "macos")]
fn position_overlay(window: &Window, monitor: &MonitorInfo) {
    let overlay_size = window.outer_size().to_logical::<f64>(monitor.scale_factor);
    let x = match OVERLAY_CORNER {
        OverlayPos::TopLeft | OverlayPos::BottomLeft => monitor.origin.x,
        OverlayPos::TopRight | OverlayPos::BottomRight => {
            monitor.origin.x + monitor.width - overlay_size.width
        }
    };
    let y = match OVERLAY_CORNER {
        OverlayPos::TopLeft | OverlayPos::TopRight => monitor.origin.y,
        OverlayPos::BottomLeft | OverlayPos::BottomRight => {
            monitor.origin.y + monitor.height - overlay_size.height
        }
    };
    window.set_outer_position(LogicalPosition::new(x, y));
}

#[cfg(not(target_os = "macos"))]
fn position_overlay(window: &Window, monitor: &MonitorInfo) {
    let overlay_size = window.outer_size();
    let x = match OVERLAY_CORNER {
        OverlayPos::TopLeft | OverlayPos::BottomLeft => monitor.origin.x,
        OverlayPos::TopRight | OverlayPos::BottomRight => {
            monitor.origin.x + monitor.width - overlay_size.width as f64
        }
    };
    let y = match OVERLAY_CORNER {
        OverlayPos::TopLeft | OverlayPos::TopRight => monitor.origin.y,
        OverlayPos::BottomLeft | OverlayPos::BottomRight => {
            monitor.origin.y + monitor.height - overlay_size.height as f64
        }
    };
    window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn draw_overlay(frame: &mut [u8], mode: Mode, overlay_size: usize) {
    // Fill the whole square first so there is never an unpainted border.
    let [r, g, b, a] = mode.background();
    for chunk in frame.chunks_exact_mut(4) {
        chunk[0] = r;
        chunk[1] = g;
        chunk[2] = b;
        chunk[3] = a;
    }

    let glyph = BASIC_FONTS
        .get(mode.label())
        .expect("overlay glyph should exist");
    // Preserve the original apparent text size while scaling with larger overlays.
    let scale = ((overlay_size * 3) + 20) / 40;
    let scale = scale.max(1);
    let mut min_col = 8usize;
    let mut max_col = 0usize;
    let mut min_row = 8usize;
    let mut max_row = 0usize;

    for (row, bits) in glyph.iter().enumerate() {
        if *bits == 0 {
            continue;
        }

        min_row = min_row.min(row);
        max_row = max_row.max(row);

        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }

            min_col = min_col.min(col);
            max_col = max_col.max(col);
        }
    }

    let glyph_width = (max_col - min_col + 1) * scale;
    let glyph_height = (max_row - min_row + 1) * scale;
    // Center the actual painted glyph bounds, not the full 8x8 font cell.
    let offset_x = (overlay_size - glyph_width) / 2;
    let offset_y = (overlay_size - glyph_height) / 2;

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }

            for dy in 0..scale {
                for dx in 0..scale {
                    let px = offset_x + ((col - min_col) * scale) + dx;
                    let py = offset_y + ((row - min_row) * scale) + dy;
                    let index = (py * overlay_size + px) * 4;
                    frame[index] = 255;
                    frame[index + 1] = 255;
                    frame[index + 2] = 255;
                    frame[index + 3] = 255;
                }
            }
        }
    }
}
