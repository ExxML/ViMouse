use crate::state::Action;
#[cfg(target_os = "linux")]
use rdev::Button;
use rdev::{simulate, EventType};

static SIMULATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn simulate_input(event_type: &EventType) -> Result<(), String> {
    let _guard = SIMULATE_LOCK.lock().expect("simulate lock poisoned");
    simulate(event_type).map_err(|_| "rdev input simulation failed".to_string())
}

pub fn action_to_event_type(action: &Action, scroll_scale: f64) -> EventType {
    match action {
        Action::MouseMove(point) => EventType::MouseMove {
            x: point.x,
            y: point.y,
        },
        Action::Scroll { delta_x, delta_y } => EventType::Wheel {
            delta_x: (delta_x * scroll_scale).round() as i64,
            delta_y: (delta_y * scroll_scale).round() as i64,
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
    #[cfg(target_os = "macos")]
    macos_grab::shutdown();
}

// macOS event suppression and simulation works differently than Windows or Linux
// therefore, we use a custom event tap on macOS instead of rdev's built-in grab/simulate functionality
// otherwise a "Trace/BPT trap: 5" error is thrown when emitting synthetic key events
#[cfg(target_os = "macos")]
pub mod macos_grab {
    use crate::caps_lock_remap;
    use core_graphics::event::{CGEventFlags, CGEventTapProxy, CGEventType};
    use rdev::{Button, Event, EventType, Key};
    use std::os::raw::c_void;
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
        use std::sync::atomic::Ordering;
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
        use core_foundation::base::TCFType;
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;

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
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

        // Win32 defines one scroll notch as 120 mouseData units; apps accumulate and act per 120,
        // so sending sub-120 values each tick enables smooth high-resolution scrolling.
        const WHEEL_DELTA: f64 = 120.0;

        match action {
            Action::MouseMove(point) => unsafe {
                if SetCursorPos(clamp_f64_to_i32(point.x), clamp_f64_to_i32(point.y)) == 0 {
                    Err("SetCursorPos failed".to_string())
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
            _ => simulate_input(&action_to_event_type(action, 1.0)),
        }
    }
}

#[cfg(target_os = "macos")]
struct PlatformEmitter {
    source: core_graphics::event_source::CGEventSource,
    click_count: i64,
    last_press_left: Option<bool>,
    last_press_time: std::time::Instant,
}

#[cfg(target_os = "macos")]
impl PlatformEmitter {
    fn new() -> Self {
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        Self {
            source: CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .expect("CGEventSource creation failed"),
            click_count: 0,
            last_press_left: None,
            last_press_time: std::time::Instant::now(),
        }
    }

    fn emit(&mut self, action: &Action) -> Result<(), String> {
        use core_graphics::event::{
            CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
        };

        const MULTI_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

        let (cg_type, cg_button) = match action {
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
            Action::Scroll { delta_x, delta_y } => {
                use core_graphics::event::ScrollEventUnit;
                // 16.0 scales ViMouse scroll units to macOS pixel scroll units.
                const PIXELS_PER_UNIT: f64 = 16.0;
                let px_y = (delta_y * PIXELS_PER_UNIT).round() as i32;
                let px_x = (delta_x * PIXELS_PER_UNIT).round() as i32;
                let event = CGEvent::new_scroll_event(
                    self.source.clone(),
                    ScrollEventUnit::PIXEL,
                    2,
                    px_y,
                    px_x,
                    0,
                )
                .map_err(|_| "CGEvent scroll creation failed".to_string())?;
                event.post(core_graphics::event::CGEventTapLocation::HID);
                return Ok(());
            }
            _ => {
                return simulate_input(&action_to_event_type(action, 1.0));
            }
        };

        if matches!(action, Action::ButtonPress(_)) {
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

        let pos = CGEvent::new(self.source.clone())
            .map_err(|_| "CGEvent creation failed".to_string())?
            .location();
        let event = CGEvent::new_mouse_event(self.source.clone(), cg_type, pos, cg_button)
            .map_err(|_| "CGEvent mouse event creation failed".to_string())?;
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, self.click_count);
        event.post(CGEventTapLocation::HID);

        Ok(())
    }
}

#[cfg(target_os = "linux")]
struct PlatformEmitter {
    xlib: Option<x11_dl::xlib::Xlib>,
    xtest: Option<x11_dl::xtest::Xf86vmode>,
    display: *mut x11_dl::xlib::Display,
    scroll_accum_x: f64,
    scroll_accum_y: f64,
}

#[cfg(target_os = "linux")]
impl PlatformEmitter {
    fn new() -> Self {
        use std::ptr;

        let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
            return Self {
                xlib: None,
                xtest: None,
                display: ptr::null_mut(),
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
            };
        };
        let Ok(xtest) = x11_dl::xtest::Xf86vmode::open() else {
            return Self {
                xlib: None,
                xtest: None,
                display: ptr::null_mut(),
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
            };
        };

        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            return Self {
                xlib: None,
                xtest: None,
                display,
                scroll_accum_x: 0.0,
                scroll_accum_y: 0.0,
            };
        }

        Self {
            xlib: Some(xlib),
            xtest: Some(xtest),
            display,
            scroll_accum_x: 0.0,
            scroll_accum_y: 0.0,
        }
    }

    fn emit(&mut self, action: &Action) -> Result<(), String> {
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
        }

        simulate_input(&action_to_event_type(action, 1.0))
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
