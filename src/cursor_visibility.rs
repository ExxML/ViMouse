// System-wide cursor hiding used by Insert mode (restored in Normal mode).
// Each platform uses its own mechanism; see the per-platform apply() for specifics.

use std::sync::atomic::{AtomicBool, Ordering};

static HIDDEN: AtomicBool = AtomicBool::new(false);

pub fn set_cursor_hidden(hidden: bool) {
    if HIDDEN.swap(hidden, Ordering::SeqCst) == hidden {
        return;
    }
    platform::apply(hidden);
}

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::display::CGDisplay;
    use std::ffi::c_void;
    use std::sync::Once;

    type CGSConnectionID = u32;
    extern "C" {
        fn CGSMainConnectionID() -> CGSConnectionID;
        fn CGSSetConnectionProperty(
            cid: CGSConnectionID,
            target_cid: CGSConnectionID,
            key: CFStringRef,
            value: *const c_void,
        ) -> i32;
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> core_graphics::geometry::CGPoint;
        fn CGWarpMouseCursorPosition(point: core_graphics::geometry::CGPoint) -> i32;
        fn CFRelease(cf: *mut c_void);
    }

    // CGDisplayShowCursor marks the cursor visible but the window server only repaints it on the
    // next cursor-position change, so it stays hidden until the mouse moves. Warping to the current
    // location forces that repaint without moving the cursor.
    fn nudge_cursor_repaint() {
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event);
            CGWarpMouseCursorPosition(point);
        }
    }

    // A hovered text field keeps re-showing the I-beam via its cursor-rect, which decrements the
    // global hide count and undoes CGDisplayHideCursor. Marking our connection as one that "sets
    // cursor in background" makes its hide win over other apps' cursor-rect resets.
    fn enable_background_cursor_control() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| unsafe {
            let key = CFString::new("SetsCursorInBackground");
            let cid = CGSMainConnectionID();
            CGSSetConnectionProperty(
                cid,
                cid,
                key.as_concrete_TypeRef(),
                CFBoolean::true_value().as_CFTypeRef(),
            );
        });
    }

    // CGDisplayHideCursor/ShowCursor are reference-counted; the swap-guard in set_cursor_hidden
    // keeps our hide/show calls balanced so the count never drifts.
    pub fn apply(hidden: bool) {
        let display = CGDisplay::main();
        if hidden {
            enable_background_cursor_control();
            let _ = display.hide_cursor();
        } else {
            let _ = display.show_cursor();
            nudge_cursor_repaint();
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateCursor, SetSystemCursor, SystemParametersInfoW, OCR_APPSTARTING, OCR_CROSS, OCR_HAND,
        OCR_HELP, OCR_IBEAM, OCR_NO, OCR_NORMAL, OCR_SIZEALL, OCR_SIZENESW, OCR_SIZENS,
        OCR_SIZENWSE, OCR_SIZEWE, OCR_UP, OCR_WAIT, SPIF_SENDCHANGE, SPI_SETCURSORS,
        SYSTEM_CURSOR_ID,
    };

    const SYSTEM_CURSORS: [SYSTEM_CURSOR_ID; 14] = [
        OCR_APPSTARTING,
        OCR_NORMAL,
        OCR_CROSS,
        OCR_HAND,
        OCR_HELP,
        OCR_IBEAM,
        OCR_NO,
        OCR_SIZEALL,
        OCR_SIZENESW,
        OCR_SIZENS,
        OCR_SIZENWSE,
        OCR_SIZEWE,
        OCR_UP,
        OCR_WAIT,
    ];

    // Windows offers no global cursor-hide, so each system cursor is swapped for a fully
    // transparent one and restored by reloading the defaults from the registry.
    pub fn apply(hidden: bool) {
        unsafe {
            if hidden {
                for id in SYSTEM_CURSORS {
                    if let Some(blank) = create_blank_cursor() {
                        // SetSystemCursor takes ownership of the handle, so a fresh one per id.
                        SetSystemCursor(blank, id);
                    }
                }
            } else {
                SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), SPIF_SENDCHANGE);
            }
        }
    }

    // A 32x32 cursor whose AND mask is all-ones and XOR mask all-zeros draws nothing.
    unsafe fn create_blank_cursor() -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HCURSOR>
    {
        const SIDE: i32 = 32;
        const BYTES: usize = (SIDE * SIDE / 8) as usize;
        let and_mask = [0xFFu8; BYTES];
        let xor_mask = [0x00u8; BYTES];
        let cursor = CreateCursor(
            std::ptr::null_mut(),
            0,
            0,
            SIDE,
            SIDE,
            and_mask.as_ptr() as *const _,
            xor_mask.as_ptr() as *const _,
        );
        if cursor.is_null() {
            None
        } else {
            Some(cursor)
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::ptr;
    use std::sync::{Mutex, OnceLock};
    use x11_dl::xfixes::Xlib as Xfixes;
    use x11_dl::xlib::{Display, Xlib};

    struct Connection {
        xlib: Xlib,
        xfixes: Xfixes,
        display: *mut Display,
    }
    // The display pointer is only ever touched under the Mutex below, one thread at a time.
    unsafe impl Send for Connection {}

    fn connection() -> &'static Mutex<Option<Connection>> {
        static CONN: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
        CONN.get_or_init(|| {
            let conn = (|| {
                let (xlib, xfixes) = (Xlib::open().ok()?, Xfixes::open().ok()?);
                let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
                (!display.is_null()).then_some(Connection {
                    xlib,
                    xfixes,
                    display,
                })
            })();
            Mutex::new(conn)
        })
    }

    // XFixes deletes a client's hide requests when it disconnects, so the connection is held
    // open for the whole hidden duration rather than closed per toggle - otherwise the hide is
    // released instantly. If the process dies the X server drops the client and restores the
    // cursor automatically.
    pub fn apply(hidden: bool) {
        let Ok(guard) = connection().lock() else {
            return;
        };
        let Some(conn) = guard.as_ref() else {
            return;
        };
        unsafe {
            let root = (conn.xlib.XDefaultRootWindow)(conn.display);
            if hidden {
                (conn.xfixes.XFixesHideCursor)(conn.display, root);
            } else {
                (conn.xfixes.XFixesShowCursor)(conn.display, root);
            }
            (conn.xlib.XFlush)(conn.display);
        }
    }
}
