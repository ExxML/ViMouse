use crate::config::{
    DEFAULT_GRID_ENABLED, DEFAULT_GRID_LETTER_ENABLED, DEFAULT_ICON_ENABLED,
    ICON_OVERLAY_INSERT_COLOR, ICON_OVERLAY_NORMAL_COLOR,
};
use rdev::{Button, Key};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;
use winit::event_loop::EventLoopProxy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

impl Mode {
    pub fn label(self) -> char {
        match self {
            Self::Normal => 'N',
            Self::Insert => 'I',
        }
    }

    pub fn background(self) -> [u8; 4] {
        match self {
            Self::Normal => ICON_OVERLAY_NORMAL_COLOR,
            Self::Insert => ICON_OVERLAY_INSERT_COLOR,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

// Monitor bounds are kept in virtual-desktop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorInfo {
    pub origin: Point,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
}

impl MonitorInfo {
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x < self.origin.x + self.width
            && point.y >= self.origin.y
            && point.y < self.origin.y + self.height
    }

    pub fn center(self) -> Point {
        Point {
            x: self.origin.x + self.width * 0.5,
            y: self.origin.y + self.height * 0.5,
        }
    }
}

// This is the shared runtime state used by the input hook, motion loop, and overlay UI.
pub struct SharedState {
    pub mode: Mode,
    pub cursor: Point,
    pub selected_monitor: usize,
    pub monitors: Vec<MonitorInfo>,
    pub pressed_keys: HashSet<Key>,
    pub left_button_down: bool,
    pub right_button_down: bool,
    pub pending_actions: Vec<Action>,
    pub show_icon: bool,
    pub show_grid: bool,
    pub show_grid_letters: bool,
    pub show_overlays: bool,
    pub motion_needed: bool,
    pub move_key_pressed_at: HashMap<Key, Instant>,
    pub pending_subcell: Option<(u8, u8, Instant)>, // Pending subcell state: (cell_col, cell_row, timestamp of first jump)
    pub marks: HashMap<Key, Point>, // Mark key -> cursor position in virtual-desktop coords
    pub emitted_cursor: Option<Point>,
}

impl SharedState {
    pub fn new(cursor: Point, selected_monitor: usize, monitors: Vec<MonitorInfo>) -> Self {
        Self {
            mode: Mode::Normal,
            cursor,
            selected_monitor,
            monitors,
            pressed_keys: HashSet::new(),
            left_button_down: false,
            right_button_down: false,
            pending_actions: Vec::new(),
            show_icon: DEFAULT_ICON_ENABLED,
            show_grid: DEFAULT_GRID_ENABLED,
            show_grid_letters: DEFAULT_GRID_LETTER_ENABLED,
            show_overlays: true,
            motion_needed: true,
            move_key_pressed_at: HashMap::new(),
            pending_subcell: None,
            marks: HashMap::new(),
            emitted_cursor: None,
        }
    }
}

pub type Shared = Arc<Mutex<SharedState>>;
pub type MotionWaker = Arc<Condvar>;
// Sends a () event to wake the winit event loop when UI-visible state changes.
pub type UiWaker = EventLoopProxy<()>;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    MouseMove(Point),
    Scroll { delta_x: f64, delta_y: f64 },
    ButtonPress(Button),
    ButtonRelease(Button),
}
