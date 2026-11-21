# Claude Usage Widget

**See your Claude plan limits at a glance, without typing `/usage`.**

A tiny always-on-top desktop widget for **Windows** and **macOS** that shows how much of
your Claude **5-hour session** limit and **weekly** limit you've used, and how long until
each one resets. It sits in a corner of your screen, refreshes itself every minute, and
stays out of your way.

| Windows | macOS |
|---|---|
| ![Widget on Windows](docs/screenshot.png) | ![Widget on macOS](docs/screenshot-mac.png) |

## Why this exists

If you use [Claude Code](https://claude.com/claude-code) (or Claude in general) on a Pro
or Max plan, your usage is limited in two ways: a rolling **5-hour session** window and a
**weekly** cap. Hit either one and you're stopped until it resets.

The only way to see where you stand is to run `/usage` in Claude Code or open
claude.ai → Settings → Usage. In the middle of a long coding session that means
interrupting yourself again and again just to check whether you're about to run out.

This widget answers that question permanently and passively: the numbers are always on
screen, so you can pace your work, decide when to start a big task, or know exactly when
your limit comes back, without breaking your flow.

## What it shows

```
Session  ██████░░░░  68%  1h 51m      ✕
Weekly   ███░░░░░░░  31%  4d 5h
```

- **Session**: how much of your current 5-hour usage window you've used.
- **Weekly**: how much of your weekly limit you've used.
- Per-model weekly rows (**Opus wk**, **Sonnet wk**) appear automatically if your plan
  has separate model limits.
- **Bar and percentage**: green below 75 %, amber from 75 %, red from 90 %. Percentages
  can go over 100 % if claude.ai reports it.
- **Time until reset** next to each row (for example `1h 51m`, `4d 5h`, `resetting…`).
- Status messages when there are no numbers to show: *Loading…*, *Signing in…*,
  *Signed out — click to sign in*, *No usage limits on this plan*, *Usage format
  changed — check for an update*. If the network drops, the last numbers stay visible,
  greyed out, with *Offline — retrying*.

## Features

- **Always on top**, small (252 × 56 points), draggable; it remembers where you put it
  and comes back on a visible screen if a monitor is unplugged.
- **Refreshes automatically** every 60 s by default (30 s, 60 s, 2 min or 5 min); the
  countdowns tick between refreshes.
- **Adjustable opacity**: 100, 85, 70 or 50 %.
- **Multiple organizations**: if your account belongs to more than one, pick which one to
  show.
- **Sign in with any method claude.ai offers**: Google, email code, Apple, SSO. You
  sign in on claude.ai's real login page; the widget never sees your password.
- **Lightweight and self-contained**: no installer, no runtime to install, no background
  service. Windows: a single ~1.6 MB `.exe` using about 3–5 MB of memory. macOS: a small
  `.app` with no Dock icon.
- **Clean exit**: quitting stops everything and releases all resources. Only one copy
  runs at a time.

> ## ⚠️ Unofficial — read before using
>
> This is **not** an Anthropic product. There is no official API for plan usage, so the
> widget reads the same internal endpoint that claude.ai's own **Settings → Usage** page
> uses, with your own claude.ai web session.
>
> - It may **stop working at any time** if Anthropic changes that endpoint.
> - Anthropic's consumer terms restrict accessing the service "through automated or
>   non-human means". A read-only check of your own usage once a minute is low-impact,
>   but it is arguably in that grey area. **Use it at your own risk**; in principle your
>   account could be warned or suspended.
> - The widget never uses Claude Code's credentials, never bypasses bot protection, and
>   identifies itself honestly (`User-Agent: ClaudeUsageWidget/<version>`).

## Download and run

Downloads are on the [Releases](../../releases) page.

### Windows (10 or 11, 64-bit)

1. Download `ClaudeUsageWidget.exe`.
2. Put it anywhere (Desktop, a tools folder…) and double-click it.
3. **Windows SmartScreen** may say "Windows protected your PC" because the exe isn't
   code-signed. Click **More info → Run anyway**.
4. A **Sign in to Claude** window opens on first run. Sign in as you normally do. When
   it's done the window closes by itself and the widget shows your numbers.

The sign-in window uses the Microsoft Edge **WebView2 Runtime**, which ships with
Windows 11 and current Windows 10. If it's missing, the widget tells you where to get it.

To start it automatically, put a shortcut to the exe in your Startup folder (press
<kbd>Win</kbd>+<kbd>R</kbd>, type `shell:startup`).

### macOS (12 Monterey or later, Apple Silicon or Intel)

1. Download `ClaudeUsageWidget-mac.zip` and unzip it.
2. Drag **ClaudeUsageWidget.app** into your **Applications** folder.
3. The app isn't notarized by Apple (that needs a paid developer account), so the first
   launch is blocked:
   - **macOS 15 or later:** open it once, then go to **System Settings → Privacy &
     Security**, scroll down, and click **Open Anyway** next to ClaudeUsageWidget.
   - **macOS 12–14:** right-click the app, choose **Open**, then **Open** again.
4. A **Sign in to Claude** window opens on first run. Sign in as you normally do.
5. **After each update**, macOS asks once whether the new version may use the saved
   sign-in in your Keychain. Click **Always Allow**. (If you deny it, the widget simply
   asks you to sign in again.)

The widget has no Dock icon and no menu bar; it's just the small floating panel, shown
on every desktop (Space). To start it automatically, add it in **System Settings →
General → Login Items**.

### Signing in

| Method | Windows | macOS |
|---|---|---|
| Google | Tested. Google opens in its own small pop-up; sign in there. | Not yet tested on a real Mac. |
| Email code | Should work (standard claude.ai page); not yet tested. | Not yet tested on a real Mac. |
| Apple, company SSO | Untested. | Untested. |

The macOS sign-in window is checked automatically on every build (it loads claude.ai's
login page), but nobody has signed in with it on a real Mac yet. Reports are welcome.

If Google refuses to sign in inside the window, choose **Continue with email** and use
your Google address instead.

## Using it

- **Drag** it anywhere.
- **Right-click** for the menu: Refresh now · Refresh every ▸ · Opacity ▸ ·
  Organization ▸ (only if you belong to more than one) · Sign out · Quit.
- **✕** quits.
- When signed out, click the widget to sign in.

## Privacy: what it stores and where

The widget talks only to `claude.ai`. It has no telemetry, no analytics and no servers
of its own.

**Windows**

| What | Where |
|---|---|
| Settings (opacity, interval, position, organization) | `%APPDATA%\ClaudeUsageWidget\settings.json` |
| Your claude.ai session, encrypted with Windows DPAPI (only your Windows account can read it) | `%APPDATA%\ClaudeUsageWidget\session.bin` |
| Temporary sign-in browser profile (deleted right after sign-in) | `%LOCALAPPDATA%\ClaudeUsageWidget\WebView2` |

To remove everything: quit the widget, delete the exe, and delete the two
`ClaudeUsageWidget` folders above.

**macOS**

| What | Where |
|---|---|
| Settings | `~/Library/Application Support/ClaudeUsageWidget/settings.json` |
| Your claude.ai session | Your login Keychain, item **ClaudeUsageWidget** (account `claude.ai`) |
| Sign-in browser data | Nowhere: the sign-in window keeps it in memory only |

To remove everything: quit the widget, delete the app, delete the
`~/Library/Application Support/ClaudeUsageWidget` folder, and delete the
**ClaudeUsageWidget** item in the Keychain Access app.

On both systems, **Sign out** in the menu deletes the widget's saved session. It doesn't
sign you out of claude.ai in your browser or elsewhere.

## How it works

1. You sign in on claude.ai's own login page, shown in an embedded browser window
   (WebView2 on Windows, WebKit on macOS).
2. The widget picks up the resulting claude.ai session cookie and stores it securely
   (DPAPI on Windows, Keychain on macOS).
3. A background thread asks claude.ai for your organizations and their usage on a timer
   and the widget redraws.

The code is a Rust workspace: a platform-neutral core shared by both apps (parsing,
fetching, state, menu and layout, all unit-tested), plus a thin native front end per
platform (Win32/GDI and AppKit). See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the
details.

## Build from source

You need [Rust](https://rustup.rs) (stable).

**Windows** (also needs the Visual Studio Build Tools with the C++ workload):

```powershell
cargo test --workspace
cargo build --release -p claude_usage_widget_win
# -> target\release\ClaudeUsageWidget.exe
```

**macOS** (also needs the Xcode command-line tools):

```sh
cargo test -p usage_core -p claude_usage_widget_mac
cargo build --release -p claude_usage_widget_mac
sh macos/scripts/bundle.sh target/release/claude-usage-widget-mac dist 0.1.0
# -> dist/ClaudeUsageWidget.app
```

Debug builds accept `--demo` to show fake data without signing in:
`cargo run -p claude_usage_widget_win -- --demo` (Windows) or
`cargo run -p claude_usage_widget_mac -- --demo` (macOS).

`scripts\leakcheck.ps1` (Windows) and `macos/scripts/leakcheck.sh` (macOS) sample the
running widget's memory to check for leaks. GitHub Actions builds and tests both apps on
every change; pushing a `v*` tag publishes a release.

## Contributing

Issues and pull requests are welcome, especially reports of signing in on macOS and with
Apple or SSO accounts. Please keep the widget's rules: no use of Claude Code's
credentials, no bot-protection workarounds, an honest user agent, read-only access, and
never logging the session key.

## License

Copyright (C) 2026 cagaozer

This program is free software: you can redistribute it and/or modify it under the terms
of the **GNU General Public License version 3** as published by the Free Software
Foundation, or (at your option) any later version. It is distributed in the hope that it
will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See [LICENSE](LICENSE) for the full
text.

"Claude" is a trademark of Anthropic. This project is not affiliated with or endorsed by
Anthropic.
