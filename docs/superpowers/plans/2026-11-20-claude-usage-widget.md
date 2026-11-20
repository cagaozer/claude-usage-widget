# Claude Usage Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single portable Windows `.exe` that shows claude.ai session (5-hour) and weekly usage in a tiny always-on-top widget, refreshing on a timer.

**Architecture:** Native Win32 layered window drawn with GDI on the UI thread; one background fetch thread behind a `UsageSource` trait (plain HTTPS first, hidden-WebView2 only if the spike proves it necessary); WebView2 sign-in window created only on demand and fully torn down afterwards. Pure logic (parsing, formatting, view model, settings, status classification) lives in its own modules and is unit-tested; Win32/WebView2 glue is verified manually.

**Tech Stack:** Rust stable (x86_64-pc-windows-msvc), `windows` crate, `webview2-com` (use the latest release and the exact `windows` version it depends on), `ureq` (rustls), `serde`/`serde_json`, `chrono` (serde, no default features beyond `std`/`clock`), `embed-resource` (build), `tempfile` (dev).

**Spec:** `docs/superpowers/specs/2026-11-20-claude-usage-widget-design.md`

## Global Constraints

- Windows 10/11 x64; single `.exe`, no installer, no admin; static CRT (`+crt-static`); WebView2 loader statically linked.
- Release profile: `opt-level="z"`, `lto=true`, `panic="abort"`, `strip=true`, `codegen-units=1`. Target exe ≤ 5 MB.
- Steady-state memory ~5–15 MB (plain-HTTP backend) or ≤ 100 MB (WebView2 backend); must stay flat over hours.
- Refresh interval: default 60 s, menu choices 30 s / 60 s / 2 min / 5 min, floor 30 s.
- Opacity choices: 100% / 85% / 70% / 50%.
- Bar colors: green < 75%, amber 75–90%, red ≥ 90%.
- Never read `~/.claude/.credentials.json`; never use Claude Code's OAuth client.
- No Cloudflare circumvention: no `cf_clearance` copying, no TLS fingerprint spoofing, no fake browser User-Agent. Plain HTTP client sends `User-Agent: ClaudeUsageWidget/<CARGO_PKG_VERSION>`.
- Never log or print the session key.
- Storage: `%APPDATA%\ClaudeUsageWidget\settings.json`, `%APPDATA%\ClaudeUsageWidget\session.bin` (DPAPI, current user), WebView2 data in `%LOCALAPPDATA%\ClaudeUsageWidget\WebView2`.
- Single instance via named mutex `Local\ClaudeUsageWidget.SingleInstance`; a second launch exits silently.
- Widget never takes focus on refresh; no taskbar button (`WS_EX_TOOLWINDOW`).
- UI copy (exact): "Signing in…", "Loading…", "Offline — retrying", "Signed out — click to sign in", "Usage format changed — check for an update", "No usage limits on this plan". Sign-in window hint: "Google sign-in blocked? Use 'Continue with email' with your Google address."
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. `resets_at` in the past, null, or the PC clock skewed → countdown shows "resetting…", never negative or garbage (Task 2 test `remaining_past_or_none`).
2. Laptop sleeps / Wi-Fi drops → widget shows last numbers dimmed with "Offline — retrying" and recovers by itself on the next tick (Task 5 test `network_error_then_recovers`).
3. Saved position is on a monitor that's no longer attached → widget reappears on the primary work area (Task 6 test `clamp_offscreen_position`).
4. Utilization above 100 or reported as a 0–1 fraction → bar clamps to full, text shows the real percent; parser normalizes scale (Task 2 test `utilization_scale_and_overflow`).
5. Session expires mid-run → one auth failure clears the stored session and shows "Signed out — click to sign in"; no retry storm (Task 5 test `auth_error_stops_polling`).

---

### Task 1: Scaffold, build config, and endpoint spike

**Files:**
- Create: `Cargo.toml`, `.cargo/config.toml`, `build.rs`, `app.manifest`, `resources.rc`, `assets/icon.ico`, `.gitignore`, `src/main.rs` (stub), `examples/spike.rs`, `tests/fixtures/usage_*.json`, `tests/fixtures/orgs.json`, `docs/superpowers/spike-result.md`

**Interfaces:**
- Produces: a building crate `claude_usage_widget`; fixtures with the real response shapes used by Task 2; a recorded decision `BACKEND=http` or `BACKEND=webview2` in `spike-result.md`.

- [ ] **Step 1:** Verify toolchain: `rustc --version` and `cargo --version` succeed; `rustup target list --installed` includes `x86_64-pc-windows-msvc`.
- [ ] **Step 2:** Create `Cargo.toml` (package `claude_usage_widget`, edition 2021, `[[bin]] name = "ClaudeUsageWidget"`, release profile from Global Constraints), `.cargo/config.toml` with `rustflags = ["-C", "target-feature=+crt-static"]`, `.gitignore` (`/target`), `app.manifest` (per-monitor-v2 DPI aware, Common Controls v6, `asInvoker`), `resources.rc` embedding manifest and `assets/icon.ico`, and `build.rs` calling `embed_resource::compile("resources.rc", embed_resource::NONE)`. Icon: a 32×32 + 16×16 ICO of an orange (#D97757) rounded square with a white bar; generate with a short dependency-free Python script kept in the scratchpad, not the repo.
- [ ] **Step 3:** `src/main.rs`: `#![windows_subsystem = "windows"]` and an empty `fn main()`. Run `cargo build --release`. Expected: success; `target/release/ClaudeUsageWidget.exe` exists and shows the icon in Explorer.
- [ ] **Step 4:** Write `examples/spike.rs`: reads a session key from stdin (prompt "Paste sessionKey cookie value:"), then with `ureq` and the honest User-Agent calls `GET https://claude.ai/api/organizations` and `GET https://claude.ai/api/organizations/{first uuid}/usage` with header `Cookie: sessionKey=<key>`. Prints HTTP status, `content-type`, whether the body looks like a Cloudflare challenge (contains `cf-chl` or `Just a moment`), and writes bodies to `tests/fixtures/orgs.json` and `tests/fixtures/usage_<plan>.json` with UUIDs, emails, and names replaced by placeholders. Never prints the key.
- [ ] **Step 5:** Ask the user to run `cargo run --example spike` in their own terminal (stdin is needed), pasting `sessionKey` from claude.ai DevTools → Application → Cookies. Expected: status 200 with JSON for both calls.
- [ ] **Step 6:** Record in `docs/superpowers/spike-result.md`: statuses, the actual JSON field names for 5-hour/weekly/per-model windows, utilization scale (0–100 vs 0–1), timestamp format, and `BACKEND=http` (200 JSON) or `BACKEND=webview2` (403/challenge). If the field names differ from the spec's assumptions (`five_hour`, `seven_day`, `utilization`, `resets_at`), Task 2 uses the observed names.
- [ ] **Step 7:** Commit: `chore: scaffold crate, build config, and usage endpoint spike`.

### Task 2: Usage model, parsing, formatting, view model

**Files:**
- Create: `src/usage.rs`, `src/format.rs`, `src/view.rs`; Modify: `src/main.rs` (module decls)
- Test: inline `#[cfg(test)]` modules in each file, using `tests/fixtures/*.json` via `include_str!`

**Interfaces:**
- Produces:
  - `usage.rs`: `pub struct Window { pub utilization: f64 /* percent 0..=∞ */, pub resets_at: Option<DateTime<Utc>> }`; `pub struct Limits { pub session: Window, pub weekly: Window, pub extra: Vec<(String, Window)> }`; `pub enum UsageState { Limits(Limits), NoLimits }`; `pub fn parse_usage(json: &str) -> Result<UsageState, ParseError>`; `pub struct Org { pub uuid: String, pub name: String }`; `pub fn parse_orgs(json: &str) -> Result<Vec<Org>, ParseError>`; `pub struct ParseError(pub String)`.
  - `format.rs`: `pub fn format_remaining(now: DateTime<Utc>, resets_at: Option<DateTime<Utc>>) -> String`; `pub enum Level { Green, Amber, Red }`; `pub fn level(pct: f64) -> Level`.
  - `view.rs`: `pub struct Row { pub label: String, pub pct: f64, pub fill: f64 /* 0..=1 */, pub pct_text: String, pub remaining: String, pub level: Level }`; `pub enum Status { Fresh, Stale /* dimmed */ }`; `pub enum ViewModel { Message(String), Rows { rows: Vec<Row>, status: Status, note: Option<String> } }`; `pub fn build_view(state: &AppState, now: DateTime<Utc>) -> ViewModel`; `pub enum AppState { SigningIn, Loading, SignedOut, NoLimits, FormatChanged, Offline, Data { limits: Limits, stale: bool } }` (offline with prior data = `Data { stale: true }` → note "Offline — retrying"; offline with no prior data = `Offline` → `Message("Offline — retrying")`).

- [ ] **Step 1: Write failing tests**
  - `parses_real_fixture`: `parse_usage(include_str!(fixture))` → `Limits` with session/weekly utilization equal to the fixture's values and `resets_at` parsed.
  - `null_windows_is_no_limits`: `{"five_hour":null,"seven_day":null}` → `NoLimits`; `{}` → `NoLimits`.
  - `garbage_is_parse_error`: `"<html>"` and `"[]"` → `Err`.
  - `utilization_scale_and_overflow`: utilization `0.42` → `42.0` only if the spike recorded a 0–1 scale, otherwise `42` stays `42.0`; `130` → row `fill == 1.0`, `pct_text == "130%"`.
  - `extra_windows_only_when_present`: fixture with a non-null per-model weekly window → `extra.len() == 1`; null → `0`.
  - `format_remaining_cases`: 1h52m → `"1h 52m"`; 4d6h10m → `"4d 6h"`; 12m30s → `"12m"`; 40s → `"<1m"`.
  - `remaining_past_or_none`: past timestamp → `"resetting…"`; `None` → `"—"`.
  - `level_thresholds`: 74.9 → Green, 75.0 → Amber, 89.9 → Amber, 90.0 → Red.
  - `view_states`: `SignedOut` → `Message("Signed out — click to sign in")`; `NoLimits` → `Message("No usage limits on this plan")`; `Data{stale:true}` → `status == Stale`, `note == Some("Offline — retrying")`; row labels `"Session"`, `"Weekly"`.
- [ ] **Step 2:** Run `cargo test`. Expected: compile errors / FAIL.
- [ ] **Step 3:** Implement with serde `Deserialize` into `Option` fields so null/absent both work; normalize scale per spike result; extra-row label is the model name capitalized (e.g. `"Opus wk"`).
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.
- [ ] **Step 5:** Commit: `feat: usage parsing, countdown formatting, and view model`.

### Task 3: Settings and encrypted session storage

**Files:**
- Create: `src/settings.rs`, `src/session.rs`; Test: inline tests with `tempfile`

**Interfaces:**
- Produces:
  - `pub struct Settings { pub opacity_pct: u8, pub interval_secs: u32, pub pos: Option<(i32, i32)>, pub org_id: Option<String> }` with `Default` = `{100, 60, None, None}`; `impl Settings { pub fn load(dir: &Path) -> Settings; pub fn save(&self, dir: &Path) -> io::Result<()>; pub fn sanitized(self) -> Settings }`.
  - `pub fn app_dir() -> PathBuf` (`%APPDATA%\ClaudeUsageWidget`, created if missing); `pub fn webview_dir() -> PathBuf` (`%LOCALAPPDATA%\ClaudeUsageWidget\WebView2`).
  - `pub struct Session { pub session_key: String, pub last_active_org: Option<String> }` (`Debug` impl redacts the key); `pub fn save_session(dir: &Path, s: &Session) -> io::Result<()>`; `pub fn load_session(dir: &Path) -> Option<Session>`; `pub fn clear_session(dir: &Path)`.

- [ ] **Step 1: Write failing tests**
  - `settings_roundtrip`: save then load in a temp dir → equal.
  - `settings_missing_or_corrupt_gives_default`: no file → default; file `"{not json"` → default.
  - `sanitize_clamps`: `interval_secs: 5` → `30`; `opacity_pct: 63` → nearest allowed `70`; `opacity_pct: 0` → `50`.
  - `session_roundtrip_dpapi`: save/load → same key; raw `session.bin` bytes do not contain the key string.
  - `session_debug_redacts`: `format!("{:?}", s)` does not contain the key.
  - `clear_session_removes_file`.
- [ ] **Step 2:** `cargo test settings session` → FAIL.
- [ ] **Step 3:** Implement; DPAPI via `CryptProtectData`/`CryptUnprotectData` (`windows` crate `Win32_Security_Cryptography`), freeing output buffers with `LocalFree`. Settings saved via write-to-temp-then-rename.
- [ ] **Step 4:** `cargo test` → PASS.
- [ ] **Step 5:** Commit: `feat: settings persistence and DPAPI-encrypted session storage`.

### Task 4: `UsageSource` trait and HTTP backend

**Files:**
- Create: `src/source.rs`; Test: inline

**Interfaces:**
- Consumes: `parse_usage`, `parse_orgs`, `UsageState`, `Org`, `Session`.
- Produces: `pub enum FetchError { Auth, Blocked(u16), Network(String), Format(String) }`; `pub trait UsageSource: Send { fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError>; fn fetch_usage(&mut self, org_id: &str) -> Result<UsageState, FetchError>; }`; `pub struct HttpSource` with `pub fn new(session: Session) -> HttpSource` (one `ureq::Agent`, 15 s timeout, reused); `pub fn classify(status: u16, body: &str) -> Option<FetchError>` (None = success).

- [ ] **Step 1: Write failing tests** for `classify`: 200 → `None`; 401 → `Auth`; 403 with JSON body containing `"permission_error"` or `"account_session_invalid"` → `Auth`; 403 with `Just a moment` HTML → `Blocked(403)`; 429 → `Blocked(429)`; 500 → `Network`.
- [ ] **Step 2:** `cargo test source` → FAIL.
- [ ] **Step 3:** Implement `classify` and `HttpSource` (headers: `Cookie: sessionKey=…`, `User-Agent: ClaudeUsageWidget/<ver>`, `Accept: application/json`; transport errors → `Network`; parse failures → `Format`).
- [ ] **Step 4:** `cargo test` → PASS. Then `cargo run --example spike` rewritten to use `HttpSource` (user runs it) → prints parsed session/weekly percentages.
- [ ] **Step 5:** Commit: `feat: UsageSource trait with plain HTTP backend`.

**Task 4b (only if `BACKEND=webview2`):** STOP and confirm with the user before starting. Add `WebViewSource` implementing `UsageSource` with a hidden WebView2 (persistent `webview_dir()`), performing `fetch('/api/…', {credentials:'include'})` via `ExecuteScriptAsync` and returning `{status, body}` JSON, reusing `classify`. Same tests for `classify`; manual check that numbers match claude.ai. Signin (Task 7) then keeps the WebView2 folder instead of deleting it.

### Task 5: Background fetcher

**Files:**
- Create: `src/fetcher.rs`; Test: inline with a `FakeSource`

**Interfaces:**
- Consumes: `UsageSource`, `FetchError`, `UsageState`, `Org`.
- Produces: `pub enum Command { RefreshNow, SetInterval(u32), SetOrg(String), Stop }`; `pub enum Event { Orgs(Vec<Org>, String /* chosen */), Usage(UsageState), Error(FetchError) }`; `pub struct Fetcher { tx: Sender<Command>, handle: Option<JoinHandle<()>> }` with `pub fn spawn(source: Box<dyn UsageSource>, interval_secs: u32, preferred_org: Option<String>, last_active_org: Option<String>, sink: impl Fn(Event) + Send + 'static) -> Fetcher`, `pub fn send(&self, c: Command)`, and `Drop` that sends `Stop` and joins.

Loop: on start list orgs; choose `preferred_org` if present in list, else `last_active_org` if its usage is not `NoLimits`, else first org whose usage is `Limits`, else first org; emit `Orgs`. Then fetch → emit → `recv_timeout(interval)`; a command wakes it early. After `FetchError::Auth`, emit it and stop polling (wait only for commands). After `Blocked`/`Network`, keep the normal interval (no tighter retry).

- [ ] **Step 1: Write failing tests** (fake source scripted with a queue of results, sink pushes into a `Mutex<Vec<Event>>`, interval set to 30 s so only explicit commands drive extra fetches):
  - `picks_org_with_limits`: orgs A (NoLimits, last-active), B (Limits) → `Orgs(_, "B")`.
  - `preferred_org_wins`.
  - `refresh_now_fetches_once_more`.
  - `network_error_then_recovers`: Error(Network) then, after `RefreshNow`, Usage(Limits).
  - `auth_error_stops_polling`: after Error(Auth), no further `fetch_usage` calls within 200 ms even with a 1 s interval.
  - `drop_joins_thread`: dropping `Fetcher` returns within 1 s while the source is idle; the fake records it was dropped (source freed).
- [ ] **Step 2:** `cargo test fetcher` → FAIL.
- [ ] **Step 3:** Implement with `std::sync::mpsc` and `std::thread`; the interval floor 30 s is enforced in `SetInterval` except under `#[cfg(test)]` constructor override `spawn_with_min(…, min_secs)`.
- [ ] **Step 4:** `cargo test` → PASS.
- [ ] **Step 5:** Commit: `feat: background fetch thread with commands and clean shutdown`.

### Task 6: Widget window

**Files:**
- Create: `src/widget.rs`, `src/gdi.rs` (RAII wrappers `Font`, `Brush`, `MemDc`), `src/geometry.rs`; Modify: `src/main.rs`
- Test: inline tests in `geometry.rs`

**Interfaces:**
- Consumes: `ViewModel`, `build_view`, `AppState`, `Settings`, `Fetcher`, `Event`, `Command`.
- Produces: `pub const WM_APP_EVENT: u32 = WM_APP + 1` (lParam = `Box<Event>` raw pointer); `pub fn run(settings: Settings, start: StartMode) -> ExitCode` where `pub enum StartMode { NeedSignIn, Session(Session) }`; `geometry.rs`: `pub fn clamp_to_work_areas(pos: (i32,i32), size: (i32,i32), areas: &[RECT], primary: RECT) -> (i32,i32)`.

- [ ] **Step 1: Write failing tests** in `geometry.rs`: `clamp_offscreen_position` (pos on a removed monitor → top-right of primary minus 16 px margin); `clamp_partially_visible` (pulled fully inside the nearest area); `inside_unchanged`.
- [ ] **Step 2:** `cargo test geometry` → FAIL; implement; → PASS.
- [ ] **Step 3:** Implement the window: named mutex single instance; `WS_POPUP` + `WS_EX_LAYERED|WS_EX_TOPMOST|WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE`; 230×56 logical px scaled by `GetDpiForWindow` (handle `WM_DPICHANGED`); `SetLayeredWindowAttributes` alpha from opacity; double-buffered GDI paint of `ViewModel` (stale → 50% text color); drag via `WM_NCHITTEST` returning `HTCAPTION` except over ✕; save position on `WM_EXITSIZEMOVE`; 60 s `SetTimer` that only repaints countdowns; right-click `TrackPopupMenu` with Refresh now / Refresh every ▸ / Opacity ▸ (checkmarks) / Organization ▸ (only if >1 org) / Sign out / Quit; click on "Signed out" message starts sign-in. `WM_APP_EVENT` handler reclaims the `Box<Event>`; sink drops the box if `PostMessageW` fails.
- [ ] **Step 4:** Quit path: kill timer → drop `Fetcher` (joins) → drain pending `WM_APP_EVENT` with `PeekMessageW(PM_REMOVE)` and drop their boxes → `DestroyWindow` → `PostQuitMessage(0)`; settings saved.
- [ ] **Step 5:** Manual check with a temporary `FakeSource` start mode (`--demo` flag, debug builds only): widget appears top-right, stays on top over a focused window, drags, menu changes opacity and interval and persists across restart, ✕ quits and the process disappears from Task Manager, second launch exits. Screenshot for the user.
- [ ] **Step 6:** Commit: `feat: always-on-top layered widget window with menu and persistence`.

### Task 7: WebView2 sign-in and app wiring

**Files:**
- Create: `src/signin.rs`; Modify: `src/main.rs`, `src/widget.rs`

**Interfaces:**
- Consumes: `Session`, `save_session`, `load_session`, `clear_session`, `webview_dir`, `HttpSource`, `Fetcher`.
- Produces: `pub enum SignInError { WebView2Missing, Cancelled, Failed(String) }`; `pub fn sign_in(owner: HWND) -> Result<Session, SignInError>` (runs its own window + nested message loop until done; returns after all WebView2 objects are released).

- [ ] **Step 1:** Implement `sign_in`: 480×720 window titled "Sign in to Claude" with the hint line from Global Constraints above the WebView; `CreateCoreWebView2EnvironmentWithOptions` with `webview_dir()`; navigate `https://claude.ai/login`; allow SSO pop-ups to open in the same WebView (`NewWindowRequested` → `put_NewWindow` or navigate in place); on each `NavigationCompleted`, `CookieManager.GetCookies("https://claude.ai")` and finish when `sessionKey` exists, capturing `lastActiveOrg` too. Closing the window → `Cancelled`. Missing runtime (`GetAvailableCoreWebView2BrowserVersionString` fails) → `WebView2Missing`.
- [ ] **Step 2:** Teardown: `Close()` controller, release all COM pointers, destroy window, then best-effort delete `webview_dir()` (retry for up to 2 s while files are locked; skip deletion if `BACKEND=webview2`).
- [ ] **Step 3:** Wire `main`: init COM (STA) and DPI; `load_session` → `StartMode::Session` else `NeedSignIn`; `NeedSignIn` → widget shows "Signing in…" and opens sign-in; success → `save_session`, start `Fetcher` with `HttpSource`; `WebView2Missing` → message box with `https://go.microsoft.com/fwlink/p/?LinkId=2124703` then exit; `Cancelled` → "Signed out — click to sign in". `Event::Error(Auth)` → `clear_session`, drop fetcher, show signed-out. Sign out menu → same plus delete WebView2 dir.
- [ ] **Step 4:** Manual end-to-end with the user: first run signs in via email code (and Google if available) → numbers match claude.ai Settings → Usage within one refresh; restart skips sign-in; Sign out returns to signed-out state; Task Manager shows no `msedgewebview2.exe` children after sign-in completes.
- [ ] **Step 5:** Commit: `feat: WebView2 sign-in flow and end-to-end wiring`.

### Task 8: Leak check, README, release, publish

**Files:**
- Create: `README.md`, `scripts/leakcheck.ps1`

- [ ] **Step 1:** `scripts/leakcheck.ps1 -Minutes 180`: samples the process's `PrivateMemorySize64`, `HandleCount`, and GDI objects (`GetGuiResources` via Add-Type) every minute to CSV and prints first/last/max. Run with interval 30 s. Expected: after the first 10 minutes, private bytes vary < 1 MB and handle/GDI counts are constant.
- [ ] **Step 2:** `cargo build --release`; confirm exe size ≤ 5 MB and it runs from a fresh folder on the Desktop with no other files.
- [ ] **Step 3:** README: what it is, screenshot, download/run, SmartScreen "More info → Run anyway", menu reference, data locations and how to remove them, **terms risk disclosure** (unofficial; uses claude.ai's internal endpoint with your session; may break or conflict with Anthropic's consumer terms; use at your own risk), tested sign-in methods vs untested (Apple, SSO), build-from-source steps.
- [ ] **Step 4:** Commit: `docs: README and leak check script`.
- [ ] **Step 5:** Ask the user public or private, then `gh repo create claude-usage-widget --<visibility> --source . --push`; report the URL.
