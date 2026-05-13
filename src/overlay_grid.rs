use crate::config::{
    GRID_ALPHA, GRID_BRIGHTNESS, GRID_LETTER_ALPHA, GRID_LETTER_BRIGHTNESS, GRID_LETTER_SIZE,
    JUMP_GRID,
};
use crate::state::MonitorInfo;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use rdev::Key;
#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, UpdateLayeredWindow,
    GWL_EXSTYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, ULW_ALPHA,
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

const GRID_COLS: usize = JUMP_GRID[0].len();
const GRID_ROWS: usize = JUMP_GRID.len();

const LINE_THICKNESS: usize = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct GridOverlayState {
    pub visible: bool,
    pub show_letters: bool,
    pub monitor: MonitorInfo,
}

// Per-platform grid surface state. The inner implementation is created lazily on first show
// to avoid allocating the pixel cache and GPU resources at startup.
pub struct GridSurface {
    imp: Option<GridSurfaceImp>,
    // Last monitor the window was sized/positioned for; None before first show.
    positioned_monitor: Option<MonitorInfo>,
}

impl GridSurface {
    pub fn new(_window: &Window, _initial_monitor: &MonitorInfo) -> Self {
        Self {
            imp: None,
            positioned_monitor: None,
        }
    }

    // prime() is kept for API compatibility but is now a no-op (initialization is lazy).
    pub fn prime(&mut self, _window: &Window, _monitor: &MonitorInfo) {}

    pub fn update(&mut self, window: &Window, state: &GridOverlayState) {
        if !state.visible {
            window.set_visible(false);
            return;
        }
        let (w, h) = monitor_size_physical(&state.monitor);
        // Create the surface implementation the first time the grid is shown.
        if self.imp.is_none() {
            self.imp = Some(GridSurfaceImp::new(window, w, h));
        }
        // Skip redundant OS resize/reposition calls when the monitor hasn't changed.
        if self.positioned_monitor != Some(state.monitor) {
            set_grid_window_size(window, &state.monitor, w, h);
            position_grid_window(window, &state.monitor);
            self.positioned_monitor = Some(state.monitor);
        }
        self.imp
            .as_mut()
            .unwrap()
            .paint(window, w, h, state.show_letters);
        window.set_visible(true);
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct GridSurfaceImp {
    // Pre-computed BGRA pixel cache. Empty until first paint; rebuilt on size or letters change.
    pixel_cache: Vec<u32>,
    texture_size: (u32, u32),
    cached_show_letters: bool,
}

#[cfg(target_os = "windows")]
impl GridSurfaceImp {
    fn new(_window: &Window, _w: u32, _h: u32) -> Self {
        Self {
            pixel_cache: Vec::new(),
            texture_size: (0, 0),
            cached_show_letters: false,
        }
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32, show_letters: bool) {
        let hwnd = window.hwnd() as HWND;

        // Build or rebuild cache when size or letter visibility changes.
        if self.texture_size != (w, h) || self.cached_show_letters != show_letters {
            let pixel_count = (w * h) as usize;
            self.pixel_cache = vec![0u32; pixel_count];
            fill_grid_bgra_premult(&mut self.pixel_cache, w as usize, h as usize, show_letters);
            self.texture_size = (w, h);
            self.cached_show_letters = show_letters;
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

            SelectObject(mem_dc, old_bm);
            DeleteObject(hbm);
            DeleteDC(mem_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
    }
}

fn axis_line_centers(length: usize, cells: usize) -> impl Iterator<Item = usize> {
    std::iter::once(0).chain(
        (1..cells)
            .map(move |i| i * length / cells)
            .chain(std::iter::once(length.saturating_sub(1))),
    )
}

fn line_range(center: usize, length: usize) -> std::ops::Range<usize> {
    let start = center.saturating_sub(LINE_THICKNESS / 2);
    let end = (start + LINE_THICKNESS).min(length);
    start..end
}

// Fill pre-multiplied BGRA pixels for UpdateLayeredWindow (DIB memory layout: B G R A bytes).
// Pre-multiplied: R,G,B are multiplied by A/255.
#[cfg(target_os = "windows")]
fn fill_grid_bgra_premult(pixels: &mut [u32], w: usize, h: usize, show_letters: bool) {
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
    }

    if show_letters {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            let cy = (row * h / GRID_ROWS) + (h / GRID_ROWS / 2);
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let cx = (col * w / GRID_COLS) + (w / GRID_COLS / 2);
                    blit_label_bgra_u32(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}

// ── macOS implementation (Core Graphics + CALayer, CPU pixel buffer) ─────────

#[cfg(target_os = "macos")]
struct GridSurfaceImp {
    pixel_cache: Vec<u8>,
    texture_size: (u32, u32),
    cached_show_letters: bool,
}

#[cfg(target_os = "macos")]
impl GridSurfaceImp {
    fn new(_window: &Window, w: u32, h: u32) -> Self {
        let mut pixel_cache = vec![0u8; (w * h * 4) as usize];
        fill_grid_premult_bgra(&mut pixel_cache, w as usize, h as usize, false);
        Self {
            pixel_cache,
            texture_size: (w, h),
            cached_show_letters: false,
        }
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32, show_letters: bool) {
        use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaPremultipliedFirst};
        use core_graphics::color_space::CGColorSpace;
        use core_graphics::context::CGContext;

        if self.texture_size != (w, h) || self.cached_show_letters != show_letters {
            self.pixel_cache = vec![0u8; (w * h * 4) as usize];
            fill_grid_premult_bgra(&mut self.pixel_cache, w as usize, h as usize, show_letters);
            self.texture_size = (w, h);
            self.cached_show_letters = show_letters;
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
struct GridSurfaceImp {
    pixel_cache: Vec<u32>,
    texture_size: (u32, u32),
    cached_show_letters: bool,
}

#[cfg(target_os = "linux")]
impl GridSurfaceImp {
    fn new(_window: &Window, w: u32, h: u32) -> Self {
        let mut pixel_cache = vec![0u32; (w * h) as usize];
        fill_grid_argb_premult(&mut pixel_cache, w as usize, h as usize, false);
        Self {
            pixel_cache,
            texture_size: (w, h),
            cached_show_letters: false,
        }
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32, show_letters: bool) {
        use std::mem::zeroed;

        if self.texture_size != (w, h) || self.cached_show_letters != show_letters {
            self.pixel_cache = vec![0u32; (w * h) as usize];
            fill_grid_argb_premult(&mut self.pixel_cache, w as usize, h as usize, show_letters);
            self.texture_size = (w, h);
            self.cached_show_letters = show_letters;
        }

        let Some(display_ptr) = window.xlib_display() else {
            return;
        };
        let Some(xwindow) = window.xlib_window() else {
            return;
        };
        let Ok(xlib_api) = xlib::Xlib::open() else {
            return;
        };
        let Ok(xrender_api) = xrender::Xrender::open() else {
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
                xrender::PictOpSrc as i32,
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

// Pre-multiplied BGRA for macOS CGBitmapContext (kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst).
// In memory: B G R A per pixel; as little-endian u32 = 0xAARRGGBB.
#[cfg(target_os = "macos")]
fn fill_grid_premult_bgra(pixels: &mut [u8], w: usize, h: usize, show_letters: bool) {
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
        for y_center in axis_line_centers(h, GRID_ROWS) {
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
    }

    if show_letters {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            let cy = (row * h / GRID_ROWS) + (h / GRID_ROWS / 2);
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let cx = (col * w / GRID_COLS) + (w / GRID_COLS / 2);
                    blit_label_bgra_u8(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}

// Pre-multiplied ARGB for Linux XRender (native-endian u32 = 0xAARRGGBB).
#[cfg(target_os = "linux")]
fn fill_grid_argb_premult(pixels: &mut [u32], w: usize, h: usize, show_letters: bool) {
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
    }

    if show_letters {
        for (row, keys) in JUMP_GRID.iter().enumerate() {
            let cy = (row * h / GRID_ROWS) + (h / GRID_ROWS / 2);
            for (col, key) in keys.iter().enumerate() {
                if let Some(ch) = key_label(*key) {
                    let cx = (col * w / GRID_COLS) + (w / GRID_COLS / 2);
                    blit_label_argb_u32(pixels, w, h, cx, cy, ch);
                }
            }
        }
    }
}

// ── Grid letter helpers ───────────────────────────────────────────────────────

fn key_label(key: Key) -> Option<char> {
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
        _ => None,
    }
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u32 BGRA premultiplied pixel buffer.
// pixel format: little-endian u32 where bytes are B G R A.
#[cfg(target_os = "windows")]
fn blit_label_bgra_u32(pixels: &mut [u32], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (GRID_LETTER_BRIGHTNESS as u32 * GRID_LETTER_ALPHA as u32) / 255;
    let pixel: u32 = pm | (pm << 8) | (pm << 16) | ((GRID_LETTER_ALPHA as u32) << 24);
    let s = GRID_LETTER_SIZE.max(1);
    let ox = cx.saturating_sub(4 * s);
    let oy = cy.saturating_sub(4 * s);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }
            for dy in 0..s {
                for dx in 0..s {
                    let px = ox + col * s + dx;
                    let py = oy + row * s + dy;
                    if px < w && py < h {
                        pixels[py * w + px] = pixel;
                    }
                }
            }
        }
    }
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u8×4 BGRA premultiplied pixel buffer.
// pixel format: bytes B G R A (macOS CGBitmapContext kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst).
#[cfg(target_os = "macos")]
fn blit_label_bgra_u8(pixels: &mut [u8], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (GRID_LETTER_BRIGHTNESS as u32 * GRID_LETTER_ALPHA as u32 / 255) as u8;
    let s = GRID_LETTER_SIZE.max(1);
    let ox = cx.saturating_sub(4 * s);
    let oy = cy.saturating_sub(4 * s);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }
            for dy in 0..s {
                for dx in 0..s {
                    let px = ox + col * s + dx;
                    let py = oy + row * s + dy;
                    if px < w && py < h {
                        let i = (py * w + px) * 4;
                        pixels[i] = pm;
                        pixels[i + 1] = pm;
                        pixels[i + 2] = pm;
                        pixels[i + 3] = GRID_LETTER_ALPHA;
                    }
                }
            }
        }
    }
}

// Blit an 8×8 font glyph centered at (cx, cy) into a u32 ARGB premultiplied pixel buffer.
// pixel format: native-endian u32 = 0xAARRGGBB.
#[cfg(target_os = "linux")]
fn blit_label_argb_u32(pixels: &mut [u32], w: usize, h: usize, cx: usize, cy: usize, ch: char) {
    let Some(glyph) = BASIC_FONTS.get(ch) else {
        return;
    };
    let pm = (GRID_LETTER_BRIGHTNESS as u32 * GRID_LETTER_ALPHA as u32) / 255;
    let pixel: u32 = ((GRID_LETTER_ALPHA as u32) << 24) | (pm << 16) | (pm << 8) | pm;
    let s = GRID_LETTER_SIZE.max(1);
    let ox = cx.saturating_sub(4 * s);
    let oy = cy.saturating_sub(4 * s);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }
            for dy in 0..s {
                for dx in 0..s {
                    let px = ox + col * s + dx;
                    let py = oy + row * s + dy;
                    if px < w && py < h {
                        pixels[py * w + px] = pixel;
                    }
                }
            }
        }
    }
}

// ── Window creation ───────────────────────────────────────────────────────────

// Owned windows are never shown in the taskbar by Windows, unlike ITaskbarList which requires the shell.
#[cfg(target_os = "windows")]
pub fn create_grid_owner_hwnd() -> HWND {
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

#[cfg(target_os = "windows")]
pub fn create_grid_window(event_loop: &EventLoop<()>, owner: HWND) -> Window {
    let builder = WindowBuilder::new()
        .with_title("ViMouse Grid")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));

    let builder = configure_grid_window_builder(builder, owner);
    let window = builder
        .build(event_loop)
        .expect("failed to create grid window");
    configure_grid_overlay_window(&window);
    window
}

#[cfg(not(target_os = "windows"))]
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
    let window = builder
        .build(event_loop)
        .expect("failed to create grid window");
    configure_grid_overlay_window(&window);
    window
}

#[cfg(target_os = "macos")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_has_shadow(false)
}

#[cfg(target_os = "windows")]
fn configure_grid_window_builder(builder: WindowBuilder, owner: HWND) -> WindowBuilder {
    builder
        .with_skip_taskbar(true)
        .with_owner_window(owner as isize)
}

#[cfg(target_os = "linux")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
        .with_override_redirect(true)
        .with_x11_window_type(vec![XWindowType::Notification])
}

fn configure_grid_overlay_window(window: &Window) {
    let _ = window.set_cursor_hittest(false);
    platform_configure_grid_window(window);
}

#[cfg(target_os = "windows")]
fn platform_configure_grid_window(window: &Window) {
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
fn platform_configure_grid_window(window: &Window) {
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
fn platform_configure_grid_window(window: &Window) {
    unsafe {
        use objc::runtime::Object;
        let ns_window = window.ns_window() as *mut Object;
        // NSPopUpMenuWindowLevel (101) sits above the dock (level 20) and most system UI.
        let _: () = msg_send![ns_window, setLevel: 101i64];
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
