use crate::config::{OverlayIconPos, OVERLAY_ICON_POSITION, OVERLAY_ICON_SIZE_MONITOR_FRACTION};
use crate::state::{Mode, MonitorInfo};
use font8x8::{UnicodeFonts, BASIC_FONTS};
#[cfg(target_os = "linux")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
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
#[cfg(target_os = "linux")]
use x11_dl::xlib;

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayIconState {
    pub mode: Mode,
    pub monitor: MonitorInfo,
}

pub struct IconSurface {
    surface: softbuffer::Surface,
}

impl IconSurface {
    pub fn new(window: &Window) -> Self {
        let context = unsafe { softbuffer::Context::new(window) }
            .expect("softbuffer context creation failed");
        let surface = unsafe { softbuffer::Surface::new(&context, window) }
            .expect("softbuffer surface creation failed");
        Self { surface }
    }
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
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(PhysicalSize::new(1u32, 1u32)),
    )
    .build(event_loop)
    .expect("failed to create overlay window");

    configure_overlay_window(&window);
    window
}

pub fn show_overlay_icon_window(window: &Window) {
    window.set_visible(true);
    finalize_overlay_window(window);
}

pub fn paint_overlay_icon(
    window: &Window,
    icon_surface: &mut IconSurface,
    overlay: &OverlayIconState,
) -> Result<(), String> {
    let inner_size = sync_overlay_size(window, icon_surface, &overlay.monitor)?;
    let (w, h) = (inner_size.width, inner_size.height);

    let mut buffer = icon_surface
        .surface
        .buffer_mut()
        .map_err(|e| format!("softbuffer buffer_mut: {e:?}"))?;

    let frame_len = (w * h) as usize;
    let mut frame: Vec<u8> = vec![0u8; frame_len * 4];
    draw_overlay(&mut frame, overlay.mode, w as usize);

    // softbuffer uses 0RGB packed u32 (high byte ignored, then R, G, B)
    for (i, pixel) in buffer.iter_mut().take(frame_len).enumerate() {
        let r = frame[i * 4] as u32;
        let g = frame[i * 4 + 1] as u32;
        let b = frame[i * 4 + 2] as u32;
        *pixel = (r << 16) | (g << 8) | b;
    }

    buffer.present().map_err(|e| format!("softbuffer present: {e:?}"))?;
    position_overlay(window, &overlay.monitor, inner_size);
    Ok(())
}

fn overlay_size_for_monitor(monitor: MonitorInfo) -> u32 {
    (monitor.width.min(monitor.height) * OVERLAY_ICON_SIZE_MONITOR_FRACTION)
        .round()
        .max(1.0) as u32
}

fn sync_overlay_size(
    window: &Window,
    icon_surface: &mut IconSurface,
    monitor: &MonitorInfo,
) -> Result<PhysicalSize<u32>, String> {
    let overlay_size = overlay_size_for_monitor(*monitor);
    let inner_size = overlay_inner_size(monitor, overlay_size);
    let (w, h) = (inner_size.width, inner_size.height);

    if window.inner_size() != inner_size {
        window.set_visible(false);
        set_overlay_inner_size(window, monitor, overlay_size);
    }

    icon_surface
        .surface
        .resize(
            std::num::NonZeroU32::new(w.max(1)).unwrap(),
            std::num::NonZeroU32::new(h.max(1)).unwrap(),
        )
        .map_err(|e| format!("softbuffer resize: {e:?}"))?;

    Ok(inner_size)
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
fn position_overlay(window: &Window, monitor: &MonitorInfo, inner_size: PhysicalSize<u32>) {
    let overlay_size = inner_size.to_logical::<f64>(monitor.scale_factor);
    let x = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::BottomLeft => monitor.origin.x,
        OverlayIconPos::TopRight | OverlayIconPos::BottomRight => {
            monitor.origin.x + monitor.width - overlay_size.width
        }
    };
    let y = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::TopRight => monitor.origin.y,
        OverlayIconPos::BottomLeft | OverlayIconPos::BottomRight => {
            monitor.origin.y + monitor.height - overlay_size.height
        }
    };
    window.set_outer_position(LogicalPosition::new(x, y));
}

#[cfg(not(target_os = "macos"))]
fn position_overlay(window: &Window, monitor: &MonitorInfo, inner_size: PhysicalSize<u32>) {
    let x = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::BottomLeft => monitor.origin.x,
        OverlayIconPos::TopRight | OverlayIconPos::BottomRight => {
            monitor.origin.x + monitor.width - inner_size.width as f64
        }
    };
    let y = match OVERLAY_ICON_POSITION {
        OverlayIconPos::TopLeft | OverlayIconPos::TopRight => monitor.origin.y,
        OverlayIconPos::BottomLeft | OverlayIconPos::BottomRight => {
            monitor.origin.y + monitor.height - inner_size.height as f64
        }
    };
    window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn draw_overlay(frame: &mut [u8], mode: Mode, overlay_size: usize) {
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

#[cfg(not(target_os = "windows"))]
fn configure_overlay_hittest(window: &Window) {
    let _ = window.set_cursor_hittest(false);
}

#[cfg(target_os = "windows")]
fn configure_overlay_hittest(_window: &Window) {}

#[cfg(target_os = "linux")]
fn configure_platform_overlay_window(window: &Window) {
    let Some(display) = window.xlib_display() else {
        return;
    };
    let Some(xwindow) = window.xlib_window() else {
        return;
    };
    let Ok(xlib) = xlib::Xlib::open() else {
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
        (xlib.XFlush)(display);
        (xlib.XFree)(hints as *mut c_void);
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_platform_overlay_window(_window: &Window) {}

#[cfg(target_os = "windows")]
fn finalize_overlay_window(window: &Window) {
    unsafe {
        let hwnd = window.hwnd() as HWND;
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let overlay_ex_style = (ex_style & !WS_EX_APPWINDOW) | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, overlay_ex_style as isize);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn finalize_overlay_window(_window: &Window) {}

#[cfg(target_os = "windows")]
pub fn reassert_topmost(window: &Window) {
    unsafe {
        let hwnd = window.hwnd() as HWND;
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn reassert_topmost(_window: &Window) {}
