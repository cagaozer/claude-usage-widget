// SPDX-License-Identifier: GPL-3.0-or-later
//! Countdown text and bar color thresholds.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Green,
    Amber,
    Red,
}

pub fn format_remaining(now: DateTime<Utc>, resets_at: Option<DateTime<Utc>>) -> String {
    let Some(at) = resets_at else { return "—".into() };
    let secs = (at - now).num_seconds();
    if secs <= 0 {
        return "resetting…".into();
    }
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h, m) {
        (0, 0, 0) => "<1m".into(),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

pub fn level(pct: f64) -> Level {
    if pct >= 90.0 {
        Level::Red
    } else if pct >= 75.0 {
        Level::Amber
    } else {
        Level::Green
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).unwrap()
    }

    fn rem(d: Duration) -> String {
        format_remaining(now(), Some(now() + d))
    }

    #[test]
    fn format_remaining_cases() {
        assert_eq!(rem(Duration::minutes(112)), "1h 52m");
        assert_eq!(rem(Duration::days(4) + Duration::hours(6) + Duration::minutes(10)), "4d 6h");
        assert_eq!(rem(Duration::seconds(12 * 60 + 30)), "12m");
        assert_eq!(rem(Duration::seconds(40)), "<1m");
    }

    #[test]
    fn remaining_past_or_none() {
        assert_eq!(rem(Duration::seconds(-5)), "resetting…");
        assert_eq!(rem(Duration::zero()), "resetting…");
        assert_eq!(format_remaining(now(), None), "—");
    }

    #[test]
    fn level_thresholds() {
        assert_eq!(level(74.9), Level::Green);
        assert_eq!(level(75.0), Level::Amber);
        assert_eq!(level(89.9), Level::Amber);
        assert_eq!(level(90.0), Level::Red);
        assert_eq!(level(130.0), Level::Red);
    }
}
