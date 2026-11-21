//! Fake usage data for `--demo` (debug builds only), used for screenshots and the
//! automated launch/leak checks. Numbers creep up on each refresh so changes are visible.

use crate::source::{FetchError, UsageSource};
use crate::usage::{Limits, Org, UsageState, Window};
use chrono::{Duration, Utc};

#[derive(Default)]
pub struct DemoSource {
    n: u32,
}

impl UsageSource for DemoSource {
    fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError> {
        Ok(vec![
            Org { uuid: "demo-1".into(), name: "Personal (demo)".into() },
            Org { uuid: "demo-2".into(), name: "Team (demo)".into() },
        ])
    }

    fn fetch_usage(&mut self, _org: &str) -> Result<UsageState, FetchError> {
        self.n += 1;
        let now = Utc::now();
        Ok(UsageState::Limits(Limits {
            session: Window {
                utilization: (60.0 + 8.0 * self.n as f64).min(120.0),
                resets_at: Some(now + Duration::minutes(112)),
            },
            weekly: Window { utilization: 31.0, resets_at: Some(now + Duration::hours(4 * 24 + 6)) },
            extra: vec![],
        }))
    }
}
