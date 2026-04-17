use rdev::Key;

#[allow(dead_code)]
pub enum OverlayPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub const OVERLAY_CORNER: OverlayPos = OverlayPos::BottomLeft;

pub const MOVE_SPEED_PX_PER_SEC: f64 = 800.0;
pub const SCROLL_SPEED_UNITS_PER_SEC: f64 = 25.0;
pub const FAST_MULTIPLIER: f64 = 2.0;
pub const SLOW_MULTIPLIER: f64 = 0.3;
pub const TICK_RATE_HZ: u64 = 240;
pub const OVERLAY_SIZE_MONITOR_FRACTION: f64 = 0.044;

// Mode switching
pub const KEY_INSERT_MODE: Key = Key::KeyI;
pub const KEY_NORMAL_MODE: Key = Key::Escape; // Recommend using a non-text key

// Cursor movement keys
pub const KEY_MOVE_LEFT: Key = Key::KeyH;
pub const KEY_MOVE_DOWN: Key = Key::KeyJ;
pub const KEY_MOVE_UP: Key = Key::KeyK;
pub const KEY_MOVE_RIGHT: Key = Key::KeyL;

// Mouse button keys
pub const KEY_LEFT_CLICK: Key = Key::SemiColon;
pub const KEY_RIGHT_CLICK: Key = Key::Quote;
pub const KEY_SCROLL: Key = Key::ShiftLeft; // Recommend using a modifier or non-text key

// Speed modifier keys (recommend using modifier or non-text keys)
pub const KEY_FAST: Key = Key::Space;
pub const KEY_SLOW: Key = Key::Alt;

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

// Quit chord
pub const KEYS_QUIT: &[Key] = &[Key::ControlLeft, Key::ShiftLeft, Key::KeyQ];
