// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure mapping from application state to what the widget draws.

use crate::format::{format_remaining, level, Level};
use crate::usage::{Limits, Window};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    SigningIn,
    Loading,
    SignedOut,
    NoLimits,
    FormatChanged,
    /// Offline with no prior data.
    Offline,
    Data { limits: Limits, stale: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub pct: f64,
    /// Bar fill, clamped to 0..=1.
    pub fill: f64,
    pub pct_text: String,
    pub remaining: String,
    pub level: Level,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Fresh,
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewModel {
    Message(String),
    Rows { rows: Vec<Row>, status: Status, note: Option<String> },
}

pub const MSG_SIGNING_IN: &str = "Signing in…";
pub const MSG_LOADING: &str = "Loading…";
pub const MSG_OFFLINE: &str = "Offline — retrying";
pub const MSG_SIGNED_OUT: &str = "Signed out — click to sign in";
pub const MSG_FORMAT: &str = "Usage format changed — check for an update";
pub const MSG_NO_LIMITS: &str = "No usage limits on this plan";

pub fn build_view(state: &AppState, now: DateTime<Utc>) -> ViewModel {
    let message = |m: &str| ViewModel::Message(m.to_string());
    match state {
        AppState::SigningIn => message(MSG_SIGNING_IN),
        AppState::Loading => message(MSG_LOADING),
        AppState::SignedOut => message(MSG_SIGNED_OUT),
        AppState::NoLimits => message(MSG_NO_LIMITS),
        AppState::FormatChanged => message(MSG_FORMAT),
        AppState::Offline => message(MSG_OFFLINE),
        AppState::Data { limits, stale } => {
            let row = |label: &str, w: &Window| Row {
                label: label.to_string(),
                pct: w.utilization,
                fill: (w.utilization / 100.0).clamp(0.0, 1.0),
                pct_text: format!("{}%", w.utilization.round() as i64),
                remaining: format_remaining(now, w.resets_at),
                level: level(w.utilization),
            };
            let mut rows = vec![row("Session", &limits.session), row("Weekly", &limits.weekly)];
            rows.extend(limits.extra.iter().map(|(l, w)| row(l, w)));
            ViewModel::Rows {
                rows,
                status: if *stale { Status::Stale } else { Status::Fresh },
                note: stale.then(|| MSG_OFFLINE.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Window;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).unwrap()
    }

    fn data(stale: bool, weekly_pct: f64) -> AppState {
        AppState::Data {
            limits: Limits {
                session: Window { utilization: 39.0, resets_at: Some(now() + Duration::minutes(112)) },
                weekly: Window { utilization: weekly_pct, resets_at: None },
                extra: vec![],
            },
            stale,
        }
    }

    fn msg(s: &AppState) -> String {
        match build_view(s, now()) {
            ViewModel::Message(m) => m,
            other => panic!("expected message, got {other:?}"),
        }
    }

    #[test]
    fn view_states() {
        assert_eq!(msg(&AppState::SignedOut), "Signed out — click to sign in");
        assert_eq!(msg(&AppState::NoLimits), "No usage limits on this plan");
        assert_eq!(msg(&AppState::SigningIn), "Signing in…");
        assert_eq!(msg(&AppState::Loading), "Loading…");
        assert_eq!(msg(&AppState::Offline), "Offline — retrying");
        assert_eq!(msg(&AppState::FormatChanged), "Usage format changed — check for an update");

        match build_view(&data(true, 48.0), now()) {
            ViewModel::Rows { rows, status, note } => {
                assert_eq!(status, Status::Stale);
                assert_eq!(note.as_deref(), Some("Offline — retrying"));
                assert_eq!(rows[0].label, "Session");
                assert_eq!(rows[1].label, "Weekly");
                assert_eq!(rows[0].pct_text, "39%");
                assert_eq!(rows[0].remaining, "1h 52m");
                assert_eq!(rows[1].remaining, "—");
            }
            other => panic!("{other:?}"),
        }
        match build_view(&data(false, 48.0), now()) {
            ViewModel::Rows { status, note, .. } => {
                assert_eq!(status, Status::Fresh);
                assert_eq!(note, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn overflow_clamps_fill_but_shows_real_percent() {
        match build_view(&data(false, 130.0), now()) {
            ViewModel::Rows { rows, .. } => {
                assert_eq!(rows[1].fill, 1.0);
                assert_eq!(rows[1].pct_text, "130%");
                assert_eq!(rows[1].level, Level::Red);
                assert!((rows[0].fill - 0.39).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
    }
}
