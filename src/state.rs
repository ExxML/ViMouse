use rdev::{Button, Key};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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
            Self::Normal => [30, 160, 98, 255],
            Self::Insert => [44, 55, 72, 255],
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
    pub scroll_remainder: Point,
    pub pending_actions: Vec<Action>,
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
            scroll_remainder: Point::default(),
            pending_actions: Vec::new(),
        }
    }
}

pub type Shared = Arc<Mutex<SharedState>>;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    MouseMove(Point),
    Scroll { delta_x: i64, delta_y: i64 },
    ButtonPress(Button),
    ButtonRelease(Button),
}
