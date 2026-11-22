# Architecture

This document explains how Claude Usage Widget is put together, for anyone who wants to
read, change or port the code. For installing and using the widget, see the
[README](../README.md).

## The idea in one paragraph

claude.ai has an internal endpoint that its own **Settings → Usage** page uses. Given
the user's claude.ai web session (the `sessionKey` cookie), it returns the 5-hour and
weekly limits as percentages with reset times. The widget gets that cookie once, by
letting the user sign in on the real claude.ai login page inside an embedded browser
window, stores it securely, and then asks the endpoint for fresh numbers on a timer.
Everything except the window, the drawing, the embedded browser and the secure storage
is plain Rust shared by both platforms.

## Repository layout

```
Cargo.toml          Cargo workspace (members: core, windows, macos) + release profile
core/               crate usage_core: platform-neutral logic, no OS APIs
windows/            crate claude_usage_widget_win → ClaudeUsageWidget.exe
macos/              crate claude_usage_widget_mac → ClaudeUsageWidget.app
tests/fixtures/     sample API responses used by core tests
scripts/            Windows leak-check script
.github/workflows/  CI: build + test both apps, publish releases on v* tags
docs/               screenshot, this file, design specs and plans
```

Each platform crate compiles to an empty `main` on the other OS, so
`cargo build --workspace` and `cargo test --workspace` work anywhere.

## core (usage_core)

| Module | Responsibility |
|---|---|
| `usage` | Parse `/api/organizations` and `/api/organizations/{id}/usage` JSON into `Org`, `Limits`, `Window` (percent + reset time). Missing windows mean "no limits on this plan". |
| `source` | `UsageSource` trait and `HttpSource` (ureq + rustls). Maps HTTP status/body to `FetchError::{Auth, Blocked, Network, Format}`. Sends an honest `User-Agent: ClaudeUsageWidget/<version>`. |
| `fetcher` | Background thread that polls a `UsageSource` on an interval (floor 30 s) and accepts commands (refresh now, change interval, switch org, stop). Picks the org to show. The controller stops it with `detach`, which never waits for a request in flight, so sign-out and quit are instant even on a stalled network. |
| `controller` | The app's state machine (see below). |
| `view` | Turns state into a `ViewModel`: rows of label / bar fill / percent / countdown, or a status message. |
| `format` | Countdown text ("1h 52m", "4d 6h", "resetting…") and color level (green < 75 %, amber 75–90 %, red ≥ 90 %). |
| `layout` | Widget size, column positions and colors shared by both renderers. |
| `menu` | The right-click menu as data (`MenuEntry`), and decoding a clicked id back into an `Action`. |
| `settings` | `settings.json` (opacity, interval, position, chosen org), sanitized on load, written atomically. |
| `session` | `Session` (redacted `Debug`), the `SessionStore` trait, and `session_from_cookies`, which accepts only cookies for `claude.ai` or its subdomains. |
| `geometry` | Keeps the widget on a visible screen (`Rect` in top-left, y-down coordinates). |
| `instance` | Single-instance lock via an OS file lock (used on macOS). |
| `demo` | Fake data for `--demo` (debug builds only). |

### Controller and data flow

```
            user actions                         fetch thread
 platform ───────────────► Controller ◄──── Sink(gen, Event) ◄──── Fetcher ──► claude.ai
 (window,   ◄─────────────     │
  menu)       Vec<Effect>      └── SessionStore (DPAPI / Keychain), settings.json
```

- The platform forwards clicks, menu choices and fetch events to the `Controller` and
  performs the `Effect`s it returns: `Relayout` (resize to fit, repaint), `Opacity`, and
  `SignInCleanup`. The controller itself never calls an OS API, which is why all of its
  behavior is unit-tested in `core`.
- Fetch events cross threads through a `Sink` closure supplied by the platform (Windows:
  `PostMessage` to the widget window; macOS: `DispatchQueue::main().exec_async`).
- **Generation tagging.** Every time the fetcher is replaced or stopped (sign-out, sign-in,
  quit), the controller bumps a generation counter. Each event carries the generation of
  the fetcher that produced it, and `handle_posted` drops events from older generations.
  This stops a fetch that was in flight during sign-out from flipping the widget back to
  showing numbers.
- Network errors keep the last numbers on screen, greyed, with "Offline — retrying". An
  auth error signs out. A response that no longer parses shows "Usage format changed".

## windows

- `app.rs`: start-up. A named mutex keeps one instance, COM is initialized for WebView2,
  settings and the DPAPI-stored session are loaded.
- `widget.rs`: a layered, topmost, tool-window popup that never takes focus. Painted with
  GDI into an off-screen bitmap (no flicker), scaled for per-monitor DPI. Dragging works
  by answering `WM_NCHITTEST` with "caption" everywhere except the close button.
  Functions that open nested message loops (the context menu, the sign-in window) take
  the `HWND`, not `&mut App`, so no reference is held while `wndproc` is re-entered.
- `topmost.rs`: keeps the widget in the always-on-top layer. `WS_EX_TOPMOST` at creation
  is not enough, because other programs can push a topmost window back among ordinary
  windows. A 1-second timer in `widget.rs` calls `ensure_topmost`, which re-asserts
  `HWND_TOPMOST` only if an ordinary window sits above the widget (or the flag is gone).
  The healthy case is a no-op, so the widget never jumps over its own menu.
- `signin.rs`: a WebView2 window on `https://claude.ai/login` with a fresh profile. Pop-ups
  are left to WebView2's default handling so Google's `window.opener` hand-back works.
  The cookie manager is polled once a second for `sessionKey`; the profile folder is
  deleted afterwards.
- `dpapi_store.rs`: the session encrypted with DPAPI (current user) in `session.bin`.

## macos

- `main.rs`: single-instance file lock, then starts the app with the Keychain session or
  sign-in.
- `app.rs`: owns the controller, the panel and the countdown timer in a main-thread
  `thread_local`. Handlers borrow it briefly and never across nested event loops (context
  menu, window drag). Timers are scheduled in the run loop's common modes so they keep
  firing while a menu is open. Converts between AppKit's bottom-left coordinates and the
  shared top-left ones. The position is saved whenever the panel moves (`windowDidMove:`),
  so it survives logout or shutdown, not only Quit.
- `panel.rs`: a borderless, non-activating `NSPanel` at floating level that joins all
  Spaces, with a custom flipped `NSView` that forwards drawing and mouse events.
- `draw.rs`: paints the `ViewModel` with AppKit (rounded background, bars, SF text) using
  the shared layout numbers.
- `menu.rs`: builds an `NSMenu` from the shared menu model.
- `signin.rs`: a normal (not modal) window with a `WKWebView` on claude.ai's login page,
  using a non-persistent data store (nothing written to disk) and WebKit's default user
  agent. Pop-ups are hosted in a real second window created with WebKit's configuration,
  at most one at a time. Cookies are polled once a second; when a claude.ai `sessionKey`
  appears, the windows are torn down and the session is handed back. The window is not
  modal on purpose: under a modal session WebKit's messages from its helper processes
  were never delivered and the page stayed blank. Because the app has no Dock icon, a
  click on the widget (or "Sign in" in its menu) while signing in brings the sign-in
  window back to the front.
- `keychain.rs`: the session as a generic password (service `ClaudeUsageWidget`, account
  `claude.ai`) in the login Keychain.
- `scripts/bundle.sh` assembles and ad-hoc signs the `.app`; `smoke.sh` and `leakcheck.sh`
  are the automated checks CI runs.

## Security and privacy rules

- Never reads or reuses Claude Code's credentials.
- Never bypasses bot protection: plain HTTPS with an honest user agent; a challenge page
  counts as "blocked, retry later". The sign-in browsers keep their default identity.
- The session key is stored only in DPAPI-encrypted form (Windows) or the Keychain (macOS),
  is redacted from `Debug` output, and is never logged. Debug-build sign-in logs show page
  addresses without query strings, never cookies.
- Read-only: the widget only reads usage; it never changes anything in the account.
- Refresh interval floor of 30 s.

## Testing

- `cargo test --workspace` runs the core tests (parsing, formatting, view, settings,
  fetcher, controller, menu, layout, geometry, cookies, instance lock) plus platform tests
  (DPAPI round-trip and text-fits-columns with GDI on Windows; Keychain round-trip and
  text-fits-columns with AppKit on macOS).
- CI on macOS also launches the debug app with demo data, takes screenshots of the widget
  and of the sign-in window, checks that a second copy exits, that quitting finishes
  within 5 s (also with the sign-in window open), and, for releases, that memory doesn't
  grow over 10 minutes.

## CI and releases

`.github/workflows/build.yml`:

- **windows** (every push): test, build `ClaudeUsageWidget.exe`, upload it.
- **macos** (when `core/`, `macos/`, Cargo files or the workflow change, on `v*` tags, or
  by hand): test, smoke check, build, bundle, sign, zip. On tags and manual runs also: a
  universal (Apple Silicon + Intel) binary and the leak check.
- **release** (on `v*` tags): publishes `ClaudeUsageWidget.exe` and
  `ClaudeUsageWidget-mac.zip` as a GitHub Release.
