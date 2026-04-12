use rdev::Key;

pub const MOVE_SPEED_PX_PER_SEC: f64 = 800.0;
pub const SCROLL_SPEED_UNITS_PER_SEC: f64 = 500.0;
pub const FAST_MULTIPLIER: f64 = 2.0;
pub const SLOW_MULTIPLIER: f64 = 0.3;
pub const TICK_RATE_HZ: u64 = 120;
pub const OVERLAY_SIZE: u32 = 64;

// Movement keys (cursor direction)
pub const KEY_MOVE_LEFT: Key = Key::KeyH;
pub const KEY_MOVE_DOWN: Key = Key::KeyJ;
pub const KEY_MOVE_UP: Key = Key::KeyK;
pub const KEY_MOVE_RIGHT: Key = Key::KeyL;

// Mouse button keys
pub const KEY_LEFT_CLICK: Key = Key::SemiColon;
pub const KEY_RIGHT_CLICK: Key = Key::CapsLock;

// Mode switching
pub const KEY_INSERT_MODE: Key = Key::KeyI;
pub const KEY_NORMAL_MODE: Key = Key::Escape;

// Monitor cycling
pub const KEY_CYCLE_MONITOR: Key = Key::BackQuote;

// Quit chord key (combined with Ctrl+Shift)
pub const KEY_QUIT: Key = Key::KeyQ;

// Speed modifier keys
pub const KEYS_FAST: &[Key] = &[Key::ControlLeft, Key::ControlRight];
pub const KEYS_SLOW: &[Key] = &[Key::Alt, Key::AltGr];
pub const KEYS_SCROLL: &[Key] = &[Key::ShiftLeft, Key::ShiftRight];

// Jump grid (5 columns × 3 rows, read left-to-right, top-to-bottom)
// Row 0: Q  W  E  R  T
// Row 1: A  S  D  F  G
// Row 2: Z  X  C  V  B
pub const JUMP_GRID: [[Key; 5]; 3] = [
    [Key::KeyQ, Key::KeyW, Key::KeyE, Key::KeyR, Key::KeyT],
    [Key::KeyA, Key::KeyS, Key::KeyD, Key::KeyF, Key::KeyG],
    [Key::KeyZ, Key::KeyX, Key::KeyC, Key::KeyV, Key::KeyB],
];
