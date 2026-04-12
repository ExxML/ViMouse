#[cfg(target_os = "linux")]
pub fn current_cursor_position() -> Option<(f64, f64)> {
    use std::ptr;
    use x11_dl::xlib;

    let xlib = xlib::Xlib::open().ok()?;

    unsafe {
        let display = (xlib.XOpenDisplay)(ptr::null());
        if display.is_null() {
            return None;
        }

        let root = (xlib.XDefaultRootWindow)(display);
        let mut root_return: xlib::Window = 0;
        let mut child_return: xlib::Window = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask_return = 0;

        let status = (xlib.XQueryPointer)(
            display,
            root,
            &mut root_return,
            &mut child_return,
            &mut root_x,
            &mut root_y,
            &mut win_x,
            &mut win_y,
            &mut mask_return,
        );

        (xlib.XCloseDisplay)(display);

        if status == 0 {
            None
        } else {
            Some((root_x as f64, root_y as f64))
        }
    }
}

#[cfg(target_os = "macos")]
pub fn current_cursor_position() -> Option<(f64, f64)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let location = event.location();
    Some((location.x, location.y))
}

#[cfg(target_os = "windows")]
pub fn current_cursor_position() -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            None
        } else {
            Some((point.x as f64, point.y as f64))
        }
    }
}
