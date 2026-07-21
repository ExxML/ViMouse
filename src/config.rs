use rdev::Key;

// Overlay configuration
#[allow(dead_code)]
pub enum IconOverlayPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
pub const ICON_OVERLAY_POSITION: IconOverlayPos = IconOverlayPos::BottomLeft;
pub const ICON_OVERLAY_SIZE_MONITOR_FRACTION: f64 = 0.044; // Icon edge length as a fraction of the monitor's smaller dimension

// Frequency of the motion loop that emits synthetic mouse/scroll events
// Recommend setting this to match the refresh rate of your display
pub const TICK_RATE_HZ: u64 = 240;

pub const ACCEL_DELAY_SECS: f64 = 0.13; // seconds before cursor/scroll acceleration starts

// Cursor speeds are logical points/sec (DPI-normalized), so apparent speed stays constant across
// monitors of differing DPI. See movement_device_scale in platform_input.rs.
pub const CURSOR_BASE_SPEED: f64 = 50.0; // logical pts/sec during initial hold (before acceleration starts)
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
pub const KEY_SCROLL: Key = Key::ShiftLeft;

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
// Toggle jump grid overlay
pub const KEY_TOGGLE_GRID: Key = Key::Slash;
// Toggle grid cell letter overlay
pub const KEY_TOGGLE_GRID_LETTERS: Key = Key::Dot;
// Toggle all overlays (icon, grid, and letters) (available in both Normal and Insert mode)
pub const KEY_TOGGLE_OVERLAY: Key = Key::AltGr;

// Grid overlay line appearance
pub const DEFAULT_GRID_ENABLED: bool = false; // Whether the grid overlay is enabled by default
pub const GRID_THICKNESS: usize = 1; // Thickness of grid lines in pixels
pub const GRID_ALPHA: u8 = 160; // Opacity of grid lines (0–255)
pub const GRID_BRIGHTNESS: u8 = 128; // RGB channel value of grid lines (greyscale intensity 0-255)

// Overlay letter appearance
pub const DEFAULT_GRID_LETTER_ENABLED: bool = false; // Whether the grid letters are enabled by default
pub const OVERLAY_LETTER_SIZE_MONITOR_FRACTION: f64 = 0.008; // Overlay letter height as a fraction of the monitor's smaller dimension
pub const OVERLAY_LETTER_ALPHA: u8 = 224; // Opacity of overlay letters (0–255)
pub const OVERLAY_LETTER_BRIGHTNESS: u8 = 255; // RGB channel value of overlay letters (greyscale intensity 0-255)
pub const OVERLAY_LETTER_OUTLINE_THICKNESS: usize = 1; // Thickness of overlay letter outlines in pixels
pub const OVERLAY_LETTER_OUTLINE_ALPHA: u8 = 224; // Opacity of overlay letter outlines (0–255)
pub const OVERLAY_LETTER_OUTLINE_BRIGHTNESS: u8 = 0; // RGB channel value of overlay letter outlines (greyscale intensity 0-255)

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
