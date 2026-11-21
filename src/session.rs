//! claude.ai web session, stored encrypted with Windows DPAPI (current user).

use std::io;
use std::path::Path;

const FILE: &str = "session.bin";

#[derive(Clone, PartialEq)]
pub struct Session {
    pub session_key: String,
    pub last_active_org: Option<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("session_key", &"<redacted>")
            .field("last_active_org", &self.last_active_org)
            .finish()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    session_key: String,
    last_active_org: Option<String>,
}

pub fn save_session(dir: &Path, s: &Session) -> io::Result<()> {
    let plain = serde_json::to_vec(&Stored {
        session_key: s.session_key.clone(),
        last_active_org: s.last_active_org.clone(),
    })?;
    let sealed = dpapi::protect(&plain)?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(FILE), sealed)
}

pub fn load_session(dir: &Path) -> Option<Session> {
    let sealed = std::fs::read(dir.join(FILE)).ok()?;
    let plain = dpapi::unprotect(&sealed).ok()?;
    let st: Stored = serde_json::from_slice(&plain).ok()?;
    Some(Session { session_key: st.session_key, last_active_org: st.last_active_org })
}

pub fn clear_session(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

mod dpapi {
    use std::io;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// Copy the DPAPI-allocated output into a Vec and free it with LocalFree.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        v
    }

    pub fn protect(data: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(io::Error::other)?;
            Ok(take(out))
        }
    }

    pub fn unprotect(data: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(io::Error::other)?;
            Ok(take(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session { session_key: "sk-ant-sid01-TESTSECRET-abcdef".into(), last_active_org: Some("org-1".into()) }
    }

    #[test]
    fn session_roundtrip_dpapi() {
        let dir = tempfile::tempdir().unwrap();
        save_session(dir.path(), &sample()).unwrap();
        assert_eq!(load_session(dir.path()), Some(sample()));
        let raw = std::fs::read(dir.path().join("session.bin")).unwrap();
        let needle = b"TESTSECRET";
        assert!(!raw.windows(needle.len()).any(|w| w == needle), "key stored in plaintext");
    }

    #[test]
    fn session_missing_or_tampered_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_session(dir.path()), None);
        std::fs::write(dir.path().join("session.bin"), b"garbage").unwrap();
        assert_eq!(load_session(dir.path()), None);
    }

    #[test]
    fn session_debug_redacts() {
        assert!(!format!("{:?}", sample()).contains("TESTSECRET"));
    }

    #[test]
    fn clear_session_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        save_session(dir.path(), &sample()).unwrap();
        clear_session(dir.path());
        assert!(!dir.path().join("session.bin").exists());
        clear_session(dir.path()); // idempotent
    }
}
