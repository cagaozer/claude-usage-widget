// SPDX-License-Identifier: GPL-3.0-or-later
//! User settings persisted as `settings.json` in the platform's app data folder
//! (`%APPDATA%\ClaudeUsageWidget` on Windows, `~/Library/Application Support/ClaudeUsageWidget` on macOS).

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

pub const OPACITY_CHOICES: [u8; 4] = [100, 85, 70, 50];
pub const INTERVAL_CHOICES: [u32; 4] = [30, 60, 120, 300];
pub const MIN_INTERVAL_SECS: u32 = 30;
const FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub opacity_pct: u8,
    pub interval_secs: u32,
    pub pos: Option<(i32, i32)>,
    pub org_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { opacity_pct: 100, interval_secs: 60, pos: None, org_id: None }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(s.trim_start_matches('\u{FEFF}')).ok())
            .unwrap_or_default()
            .sanitized()
    }

    /// Write to a temp file then rename, so a crash never leaves a half-written file.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(FILE))
    }

    pub fn sanitized(mut self) -> Settings {
        self.interval_secs = self.interval_secs.max(MIN_INTERVAL_SECS);
        let want = self.opacity_pct as i32;
        self.opacity_pct = *OPACITY_CHOICES
            .iter()
            .min_by_key(|&&c| (c as i32 - want).abs())
            .unwrap();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings { opacity_pct: 85, interval_secs: 120, pos: Some((10, -20)), org_id: Some("abc".into()) };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
    }

    #[test]
    fn settings_missing_or_corrupt_gives_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        std::fs::write(dir.path().join("settings.json"), "{not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn sanitize_clamps() {
        let s = |opacity_pct, interval_secs| Settings { opacity_pct, interval_secs, ..Settings::default() }.sanitized();
        assert_eq!(s(100, 5).interval_secs, 30);
        assert_eq!(s(63, 60).opacity_pct, 70);
        assert_eq!(s(0, 60).opacity_pct, 50);
        assert_eq!(s(85, 300), Settings { opacity_pct: 85, interval_secs: 300, ..Settings::default() });
    }

    #[test]
    fn load_accepts_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), "\u{FEFF}{\"interval_secs\":120,\"opacity_pct\":85}").unwrap();
        let s = Settings::load(dir.path());
        assert_eq!((s.interval_secs, s.opacity_pct), (120, 85));
    }

    #[test]
    fn load_sanitizes_hand_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"interval_secs":1,"opacity_pct":7}"#).unwrap();
        let s = Settings::load(dir.path());
        assert_eq!((s.interval_secs, s.opacity_pct), (30, 50));
    }
}
