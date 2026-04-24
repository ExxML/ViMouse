use rdev::Key;

// Overlay configuration
#[allow(dead_code)]
pub enum OverlayIconPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
pub const OVERLAY_ICON_POSITION: OverlayIconPos = OverlayIconPos::BottomLeft;
pub const OVERLAY_ICON_SIZE_MONITOR_FRACTION: f64 = 0.044;

// Frequency of the motion loop that emits synthetic mouse/scroll events
// Recommend setting this to match the refresh rate of your display
pub const TICK_RATE_HZ: u64 = 240;

pub const ACCEL_DELAY_SECS: f64 = 0.15; // seconds before cursor/scroll acceleration starts

pub const CURSOR_BASE_SPEED: f64 = 100.0; // px/sec during initial hold (before acceleration starts)
pub const CURSOR_ACCELERATION: f64 = 2600.0; // additional px/sec² after ACCEL_DELAY_SECS
pub const CURSOR_MAX_SPEED: f64 = 2400.0; // px/sec ceiling when accelerating

pub const SCROLL_BASE_SPEED: f64 = 15.0; // scroll units/sec during initial hold (before acceleration starts)
pub const SCROLL_ACCELERATION: f64 = 200.0; // additional scroll units/sec² after ACCEL_DELAY_SECS
pub const SCROLL_MAX_SPEED: f64 = 120.0; // scroll units/sec ceiling when accelerating

// Mode switching
pub const KEY_INSERT_MODE: Key = Key::KeyI;
pub const KEY_NORMAL_MODE: Key = Key::CapsLock; // Recommend using a non-text key

// Cursor movement keys
pub const KEY_MOVE_LEFT: Key = Key::KeyH;
pub const KEY_MOVE_DOWN: Key = Key::KeyJ;
pub const KEY_MOVE_UP: Key = Key::KeyK;
pub const KEY_MOVE_RIGHT: Key = Key::KeyL;

// Mouse button keys
pub const KEY_LEFT_CLICK: Key = Key::SemiColon;
pub const KEY_RIGHT_CLICK: Key = Key::Quote;
pub const KEY_SCROLL: Key = Key::ShiftLeft; // Recommend using a modifier or non-text key

// Speed modifier keys for cursor movement and scrolling (recommend using modifier or non-text keys)
pub const KEY_FAST: Key = Key::Space;
pub const KEY_SLOW: Key = Key::Alt;
// Speed multipliers
pub const FAST_MULTIPLIER: f64 = 2.0;
pub const SLOW_MULTIPLIER: f64 = 0.3;

// Monitor cycling
pub const KEY_CYCLE_MONITOR: Key = Key::KeyN;

// Jump grid (5 columns × 3 rows, read left-to-right, top-to-bottom)
// Row 0: Q  W  E  R  T
// Row 1: A  S  D  F  G
// Row 2: Z  X  C  V  B
pub const JUMP_GRID: [[Key; 5]; 3] = [
    [Key::KeyQ, Key::KeyW, Key::KeyE, Key::KeyR, Key::KeyT],
    [Key::KeyA, Key::KeyS, Key::KeyD, Key::KeyF, Key::KeyG],
    [Key::KeyZ, Key::KeyX, Key::KeyC, Key::KeyV, Key::KeyB],
];
// Toggle jump grid overlay (Normal mode only)
pub const KEY_TOGGLE_GRID: Key = Key::ShiftRight;

// Quit chord
pub const KEYS_QUIT: &[Key] = &[Key::ControlLeft, Key::ShiftLeft, Key::KeyQ];
