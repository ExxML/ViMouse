// Generic fullscreen transparent overlay surface, shared by the grid overlay and the
// mark overlay. It owns an always-on-top, click-through, monitor-sized window and the
// per-platform CPU-buffer compositing (Windows UpdateLayeredWindow, macOS CALayer
// contents, Linux XRender). The caller supplies a "fill" closure that writes the
// premultiplied pixels for its content; this module knows nothing about grids or marks.
//
// Lazy: the surface implementation (and its pixel cache) is created on first show, so an
// overlay that is never displayed costs nothing.

use crate::state::MonitorInfo;
#[cfg(target_os = "linux")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateRectRgn, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, SetWindowRgn, AC_SRC_ALPHA, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, UpdateLayeredWindow, GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_POPUP,
};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::EventLoop;
#[cfg(target_os = "macos")]
use winit::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
#[cfg(target_os = "windows")]
use winit::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
#[cfg(target_os = "linux")]
use winit::platform::x11::{WindowBuilderExtX11, WindowExtX11, XWindowType};
use winit::window::{Window, WindowBuilder, WindowLevel};
#[cfg(target_os = "linux")]
use x11_dl::xlib;
#[cfg(target_os = "linux")]
use x11_dl::xrender;

// Per-platform pixel-buffer element type passed to fill closures.
// Windows/Linux pack a pixel into one u32; macOS uses 4 separate bytes per pixel.
#[cfg(not(target_os = "macos"))]
pub type FillBuf = u32;
#[cfg(target_os = "macos")]
pub type FillBuf = u8;

// Per-platform pixel-buffer element type passed to fill closures.
pub struct OverlaySurface {
    imp: Option<OverlaySurfaceImp>,
    // Last monitor the window was sized/positioned for; None before first show.
    positioned_monitor: Option<MonitorInfo>,
}

impl OverlaySurface {
    pub fn new() -> Self {
        Self {
            imp: None,
            positioned_monitor: None,
        }
    }

    // Show (or hide) the overlay for `monitor`, painting `fill` if the content `version`
    // changed since the last paint. `version` is an opaque content hash supplied by the
    // caller: bump it whenever the pixels would differ (grid letters toggled, marks
    // added/removed, etc.). The surface rebuilds its cache on size or version change.
    pub fn update(
        &mut self,
        window: &Window,
        monitor: &MonitorInfo,
        visible: bool,
        version: u64,
        fill: impl FnOnce(&mut [FillBuf], usize, usize),
    ) {
        if !visible {
            window.set_visible(false);
            return;
        }
        let (w, h) = monitor_size_physical(monitor);
        if self.imp.is_none() {
            self.imp = Some(OverlaySurfaceImp::new(w, h));
        }
        // Skip redundant OS resize/reposition calls when the monitor hasn't changed.
        if self.positioned_monitor != Some(*monitor) {
            set_overlay_window_size(window, monitor, w, h);
            position_overlay_window(window, monitor);
            self.positioned_monitor = Some(*monitor);
        }
        self.imp
            .as_mut()
            .unwrap()
            .paint(window, w, h, version, fill);
        window.set_visible(true);
    }
}

impl Default for OverlaySurface {
    fn default() -> Self {
        Self::new()
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct OverlaySurfaceImp {
    // Pre-computed BGRA pixel cache. Empty until first paint; rebuilt on size or version change.
    pixel_cache: Vec<u32>,
    texture_size: (u32, u32),
    cached_version: u64,
    cache_valid: bool,
}

#[cfg(target_os = "windows")]
impl OverlaySurfaceImp {
    fn new(_w: u32, _h: u32) -> Self {
        Self {
            pixel_cache: Vec::new(),
            texture_size: (0, 0),
            cached_version: 0,
            cache_valid: false,
        }
    }

    fn paint(
        &mut self,
        window: &Window,
        w: u32,
        h: u32,
        version: u64,
        fill: impl FnOnce(&mut [u32], usize, usize),
    ) {
        let hwnd = window.hwnd() as HWND;

        // Build or rebuild cache when size or content version changes.
        if !self.cache_valid || self.texture_size != (w, h) || self.cached_version != version {
            let pixel_count = (w * h) as usize;
            self.pixel_cache = vec![0u32; pixel_count];
            fill(&mut self.pixel_cache, w as usize, h as usize);
            self.texture_size = (w, h);
            self.cached_version = version;
            self.cache_valid = true;
        }

        let pixel_count = (w * h) as usize;

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

            let dib_slice = std::slice::from_raw_parts_mut(dib_bits as *mut u32, pixel_count);
            dib_slice.copy_from_slice(&self.pixel_cache);

            let blend = BLENDFUNCTION {
                BlendOp: 0, // AC_SRC_OVER
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let outer = window.outer_position().unwrap_or_default();
            let pt_dst = POINT {
                x: outer.x,
                y: outer.y,
            };
            let sz = SIZE {
                cx: w as i32,
                cy: h as i32,
            };
            let pt_src = POINT { x: 0, y: 0 };

            UpdateLayeredWindow(
                hwnd, screen_dc, &pt_dst, &sz, mem_dc, &pt_src, 0, &blend, ULW_ALPHA,
            );

            // Force rectangular window region so DWM doesn't round the corners.
            let rgn = CreateRectRgn(0, 0, w as i32, h as i32);
            SetWindowRgn(hwnd, rgn, 0);

            SelectObject(mem_dc, old_bm);
            DeleteObject(hbm);
            DeleteDC(mem_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
    }
}

// ── macOS implementation (Core Graphics + CALayer, CPU pixel buffer) ─────────

#[cfg(target_os = "macos")]
struct OverlaySurfaceImp {
    pixel_cache: Vec<u8>,
    texture_size: (u32, u32),
    cached_version: u64,
    cache_valid: bool,
}

#[cfg(target_os = "macos")]
impl OverlaySurfaceImp {
    fn new(_w: u32, _h: u32) -> Self {
        Self {
            pixel_cache: Vec::new(),
            texture_size: (0, 0),
            cached_version: 0,
            cache_valid: false,
        }
    }

    fn paint(
        &mut self,
        window: &Window,
        w: u32,
        h: u32,
        version: u64,
        fill: impl FnOnce(&mut [u8], usize, usize),
    ) {
        use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaPremultipliedFirst};
        use core_graphics::color_space::CGColorSpace;
        use core_graphics::context::CGContext;

        if !self.cache_valid || self.texture_size != (w, h) || self.cached_version != version {
            self.pixel_cache = vec![0u8; (w * h * 4) as usize];
            fill(&mut self.pixel_cache, w as usize, h as usize);
            self.texture_size = (w, h);
            self.cached_version = version;
            self.cache_valid = true;
        }

        let color_space = CGColorSpace::create_device_rgb();
        // CGBitmapContext requires a mutable data pointer; we own pixel_cache so this is safe.
        let ctx = CGContext::create_bitmap_context(
            Some(self.pixel_cache.as_mut_ptr() as *mut std::ffi::c_void),
            w as usize,
            h as usize,
            8,
            (w * 4) as usize,
            &color_space,
            kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst,
        );
        let image = ctx
            .create_image()
            .expect("CGBitmapContextCreateImage failed");

        // Set the CGImage as the contents of the window's root CALayer.
        unsafe {
            use foreign_types_shared::ForeignType;
            use objc::runtime::Object;
            let ns_view = window.ns_view() as *mut Object;
            let layer: *mut Object = msg_send![ns_view, layer];
            if layer.is_null() {
                return;
            }
            let cg_image = image.as_ptr();
            let () = msg_send![layer, setContents: cg_image];
        }
    }
}

// ── Linux implementation (XRender ARGB32 pixmap, CPU pixel buffer) ───────────

#[cfg(target_os = "linux")]
struct OverlaySurfaceImp {
    pixel_cache: Vec<u32>,
    texture_size: (u32, u32),
    cached_version: u64,
    cache_valid: bool,
    logged_open_error: bool,
}

#[cfg(target_os = "linux")]
impl OverlaySurfaceImp {
    fn new(_w: u32, _h: u32) -> Self {
        Self {
            pixel_cache: Vec::new(),
            texture_size: (0, 0),
            cached_version: 0,
            cache_valid: false,
            logged_open_error: false,
        }
    }

    fn paint(
        &mut self,
        window: &Window,
        w: u32,
        h: u32,
        version: u64,
        fill: impl FnOnce(&mut [u32], usize, usize),
    ) {
        use std::mem::zeroed;

        if !self.cache_valid || self.texture_size != (w, h) || self.cached_version != version {
            self.pixel_cache = vec![0u32; (w * h) as usize];
            fill(&mut self.pixel_cache, w as usize, h as usize);
            self.texture_size = (w, h);
            self.cached_version = version;
            self.cache_valid = true;
        }

        let Some(display_ptr) = window.xlib_display() else {
            return;
        };
        let Some(xwindow) = window.xlib_window() else {
            return;
        };
        let Ok(xlib_api) = xlib::Xlib::open() else {
            if !self.logged_open_error {
                eprintln!(
                    "ViMouse: failed to load libX11 - grid and mark overlays will not render."
                );
                self.logged_open_error = true;
            }
            return;
        };
        let Ok(xrender_api) = xrender::Xrender::open() else {
            if !self.logged_open_error {
                eprintln!(
                    "ViMouse: failed to load libXrender - grid and mark overlays will not render. \
                     Install libXrender (e.g. libxrender1)."
                );
                self.logged_open_error = true;
            }
            return;
        };

        unsafe {
            let display = display_ptr as *mut xlib::Display;
            let screen = (xlib_api.XDefaultScreen)(display);

            // Find an ARGB32 visual for the pixmap.
            let mut vinfo: xlib::XVisualInfo = zeroed();
            let found =
                (xlib_api.XMatchVisualInfo)(display, screen, 32, xlib::TrueColor, &mut vinfo);
            if found == 0 {
                return;
            }

            // Create an ARGB32 pixmap and upload pixels via XImage.
            let pixmap = (xlib_api.XCreatePixmap)(display, xwindow, w, h, 32);

            let gc_values: xlib::XGCValues = zeroed();
            let gc = (xlib_api.XCreateGC)(display, pixmap, 0, &gc_values as *const _ as *mut _);

            // XCreateImage wraps our data pointer without taking ownership.
            // We null out ximage->data before XDestroyImage to prevent double-free.
            let data_ptr = self.pixel_cache.as_mut_ptr() as *mut std::ffi::c_char;
            let ximage = (xlib_api.XCreateImage)(
                display,
                vinfo.visual,
                32,
                xlib::ZPixmap,
                0,
                data_ptr,
                w,
                h,
                32,
                (w * 4) as i32,
            );
            if ximage.is_null() {
                (xlib_api.XFreeGC)(display, gc);
                (xlib_api.XFreePixmap)(display, pixmap);
                return;
            }

            (xlib_api.XPutImage)(display, pixmap, gc, ximage, 0, 0, 0, 0, w, h);

            (*ximage).data = std::ptr::null_mut();
            (xlib_api.XDestroyImage)(ximage);
            (xlib_api.XFreeGC)(display, gc);

            // XRender: composite pixmap onto the window using PictOpSrc.
            let argb_fmt =
                (xrender_api.XRenderFindStandardFormat)(display, xrender::PictStandardARGB32);
            if argb_fmt.is_null() {
                (xlib_api.XFreePixmap)(display, pixmap);
                return;
            }
            let win_fmt = (xrender_api.XRenderFindVisualFormat)(display, vinfo.visual);
            if win_fmt.is_null() {
                (xlib_api.XFreePixmap)(display, pixmap);
                return;
            }

            let pic_attrs: xrender::XRenderPictureAttributes = zeroed();
            let src_pic =
                (xrender_api.XRenderCreatePicture)(display, pixmap, argb_fmt, 0, &pic_attrs);
            let dst_pic =
                (xrender_api.XRenderCreatePicture)(display, xwindow, win_fmt, 0, &pic_attrs);

            (xrender_api.XRenderComposite)(
                display,
                xrender::PictOpSrc,
                src_pic,
                0, // no mask
                dst_pic,
                0,
                0,
                0,
                0,
                0,
                0,
                w,
                h,
            );

            (xrender_api.XRenderFreePicture)(display, src_pic);
            (xrender_api.XRenderFreePicture)(display, dst_pic);
            (xlib_api.XFreePixmap)(display, pixmap);
            (xlib_api.XFlush)(display);
        }
    }
}

// ── Window creation ───────────────────────────────────────────────────────────

// Owned windows are never shown in the taskbar by Windows, unlike ITaskbarList which requires the shell.
#[cfg(target_os = "windows")]
pub fn create_overlay_owner_hwnd() -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            windows_sys::w!("Static"),
            windows_sys::w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
}

// A fully transparent, click-through, always-on-top window spanning the primary monitor that stays
// visible for the whole process. It permanently occupies the top of the "always on top" band so
// the window manager keeps ViMouse's overlays (icon, grid, marks) above every other window,
// including ones that aggressively raise themselves such as the Windows taskbar. Invisible to the
// user, it counts as a visible topmost window to the window manager.
//
// Held by the caller for the process lifetime; dropping it destroys the anchor.
pub struct TopmostAnchor {
    // On Windows the anchor is a raw layered HWND (no event-loop integration needed). On other
    // platforms it is a winit window kept permanently visible.
    #[cfg(not(target_os = "windows"))]
    _window: Window,
}

#[cfg(target_os = "windows")]
pub fn create_topmost_anchor(_event_loop: &EventLoop<()>, _monitor: &MonitorInfo) -> TopmostAnchor {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        // Span the full virtual desktop so the anchor overlaps the taskbar. Alpha 0 keeps it
        // invisible and WS_EX_TRANSPARENT keeps it click-through.
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            windows_sys::w!("Static"),
            windows_sys::w!(""),
            WS_POPUP,
            x,
            y,
            w,
            h,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        if !hwnd.is_null() {
            SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
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
        TopmostAnchor {}
    }
}

#[cfg(not(target_os = "windows"))]
pub fn create_topmost_anchor(event_loop: &EventLoop<()>, monitor: &MonitorInfo) -> TopmostAnchor {
    // Built like a normal overlay window (transparent, always-on-top, click-through), then sized to
    // the primary monitor and left permanently visible so it always occupies the topmost band.
    let builder = WindowBuilder::new()
        .with_title("ViMouse Anchor")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));
    let builder = configure_overlay_window_builder(builder);
    let window = builder
        .build(event_loop)
        .expect("failed to create topmost anchor window");
    configure_overlay_surface_window(&window);
    let (w, h) = monitor_size_physical(monitor);
    set_overlay_window_size(&window, monitor, w, h);
    position_overlay_window(&window, monitor);
    window.set_visible(true);
    TopmostAnchor { _window: window }
}

#[cfg(target_os = "windows")]
pub fn create_overlay_window(event_loop: &EventLoop<()>, owner: HWND) -> Window {
    let builder = WindowBuilder::new()
        .with_title("ViMouse Overlay")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));

    let builder = configure_overlay_window_builder(builder, owner);
    let window = builder
        .build(event_loop)
        .expect("failed to create overlay window");
    configure_overlay_surface_window(&window);
    window
}

#[cfg(not(target_os = "windows"))]
pub fn create_overlay_window(event_loop: &EventLoop<()>) -> Window {
    let builder = WindowBuilder::new()
        .with_title("ViMouse Overlay")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));

    let builder = configure_overlay_window_builder(builder);
    let window = builder
        .build(event_loop)
        .expect("failed to create overlay window");
    configure_overlay_surface_window(&window);
    window
}

#[cfg(target_os = "macos")]
fn configure_overlay_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_has_shadow(false)
}

#[cfg(target_os = "windows")]
fn configure_overlay_window_builder(builder: WindowBuilder, owner: HWND) -> WindowBuilder {
    builder
        .with_skip_taskbar(true)
        .with_owner_window(owner as isize)
}

#[cfg(target_os = "linux")]
fn configure_overlay_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
        .with_override_redirect(true)
        .with_x11_window_type(vec![XWindowType::Notification])
}

fn configure_overlay_surface_window(window: &Window) {
    if let Err(error) = window.set_cursor_hittest(false) {
        eprintln!(
            "ViMouse: failed to make an overlay click-through ({error}) - it may intercept mouse clicks."
        );
    }
    platform_configure_overlay_window(window);
}

#[cfg(target_os = "windows")]
fn platform_configure_overlay_window(window: &Window) {
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

#[cfg(target_os = "linux")]
fn platform_configure_overlay_window(window: &Window) {
    let Some(display) = window.xlib_display() else {
        return;
    };
    let Some(xwindow) = window.xlib_window() else {
        return;
    };
    let Ok(xlib) = xlib::Xlib::open() else { return };
    unsafe {
        let display = display as *mut xlib::Display;
        let hints = {
            let p = (xlib.XGetWMHints)(display, xwindow);
            if p.is_null() {
                (xlib.XAllocWMHints)()
            } else {
                p
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
fn platform_configure_overlay_window(window: &Window) {
    unsafe {
        use objc::runtime::Object;
        let ns_window = window.ns_window() as *mut Object;
        // kCGDockWindowLevel is 20; use 21 so the overlay sits just above the dock but below the icon (102).
        let _: () = msg_send![ns_window, setLevel: 21i64];
    }
}

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
fn set_overlay_window_size(window: &Window, monitor: &MonitorInfo, _w: u32, _h: u32) {
    window.set_inner_size(LogicalSize::new(monitor.width, monitor.height));
}

#[cfg(not(target_os = "macos"))]
fn set_overlay_window_size(window: &Window, _monitor: &MonitorInfo, w: u32, h: u32) {
    window.set_inner_size(PhysicalSize::new(w, h));
}

#[cfg(target_os = "macos")]
fn position_overlay_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(LogicalPosition::new(monitor.origin.x, monitor.origin.y));
}

#[cfg(not(target_os = "macos"))]
fn position_overlay_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(PhysicalPosition::new(
        monitor.origin.x.round() as i32,
        monitor.origin.y.round() as i32,
    ));
}
