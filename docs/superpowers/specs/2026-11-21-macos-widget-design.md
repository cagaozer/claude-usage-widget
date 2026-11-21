# Claude Usage Widget for macOS — Design

Date: 2026-11-21
Status: Approved 2026-11-21
Builds on: `2026-11-20-claude-usage-widget-design.md` (Windows version, shipped)

## Intent and why

The Windows widget is done and in a private GitHub repo. The user wants the same tool for
Claude Code users on Apple desktop computers (MacBook etc.): a tiny always-on-top floating
widget showing 5-hour session and weekly usage with time until reset, so they don't have
to keep typing `/usage`. The user has no Mac; GitHub's macOS machines build and check it,
and a one-off hands-on test happens later on a rented cloud Mac over remote desktop.

## Goals

- Same behavior as the Windows widget: floating, always on top, draggable, position
  remembered; Session + Weekly rows with countdown; refresh 30 s / 60 s / 2 min / 5 min
  (default 60 s, floor 30 s); opacity 100 / 85 / 70 / 50 %; right-click menu (Refresh now,
  Refresh every ▸, Opacity ▸, Organization ▸ when >1, Sign out / Sign in, Quit); ✕ quits.
- Same states and exact copy as Windows ("Signing in…", "Loading…", "Offline — retrying",
  "Signed out — click to sign in", "Usage format changed — check for an update",
  "No usage limits on this plan"); same color thresholds (green < 75, amber 75–90, red ≥ 90).
- Sign in on the real claude.ai page with any method it offers (Google, email code, Apple,
  SSO), inside an embedded Safari-engine (WKWebView) window.
- Lightweight: app ≤ 10 MB zipped, steady-state memory ~10–25 MB, no leaks, full release on quit.
- Runs on Apple Silicon and Intel, macOS 12 (Monterey) or later.
- One repo for both platforms, sharing all platform-neutral logic and its tests.
- GitHub Actions builds and tests both platforms (lean on Mac minutes, see CI below), and
  attaches `ClaudeUsageWidget.exe` and `ClaudeUsageWidget-mac.zip` to tagged releases.

## Non-goals

- Menu bar item mode (user chose the floating widget).
- iOS/iPadOS; Mac App Store distribution; auto-update.
- Apple Developer ID signing / notarization (no paid account). Ad-hoc signed only.

## Anti-goals

- Anything the Windows design forbids still applies: no Claude Code credentials, no
  Cloudflare/bot-protection circumvention, honest `User-Agent: ClaudeUsageWidget/<ver>`,
  never log the session key, read-only, ≥ 30 s interval. README terms-risk warning applies.
- Breaking or changing the Windows widget's behavior while restructuring.
- A Dock icon or a window that steals focus on refresh.
- Running macOS in an unlicensed VM on the user's Windows PC (rejected; violates Apple's license).

## Constraints

- **Repo stays private** (user decision). GitHub bills private-repo macOS runner minutes at
  10× Linux/Windows against the free monthly allowance, so CI must be lean (see phase 2).
  Only people the user invites can download builds.
- **No browser impersonation on Mac** (user decision). The sign-in WKWebView keeps its
  default user agent; it never claims to be Safari. If Google refuses sign-in inside the
  embedded window, the supported fallback is claude.ai's email-code sign-in, and the README
  says so.

- No Mac available to the developer. All Mac compilation, tests, and smoke checks run on
  GitHub-hosted `macos-14` (Apple Silicon) runners; the user does one manual test later on
  a rented cloud Mac. Nothing can be claimed about real sign-in on Mac until then.
- Unsigned/ad-hoc apps downloaded from the internet are quarantined by Gatekeeper; on
  macOS 15+ the user must approve via System Settings → Privacy & Security → Open Anyway.
- WKWebView does not open pop-up windows by itself; Google/SSO sign-in pop-ups must be
  hosted explicitly via `WKUIDelegate` (lesson from the Windows white-page bug).

## Approaches considered

1. **Rust + Apple frameworks via `objc2` crates (CHOSEN).** Shares `core/` directly; tiny
   binary; one language. Trade-off: AppKit code is wordier in Rust than in Swift.
2. **Swift UI + Rust core over FFI.** Cleaner Mac UI code, but two toolchains and an FFI
   bridge to maintain on CI, with no user-visible benefit. Rejected.
3. **Cross-platform rewrite (Tauri or similar).** Bundles a browser engine for the UI,
   ~30–60 MB memory everywhere, against the lightweight goal. Rejected.

Repo: **same repo** (user agreed), restructured into a Cargo workspace.

## Design

### Repository layout (Cargo workspace)

```
Cargo.toml            [workspace] members = ["core", "windows", "macos"]
core/                 crate `usage_core` — platform-neutral, no OS APIs
  src/usage.rs format.rs view.rs fetcher.rs source.rs settings.rs geometry.rs session.rs(model only)
windows/              crate `claude_usage_widget_win` → bin ClaudeUsageWidget.exe (today's code)
  src/main.rs widget.rs gdi.rs signin.rs dpapi.rs  + build.rs, app.manifest, resources.rc, assets/
macos/                crate `claude_usage_widget_mac` → bin ClaudeUsageWidget, bundled as .app
  src/main.rs widget.rs signin.rs keychain.rs  + Info.plist, assets/AppIcon.icns, bundle script
tests/fixtures/       shared JSON fixtures (used by core tests)
.github/workflows/build.yml
```

- Windows-only crates (`windows`, `webview2-com`, `embed-resource`) become target-specific
  dependencies of `windows/`; Apple crates of `macos/`. Each platform crate compiles to an
  empty `main` on the other OS so `cargo build --workspace` works everywhere.
- `core` must not depend on any OS API. Today's `geometry.rs` uses Win32 `RECT`; it moves
  to core with its own `Rect { left, top, right, bottom }` and Windows converts.
- `settings.rs` stays in core but takes the data directory as a parameter; each platform
  supplies its own (`%APPDATA%\ClaudeUsageWidget` / `~/Library/Application Support/ClaudeUsageWidget`).
- Session persistence becomes a small trait in core (`SessionStore { load, save, clear }`);
  Windows implements it with DPAPI (today's code), macOS with Keychain.
- The fetcher posts events through a platform-provided sink (already a closure today);
  generation tagging (from the Windows review fix) moves into the shared event type.

### Phase order

1. **Restructure only** — move code into the workspace, no behavior change. Windows tests
   (41) must still pass and the Windows exe must look and behave identically (demo
   screenshot compared). Commit before any Mac code exists.
2. **CI (lean)** — GitHub Actions:
   - Windows job on every push: `cargo test --workspace`, release build, upload exe.
   - macOS job only when `core/`, `macos/`, `Cargo.lock` or the workflow changes (path
     filter), arm64 only: `cargo test -p usage_core -p claude_usage_widget_mac`, debug +
     release build, smoke check (phase 4). Target ~3–5 Mac minutes per run; cargo cache on.
   - On tag `v*` or manual dispatch only: universal build (`aarch64` + `x86_64`, `lipo`),
     assemble `.app`, ad-hoc `codesign --sign -`, zip, 10-minute leak check, GitHub Release
     with both artifacts.
3. **macOS app** — widget window, then sign-in, then Keychain, wired to core.
4. **Automated Mac smoke check in CI** — `--demo` stays debug-build-only (as on Windows), so
   CI assembles a separate debug `.app` for this check; the shipped release has no demo
   mode. Launch the debug `.app` with `--demo` on the runner,
   wait, `screencapture` the screen, upload the PNG as a build artifact, then quit it via
   the app's own quit path and assert the process exits within 5 s.
5. **Manual test** on a rented cloud Mac by the user (sign-in with Google, live numbers,
   menu, drag, quit, restart without re-sign-in).

### macOS widget window

- `NSPanel` with style `nonactivatingPanel | borderless`, `level = .floating`,
  `collectionBehavior = canJoinAllSpaces | fullScreenAuxiliary | stationary`,
  `hidesOnDeactivate = false`, `becomesKeyOnlyIfNeeded = true`. Rounded corners via the
  content view's layer (`cornerRadius` 6, `masksToBounds`).
- App is an accessory: `Info.plist` `LSUIElement = true` → no Dock icon, no menu bar menu.
- Size 252 × (16 + 20 × rows [+16 note]) points — same logical layout and column constants
  as Windows (move the `COL_*` constants into core so both share them). macOS handles
  Retina scaling automatically (points, not pixels).
- Drawing: a custom `NSView` subclass (objc2 `define_class!`) whose `drawRect:` paints the
  shared `ViewModel` with Core Graphics / `NSString` drawing, system font 12 pt (SF), same
  colors as Windows. `isFlipped = true` so coordinates match the Windows layout.
- Drag: `isMovableByWindowBackground = true`, except over ✕ and when Signed out (click =
  sign in). Position saved on `windowDidMove`. Clamp to visible screens on start using the
  shared `clamp_to_work_areas` with `NSScreen.visibleFrame` values.
- Opacity: `panel.alphaValue`.
- Right-click: `menuForEvent:` returns an `NSMenu` built from the same snapshot logic as
  Windows (`menu_action`), so behavior is identical.
- Countdown repaint every 60 s via `NSTimer`; fetch events arrive on the fetch thread and
  are forwarded to the main thread with `dispatch_async` to the main queue (GCD), carrying
  the generation tag; the main thread ignores stale generations.
- Quit: stop timer → drop fetcher (joins) → save settings → `NSApp.terminate`.
- Single instance: an advisory `flock` on `~/Library/Application Support/ClaudeUsageWidget/.lock`;
  if already held, the new process exits silently.

### macOS sign-in

- Separate titled `NSWindow` (480 × 720 pt, centered) with the same hint line as Windows
  above a `WKWebView` using a **non-persistent** `WKWebsiteDataStore` (nothing written to
  disk; equivalent to deleting the WebView2 profile on Windows).
- Default WKWebView user agent, never overridden (see Constraints).
- Navigate to `https://claude.ai/login`; every 1 s read `WKHTTPCookieStore.getAllCookies`
  and finish when `sessionKey` (domain claude.ai) appears; also capture `lastActiveOrg`.
- `WKUIDelegate.webView(_:createWebViewWith:for:windowFeatures:)` creates a pop-up
  `WKWebView` **with the supplied configuration** in its own small window and returns it,
  so Google's `window.opener` hand-back works; `webViewDidClose` closes that window. Only
  one pop-up at a time: a new request closes any existing pop-up first (also fixes the
  Windows "two Google windows" annoyance on Mac).
- Run modally (`NSApp.runModalForWindow`) while the widget ignores clicks; closing the
  window = cancelled → "Signed out — click to sign in".
- Teardown: stop timer, close pop-up, `webView.stopLoading`, remove from superview, release
  web view and window.

### macOS session storage

- Keychain generic password via the `security-framework` crate: service
  `ClaudeUsageWidget`, account `claude.ai`, value = JSON `{session_key, last_active_org}`.
  Sign out / auth failure deletes the item. Settings JSON in Application Support.
- Because builds are ad-hoc signed, each new version is a "different app" to the Keychain:
  after updating, macOS asks once to allow access to the saved sign-in. Accepted (builder
  call); the README tells users to click "Always Allow".

### HTTP

- Shared `HttpSource` (ureq + rustls) works unchanged on macOS. Same honest User-Agent.

### Packaging

- `macos/Info.plist`: `CFBundleIdentifier` `io.github.claudeusagewidget`, `LSUIElement` true,
  `LSMinimumSystemVersion` 12.0, `NSHighResolutionCapable` true, version from Cargo.
- `AppIcon.icns` generated from the same design as the Windows icon (CI step or committed).
- Universal binary via `lipo`; `.app` assembled by a small script; `codesign --force
  --deep --sign -`; zipped with `ditto -c -k --keepParent` (preserves bundle metadata).

### Testing

- `core` tests (all current pure tests: parsing, formatting, view, settings, fetcher,
  geometry, menu_action, generation gating) run on both Windows and macOS CI.
- Windows-only tests (DPAPI, GDI text measurement) stay in `windows/`.
- macOS-only tests: Keychain round-trip and redaction (runs on CI; macOS runners have an
  unlocked login keychain), text-measurement column-fit test using `NSString.sizeWithAttributes`
  with the system 12 pt font.
- CI smoke check (phase 4) proves the app launches, draws, and quits cleanly.
- Manual cloud-Mac test proves sign-in and live data. Until then, README marks macOS
  sign-in as untested.
- Leak check on Mac (tags/manual runs only, to save minutes): CI samples the debug demo
  app's RSS via `ps` for 10 minutes at a 30 s interval and fails if it grows by > 2 MB
  after warm-up.

### Documentation

- README gains a macOS section: email-code fallback if Google sign-in is refused, the
  Keychain "Always Allow" note, download zip, unzip, move to Applications, first-launch
  Gatekeeper approval steps (with macOS 15+ "Open Anyway" path), where data lives
  (Keychain item + Application Support), how to remove, and the tested/untested matrix.

## Calls made by the designer (not user decisions)

- Demo mode stays debug-only; CI builds a separate debug `.app` for smoke and leak checks.
- Accept the one-time Keychain "Always Allow" prompt after each manual update.
- Lean CI shape: Mac job path-filtered and arm64-only per push; universal build, leak
  check and release only on tags or manual runs.
- One sign-in pop-up at a time on Mac.

## Left to the builder

- Exact objc2 crate set and versions (`objc2`, `objc2-foundation`, `objc2-app-kit`,
  `objc2-web-kit`, `block2`, `dispatch2` or equivalent), and whether text drawing uses
  `NSAttributedString` or Core Text.
- Bundle-assembly script language (shell in CI is fine) and icon generation method.
- Exact GitHub Actions caching and artifact names beyond the two release asset names above.
