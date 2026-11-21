// SPDX-License-Identifier: GPL-3.0-or-later
//! Where usage data comes from. `HttpSource` talks to claude.ai with the user's web session.

use crate::session::Session;
use crate::usage::{parse_orgs, parse_usage, Org, UsageState};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// Session invalid or expired: the user must sign in again.
    Auth,
    /// Rate limited or bot-protection page; retry at the normal interval.
    Blocked(u16),
    Network(String),
    /// Response didn't parse.
    Format(String),
}

pub trait UsageSource: Send {
    fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError>;
    fn fetch_usage(&mut self, org_id: &str) -> Result<UsageState, FetchError>;
}

/// Map an HTTP status + body to an error, or `None` when the response is usable.
pub fn classify(status: u16, body: &str) -> Option<FetchError> {
    let challenge = body.contains("cf-chl") || body.contains("Just a moment");
    if challenge {
        return Some(FetchError::Blocked(status));
    }
    let auth_body = body.contains("permission_error") || body.contains("account_session_invalid");
    match status {
        200..=299 => None,
        401 => Some(FetchError::Auth),
        403 if auth_body => Some(FetchError::Auth),
        403 | 429 => Some(FetchError::Blocked(status)),
        _ => Some(FetchError::Network(format!("HTTP {status}"))),
    }
}

const BASE: &str = "https://claude.ai/api";
const USER_AGENT: &str = concat!("ClaudeUsageWidget/", env!("CARGO_PKG_VERSION"));

/// Plain HTTPS backend. One agent (connection pool) reused for the life of the source.
pub struct HttpSource {
    agent: ureq::Agent,
    cookie: String,
}

impl HttpSource {
    pub fn new(session: Session) -> HttpSource {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        HttpSource { agent, cookie: format!("sessionKey={}", session.session_key) }
    }

    fn get(&self, path: &str) -> Result<String, FetchError> {
        let mut resp = self
            .agent
            .get(&format!("{BASE}{path}"))
            .header("Cookie", &self.cookie)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        match classify(status, &body) {
            None => Ok(body),
            Some(e) => Err(e),
        }
    }
}

impl UsageSource for HttpSource {
    fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError> {
        parse_orgs(&self.get("/organizations")?).map_err(|e| FetchError::Format(e.0))
    }

    fn fetch_usage(&mut self, org_id: &str) -> Result<UsageState, FetchError> {
        parse_usage(&self.get(&format!("/organizations/{org_id}/usage"))?).map_err(|e| FetchError::Format(e.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHALLENGE: &str = "<!DOCTYPE html><title>Just a moment...</title><script src=\"/cdn-cgi/challenge-platform/cf-chl\">";

    #[test]
    fn success_is_none() {
        assert_eq!(classify(200, "{}"), None);
    }

    #[test]
    fn auth_failures() {
        assert_eq!(classify(401, ""), Some(FetchError::Auth));
        assert_eq!(
            classify(403, r#"{"type":"error","error":{"type":"permission_error","message":"Invalid authorization"}}"#),
            Some(FetchError::Auth)
        );
        assert_eq!(classify(403, r#"{"error":{"type":"account_session_invalid"}}"#), Some(FetchError::Auth));
    }

    #[test]
    fn blocked_and_rate_limited() {
        assert_eq!(classify(403, CHALLENGE), Some(FetchError::Blocked(403)));
        assert_eq!(classify(200, CHALLENGE), Some(FetchError::Blocked(200)));
        assert_eq!(classify(429, "{}"), Some(FetchError::Blocked(429)));
    }

    #[test]
    fn server_errors_are_network() {
        assert!(matches!(classify(500, ""), Some(FetchError::Network(_))));
        assert!(matches!(classify(503, "oops"), Some(FetchError::Network(_))));
    }
}
