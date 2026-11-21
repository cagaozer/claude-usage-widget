//! Widget geometry and colors shared by both platforms.
//!
//! Units are logical pixels on Windows (scaled by the monitor DPI at draw time) and
//! points on macOS (scaled by the system), so the widget looks the same on both.

use crate::format::Level;
use crate::view::ViewModel;

/// Widget width.
pub const W: i32 = 252;
/// Inner padding around the content.
pub const PAD: i32 = 8;
/// Height of one usage row.
pub const ROW_H: i32 = 20;
/// Height of the "Offline — retrying" note under the rows.
pub const NOTE_H: i32 = 16;
/// Size of the close (✕) box in the top-right corner.
pub const CLOSE: i32 = 14;

// Column spans (left, right) inside a row. Wide enough for the longest strings
// ("Sonnet wk", "130%", "resetting…"); each platform has a test that measures them.
pub const COL_LABEL: (i32, i32) = (PAD, 74);
pub const COL_BAR: (i32, i32) = (76, 124);
pub const COL_PCT: (i32, i32) = (126, 160);
pub const COL_REMAIN: (i32, i32) = (166, W - CLOSE - 6);

/// An sRGB color.
pub type Rgb = (u8, u8, u8);

pub const BG: Rgb = (0x1F, 0x1F, 0x1E);
pub const TEXT: Rgb = (0xEC, 0xEC, 0xEA);
pub const DIM: Rgb = (0x8A, 0x8A, 0x85);
pub const TRACK: Rgb = (0x3A, 0x3A, 0x38);
pub const GREEN: Rgb = (0x5C, 0xB8, 0x5C);
pub const AMBER: Rgb = (0xE0, 0xA0, 0x30);
pub const RED: Rgb = (0xE0, 0x55, 0x45);

/// Bar color for a usage level. Stale data (offline) is drawn grey so old numbers
/// don't look current.
pub fn level_color(l: Level, stale: bool) -> Rgb {
    if stale {
        return DIM;
    }
    match l {
        Level::Green => GREEN,
        Level::Amber => AMBER,
        Level::Red => RED,
    }
}

/// Height of the widget for what it currently shows. Messages get two rows of room
/// so longer ones can wrap.
pub fn content_height(v: &ViewModel) -> i32 {
    match v {
        ViewModel::Message(_) => PAD * 2 + ROW_H * 2,
        ViewModel::Rows { rows, note, .. } => {
            PAD * 2 + ROW_H * rows.len() as i32 + if note.is_some() { NOTE_H } else { 0 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Level;
    use crate::usage::{Limits, Window};
    use crate::view::{build_view, AppState};
    use chrono::Utc;

    fn rows(stale: bool) -> ViewModel {
        let w = Window { utilization: 10.0, resets_at: None };
        let limits = Limits { session: w.clone(), weekly: w, extra: vec![] };
        build_view(&AppState::Data { limits, stale }, Utc::now())
    }

    #[test]
    fn content_height_matches_windows() {
        assert_eq!(content_height(&ViewModel::Message("x".into())), 56);
        assert_eq!(content_height(&rows(false)), 56);
        assert_eq!(content_height(&rows(true)), 72); // stale adds the "Offline" note line
    }

    #[test]
    fn stale_rows_use_dim() {
        assert_eq!(level_color(Level::Red, true), DIM);
        assert_eq!(level_color(Level::Amber, false), AMBER);
        assert_eq!(level_color(Level::Green, false), GREEN);
        assert_eq!(level_color(Level::Red, false), RED);
    }
}
