use crate::config::{
    ModeOverlayPos, MODE_OVERLAY_POSITION, MODE_OVERLAY_THICKNESS_MONITOR_FRACTION,
};
use crate::state::{Mode, MonitorInfo};
#[cfg(target_os = "linux")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::{EventLoop, EventLoopBuilder};
#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS, WindowBuilderExtMacOS};
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
pub struct ModeOverlayState {
    pub visible: bool,
    pub mode: Mode,
    pub monitor: MonitorInfo,
}

pub struct ModeSurface {
    surface: softbuffer::Surface,
}

impl ModeSurface {
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
    .expect("failed to create window overlay");

    configure_overlay_window(&window);
    window
}

pub fn show_mode_overlay_window(window: &Window) {
    window.set_visible(true);
    finalize_overlay_window(window);
}

// Repaint the line for `overlay.mode` on `overlay.monitor`, then show or hide the window per
// `overlay.visible`. Painting a hidden overlay keeps the buffer current so a later show is
// a plain window-visibility change with nothing to redraw.
pub fn update_mode_overlay(
    window: &Window,
    mode_surface: &mut ModeSurface,
    overlay: &ModeOverlayState,
) -> Result<(), String> {
    let inner_size = sync_overlay_size(window, mode_surface, &overlay.monitor)?;

    let mut buffer = mode_surface
        .surface
        .buffer_mut()
        .map_err(|e| format!("softbuffer buffer_mut: {e:?}"))?;

    // The line is a single flat color, so the whole surface is one fill.
    // softbuffer uses 0RGB packed u32 (high byte ignored, then R, G, B).
    let [r, g, b, _] = overlay.mode.color();
    let pixel = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    let pixel_count = (inner_size.width * inner_size.height) as usize;
    for slot in buffer.iter_mut().take(pixel_count) {
        *slot = pixel;
    }

    buffer
        .present()
        .map_err(|e| format!("softbuffer present: {e:?}"))?;
    position_overlay(window, &overlay.monitor, inner_size);

    if overlay.visible {
        show_mode_overlay_window(window);
    } else {
        super::surface_overlay::hide_window_overlay(window);
    }
    Ok(())
}

impl ModeOverlayPos {
    // Horizontal lines span the monitor width; vertical lines span its height.
    fn is_horizontal(&self) -> bool {
        matches!(self, ModeOverlayPos::Top | ModeOverlayPos::Bottom)
    }
}

// Line extent along the monitor edge it runs on, in the same units as MonitorInfo (logical).
fn overlay_extent_for_monitor(monitor: &MonitorInfo) -> f64 {
    if MODE_OVERLAY_POSITION.is_horizontal() {
        monitor.width
    } else {
        monitor.height
    }
}

fn sync_overlay_size(
    window: &Window,
    mode_surface: &mut ModeSurface,
    monitor: &MonitorInfo,
) -> Result<PhysicalSize<u32>, String> {
    let inner_size = overlay_inner_size(monitor);

    if window.inner_size() != inner_size {
        window.set_visible(false);
        set_overlay_inner_size(window, monitor);
    }

    mode_surface
        .surface
        .resize(
            std::num::NonZeroU32::new(inner_size.width.max(1)).unwrap(),
            std::num::NonZeroU32::new(inner_size.height.max(1)).unwrap(),
        )
        .map_err(|e| format!("softbuffer resize: {e:?}"))?;

    Ok(inner_size)
}

// Both dimensions are physical pixels: the extent follows the monitor edge, while the
// thickness is a fraction of the monitor height so the line looks the same on any display.
fn line_physical_size(extent: u32, monitor_physical_height: f64) -> PhysicalSize<u32> {
    let thickness =
        ((monitor_physical_height * MODE_OVERLAY_THICKNESS_MONITOR_FRACTION).round() as u32).max(1);
    if MODE_OVERLAY_POSITION.is_horizontal() {
        PhysicalSize::new(extent.max(1), thickness)
    } else {
        PhysicalSize::new(thickness, extent.max(1))
    }
}

#[cfg(target_os = "macos")]
fn overlay_inner_size(monitor: &MonitorInfo) -> PhysicalSize<u32> {
    let extent = (overlay_extent_for_monitor(monitor) * monitor.scale_factor).round() as u32;
    line_physical_size(extent, monitor.height * monitor.scale_factor)
}

#[cfg(not(target_os = "macos"))]
fn overlay_inner_size(monitor: &MonitorInfo) -> PhysicalSize<u32> {
    line_physical_size(
        overlay_extent_for_monitor(monitor).round() as u32,
        monitor.height,
    )
}

#[cfg(target_os = "macos")]
fn set_overlay_inner_size(window: &Window, monitor: &MonitorInfo) {
    let size = overlay_inner_size(monitor);
    window.set_inner_size(LogicalSize::new(
        size.width as f64 / monitor.scale_factor,
        size.height as f64 / monitor.scale_factor,
    ));
}

#[cfg(not(target_os = "macos"))]
fn set_overlay_inner_size(window: &Window, monitor: &MonitorInfo) {
    window.set_inner_size(overlay_inner_size(monitor));
}

#[cfg(target_os = "macos")]
fn position_overlay(window: &Window, monitor: &MonitorInfo, inner_size: PhysicalSize<u32>) {
    let size = inner_size.to_logical::<f64>(monitor.scale_factor);
    let (x, y) = overlay_origin(monitor, size.width, size.height);
    window.set_outer_position(LogicalPosition::new(x, y));
}

#[cfg(not(target_os = "macos"))]
fn position_overlay(window: &Window, monitor: &MonitorInfo, inner_size: PhysicalSize<u32>) {
    let (x, y) = overlay_origin(monitor, inner_size.width as f64, inner_size.height as f64);
    window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

// Top-left corner of the line, flush against the configured monitor edge.
fn overlay_origin(monitor: &MonitorInfo, width: f64, height: f64) -> (f64, f64) {
    match MODE_OVERLAY_POSITION {
        ModeOverlayPos::Top => (monitor.origin.x, monitor.origin.y),
        ModeOverlayPos::Bottom => (monitor.origin.x, monitor.origin.y + monitor.height - height),
        ModeOverlayPos::Left => (monitor.origin.x, monitor.origin.y),
        ModeOverlayPos::Right => (monitor.origin.x + monitor.width - width, monitor.origin.y),
    }
}

#[cfg(target_os = "macos")]
fn configure_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_has_shadow(false)
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

fn configure_overlay_window(window: &Window) {
    configure_overlay_hittest(window);
    configure_platform_overlay_window(window);
}

#[cfg(not(target_os = "windows"))]
fn configure_overlay_hittest(window: &Window) {
    if let Err(error) = window.set_cursor_hittest(false) {
        eprintln!(
            "ViMouse: failed to make the mode overlay click-through ({error}) - it may intercept mouse clicks."
        );
    }
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

#[cfg(target_os = "macos")]
fn configure_platform_overlay_window(window: &Window) {
    super::raise_window_overlay_level(window);
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
