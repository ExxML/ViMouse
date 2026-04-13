use crate::config::{
     FAST_MULTIPLIER, JUMP_GRID, KEYS_FAST, KEYS_SCROLL, KEYS_SLOW, KEY_CYCLE_MONITOR,
    KEY_INSERT_MODE, KEY_LEFT_CLICK, KEY_MOVE_DOWN, KEY_MOVE_LEFT, KEY_MOVE_RIGHT, KEY_MOVE_UP,
    KEY_NORMAL_MODE, KEY_QUIT, KEY_RIGHT_CLICK, MOVE_SPEED_PX_PER_SEC,
    SCROLL_SPEED_UNITS_PER_SEC, SLOW_MULTIPLIER, TICK_RATE_HZ,
};
use crate::monitor::{clamp_to_virtual_bounds, monitor_index_for_point};
use crate::state::{Action, Mode, Point, Shared, SharedState};
#[cfg(not(target_os = "macos"))]
use rdev::grab;
use rdev::{simulate, Button, Event, EventType, Key, SimulateError};
use std::collections::HashSet;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// On macOS, rdev::grab suppresses events by calling CGEventSetType(event, kCGEventNull).
// On macOS 15 (Sequoia) this triggers a CoreGraphics assertion (SIGTRAP). The correct
// way to suppress is to return NULL from the CGEventTap callback. We implement our own
// grab here that does exactly that.
#[cfg(target_os = "macos")]
mod macos_grab {
    use core_graphics::event::{CGEventTapProxy, CGEventType};
    use rdev::{Button, Event, EventType, Key};
    use std::os::raw::c_void;
    use std::time::SystemTime;

    type GrabCb = Box<dyn FnMut(Event) -> Option<Event> + Send>;
    static mut CB: Option<GrabCb> = None;
    static mut TAP_REF: *mut c_void = std::ptr::null_mut();
    static mut LAST_FLAGS: u64 = 0;

    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events: u64,
            cb: unsafe extern "C" fn(CGEventTapProxy, CGEventType, *mut c_void, *const c_void)
                -> *mut c_void,
            info: *const c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventGetLocation(event: *mut c_void) -> CgPoint;
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: *mut c_void,
            order: isize,
        ) -> *mut c_void;
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    #[repr(C)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    unsafe extern "C" fn tap_cb(
        _proxy: CGEventTapProxy,
        kind: CGEventType,
        raw: *mut c_void,
        _info: *const c_void,
    ) -> *mut c_void {
        match kind {
            // If the tap was disabled (e.g. callback was too slow), re-enable it.
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                CGEventTapEnable(TAP_REF, true);
                return raw;
            }
            _ => {}
        }
        let Some(event) = to_rdev(kind, raw) else {
            return raw;
        };
        #[allow(static_mut_refs)]
        if let Some(cb) = CB.as_mut() {
            if cb(event).is_none() {
                return std::ptr::null_mut(); // Return NULL — the correct way to suppress
            }
        }
        raw
    }

    unsafe fn to_rdev(kind: CGEventType, raw: *mut c_void) -> Option<Event> {
        const KEYCODE: u32 = 9;
        const SCROLL_DY: u32 = 96;
        const SCROLL_DX: u32 = 97;

        let et = match kind {
            CGEventType::LeftMouseDown => EventType::ButtonPress(Button::Left),
            CGEventType::LeftMouseUp => EventType::ButtonRelease(Button::Left),
            CGEventType::RightMouseDown => EventType::ButtonPress(Button::Right),
            CGEventType::RightMouseUp => EventType::ButtonRelease(Button::Right),
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged => {
                let pt = CGEventGetLocation(raw);
                EventType::MouseMove { x: pt.x, y: pt.y }
            }
            CGEventType::KeyDown => {
                let code = CGEventGetIntegerValueField(raw, KEYCODE) as u16;
                EventType::KeyPress(keycode(code))
            }
            CGEventType::KeyUp => {
                let code = CGEventGetIntegerValueField(raw, KEYCODE) as u16;
                EventType::KeyRelease(keycode(code))
            }
            CGEventType::FlagsChanged => {
                let code = CGEventGetIntegerValueField(raw, KEYCODE) as u16;
                let flags = CGEventGetFlags(raw);
                if flags < LAST_FLAGS {
                    LAST_FLAGS = flags;
                    EventType::KeyRelease(keycode(code))
                } else {
                    LAST_FLAGS = flags;
                    EventType::KeyPress(keycode(code))
                }
            }
            CGEventType::ScrollWheel => {
                let dy = CGEventGetIntegerValueField(raw, SCROLL_DY);
                let dx = CGEventGetIntegerValueField(raw, SCROLL_DX);
                EventType::Wheel { delta_x: dx, delta_y: dy }
            }
            _ => return None,
        };
        Some(Event { event_type: et, time: SystemTime::now(), name: None })
    }

    fn keycode(code: u16) -> Key {
        match code {
            0 => Key::KeyA,      1 => Key::KeyS,    2 => Key::KeyD,    3 => Key::KeyF,
            4 => Key::KeyH,      5 => Key::KeyG,    6 => Key::KeyZ,    7 => Key::KeyX,
            8 => Key::KeyC,      9 => Key::KeyV,   11 => Key::KeyB,   12 => Key::KeyQ,
           13 => Key::KeyW,     14 => Key::KeyE,   15 => Key::KeyR,   16 => Key::KeyY,
           17 => Key::KeyT,     18 => Key::Num1,   19 => Key::Num2,   20 => Key::Num3,
           21 => Key::Num4,     22 => Key::Num6,   23 => Key::Num5,   24 => Key::Equal,
           25 => Key::Num9,     26 => Key::Num7,   27 => Key::Minus,  28 => Key::Num8,
           29 => Key::Num0,     30 => Key::RightBracket,  31 => Key::KeyO,  32 => Key::KeyU,
           33 => Key::LeftBracket, 34 => Key::KeyI, 35 => Key::KeyP,  36 => Key::Return,
           37 => Key::KeyL,     38 => Key::KeyJ,   39 => Key::Quote,  40 => Key::KeyK,
           41 => Key::SemiColon, 42 => Key::BackSlash, 43 => Key::Comma, 44 => Key::Slash,
           45 => Key::KeyN,     46 => Key::KeyM,   47 => Key::Dot,    48 => Key::Tab,
           49 => Key::Space,    50 => Key::BackQuote,   51 => Key::Backspace,
           53 => Key::Escape,   54 => Key::MetaRight,   55 => Key::MetaLeft,
           56 => Key::ShiftLeft, 57 => Key::CapsLock,  58 => Key::Alt,
           59 => Key::ControlLeft, 60 => Key::ShiftRight, 61 => Key::AltGr,
           62 => Key::ControlRight, 63 => Key::Function,
           96 => Key::F5,       97 => Key::F6,     98 => Key::F7,     99 => Key::F3,
          100 => Key::F8,      101 => Key::F9,    103 => Key::F11,   109 => Key::F10,
          111 => Key::F12,     118 => Key::F4,    120 => Key::F2,    122 => Key::F1,
          123 => Key::LeftArrow, 124 => Key::RightArrow, 125 => Key::DownArrow, 126 => Key::UpArrow,
            _ => Key::Unknown(code as u32),
        }
    }

    pub fn run<F: FnMut(Event) -> Option<Event> + Send + 'static>(callback: F) {
        // Matches rdev's kCGEventMaskForAllEvents
        let mask: u64 = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5)
            | (1 << 6)  | (1 << 7)
            | (1 << 10) | (1 << 11) | (1 << 12)
            | (1 << 22);
        unsafe {
            CB = Some(Box::new(callback));
            let tap = CGEventTapCreate(0, 0, 0, mask, tap_cb, std::ptr::null());
            if tap.is_null() {
                eprintln!("global input grab failed: CGEventTapCreate returned null (check Accessibility permissions)");
                return;
            }
            TAP_REF = tap;
            CGEventTapEnable(tap, true);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            let rl = CFRunLoopGetCurrent();
            CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
            CFRunLoopRun();
        }
    }
}

// rdev simulation is global OS state, so serialize synthetic events.
static SIMULATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug)]
struct DeferredModifier {
    key: Key,
    used_by_suppressed_combo: bool,
}

#[derive(Debug, Default)]
struct HookState {
    deferred_modifiers: Vec<DeferredModifier>,
    suppressed_keys: HashSet<Key>,
    // Windows and macOS re-deliver synthetic events back through the grab hook,
    // so we track pending passthroughs to skip them on those platforms.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    synthetic_passthrough: Vec<EventType>,
}

#[derive(Debug, Default)]
struct InputOutcome {
    actions: Vec<Action>,
    passthrough_events: Vec<EventType>,
    suppress_event: bool,
}

pub fn spawn_input_hook(shared: Shared) {
    thread::Builder::new()
        .name("vimouse-hook".into())
        .spawn(move || {
            let (tx, rx) = mpsc::channel::<InputOutcome>();
            let hook_state = Arc::new(Mutex::new(HookState::default()));
            let hook_state_dispatch = Arc::clone(&hook_state);

            // Dispatch on a separate thread: rdev::simulate must not be called
            // from within the grab callback on macOS (CGEventTap restriction).
            thread::Builder::new()
                .name("vimouse-dispatch".into())
                .spawn(move || {
                    for outcome in rx {
                        dispatch_actions(&outcome.actions);
                        dispatch_passthrough_events(&hook_state_dispatch, &outcome.passthrough_events);
                    }
                })
                .expect("failed to spawn dispatch thread");

            let hook_shared = shared.clone();
            let hook_runtime = Arc::clone(&hook_state);
            let callback =
                move |event: Event| handle_input_event(&hook_shared, &hook_runtime, event, &tx);
            #[cfg(target_os = "macos")]
            macos_grab::run(callback);
            #[cfg(not(target_os = "macos"))]
            if let Err(error) = grab(callback) {
                eprintln!("global input grab failed: {error:?}");
            }
        })
        .expect("failed to spawn input hook thread");
}

pub fn spawn_motion_loop(shared: Shared) {
    thread::Builder::new()
        .name("vimouse-motion".into())
        .spawn(move || {
            // Drive movement and scrolling from a fixed-rate loop for smooth output.
            let target_frame = Duration::from_secs_f64(1.0 / TICK_RATE_HZ as f64);
            let mut last_tick = Instant::now();

            loop {
                let now = Instant::now();
                let dt = now.saturating_duration_since(last_tick);
                last_tick = now;

                let actions = {
                    let mut state = shared.lock().expect("shared state poisoned");
                    tick_state(&mut state, dt)
                };

                dispatch_actions(&actions);

                let elapsed = Instant::now().saturating_duration_since(now);
                if elapsed < target_frame {
                    thread::sleep(target_frame - elapsed);
                }
            }
        })
        .expect("failed to spawn motion loop thread");
}

fn handle_input_event(
    shared: &Shared,
    hook_state: &Arc<Mutex<HookState>>,
    event: Event,
    tx: &mpsc::Sender<InputOutcome>,
) -> Option<Event> {
    {
        let mut hook = hook_state.lock().expect("hook state poisoned");
        if consume_synthetic_passthrough(&mut hook, event.event_type) {
            return Some(event);
        }
    }

    let outcome = {
        let mut state = shared.lock().expect("shared state poisoned");
        let mut hook = hook_state.lock().expect("hook state poisoned");
        process_input_event(&mut state, &mut hook, event.event_type)
    };

    let suppress = outcome.suppress_event;
    let _ = tx.send(outcome);

    if suppress {
        None
    } else {
        Some(event)
    }
}

fn process_input_event(
    state: &mut SharedState,
    hook_state: &mut HookState,
    event_type: EventType,
) -> InputOutcome {
    let mut outcome = InputOutcome::default();

    match event_type {
        EventType::MouseMove { x, y } => {
            update_cursor(state, Point { x, y });
        }
        EventType::KeyPress(key) => {
            outcome.suppress_event = handle_key_press(state, hook_state, key, &mut outcome);
        }
        EventType::KeyRelease(key) => {
            outcome.suppress_event = handle_key_release(state, hook_state, key, &mut outcome);
        }
        _ => {}
    }

    outcome
}

fn update_cursor(state: &mut SharedState, cursor: Point) {
    state.cursor = cursor;
    if let Some(index) = monitor_index_for_point(&state.monitors, cursor) {
        state.selected_monitor = index;
    }
}

fn handle_key_press(
    state: &mut SharedState,
    hook_state: &mut HookState,
    key: Key,
    outcome: &mut InputOutcome,
) -> bool {
    let is_repeat = !state.pressed_keys.insert(key);
    if !is_repeat && is_quit_chord(&state.pressed_keys) {
        std::process::exit(0);
    }

    match state.mode {
        Mode::Insert => {
            handle_insert_key_press(state, key, &mut outcome.actions);
            false
        }
        Mode::Normal => handle_normal_key_press(state, hook_state, key, is_repeat, outcome),
    }
}

fn handle_key_release(
    state: &mut SharedState,
    hook_state: &mut HookState,
    key: Key,
    outcome: &mut InputOutcome,
) -> bool {
    state.pressed_keys.remove(&key);

    if hook_state.suppressed_keys.remove(&key) {
        if state.mode == Mode::Normal {
            handle_normal_key_release(state, key, &mut outcome.actions);
        }
        return true;
    }

    if state.mode == Mode::Normal {
        handle_normal_key_release(state, key, &mut outcome.actions);
    }

    if let Some(deferred) = take_deferred_modifier(hook_state, key) {
        if deferred.used_by_suppressed_combo {
            return true;
        }

        outcome.passthrough_events.push(EventType::KeyPress(key));
    }

    false
}

fn handle_insert_key_press(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) {
    if key == KEY_NORMAL_MODE {
        set_mode(state, Mode::Normal, actions);
    }
}

fn handle_normal_key_press(
    state: &mut SharedState,
    hook_state: &mut HookState,
    key: Key,
    is_repeat: bool,
    outcome: &mut InputOutcome,
) -> bool {
    if is_modifier_key(key) {
        if !is_repeat && !has_deferred_modifier(hook_state, key) {
            hook_state.deferred_modifiers.push(DeferredModifier {
                key,
                used_by_suppressed_combo: false,
            });
        }
        return true;
    }

    let suppress_event = should_suppress_normal_key_press(&state.pressed_keys, key);
    if suppress_event {
        mark_deferred_modifiers_used_by_suppressed_combo(hook_state);
        if !is_repeat {
            hook_state.suppressed_keys.insert(key);
        }
    } else {
        flush_deferred_modifiers(hook_state, &mut outcome.passthrough_events);
    }

    if key == KEY_NORMAL_MODE {
        set_mode(state, Mode::Normal, &mut outcome.actions);
        return suppress_event;
    }

    if key == KEY_INSERT_MODE && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        set_mode(state, Mode::Insert, &mut outcome.actions);
        return suppress_event;
    }

    if key == KEY_CYCLE_MONITOR && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        cycle_monitor(state);
        return suppress_event;
    }

    if !is_repeat
        && exact_single_key(&state.pressed_keys, key)
        && jump_to_cell(state, key, &mut outcome.actions)
    {
        return suppress_event;
    }

    if let Some(button) = button_from_key(key) {
        if is_valid_button_set(&state.pressed_keys, key) {
            set_button_state(state, button, true, &mut outcome.actions);
        }
    }

    suppress_event
}

fn handle_normal_key_release(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) {
    if let Some(button) = button_from_key(key) {
        set_button_state(state, button, false, actions);
    }
}

fn cycle_monitor(state: &mut SharedState) {
    if !state.monitors.is_empty() {
        state.selected_monitor = (state.selected_monitor + 1) % state.monitors.len();
    }
}

fn jump_to_cell(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) -> bool {
    let Some((column, row)) = jump_cell(key) else {
        return false;
    };
    let Some(monitor) = state.monitors.get(state.selected_monitor).copied() else {
        return true;
    };

    // Jump to the center of the chosen 5x3 screen cell.
    let target = Point {
        x: monitor.origin.x + ((column as f64) + 0.5) * (monitor.width / 5.0),
        y: monitor.origin.y + ((row as f64) + 0.5) * (monitor.height / 3.0),
    };
    update_cursor(state, target);
    actions.push(Action::MouseMove(target));
    true
}

// Convert the currently held movement keys into smooth cursor or scroll output.
fn tick_state(state: &mut SharedState, dt: Duration) -> Vec<Action> {
    let mut actions = Vec::new();

    if state.mode != Mode::Normal {
        return actions;
    }

    let Some(direction) = normalized_direction(&state.pressed_keys) else {
        state.scroll_remainder = Point::default();
        return actions;
    };

    let speed_multiplier = speed_multiplier(&state.pressed_keys);
    let dt_seconds = dt.as_secs_f64();

    if is_valid_scroll_set(&state.pressed_keys) {
        tick_scroll(state, direction, speed_multiplier, dt_seconds, &mut actions);
    } else if is_valid_move_set(&state.pressed_keys) {
        tick_move(state, direction, speed_multiplier, dt_seconds, &mut actions);
    } else {
        state.scroll_remainder = Point::default();
    }

    actions
}

fn tick_scroll(
    state: &mut SharedState,
    direction: Point,
    speed_multiplier: f64,
    dt_seconds: f64,
    actions: &mut Vec<Action>,
) {
    // Keep fractional scroll in the accumulator so small per-frame steps stay smooth.
    state.scroll_remainder.x +=
        direction.x * SCROLL_SPEED_UNITS_PER_SEC * speed_multiplier * dt_seconds;
    state.scroll_remainder.y +=
        -direction.y * SCROLL_SPEED_UNITS_PER_SEC * speed_multiplier * dt_seconds;

    let whole_x = state.scroll_remainder.x.trunc() as i64;
    let whole_y = state.scroll_remainder.y.trunc() as i64;

    state.scroll_remainder.x -= whole_x as f64;
    state.scroll_remainder.y -= whole_y as f64;

    if whole_x != 0 || whole_y != 0 {
        actions.push(Action::Scroll {
            delta_x: whole_x,
            delta_y: whole_y,
        });
    }
}

fn tick_move(
    state: &mut SharedState,
    direction: Point,
    speed_multiplier: f64,
    dt_seconds: f64,
    actions: &mut Vec<Action>,
) {
    state.scroll_remainder = Point::default();
    let step = MOVE_SPEED_PX_PER_SEC * speed_multiplier * dt_seconds;
    let mut target = Point {
        x: state.cursor.x + direction.x * step,
        y: state.cursor.y + direction.y * step,
    };
    clamp_to_virtual_bounds(&mut target, &state.monitors);
    update_cursor(state, target);
    actions.push(Action::MouseMove(target));
}

// Mode switches release held buttons so we do not leave the OS in a stuck drag state.
fn set_mode(state: &mut SharedState, mode: Mode, actions: &mut Vec<Action>) {
    release_mouse_buttons(state, actions);
    state.mode = mode;
    state.pressed_keys.clear();
    state.scroll_remainder = Point::default();
}

fn release_mouse_buttons(state: &mut SharedState, actions: &mut Vec<Action>) {
    set_button_state(state, Button::Left, false, actions);
    set_button_state(state, Button::Right, false, actions);
}

fn set_button_state(
    state: &mut SharedState,
    button: Button,
    is_down: bool,
    actions: &mut Vec<Action>,
) {
    let button_state = match button {
        Button::Left => &mut state.left_button_down,
        Button::Right => &mut state.right_button_down,
        _ => return,
    };

    if *button_state == is_down {
        return;
    }

    *button_state = is_down;
    actions.push(if is_down {
        Action::ButtonPress(button)
    } else {
        Action::ButtonRelease(button)
    });
}

// Dispatch side effects after releasing the main state lock to avoid re-entrancy issues.
fn dispatch_actions(actions: &[Action]) {
    for action in actions {
        match action {
            Action::MouseMove(target) => {
                let _ = simulate_event(&EventType::MouseMove {
                    x: target.x,
                    y: target.y,
                });
            }
            Action::Scroll { delta_x, delta_y } => {
                let _ = simulate_event(&EventType::Wheel {
                    delta_x: *delta_x,
                    delta_y: *delta_y,
                });
            }
            Action::ButtonPress(button) => {
                let _ = simulate_event(&EventType::ButtonPress(*button));
            }
            Action::ButtonRelease(button) => {
                let _ = simulate_event(&EventType::ButtonRelease(*button));
            }
        }
    }
}

fn simulate_event(event: &EventType) -> Result<(), SimulateError> {
    let _guard = SIMULATE_LOCK.lock().expect("simulate lock poisoned");
    // A tiny delay helps some platforms keep up with back-to-back synthetic events.
    let result = simulate(event);
    thread::sleep(Duration::from_millis(1));
    result
}

fn dispatch_passthrough_events(hook_state: &Arc<Mutex<HookState>>, events: &[EventType]) {
    for event in events {
        {
            let mut hook = hook_state.lock().expect("hook state poisoned");
            record_synthetic_passthrough(&mut hook, *event);
        }
        let _ = simulate_event(event);
    }
}

fn exact_single_key(keys: &HashSet<Key>, expected: Key) -> bool {
    keys.len() == 1 && keys.contains(&expected)
}

fn has_deferred_modifier(hook_state: &HookState, key: Key) -> bool {
    hook_state
        .deferred_modifiers
        .iter()
        .any(|modifier| modifier.key == key)
}

fn take_deferred_modifier(hook_state: &mut HookState, key: Key) -> Option<DeferredModifier> {
    let index = hook_state
        .deferred_modifiers
        .iter()
        .position(|modifier| modifier.key == key)?;
    Some(hook_state.deferred_modifiers.remove(index))
}

fn mark_deferred_modifiers_used_by_suppressed_combo(hook_state: &mut HookState) {
    for modifier in &mut hook_state.deferred_modifiers {
        modifier.used_by_suppressed_combo = true;
    }
}

fn flush_deferred_modifiers(hook_state: &mut HookState, passthrough_events: &mut Vec<EventType>) {
    for modifier in hook_state.deferred_modifiers.drain(..) {
        passthrough_events.push(EventType::KeyPress(modifier.key));
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn record_synthetic_passthrough(hook_state: &mut HookState, event_type: EventType) {
    hook_state.synthetic_passthrough.push(event_type);
}

#[cfg(target_os = "linux")]
fn record_synthetic_passthrough(_hook_state: &mut HookState, _event_type: EventType) {}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn consume_synthetic_passthrough(hook_state: &mut HookState, event_type: EventType) -> bool {
    if hook_state.synthetic_passthrough.first() == Some(&event_type) {
        hook_state.synthetic_passthrough.remove(0);
        return true;
    }

    false
}

#[cfg(target_os = "linux")]
fn consume_synthetic_passthrough(_hook_state: &mut HookState, _event_type: EventType) -> bool {
    false
}

// Quit only when the chord is exactly Ctrl+Shift+Q, with no extra keys mixed in.
fn is_quit_chord(keys: &HashSet<Key>) -> bool {
    keys.contains(&KEY_QUIT)
        && keys.iter().any(|key| is_control_key(*key))
        && keys.iter().any(|key| is_shift_key(*key))
        && keys
            .iter()
            .all(|key| *key == KEY_QUIT || is_control_key(*key) || is_shift_key(*key))
}

fn should_suppress_normal_key_press(keys: &HashSet<Key>, key: Key) -> bool {
    is_reserved_normal_combo_key(key)
        || (is_single_key_binding(key) && exact_single_key(keys, key))
        || is_quit_chord(keys)
}

fn is_reserved_normal_combo_key(key: Key) -> bool {
    is_movement_key(key) || button_from_key(key).is_some()
}

fn is_single_key_binding(key: Key) -> bool {
    key == KEY_NORMAL_MODE || key == KEY_INSERT_MODE || key == KEY_CYCLE_MONITOR || jump_cell(key).is_some()
}

// Movement accepts Ctrl (fast) or Alt (slow) speed modifiers and held mouse buttons for dragging.
fn is_valid_move_set(keys: &HashSet<Key>) -> bool {
    let has_movement = keys.iter().any(|key| is_movement_key(*key));
    if !has_movement {
        return false;
    }

    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_control_key(*key)
            || is_alt_key(*key)
            || *key == KEY_LEFT_CLICK
            || *key == KEY_RIGHT_CLICK
    })
}

// Clicking is allowed alone or while moving so the user can drag with the keyboard.
fn is_valid_button_set(keys: &HashSet<Key>, button_key: Key) -> bool {
    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_control_key(*key)
            || is_alt_key(*key)
            || *key == KEY_LEFT_CLICK
            || *key == KEY_RIGHT_CLICK
    }) && keys.contains(&button_key)
}
/// Scrolling uses Shift+H/J/K/L. Ctrl (fast) and Alt (slow) modify scroll speed.
fn is_valid_scroll_set(keys: &HashSet<Key>) -> bool {
    let has_movement = keys.iter().any(|key| is_movement_key(*key));
    let has_shift = keys.iter().any(|key| is_shift_key(*key));
    if !has_movement || !has_shift {
        return false;
    }

    keys.iter()
        .all(|key| is_movement_key(*key) || is_shift_key(*key) || is_control_key(*key) || is_alt_key(*key))

}

// Normalize diagonal movement so holding two directions is not faster than one.
fn normalized_direction(keys: &HashSet<Key>) -> Option<Point> {
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;

    if keys.contains(&KEY_MOVE_LEFT) {
        x -= 1.0;
    }
    if keys.contains(&KEY_MOVE_RIGHT) {
        x += 1.0;
    }
    if keys.contains(&KEY_MOVE_DOWN) {
        y += 1.0;
    }
    if keys.contains(&KEY_MOVE_UP) {
        y -= 1.0;
    }

    let length = (x * x + y * y).sqrt();
    if length == 0.0 {
        None
    } else {
        Some(Point {
            x: x / length,
            y: y / length,
        })
    }
}

fn speed_multiplier(keys: &HashSet<Key>) -> f64 {
    let mut multiplier = 1.0;

    if keys.iter().any(|key| KEYS_FAST.contains(key)) {
        multiplier *= FAST_MULTIPLIER;
    }
    if keys.iter().any(|key| KEYS_SLOW.contains(key)) {
        multiplier *= SLOW_MULTIPLIER;
    }

    multiplier
}

// The quick-jump grid is laid out as a 5x3 matrix over the selected monitor.
fn jump_cell(key: Key) -> Option<(usize, usize)> {
    for (row, keys) in JUMP_GRID.iter().enumerate() {
        for (col, grid_key) in keys.iter().enumerate() {
            if *grid_key == key {
                return Some((col, row));
            }
        }
    }
    None
}

fn is_movement_key(key: Key) -> bool {
    key == KEY_MOVE_LEFT || key == KEY_MOVE_DOWN || key == KEY_MOVE_UP || key == KEY_MOVE_RIGHT
}

fn is_shift_key(key: Key) -> bool {
    KEYS_SCROLL.contains(&key)
}

fn is_alt_key(key: Key) -> bool {
    KEYS_SLOW.contains(&key)
}

fn is_control_key(key: Key) -> bool {
    KEYS_FAST.contains(&key)
}

fn is_meta_key(key: Key) -> bool {
    matches!(key, Key::MetaLeft | Key::MetaRight)
}

fn is_modifier_key(key: Key) -> bool {
    is_shift_key(key) || is_control_key(key) || is_alt_key(key) || is_meta_key(key)
}

fn button_from_key(key: Key) -> Option<Button> {
    if key == KEY_LEFT_CLICK {
        Some(Button::Left)
    } else if key == KEY_RIGHT_CLICK {
        Some(Button::Right)
    } else {
        None
    }
}
