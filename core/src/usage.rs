//! claude.ai usage response model and parsing.

use chrono::{DateTime, Utc};

/// One rate-limit window. `utilization` is a percent (0..=100, may exceed 100).
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub utilization: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub session: Window,
    pub weekly: Window,
    /// Optional per-model weekly windows, e.g. ("Opus wk", ...).
    pub extra: Vec<(String, Window)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageState {
    Limits(Limits),
    NoLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Org {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

#[derive(serde::Deserialize)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

#[derive(serde::Deserialize)]
struct RawUsage {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    seven_day_opus: Option<RawWindow>,
    seven_day_sonnet: Option<RawWindow>,
}

#[derive(serde::Deserialize)]
struct RawOrg {
    uuid: String,
    #[serde(default)]
    name: Option<String>,
}

impl RawWindow {
    fn into_window(self) -> Option<Window> {
        Some(Window { utilization: self.utilization?, resets_at: self.resets_at })
    }
}

fn object(json: &str) -> Result<serde_json::Value, ParseError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| ParseError(e.to_string()))?;
    if v.is_object() {
        Ok(v)
    } else {
        Err(ParseError("expected a JSON object".into()))
    }
}

pub fn parse_usage(json: &str) -> Result<UsageState, ParseError> {
    let raw: RawUsage = serde_json::from_value(object(json)?).map_err(|e| ParseError(e.to_string()))?;
    let session = raw.five_hour.and_then(RawWindow::into_window);
    let weekly = raw.seven_day.and_then(RawWindow::into_window);
    let (Some(session), Some(weekly)) = (session, weekly) else {
        return Ok(UsageState::NoLimits);
    };
    let extra = [("Opus wk", raw.seven_day_opus), ("Sonnet wk", raw.seven_day_sonnet)]
        .into_iter()
        .filter_map(|(label, w)| Some((label.to_string(), w?.into_window()?)))
        .collect();
    Ok(UsageState::Limits(Limits { session, weekly, extra }))
}

pub fn parse_orgs(json: &str) -> Result<Vec<Org>, ParseError> {
    let raw: Vec<RawOrg> = serde_json::from_str(json).map_err(|e| ParseError(e.to_string()))?;
    Ok(raw
        .into_iter()
        .map(|o| Org { name: o.name.unwrap_or_else(|| o.uuid.clone()), uuid: o.uuid })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRO: &str = include_str!("../../tests/fixtures/usage_pro.json");
    const ORGS: &str = include_str!("../../tests/fixtures/orgs.json");

    fn limits(s: &str) -> Limits {
        match parse_usage(s).expect("parses") {
            UsageState::Limits(l) => l,
            UsageState::NoLimits => panic!("expected limits"),
        }
    }

    #[test]
    fn parses_real_fixture() {
        let l = limits(PRO);
        assert_eq!(l.session.utilization, 39.0);
        assert_eq!(l.weekly.utilization, 48.0);
        assert_eq!(
            l.session.resets_at.unwrap().to_rfc3339(),
            "2026-11-21T03:50:00.185445+00:00"
        );
        assert_eq!(
            l.weekly.resets_at.unwrap().to_rfc3339(),
            "2026-11-25T05:00:00.185464+00:00"
        );
        assert!(l.extra.is_empty());
    }

    #[test]
    fn null_windows_is_no_limits() {
        assert_eq!(
            parse_usage(r#"{"five_hour":null,"seven_day":null}"#),
            Ok(UsageState::NoLimits)
        );
        assert_eq!(parse_usage("{}"), Ok(UsageState::NoLimits));
    }

    #[test]
    fn garbage_is_parse_error() {
        assert!(parse_usage("<html>").is_err());
        assert!(parse_usage("[]").is_err());
    }

    #[test]
    fn utilization_scale_and_overflow() {
        // Spike recorded a 0-100 scale: values are taken as-is, never rescaled.
        let l = limits(r#"{"five_hour":{"utilization":42,"resets_at":null},"seven_day":{"utilization":130.0,"resets_at":null}}"#);
        assert_eq!(l.session.utilization, 42.0);
        assert_eq!(l.weekly.utilization, 130.0);
        assert_eq!(l.session.resets_at, None);
    }

    #[test]
    fn extra_windows_only_when_present() {
        let with = r#"{"five_hour":{"utilization":1,"resets_at":null},"seven_day":{"utilization":2,"resets_at":null},
                       "seven_day_opus":{"utilization":3,"resets_at":"2026-11-25T05:00:00+00:00"},"seven_day_sonnet":null}"#;
        let l = limits(with);
        assert_eq!(l.extra.len(), 1);
        assert_eq!(l.extra[0].0, "Opus wk");
        assert_eq!(l.extra[0].1.utilization, 3.0);
        assert_eq!(limits(PRO).extra.len(), 0);
    }

    #[test]
    fn parses_orgs() {
        let orgs = parse_orgs(ORGS).unwrap();
        assert_eq!(orgs.len(), 2);
        assert_eq!(orgs[0].uuid, "00000000-0000-4000-8000-000000000001");
        assert_eq!(orgs[0].name, "Personal");
        assert!(parse_orgs("{}").is_err());
    }
}
