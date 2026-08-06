use rdev::Key;

// Frequency of the motion loop that emits synthetic mouse/scroll events
// Recommend setting this to match the refresh rate of your display
pub const TICK_RATE_HZ: u64 = 240;

pub const ACCEL_DELAY_SECS: f64 = 0.13; // seconds before cursor/scroll acceleration starts

// Cursor speeds are logical points/sec (DPI-normalized), so apparent speed stays constant across
// monitors of differing DPI. See movement_device_scale in platform_input.rs.
pub const CURSOR_BASE_SPEED: f64 = 60.0; // logical pts/sec during initial hold (before acceleration starts)
pub const CURSOR_ACCELERATION: f64 = f64::INFINITY; // additional logical pts/sec² after ACCEL_DELAY_SECS
pub const CURSOR_MAX_SPEED: f64 = 300.0; // logical pts/sec ceiling when accelerating

pub const SCROLL_BASE_SPEED: f64 = 10.0; // scroll units/sec during initial hold (before acceleration starts)
pub const SCROLL_ACCELERATION: f64 = 0.0; // additional scroll units/sec² after ACCEL_DELAY_SECS
pub const SCROLL_MAX_SPEED: f64 = f64::INFINITY; // scroll units/sec ceiling when accelerating

// In Normal mode every key is captured by ViMouse except OS modifiers (Ctrl/Alt/Shift/Meta),
// KEYS_EXEMPT, and unmapped keys; holding an OS modifier first leaks subsequent keys to the OS.
// See handle_key_press() in input.rs for the full decision order.

// Unless otherwise specified, ViMouse keybinds only work in Normal mode; Insert mode is reserved for typing.

// Mode switching
pub const KEY_INSERT_MODE: Key = Key::KeyI; // Only works in Normal mode
pub const KEY_NORMAL_MODE: Key = Key::CapsLock; // Only works in Insert mode
pub const INSERT_MODE_HIDE_CURSOR: bool = false; // Hide the mouse cursor in Insert mode (restored in Normal mode)

// Cursor movement keys
pub const KEY_MOVE_LEFT: Key = Key::KeyH;
pub const KEY_MOVE_DOWN: Key = Key::KeyJ;
pub const KEY_MOVE_UP: Key = Key::KeyK;
pub const KEY_MOVE_RIGHT: Key = Key::KeyL;

// Mouse button keys
pub const KEY_MOUSE_1: Key = Key::SemiColon; // Left click
pub const KEY_MOUSE_2: Key = Key::Quote; // Right click
pub const KEY_MOUSE_3: Key = Key::KeyM; // Middle (scroll) click
pub const KEY_MOUSE_4: Key = Key::KeyO; // Back (X1) click
pub const KEY_MOUSE_5: Key = Key::KeyP; // Forward (X2) click
pub const KEYS_SCROLL: &[Key] = &[Key::ShiftLeft, Key::ShiftRight]; // Hold any KEYS_SCROLL to scroll

// Speed modifier keys for cursor movement and scrolling
pub const KEY_FAST: Key = Key::Space;
pub const KEY_SLOW: Key = Key::Alt;
// Speed multipliers
pub const FAST_MULTIPLIER: f64 = 5.0;
pub const SLOW_MULTIPLIER: f64 = 0.5;

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
// Seconds to wait for the second jump grid key (subcell jump)
// - 0.0 to disable subcell jumps
// - f64::INFINITY to keep the subcell jump primed indefinitely
pub const JUMP_GRID_DELAY: f64 = f64::INFINITY;

// Overlay visibility
pub const KEY_TOGGLE_MODE_LINE: Key = Key::Comma; // Toggle mode overlay line
pub const KEY_TOGGLE_GRID: Key = Key::Dot; // Toggle jump grid overlay
pub const KEY_TOGGLE_GRID_LETTERS: Key = Key::Slash; // Toggle grid cell letter overlay
pub const KEY_TOGGLE_OVERLAY: Key = Key::AltGr; // Toggle all overlays (available in both Normal and Insert mode)

// Mode overlay appearance
pub const DEFAULT_MODE_LINE_ENABLED: bool = true; // Whether the mode overlay line is enabled by default
#[allow(dead_code)]
pub enum ModeOverlayPos {
    Top,
    Right,
    Left,
    Bottom,
}
pub const MODE_OVERLAY_POSITION: ModeOverlayPos = ModeOverlayPos::Bottom;
pub const MODE_OVERLAY_THICKNESS: usize = 3; // Thickness of mode overlay line in pixels
pub const MODE_OVERLAY_NORMAL_COLOR: [u8; 4] = [40, 150, 230, 255];
pub const MODE_OVERLAY_INSERT_COLOR: [u8; 4] = [230, 150, 40, 255];

// Grid overlay line appearance
pub const DEFAULT_GRID_ENABLED: bool = false; // Whether the grid overlay is enabled by default
pub const GRID_THICKNESS: usize = 1; // Thickness of grid lines in pixels
pub const GRID_ALPHA: u8 = 160; // Opacity of grid lines (0–255)
pub const GRID_BRIGHTNESS: u8 = 128; // RGB channel value of grid lines (greyscale intensity 0-255)

// Letter overlay appearance
pub const DEFAULT_GRID_LETTER_ENABLED: bool = false; // Whether the grid letters are enabled by default
pub const LETTER_OVERLAY_SIZE_MONITOR_FRACTION: f64 = 0.008; // Letter overlay height as a fraction of the monitor's smaller dimension
pub const LETTER_OVERLAY_ALPHA: u8 = 224; // Opacity of letter overlays (0–255)
pub const LETTER_OVERLAY_BRIGHTNESS: u8 = 255; // RGB channel value of letter overlays (greyscale intensity 0-255)
pub const LETTER_OVERLAY_OUTLINE_THICKNESS: usize = 1; // Thickness of letter overlay outlines in pixels
pub const LETTER_OVERLAY_OUTLINE_ALPHA: u8 = 224; // Opacity of letter overlay outlines (0–255)
pub const LETTER_OVERLAY_OUTLINE_BRIGHTNESS: u8 = 0; // RGB channel value of letter overlay outlines (greyscale intensity 0-255)

// Mark keys
pub const KEYS_MARK: &[Key] = &[
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
    Key::Num0,
];
pub const KEY_UNMARK: Key = Key::ShiftLeft; // Hold + KEYS_MARK to unmark; recommend using a modifier or non-text key
pub const KEY_UNMARK_ALL: Key = Key::BackQuote; // KEY_UNMARK + KEY_UNMARK_ALL to remove all marks

// Keys that always pass through to the OS, even in Normal mode. Unmappable keys pass through by default.
pub const KEYS_EXEMPT: &[Key] = &[
    Key::Return,
    Key::Escape,
    Key::Tab,
    Key::Backspace,
    Key::Delete,
    Key::Insert,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::UpArrow,
    Key::DownArrow,
    Key::LeftArrow,
    Key::RightArrow,
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::PrintScreen,
    Key::ScrollLock,
    Key::Pause,
    Key::NumLock,
    Key::Kp0,
    Key::Kp1,
    Key::Kp2,
    Key::Kp3,
    Key::Kp4,
    Key::Kp5,
    Key::Kp6,
    Key::Kp7,
    Key::Kp8,
    Key::Kp9,
    Key::KpReturn,
    Key::KpMinus,
    Key::KpPlus,
    Key::KpMultiply,
    Key::KpDivide,
    Key::KpDelete,
];

// Quit chord (available in both Normal and Insert mode)
pub const CHORD_QUIT: &[Key] = &[Key::ControlLeft, Key::Alt, Key::ShiftLeft, Key::KeyQ];
