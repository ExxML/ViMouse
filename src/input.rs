use crate::config::{
    FAST_MULTIPLIER, JUMP_GRID, KEYS_FAST, KEYS_QUIT_MODIFIERS, KEYS_SCROLL, KEYS_SLOW,
    KEY_CYCLE_MONITOR, KEY_INSERT_MODE, KEY_LEFT_CLICK, KEY_MOVE_DOWN, KEY_MOVE_LEFT,
    KEY_MOVE_RIGHT, KEY_MOVE_UP, KEY_NORMAL_MODE, KEY_QUIT, KEY_RIGHT_CLICK, MOVE_SPEED_PX_PER_SEC,
    SCROLL_SPEED_UNITS_PER_SEC, SLOW_MULTIPLIER, TICK_RATE_HZ,
};
use crate::monitor::{clamp_to_virtual_bounds, monitor_index_for_point};
use crate::platform_input::{
    set_normal_mode_key_remap, shutdown_platform_input, simulate_input, InputEmitter,
};
use crate::state::{Action, Mode, Point, Shared, SharedState};
#[cfg(not(target_os = "macos"))]
use rdev::grab;
use rdev::{Button, Event, EventType, Key};
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

const MOVE_KEYS: [Key; 4] = [KEY_MOVE_LEFT, KEY_MOVE_DOWN, KEY_MOVE_UP, KEY_MOVE_RIGHT];

#[derive(Default)]
struct HookTracker {
    held_keys: HashSet<Key>,
    captured_keys: HashSet<Key>,
    suppressed_modifiers: HashSet<Key>,
    passthrough_key_events: Vec<(Key, bool)>,
    pending_key_events: Vec<(Key, bool)>,
}

pub fn spawn_input_hook(shared: Shared) {
    thread::Builder::new()
        .name("vimouse-input-hook".to_string())
        .spawn(move || {
            let tracker = std::sync::Mutex::new(HookTracker::default());

            #[cfg(target_os = "macos")]
            {
                let normal_mode_active =
                    shared.lock().expect("shared state poisoned").mode == Mode::Normal;
                set_normal_mode_key_remap(normal_mode_active);
                crate::platform_input::macos_grab::run(move |event| {
                    handle_hook_event(&shared, &tracker, event)
                });
                shutdown_platform_input();
            }

            #[cfg(not(target_os = "macos"))]
            if let Err(error) = grab(move |event| handle_hook_event(&shared, &tracker, event)) {
                eprintln!("input hook error: {error:?}");
            }
        })
        .expect("failed to spawn input hook thread");
}

pub fn spawn_motion_loop(shared: Shared) {
    thread::Builder::new()
        .name("vimouse-motion-loop".to_string())
        .spawn(move || {
            let mut emitter = InputEmitter::new();
            let frame_time = Duration::from_secs_f64(1.0 / TICK_RATE_HZ as f64);
            let mut last_tick = Instant::now();
            let mut next_tick = last_tick + frame_time;

            loop {
                // Drive movement from elapsed time instead of key-repeat cadence so hold-to-move
                // feels consistent on different keyboards and refresh rates.
                let now = Instant::now();
                let delta_seconds = now
                    .saturating_duration_since(last_tick)
                    .as_secs_f64()
                    .min(0.050);
                last_tick = now;

                let actions = collect_pending_actions(&shared, delta_seconds);
                emitter.emit_all(&actions);

                let now = Instant::now();
                if next_tick > now {
                    thread::sleep(next_tick - now);
                    next_tick += frame_time;
                } else {
                    next_tick = now + frame_time;
                }
            }
        })
        .expect("failed to spawn motion loop thread");
}

fn handle_hook_event(
    shared: &Shared,
    tracker: &std::sync::Mutex<HookTracker>,
    event: Event,
) -> Option<Event> {
    match event.event_type {
        EventType::KeyPress(key) => handle_key_event(shared, tracker, event, key, true),
        EventType::KeyRelease(key) => handle_key_event(shared, tracker, event, key, false),
        EventType::MouseMove { x, y } => {
            let mut state = shared.lock().expect("shared state poisoned");
            update_cursor(&mut state, Point { x, y });
            Some(event)
        }
        _ => Some(event),
    }
}

fn handle_key_event(
    shared: &Shared,
    tracker: &std::sync::Mutex<HookTracker>,
    event: Event,
    key: Key,
    is_press: bool,
) -> Option<Event> {
    if take_passthrough_key_event(tracker, key, is_press) {
        return Some(event);
    }

    let captured = if is_press {
        handle_key_press(shared, tracker, key)
    } else {
        handle_key_release(shared, tracker, key)
    };

    emit_pending_key_events(tracker);

    if captured {
        None
    } else {
        Some(event)
    }
}

fn handle_key_press(shared: &Shared, tracker: &std::sync::Mutex<HookTracker>, key: Key) -> bool {
    let mut tracker = tracker.lock().expect("hook tracker poisoned");
    let is_repeat = !tracker.held_keys.insert(key);

    let mut state = shared.lock().expect("shared state poisoned");
    update_runtime_modifier_state(&mut state, key, true);

    if is_repeat {
        return tracker.captured_keys.contains(&key);
    }

    let should_capture = match state.mode {
        Mode::Insert => key == KEY_NORMAL_MODE && no_modifiers_held(&tracker.held_keys),
        Mode::Normal => {
            // Identify quit chord instead of a jump-grid Q press.
            if quit_chord_active(&tracker.held_keys, key) {
                shutdown_platform_input();
                std::process::exit(0);
            }

            // Suppress runtime modifiers while moving
            if is_runtime_modifier(key) && movement_active(&state.pressed_keys) {
                true
            }
            // If a non-ViMouse key started the chord, let the rest of that chord pass through.
            else if has_uncaptured_non_modifier(&tracker, key) {
                false
            } else if is_move_key(key) {
                true
            } else if key == KEY_INSERT_MODE
                || key == KEY_NORMAL_MODE
                || key == KEY_CYCLE_MONITOR
                || key == KEY_LEFT_CLICK
                || key == KEY_RIGHT_CLICK
                || is_jump_key(key)
            {
                no_modifiers_held(&tracker.held_keys)
            } else {
                false
            }
        }
    };

    if !should_capture {
        return false;
    }

    tracker.captured_keys.insert(key);

    match state.mode {
        Mode::Insert => enter_normal_mode(&mut state, &tracker.held_keys),
        Mode::Normal => apply_normal_mode_press(&mut state, key),
    }
    sync_runtime_modifier_suppression(&state, &mut tracker);

    true
}

fn handle_key_release(shared: &Shared, tracker: &std::sync::Mutex<HookTracker>, key: Key) -> bool {
    let mut tracker = tracker.lock().expect("hook tracker poisoned");
    tracker.held_keys.remove(&key);
    let was_captured = tracker.captured_keys.remove(&key);
    let was_suppressed = tracker.suppressed_modifiers.contains(&key);

    let mut state = shared.lock().expect("shared state poisoned");
    update_runtime_modifier_state(&mut state, key, false);

    if was_captured {
        match key {
            KEY_MOVE_LEFT | KEY_MOVE_DOWN | KEY_MOVE_UP | KEY_MOVE_RIGHT => {
                state.pressed_keys.remove(&key);
            }
            KEY_LEFT_CLICK => release_mouse_button(&mut state, Button::Left),
            KEY_RIGHT_CLICK => release_mouse_button(&mut state, Button::Right),
            _ => {}
        }
    }

    sync_runtime_modifier_suppression(&state, &mut tracker);

    was_captured || was_suppressed
}

fn apply_normal_mode_press(state: &mut SharedState, key: Key) {
    match key {
        KEY_INSERT_MODE => enter_insert_mode(state),
        KEY_NORMAL_MODE => {}
        KEY_CYCLE_MONITOR => cycle_monitor(state),
        KEY_LEFT_CLICK => press_mouse_button(state, Button::Left),
        KEY_RIGHT_CLICK => press_mouse_button(state, Button::Right),
        KEY_MOVE_LEFT | KEY_MOVE_DOWN | KEY_MOVE_UP | KEY_MOVE_RIGHT => {
            state.pressed_keys.insert(key);
        }
        _ if is_jump_key(key) => queue_jump(state, key),
        _ => {}
    }
}

fn enter_insert_mode(state: &mut SharedState) {
    state.mode = Mode::Insert;
    set_normal_mode_key_remap(false);
    state.pressed_keys.clear();
    state.scroll_remainder = Point::default();
    release_mouse_button(state, Button::Left);
    release_mouse_button(state, Button::Right);
}

fn enter_normal_mode(state: &mut SharedState, held_keys: &HashSet<Key>) {
    state.mode = Mode::Normal;
    set_normal_mode_key_remap(true);
    state.pressed_keys.clear();
    state.scroll_remainder = Point::default();

    for key in held_keys {
        if is_runtime_modifier(*key) {
            state.pressed_keys.insert(*key);
        }
    }

    if let Some(index) = monitor_index_for_point(&state.monitors, state.cursor) {
        state.selected_monitor = index;
    }
}

fn cycle_monitor(state: &mut SharedState) {
    if state.monitors.is_empty() {
        return;
    }

    state.selected_monitor = (state.selected_monitor + 1) % state.monitors.len();
}

fn queue_jump(state: &mut SharedState, key: Key) {
    let Some(monitor) = state.monitors.get(state.selected_monitor).copied() else {
        return;
    };

    let Some(target) = jump_target(monitor, key) else {
        return;
    };

    update_cursor(state, target);
    state.pending_actions.push(Action::MouseMove(state.cursor));
}

fn press_mouse_button(state: &mut SharedState, button: Button) {
    match button {
        Button::Left if !state.left_button_down => {
            state.left_button_down = true;
            state
                .pending_actions
                .push(Action::ButtonPress(Button::Left));
        }
        Button::Right if !state.right_button_down => {
            state.right_button_down = true;
            state
                .pending_actions
                .push(Action::ButtonPress(Button::Right));
        }
        _ => {}
    }
}

fn release_mouse_button(state: &mut SharedState, button: Button) {
    match button {
        Button::Left if state.left_button_down => {
            state.left_button_down = false;
            state
                .pending_actions
                .push(Action::ButtonRelease(Button::Left));
        }
        Button::Right if state.right_button_down => {
            state.right_button_down = false;
            state
                .pending_actions
                .push(Action::ButtonRelease(Button::Right));
        }
        _ => {}
    }
}

fn update_runtime_modifier_state(state: &mut SharedState, key: Key, is_down: bool) {
    if !is_runtime_modifier(key) {
        return;
    }

    if is_down {
        if state.mode == Mode::Normal {
            state.pressed_keys.insert(key);
        }
    } else {
        state.pressed_keys.remove(&key);
    }
}

#[cfg(target_os = "macos")]
fn sync_runtime_modifier_suppression(_state: &SharedState, tracker: &mut HookTracker) {
    // Keep the macOS hook simple: avoid replaying keyboard events from inside the event tap.
    tracker.pending_key_events.clear();
    tracker.suppressed_modifiers.clear();
}

#[cfg(not(target_os = "macos"))]
// Keep runtime modifiers active for ViMouse itself while making them temporarily invisible to
// the OS whenever captured movement is in progress.
fn sync_runtime_modifier_suppression(state: &SharedState, tracker: &mut HookTracker) {
    let desired_modifiers = if movement_active(&state.pressed_keys) {
        tracker
            .held_keys
            .iter()
            .copied()
            .filter(|key| is_runtime_modifier(*key))
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    let modifiers_to_suppress = desired_modifiers
        .iter()
        .copied()
        .filter(|key| !tracker.suppressed_modifiers.contains(key))
        .collect::<Vec<_>>();
    let modifiers_to_restore = tracker
        .suppressed_modifiers
        .iter()
        .copied()
        .filter(|key| !desired_modifiers.contains(key))
        .collect::<Vec<_>>();

    for key in modifiers_to_suppress {
        tracker.suppressed_modifiers.insert(key);

        if !tracker.captured_keys.contains(&key) {
            tracker.pending_key_events.push((key, false));
        }
    }

    for key in modifiers_to_restore {
        tracker.suppressed_modifiers.remove(&key);

        if tracker.held_keys.contains(&key) {
            tracker.pending_key_events.push((key, true));
            tracker.captured_keys.remove(&key);
        }
    }
}

fn collect_pending_actions(shared: &Shared, delta_seconds: f64) -> Vec<Action> {
    let mut state = shared.lock().expect("shared state poisoned");
    // The hook thread only mutates state; all synthetic mouse output is emitted here so
    // cursor movement, clicks, and scrolling stay serialized and predictable.
    let mut actions = Vec::with_capacity(state.pending_actions.len() + 2);
    actions.append(&mut state.pending_actions);

    if state.mode != Mode::Normal {
        state.scroll_remainder = Point::default();
        return actions;
    }

    let direction = normalized_direction(&state.pressed_keys);
    if direction.x == 0.0 && direction.y == 0.0 {
        state.scroll_remainder = Point::default();
        return actions;
    }

    let speed_multiplier = movement_multiplier(&state.pressed_keys);
    if scroll_mode_active(&state.pressed_keys) {
        // Keep fractional scroll remainder so slower motion still feels steady.
        state.scroll_remainder.x +=
            direction.x * SCROLL_SPEED_UNITS_PER_SEC * speed_multiplier * delta_seconds;
        state.scroll_remainder.y +=
            -direction.y * SCROLL_SPEED_UNITS_PER_SEC * speed_multiplier * delta_seconds;

        let delta_x = take_scroll_steps(&mut state.scroll_remainder.x);
        let delta_y = take_scroll_steps(&mut state.scroll_remainder.y);

        if delta_x != 0 || delta_y != 0 {
            actions.push(Action::Scroll { delta_x, delta_y });
        }

        return actions;
    }

    state.scroll_remainder = Point::default();

    let previous_cursor = state.cursor;
    let mut next_cursor = previous_cursor;
    next_cursor.x += direction.x * MOVE_SPEED_PX_PER_SEC * speed_multiplier * delta_seconds;
    next_cursor.y += direction.y * MOVE_SPEED_PX_PER_SEC * speed_multiplier * delta_seconds;
    clamp_to_virtual_bounds(&mut next_cursor, &state.monitors);

    if next_cursor != previous_cursor {
        state.cursor = next_cursor;

        if let Some(index) = monitor_index_for_point(&state.monitors, state.cursor) {
            state.selected_monitor = index;
        }

        actions.push(Action::MouseMove(state.cursor));
    }

    actions
}

fn update_cursor(state: &mut SharedState, point: Point) {
    let mut clamped = point;
    clamp_to_virtual_bounds(&mut clamped, &state.monitors);
    state.cursor = clamped;

    if let Some(index) = monitor_index_for_point(&state.monitors, clamped) {
        state.selected_monitor = index;
    }
}

fn normalized_direction(keys: &HashSet<Key>) -> Point {
    let horizontal =
        (keys.contains(&KEY_MOVE_RIGHT) as i8 - keys.contains(&KEY_MOVE_LEFT) as i8) as f64;
    let vertical = (keys.contains(&KEY_MOVE_DOWN) as i8 - keys.contains(&KEY_MOVE_UP) as i8) as f64;

    let length = (horizontal * horizontal + vertical * vertical).sqrt();
    if length == 0.0 {
        Point::default()
    } else {
        Point {
            x: horizontal / length,
            y: vertical / length,
        }
    }
}

fn movement_multiplier(keys: &HashSet<Key>) -> f64 {
    let mut multiplier = 1.0;

    if contains_any(keys, KEYS_FAST) {
        multiplier *= FAST_MULTIPLIER;
    }

    if contains_any(keys, KEYS_SLOW) {
        multiplier *= SLOW_MULTIPLIER;
    }

    multiplier
}

fn take_scroll_steps(remainder: &mut f64) -> i64 {
    let whole = remainder.trunc() as i64;
    *remainder -= whole as f64;
    whole
}

fn jump_target(monitor: crate::state::MonitorInfo, key: Key) -> Option<Point> {
    for (row, keys) in JUMP_GRID.iter().enumerate() {
        for (column, cell_key) in keys.iter().enumerate() {
            if *cell_key != key {
                continue;
            }

            let cell_width = monitor.width / JUMP_GRID[0].len() as f64;
            let cell_height = monitor.height / JUMP_GRID.len() as f64;
            return Some(Point {
                x: monitor.origin.x + (column as f64 + 0.5) * cell_width,
                y: monitor.origin.y + (row as f64 + 0.5) * cell_height,
            });
        }
    }

    None
}

fn contains_any(keys: &HashSet<Key>, candidates: &[Key]) -> bool {
    candidates.iter().any(|candidate| keys.contains(candidate))
}

fn emit_pending_key_events(tracker: &std::sync::Mutex<HookTracker>) {
    let events = {
        let mut tracker = tracker.lock().expect("hook tracker poisoned");
        std::mem::take(&mut tracker.pending_key_events)
    };

    for (key, is_press) in events {
        if let Err(error) = emit_synthetic_key_event(tracker, key, is_press) {
            eprintln!("key emit error: {error}");
        }
    }
}

fn emit_synthetic_key_event(
    tracker: &std::sync::Mutex<HookTracker>,
    key: Key,
    is_press: bool,
) -> Result<(), String> {
    let event_type = synthetic_key_event_type(key, is_press);

    mark_passthrough_key_event(tracker, key, is_press);

    if let Err(error) = simulate_input(&event_type) {
        clear_passthrough_key_event(tracker, key, is_press);
        return Err(error);
    }

    Ok(())
}

fn synthetic_key_event_type(key: Key, is_press: bool) -> EventType {
    if is_press {
        EventType::KeyPress(key)
    } else {
        EventType::KeyRelease(key)
    }
}

fn take_passthrough_key_event(
    tracker: &std::sync::Mutex<HookTracker>,
    key: Key,
    is_press: bool,
) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        // Low-level hooks on Windows/macOS observe our own replayed key events, so skip any
        // internal state updates and let those synthetic events continue to the OS.
        let mut tracker = tracker.lock().expect("hook tracker poisoned");

        if let Some(index) = tracker
            .passthrough_key_events
            .iter()
            .position(|event| *event == (key, is_press))
        {
            tracker.passthrough_key_events.swap_remove(index);
            return true;
        }
    }

    false
}

fn mark_passthrough_key_event(tracker: &std::sync::Mutex<HookTracker>, key: Key, is_press: bool) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let mut tracker = tracker.lock().expect("hook tracker poisoned");
        tracker.passthrough_key_events.push((key, is_press));
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (tracker, key, is_press);
    }
}

fn clear_passthrough_key_event(tracker: &std::sync::Mutex<HookTracker>, key: Key, is_press: bool) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let mut tracker = tracker.lock().expect("hook tracker poisoned");

        if let Some(index) = tracker
            .passthrough_key_events
            .iter()
            .position(|event| *event == (key, is_press))
        {
            tracker.passthrough_key_events.swap_remove(index);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (tracker, key, is_press);
    }
}

fn quit_chord_active(held_keys: &HashSet<Key>, current_key: Key) -> bool {
    current_key == KEY_QUIT
        && contains_any(held_keys, &[Key::ControlLeft, Key::ControlRight])
        && contains_any(held_keys, &[Key::ShiftLeft, Key::ShiftRight])
        && held_keys
            .iter()
            .all(|key| *key == KEY_QUIT || KEYS_QUIT_MODIFIERS.contains(key))
}

fn no_modifiers_held(keys: &HashSet<Key>) -> bool {
    !keys.iter().any(|key| is_modifier_key(*key))
}

fn has_uncaptured_non_modifier(tracker: &HookTracker, key: Key) -> bool {
    tracker.held_keys.iter().any(|held_key| {
        *held_key != key && !is_modifier_key(*held_key) && !tracker.captured_keys.contains(held_key)
    })
}

fn scroll_mode_active(keys: &HashSet<Key>) -> bool {
    contains_any(keys, KEYS_SCROLL)
}

fn movement_active(keys: &HashSet<Key>) -> bool {
    MOVE_KEYS.iter().any(|key| keys.contains(key))
}

fn is_move_key(key: Key) -> bool {
    MOVE_KEYS.contains(&key)
}

fn is_jump_key(key: Key) -> bool {
    JUMP_GRID
        .iter()
        .flatten()
        .any(|candidate| *candidate == key)
}

fn is_modifier_key(key: Key) -> bool {
    is_runtime_modifier(key)
}

fn is_runtime_modifier(key: Key) -> bool {
    KEYS_SCROLL.contains(&key) || KEYS_FAST.contains(&key) || KEYS_SLOW.contains(&key)
}
