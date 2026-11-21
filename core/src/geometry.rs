//! Keep the widget on a visible monitor.

/// A screen rectangle in pixels (Windows) or points (macOS), y growing downwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const MARGIN: i32 = 16;

/// Top-right corner of the primary work area, inset by `MARGIN`.
pub fn default_position(size: (i32, i32), primary: Rect) -> (i32, i32) {
    (primary.right - size.0 - MARGIN, primary.top + MARGIN)
}

fn overlap_area(a: &Rect, b: &Rect) -> i64 {
    let w = (a.right.min(b.right) - a.left.max(b.left)).max(0) as i64;
    let h = (a.bottom.min(b.bottom) - a.top.max(b.top)).max(0) as i64;
    w * h
}

/// Return a position where the whole widget is visible: unchanged if already inside a
/// work area, pulled inside the most-overlapping area if partly visible, otherwise the
/// default position on the primary monitor.
pub fn clamp_to_work_areas(pos: (i32, i32), size: (i32, i32), areas: &[Rect], primary: Rect) -> (i32, i32) {
    let win = Rect { left: pos.0, top: pos.1, right: pos.0 + size.0, bottom: pos.1 + size.1 };
    let best = areas
        .iter()
        .map(|a| (overlap_area(&win, a), a))
        .filter(|(o, _)| *o > 0)
        .max_by_key(|(o, _)| *o);
    match best {
        Some((_, a)) => (
            pos.0.clamp(a.left, (a.right - size.0).max(a.left)),
            pos.1.clamp(a.top, (a.bottom - size.1).max(a.top)),
        ),
        None => default_position(size, primary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }

    const SIZE: (i32, i32) = (230, 56);

    #[test]
    fn inside_unchanged() {
        let primary = r(0, 0, 1920, 1040);
        assert_eq!(clamp_to_work_areas((100, 100), SIZE, &[primary], primary), (100, 100));
        // On a second monitor to the left.
        let left = r(-1280, 0, 0, 1024);
        assert_eq!(clamp_to_work_areas((-500, 50), SIZE, &[primary, left], primary), (-500, 50));
    }

    #[test]
    fn clamp_offscreen_position() {
        let primary = r(0, 0, 1920, 1040);
        // Saved on a monitor that's gone.
        assert_eq!(clamp_to_work_areas((3000, 200), SIZE, &[primary], primary), (1920 - 230 - 16, 16));
        assert_eq!(default_position(SIZE, primary), (1674, 16));
    }

    #[test]
    fn clamp_partially_visible() {
        let primary = r(0, 0, 1920, 1040);
        assert_eq!(clamp_to_work_areas((1800, 1020), SIZE, &[primary], primary), (1920 - 230, 1040 - 56));
        assert_eq!(clamp_to_work_areas((-50, -10), SIZE, &[primary], primary), (0, 0));
    }
}
