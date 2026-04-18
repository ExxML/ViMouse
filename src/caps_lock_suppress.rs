// Windows/Linux only. Disables the Caps Lock toggle state and LED for the duration of the app so
// the key acts purely as a ViMouse trigger with no side effects. Restored on exit or panic.
use std::sync::atomic::{AtomicBool, Ordering};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub fn caps_lock_used_in_config() -> bool {
    use crate::config::*;
    use rdev::Key;
    [
        KEY_NORMAL_MODE,
        KEY_INSERT_MODE,
        KEY_SCROLL,
        KEY_FAST,
        KEY_SLOW,
        KEY_LEFT_CLICK,
        KEY_RIGHT_CLICK,
        KEY_CYCLE_MONITOR,
        KEY_MOVE_LEFT,
        KEY_MOVE_DOWN,
        KEY_MOVE_UP,
        KEY_MOVE_RIGHT,
    ]
    .contains(&Key::CapsLock)
        || KEYS_QUIT.contains(&Key::CapsLock)
        || JUMP_GRID.iter().flatten().any(|k| *k == Key::CapsLock)
}

pub fn suppress() {
    if !caps_lock_used_in_config() || SUPPRESSED.load(Ordering::Acquire) {
        return;
    }
    if platform_suppress() {
        SUPPRESSED.store(true, Ordering::Release);
    }
}

pub fn restore() {
    if !SUPPRESSED.swap(false, Ordering::AcqRel) {
        return;
    }
    platform_restore();
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

#[cfg(target_os = "windows")]
fn platform_restore() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, KEYEVENTF_KEYUP, VK_CAPITAL, keybd_event,
    };
    unsafe {
        // If Caps Lock is currently off, toggle it back on.
        if GetKeyState(VK_CAPITAL as i32) & 0x0001 == 0 {
            keybd_event(VK_CAPITAL as u8, 0x3A, 0, 0);
            keybd_event(VK_CAPITAL as u8, 0x3A, KEYEVENTF_KEYUP, 0);
        }
    }
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
fn platform_restore() {
    with_display(|xlib, display| unsafe {
        (xlib.XkbLockModifiers)(display, 0x100, 0x2, 0x2);
        (xlib.XFlush)(display);
    });
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
