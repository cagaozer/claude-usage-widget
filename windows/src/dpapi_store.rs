//! Windows session storage: the claude.ai session, encrypted with DPAPI for the current
//! Windows user and written to `%APPDATA%\ClaudeUsageWidget\session.bin`.

use std::io;
use std::path::PathBuf;
use usage_core::session::{Session, SessionStore};

const FILE: &str = "session.bin";

/// On-disk JSON shape (before encryption).
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    session_key: String,
    last_active_org: Option<String>,
}

/// `SessionStore` backed by a DPAPI-encrypted file in `dir`.
pub struct DpapiStore {
    pub dir: PathBuf,
}

impl SessionStore for DpapiStore {
    fn save(&self, s: &Session) -> io::Result<()> {
        let plain = serde_json::to_vec(&Stored {
            session_key: s.session_key.clone(),
            last_active_org: s.last_active_org.clone(),
        })?;
        let sealed = dpapi::protect(&plain)?;
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join(FILE), sealed)
    }

    fn load(&self) -> Option<Session> {
        let sealed = std::fs::read(self.dir.join(FILE)).ok()?;
        let plain = dpapi::unprotect(&sealed).ok()?;
        let st: Stored = serde_json::from_slice(&plain).ok()?;
        Some(Session { session_key: st.session_key, last_active_org: st.last_active_org })
    }

    fn clear(&self) {
        let _ = std::fs::remove_file(self.dir.join(FILE));
    }
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

    fn store(dir: &tempfile::TempDir) -> DpapiStore {
        DpapiStore { dir: dir.path().to_path_buf() }
    }

    #[test]
    fn session_roundtrip_dpapi() {
        let dir = tempfile::tempdir().unwrap();
        store(&dir).save(&sample()).unwrap();
        assert_eq!(store(&dir).load(), Some(sample()));
        let raw = std::fs::read(dir.path().join("session.bin")).unwrap();
        let needle = b"TESTSECRET";
        assert!(!raw.windows(needle.len()).any(|w| w == needle), "key stored in plaintext");
    }

    #[test]
    fn session_missing_or_tampered_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(store(&dir).load(), None);
        std::fs::write(dir.path().join("session.bin"), b"garbage").unwrap();
        assert_eq!(store(&dir).load(), None);
    }

    #[test]
    fn clear_session_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        store(&dir).save(&sample()).unwrap();
        store(&dir).clear();
        assert!(!dir.path().join("session.bin").exists());
        store(&dir).clear(); // idempotent
    }
}
