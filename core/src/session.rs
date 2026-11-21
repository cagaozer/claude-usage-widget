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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_debug_redacts() {
        let s = Session { session_key: "sk-ant-sid01-TESTSECRET-abcdef".into(), last_active_org: Some("org-1".into()) };
        assert!(!format!("{s:?}").contains("TESTSECRET"));
    }
}
