use crate::platform_cursor;
use crate::state::{MonitorInfo, Point};
use winit::window::Window;

// Winit gives us monitor geometry in virtual desktop coordinates.
pub fn collect_monitors(window: &Window) -> Vec<MonitorInfo> {
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

    monitors
}

pub fn initial_cursor(monitors: &[MonitorInfo]) -> Point {
    // Prefer the real cursor location so startup does not snap unexpectedly.
    if let Some((x, y)) = platform_cursor::current_cursor_position() {
        return Point { x, y };
    }

    monitors
        .first()
        .copied()
        .expect("no monitors available")
        .center()
}

// Clamp movement to the union of all monitors so the cursor stays on-screen.
pub fn clamp_to_virtual_bounds(point: &mut Point, monitors: &[MonitorInfo]) {
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

// If the cursor is between monitors, pick the nearest one for overlay placement.
pub fn monitor_index_for_point(monitors: &[MonitorInfo], point: Point) -> Option<usize> {
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
