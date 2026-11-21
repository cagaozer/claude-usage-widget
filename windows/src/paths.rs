//! Where the Windows build keeps its files.

use std::path::PathBuf;

/// `%APPDATA%\ClaudeUsageWidget`, created if missing.
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("ClaudeUsageWidget");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `%LOCALAPPDATA%\ClaudeUsageWidget\WebView2` (not created here).
pub fn webview_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("ClaudeUsageWidget").join("WebView2")
}
