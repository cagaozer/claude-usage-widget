// SPDX-License-Identifier: GPL-3.0-or-later
//! The claude.ai web session (the `sessionKey` cookie) and the interface each platform
//! implements to keep it between runs (DPAPI on Windows, Keychain on macOS).

use std::io;

#[derive(Clone, PartialEq)]
pub struct Session {
    pub session_key: String,
    pub last_active_org: Option<String>,
}

/// Never print the session key: it grants access to the user's claude.ai account.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("session_key", &"<redacted>")
            .field("last_active_org", &self.last_active_org)
            .finish()
    }
}

/// Platform storage for the session. Implementations must keep the key encrypted or in
/// the OS credential store, never in plain text.
pub trait SessionStore {
    fn load(&self) -> Option<Session>;
    fn save(&self, s: &Session) -> io::Result<()>;
    /// Forget the stored session. Missing is not an error.
    fn clear(&self);
}

/// Pick the claude.ai session out of a browser cookie jar, given `(name, domain, value)`
/// triples. Only cookies for `claude.ai` or its subdomains count: a sign-in pop-up (e.g.
/// Google) can set cookies on other domains, and those must never be taken for ours.
pub fn session_from_cookies<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Option<Session> {
    let is_claude = |domain: &str| {
        let d = domain.strip_prefix('.').unwrap_or(domain);
        d == "claude.ai" || d.ends_with(".claude.ai")
    };
    let (mut key, mut org) = (None, None);
    for (name, domain, value) in cookies {
        if value.is_empty() || !is_claude(domain) {
            continue;
        }
        match name {
            "sessionKey" => key = Some(value.to_string()),
            "lastActiveOrg" => org = Some(value.to_string()),
            _ => {}
        }
    }
    Some(Session { session_key: key?, last_active_org: org })
}

/// One-line tip shown above the sign-in page on both platforms.
pub const SIGN_IN_HINT: &str = "Google sign-in blocked? Use 'Continue with email' with your Google address.";

/// In-memory store for tests. Clones share the same slot, so a test can keep a handle
/// and inspect what the controller saved.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct MemoryStore(std::rc::Rc<std::cell::RefCell<Option<Session>>>);

#[cfg(test)]
impl SessionStore for MemoryStore {
    fn load(&self) -> Option<Session> {
        self.0.borrow().clone()
    }
    fn save(&self, s: &Session) -> io::Result<()> {
        *self.0.borrow_mut() = Some(s.clone());
        Ok(())
    }
    fn clear(&self) {
        *self.0.borrow_mut() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_claude_ai_session_cookie() {
        let s = session_from_cookies([
            ("other", "claude.ai", "x"),
            ("sessionKey", ".claude.ai", "k"),
            ("lastActiveOrg", "claude.ai", "o"),
        ]);
        assert_eq!(s, Some(Session { session_key: "k".into(), last_active_org: Some("o".into()) }));
        let no_org = session_from_cookies([("sessionKey", "claude.ai", "k")]);
        assert_eq!(no_org, Some(Session { session_key: "k".into(), last_active_org: None }));
    }

    #[test]
    fn ignores_cookies_from_other_domains() {
        assert_eq!(session_from_cookies([("sessionKey", "accounts.google.com", "x")]), None);
        assert_eq!(session_from_cookies([("sessionKey", "evilclaude.ai", "x")]), None);
        assert_eq!(session_from_cookies([("sessionKey", "claude.ai.example.com", "x")]), None);
        // A subdomain of claude.ai is still claude.ai.
        assert!(session_from_cookies([("sessionKey", "www.claude.ai", "k")]).is_some());
    }

    #[test]
    fn empty_session_key_is_none() {
        assert_eq!(session_from_cookies([("sessionKey", "claude.ai", ""), ("lastActiveOrg", "claude.ai", "o")]), None);
    }

    #[test]
    fn session_debug_redacts() {
        let s = Session { session_key: "sk-ant-sid01-TESTSECRET-abcdef".into(), last_active_org: Some("org-1".into()) };
        assert!(!format!("{s:?}").contains("TESTSECRET"));
    }
}
