use crate::config::JUMP_GRID;
use crate::state::{MonitorInfo, Shared};
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder, WindowLevel};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
#[cfg(target_os = "linux")]
use winit::platform::x11::{WindowBuilderExtX11, WindowExtX11, XWindowType};
#[cfg(target_os = "windows")]
use winit::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

const GRID_COLS: usize = JUMP_GRID[0].len();
const GRID_ROWS: usize = JUMP_GRID.len();

const LINE_THICKNESS: usize = 1;

// Pre-multiplied ARGB for UpdateLayeredWindow on Windows (alpha=200 grey line)
const LINE_ALPHA: u8 = 200;
const LINE_GREY: u8 = 128;

#[derive(Clone, Debug, PartialEq)]
pub struct GridOverlayState {
    pub visible: bool,
    pub monitor: MonitorInfo,
}

pub fn current_grid_state(shared: &Shared) -> GridOverlayState {
    let state = shared.lock().expect("shared state poisoned");
    let monitor = state
        .monitors
        .get(state.selected_monitor)
        .copied()
        .expect("selected monitor out of bounds");
    GridOverlayState {
        visible: state.show_grid && state.mode == crate::state::Mode::Normal,
        monitor,
    }
}

// Per-platform grid surface state.
pub struct GridSurface {
    imp: GridSurfaceImp,
}

impl GridSurface {
    pub fn new(window: &Window) -> Self {
        Self {
            imp: GridSurfaceImp::new(window),
        }
    }

    pub fn update(&mut self, window: &Window, state: &GridOverlayState) {
        if !state.visible {
            window.set_visible(false);
            return;
        }
        position_grid_window(window, &state.monitor);
        let (w, h) = monitor_size_physical(&state.monitor);
        set_grid_window_size(window, &state.monitor, w, h);
        window.set_visible(true);
        self.imp.paint(window, w, h);
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct GridSurfaceImp;

#[cfg(target_os = "windows")]
impl GridSurfaceImp {
    fn new(_window: &Window) -> Self {
        Self
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32) {
        use std::ptr;
        use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
        use windows_sys::Win32::Graphics::Gdi::{
            AC_SRC_ALPHA, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BI_RGB,
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
            DIB_RGB_COLORS, GetDC, ReleaseDC, SelectObject,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

        let hwnd = window.hwnd() as HWND;

        let pixel_count = (w * h) as usize;
        let mut pixels: Vec<u32> = vec![0u32; pixel_count];
        fill_grid_argb_premult(&mut pixels, w as usize, h as usize);

        unsafe {
            let screen_dc = GetDC(ptr::null_mut());
            let mem_dc = CreateCompatibleDC(screen_dc);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [std::mem::zeroed()],
            };

            let mut dib_bits: *mut std::ffi::c_void = ptr::null_mut();
            let hbm = CreateDIBSection(
                mem_dc,
                &bmi,
                DIB_RGB_COLORS,
                &mut dib_bits,
                ptr::null_mut(),
                0,
            );
            if hbm.is_null() || dib_bits.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return;
            }

            let old_bm = SelectObject(mem_dc, hbm);

            // DIB stores BGRA in memory; our pixel value is pre-multiplied 0xAARRGGBB.
            // Rearrange to BGRA bytes: store as 0xAARRGGBB u32 in little-endian = B G R A.
            let dib_slice = std::slice::from_raw_parts_mut(dib_bits as *mut u32, pixel_count);
            for (dst, &src) in dib_slice.iter_mut().zip(pixels.iter()) {
                let a = (src >> 24) as u8;
                let r = (src >> 16) as u8;
                let g = (src >> 8) as u8;
                let b = src as u8;
                // Store BGRA: byte0=B, byte1=G, byte2=R, byte3=A
                *dst = (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | ((a as u32) << 24);
            }

            let blend = BLENDFUNCTION {
                BlendOp: 0, // AC_SRC_OVER
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let outer = window.outer_position().unwrap_or_default();
            let pt_dst = POINT { x: outer.x, y: outer.y };
            let sz = SIZE { cx: w as i32, cy: h as i32 };
            let pt_src = POINT { x: 0, y: 0 };

            UpdateLayeredWindow(
                hwnd,
                screen_dc,
                &pt_dst,
                &sz,
                mem_dc,
                &pt_src,
                0,
                &blend,
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old_bm);
            DeleteObject(hbm);
            DeleteDC(mem_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
    }
}

// Fill pre-multiplied ARGB pixels for the grid lines (Windows DIB order: 0xAARRGGBB).
#[cfg(target_os = "windows")]
fn fill_grid_argb_premult(pixels: &mut [u32], w: usize, h: usize) {
    // Pre-multiplied: R,G,B are multiplied by A/255.
    let pm = (LINE_GREY as u32 * LINE_ALPHA as u32) / 255;
    let line_pixel: u32 = ((LINE_ALPHA as u32) << 24) | (pm << 16) | (pm << 8) | pm;

    for col in 1..GRID_COLS {
        let x_center = col * w / GRID_COLS;
        let x_start = x_center.saturating_sub(LINE_THICKNESS / 2);
        let x_end = (x_start + LINE_THICKNESS).min(w);
        for y in 0..h {
            for x in x_start..x_end {
                pixels[y * w + x] = line_pixel;
            }
        }
    }

    for row in 1..GRID_ROWS {
        let y_center = row * h / GRID_ROWS;
        let y_start = y_center.saturating_sub(LINE_THICKNESS / 2);
        let y_end = (y_start + LINE_THICKNESS).min(h);
        for y in y_start..y_end {
            for x in 0..w {
                pixels[y * w + x] = line_pixel;
            }
        }
    }
}

// ── macOS / Linux implementation (pixels + wgpu with transparent window) ─────

#[cfg(not(target_os = "windows"))]
struct GridSurfaceImp {
    pixels: pixels::Pixels,
}

#[cfg(not(target_os = "windows"))]
impl GridSurfaceImp {
    fn new(window: &Window) -> Self {
        use pixels::{PixelsBuilder, SurfaceTexture};
        let size = window.inner_size();
        let w = size.width.max(1);
        let h = size.height.max(1);
        let surface = SurfaceTexture::new(w, h, window);
        let pixels = PixelsBuilder::new(w, h, surface)
            .build()
            .expect("grid pixels init failed");
        Self { pixels }
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32) {
        let cur = window.inner_size();
        if cur.width != w || cur.height != h {
            let _ = self.pixels.resize_buffer(w, h);
            let _ = self.pixels.resize_surface(w, h);
        }
        draw_grid_rgba(self.pixels.frame_mut(), w as usize, h as usize);
        if let Err(e) = self.pixels.render() {
            eprintln!("grid render error: {e}");
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn draw_grid_rgba(frame: &mut [u8], w: usize, h: usize) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[0, 0, 0, 0]);
    }
    for col in 1..GRID_COLS {
        let x_center = col * w / GRID_COLS;
        let x_start = x_center.saturating_sub(LINE_THICKNESS / 2);
        let x_end = (x_start + LINE_THICKNESS).min(w);
        for y in 0..h {
            for x in x_start..x_end {
                let i = (y * w + x) * 4;
                frame[i] = LINE_GREY;
                frame[i + 1] = LINE_GREY;
                frame[i + 2] = LINE_GREY;
                frame[i + 3] = LINE_ALPHA;
            }
        }
    }
    for row in 1..GRID_ROWS {
        let y_center = row * h / GRID_ROWS;
        let y_start = y_center.saturating_sub(LINE_THICKNESS / 2);
        let y_end = (y_start + LINE_THICKNESS).min(h);
        for y in y_start..y_end {
            for x in 0..w {
                let i = (y * w + x) * 4;
                frame[i] = LINE_GREY;
                frame[i + 1] = LINE_GREY;
                frame[i + 2] = LINE_GREY;
                frame[i + 3] = LINE_ALPHA;
            }
        }
    }
}

// ── Window creation ───────────────────────────────────────────────────────────

pub fn create_grid_window(event_loop: &EventLoop<()>) -> Window {
    let builder = WindowBuilder::new()
        .with_title("ViMouse Grid")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));

    let builder = configure_grid_window_builder(builder);
    let window = builder.build(event_loop).expect("failed to create grid window");
    configure_grid_overlay_window(&window);
    window
}

#[cfg(target_os = "windows")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_skip_taskbar(true)
}

#[cfg(target_os = "linux")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
        .with_override_redirect(true)
        .with_x11_window_type(vec![XWindowType::Notification])
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
}

fn configure_grid_overlay_window(window: &Window) {
    let _ = window.set_cursor_hittest(false);
    platform_configure_grid_window(window);
}

#[cfg(target_os = "windows")]
fn platform_configure_grid_window(window: &Window) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };
    unsafe {
        let hwnd = window.hwnd() as HWND;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ((ex & !WS_EX_APPWINDOW)
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW) as isize,
        );
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
}

#[cfg(target_os = "linux")]
fn platform_configure_grid_window(window: &Window) {
    use std::ffi::c_void;
    use x11_dl::xlib;
    let Some(display) = window.xlib_display() else { return };
    let Some(xwindow) = window.xlib_window() else { return };
    let Ok(xlib) = xlib::Xlib::open() else { return };
    unsafe {
        let display = display as *mut xlib::Display;
        let hints = {
            let p = (xlib.XGetWMHints)(display, xwindow);
            if p.is_null() { (xlib.XAllocWMHints)() } else { p }
        };
        if hints.is_null() { return; }
        (*hints).flags |= xlib::InputHint;
        (*hints).input = 0;
        (xlib.XSetWMHints)(display, xwindow, hints);
        (xlib.XFlush)(display);
        (xlib.XFree)(hints as *mut c_void);
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_configure_grid_window(_window: &Window) {}

// ── size / position helpers ───────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn monitor_size_physical(monitor: &MonitorInfo) -> (u32, u32) {
    let w = (monitor.width * monitor.scale_factor).round() as u32;
    let h = (monitor.height * monitor.scale_factor).round() as u32;
    (w.max(1), h.max(1))
}

#[cfg(not(target_os = "macos"))]
fn monitor_size_physical(monitor: &MonitorInfo) -> (u32, u32) {
    (monitor.width.round() as u32, monitor.height.round() as u32)
}

#[cfg(target_os = "macos")]
fn set_grid_window_size(window: &Window, monitor: &MonitorInfo, _w: u32, _h: u32) {
    window.set_inner_size(LogicalSize::new(monitor.width, monitor.height));
}

#[cfg(not(target_os = "macos"))]
fn set_grid_window_size(window: &Window, _monitor: &MonitorInfo, w: u32, h: u32) {
    window.set_inner_size(PhysicalSize::new(w, h));
}

#[cfg(target_os = "macos")]
fn position_grid_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(LogicalPosition::new(monitor.origin.x, monitor.origin.y));
}

#[cfg(not(target_os = "macos"))]
fn position_grid_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(PhysicalPosition::new(
        monitor.origin.x.round() as i32,
        monitor.origin.y.round() as i32,
    ));
}
