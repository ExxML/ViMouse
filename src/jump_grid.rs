use crate::config::JUMP_GRID;
use crate::state::{MonitorInfo, Point};
use rdev::Key;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpGridMetrics {
    columns: usize,
    rows: usize,
    cell_width: f64,
    cell_height: f64,
}

impl JumpGridMetrics {
    pub fn column_boundaries(self) -> impl Iterator<Item = f64> {
        (1..self.columns).map(move |column| column as f64 * self.cell_width)
    }

    pub fn row_boundaries(self) -> impl Iterator<Item = f64> {
        (1..self.rows).map(move |row| row as f64 * self.cell_height)
    }

    pub fn target(self, monitor: MonitorInfo, row: usize, column: usize) -> Point {
        Point {
            x: monitor.origin.x + (column as f64 + 0.5) * self.cell_width,
            y: monitor.origin.y + (row as f64 + 0.5) * self.cell_height,
        }
    }
}

pub fn metrics_for_size(width: f64, height: f64) -> JumpGridMetrics {
    JumpGridMetrics {
        columns: JUMP_GRID[0].len(),
        rows: JUMP_GRID.len(),
        cell_width: width / JUMP_GRID[0].len() as f64,
        cell_height: height / JUMP_GRID.len() as f64,
    }
}

pub fn jump_target(monitor: MonitorInfo, key: Key) -> Option<Point> {
    let (row, column) = jump_cell(key)?;
    Some(metrics_for_size(monitor.width, monitor.height).target(monitor, row, column))
}

fn jump_cell(key: Key) -> Option<(usize, usize)> {
    for (row, keys) in JUMP_GRID.iter().enumerate() {
        for (column, cell_key) in keys.iter().enumerate() {
            if *cell_key == key {
                return Some((row, column));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{jump_target, metrics_for_size};
    use crate::state::{MonitorInfo, Point};
    use rdev::Key;

    #[test]
    fn boundaries_match_grid_shape() {
        let metrics = metrics_for_size(500.0, 300.0);

        assert_eq!(
            metrics.column_boundaries().collect::<Vec<_>>(),
            vec![100.0, 200.0, 300.0, 400.0]
        );
        assert_eq!(
            metrics.row_boundaries().collect::<Vec<_>>(),
            vec![100.0, 200.0]
        );
    }

    #[test]
    fn jump_target_uses_cell_center() {
        let monitor = MonitorInfo {
            origin: Point { x: 10.0, y: 20.0 },
            width: 500.0,
            height: 300.0,
            scale_factor: 1.0,
        };

        assert_eq!(
            jump_target(monitor, Key::KeyF),
            Some(Point { x: 360.0, y: 170.0 })
        );
    }
}
