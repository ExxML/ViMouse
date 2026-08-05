use crate::state::Action;
#[cfg(target_os = "macos")]
use crate::state::Point;
#[cfg(target_os = "macos")]
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use rdev::Button;
use rdev::{simulate, EventType};
#[cfg(target_os = "linux")]
use std::os::raw::{c_int, c_uint, c_ulong};
#[cfg(target_os = "linux")]
use std::ptr;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_LBUTTON,
    VK_RBUTTON,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
#[cfg(target_os = "linux")]
use x11_dl::xlib::Xlib;

// Platform-specific button codes for mouse buttons 4 and 5 (back/forward)
#[cfg(target_os = "windows")]
pub const BUTTON_MOUSE_4: Button = Button::Unknown(1);
#[cfg(target_os = "windows")]
pub const BUTTON_MOUSE_5: Button = Button::Unknown(2);
#[cfg(target_os = "linux")]
pub const BUTTON_MOUSE_4: Button = Button::Unknown(8);
#[cfg(target_os = "linux")]
pub const BUTTON_MOUSE_5: Button = Button::Unknown(9);
#[cfg(target_os = "macos")]
pub const BUTTON_MOUSE_4: Button = Button::Unknown(3);
#[cfg(target_os = "macos")]
pub const BUTTON_MOUSE_5: Button = Button::Unknown(4);

static SIMULATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn simulate_input(event_type: &EventType) -> Result<(), String> {
    let _guard = SIMULATE_LOCK.lock().expect("simulate lock poisoned");
    simulate(event_type).map_err(|_| "rdev input simulation failed".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn action_to_event_type(action: &Action) -> EventType {
    match action {
        Action::MouseMove(point) => EventType::MouseMove {
            x: point.x,
            y: point.y,
        },
        Action::Scroll { delta_x, delta_y } => EventType::Wheel {
            delta_x: delta_x.round() as i64,
            delta_y: delta_y.round() as i64,
        },
        Action::ButtonPress(button) => EventType::ButtonPress(*button),
        Action::ButtonRelease(button) => EventType::ButtonRelease(*button),
    }
}

#[cfg(target_os = "macos")]
pub fn set_caps_lock_remap(enabled: bool) {
    macos_grab::set_caps_lock_remap_enabled(enabled);
}

pub fn shutdown_platform_input() {
    // Restore the cursor if it was hidden in Insert mode
    crate::cursor_visibility::set_cursor_hidden(false);
    #[cfg(target_os = "macos")]
    macos_grab::shutdown();
}

/// Returns true if the given mouse button is physically held down according to the OS.
/// Returns false on platforms where this cannot be determined cheaply.
pub fn mouse_button_is_down(button: rdev::Button) -> bool {
    #[cfg(target_os = "windows")]
    {
        let vk = match button {
            rdev::Button::Left => VK_LBUTTON,
            rdev::Button::Right => VK_RBUTTON,
            _ => return false,
        };
        // Bit 15 set means the key is currently down.
        unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
    }

    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceButtonState(state_id: CGEventSourceStateID, button: u32) -> bool;
        }
        let cg_button: u32 = match button {
            rdev::Button::Left => 0,
            rdev::Button::Right => 1,
            _ => return false,
        };
        unsafe { CGEventSourceButtonState(CGEventSourceStateID::CombinedSessionState, cg_button) }
    }

    #[cfg(target_os = "linux")]
    {
        let mask: c_uint = match button {
            rdev::Button::Left => 1 << 8,   // Button1Mask
            rdev::Button::Right => 1 << 10, // Button3Mask
            _ => return false,
        };

        let Ok(xlib) = Xlib::open() else { return false };
        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            return false;
        }
        let mut mask_return: c_uint = 0;
        unsafe {
            let root = (xlib.XDefaultRootWindow)(display);
            let (mut _a, mut _b) = (0 as c_ulong, 0 as c_ulong);
            let (mut _c, mut _d, mut _e, mut _f) = (0 as c_int, 0, 0, 0);
            (xlib.XQueryPointer)(
                display,
                root,
                &mut _a,
                &mut _b,
                &mut _c,
                &mut _d,
                &mut _e,
                &mut _f,
                &mut mask_return,
            );
            (xlib.XCloseDisplay)(display);
        }
        mask_return & mask != 0
    }
}

/// Sign multipliers `(x, y)` applied to ViMouse's scroll deltas so a given key always scrolls the
/// same physical direction regardless of the OS scroll-direction setting. Cached on launch.
pub fn scroll_direction_sign() -> (f64, f64) {
    use std::sync::OnceLock;
    static SIGN: OnceLock<(f64, f64)> = OnceLock::new();
    *SIGN.get_or_init(detect_scroll_direction_sign)
}

#[cfg(target_os = "windows")]
fn detect_scroll_direction_sign() -> (f64, f64) {
    // ReverseMouseWheelDirection inverts both synthetic wheels, so both axes track it (with
    // opposite base polarity) to keep Shift+H left / Shift+J down in either setting.
    let reversed = read_reverse_wheel_registry().unwrap_or(false);
    let sign_x = if reversed { 1.0 } else { -1.0 };
    let sign_y = if reversed { -1.0 } else { 1.0 };
    (sign_x, sign_y)
}

#[cfg(target_os = "windows")]
fn read_reverse_wheel_registry() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    // HKCU\Control Panel\Mouse\ReverseMouseWheelDirection, a REG_DWORD: 1 => reverse on, 0 => off.
    let subkey: Vec<u16> = "Control Panel\\Mouse\0".encode_utf16().collect();
    let value: Vec<u16> = "ReverseMouseWheelDirection\0".encode_utf16().collect();
    unsafe {
        let mut hkey = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let mut kind = 0u32;
        let mut data = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32; // bytes
        let status = RegQueryValueExW(
            hkey,
            value.as_ptr(),
            std::ptr::null(),
            &mut kind,
            &mut data as *mut u32 as *mut u8,
            &mut len,
        );
        RegCloseKey(hkey);
        if status != 0 || kind != REG_DWORD {
            return None;
        }
        Some(data != 0)
    }
}

#[cfg(not(target_os = "windows"))]
fn detect_scroll_direction_sign() -> (f64, f64) {
    // No compensation on macOS or Linux: their synthetic scroll events bypass the OS
    // reverse/natural-scroll setting, so ViMouse's direction is already consistent.
    (1.0, 1.0)
}

/// Scales a logical-point movement delta into the monitor's device space, keeping apparent speed
/// DPI-constant. Windows/Linux cursor APIs take physical pixels (scale by `scale_factor`); macOS's
/// event-tap space is already logical points (unscaled). On Windows this relies on winit making the
/// process per-monitor-DPI-aware, so cursor/monitor coords are true physical pixels.
#[cfg(not(target_os = "macos"))]
pub fn movement_device_scale(scale_factor: f64) -> f64 {
    scale_factor
}

#[cfg(target_os = "macos")]
pub fn movement_device_scale(_scale_factor: f64) -> f64 {
    1.0
}

// macOS event suppression and simulation works differently than Windows or Linux
// therefore, we use a custom event tap on macOS instead of rdev's built-in grab/simulate functionality
// otherwise a "Trace/BPT trap: 5" error is thrown when emitting synthetic key events
#[cfg(target_os = "macos")]
pub mod macos_grab {
    use crate::caps_lock_remap;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use core_graphics::event::{CGEventFlags, CGEventTapProxy, CGEventType};
    use rdev::{Button, Event, EventType, Key};
    use std::os::raw::c_void;
    use std::sync::atomic::Ordering;
    use std::time::SystemTime;

    type GrabCallback = Box<dyn FnMut(Event) -> Option<Event> + Send>;

    const KEYCODE_FIELD: u32 = 9;
    const SCROLL_DELTA_Y_FIELD: u32 = 96;
    const SCROLL_DELTA_X_FIELD: u32 = 97;

    static mut CALLBACK: Option<GrabCallback> = None;
    static mut TAP_REF: *mut c_void = std::ptr::null_mut();
    static mut CAPS_LOCK_KEY_DOWN: bool = false;

    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events: u64,
            callback: unsafe extern "C" fn(
                CGEventTapProxy,
                CGEventType,
                *mut c_void,
                *const c_void,
            ) -> *mut c_void,
            info: *const c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: *mut c_void,
            order: isize,
        ) -> *mut c_void;
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    unsafe extern "C" fn tap_callback(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        raw_event: *mut c_void,
        _info: *const c_void,
    ) -> *mut c_void {
        match event_type {
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                CGEventTapEnable(TAP_REF, true);
                return raw_event;
            }
            _ => {}
        }

        let Some(event) = to_rdev_event(event_type, raw_event) else {
            return raw_event;
        };

        #[allow(static_mut_refs)]
        if let Some(callback) = CALLBACK.as_mut() {
            if callback(event).is_none() {
                // Returning NULL is the safe way to suppress a macOS event-tap event.
                return std::ptr::null_mut();
            }
        }

        raw_event
    }

    unsafe fn to_rdev_event(event_type: CGEventType, raw_event: *mut c_void) -> Option<Event> {
        let event_type = match event_type {
            CGEventType::LeftMouseDown => EventType::ButtonPress(Button::Left),
            CGEventType::LeftMouseUp => EventType::ButtonRelease(Button::Left),
            CGEventType::RightMouseDown => EventType::ButtonPress(Button::Right),
            CGEventType::RightMouseUp => EventType::ButtonRelease(Button::Right),
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged => {
                let point = CGEventGetLocation(raw_event);
                EventType::MouseMove {
                    x: point.x,
                    y: point.y,
                }
            }
            CGEventType::KeyDown => {
                let code = CGEventGetIntegerValueField(raw_event, KEYCODE_FIELD) as u16;
                EventType::KeyPress(key_from_code(code))
            }
            CGEventType::KeyUp => {
                let code = CGEventGetIntegerValueField(raw_event, KEYCODE_FIELD) as u16;
                EventType::KeyRelease(key_from_code(code))
            }
            CGEventType::FlagsChanged => {
                let code = CGEventGetIntegerValueField(raw_event, KEYCODE_FIELD) as u16;
                let is_press = if code == 57 {
                    // Caps Lock toggles state on each physical press; track manually.
                    let was_down = CAPS_LOCK_KEY_DOWN;
                    CAPS_LOCK_KEY_DOWN = !was_down;
                    !was_down
                } else {
                    modifier_flag_active(code, CGEventGetFlags(raw_event))
                };

                if is_press {
                    EventType::KeyPress(key_from_code(code))
                } else {
                    EventType::KeyRelease(key_from_code(code))
                }
            }
            CGEventType::ScrollWheel => {
                let delta_y = CGEventGetIntegerValueField(raw_event, SCROLL_DELTA_Y_FIELD);
                let delta_x = CGEventGetIntegerValueField(raw_event, SCROLL_DELTA_X_FIELD);
                EventType::Wheel { delta_x, delta_y }
            }
            _ => return None,
        };

        Some(Event {
            event_type,
            time: SystemTime::now(),
            name: None,
        })
    }

    fn modifier_flag_active(code: u16, flags: u64) -> bool {
        let modifier_flag = match code {
            54 | 55 => CGEventFlags::CGEventFlagCommand.bits(),
            56 | 60 => CGEventFlags::CGEventFlagShift.bits(),
            58 | 61 => CGEventFlags::CGEventFlagAlternate.bits(),
            59 | 62 => CGEventFlags::CGEventFlagControl.bits(),
            63 => CGEventFlags::CGEventFlagSecondaryFn.bits(),
            _ => 0,
        };
        flags & modifier_flag != 0
    }

    fn key_from_code(code: u16) -> Key {
        if caps_lock_remap::CAPS_LOCK_REMAP_ACTIVE.load(Ordering::Acquire)
            && code == caps_lock_remap::VKEY_F18
        {
            return Key::CapsLock;
        }

        match code {
            // Letter keys
            0 => Key::KeyA,
            1 => Key::KeyS,
            2 => Key::KeyD,
            3 => Key::KeyF,
            4 => Key::KeyH,
            5 => Key::KeyG,
            6 => Key::KeyZ,
            7 => Key::KeyX,
            8 => Key::KeyC,
            9 => Key::KeyV,
            11 => Key::KeyB,
            12 => Key::KeyQ,
            13 => Key::KeyW,
            14 => Key::KeyE,
            15 => Key::KeyR,
            16 => Key::KeyY,
            17 => Key::KeyT,
            31 => Key::KeyO,
            32 => Key::KeyU,
            34 => Key::KeyI,
            35 => Key::KeyP,
            37 => Key::KeyL,
            38 => Key::KeyJ,
            40 => Key::KeyK,
            45 => Key::KeyN,
            46 => Key::KeyM,
            // Number row
            18 => Key::Num1,
            19 => Key::Num2,
            20 => Key::Num3,
            21 => Key::Num4,
            22 => Key::Num6,
            23 => Key::Num5,
            25 => Key::Num9,
            26 => Key::Num7,
            28 => Key::Num8,
            29 => Key::Num0,
            // Punctuation
            24 => Key::Equal,
            27 => Key::Minus,
            30 => Key::RightBracket,
            33 => Key::LeftBracket,
            39 => Key::Quote,
            41 => Key::SemiColon,
            42 => Key::BackSlash,
            43 => Key::Comma,
            44 => Key::Slash,
            47 => Key::Dot,
            50 => Key::BackQuote,
            // Whitespace / editing
            36 => Key::Return,
            48 => Key::Tab,
            49 => Key::Space,
            51 => Key::Backspace,
            53 => Key::Escape,
            117 => Key::Delete,
            // Modifiers
            54 => Key::MetaRight,
            55 => Key::MetaLeft,
            56 => Key::ShiftLeft,
            57 => Key::CapsLock,
            58 => Key::Alt,
            59 => Key::ControlLeft,
            60 => Key::ShiftRight,
            61 => Key::AltGr,
            62 => Key::ControlRight,
            63 => Key::Function,
            // Navigation
            115 => Key::Home,
            116 => Key::PageUp,
            119 => Key::End,
            121 => Key::PageDown,
            123 => Key::LeftArrow,
            124 => Key::RightArrow,
            125 => Key::DownArrow,
            126 => Key::UpArrow,
            // Function keys
            96 => Key::F5,
            97 => Key::F6,
            98 => Key::F7,
            99 => Key::F3,
            100 => Key::F8,
            101 => Key::F9,
            103 => Key::F11,
            109 => Key::F10,
            111 => Key::F12,
            118 => Key::F4,
            120 => Key::F2,
            122 => Key::F1,
            // Numpad
            65 => Key::KpDelete,
            67 => Key::KpMultiply,
            69 => Key::KpPlus,
            71 => Key::NumLock,
            75 => Key::KpDivide,
            76 => Key::KpReturn,
            78 => Key::KpMinus,
            82 => Key::Kp0,
            83 => Key::Kp1,
            84 => Key::Kp2,
            85 => Key::Kp3,
            86 => Key::Kp4,
            87 => Key::Kp5,
            88 => Key::Kp6,
            89 => Key::Kp7,
            91 => Key::Kp8,
            92 => Key::Kp9,
            _ => Key::Unknown(code as u32),
        }
    }

    pub fn set_caps_lock_remap_enabled(enabled: bool) {
        caps_lock_remap::set_enabled(enabled);
    }

    pub fn shutdown() {
        caps_lock_remap::shutdown();
    }

    pub fn run<F>(callback: F)
    where
        F: FnMut(Event) -> Option<Event> + Send + 'static,
    {
        let mask: u64 = (1 << 1)
            | (1 << 2)
            | (1 << 3)
            | (1 << 4)
            | (1 << 5)
            | (1 << 6)
            | (1 << 7)
            | (1 << 10)
            | (1 << 11)
            | (1 << 12)
            | (1 << 22);

        unsafe {
            CALLBACK = Some(Box::new(callback));

            let tap = CGEventTapCreate(0, 0, 0, mask, tap_callback, std::ptr::null());
            if tap.is_null() {
                eprintln!(
                    "input hook error: failed to create macOS event tap; check Accessibility permissions"
                );
                return;
            }

            TAP_REF = tap;

            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            if source.is_null() {
                eprintln!("input hook error: failed to create macOS run loop source");
                return;
            }

            let current_run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(current_run_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRun();
        }
    }

    pub fn is_accessibility_trusted(prompt: bool) -> bool {
        extern "C" {
            fn AXIsProcessTrustedWithOptions(
                options: core_foundation::dictionary::CFDictionaryRef,
            ) -> bool;
        }

        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let val = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
    }
}

pub struct InputEmitter {
    platform: PlatformEmitter,
    logged_error: bool,
}

impl InputEmitter {
    pub fn new() -> Self {
        Self {
            platform: PlatformEmitter::new(),
            logged_error: false,
        }
    }

    pub fn emit_all(&mut self, actions: &[Action]) {
        for action in actions {
            if let Err(error) = self.platform.emit(action) {
                if !self.logged_error {
                    eprintln!("mouse emit error: {error}");
                    self.logged_error = true;
                }
                break;
            }
        }
    }
}

#[cfg(target_os = "windows")]
struct PlatformEmitter;

#[cfg(target_os = "windows")]
impl PlatformEmitter {
    fn new() -> Self {
        Self
    }

    fn emit(&mut self, action: &Action) -> Result<(), String> {
        // Win32 defines one scroll notch as 120 mouseData units; apps accumulate and act per 120,
        // so sending sub-120 values each tick enables smooth high-resolution scrolling.
        const WHEEL_DELTA: f64 = 120.0;

        match action {
            // A real MOUSEEVENTF_MOVE event, not SetCursorPos: SetCursorPos only warps the cursor
            // position and injects no move into the input stream, so apps driven by mouse-move
            // messages (drag-select overlays like Snipping Tool, hover crosshairs) never update.
            Action::MouseMove(point) => unsafe {
                let (dx, dy) = virtual_desktop_absolute(point.x, point.y);
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        mi: MOUSEINPUT {
                            dx,
                            dy,
                            mouseData: 0,
                            dwFlags: MOUSEEVENTF_MOVE
                                | MOUSEEVENTF_ABSOLUTE
                                | MOUSEEVENTF_VIRTUALDESK,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                if SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) != 1 {
                    Err("SendInput move failed".to_string())
                } else {
                    Ok(())
                }
            },
            Action::Scroll { delta_x, delta_y } => unsafe {
                let mut result = Ok(());
                if *delta_y != 0.0 {
                    let data = (delta_y * WHEEL_DELTA).round() as i32 as u32;
                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: data,
                                dwFlags: MOUSEEVENTF_WHEEL,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    if SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) != 1 {
                        result = Err("SendInput scroll Y failed".to_string());
                    }
                }
                if *delta_x != 0.0 {
                    let data = (delta_x * WHEEL_DELTA).round() as i32 as u32;
                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: data,
                                dwFlags: MOUSEEVENTF_HWHEEL,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    if SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) != 1 {
                        result = Err("SendInput scroll X failed".to_string());
                    }
                }
                result
            },
            _ => simulate_input(&action_to_event_type(action)),
        }
    }
}

/// Maps a physical-pixel virtual-desktop point to the normalized 0..=65535 coordinates that
/// MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK expect. Dividing by (dimension - 1) lands the
/// far edge exactly on 65535 rather than one pixel short.
#[cfg(target_os = "windows")]
fn virtual_desktop_absolute(x: f64, y: f64) -> (i32, i32) {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
        let norm = |value: i32, origin: i32, extent: i32| {
            let span = (extent - 1).max(1) as f64;
            (((value - origin) as f64 * 65535.0 / span).round() as i32).clamp(0, 65535)
        };
        (
            norm(clamp_f64_to_i32(x), left, width),
            norm(clamp_f64_to_i32(y), top, height),
        )
    }
}

#[cfg(target_os = "macos")]
struct PlatformEmitter {
    source: core_graphics::event_source::CGEventSource,
    click_count: i64,
    last_press_left: Option<bool>,
    last_press_time: std::time::Instant,
    // Last known cursor position; updated on MouseMove so button events don't need to query it.
    last_cursor: core_graphics::geometry::CGPoint,
    // Fractional line deltas carried between scroll events (see the Scroll arm in emit).
    scroll_line_accum_x: f64,
    scroll_line_accum_y: f64,
}

#[cfg(target_os = "macos")]
impl PlatformEmitter {
    fn new() -> Self {
        extern "C" {
            fn CGSetLocalEventsSuppressionInterval(seconds: f64);
        }
        unsafe {
            // By default macOS ignores physical mouse input for ~0.25s after each
            // synthetic mouse event; disable that so the real mouse always works.
            CGSetLocalEventsSuppressionInterval(0.0);
        }
        Self {
            source: CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .expect("CGEventSource creation failed"),
            click_count: 0,
            last_press_left: None,
            last_press_time: std::time::Instant::now(),
            last_cursor: core_graphics::geometry::CGPoint { x: 0.0, y: 0.0 },
            scroll_line_accum_x: 0.0,
            scroll_line_accum_y: 0.0,
        }
    }

    /// Refresh `last_cursor` from the OS's current cursor location. The physical mouse may
    /// have moved since the last synthetic MouseMove, so button events must read the live
    /// position rather than the stale cached one to avoid teleporting the cursor back.
    fn sync_last_cursor_from_os(&mut self) {
        extern "C" {
            fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
            fn CGEventGetLocation(event: *mut std::ffi::c_void)
                -> core_graphics::geometry::CGPoint;
            fn CFRelease(cf: *mut std::ffi::c_void);
        }
        unsafe {
            let ev = CGEventCreate(std::ptr::null());
            if !ev.is_null() {
                self.last_cursor = CGEventGetLocation(ev);
                CFRelease(ev);
            }
        }
    }

    fn emit(&mut self, action: &Action) -> Result<(), String> {
        const MULTI_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

        let (cg_type, cg_button) = match action {
            Action::MouseMove(Point { x, y }) => {
                self.last_cursor = core_graphics::geometry::CGPoint { x: *x, y: *y };
                // macOS requires LeftMouseDragged/RightMouseDragged when a button is held;
                // plain MouseMoved is ignored by apps that use OS drag sessions (Finder, Chrome tabs).
                let (cg_type, cg_button) = if mouse_button_is_down(rdev::Button::Left) {
                    (CGEventType::LeftMouseDragged, CGMouseButton::Left)
                } else if mouse_button_is_down(rdev::Button::Right) {
                    (CGEventType::RightMouseDragged, CGMouseButton::Right)
                } else {
                    // Warping instead would skip the event stream, leaving apps' hover
                    // cursors (I-beam, pointer) stale until the next click.
                    (CGEventType::MouseMoved, CGMouseButton::Left)
                };
                let event = CGEvent::new_mouse_event(
                    self.source.clone(),
                    cg_type,
                    self.last_cursor,
                    cg_button,
                )
                .map_err(|_| "CGEvent mouse move creation failed".to_string())?;
                // Plain moves post at Session, above the HID insertion point, so these
                // high-rate events skip our own HID tap (motion loop already updates its state).
                event.post(if matches!(cg_type, CGEventType::MouseMoved) {
                    CGEventTapLocation::Session
                } else {
                    CGEventTapLocation::HID
                });
                return Ok(());
            }
            Action::ButtonPress(rdev::Button::Left) => {
                (CGEventType::LeftMouseDown, CGMouseButton::Left)
            }
            Action::ButtonRelease(rdev::Button::Left) => {
                (CGEventType::LeftMouseUp, CGMouseButton::Left)
            }
            Action::ButtonPress(rdev::Button::Right) => {
                (CGEventType::RightMouseDown, CGMouseButton::Right)
            }
            Action::ButtonRelease(rdev::Button::Right) => {
                (CGEventType::RightMouseUp, CGMouseButton::Right)
            }
            Action::ButtonPress(b @ (rdev::Button::Middle | rdev::Button::Unknown(_))) => {
                self.sync_last_cursor_from_os();
                return emit_other_mouse_button(
                    &self.source,
                    CGEventType::OtherMouseDown,
                    self.last_cursor,
                    other_button_index(b),
                );
            }
            Action::ButtonRelease(b @ (rdev::Button::Middle | rdev::Button::Unknown(_))) => {
                return emit_other_mouse_button(
                    &self.source,
                    CGEventType::OtherMouseUp,
                    self.last_cursor,
                    other_button_index(b),
                );
            }
            Action::Scroll { delta_x, delta_y } => {
                // 48.0 accurately scales ViMouse scroll units to macOS pixel units.
                const PIXELS_PER_UNIT: f64 = 48.0;
                // macOS derives an event's line (fixed-point) delta as 1/10 of its pixel delta.
                const PIXELS_PER_LINE: f64 = 10.0;
                let px_y = (delta_y * PIXELS_PER_UNIT).round() as i32;
                let px_x = (delta_x * PIXELS_PER_UNIT).round() as i32;

                // Coarse consumers (e.g. Tk) clamp any sub-line delta to a whole step, erasing
                // speed differences; carry fractions so speed lives in the whole-line event rate.
                self.scroll_line_accum_y += delta_y * PIXELS_PER_UNIT / PIXELS_PER_LINE;
                self.scroll_line_accum_x += delta_x * PIXELS_PER_UNIT / PIXELS_PER_LINE;
                let lines_y = self.scroll_line_accum_y.trunc();
                let lines_x = self.scroll_line_accum_x.trunc();
                self.scroll_line_accum_y -= lines_y;
                self.scroll_line_accum_x -= lines_x;

                let event = CGEvent::new_scroll_event(
                    self.source.clone(),
                    ScrollEventUnit::PIXEL,
                    2,
                    px_y,
                    px_x,
                    0,
                )
                .map_err(|_| "CGEvent scroll creation failed".to_string())?;
                // Writing a line field rewrites its derived pixel/fixed-point fields, so the
                // pixel and fixed-point fields must be (re)written after the line fields.
                event.set_integer_value_field(
                    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                    lines_y as i64,
                );
                event.set_integer_value_field(
                    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
                    lines_x as i64,
                );
                event.set_integer_value_field(
                    EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
                    px_y as i64,
                );
                event.set_integer_value_field(
                    EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
                    px_x as i64,
                );
                event.set_double_value_field(
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
                    lines_y,
                );
                event.set_double_value_field(
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
                    lines_x,
                );
                // The event source seeds flags from the live modifier state, and runtime
                // modifiers are not suppressed on macOS - strip theirs for a plain scroll.
                let mut flags = event.get_flags();
                flags.remove(runtime_modifier_flags());
                event.set_flags(flags);
                event.post(core_graphics::event::CGEventTapLocation::HID);
                return Ok(());
            }
        };

        if matches!(action, Action::ButtonPress(_)) {
            self.sync_last_cursor_from_os();

            let is_left = matches!(cg_button, CGMouseButton::Left);
            let same_button = self.last_press_left == Some(is_left);
            self.click_count = if same_button && self.last_press_time.elapsed() < MULTI_CLICK_WINDOW
            {
                self.click_count + 1
            } else {
                1
            };
            self.last_press_left = Some(is_left);
            self.last_press_time = std::time::Instant::now();
        }

        let event =
            CGEvent::new_mouse_event(self.source.clone(), cg_type, self.last_cursor, cg_button)
                .map_err(|_| "CGEvent mouse event creation failed".to_string())?;
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, self.click_count);
        event.post(CGEventTapLocation::HID);

        Ok(())
    }
}

/// Combined modifier flags of the runtime modifier keys (KEYS_SCROLL / KEY_FAST / KEY_SLOW),
/// so synthetic scrolls can shed the flags those held keys would otherwise stamp on them.
#[cfg(target_os = "macos")]
fn runtime_modifier_flags() -> CGEventFlags {
    let mut flags = CGEventFlags::CGEventFlagNull;
    for key in crate::input::runtime_modifiers() {
        flags |= match key {
            rdev::Key::ShiftLeft | rdev::Key::ShiftRight => CGEventFlags::CGEventFlagShift,
            rdev::Key::Alt | rdev::Key::AltGr => CGEventFlags::CGEventFlagAlternate,
            rdev::Key::ControlLeft | rdev::Key::ControlRight => CGEventFlags::CGEventFlagControl,
            rdev::Key::MetaLeft | rdev::Key::MetaRight => CGEventFlags::CGEventFlagCommand,
            rdev::Key::Function => CGEventFlags::CGEventFlagSecondaryFn,
            _ => CGEventFlags::CGEventFlagNull,
        };
    }
    flags
}

#[cfg(target_os = "macos")]
fn other_button_index(button: &rdev::Button) -> u32 {
    match button {
        rdev::Button::Middle => 2,
        rdev::Button::Unknown(n) => *n as u32,
        _ => unreachable!(),
    }
}

#[cfg(target_os = "macos")]
fn emit_other_mouse_button(
    source: &core_graphics::event_source::CGEventSource,
    event_type: CGEventType,
    position: core_graphics::geometry::CGPoint,
    button: u32,
) -> Result<(), String> {
    use foreign_types_shared::ForeignType;
    extern "C" {
        fn CGEventCreateMouseEvent(
            source: *mut std::ffi::c_void,
            mouse_type: CGEventType,
            mouse_cursor_position: core_graphics::geometry::CGPoint,
            mouse_button: u32,
        ) -> *mut std::ffi::c_void;
        fn CGEventPost(tap: CGEventTapLocation, event: *mut std::ffi::c_void);
        fn CFRelease(cf: *mut std::ffi::c_void);
    }
    unsafe {
        let event_ref =
            CGEventCreateMouseEvent(source.as_ptr() as *mut _, event_type, position, button);
        if event_ref.is_null() {
            return Err("CGEvent other mouse event creation failed".to_string());
        }
        CGEventPost(CGEventTapLocation::HID, event_ref);
        CFRelease(event_ref);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct PlatformEmitter {
    xlib: Option<x11_dl::xlib::Xlib>,
    xtest: Option<x11_dl::xtest::Xf86vmode>,
    display: *mut x11_dl::xlib::Display,
    scroll_accum_x: f64,
    scroll_accum_y: f64,
    logged_xtest_fallback: bool,
}

#[cfg(target_os = "linux")]
impl PlatformEmitter {
    fn new() -> Self {
        let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
            eprintln!(
                "ViMouse: failed to load libX11 - mouse movement, clicks, and scrolling \
                 will fall back to a degraded path and may not work. Install libX11 (e.g. libx11-6)."
            );
            return Self {
                xlib: None,
                xtest: None,
                display: ptr::null_mut(),
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
                logged_xtest_fallback: false,
            };
        };
        let Ok(xtest) = x11_dl::xtest::Xf86vmode::open() else {
            eprintln!(
                "ViMouse: failed to load libXtst (XTest extension) - mouse movement, \
                 clicks, and scrolling will not work. Install libXtst (e.g. libxtst6)."
            );
            return Self {
                xlib: None,
                xtest: None,
                display: ptr::null_mut(),
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
                logged_xtest_fallback: false,
            };
        };

        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            eprintln!(
                "ViMouse: XOpenDisplay failed (no X11 display - is DISPLAY set / are you on X11?). \
                 Mouse movement, clicks, and scrolling will fall back to a degraded path."
            );
            return Self {
                xlib: None,
                xtest: None,
                display,
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
                logged_xtest_fallback: false,
            };
        }

        Self {
            xlib: Some(xlib),
            xtest: Some(xtest),
            display,
            scroll_accum_x: 0.0,
            scroll_accum_y: 0.0,
            logged_xtest_fallback: false,
        }
    }

    fn emit(&mut self, action: &Action) -> Result<(), String> {
        let mut xtest_failed = false;
        if let (Some(xlib), Some(xtest)) = (&self.xlib, &self.xtest) {
            let status = unsafe {
                match action {
                    Action::MouseMove(point) => (xtest.XTestFakeMotionEvent)(
                        self.display,
                        0,
                        clamp_f64_to_i32(point.x),
                        clamp_f64_to_i32(point.y),
                        0,
                    ),
                    Action::Scroll { delta_x, delta_y } => {
                        self.scroll_accum_x += delta_x;
                        self.scroll_accum_y += delta_y;
                        let clicks_x = self.scroll_accum_x.trunc() as i64;
                        let clicks_y = self.scroll_accum_y.trunc() as i64;
                        self.scroll_accum_x -= clicks_x as f64;
                        self.scroll_accum_y -= clicks_y as f64;
                        let mut result = 1;
                        if clicks_x != 0 {
                            result &= emit_scroll_axis(xtest, self.display, clicks_x, 6, 7);
                        }
                        if clicks_y != 0 {
                            result &= emit_scroll_axis(xtest, self.display, clicks_y, 5, 4);
                        }
                        result
                    }
                    Action::ButtonPress(button) => {
                        if let Some(code) = linux_button_code(*button) {
                            (xtest.XTestFakeButtonEvent)(self.display, code, 1, 0)
                        } else {
                            1
                        }
                    }
                    Action::ButtonRelease(button) => {
                        if let Some(code) = linux_button_code(*button) {
                            (xtest.XTestFakeButtonEvent)(self.display, code, 0, 0)
                        } else {
                            1
                        }
                    }
                }
            };

            if status != 0 {
                unsafe {
                    (xlib.XFlush)(self.display);
                }
                return Ok(());
            }
            xtest_failed = true;
        }

        if xtest_failed && !self.logged_xtest_fallback {
            eprintln!(
                "ViMouse: an XTest event failed - falling back to the rdev simulate path. \
                 Mouse control may be degraded."
            );
            self.logged_xtest_fallback = true;
        }

        simulate_input(&action_to_event_type(action))
    }
}

#[cfg(target_os = "linux")]
impl Drop for PlatformEmitter {
    fn drop(&mut self) {
        if let Some(xlib) = &self.xlib {
            if !self.display.is_null() {
                unsafe {
                    (xlib.XCloseDisplay)(self.display);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn emit_scroll_axis(
    xtest: &x11_dl::xtest::Xf86vmode,
    display: *mut x11_dl::xlib::Display,
    delta: i64,
    negative_button: u32,
    positive_button: u32,
) -> i32 {
    let mut result = 1;
    let button = if delta >= 0 {
        positive_button
    } else {
        negative_button
    };

    for _ in 0..delta.abs() {
        unsafe {
            result &= (xtest.XTestFakeButtonEvent)(display, button, 1, 0);
            result &= (xtest.XTestFakeButtonEvent)(display, button, 0, 0);
        }
    }

    result
}

#[cfg(target_os = "linux")]
fn linux_button_code(button: Button) -> Option<u32> {
    match button {
        Button::Left => Some(1),
        Button::Middle => Some(2),
        Button::Right => Some(3),
        Button::Unknown(code) => Some(u32::from(code)),
    }
}

#[cfg(not(target_os = "macos"))]
fn clamp_f64_to_i32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    value.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}
