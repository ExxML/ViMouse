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

pub const ACCEL_DELAY_SECS: f64 = 0.15; // seconds before cursor/scroll acceleration starts

pub const CURSOR_BASE_SPEED: f64 = 100.0; // px/sec during initial hold (before acceleration starts)
pub const CURSOR_ACCELERATION: f64 = f64::INFINITY; // additional px/sec² after ACCEL_DELAY_SECS
pub const CURSOR_MAX_SPEED: f64 = 500.0; // px/sec ceiling when accelerating

pub const SCROLL_BASE_SPEED: f64 = 10.0; // scroll units/sec during initial hold (before acceleration starts)
pub const SCROLL_ACCELERATION: f64 = 0.0; // additional scroll units/sec² after ACCEL_DELAY_SECS
pub const SCROLL_MAX_SPEED: f64 = f64::INFINITY; // scroll units/sec ceiling when accelerating

// Most keys below are subject to capture/suppression - see handle_key_press() comment in input.rs for details
// Generally, the keybinds with the comment "Recommend using a modifier or non-text key" are not captured/suppressed

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
pub const KEY_SCROLL: Key = Key::ShiftLeft; // Recommend using a modifier or non-text key

// Speed modifier keys for cursor movement and scrolling (recommend using modifier or non-text keys)
pub const KEY_FAST: Key = Key::Space;
pub const KEY_SLOW: Key = Key::Alt;
// Speed multipliers
pub const FAST_MULTIPLIER: f64 = 3.0;
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
// Seconds to wait for the second jump grid key (subcell jump)
// - 0.0 to disable subcell jumps
// - f64::INFINITY to keep the subcell jump primed indefinitely
pub const JUMP_GRID_DELAY: f64 = 1.0;
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
pub const KEY_UNMARK: Key = Key::ShiftLeft; // Recommend using a modifier or non-text key
pub const CHORD_UNMARK_ALL: &[Key] = &[Key::ShiftLeft, Key::BackQuote];

// Quit chord (available in both Normal and Insert mode)
pub const CHORD_QUIT: &[Key] = &[Key::ControlLeft, Key::Alt, Key::ShiftLeft, Key::KeyQ];
