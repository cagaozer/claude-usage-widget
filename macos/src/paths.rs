// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the macOS build keeps its files.

use std::path::PathBuf;

/// `~/Library/Application Support/ClaudeUsageWidget`, created if missing. Holds
/// settings.json and the single-instance lock (the session is in the Keychain).
pub fn app_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = home.join("Library/Application Support/ClaudeUsageWidget");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
