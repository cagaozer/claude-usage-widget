//! macOS session storage: the claude.ai session lives in the user's login Keychain as a
//! generic password (service "ClaudeUsageWidget", account "claude.ai"), never on disk.

use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use std::io;
use usage_core::session::{Session, SessionStore};

const SERVICE: &str = "ClaudeUsageWidget";
const ACCOUNT: &str = "claude.ai";

/// JSON shape of the Keychain item's secret.
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    session_key: String,
    last_active_org: Option<String>,
}

pub struct KeychainStore {
    account: String,
}

impl KeychainStore {
    /// The store the app uses.
    pub fn new() -> KeychainStore {
        KeychainStore { account: ACCOUNT.to_string() }
    }

    /// A store under a different account name, so tests never touch the real item.
    #[cfg(test)]
    pub fn with_account(account: String) -> KeychainStore {
        KeychainStore { account }
    }
}

impl SessionStore for KeychainStore {
    fn load(&self) -> Option<Session> {
        let bytes = get_generic_password(SERVICE, &self.account).ok()?;
        let st: Stored = serde_json::from_slice(&bytes).ok()?;
        Some(Session { session_key: st.session_key, last_active_org: st.last_active_org })
    }

    fn save(&self, s: &Session) -> io::Result<()> {
        let json = serde_json::to_vec(&Stored {
            session_key: s.session_key.clone(),
            last_active_org: s.last_active_org.clone(),
        })?;
        // set_generic_password updates the item if it already exists.
        set_generic_password(SERVICE, &self.account, &json).map_err(io::Error::other)
    }

    fn clear(&self) {
        let _ = delete_generic_password(SERVICE, &self.account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_roundtrip() {
        let store = KeychainStore::with_account(format!("claude.ai-test-{}", std::process::id()));
        let s = Session { session_key: "sk-ant-sid01-TESTSECRET".into(), last_active_org: Some("org-1".into()) };
        store.save(&s).unwrap();
        let loaded = store.load();
        store.clear();
        assert_eq!(loaded, Some(s));
        assert_eq!(store.load(), None);
        store.clear(); // clearing a missing item is fine
    }

    #[test]
    fn debug_never_shows_key() {
        let s = Session { session_key: "sk-ant-sid01-TESTSECRET".into(), last_active_org: None };
        assert!(!format!("{s:?}").contains("TESTSECRET"));
    }
}
