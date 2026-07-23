use crate::config::{
    ACCEL_DELAY_SECS, CHORD_QUIT, CURSOR_ACCELERATION, CURSOR_BASE_SPEED, CURSOR_MAX_SPEED,
    FAST_MULTIPLIER, INSERT_MODE_HIDE_CURSOR, JUMP_GRID, JUMP_GRID_DELAY, KEYS_EXEMPT, KEYS_MARK,
    KEY_CYCLE_MONITOR, KEY_FAST, KEY_INSERT_MODE, KEY_MOUSE_1, KEY_MOUSE_2, KEY_MOUSE_3,
    KEY_MOUSE_4, KEY_MOUSE_5, KEY_MOVE_DOWN, KEY_MOVE_LEFT, KEY_MOVE_RIGHT, KEY_MOVE_UP,
    KEY_NORMAL_MODE, KEY_SCROLL, KEY_SLOW, KEY_TOGGLE_GRID, KEY_TOGGLE_GRID_LETTERS,
    KEY_TOGGLE_OVERLAY, KEY_UNMARK, KEY_UNMARK_ALL, SCROLL_ACCELERATION, SCROLL_BASE_SPEED,
    SCROLL_MAX_SPEED, SLOW_MULTIPLIER, TICK_RATE_HZ,
};
use crate::monitor::{clamp_and_find_monitor, monitor_index_for_point};
#[cfg(target_os = "macos")]
use crate::platform_input::set_caps_lock_remap;
use crate::platform_input::{
    movement_device_scale, scroll_direction_sign, shutdown_platform_input, simulate_input,
    InputEmitter, BUTTON_MOUSE_4, BUTTON_MOUSE_5,
};
use crate::state::{Action, Mode, MotionWaker, Point, Shared, SharedState, UiWaker};
#[cfg(not(target_os = "macos"))]
use rdev::grab;
use rdev::{Button, Event, EventType, Key};
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

const MOVE_KEYS: [Key; 4] = [KEY_MOVE_LEFT, KEY_MOVE_DOWN, KEY_MOVE_UP, KEY_MOVE_RIGHT];

// Key ownership model
//
// Captured: dropped from the OS stream; ViMouse acts on it (or it is reserved-dead). Decided
// at press time and sticks for the key's lifetime - a captured key's release is dropped too.
//
// Forwarded: an OS modifier (Ctrl/Alt/Shift/Meta) pressed while ViMouse is idle is passed to
// the OS and recorded in press order. While any forwarded modifier is held ("leak mode"),
// other keys pass through so OS shortcuts keep working - with a few exceptions, see
// handle_key_press.
//
// Swallowed: an OS modifier pressed while a ViMouse action is active (any captured key held)
// is hidden from the OS until its physical release; the release is hidden too.
//
// Ghosted: a forwarded movement modifier is temporarily hidden with a synthetic key-release
// while ViMouse moves or scrolls, then restored with a synthetic key-press - see
// sync_movement_modifier_ghosting.
#[derive(Default)]
struct HookTracker {
    held_keys: HashSet<Key>,
    captured_keys: HashSet<Key>,
    // OS modifiers currently forwarded to the OS, oldest press first.
    forwarded_modifiers: Vec<Key>,
    swallowed_modifiers: HashSet<Key>,
    ghosted_modifiers: HashSet<Key>,
    // Windows/macOS only: their low-level hooks re-observe our synthetic events; Linux's grab doesn't.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    passthrough_key_events: Vec<(Key, bool)>,
    pending_key_events: Vec<(Key, bool)>,
}

pub fn spawn_input_hook(shared: Shared, waker: MotionWaker, ui_waker: UiWaker) {
    thread::Builder::new()
        .name("vimouse-input-hook".to_string())
        .stack_size(512 * 1024)
        .spawn(move || {
            let tracker = std::sync::Mutex::new(HookTracker::default());

            #[cfg(target_os = "macos")]
            {
                if crate::caps_lock_remap::caps_lock_used_in_config() {
                    crate::caps_lock_remap::turn_off_caps_lock();
                    set_caps_lock_remap(true);
                }
                crate::platform_input::macos_grab::run(move |event| {
                    handle_hook_event(&shared, &tracker, &waker, &ui_waker, event)
                });
                shutdown_platform_input();
            }

            #[cfg(not(target_os = "macos"))]
            if let Err(error) =
                grab(move |event| handle_hook_event(&shared, &tracker, &waker, &ui_waker, event))
            {
                eprintln!("input hook error: {error:?}");
            }
        })
        .expect("failed to spawn input hook thread");
}

pub fn spawn_motion_loop(shared: Shared, waker: MotionWaker, ui_waker: UiWaker) {
    thread::Builder::new()
        .name("vimouse-motion-loop".to_string())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut emitter = InputEmitter::new();
            let frame_time = Duration::from_secs_f64(1.0 / TICK_RATE_HZ as f64);
            let mut last_tick = Instant::now();
            let mut next_tick = last_tick + frame_time;
            let mut action_buf: Vec<Action> = Vec::with_capacity(8);
            let mut last_monitor = shared
                .lock()
                .expect("shared state poisoned")
                .selected_monitor;

            loop {
                // Park until the input hook signals that motion is needed.
                {
                    let guard = shared.lock().expect("shared state poisoned");
                    let _guard = waker
                        .wait_while(guard, |s| !s.motion_needed)
                        .expect("condvar wait failed");
                }

                // Drive movement from elapsed time instead of key-repeat cadence so hold-to-move
                // feels consistent on different keyboards and refresh rates.
                let now = Instant::now();
                let delta_seconds = now
                    .saturating_duration_since(last_tick)
                    .as_secs_f64()
                    .min(0.050);
                last_tick = now;

                collect_pending_actions(&shared, delta_seconds, &mut action_buf);
                emitter.emit_all(&action_buf);
                action_buf.clear();
                let current_monitor = shared
                    .lock()
                    .expect("shared state poisoned")
                    .selected_monitor;
                if current_monitor != last_monitor {
                    last_monitor = current_monitor;
                    let _ = ui_waker.send_event(());
                }

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
    waker: &MotionWaker,
    ui_waker: &UiWaker,
    event: Event,
) -> Option<Event> {
    match event.event_type {
        EventType::KeyPress(key) => {
            handle_key_event(shared, tracker, waker, ui_waker, event, key, true)
        }
        EventType::KeyRelease(key) => {
            handle_key_event(shared, tracker, waker, ui_waker, event, key, false)
        }
        EventType::MouseMove { x, y } => {
            let mut state = shared.lock().expect("shared state poisoned");
            let prev_monitor = state.selected_monitor;
            update_cursor(&mut state, Point { x, y });
            let monitor_changed = state.selected_monitor != prev_monitor;
            drop(state);
            if monitor_changed {
                let _ = ui_waker.send_event(());
            }
            Some(event)
        }
        _ => Some(event),
    }
}

fn handle_key_event(
    shared: &Shared,
    tracker: &std::sync::Mutex<HookTracker>,
    waker: &MotionWaker,
    ui_waker: &UiWaker,
    event: Event,
    key: Key,
    is_press: bool,
) -> Option<Event> {
    if take_passthrough_key_event(tracker, key, is_press) {
        return Some(event);
    }

    let captured = if is_press {
        handle_key_press(shared, tracker, key, waker, ui_waker)
    } else {
        handle_key_release(shared, tracker, key, waker)
    };

    emit_pending_key_events(tracker);

    if captured {
        None
    } else {
        Some(event)
    }
}

// Returns true if the key press is captured (dropped from the OS stream).
//
// Normal mode owns the keyboard: every key is captured by default. The exceptions:
//   - KEY_NORMAL_MODE is always captured, in both modes, regardless of modifiers.
//   - Exempt keys (KEYS_EXEMPT + keys rdev can't map) always pass through.
//   - KEY_TOGGLE_OVERLAY is captured (in both modes) only while no forwarded modifier is held.
//   - OS modifiers are never captured: idle presses are forwarded to the OS and recorded in
//     press order; presses during a ViMouse action (any captured key held) are swallowed -
//     hidden from the OS until physically released.
//   - Leak mode (any forwarded modifier held): keys pass through so OS chords like Ctrl+C or
//     Ctrl+Shift+T keep working. Still captured in leak mode:
//       - Move keys while the oldest forwarded modifier is a movement modifier: Shift+J
//         scrolls, Alt+J moves slowly. Later-pressed foreign modifiers stay forwarded, so
//         Shift then Ctrl then J emits Ctrl+scroll.
//       - KEY_MOUSE_1/2: the synthetic click carries whatever modifiers the OS still sees
//         (Ctrl+click, Shift+click); movement modifiers are ghosted while moving, so a
//         mid-move click is a plain click (see sync_movement_modifier_ghosting).
//       - Mark keys and KEY_UNMARK_ALL while KEY_UNMARK is active (unmark / unmark all,
//         see unmark_held).
//   - The quit chord fires in both modes, regardless of modifiers.
fn handle_key_press(
    shared: &Shared,
    tracker: &std::sync::Mutex<HookTracker>,
    key: Key,
    waker: &MotionWaker,
    ui_waker: &UiWaker,
) -> bool {
    let mut tracker = tracker.lock().expect("hook tracker poisoned");
    let is_repeat = !tracker.held_keys.insert(key);

    let mut state = shared.lock().expect("shared state poisoned");
    update_runtime_modifier_state(&mut state, key, true);

    if is_repeat {
        // OS auto-repeats of hidden keys must stay hidden or they would re-assert the key.
        return tracker.captured_keys.contains(&key)
            || tracker.swallowed_modifiers.contains(&key)
            || tracker.ghosted_modifiers.contains(&key);
    }

    if quit_chord_active(&tracker.held_keys, key) {
        shutdown_platform_input();
        std::process::exit(0);
    }

    if is_os_modifier(key) {
        // A runtime modifier (Shift/Alt) pressed mid-action is swallowed so it can't corrupt a
        // synthetic scroll/click; ghosting restores it separately while moving. A foreign
        // modifier (Ctrl/Meta) always forwards into leak mode so OS chords formed after a
        // ViMouse key is held - e.g. hold J, then Cmd+C - still reach the OS intact.
        if state.mode == Mode::Normal
            && !tracker.captured_keys.is_empty()
            && is_runtime_modifier(key)
        {
            tracker.swallowed_modifiers.insert(key);
            return true;
        }
        tracker.forwarded_modifiers.push(key);
        return false;
    }

    let should_capture = if key == KEY_NORMAL_MODE {
        true
    } else if is_exempt_key(key) {
        false
    } else if key == KEY_TOGGLE_OVERLAY {
        tracker.forwarded_modifiers.is_empty()
    } else {
        match state.mode {
            Mode::Insert => false,
            Mode::Normal => normal_mode_captures(&tracker, key),
        }
    };

    let ui_changed = if should_capture {
        tracker.captured_keys.insert(key);

        // Snapshot UI-visible state before applying the key action so we can detect changes.
        let ui_before = ui_snapshot(&state);

        if key == KEY_NORMAL_MODE {
            if state.mode == Mode::Insert {
                enter_normal_mode(&mut state, &tracker.held_keys);
            }
        } else if key == KEY_TOGGLE_OVERLAY {
            state.show_overlays = !state.show_overlays;
        } else {
            if !is_jump_key(key) {
                state.pending_subcell = None;
            }
            apply_normal_mode_press(&mut state, key, &tracker);
        }

        ui_snapshot(&state) != ui_before
    } else {
        false
    };

    sync_movement_modifier_ghosting(&state, &mut tracker);

    state.motion_needed = true;
    drop(state);
    drop(tracker);
    waker.notify_one();

    if ui_changed {
        let _ = ui_waker.send_event(());
    }

    should_capture
}

// Capture decision for a non-modifier, non-exempt key press in Normal mode.
fn normal_mode_captures(tracker: &HookTracker, key: Key) -> bool {
    // Runtime modifiers that aren't OS modifiers (e.g. Space) never reach the OS.
    if is_runtime_modifier(key) {
        return true;
    }

    // Movement stays ours while the oldest forwarded modifier is a movement modifier (Shift+J
    // scrolls, Alt+J moves slowly); a leading foreign modifier means an OS chord like Ctrl+J.
    if is_move_key(key) {
        return tracker
            .forwarded_modifiers
            .first()
            .is_none_or(|first| is_runtime_modifier(*first));
    }

    // Clicks are always ours; forwarded modifiers carry into them (Ctrl+click, Shift+click).
    if matches!(key, KEY_MOUSE_1 | KEY_MOUSE_2) {
        return true;
    }

    if is_mark_key(key) || key == KEY_UNMARK_ALL {
        return tracker.forwarded_modifiers.is_empty() || unmark_held(tracker);
    }

    // Everything else - remaining ViMouse actions and unbound keys alike - is captured unless
    // a forwarded modifier puts us in leak mode.
    tracker.forwarded_modifiers.is_empty()
}

fn handle_key_release(
    shared: &Shared,
    tracker: &std::sync::Mutex<HookTracker>,
    key: Key,
    waker: &MotionWaker,
) -> bool {
    let mut tracker = tracker.lock().expect("hook tracker poisoned");
    tracker.held_keys.remove(&key);
    let was_captured = tracker.captured_keys.remove(&key);
    let was_swallowed = tracker.swallowed_modifiers.remove(&key);
    // A ghosted modifier's release stays hidden: the OS already saw its ghost key-up.
    let was_ghosted = tracker.ghosted_modifiers.remove(&key);
    tracker
        .forwarded_modifiers
        .retain(|modifier| *modifier != key);

    let mut state = shared.lock().expect("shared state poisoned");
    update_runtime_modifier_state(&mut state, key, false);

    let mut wake_motion = false;
    if was_captured {
        match key {
            KEY_MOVE_LEFT | KEY_MOVE_DOWN | KEY_MOVE_UP | KEY_MOVE_RIGHT => {
                state.pressed_keys.remove(&key);
                state.move_key_pressed_at.remove(&key);
                if movement_active(&state.pressed_keys) {
                    state.motion_needed = true;
                    wake_motion = true;
                }
            }
            KEY_MOUSE_1 => release_mouse_button(&mut state, Button::Left),
            KEY_MOUSE_2 => release_mouse_button(&mut state, Button::Right),
            KEY_MOUSE_3 => release_mouse_button(&mut state, Button::Middle),
            KEY_MOUSE_4 => release_mouse_button(&mut state, BUTTON_MOUSE_4),
            KEY_MOUSE_5 => release_mouse_button(&mut state, BUTTON_MOUSE_5),
            _ => {}
        }
        if is_mouse_key(key) {
            state.motion_needed = true;
            wake_motion = true;
        }
    }

    sync_movement_modifier_ghosting(&state, &mut tracker);
    drop(state);
    drop(tracker);

    if wake_motion {
        waker.notify_one();
    }

    was_captured || was_swallowed || was_ghosted
}

fn apply_normal_mode_press(state: &mut SharedState, key: Key, tracker: &HookTracker) {
    match key {
        KEY_INSERT_MODE => enter_insert_mode(state),
        KEY_CYCLE_MONITOR => cycle_monitor(state),
        KEY_MOUSE_1 => press_mouse_button(state, Button::Left),
        KEY_MOUSE_2 => press_mouse_button(state, Button::Right),
        KEY_MOUSE_3 => press_mouse_button(state, Button::Middle),
        KEY_MOUSE_4 => press_mouse_button(state, BUTTON_MOUSE_4),
        KEY_MOUSE_5 => press_mouse_button(state, BUTTON_MOUSE_5),
        KEY_TOGGLE_GRID => state.show_grid = !state.show_grid,
        KEY_TOGGLE_GRID_LETTERS => state.show_grid_letters = !state.show_grid_letters,
        KEY_MOVE_LEFT | KEY_MOVE_DOWN | KEY_MOVE_UP | KEY_MOVE_RIGHT => {
            state.pressed_keys.insert(key);
            state
                .move_key_pressed_at
                .entry(key)
                .or_insert_with(Instant::now);
        }
        _ if is_jump_key(key) => queue_jump(state, key),
        _ if key == KEY_UNMARK_ALL => {
            if unmark_held(tracker) {
                state.marks.clear();
            }
        }
        _ if is_mark_key(key) => apply_mark_press(state, key, unmark_held(tracker)),
        _ => {}
    }
}

// A mark key was pressed in Normal mode. With KEY_UNMARK held, remove the mark (no-op if
// absent). Otherwise jump to it if it exists, or set it at the current cursor if it doesn't.
fn apply_mark_press(state: &mut SharedState, key: Key, unmark_held: bool) {
    if unmark_held {
        state.marks.remove(&key);
        return;
    }

    if let Some(target) = state.marks.get(&key).copied() {
        update_cursor(state, target);
        state.pending_actions.push(Action::MouseMove(state.cursor));
    } else {
        state.marks.insert(key, state.cursor);
    }
}

fn enter_insert_mode(state: &mut SharedState) {
    state.mode = Mode::Insert;
    state.pressed_keys.clear();
    state.move_key_pressed_at.clear();
    release_all_buttons(state);
    if INSERT_MODE_HIDE_CURSOR {
        crate::cursor_visibility::set_cursor_hidden(true);
    }
}

fn enter_normal_mode(state: &mut SharedState, held_keys: &HashSet<Key>) {
    state.mode = Mode::Normal;
    if INSERT_MODE_HIDE_CURSOR {
        crate::cursor_visibility::set_cursor_hidden(false);
    }
    state.pressed_keys.clear();
    state.move_key_pressed_at.clear();

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

    let prev = state.selected_monitor;
    state.selected_monitor = (state.selected_monitor + 1) % state.monitors.len();

    // If there is only one monitor, do not move mouse to center of screen
    if state.selected_monitor == prev {
        return;
    }

    if let Some(monitor) = state.monitors.get(state.selected_monitor).copied() {
        state.cursor = monitor.center();
        state.pending_actions.push(Action::MouseMove(state.cursor));
    }
}

fn queue_jump(state: &mut SharedState, key: Key) {
    let Some(monitor) = state.monitors.get(state.selected_monitor).copied() else {
        return;
    };

    if let Some((cell_col, cell_row, pressed_at)) = state.pending_subcell.take() {
        if pressed_at.elapsed().as_secs_f64() <= JUMP_GRID_DELAY {
            if let Some(target) = subcell_target(monitor, cell_col, cell_row, key) {
                update_cursor(state, target);
                state.pending_actions.push(Action::MouseMove(state.cursor));
                return;
            }
        }
        // Timed out or key lookup failed - fall through to normal first-level jump.
    }

    let Some((target, col, row)) = jump_target_with_index(monitor, key) else {
        return;
    };

    update_cursor(state, target);
    state.pending_actions.push(Action::MouseMove(state.cursor));
    state.pending_subcell = Some((col, row, Instant::now()));
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
        Button::Middle | Button::Unknown(_) => {
            state.pending_actions.push(Action::ButtonPress(button));
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
        Button::Middle | Button::Unknown(_) => {
            state.pending_actions.push(Action::ButtonRelease(button));
        }
        _ => {}
    }
}

fn release_all_buttons(state: &mut SharedState) {
    release_mouse_button(state, Button::Left);
    release_mouse_button(state, Button::Right);
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

// Movement modifiers that double as OS modifiers (e.g. Shift as KEY_SCROLL, Alt as KEY_SLOW)
// are forwarded to the OS when pressed idle, but must be hidden while ViMouse moves or scrolls:
// a held Shift turns a vertical synthetic wheel horizontal, and mid-action clicks should not
// carry them. A ghost key-release hides them when movement starts; a ghost key-press restores
// them once movement stops. Forwarded foreign modifiers (Ctrl/Meta) are left visible so chords
// like Ctrl+scroll reach apps intact.
fn sync_movement_modifier_ghosting(state: &SharedState, tracker: &mut HookTracker) {
    // There are at most 3 runtime modifiers - use a stack array to avoid heap allocation.
    const RUNTIME_MODIFIERS: [Key; 3] = [KEY_SCROLL, KEY_FAST, KEY_SLOW];

    let moving = movement_active(&state.pressed_keys);

    for &key in &RUNTIME_MODIFIERS {
        if !is_os_modifier(key) {
            continue;
        }

        let want_ghosted = moving && tracker.forwarded_modifiers.contains(&key);
        let is_ghosted = tracker.ghosted_modifiers.contains(&key);

        if want_ghosted && !is_ghosted {
            tracker.ghosted_modifiers.insert(key);
            tracker.pending_key_events.push((key, false));
        } else if !want_ghosted && is_ghosted {
            tracker.ghosted_modifiers.remove(&key);
            tracker.pending_key_events.push((key, true));
        }
    }
}

fn collect_pending_actions(shared: &Shared, delta_seconds: f64, actions: &mut Vec<Action>) {
    let mut state = shared.lock().expect("shared state poisoned");
    // The hook thread only mutates state; all synthetic mouse output is emitted here so
    // cursor movement, clicks, and scrolling stay serialized and predictable.
    actions.append(&mut state.pending_actions);

    if state.mode != Mode::Normal {
        state.motion_needed = false;
        return;
    }

    let direction = normalized_direction(&state.pressed_keys);
    if direction.x == 0.0 && direction.y == 0.0 {
        state.motion_needed = false;
        return;
    }

    let speed_multiplier = movement_multiplier(&state.pressed_keys);
    if state.monitors.is_empty() {
        return;
    }

    let now = Instant::now();
    let elapsed_h = key_elapsed(&state, KEY_MOVE_LEFT, now);
    let elapsed_j = key_elapsed(&state, KEY_MOVE_DOWN, now);
    let elapsed_k = key_elapsed(&state, KEY_MOVE_UP, now);
    let elapsed_l = key_elapsed(&state, KEY_MOVE_RIGHT, now);
    // Per-axis elapsed: oldest held key wins so both directions in a diagonal accelerate independently.
    let elapsed_x = elapsed_h.max(elapsed_l);
    let elapsed_y = elapsed_j.max(elapsed_k);

    if scroll_mode_active(&state.pressed_keys) {
        let speed_x = acceleration_speed(
            elapsed_x,
            SCROLL_BASE_SPEED,
            SCROLL_ACCELERATION,
            SCROLL_MAX_SPEED,
        ) * speed_multiplier;
        let speed_y = acceleration_speed(
            elapsed_y,
            SCROLL_BASE_SPEED,
            SCROLL_ACCELERATION,
            SCROLL_MAX_SPEED,
        ) * speed_multiplier;
        // ±1 signs that cancel the OS scroll-direction setting so move keys always scroll the same
        // physical direction (see scroll_direction_sign; only Windows ever flips).
        let (sign_x, sign_y) = scroll_direction_sign();
        let delta_x = -direction.x * speed_x * delta_seconds * sign_x;
        let delta_y = -direction.y * speed_y * delta_seconds * sign_y;
        actions.push(Action::Scroll { delta_x, delta_y });
        return;
    }

    let speed_x = acceleration_speed(
        elapsed_x,
        CURSOR_BASE_SPEED,
        CURSOR_ACCELERATION,
        CURSOR_MAX_SPEED,
    ) * speed_multiplier;
    let speed_y = acceleration_speed(
        elapsed_y,
        CURSOR_BASE_SPEED,
        CURSOR_ACCELERATION,
        CURSOR_MAX_SPEED,
    ) * speed_multiplier;

    // Speeds are logical points/sec; scale the delta into the current monitor's device space so
    // apparent speed stays constant across monitors of differing DPI (see movement_device_scale).
    let device_scale = state
        .monitors
        .get(state.selected_monitor)
        .map_or(1.0, |monitor| movement_device_scale(monitor.scale_factor));

    let previous_cursor = state.cursor;
    let mut next_cursor = previous_cursor;
    next_cursor.x += direction.x * speed_x * delta_seconds * device_scale;
    next_cursor.y += direction.y * speed_y * delta_seconds * device_scale;

    if let Some(index) = clamp_and_find_monitor(&mut next_cursor, &state.monitors) {
        if next_cursor != previous_cursor {
            state.cursor = next_cursor;
            state.selected_monitor = index;
            actions.push(Action::MouseMove(state.cursor));
        }
    }
}

fn key_elapsed(state: &SharedState, key: Key, now: Instant) -> f64 {
    state
        .move_key_pressed_at
        .get(&key)
        .map(|t| now.saturating_duration_since(*t).as_secs_f64())
        .unwrap_or(0.0)
}

fn acceleration_speed(elapsed_secs: f64, base: f64, accel: f64, max: f64) -> f64 {
    if elapsed_secs < ACCEL_DELAY_SECS {
        base
    } else {
        (base + accel * (elapsed_secs - ACCEL_DELAY_SECS)).min(max)
    }
}

fn update_cursor(state: &mut SharedState, point: Point) {
    let mut clamped = point;
    if let Some(index) = clamp_and_find_monitor(&mut clamped, &state.monitors) {
        state.selected_monitor = index;
    }
    state.cursor = clamped;
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

    if keys.contains(&KEY_FAST) {
        multiplier *= FAST_MULTIPLIER;
    }

    if keys.contains(&KEY_SLOW) {
        multiplier *= SLOW_MULTIPLIER;
    }

    multiplier
}

fn jump_grid_index(key: Key) -> Option<(usize, usize)> {
    for (row, keys) in JUMP_GRID.iter().enumerate() {
        for (col, cell_key) in keys.iter().enumerate() {
            if *cell_key == key {
                return Some((col, row));
            }
        }
    }
    None
}

fn jump_target_with_index(monitor: crate::state::MonitorInfo, key: Key) -> Option<(Point, u8, u8)> {
    let (col, row) = jump_grid_index(key)?;
    let cell_w = monitor.width / JUMP_GRID[0].len() as f64;
    let cell_h = monitor.height / JUMP_GRID.len() as f64;
    Some((
        Point {
            x: monitor.origin.x + (col as f64 + 0.5) * cell_w,
            y: monitor.origin.y + (row as f64 + 0.5) * cell_h,
        },
        col as u8,
        row as u8,
    ))
}

fn subcell_target(
    monitor: crate::state::MonitorInfo,
    cell_col: u8,
    cell_row: u8,
    key: Key,
) -> Option<Point> {
    let (sc, sr) = jump_grid_index(key)?;
    let cell_w = monitor.width / JUMP_GRID[0].len() as f64;
    let cell_h = monitor.height / JUMP_GRID.len() as f64;
    let sub_w = cell_w / JUMP_GRID[0].len() as f64;
    let sub_h = cell_h / JUMP_GRID.len() as f64;
    Some(Point {
        x: monitor.origin.x + cell_col as f64 * cell_w + sc as f64 * sub_w + sub_w * 0.5,
        y: monitor.origin.y + cell_row as f64 * cell_h + sr as f64 * sub_h + sub_h * 0.5,
    })
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

    #[cfg(target_os = "linux")]
    {
        let _ = (tracker, key, is_press);
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
    CHORD_QUIT.contains(&current_key)
        && CHORD_QUIT
            .iter()
            .all(|k| held_keys.contains(k) || *k == current_key)
        && held_keys.iter().all(|k| CHORD_QUIT.contains(k))
}

// KEY_UNMARK is active, turning mark keys into unmark keys and KEY_UNMARK_ALL into unmark-all.
// As an OS modifier it must be the sole forwarded one (leak-mode exception); as a regular key
// it is captured-held like any other ViMouse key. The branch folds away at compile time.
fn unmark_held(tracker: &HookTracker) -> bool {
    if is_os_modifier(KEY_UNMARK) {
        tracker.forwarded_modifiers == [KEY_UNMARK]
    } else {
        tracker.captured_keys.contains(&KEY_UNMARK)
    }
}

// AltGr is deliberately not an OS modifier so it stays bindable (KEY_TOGGLE_OVERLAY).
fn is_os_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::MetaLeft
            | Key::MetaRight
    )
}

fn is_exempt_key(key: Key) -> bool {
    static EXEMPT_KEYS: std::sync::OnceLock<HashSet<Key>> = std::sync::OnceLock::new();
    let set = EXEMPT_KEYS.get_or_init(|| KEYS_EXEMPT.iter().copied().collect());
    // Keys rdev can't map (media/volume keys and the like) can't be acted on - let them through.
    matches!(key, Key::Unknown(_) | Key::Function) || set.contains(&key)
}

fn scroll_mode_active(keys: &HashSet<Key>) -> bool {
    keys.contains(&KEY_SCROLL)
}

fn movement_active(keys: &HashSet<Key>) -> bool {
    MOVE_KEYS.iter().any(|key| keys.contains(key))
}

fn is_move_key(key: Key) -> bool {
    MOVE_KEYS.contains(&key)
}

fn is_mouse_key(key: Key) -> bool {
    matches!(
        key,
        KEY_MOUSE_1 | KEY_MOUSE_2 | KEY_MOUSE_3 | KEY_MOUSE_4 | KEY_MOUSE_5
    )
}

fn is_jump_key(key: Key) -> bool {
    static JUMP_KEYS: std::sync::OnceLock<HashSet<Key>> = std::sync::OnceLock::new();
    let set = JUMP_KEYS.get_or_init(|| JUMP_GRID.iter().flatten().copied().collect());
    set.contains(&key)
}

fn is_mark_key(key: Key) -> bool {
    static MARK_KEYS: std::sync::OnceLock<HashSet<Key>> = std::sync::OnceLock::new();
    let set = MARK_KEYS.get_or_init(|| KEYS_MARK.iter().copied().collect());
    set.contains(&key)
}

fn is_runtime_modifier(key: Key) -> bool {
    key == KEY_SCROLL || key == KEY_FAST || key == KEY_SLOW
}

/// Captures the subset of SharedState that drives overlay rendering.
/// Used to detect whether a key press changed anything the UI cares about.
#[derive(PartialEq)]
struct UiStateSnapshot {
    mode: Mode,
    show_grid: bool,
    show_grid_letters: bool,
    show_overlays: bool,
    selected_monitor: usize,
    // Marks are few (≤ KEYS_MARK.len()), so cloning the map per key press is cheap and lets
    // the event loop detect mark changes and repaint the mark overlay.
    marks: HashMap<Key, Point>,
}

fn ui_snapshot(state: &SharedState) -> UiStateSnapshot {
    UiStateSnapshot {
        mode: state.mode,
        show_grid: state.show_grid,
        show_grid_letters: state.show_grid_letters,
        show_overlays: state.show_overlays,
        selected_monitor: state.selected_monitor,
        marks: state.marks.clone(),
    }
}

pub fn caps_lock_used_in_config() -> bool {
    static RESULT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RESULT.get_or_init(|| {
        [
            KEY_NORMAL_MODE,
            KEY_INSERT_MODE,
            KEY_SCROLL,
            KEY_FAST,
            KEY_SLOW,
            KEY_MOUSE_1,
            KEY_MOUSE_2,
            KEY_MOUSE_3,
            KEY_MOUSE_4,
            KEY_MOUSE_5,
            KEY_CYCLE_MONITOR,
            KEY_TOGGLE_GRID,
            KEY_TOGGLE_GRID_LETTERS,
            KEY_MOVE_LEFT,
            KEY_MOVE_DOWN,
            KEY_MOVE_UP,
            KEY_MOVE_RIGHT,
        ]
        .contains(&Key::CapsLock)
            || CHORD_QUIT.contains(&Key::CapsLock)
            || JUMP_GRID.iter().flatten().any(|k| *k == Key::CapsLock)
    })
}
