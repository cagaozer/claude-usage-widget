# Changelog

All notable changes to Claude Usage Widget are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-21

### Fixed

- **Windows: the widget could disappear behind other windows.** Some programs (seen
  with a maximized Warp terminal) can push an always-on-top window back among ordinary
  windows. The widget kept running but stayed hidden until it was restarted. It now
  checks once a second and puts itself back on top when that happens. When nothing is
  wrong it does nothing, so it never jumps above its own menu or other always-on-top
  windows, and it uses no measurable CPU.

## [0.1.0] - 2026-11-21

First public release.

### Added

- Always-on-top widget for **Windows 10/11** and **macOS 12+** (Apple Silicon and
  Intel) showing the Claude 5-hour session and weekly usage, per-model weekly rows when
  the plan has them, and the time until each limit resets.
- Bars colored green, amber (75 % and up) and red (90 % and up). The last numbers
  stay visible, greyed out, while offline.
- Automatic refresh every 60 s (30 s, 2 min or 5 min selectable); countdowns tick
  between refreshes.
- Adjustable opacity (100, 85, 70 or 50 %), drag to move, position remembered and
  kept on a visible screen.
- Sign-in on claude.ai's own login page (Google, email code, Apple, SSO). The session
  is stored with Windows DPAPI or in the macOS Keychain.
- Organization picker for accounts in more than one organization, sign out, a single
  running copy, and a clean quit.

[0.1.1]: https://github.com/cagaozer/claude-usage-widget/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/cagaozer/claude-usage-widget/releases/tag/v0.1.0
