use crate::config::{OverlayIconPos, OVERLAY_ICON_POSITION, OVERLAY_ICON_SIZE_MONITOR_FRACTION};
use crate::jump_grid::metrics_for_size;
use crate::state::{Mode, MonitorInfo, Shared};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use pixels::{Error, Pixels, SurfaceTexture};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::{EventLoop, EventLoopBuilder};
#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
#[cfg(target_os = "windows")]
use winit::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
#[cfg(target_os = "linux")]
use winit::platform::x11::{
    EventLoopBuilderExtX11, WindowBuilderExtX11, WindowExtX11, XWindowType,
};
use winit::window::{Window, WindowBuilder, WindowLevel};

const GRID_COLOR: [u8; 4] = [128, 128, 128, 112];

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayState {
    pub mode: Mode,
    pub monitor: MonitorInfo,
    pub grid_visible: bool,
}

pub fn create_event_loop() -> EventLoop<()> {
    let mut builder = EventLoopBuilder::new();

    #[cfg(target_os = "macos")]
    {
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.with_default_menu(false);
        builder.with_activate_ignoring_other_apps(false);
    }

    #[cfg(target_os = "linux")]
    {
        // The Linux input and cursor backends already rely on X11 APIs.
        builder.with_x11();
    }

    builder.build()
}

pub fn create_window(event_loop: &EventLoop<()>) -> Window {
    let window = configure_window_builder(
        WindowBuilder::new()
            .with_title("ViMouse")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_active(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(PhysicalSize::new(1, 1)),
    )
    .build(event_loop)
    .expect("failed to create overlay window");

    configure_overlay_window(&window);
    window
}

pub fn show_overlay_window(window: &Window) {
    window.set_visible(true);
    finalize_overlay_window(window);
    configure_overlay_hittest(window);
}

pub fn create_pixels(window: &Window) -> Pixels {
    let window_size = window.inner_size();
    let surface = SurfaceTexture::new(window_size.width, window_size.height, window);
    let mut pixels =
        Pixels::new(window_size.width, window_size.height, surface).expect("pixels init failed");
    pixels.clear_color(pixels::wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    });
    pixels
}

pub fn current_overlay_state(shared: &Shared) -> OverlayState {
    let state = shared.lock().expect("shared state poisoned");
    OverlayState {
        mode: state.mode,
        monitor: state
            .monitors
            .get(state.selected_monitor)
            .copied()
            .expect("selected monitor out of bounds"),
        grid_visible: state.mode == Mode::Normal && state.overlay_grid_enabled,
    }
}

pub fn paint_overlay(
    window: &Window,
    pixels: &mut Pixels,
    overlay: &OverlayState,
) -> Result<(), Error> {
    let frame_size = sync_overlay_size(window, pixels, &overlay.monitor)?;
    position_overlay(window, &overlay.monitor);
    draw_overlay(
        pixels.frame_mut(),
        overlay,
        frame_size.width as usize,
        frame_size.height as usize,
    );
    pixels.render()
}

fn sync_overlay_size(
    window: &Window,
    pixels: &mut Pixels,
    monitor: &MonitorInfo,
) -> Result<PhysicalSize<u32>, Error> {
    let inner_size = overlay_inner_size(monitor);

    if window.inner_size() != inner_size {
        set_overlay_inner_size(window, monitor);
        pixels.resize_buffer(inner_size.width, inner_size.height)?;
        pixels.resize_surface(inner_size.width, inner_size.height)?;
    }

    Ok(inner_size)
}

#[cfg(target_os = "macos")]
fn overlay_inner_size(monitor: &MonitorInfo) -> PhysicalSize<u32> {
    let width = (monitor.width * monitor.scale_factor).round().max(1.0) as u32;
    let height = (monitor.height * monitor.scale_factor).round().max(1.0) as u32;
    PhysicalSize::new(width, height)
}

#[cfg(not(target_os = "macos"))]
fn overlay_inner_size(monitor: &MonitorInfo) -> PhysicalSize<u32> {
    let width = monitor.width.round().max(1.0) as u32;
    let height = monitor.height.round().max(1.0) as u32;
    PhysicalSize::new(width, height)
}

#[cfg(target_os = "macos")]
fn set_overlay_inner_size(window: &Window, monitor: &MonitorInfo) {
    window.set_inner_size(LogicalSize::new(
        monitor.width.max(1.0),
        monitor.height.max(1.0),
    ));
}

#[cfg(not(target_os = "macos"))]
fn set_overlay_inner_size(window: &Window, monitor: &MonitorInfo) {
    window.set_inner_size(PhysicalSize::new(
        monitor.width.round().max(1.0) as u32,
        monitor.height.round().max(1.0) as u32,
    ));
}

#[cfg(target_os = "macos")]
fn position_overlay(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(LogicalPosition::new(monitor.origin.x, monitor.origin.y));
}

#[cfg(not(target_os = "macos"))]
fn position_overlay(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(PhysicalPosition::new(
        monitor.origin.x.round() as i32,
        monitor.origin.y.round() as i32,
    ));
}

fn draw_overlay(frame: &mut [u8], overlay: &OverlayState, frame_width: usize, frame_height: usize) {
    clear_frame(frame);

    if overlay.grid_visible {
        draw_grid(frame, frame_width, frame_height);
    }

    draw_mode_badge(frame, frame_width, frame_height, overlay.mode);
}

fn clear_frame(frame: &mut [u8]) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk[0] = 0;
        chunk[1] = 0;
        chunk[2] = 0;
        chunk[3] = 0;
    }
}

fn draw_grid(frame: &mut [u8], frame_width: usize, frame_height: usize) {
    let metrics = metrics_for_size(frame_width as f64, frame_height as f64);

    for x in metrics.column_boundaries() {
        draw_vertical_line(frame, frame_width, frame_height, x, GRID_COLOR);
    }

    for y in metrics.row_boundaries() {
        draw_horizontal_line(frame, frame_width, frame_height, y, GRID_COLOR);
    }
}

fn draw_vertical_line(
    frame: &mut [u8],
    frame_width: usize,
    frame_height: usize,
    x: f64,
    color: [u8; 4],
) {
    let left = x.floor();
    let right = left + 1.0;
    let right_weight = x - left;

    blend_column(
        frame,
        frame_width,
        frame_height,
        left as isize,
        color,
        1.0 - right_weight,
    );
    blend_column(
        frame,
        frame_width,
        frame_height,
        right as isize,
        color,
        right_weight,
    );
}

fn draw_horizontal_line(
    frame: &mut [u8],
    frame_width: usize,
    frame_height: usize,
    y: f64,
    color: [u8; 4],
) {
    let top = y.floor();
    let bottom = top + 1.0;
    let bottom_weight = y - top;

    blend_row(
        frame,
        frame_width,
        frame_height,
        top as isize,
        color,
        1.0 - bottom_weight,
    );
    blend_row(
        frame,
        frame_width,
        frame_height,
        bottom as isize,
        color,
        bottom_weight,
    );
}

fn blend_column(
    frame: &mut [u8],
    frame_width: usize,
    frame_height: usize,
    x: isize,
    color: [u8; 4],
    weight: f64,
) {
    if !(0..frame_width as isize).contains(&x) || weight <= 0.0 {
        return;
    }

    let mut weighted = color;
    weighted[3] = (color[3] as f64 * weight).round() as u8;
    for y in 0..frame_height {
        blend_pixel(frame, frame_width, x as usize, y, weighted);
    }
}

fn blend_row(
    frame: &mut [u8],
    frame_width: usize,
    frame_height: usize,
    y: isize,
    color: [u8; 4],
    weight: f64,
) {
    if !(0..frame_height as isize).contains(&y) || weight <= 0.0 {
        return;
    }

    let mut weighted = color;
    weighted[3] = (color[3] as f64 * weight).round() as u8;
    for x in 0..frame_width {
        blend_pixel(frame, frame_width, x, y as usize, weighted);
    }
}

fn blend_pixel(frame: &mut [u8], frame_width: usize, x: usize, y: usize, color: [u8; 4]) {
    let index = (y * frame_width + x) * 4;
    let src_alpha = color[3] as f32 / 255.0;

    if src_alpha == 0.0 {
        return;
    }

    let dst_alpha = frame[index + 3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    let blend_channel = |src: u8, dst: u8| -> u8 {
        if out_alpha == 0.0 {
            return 0;
        }

        (((src as f32 * src_alpha) + (dst as f32 * dst_alpha * (1.0 - src_alpha))) / out_alpha)
            .round() as u8
    };

    frame[index] = blend_channel(color[0], frame[index]);
    frame[index + 1] = blend_channel(color[1], frame[index + 1]);
    frame[index + 2] = blend_channel(color[2], frame[index + 2]);
    frame[index + 3] = (out_alpha * 255.0).round() as u8;
}

fn draw_mode_badge(frame: &mut [u8], frame_width: usize, frame_height: usize, mode: Mode) {
    let badge_size = badge_size(frame_width, frame_height);
    let (badge_x, badge_y) = badge_origin(frame_width, frame_height, badge_size);
    fill_badge_background(
        frame,
        frame_width,
        badge_x,
        badge_y,
        badge_size,
        mode.background(),
    );
    draw_badge_glyph(
        frame,
        frame_width,
        badge_x,
        badge_y,
        badge_size,
        mode.label(),
    );
}

fn badge_size(frame_width: usize, frame_height: usize) -> usize {
    ((frame_width.min(frame_height) as f64) * OVERLAY_ICON_SIZE_MONITOR_FRACTION)
        .round()
        .max(1.0) as usize
}

fn badge_origin(frame_width: usize, frame_height: usize, badge_size: usize) -> (usize, usize) {
    let x = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::BottomLeft => 0,
        OverlayIconPos::TopRight | OverlayIconPos::BottomRight => {
            frame_width.saturating_sub(badge_size)
        }
    };
    let y = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::TopRight => 0,
        OverlayIconPos::BottomLeft | OverlayIconPos::BottomRight => {
            frame_height.saturating_sub(badge_size)
        }
    };

    (x, y)
}

fn fill_badge_background(
    frame: &mut [u8],
    frame_width: usize,
    badge_x: usize,
    badge_y: usize,
    badge_size: usize,
    color: [u8; 4],
) {
    for y in badge_y..badge_y + badge_size {
        for x in badge_x..badge_x + badge_size {
            let index = (y * frame_width + x) * 4;
            frame[index] = color[0];
            frame[index + 1] = color[1];
            frame[index + 2] = color[2];
            frame[index + 3] = color[3];
        }
    }
}

fn draw_badge_glyph(
    frame: &mut [u8],
    frame_width: usize,
    badge_x: usize,
    badge_y: usize,
    badge_size: usize,
    label: char,
) {
    let glyph = BASIC_FONTS.get(label).expect("overlay glyph should exist");
    let scale = ((badge_size * 3) + 20) / 40;
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
    let glyph_x = badge_x + badge_size.saturating_sub(glyph_width) / 2;
    let glyph_y = badge_y + badge_size.saturating_sub(glyph_height) / 2;

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }

            for dy in 0..scale {
                for dx in 0..scale {
                    let x = glyph_x + ((col - min_col) * scale) + dx;
                    let y = glyph_y + ((row - min_row) * scale) + dy;
                    let index = (y * frame_width + x) * 4;
                    frame[index] = 255;
                    frame[index + 1] = 255;
                    frame[index + 2] = 255;
                    frame[index + 3] = 255;
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn configure_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_skip_taskbar(true)
}

#[cfg(target_os = "linux")]
fn configure_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
        .with_override_redirect(true)
        .with_x11_window_type(vec![XWindowType::Notification])
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn configure_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
}

fn configure_overlay_window(window: &Window) {
    configure_overlay_hittest(window);
    configure_platform_overlay_window(window);
}

#[cfg(target_os = "macos")]
fn configure_overlay_hittest(window: &Window) {
    // macOS supports click-through directly through winit.
    let _ = window.set_cursor_hittest(false);
}

#[cfg(not(target_os = "macos"))]
fn configure_overlay_hittest(_window: &Window) {}

#[cfg(target_os = "linux")]
fn configure_platform_overlay_window(window: &Window) {
    use std::ffi::c_void;
    use x11_dl::{xfixes, xlib};

    const SHAPE_INPUT: i32 = 2;

    let Some(display) = window.xlib_display() else {
        return;
    };
    let Some(xwindow) = window.xlib_window() else {
        return;
    };
    let Ok(xlib) = xlib::Xlib::open() else {
        return;
    };
    let Ok(xfixes) = xfixes::Xfixes::open() else {
        return;
    };

    unsafe {
        let display = display as *mut xlib::Display;
        let hints = {
            let existing = (xlib.XGetWMHints)(display, xwindow);
            if existing.is_null() {
                (xlib.XAllocWMHints)()
            } else {
                existing
            }
        };

        if hints.is_null() {
            return;
        }

        (*hints).flags |= xlib::InputHint;
        (*hints).input = 0;
        (xlib.XSetWMHints)(display, xwindow, hints);

        let empty_region = (xfixes.XFixesCreateRegion)(display, std::ptr::null_mut(), 0);
        (xfixes.XFixesSetWindowShapeRegion)(display, xwindow, SHAPE_INPUT, 0, 0, empty_region);
        (xfixes.XFixesDestroyRegion)(display, empty_region);

        (xlib.XFlush)(display);
        (xlib.XFree)(hints as *mut c_void);
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_platform_overlay_window(_window: &Window) {}

#[cfg(target_os = "windows")]
fn finalize_overlay_window(window: &Window) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
        GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TRANSPARENT,
    };

    unsafe {
        let hwnd = window.hwnd() as HWND;
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let overlay_ex_style = (ex_style & !WS_EX_APPWINDOW)
            | WS_EX_LAYERED
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW
            | WS_EX_TRANSPARENT;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, overlay_ex_style as isize);
        SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);

        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn finalize_overlay_window(_window: &Window) {}
