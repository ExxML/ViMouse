mod platform;

use font8x8::{UnicodeFonts, BASIC_FONTS};
use pixels::{Pixels, SurfaceTexture};
use rdev::{grab, simulate, Button, Event, EventType, Key, SimulateError};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{WindowBuilder, WindowLevel};

const MOVE_SPEED_PX_PER_SEC: f64 = 1200.0;
const SCROLL_SPEED_UNITS_PER_SEC: f64 = 1000.0;
const FAST_MULTIPLIER: f64 = 2.0;
const SLOW_MULTIPLIER: f64 = 0.3;
const TICK_RATE_HZ: u64 = 120;
const OVERLAY_SIZE: u32 = 64;

static SIMULATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
}

impl Mode {
    fn label(self) -> char {
        match self {
            Self::Normal => 'N',
            Self::Insert => 'I',
        }
    }

    fn background(self) -> [u8; 4] {
        match self {
            Self::Normal => [30, 160, 98, 255],
            Self::Insert => [44, 55, 72, 255],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MonitorInfo {
    origin: Point,
    width: f64,
    height: f64,
}

impl MonitorInfo {
    fn contains(self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x < self.origin.x + self.width
            && point.y >= self.origin.y
            && point.y < self.origin.y + self.height
    }

    fn center(self) -> Point {
        Point {
            x: self.origin.x + self.width * 0.5,
            y: self.origin.y + self.height * 0.5,
        }
    }
}

struct SharedState {
    mode: Mode,
    cursor: Point,
    selected_monitor: usize,
    monitors: Vec<MonitorInfo>,
    pressed_keys: HashSet<Key>,
    passthrough_keys: HashSet<Key>,
    left_button_down: bool,
    right_button_down: bool,
    scroll_remainder: Point,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    MouseMove(Point),
    Scroll { delta_x: i64, delta_y: i64 },
    ButtonPress(Button),
    ButtonRelease(Button),
}

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("ViMouse")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(OVERLAY_SIZE, OVERLAY_SIZE))
        .build(&event_loop)
        .expect("failed to create overlay window");

    let monitors = collect_monitors(&window);
    let initial_cursor = initial_cursor(&monitors);
    let initial_monitor = monitor_index_for_point(&monitors, initial_cursor).unwrap_or(0);

    let shared = Arc::new(Mutex::new(SharedState {
        mode: Mode::Normal,
        cursor: initial_cursor,
        selected_monitor: initial_monitor,
        monitors,
        pressed_keys: HashSet::new(),
        passthrough_keys: HashSet::new(),
        left_button_down: false,
        right_button_down: false,
        scroll_remainder: Point::default(),
    }));

    spawn_input_hook(Arc::clone(&shared));
    spawn_motion_loop(Arc::clone(&shared));

    let window_size = window.inner_size();
    let surface = SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels = Pixels::new(OVERLAY_SIZE, OVERLAY_SIZE, surface).expect("pixels init failed");
    let mut last_overlay = current_overlay(&shared);
    if let Err(error) = paint_overlay(&window, &mut pixels, &last_overlay) {
        eprintln!("initial overlay render error: {error}");
        return;
    }
    window.set_visible(true);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33));

        match event {
            WinitEvent::MainEventsCleared => {
                let overlay = current_overlay(&shared);
                if last_overlay != overlay {
                    position_overlay(&window, &overlay.monitor);
                    window.request_redraw();
                    last_overlay = overlay;
                }
            }
            WinitEvent::RedrawRequested(_) => {
                if let Err(error) = paint_overlay(&window, &mut pixels, &last_overlay) {
                    eprintln!("overlay render error: {error}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            WinitEvent::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

#[derive(Clone, Debug, PartialEq)]
struct OverlayState {
    mode: Mode,
    monitor: MonitorInfo,
}

fn current_overlay(shared: &Arc<Mutex<SharedState>>) -> OverlayState {
    let state = shared.lock().expect("shared state poisoned");
    OverlayState {
        mode: state.mode,
        monitor: state
            .monitors
            .get(state.selected_monitor)
            .copied()
            .unwrap_or_else(fallback_monitor),
    }
}

fn paint_overlay(
    window: &winit::window::Window,
    pixels: &mut Pixels,
    overlay: &OverlayState,
) -> Result<(), pixels::Error> {
    position_overlay(window, &overlay.monitor);
    draw_overlay(pixels.frame_mut(), overlay.mode);
    pixels.render()
}

fn spawn_input_hook(shared: Arc<Mutex<SharedState>>) {
    thread::Builder::new()
        .name("vimouse-hook".into())
        .spawn(move || {
            let hook_shared = Arc::clone(&shared);
            let callback = move |event: Event| handle_input_event(&hook_shared, event);
            if let Err(error) = grab(callback) {
                eprintln!("global input grab failed: {error:?}");
            }
        })
        .expect("failed to spawn input hook thread");
}

fn spawn_motion_loop(shared: Arc<Mutex<SharedState>>) {
    thread::Builder::new()
        .name("vimouse-motion".into())
        .spawn(move || {
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

fn handle_input_event(shared: &Arc<Mutex<SharedState>>, event: Event) -> Option<Event> {
    let event_type = event.event_type.clone();
    let mut actions = Vec::new();
    let result = {
        let mut state = shared.lock().expect("shared state poisoned");
        match event_type {
            EventType::MouseMove { x, y } => {
                state.cursor = Point { x, y };
                if let Some(index) = monitor_index_for_point(&state.monitors, state.cursor) {
                    state.selected_monitor = index;
                }
                Some(event)
            }
            EventType::KeyPress(key) => {
                let is_repeat = !state.pressed_keys.insert(key);
                if !is_repeat && is_quit_chord(&state.pressed_keys) {
                    std::process::exit(0);
                }
                if state.mode == Mode::Insert {
                    handle_insert_mode_press(&mut state, key, is_repeat, &mut actions, event)
                } else {
                    handle_normal_mode_press(&mut state, key, is_repeat, &mut actions, event)
                }
            }
            EventType::KeyRelease(key) => {
                state.pressed_keys.remove(&key);

                if state.passthrough_keys.remove(&key) {
                    Some(event)
                } else if state.mode == Mode::Insert {
                    handle_insert_mode_release(&mut state, key, &mut actions)
                } else {
                    handle_normal_mode_release(&mut state, key, &mut actions)
                }
            }
            _ => Some(event),
        }
    };

    dispatch_actions(&actions);
    result
}

fn handle_insert_mode_press(
    state: &mut SharedState,
    key: Key,
    _is_repeat: bool,
    actions: &mut Vec<Action>,
    event: Event,
) -> Option<Event> {
    if key == Key::Escape {
        enter_normal_mode(state, actions);
        return None;
    }

    state.passthrough_keys.insert(key);
    Some(event)
}

fn handle_insert_mode_release(
    _state: &mut SharedState,
    key: Key,
    _actions: &mut Vec<Action>,
) -> Option<Event> {
    if key == Key::Escape {
        None
    } else {
        None
    }
}

fn handle_normal_mode_press(
    state: &mut SharedState,
    key: Key,
    is_repeat: bool,
    actions: &mut Vec<Action>,
    event: Event,
) -> Option<Event> {
    if key == Key::Escape {
        enter_normal_mode(state, actions);
        return None;
    }

    if key == Key::KeyI && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        enter_insert_mode(state, actions);
        return None;
    }

    if key == Key::BackQuote && exact_single_key(&state.pressed_keys, key) && !is_repeat {
        if !state.monitors.is_empty() {
            state.selected_monitor = (state.selected_monitor + 1) % state.monitors.len();
        }
        return None;
    }

    if let Some((column, row)) = jump_cell(key) {
        if exact_single_key(&state.pressed_keys, key) && !is_repeat {
            if let Some(monitor) = state.monitors.get(state.selected_monitor).copied() {
                let target = Point {
                    x: monitor.origin.x + ((column as f64) + 0.5) * (monitor.width / 5.0),
                    y: monitor.origin.y + ((row as f64) + 0.5) * (monitor.height / 3.0),
                };
                state.cursor = target;
                actions.push(Action::MouseMove(target));
            }
            return None;
        }

        return pass_through_or_swallow(state, key, event);
    }

    if key == Key::SemiColon {
        if !state.left_button_down && is_valid_left_button_set(&state.pressed_keys) {
            state.left_button_down = true;
            actions.push(Action::ButtonPress(Button::Left));
        }
        return None;
    }

    if key == Key::CapsLock {
        if !state.right_button_down && is_valid_right_button_set(&state.pressed_keys) {
            state.right_button_down = true;
            actions.push(Action::ButtonPress(Button::Right));
        }
        return None;
    }

    if is_movement_key(key)
        || is_shift_key(key)
        || is_alt_key(key)
        || is_control_key(key)
        || is_button_key(key)
    {
        if is_valid_scroll_set(&state.pressed_keys) {
            return None;
        }

        if is_valid_move_set(&state.pressed_keys) {
            return None;
        }

        return pass_through_or_swallow(state, key, event);
    }

    pass_through_or_swallow(state, key, event)
}

fn handle_normal_mode_release(
    state: &mut SharedState,
    key: Key,
    actions: &mut Vec<Action>,
) -> Option<Event> {
    match key {
        Key::SemiColon => {
            if state.left_button_down {
                state.left_button_down = false;
                actions.push(Action::ButtonRelease(Button::Left));
            }
            None
        }
        Key::CapsLock => {
            if state.right_button_down {
                state.right_button_down = false;
                actions.push(Action::ButtonRelease(Button::Right));
            }
            None
        }
        _ => None,
    }
}

fn pass_through_or_swallow(state: &mut SharedState, key: Key, event: Event) -> Option<Event> {
    if should_passthrough_in_normal_mode(&state.pressed_keys, key) {
        state.passthrough_keys.insert(key);
        Some(event)
    } else {
        None
    }
}

fn tick_state(state: &mut SharedState, dt: Duration) -> Vec<Action> {
    let mut actions = Vec::new();

    if state.mode != Mode::Normal {
        return actions;
    }

    if let Some(direction) = normalized_direction(&state.pressed_keys) {
        let speed_multiplier = speed_multiplier(&state.pressed_keys);
        let dt_seconds = dt.as_secs_f64();

        if is_valid_scroll_set(&state.pressed_keys) {
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
        } else if is_valid_move_set(&state.pressed_keys) {
            state.scroll_remainder = Point::default();
            let step = MOVE_SPEED_PX_PER_SEC * speed_multiplier * dt_seconds;
            let mut target = Point {
                x: state.cursor.x + direction.x * step,
                y: state.cursor.y + direction.y * step,
            };
            clamp_to_virtual_bounds(&mut target, &state.monitors);
            state.cursor = target;

            if let Some(index) = monitor_index_for_point(&state.monitors, target) {
                state.selected_monitor = index;
            }

            actions.push(Action::MouseMove(target));
        }
    } else {
        state.scroll_remainder = Point::default();
    }

    actions
}

fn enter_insert_mode(state: &mut SharedState, actions: &mut Vec<Action>) {
    release_mouse_buttons(state, actions);
    state.mode = Mode::Insert;
    state.pressed_keys.clear();
    state.passthrough_keys.clear();
    state.scroll_remainder = Point::default();
}

fn enter_normal_mode(state: &mut SharedState, actions: &mut Vec<Action>) {
    release_mouse_buttons(state, actions);
    state.mode = Mode::Normal;
    state.pressed_keys.clear();
    state.passthrough_keys.clear();
    state.scroll_remainder = Point::default();
}

fn release_mouse_buttons(state: &mut SharedState, actions: &mut Vec<Action>) {
    if state.left_button_down {
        state.left_button_down = false;
        actions.push(Action::ButtonRelease(Button::Left));
    }

    if state.right_button_down {
        state.right_button_down = false;
        actions.push(Action::ButtonRelease(Button::Right));
    }
}

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
    let result = simulate(event);
    thread::sleep(Duration::from_millis(1));
    result
}

fn collect_monitors(window: &winit::window::Window) -> Vec<MonitorInfo> {
    let mut monitors: Vec<_> = window
        .available_monitors()
        .map(|monitor| {
            let origin = monitor.position();
            let size = monitor.size();
            MonitorInfo {
                origin: Point {
                    x: origin.x as f64,
                    y: origin.y as f64,
                },
                width: size.width as f64,
                height: size.height as f64,
            }
        })
        .collect();

    monitors.sort_by(|left, right| {
        left.origin
            .x
            .partial_cmp(&right.origin.x)
            .unwrap()
            .then_with(|| left.origin.y.partial_cmp(&right.origin.y).unwrap())
    });

    if monitors.is_empty() {
        monitors.push(fallback_monitor());
    }

    monitors
}

fn fallback_monitor() -> MonitorInfo {
    MonitorInfo {
        origin: Point { x: 0.0, y: 0.0 },
        width: 1920.0,
        height: 1080.0,
    }
}

fn initial_cursor(monitors: &[MonitorInfo]) -> Point {
    if let Some((x, y)) = platform::current_cursor_position() {
        return Point { x, y };
    }

    monitors
        .first()
        .copied()
        .unwrap_or_else(fallback_monitor)
        .center()
}

fn position_overlay(window: &winit::window::Window, monitor: &MonitorInfo) {
    let x = (monitor.origin.x as i32) + monitor.width as i32 - OVERLAY_SIZE as i32;
    let y = (monitor.origin.y as i32) + monitor.height as i32 - OVERLAY_SIZE as i32;
    window.set_outer_position(PhysicalPosition::new(x, y));
}

fn draw_overlay(frame: &mut [u8], mode: Mode) {
    let [r, g, b, a] = mode.background();
    for chunk in frame.chunks_exact_mut(4) {
        chunk[0] = r;
        chunk[1] = g;
        chunk[2] = b;
        chunk[3] = a;
    }

    let glyph = BASIC_FONTS
        .get(mode.label())
        .expect("overlay glyph should exist");
    let scale = (((OVERLAY_SIZE as usize) * 3) + 20) / 40;
    let scale = scale.max(1);
    let mut min_col = 8usize;
    let mut max_col = 0usize;
    let mut min_row = 8usize;
    let mut max_row = 0usize;

    for (row, bits) in glyph.iter().enumerate() {
        if *bits == 0 {
            continue;
        }

        min_row = min_row.min(row);
        max_row = max_row.max(row);

        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }

            min_col = min_col.min(col);
            max_col = max_col.max(col);
        }
    }

    let glyph_width = (max_col - min_col + 1) * scale;
    let glyph_height = (max_row - min_row + 1) * scale;
    let offset_x = ((OVERLAY_SIZE as usize) - glyph_width) / 2;
    let offset_y = ((OVERLAY_SIZE as usize) - glyph_height) / 2;

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8usize {
            if (bits >> col) & 1 == 0 {
                continue;
            }

            for dy in 0..scale {
                for dx in 0..scale {
                    let px = offset_x + ((col - min_col) * scale) + dx;
                    let py = offset_y + ((row - min_row) * scale) + dy;
                    let index = (py * OVERLAY_SIZE as usize + px) * 4;
                    frame[index] = 255;
                    frame[index + 1] = 255;
                    frame[index + 2] = 255;
                    frame[index + 3] = 255;
                }
            }
        }
    }
}

fn should_passthrough_in_normal_mode(pressed_keys: &HashSet<Key>, key: Key) -> bool {
    is_meta_key(key)
        || is_control_key(key)
        || (pressed_keys
            .iter()
            .any(|pressed| is_meta_key(*pressed) || is_alt_key(*pressed))
            && !is_valid_move_set(pressed_keys)
            && !is_valid_scroll_set(pressed_keys))
        || (pressed_keys.iter().any(|pressed| is_control_key(*pressed))
            && !is_valid_scroll_set(pressed_keys)
            && !is_valid_move_set(pressed_keys))
}

fn exact_single_key(keys: &HashSet<Key>, expected: Key) -> bool {
    keys.len() == 1 && keys.contains(&expected)
}

fn is_quit_chord(keys: &HashSet<Key>) -> bool {
    keys.contains(&Key::KeyQ)
        && keys.iter().any(|key| is_control_key(*key))
        && keys.iter().any(|key| is_shift_key(*key))
        && keys
            .iter()
            .all(|key| *key == Key::KeyQ || is_control_key(*key) || is_shift_key(*key))
}

fn is_valid_move_set(keys: &HashSet<Key>) -> bool {
    let has_movement = keys.iter().any(|key| is_movement_key(*key));
    if !has_movement {
        return false;
    }

    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_shift_key(*key)
            || is_alt_key(*key)
            || *key == Key::SemiColon
            || *key == Key::CapsLock
    })
}

fn is_valid_left_button_set(keys: &HashSet<Key>) -> bool {
    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_shift_key(*key)
            || is_alt_key(*key)
            || *key == Key::SemiColon
            || *key == Key::CapsLock
    }) && keys.contains(&Key::SemiColon)
}

fn is_valid_right_button_set(keys: &HashSet<Key>) -> bool {
    keys.iter().all(|key| {
        is_movement_key(*key)
            || is_shift_key(*key)
            || is_alt_key(*key)
            || *key == Key::SemiColon
            || *key == Key::CapsLock
    }) && keys.contains(&Key::CapsLock)
}

fn is_valid_scroll_set(keys: &HashSet<Key>) -> bool {
    let has_movement = keys.iter().any(|key| is_movement_key(*key));
    let has_control = keys.iter().any(|key| is_control_key(*key));
    if !has_movement || !has_control {
        return false;
    }

    keys.iter().all(|key| {
        is_movement_key(*key) || is_shift_key(*key) || is_alt_key(*key) || is_control_key(*key)
    })
}

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

    if keys.iter().any(|key| is_shift_key(*key)) {
        multiplier *= FAST_MULTIPLIER;
    }
    if keys.iter().any(|key| is_alt_key(*key)) {
        multiplier *= SLOW_MULTIPLIER;
    }

    multiplier
}

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

fn is_button_key(key: Key) -> bool {
    matches!(key, Key::SemiColon | Key::CapsLock)
}

fn is_meta_key(key: Key) -> bool {
    matches!(key, Key::MetaLeft | Key::MetaRight)
}

fn clamp_to_virtual_bounds(point: &mut Point, monitors: &[MonitorInfo]) {
    let min_x = monitors
        .iter()
        .map(|monitor| monitor.origin.x)
        .fold(f64::INFINITY, f64::min);
    let min_y = monitors
        .iter()
        .map(|monitor| monitor.origin.y)
        .fold(f64::INFINITY, f64::min);
    let max_x = monitors
        .iter()
        .map(|monitor| monitor.origin.x + monitor.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = monitors
        .iter()
        .map(|monitor| monitor.origin.y + monitor.height)
        .fold(f64::NEG_INFINITY, f64::max);

    point.x = point.x.clamp(min_x, (max_x - 1.0).max(min_x));
    point.y = point.y.clamp(min_y, (max_y - 1.0).max(min_y));
}

fn monitor_index_for_point(monitors: &[MonitorInfo], point: Point) -> Option<usize> {
    if monitors.is_empty() {
        return None;
    }

    if let Some(index) = monitors.iter().position(|monitor| monitor.contains(point)) {
        return Some(index);
    }

    let mut best_index = 0usize;
    let mut best_distance = f64::INFINITY;

    for (index, monitor) in monitors.iter().enumerate() {
        let dx = if point.x < monitor.origin.x {
            monitor.origin.x - point.x
        } else if point.x > monitor.origin.x + monitor.width {
            point.x - (monitor.origin.x + monitor.width)
        } else {
            0.0
        };

        let dy = if point.y < monitor.origin.y {
            monitor.origin.y - point.y
        } else if point.y > monitor.origin.y + monitor.height {
            point.y - (monitor.origin.y + monitor.height)
        } else {
            0.0
        };

        let distance = dx * dx + dy * dy;
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }

    Some(best_index)
}
