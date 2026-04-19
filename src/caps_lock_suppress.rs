// Windows/Linux only. Disables the Caps Lock toggle state and LED on launch so
// the key acts purely as a ViMouse trigger with no side effects. Re-enabled on exit or panic.
use std::sync::atomic::{AtomicBool, Ordering};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub use crate::input::caps_lock_used_in_config;

pub fn suppress() {
    if !caps_lock_used_in_config() || SUPPRESSED.load(Ordering::Acquire) {
        return;
    }
    if platform_suppress() {
        SUPPRESSED.store(true, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn platform_suppress() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, KEYEVENTF_KEYUP, VK_CAPITAL, keybd_event,
    };
    unsafe {
        // If Caps Lock is currently on, send a key press+release to toggle it off.
        if GetKeyState(VK_CAPITAL as i32) & 0x0001 != 0 {
            keybd_event(VK_CAPITAL as u8, 0x3A, 0, 0);
            keybd_event(VK_CAPITAL as u8, 0x3A, KEYEVENTF_KEYUP, 0);
        }
    }
    true
}

#[cfg(target_os = "linux")]
fn platform_suppress() -> bool {
    with_display(|xlib, display| unsafe {
        // XkbUseCoreKbd = 0x100, LockMask = 0x2
        (xlib.XkbLockModifiers)(display, 0x100, 0x2, 0);
        (xlib.XFlush)(display);
    })
}

#[cfg(target_os = "linux")]
fn with_display<F: FnOnce(&x11_dl::xlib::Xlib, *mut x11_dl::xlib::Display)>(f: F) -> bool {
    use std::ptr;
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
        return false;
    };
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        return false;
    }
    f(&xlib, display);
    unsafe {
        (xlib.XCloseDisplay)(display);
    }
    true
}
