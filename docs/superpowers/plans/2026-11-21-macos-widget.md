# Claude Usage Widget for macOS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the shipped Windows widget into a Cargo workspace with a shared core, then add a native macOS floating widget built and checked entirely on GitHub Actions.

**Architecture:** `core/` (crate `usage_core`) holds everything platform-neutral, including a new `Controller` that owns app state, the fetcher and the generation counter and returns `Effect`s for the platform to perform, plus a platform-neutral menu model. `windows/` keeps today's Win32/WebView2 code, slimmed to rendering and OS glue. `macos/` is a new objc2 AppKit/WebKit front end over the same core. CI (private repo, lean on Mac minutes) builds and checks both.

**Tech Stack:** Rust stable (1.100), Cargo workspace; Windows: `windows` 0.62, `webview2-com` 0.39; macOS: `objc2` / `objc2-foundation` / `objc2-app-kit` / `objc2-web-kit` / `block2` / `dispatch2` (current releases), `security-framework` 3; shared: `ureq` 3 (rustls), `serde`, `chrono`; GitHub Actions `windows-latest`, `macos-14`.

**Spec:** `docs/superpowers/specs/2026-11-21-macos-widget-design.md` (builds on `docs/superpowers/specs/2026-11-20-claude-usage-widget-design.md`)

## Global Constraints

- Never read or reuse `~/.claude/.credentials.json` or any Claude Code OAuth token.
- No Cloudflare/bot-protection circumvention; honest `User-Agent: ClaudeUsageWidget/<version>` on HTTP; the Mac WKWebView keeps its default user agent and never claims to be Safari.
- Never log, print or put in test output/CI logs the session key. Read-only. Refresh interval floor 30 s.
- Windows behavior must not change in phase 1 (Tasks 1–2): same exe name `ClaudeUsageWidget.exe`, same window, menu, copy, settings/session file locations and formats.
- Exact copy (shared): "Signing in…", "Loading…", "Offline — retrying", "Signed out — click to sign in", "Usage format changed — check for an update", "No usage limits on this plan". Thresholds: green < 75, amber 75–90, red ≥ 90.
- Menu: Refresh now · Refresh every ▸ (30 s, 60 s, 2 min, 5 min) · Opacity ▸ (100%, 85%, 70%, 50%) · Organization ▸ (only when > 1 org) · separator · Sign out / Sign in · Quit.
- Mac: macOS 12+, `LSUIElement` true, bundle id `io.github.claudeusagewidget`, Keychain service `ClaudeUsageWidget` account `claude.ai`, settings in `~/Library/Application Support/ClaudeUsageWidget`, non-persistent `WKWebsiteDataStore`.
- `--demo` exists only in debug builds on both platforms. Release builds have no demo code.
- Release assets are named exactly `ClaudeUsageWidget.exe` and `ClaudeUsageWidget-mac.zip`.
- Repo is private: Mac CI runs only on path changes to `core/`, `macos/`, `Cargo.toml`, `Cargo.lock` or the workflow; arm64-only per push; universal build, leak check and release only on `v*` tags or manual dispatch.
- No pushing to `main` or creating tags without the user's say-so. Mac work is pushed to branch `mac-port` (private repo, user authorized pushes to it) so CI can build it.
- Never commit the user's email or account data.

## Review Focus

1. **Modal sign-in on Mac starving timers/events** — `runModalForWindow` runs the run loop in modal mode; the 1 s cookie timer and the 60 s countdown timer must be added in `NSRunLoopCommonModes`, and fetch events delivered via the GCD main queue. Expect: cookie polling keeps working while the sign-in window is up.
2. **Events from a stopped fetcher** — after sign-out, org switch or quit, a late event must not change state (generation gating now lives in `Controller`). Tests: `late_event_after_sign_out_is_ignored`, `current_generation_event_is_applied` (moved to core, Task 2).
3. **A `sessionKey` cookie on a non-claude.ai domain** (e.g. Google pop-up domains) must not be taken as the session. Test: `ignores_cookies_from_other_domains` (Task 6).
4. **Second launch on Mac** must exit silently while the first keeps running. Test: `second_lock_fails_while_first_held` (Task 4).
5. **Quit while a fetch is in flight** must exit within 5 s with no crash on both platforms. Covered by the CI smoke check (Task 5) and the existing `drop_joins_thread` test.

---

## File Structure

```
Cargo.toml                    [workspace] members core, windows, macos; resolver 2; [profile.release] (moved here)
.cargo/config.toml            unchanged (crt-static for windows-msvc)
core/Cargo.toml               usage_core: serde, serde_json, chrono, ureq; dev: tempfile
core/src/lib.rs               pub mod usage, format, view, source, fetcher, settings, geometry, session, layout, menu, controller, instance
core/src/{usage,format,view,source,fetcher}.rs   moved verbatim from src/ (imports → crate::)
core/src/settings.rs          moved; app_dir()/webview_dir() removed (platform-specific)
core/src/geometry.rs          moved; RECT → core Rect
core/src/session.rs           Session (+ redacted Debug), SessionStore trait, session_from_cookies, MemoryStore (cfg test)
core/src/layout.rs            W, PAD, ROW_H, NOTE_H, CLOSE, COL_*, colors as (u8,u8,u8), content_height, level_color
core/src/menu.rs              ID_* constants, Action, MenuSnapshot, menu_action, MenuEntry/MenuItem, build_menu
core/src/controller.rs        Controller, Effect
core/src/instance.rs          lock_single_instance (used by macOS)
windows/Cargo.toml            claude_usage_widget_win, bin "ClaudeUsageWidget"; windows-only deps under cfg(windows)
windows/build.rs              moved; embeds resources only when CARGO_CFG_TARGET_OS == "windows"
windows/app.manifest, resources.rc, assets/icon.ico   moved (git mv)
windows/src/main.rs           moved; mods + main under #[cfg(windows)], #[cfg(not(windows))] fn main() {}
windows/src/{widget,gdi,signin}.rs   moved; widget uses Controller/menu/layout from core
windows/src/dpapi_store.rs    DPAPI SessionStore (today's session.rs file I/O + dpapi module)
windows/src/paths.rs          app_dir(), webview_dir() (moved from settings.rs)
windows/examples/spike.rs     moved
macos/Cargo.toml              claude_usage_widget_mac, bin "claude-usage-widget-mac"; Apple deps under cfg(target_os = "macos")
macos/src/main.rs             #[cfg(target_os="macos")] app; else empty main
macos/src/{app,panel,draw,menu,signin,keychain,paths}.rs
macos/Info.plist              template with __VERSION__ placeholder
macos/scripts/bundle.sh       assemble .app (icon from windows/assets/icon.ico via sips + iconutil), ad-hoc sign
macos/scripts/smoke.sh        launch debug demo, screenshot, assert quit ≤ 5 s
macos/scripts/leakcheck.sh    10-min RSS sampling, fail on > 2 MB growth after warm-up
tests/fixtures/               unchanged (core tests include_str!("../../tests/fixtures/…"))
scripts/leakcheck.ps1         unchanged
.github/workflows/build.yml   CI
```

Mac binary is named `claude-usage-widget-mac` so it never collides with `ClaudeUsageWidget(.exe)` in the shared `target/` dir; `bundle.sh` copies it to `Contents/MacOS/ClaudeUsageWidget`.

---

### Task 1: Workspace + move pure modules to `core`

**Files:**
- Create: `core/Cargo.toml`, `core/src/lib.rs`, `windows/Cargo.toml`
- Move (git mv): `src/{usage,format,view,source,fetcher,settings,geometry,session}.rs` → `core/src/`; `src/{main,widget,gdi,signin}.rs` → `windows/src/`; `build.rs app.manifest resources.rc assets/ examples/` → `windows/`
- Create: `windows/src/paths.rs`, `windows/src/dpapi_store.rs`
- Modify: root `Cargo.toml`, `README.md` (build section), `.gitignore` unchanged

**Interfaces:**
- Produces (core): every existing pub item keeps its name and signature, now under `usage_core::<module>`, except:
  - `geometry`: `pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }` (Debug, Clone, Copy, PartialEq, Default); `default_position(size, primary: Rect)`, `clamp_to_work_areas(pos, size, areas: &[Rect], primary: Rect)`.
  - `settings`: no `app_dir`/`webview_dir`.
  - `session`: `pub struct Session` + redacted `Debug` only; `pub trait SessionStore { fn load(&self) -> Option<Session>; fn save(&self, s: &Session) -> std::io::Result<()>; fn clear(&self); }`.
  - `fetcher::Fetcher::spawn` reads the floor from `crate::settings::MIN_INTERVAL_SECS` (unchanged).
- Produces (windows): `paths::app_dir() -> PathBuf`, `paths::webview_dir() -> PathBuf` (bodies moved verbatim); `dpapi_store::DpapiStore { dir: PathBuf }` implementing `SessionStore` with today's `session.bin` format; free fns `save_session/load_session/clear_session` removed in favor of it.

- [ ] **Step 1: Record the baseline.** Run `cargo test 2>&1 | tail -5` on the current tree; note the passing count (expected 41). Run `cargo build --release` and note the exe size (~1.5 MB).
- [ ] **Step 2: Create the workspace and move files** with `git mv` (keeps history). Root `Cargo.toml` becomes `[workspace] members = ["core", "windows", "macos"]`, `resolver = "2"`, plus the existing `[profile.release]` block. Create a placeholder `macos/Cargo.toml` (`claude_usage_widget_mac`, bin `claude-usage-widget-mac`, path `src/main.rs`) and `macos/src/main.rs` containing only `fn main() {}` so the workspace resolves.
- [ ] **Step 3: Windows crate.** `windows/Cargo.toml`: package `claude_usage_widget_win`, version 0.1.0, `[[bin]] name = "ClaudeUsageWidget" path = "src/main.rs"`; `usage_core = { path = "../core" }`; `windows`, `windows-core`, `webview2-com` (same versions/features as today) under `[target.'cfg(windows)'.dependencies]`; `embed-resource` build-dep; `tempfile` dev-dep. `build.rs` returns early unless `std::env::var("CARGO_CFG_TARGET_OS") == Ok("windows")`. In `main.rs`, keep `#![windows_subsystem = "windows"]`, put `#[cfg(windows)]` on every `mod`, `use` and `fn` item, and add `#[cfg(not(windows))] fn main() {}`. Replace `crate::{usage,format,…}` imports with `usage_core::…`.
- [ ] **Step 4: Port geometry to `Rect`.** In core tests, replace `RECT` with `Rect` (same field names, so test bodies only change the type name). In `windows/src/widget.rs`, `work_areas()` converts each Win32 `RECT` to `Rect` field by field.
- [ ] **Step 5: Split session.** `core/src/session.rs` keeps `Session`, its `Debug`, and the `session_debug_redacts` test, and adds `SessionStore`. `windows/src/dpapi_store.rs` gets `Stored`, the `dpapi` module, and the DPAPI tests renamed to use `DpapiStore { dir }` (`session_roundtrip_dpapi`, `session_missing_or_tampered_is_none`, `clear_session_removes_file`). `main.rs`/`widget.rs` call `DpapiStore::load/save/clear` where they called the free functions.
- [ ] **Step 6: Fixture paths.** In `core/src/usage.rs` tests: `include_str!("../../tests/fixtures/usage_pro.json")` and `…/orgs.json`.
- [ ] **Step 7: Run the whole suite.** `cargo test --workspace 2>&1 | tail -15`. Expected: same total as Step 1 (41), all passing, split across `usage_core` and `claude_usage_widget_win`.
- [ ] **Step 8: Release build parity.** `cargo build --release -p claude_usage_widget_win`; expected `target/release/ClaudeUsageWidget.exe` exists, size within ±5% of Step 1. Update README "Build from source" to `cargo build --release -p claude_usage_widget_win` (output path unchanged).
- [ ] **Step 9: Commit** on a new branch `mac-port`: `git checkout -b mac-port`, `git add -A`, `git commit -m "refactor: split into workspace (core + windows), no behavior change"`.

### Task 2: Shared `Controller`, menu model and layout in core

**Files:**
- Create: `core/src/layout.rs`, `core/src/menu.rs`, `core/src/controller.rs`
- Modify: `core/src/lib.rs`, `core/src/session.rs` (add `MemoryStore` under `#[cfg(any(test, feature = "test-util"))]`), `windows/src/widget.rs`, `windows/src/main.rs`, `windows/src/signin.rs` (HINT moves to core `session::SIGN_IN_HINT`)

**Interfaces:**
- Consumes: Task 1's core modules, `SessionStore`.
- Produces:
  - `layout`: `pub const W: i32 = 252, PAD = 8, ROW_H = 20, NOTE_H = 16, CLOSE = 14, COL_LABEL = (PAD, 74), COL_BAR = (76, 124), COL_PCT = (126, 160), COL_REMAIN = (166, W - CLOSE - 6)`; `pub type Rgb = (u8, u8, u8)`; `BG = (0x1F,0x1F,0x1E), TEXT = (0xEC,0xEC,0xEA), DIM = (0x8A,0x8A,0x85), TRACK = (0x3A,0x3A,0x38), GREEN = (0x5C,0xB8,0x5C), AMBER = (0xE0,0xA0,0x30), RED = (0xE0,0x55,0x45)`; `pub fn level_color(l: Level, stale: bool) -> Rgb`; `pub fn content_height(v: &ViewModel) -> i32` (today's `logical_height` body).
  - `menu`: `ID_REFRESH = 100, ID_INTERVAL = 110, ID_OPACITY = 120, ID_ORG = 200, ID_SIGN_OUT = 300, ID_QUIT = 301, ID_SIGN_IN = 302`; `pub enum Action { None, Refresh, Interval(u32), Opacity(u8), Org(String), SignOut, SignIn, Quit }`; `pub struct MenuSnapshot { pub orgs: Vec<String> }`; `pub fn menu_action(id: usize, snap: &MenuSnapshot) -> Action` (moved); `pub struct MenuItem { pub id: usize, pub label: String, pub checked: bool, pub enabled: bool }`; `pub enum MenuEntry { Item(MenuItem), Submenu(String, Vec<MenuItem>), Separator }`; `pub fn build_menu(c: &Controller) -> (Vec<MenuEntry>, MenuSnapshot)`.
  - `controller`:
    ```rust
    pub enum Effect { Relayout, Opacity(u8), SignInCleanup }
    pub type Sink = std::sync::Arc<dyn Fn(u64, Event) + Send + Sync>;
    pub struct Controller { pub settings: Settings, pub state: AppState, pub orgs: Vec<Org>,
        pub current_org: Option<String>, pub session: Option<Session>, pub quitting: bool, /* private: dir, gen, fetcher, store, sink */ }
    impl Controller {
        pub fn new(dir: PathBuf, settings: Settings, store: Box<dyn SessionStore>, sink: Sink) -> Controller;
        pub fn gen(&self) -> u64;
        pub fn is_fetching(&self) -> bool;
        pub fn view(&self, now: DateTime<Utc>) -> ViewModel;
        pub fn set_state(&mut self, s: AppState) -> Vec<Effect>;          // [Relayout]
        pub fn start(&mut self, source: Box<dyn UsageSource>, last_active: Option<String>) -> Vec<Effect>;
        pub fn start_with_session(&mut self, s: Session) -> Vec<Effect>;  // HttpSource, state Loading
        pub fn signed_in(&mut self, s: Session) -> Vec<Effect>;           // store.save then start_with_session
        pub fn signed_out(&mut self) -> Vec<Effect>;                      // drop fetcher, gen+1, store.clear, SignedOut
        pub fn handle_posted(&mut self, gen: u64, ev: Event) -> Vec<Effect>;
        pub fn apply(&mut self, a: Action) -> Vec<Effect>;                // SignOut adds SignInCleanup
        pub fn save_settings(&self);
        pub fn prepare_quit(&mut self) -> bool;                           // false if already quitting; drops fetcher, saves settings
    }
    ```
    Bodies are today's `App` methods with Win32 calls replaced by returned effects; `sign_in`/`quit` stay platform-side.

- [ ] **Step 1: Write the failing tests** in core (they reference modules that don't exist yet):
  - `controller.rs`: move `late_event_after_sign_out_is_ignored` and `current_generation_event_is_applied` from `windows/src/widget.rs`, rewritten against `Controller::new(tempdir, Settings::default(), Box::new(MemoryStore::default()), Arc::new(|_, _| {}))`. Add:
    - `signed_out_clears_store`: store holds a session → `signed_out()` → `store.load() == None`, `state == SignedOut`, effects contain `Relayout`.
    - `signed_in_saves_store`: `signed_in(s)` → store returns `s`, `state == Loading`, `is_fetching()`.
    - `network_error_marks_data_stale`: state Data → `handle_posted(gen, Event::Error(Network))` → `Data { stale: true }`; from Loading → `Offline`.
    - `auth_error_signs_out`, `format_error_shows_format_changed`.
    - `opacity_action_saves_and_returns_effect`: `apply(Action::Opacity(70))` → effects `[Opacity(70)]`, `Settings::load(dir).opacity_pct == 70`.
    - `sign_out_action_requests_cleanup`: effects contain `SignInCleanup`.
  - `menu.rs`: move `menu_action_uses_snapshot_not_live_state`. Add `menu_signed_in_two_orgs` asserting the exact entry sequence: `Item("Refresh now", enabled)`, `Submenu("Refresh every", ["30 s","60 s","2 min","5 min"] with "60 s" checked)`, `Submenu("Opacity", ["100%","85%","70%","50%"] with "100%" checked)`, `Submenu("Organization", [org names] with current checked)`, `Separator`, `Item("Sign out")`, `Item("Quit")`; snapshot orgs = uuids in order. Add `menu_signed_out_one_org`: "Refresh now" disabled, no Organization submenu, "Sign in" instead of "Sign out".
  - `layout.rs`: `content_height_matches_windows`: Message → `PAD*2 + ROW_H*2` (56); 2 rows no note → 56; 2 rows + note → 72. `stale_rows_use_dim`: `level_color(Red, true) == DIM`, `level_color(Amber, false) == AMBER`.
- [ ] **Step 2: Run** `cargo test -p usage_core 2>&1 | tail -20`. Expected: compile errors for missing `controller`/`menu`/`layout` items.
- [ ] **Step 3: Implement** `layout.rs`, `menu.rs`, `controller.rs`, and `MemoryStore` (a `RefCell<Option<Session>>`-backed `SessionStore`, behind `cfg(any(test, feature = "test-util"))`; add `[features] test-util = []`). Move `HINT` to `usage_core::session::SIGN_IN_HINT` (same text).
- [ ] **Step 4: Run** `cargo test -p usage_core`. Expected: all pass.
- [ ] **Step 5: Slim the Windows widget.** `App` keeps `hwnd`, `ctl: Controller`, `sign_in`, `dpi`, `font`, `font_bold`; add `fn run_effects(&mut self, e: Vec<Effect>)` (Relayout → `resize_to_content`; Opacity → `SetLayeredWindowAttributes`; SignInCleanup → `crate::signin_cleanup()`). `build_menu` renders `menu::build_menu(&self.ctl)` into an `HMENU` (labels, `MF_CHECKED`, `MF_GRAYED`, `MF_SEPARATOR`, `MF_POPUP`). Paint uses `layout::*` and `level_color` via `rgb(r,g,b)`. The sink is `Arc::new(move |gen, ev| post_event(hwnd, gen, ev))`, so the `Controller` is created after `CreateWindowExW`. Demo: `ctl.start(Box::new(DemoSource::default()), None)`. `main.rs` sign-in hook no longer saves the session (Controller `signed_in` does); `begin_sign_in` calls `ctl.signed_in(session)`. Keep `columns_fit_longest_strings` in `windows/` using `layout::COL_*`.
- [ ] **Step 6: Run the whole suite.** `cargo test --workspace 2>&1 | tail -15`. Expected: all pass; total ≥ 41 + the new tests.
- [ ] **Step 7: Windows parity check.** `cargo build -p claude_usage_widget_win` then run `target/debug/ClaudeUsageWidget.exe --demo`; screenshot the widget (PowerShell `CopyFromScreen` on its window rect) to the scratchpad and compare against `docs/screenshot.png`: same layout, colors, text. Right-click: menu entries in the Global Constraints order. Click ✕: process exits. Then `cargo build --release -p claude_usage_widget_win`: exe size within ±5% of Task 1's.
- [ ] **Step 8: Commit** `refactor: shared Controller, menu model and layout in core`.

### Task 3: CI workflow (Windows + lean Mac)

**Files:**
- Create: `.github/workflows/build.yml`

**Interfaces:**
- Consumes: workspace from Tasks 1–2.
- Produces: jobs `windows`, `mac-changes` (path filter), `macos`, `release` (later tasks extend `macos` and `release`); artifact names `ClaudeUsageWidget.exe`, `ClaudeUsageWidget-mac` (zip), `mac-smoke-screenshot`.

- [ ] **Step 1: Write the workflow.**
  - Triggers: `push` (all branches), `pull_request`, `workflow_dispatch`, tags `v*`.
  - `windows` (windows-latest, every run): `actions/checkout`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache`, `cargo test --workspace`, `cargo build --release -p claude_usage_widget_win`, upload `target/release/ClaudeUsageWidget.exe`.
  - `mac-changes` (ubuntu-latest): `dorny/paths-filter` on `core/**`, `macos/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/**`; output `mac`.
  - `macos` (macos-14): `if: needs.mac-changes.outputs.mac == 'true' || startsWith(github.ref, 'refs/tags/v') || github.event_name == 'workflow_dispatch'`; checkout, toolchain, rust-cache, `cargo test -p usage_core -p claude_usage_widget_mac`, `cargo build -p claude_usage_widget_mac` (debug), `cargo build --release -p claude_usage_widget_mac`. `timeout-minutes: 15`.
  - `release` placeholder job is added in Task 7.
- [ ] **Step 2: Lint locally.** Parse the YAML: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/build.yml'))"`. Expected: no error.
- [ ] **Step 3: Commit and push the branch.** `git add .github && git commit -m "ci: windows + lean macOS build"`, `git push -u origin mac-port`.
- [ ] **Step 4: Watch the run.** `gh run watch --exit-status $(gh run list --branch mac-port --limit 1 --json databaseId -q '.[0].databaseId')`. Expected: `windows` and `macos` both green (macOS builds the placeholder mac crate and passes core tests). Note the macOS job's minutes in the ledger.

### Task 4: Mac crate foundation — Keychain, single instance, bundle

**Files:**
- Create: `core/src/instance.rs`, `macos/src/keychain.rs`, `macos/src/paths.rs`, `macos/Info.plist`, `macos/scripts/bundle.sh`
- Modify: `core/src/lib.rs`, `macos/Cargo.toml`, `macos/src/main.rs`, `.github/workflows/build.yml`

**Interfaces:**
- Consumes: `SessionStore`, `Session`.
- Produces:
  - core `instance::lock_single_instance(dir: &Path) -> Option<std::fs::File>`: creates `dir/.lock`, returns `Some(file)` holding `File::try_lock()` (std, stable since 1.89), `None` if already locked. Lock released when the `File` drops.
  - macos `keychain::KeychainStore` (unit struct) implementing `SessionStore` via `security_framework::passwords::{set_generic_password, get_generic_password, delete_generic_password}` with service `"ClaudeUsageWidget"`, account `"claude.ai"`, value = JSON `{session_key, last_active_org}`; a constructor `KeychainStore::with_account(&'static str)` used by tests so they never touch the real item.
  - macos `paths::app_dir() -> PathBuf`: `$HOME/Library/Application Support/ClaudeUsageWidget`, created if missing.
  - `bundle.sh <binary> <out_dir> <version>` → `<out_dir>/ClaudeUsageWidget.app` (Contents/MacOS/ClaudeUsageWidget, Contents/Info.plist with `__VERSION__` replaced, Contents/Resources/AppIcon.icns), then `codesign --force --deep --sign - <app>`.

- [ ] **Step 1: Write the failing tests.**
  - core `instance.rs`: `second_lock_fails_while_first_held` (first `Some`, second `None`), `lock_released_on_drop` (drop first → second `Some`).
  - macos `keychain.rs` (runs only on macOS CI): `keychain_roundtrip` with account `"claude.ai-test-<pid>"`: save → load equals → clear → load `None`; `debug_never_shows_key`: `format!("{:?}", loaded)` doesn't contain the test key.
- [ ] **Step 2: Run** `cargo test -p usage_core instance`. Expected: fails (module missing).
- [ ] **Step 3: Implement** `instance.rs`, `keychain.rs`, `paths.rs`. `macos/Cargo.toml`: deps `usage_core`; under `[target.'cfg(target_os = "macos")'.dependencies]`: `objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-web-kit`, `block2`, `dispatch2`, `security-framework`, `serde_json`, `chrono`. `main.rs`: `#[cfg(target_os = "macos")] mod …;` for each module and `#[cfg(not(target_os = "macos"))] fn main() {}`.
- [ ] **Step 4: Run** `cargo test --workspace` locally. Expected: core instance tests pass on Windows; mac crate builds as an empty main.
- [ ] **Step 5: Info.plist and bundle.sh.** Info.plist keys: `CFBundleIdentifier io.github.claudeusagewidget`, `CFBundleName`/`CFBundleDisplayName` "Claude Usage Widget", `CFBundleExecutable ClaudeUsageWidget`, `CFBundlePackageType APPL`, `CFBundleShortVersionString`/`CFBundleVersion __VERSION__`, `LSUIElement true`, `LSMinimumSystemVersion 12.0`, `NSHighResolutionCapable true`, `CFBundleIconFile AppIcon`. Icon: `sips -s format png windows/assets/icon.ico --out` → resize with `sips -z` into an `.iconset` (16, 32, 64, 128, 256, plus @2x where source allows) → `iconutil -c icns`.
- [ ] **Step 6: Extend CI `macos` job:** after the release build, `macos/scripts/bundle.sh target/release/claude-usage-widget-mac dist <version from Cargo>`; `codesign --verify --verbose dist/ClaudeUsageWidget.app`; `ditto -c -k --keepParent dist/ClaudeUsageWidget.app ClaudeUsageWidget-mac.zip`; upload as `ClaudeUsageWidget-mac`.
- [ ] **Step 7: Commit, push, watch** (as Task 3 Steps 3–4). Expected: macOS job green including `keychain_roundtrip`, `codesign --verify` passes, zip artifact present.

### Task 5: Mac widget window (+ CI smoke check)

**Files:**
- Create: `macos/src/app.rs`, `macos/src/panel.rs`, `macos/src/draw.rs`, `macos/src/menu.rs`, `macos/scripts/smoke.sh`
- Modify: `macos/src/main.rs`, `.github/workflows/build.yml`

**Interfaces:**
- Consumes: `Controller`, `Effect`, `menu::{build_menu, menu_action, Action, MenuEntry}`, `layout::*`, `geometry::{clamp_to_work_areas, default_position, Rect}`, `instance::lock_single_instance`, `KeychainStore`, `paths::app_dir`.
- Produces: `app::run(start: StartMode) -> i32` where `enum StartMode { NeedSignIn, Session(Session), #[cfg(debug_assertions)] Demo }`; a main-thread `App` holding the `Controller`, `NSPanel`, view, countdown `NSTimer`; `app::begin_sign_in()` hook that Task 6 fills (until then: sets SignedOut).

- [ ] **Step 1: Write the failing test** `columns_fit_longest_strings_mac` in `draw.rs` (macOS only): with `NSFont::systemFontOfSize(12)` (bold for labels), measure via `NSString` `sizeWithAttributes:` and assert each string fits its column: "Sonnet wk" (bold) ≤ `COL_LABEL.1 - COL_LABEL.0`, "130%" ≤ `COL_PCT` width, "resetting…", "4d 23h" ≤ `COL_REMAIN` width.
- [ ] **Step 2: Implement the window.**
  - `main.rs`: `lock_single_instance(&app_dir())` → `None` exits 0. Debug: `--demo` → `StartMode::Demo`; `--quit-after <secs>` (debug only) schedules the normal quit path. Otherwise `KeychainStore.load()` → `Session` or `NeedSignIn`.
  - `NSApplication` with `ActivationPolicy::Accessory`.
  - `panel.rs`: `NSPanel` styles `NonactivatingPanel | Borderless`, level floating, `collectionBehavior = CanJoinAllSpaces | FullScreenAuxiliary | Stationary`, `hidesOnDeactivate = false`, `becomesKeyOnlyIfNeeded = true`, `movableByWindowBackground = true`, clear background, `alphaValue = opacity/100`. Content view layer `cornerRadius 6`, `masksToBounds`. Size `W × content_height` points.
  - `draw.rs`: `define_class!` NSView subclass, `isFlipped = true`, `drawRect:` paints `controller.view(Utc::now())` with the same geometry as Windows (`layout::*`), colors via `level_color`, close glyph "✕" at the top-right `CLOSE` box, message text word-wrapped within `PAD..W-CLOSE-8`. `mouseDown:` on ✕ → quit; when state is `SignedOut` → `begin_sign_in`; `mouseDownCanMoveWindow` returns false over ✕ and when SignedOut. `menuForEvent:` returns an `NSMenu` built from `build_menu` (tag = id, `state` on for checked, `enabled`), action routes through `menu_action` with the snapshot.
  - Position: on start, clamp saved `settings.pos` against `NSScreen.screens` `visibleFrame`s converted to top-left-origin `Rect`s (flip y using the primary screen height); `windowDidMove:` saves the new top-left.
  - Sink: `Arc::new(|gen, ev| DispatchQueue::main().exec_async(move || App::with(|a| a.on_posted(gen, ev))))`; `run_effects`: Relayout → resize panel (keep top-left fixed) + `setNeedsDisplay`; Opacity → `alphaValue`; SignInCleanup → no-op on Mac (non-persistent store).
  - Countdown `NSTimer` 60 s, added to the run loop in `NSRunLoopCommonModes`.
  - Quit: `prepare_quit()` → invalidate timer → `orderOut` → `NSApp.terminate`. The lock file drops on exit.
- [ ] **Step 3: smoke.sh.** Build a debug bundle with `bundle.sh target/debug/claude-usage-widget-mac dist-debug dev`; run `dist-debug/ClaudeUsageWidget.app/Contents/MacOS/ClaudeUsageWidget --demo --quit-after 20 &`; after 10 s `screencapture -x smoke.png` (non-fatal if it fails); `wait` with a 25 s ceiling; fail unless exit code 0 within 25 s (quit-after 20 + ≤ 5 s). Also launch a second instance during the first and assert it exits 0 within 2 s.
- [ ] **Step 4: CI.** `macos` job: build debug, `cargo test -p claude_usage_widget_mac`, run `smoke.sh`, upload `smoke.png` as `mac-smoke-screenshot` (`if: always()`).
- [ ] **Step 5: Commit, push, watch.** Expected: job green; download `mac-smoke-screenshot` with `gh run download` and look at it: widget top-right, Session + Weekly rows, bars, percents, countdowns, matching the Windows screenshot's layout. Fix and re-push until it does (each push costs Mac minutes; batch fixes).

### Task 6: Mac sign-in (WKWebView)

**Files:**
- Create: `macos/src/signin.rs`
- Modify: `core/src/session.rs` (add `session_from_cookies`), `macos/src/app.rs`, `macos/scripts/smoke.sh`

**Interfaces:**
- Consumes: `SIGN_IN_HINT`, `Controller::signed_in`, `AppState::{SigningIn, SignedOut}`.
- Produces:
  - core `pub fn session_from_cookies<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Option<Session>` over `(name, domain, value)`: domain (leading `.` stripped) must equal `claude.ai` or end with `.claude.ai`; non-empty `sessionKey` required; `lastActiveOrg` optional.
  - macos `signin::sign_in(mtm) -> Result<Session, SignInError>` with `enum SignInError { Cancelled, Failed(String) }`.

- [ ] **Step 1: Write the failing tests** in core: `takes_claude_ai_session_cookie` (`("sessionKey", ".claude.ai", "k")`, `("lastActiveOrg", "claude.ai", "o")` → `Session { "k", Some("o") }`); `ignores_cookies_from_other_domains` (`("sessionKey", "accounts.google.com", "x")` → `None`; `("sessionKey", "evilclaude.ai", "x")` → `None`); `empty_session_key_is_none`.
- [ ] **Step 2: Run** `cargo test -p usage_core session`. Expected: fails (fn missing).
- [ ] **Step 3: Implement** `session_from_cookies`; run again, expected pass.
- [ ] **Step 4: Implement `signin.rs`.** Titled, closable `NSWindow` 480 × 720 pt, centered, title "Sign in to Claude"; a 30 pt hint label with `SIGN_IN_HINT` above a `WKWebView` whose configuration uses `WKWebsiteDataStore::nonPersistentDataStore()`; never set `customUserAgent`. Load `https://claude.ai/login`. A `define_class!` delegate implementing `WKUIDelegate` `webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:` (close any existing pop-up, create a `WKWebView` with the supplied configuration in its own 480 × 640 window, return it) and `webViewDidClose:` (close that window), plus `NSWindowDelegate windowWillClose:` on the main sign-in window → cancelled. A 1 s `NSTimer` in `NSRunLoopCommonModes` calls `httpCookieStore().getAllCookiesWithCompletionHandler` and passes `(name, domain, value)` to `session_from_cookies`; on `Some` → done. Run with `NSApp.runModalForWindow`; `stopModal` on done/cancel. Teardown: invalidate timer, close pop-up, `stopLoading`, remove web views, close windows. Never log cookie values.
- [ ] **Step 5: Wire** `app::begin_sign_in`: ignore if already `SigningIn` or quitting; `set_state(SigningIn)`; `sign_in()`; `Ok(s)` → `ctl.signed_in(s)`; `Err(Cancelled)` → `SignedOut`; `Err(Failed(e))` → `NSAlert` "Sign-in failed: {e}" then `SignedOut`. On `NeedSignIn` start, schedule `begin_sign_in` on the main queue after launch.
- [ ] **Step 6: CI sign-in smoke (non-fatal).** Debug flag `--signin-smoke` (debug only): opens the sign-in window, waits 15 s, and quits. `smoke.sh` runs it, screenshots to `signin.png`, uploads it with the smoke artifact; asserts only that the process exits 0 within 25 s.
- [ ] **Step 7: Commit, push, watch.** Expected: green; `signin.png` shows the claude.ai login page under the hint line (a Cloudflare page there is information for the README, not a failure). Mac sign-in itself stays "untested" until the user's cloud-Mac test.

### Task 7: Release pipeline, leak check, README

**Files:**
- Create: `macos/scripts/leakcheck.sh`
- Modify: `.github/workflows/build.yml`, `README.md`

**Interfaces:**
- Consumes: bundle.sh, smoke tooling, Windows exe artifact.
- Produces: on `v*` tags/manual dispatch: universal `ClaudeUsageWidget-mac.zip`, leak-check result, and (tags only) a GitHub Release with `ClaudeUsageWidget.exe` + `ClaudeUsageWidget-mac.zip`.

- [ ] **Step 1: leakcheck.sh.** Launch the debug demo app with `--quit-after 660`; every 30 s record `ps -o rss= -p <pid>` (KB) to `leak.csv`; after it exits, compare the max of the last 10 samples to the sample at minute 2; exit 1 if growth > 2048 KB, and print the summary either way.
- [ ] **Step 2: Workflow.** In `macos`, steps gated on `startsWith(github.ref, 'refs/tags/v') || github.event_name == 'workflow_dispatch'`: `rustup target add x86_64-apple-darwin`, release build for `aarch64-apple-darwin` and `x86_64-apple-darwin`, `lipo -create -output target/universal/claude-usage-widget-mac …`, `lipo -info` must list both archs, bundle + verify + zip the universal binary (replacing the arm64 zip), run `leakcheck.sh`, upload `leak.csv`. New `release` job (`needs: [windows, macos]`, `if: startsWith(github.ref, 'refs/tags/v')`): download both artifacts, `softprops/action-gh-release` with exactly `ClaudeUsageWidget.exe` and `ClaudeUsageWidget-mac.zip`; `permissions: contents: write`.
- [ ] **Step 3: README.** Retitle intro to Windows and macOS. New "macOS" section: download zip → unzip → drag to Applications → first launch: right-click → Open, or on macOS 15+ System Settings → Privacy & Security → **Open Anyway**; Keychain prompt after each update → **Always Allow**; Google sign-in refused → use **Continue with email**; data: Keychain item "ClaudeUsageWidget" + `~/Library/Application Support/ClaudeUsageWidget`; removal steps (delete app, folder, Keychain item in Keychain Access); sign-in matrix marks macOS **untested** until the manual test. Build from source: `cargo build --release -p claude_usage_widget_mac` + `macos/scripts/bundle.sh`.
- [ ] **Step 4: Commit, push, trigger manual run.** `gh workflow run build.yml --ref mac-port`, then watch. Expected: all jobs green, `lipo -info` shows `x86_64 arm64`, leak check passes (growth ≤ 2 MB), no `release` job (not a tag). Record Mac minutes used in the ledger.
- [ ] **Step 5: Hand off to the user** for the manual cloud-Mac test (spec phase 5: Google sign-in, live numbers, menu, drag, quit, restart without re-sign-in) and for permission to merge `mac-port` into `main` and tag a release.
