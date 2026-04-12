use crate::config::{
    FAST_MULTIPLIER, MOVE_SPEED_PX_PER_SEC, SCROLL_SPEED_UNITS_PER_SEC, SLOW_MULTIPLIER,
    TICK_RATE_HZ,
};
use crate::monitor::{clamp_to_virtual_bounds, monitor_index_for_point};
use crate::state::{Action, Mode, Point, Shared, SharedState};
use rdev::{listen, simulate, Button, Event, EventType, Key, SimulateError};
use std::collections::HashSet;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

// rdev simulation is global OS state, so serialize synthetic events.
static SIMULATE_LOCK: Mutex<()> = Mutex::new(());

pub fn spawn_input_hook(shared: Shared) {
    thread::Builder::new()
        .name("vimouse-hook".into())
        .spawn(move || {
            let hook_shared = shared.clone();
            let callback = move |event: Event| handle_input_event(&hook_shared, event);
            if let Err(error) = listen(callback) {
                eprintln!("global input listen failed: {error:?}");
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

fn handle_input_event(shared: &Shared, event: Event) {
    let mut actions = Vec::new();
    {
        let mut state = shared.lock().expect("shared state poisoned");
        match event.event_type {
            EventType::MouseMove { x, y } => {
                update_cursor(&mut state, Point { x, y });
            }
            EventType::KeyPress(key) => handle_key_press(&mut state, key, &mut actions),
            EventType::KeyRelease(key) => {
                state.pressed_keys.remove(&key);
                handle_key_release(&mut state, key, &mut actions);
            }
            _ => {}
        }
    }

    dispatch_actions(&actions);
}

fn update_cursor(state: &mut SharedState, cursor: Point) {
    state.cursor = cursor;
    if let Some(index) = monitor_index_for_point(&state.monitors, cursor) {
        state.selected_monitor = index;
    }
}

fn handle_key_press(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) {
    let is_repeat = !state.pressed_keys.insert(key);
    if !is_repeat && is_quit_chord(&state.pressed_keys) {
        std::process::exit(0);
    }

    match state.mode {
        Mode::Insert => handle_insert_key_press(state, key, actions),
        Mode::Normal => handle_normal_key_press(state, key, is_repeat, actions),
    }
}

fn handle_key_release(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) {
    match state.mode {
        Mode::Insert => {}
        Mode::Normal => handle_normal_key_release(state, key, actions),
    }
}

fn handle_insert_key_press(state: &mut SharedState, key: Key, actions: &mut Vec<Action>) {
    if key == Key::Escape {
        set_mode(state, Mode::Normal, actions);
    }
}

fn handle_normal_key_press(
    state: &mut SharedState,
    key: Key,
    is_repeat: bool,
    actions: &mut Vec<Action>,
) {
    if key == Key::Escape {
        set_mode(state, Mode::Normal, actions);
        return;
    }

    if key == Key::KeyI && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        set_mode(state, Mode::Insert, actions);
        return;
    }

    if key == Key::BackQuote && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        cycle_monitor(state);
        return;
    }

    if !is_repeat && exact_single_key(&state.pressed_keys, key) && jump_to_cell(state, key, actions)
    {
        return;
    }

    if let Some(button) = button_from_key(key) {
        if is_valid_button_set(&state.pressed_keys, key) {
            set_button_state(state, button, true, actions);
        }
    }
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

fn exact_single_key(keys: &HashSet<Key>, expected: Key) -> bool {
    keys.len() == 1 && keys.contains(&expected)
}

// Quit only when the chord is exactly Ctrl+Shift+Q, with no extra keys mixed in.
fn is_quit_chord(keys: &HashSet<Key>) -> bool {
    keys.contains(&Key::KeyQ)
        && keys.iter().any(|key| is_control_key(*key))
        && keys.iter().any(|key| is_shift_key(*key))
        && keys
            .iter()
            .all(|key| *key == Key::KeyQ || is_control_key(*key) || is_shift_key(*key))
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
            || *key == Key::SemiColon
            || *key == Key::CapsLock
    })
}

// Clicking is allowed alone or while moving so the user can drag with the keyboard.
fn is_valid_button_set(keys: &HashSet<Key>, button_key: Key) -> bool {
    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_control_key(*key)
            || is_alt_key(*key)
            || *key == Key::SemiColon
            || *key == Key::CapsLock
    }) && keys.contains(&button_key)
}

// Scrolling uses Shift+H/J/K/L.
fn is_valid_scroll_set(keys: &HashSet<Key>) -> bool {
    let has_movement = keys.iter().any(|key| is_movement_key(*key));
    let has_shift = keys.iter().any(|key| is_shift_key(*key));
    if !has_movement || !has_shift {
        return false;
    }

    keys.iter().all(|key| is_movement_key(*key) || is_shift_key(*key))
}

// Normalize diagonal movement so holding two directions is not faster than one.
fn normalized_direction(keys: &HashSet<Key>) -> Option<Point> {
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;

    if keys.contains(&Key::KeyH) {
        x -= 1.0;
    }
    if keys.contains(&Key::KeyL) {
        x += 1.0;
    }
    if keys.contains(&Key::KeyJ) {
        y += 1.0;
    }
    if keys.contains(&Key::KeyK) {
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

    if keys.iter().any(|key| is_control_key(*key)) {
        multiplier *= FAST_MULTIPLIER;
    }
    if keys.iter().any(|key| is_alt_key(*key)) {
        multiplier *= SLOW_MULTIPLIER;
    }

    multiplier
}

// The quick-jump grid is laid out as a 5x3 matrix over the selected monitor.
fn jump_cell(key: Key) -> Option<(usize, usize)> {
    match key {
        Key::KeyQ => Some((0, 0)),
        Key::KeyW => Some((1, 0)),
        Key::KeyE => Some((2, 0)),
        Key::KeyR => Some((3, 0)),
        Key::KeyT => Some((4, 0)),
        Key::KeyA => Some((0, 1)),
        Key::KeyS => Some((1, 1)),
        Key::KeyD => Some((2, 1)),
        Key::KeyF => Some((3, 1)),
        Key::KeyG => Some((4, 1)),
        Key::KeyZ => Some((0, 2)),
        Key::KeyX => Some((1, 2)),
        Key::KeyC => Some((2, 2)),
        Key::KeyV => Some((3, 2)),
        Key::KeyB => Some((4, 2)),
        _ => None,
    }
}

fn is_movement_key(key: Key) -> bool {
    matches!(key, Key::KeyH | Key::KeyJ | Key::KeyK | Key::KeyL)
}

fn is_shift_key(key: Key) -> bool {
    matches!(key, Key::ShiftLeft | Key::ShiftRight)
}

fn is_alt_key(key: Key) -> bool {
    matches!(key, Key::Alt | Key::AltGr)
}

fn is_control_key(key: Key) -> bool {
    matches!(key, Key::ControlLeft | Key::ControlRight)
}

fn button_from_key(key: Key) -> Option<Button> {
    match key {
        Key::SemiColon => Some(Button::Left),
        Key::CapsLock => Some(Button::Right),
        _ => None,
    }
}
