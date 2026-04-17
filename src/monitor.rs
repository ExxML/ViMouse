use crate::state::{MonitorInfo, Point};
use winit::monitor::MonitorHandle;
use winit::window::Window;

// Keep monitor bounds in the same desktop coordinate space as our cursor hook.
pub fn collect_monitors(window: &Window) -> Vec<MonitorInfo> {
    let mut monitors: Vec<_> = window
        .available_monitors()
        .map(|monitor| monitor_info(&monitor))
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

#[cfg(target_os = "macos")]
fn monitor_info(monitor: &MonitorHandle) -> MonitorInfo {
    let scale_factor = monitor.scale_factor();
    let origin = monitor.position().to_logical::<f64>(scale_factor);
    let size = monitor.size().to_logical::<f64>(scale_factor);

    MonitorInfo {
        origin: Point {
            x: origin.x,
            y: origin.y,
        },
        width: size.width,
        height: size.height,
        scale_factor,
    }
}

#[cfg(not(target_os = "macos"))]
fn monitor_info(monitor: &MonitorHandle) -> MonitorInfo {
    let origin = monitor.position();
    let size = monitor.size();

    MonitorInfo {
        origin: Point {
            x: origin.x as f64,
            y: origin.y as f64,
        },
        width: size.width as f64,
        height: size.height as f64,
        scale_factor: monitor.scale_factor(),
    }
}

fn clamp_axis(value: f64, start: f64, length: f64) -> f64 {
    let end = (start + length - 1.0).max(start);
    value.clamp(start, end)
}

fn clamp_point_to_monitor(point: Point, monitor: &MonitorInfo) -> Point {
    Point {
        x: clamp_axis(point.x, monitor.origin.x, monitor.width),
        y: clamp_axis(point.y, monitor.origin.y, monitor.height),
    }
}

fn distance_squared(left: Point, right: Point) -> f64 {
    let dx = left.x - right.x;
    let dy = left.y - right.y;
    dx * dx + dy * dy
}

fn nearest_monitor(monitors: &[MonitorInfo], point: Point) -> Option<(usize, Point)> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;

    for (index, monitor) in monitors.iter().enumerate() {
        if monitor.contains(point) {
            return Some((index, point));
        }

        let clamped = clamp_point_to_monitor(point, monitor);
        let distance = distance_squared(point, clamped);

        if distance < best_distance {
            best = Some((index, clamped));
            best_distance = distance;
        }
    }

    best
}

// Clamp movement to the actual monitor rectangles so the cursor stays on-screen.
pub fn clamp_to_virtual_bounds(point: &mut Point, monitors: &[MonitorInfo]) {
    if let Some((_, clamped)) = nearest_monitor(monitors, *point) {
        *point = clamped;
    }
}

// If the cursor is between monitors, pick the nearest one for overlay placement.
pub fn monitor_index_for_point(monitors: &[MonitorInfo], point: Point) -> Option<usize> {
    nearest_monitor(monitors, point).map(|(index, _)| index)
}
