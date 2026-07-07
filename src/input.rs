use crate::config::{
    ACCEL_DELAY_SECS, CHORD_QUIT, CHORD_UNMARK_ALL, CURSOR_ACCELERATION, CURSOR_BASE_SPEED,
    CURSOR_MAX_SPEED, FAST_MULTIPLIER, JUMP_GRID, JUMP_GRID_DELAY, KEYS_MARK, KEY_CYCLE_MONITOR,
    KEY_FAST, KEY_INSERT_MODE, KEY_MOUSE_1, KEY_MOUSE_2, KEY_MOUSE_3, KEY_MOUSE_4, KEY_MOUSE_5,
    KEY_MOVE_DOWN, KEY_MOVE_LEFT, KEY_MOVE_RIGHT, KEY_MOVE_UP, KEY_NORMAL_MODE, KEY_SCROLL,
    KEY_SLOW, KEY_TOGGLE_GRID, KEY_TOGGLE_GRID_LETTERS, KEY_TOGGLE_OVERLAY, KEY_UNMARK,
    SCROLL_ACCELERATION, SCROLL_BASE_SPEED, SCROLL_MAX_SPEED, SLOW_MULTIPLIER, TICK_RATE_HZ,
};
use crate::monitor::{clamp_and_find_monitor, monitor_index_for_point};
#[cfg(target_os = "macos")]
use crate::platform_input::set_caps_lock_remap;
use crate::platform_input::{
    scroll_direction_sign, shutdown_platform_input, simulate_input, InputEmitter, BUTTON_MOUSE_4,
    BUTTON_MOUSE_5,
};
use crate::state::{Action, Mode, MotionWaker, Point, Shared, SharedState, UiWaker};
#[cfg(not(target_os = "macos"))]
use rdev::grab;
use rdev::{Button, Event, EventType, Key};
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

const MOVE_KEYS: [Key; 4] = [KEY_MOVE_LEFT, KEY_MOVE_DOWN, KEY_MOVE_UP, KEY_MOVE_RIGHT];

// Definitions
//
// Captured: Never sent to the OS. ViMouse acts on it (move cursor, click, switch
// mode, etc.) and the OS never sees the event. Always captured from key-down
// until key-up.
//
// Suppressed: Conditionally sent to the OS. ViMouse does not always act on the
// key, but temporarily hides it from the OS. This applies to runtime modifiers
// (KEY_SCROLL / KEY_FAST / KEY_SLOW) while the cursor is moving, otherwise the
// OS would treat them as held modifier keys and do unexpected things (e.g.
// interpret a scroll as Shift+scroll). ViMouse sends a fake key-release to hide
// them, then a fake key-press to restore them once movement stops.
//
// Captured keys are owned by ViMouse for their full lifetime; suppressed keys
// are still "held" from ViMouse's perspective but transiently hidden from the OS.
#[derive(Default)]
struct HookTracker {
    held_keys: HashSet<Key>,
    captured_keys: HashSet<Key>,
    suppressed_modifiers: HashSet<Key>,
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

// Returns true if the key is captured (its press event is dropped from the OS stream).
//   - See Captured vs Suppressed definition in the comment at the top of this file
//
// Capture key logic:
//   Insert mode
//     - KEY_NORMAL_MODE (no modifiers held).
//   Normal mode
//     - Move keys: always captured.
//     - KEY_FAST / KEY_SLOW: captured only while movement or scroll is already active.
//     - Mouse clicks, grid toggle, jump, KEY_INSERT_MODE, KEY_CYCLE_MONITOR:
//       captured when no runtime modifiers (KEY_SCROLL / KEY_FAST / KEY_SLOW) are held.
//     - EXCEPTION: if an uncaptured non-ViMouse key is already held, the whole
//       chord passes through; ViMouse won't steal a foreign shortcut mid-chord.
//       - HOWEVER: clicks and scroll-move keys ignore OS modifiers
//         (Ctrl/Alt/Shift/Meta) when checking for foreign chords, so
//         ex. Ctrl+click still works (still captured).
//
// Runtime modifiers (KEY_SCROLL / KEY_FAST / KEY_SLOW) are suppressed separately
// from the OS while movement is active (see sync_runtime_modifier_suppression);
// they are not in captured_keys.
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
        return tracker.captured_keys.contains(&key);
    }

    if quit_chord_active(&tracker.held_keys, key) {
        shutdown_platform_input();
        std::process::exit(0);
    }

    let should_capture = if key == KEY_TOGGLE_OVERLAY && no_modifiers_held(&tracker.held_keys) {
        true
    } else {
        match state.mode {
            Mode::Insert => key == KEY_NORMAL_MODE && no_modifiers_held(&tracker.held_keys),
            Mode::Normal => {
                #[allow(clippy::if_same_then_else)]
                // Mark keys (set / jump / unmark single); only suppressed when pressed exclusively
                if is_mark_key(key) && only_key_held(&tracker.held_keys, key) {
                    true
                }
                // Unmark-all chord: capture the completing key (e.g. BackQuote) so the chord is
                // suppressed. KEY_UNMARK itself is never captured here, so it stays consistent for
                // the OS and scroll suppression is untouched.
                else if unmark_all_chord_active(&tracker.held_keys, key) {
                    true
                }
                // Only capture KEY_FAST/KEY_SLOW when scroll/move active.
                else if (key == KEY_FAST || key == KEY_SLOW)
                    && (tracker.held_keys.contains(&KEY_SCROLL)
                        || movement_active(&state.pressed_keys))
                {
                    true
                }
                // Capture mouse keys even when OS modifiers (Ctrl/Alt/Shift/Meta) are held, so the
                // modifier state is preserved on mouse clicks (allowing Ctrl+clicks, etc.).
                // MOUSE_3/4/5 pass through when a modifier is held, preserving shortcuts like Cmd+O.
                else if is_mouse_key(key)
                    && !has_uncaptured_non_modifier_non_os(&tracker, key)
                    && (matches!(key, KEY_MOUSE_1 | KEY_MOUSE_2)
                        || no_os_modifiers_held(&tracker.held_keys))
                {
                    true
                }
                // Capture move keys mid-scroll so modifier state is preserved on the scroll event
                // (same rationale as click keys above).
                else if is_move_key(key)
                    && scroll_mode_active(&state.pressed_keys)
                    && !has_uncaptured_non_modifier_non_os(&tracker, key)
                {
                    true
                }
                // If a non-ViMouse key started the chord, let the rest of that chord pass through.
                else if has_uncaptured_non_modifier(&tracker, key) {
                    false
                }
                // Always capture move keys (no scroll active); handled above if scroll active.
                else if is_move_key(key) {
                    true
                }
                // Capture ViMouse action keys, but only when no modifiers are held to avoid
                // stealing shortcuts like Ctrl+T or Alt+N that other apps use.
                else if key == KEY_INSERT_MODE
                    || key == KEY_CYCLE_MONITOR
                    || key == KEY_TOGGLE_GRID
                    || key == KEY_TOGGLE_GRID_LETTERS
                    || is_jump_key(key)
                    || (key == Key::CapsLock && caps_lock_used_in_config())
                {
                    no_modifiers_held(&tracker.held_keys)
                }
                // Let all non-ViMouse keys pass through.
                else {
                    false
                }
            }
        }
    };

    let ui_changed = if should_capture {
        tracker.captured_keys.insert(key);

        // Snapshot UI-visible state before applying the key action so we can detect changes.
        let ui_before = ui_snapshot(&state);

        if key == KEY_TOGGLE_OVERLAY {
            state.show_overlays = !state.show_overlays;
        } else {
            match state.mode {
                Mode::Insert => enter_normal_mode(&mut state, &tracker.held_keys),
                Mode::Normal => {
                    if !is_jump_key(key) {
                        state.pending_subcell = None;
                    }
                    apply_normal_mode_press(&mut state, key, &tracker.held_keys);
                }
            }
        }

        ui_snapshot(&state) != ui_before
    } else {
        false
    };

    // Run unconditionally: a runtime modifier (e.g. KEY_SCROLL) is never "captured", but while
    // moving it must be hidden from the OS so it doesn't corrupt synthetic events (a held Shift
    // turns a vertical wheel into a horizontal one). This may start suppressing `key` itself.
    sync_runtime_modifier_suppression(&state, &mut tracker);

    // Drop the event from the OS stream if captured, OR if this press is the modifier we just
    // started suppressing - otherwise the real key-down would reach the OS after sync's fake
    // key-up and re-assert the modifier, making press order (move-then-scroll) misbehave.
    let drop_from_os = should_capture || tracker.suppressed_modifiers.contains(&key);

    state.motion_needed = true;
    drop(state);
    drop(tracker);
    waker.notify_one();

    if ui_changed {
        let _ = ui_waker.send_event(());
    }

    drop_from_os
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
    let was_suppressed = tracker.suppressed_modifiers.contains(&key);

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

    sync_runtime_modifier_suppression(&state, &mut tracker);
    drop(state);
    drop(tracker);

    if wake_motion {
        waker.notify_one();
    }

    was_captured || was_suppressed
}

fn apply_normal_mode_press(state: &mut SharedState, key: Key, held_keys: &HashSet<Key>) {
    // Unmark-all takes priority: the chord's completing key may itself be a mark key.
    if unmark_all_chord_active(held_keys, key) {
        state.marks.clear();
        return;
    }

    match key {
        KEY_INSERT_MODE => enter_insert_mode(state),
        KEY_NORMAL_MODE => {}
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
        _ if is_mark_key(key) => apply_mark_press(state, key, held_keys.contains(&KEY_UNMARK)),
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
}

fn enter_normal_mode(state: &mut SharedState, held_keys: &HashSet<Key>) {
    state.mode = Mode::Normal;
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

#[cfg(target_os = "macos")]
fn sync_runtime_modifier_suppression(_state: &SharedState, tracker: &mut HookTracker) {
    // Keep the macOS hook simple: avoid replaying keyboard events from inside the event tap.
    tracker.pending_key_events.clear();
    tracker.suppressed_modifiers.clear();
}

#[cfg(not(target_os = "macos"))]
// Hide runtime modifiers from the OS during movement so they don't corrupt synthetic events
// (e.g. Shift leaking onto scroll). Send fake key-release to hide, then fake key-press to restore when
// movement stops, so the OS key state stays consistent with what the user is physically holding.
fn sync_runtime_modifier_suppression(state: &SharedState, tracker: &mut HookTracker) {
    // There are at most 3 runtime modifiers - use a stack array to avoid heap allocation.
    const RUNTIME_MODIFIERS: [Key; 3] = [KEY_SCROLL, KEY_FAST, KEY_SLOW];

    let moving = movement_active(&state.pressed_keys);

    for &key in &RUNTIME_MODIFIERS {
        let want_suppressed = moving && tracker.held_keys.contains(&key);
        let is_suppressed = tracker.suppressed_modifiers.contains(&key);

        if want_suppressed && !is_suppressed {
            tracker.suppressed_modifiers.insert(key);
            if !tracker.captured_keys.contains(&key) {
                tracker.pending_key_events.push((key, false));
            }
        } else if !want_suppressed && is_suppressed {
            tracker.suppressed_modifiers.remove(&key);
            if tracker.held_keys.contains(&key) {
                tracker.pending_key_events.push((key, true));
                tracker.captured_keys.remove(&key);
            }
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

    let previous_cursor = state.cursor;
    let mut next_cursor = previous_cursor;
    next_cursor.x += direction.x * speed_x * delta_seconds;
    next_cursor.y += direction.y * speed_y * delta_seconds;

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

fn unmark_all_chord_active(held_keys: &HashSet<Key>, current_key: Key) -> bool {
    CHORD_UNMARK_ALL.contains(&current_key)
        && CHORD_UNMARK_ALL
            .iter()
            .all(|k| held_keys.contains(k) || *k == current_key)
        && held_keys.iter().all(|k| CHORD_UNMARK_ALL.contains(k))
}

fn no_modifiers_held(keys: &HashSet<Key>) -> bool {
    !keys.iter().any(|key| is_runtime_modifier(*key))
}

// True when `key` is the only key held, allowing KEY_SCROLL and KEY_UNMARK as concurrent modifiers
// (they may share keybinds). Used to gate mark keys so they fire only when pressed exclusively.
fn only_key_held(keys: &HashSet<Key>, key: Key) -> bool {
    keys.iter()
        .all(|held| *held == key || *held == KEY_SCROLL || *held == KEY_UNMARK)
}

fn has_uncaptured_non_modifier(tracker: &HookTracker, key: Key) -> bool {
    tracker.held_keys.iter().any(|held_key| {
        *held_key != key
            && !is_runtime_modifier(*held_key)
            && *held_key != Key::CapsLock
            && !tracker.captured_keys.contains(held_key)
    })
}

fn has_uncaptured_non_modifier_non_os(tracker: &HookTracker, key: Key) -> bool {
    tracker.held_keys.iter().any(|held_key| {
        *held_key != key
            && !is_runtime_modifier(*held_key)
            && !is_os_modifier(*held_key)
            && *held_key != Key::CapsLock
            && !tracker.captured_keys.contains(held_key)
    })
}

fn is_os_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::MetaLeft
            | Key::MetaRight
    )
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

fn no_os_modifiers_held(keys: &HashSet<Key>) -> bool {
    !keys.iter().any(|key| is_os_modifier(*key))
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
