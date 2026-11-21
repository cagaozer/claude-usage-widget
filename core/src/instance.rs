//! Single-instance guard based on an OS file lock (used by the macOS app; Windows uses a
//! named mutex). The OS releases the lock when the process exits, even after a crash, so
//! a stale lock file never blocks the next launch.

use std::fs::{File, OpenOptions};
use std::path::Path;

/// Take the app-wide lock on `dir/.lock`. Returns the open file, which holds the lock
/// until it is dropped, or `None` if another process already holds it.
pub fn lock_single_instance(dir: &Path) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_while_first_held() {
        let dir = tempfile::tempdir().unwrap();
        let first = lock_single_instance(dir.path());
        assert!(first.is_some());
        assert!(lock_single_instance(dir.path()).is_none());
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        drop(lock_single_instance(dir.path()).unwrap());
        assert!(lock_single_instance(dir.path()).is_some());
    }
}
