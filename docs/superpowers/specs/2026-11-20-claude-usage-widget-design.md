# Claude Usage Widget — Design

Date: 2026-11-20
Status: Draft, awaiting approval

## Intent and why

Claude Code (CLI) users check how close they are to their plan limits by typing `/usage`
over and over. This widget puts those same numbers permanently on screen: a tiny,
always-on-top Windows window showing the 5-hour session usage and the weekly usage, each
with time remaining until reset. It is meant to be shared with other people, so it must
work on its own (no Claude Code install required) and must not violate Anthropic's terms.

## Goals

- Show session (5-hour) and weekly utilization with time-until-reset, matching what
  `/usage` and claude.ai → Settings → Usage show (these are account-wide numbers).
- Always on top, small, minimal; draggable; position remembered between runs.
- Auto-refresh, default every 60 s; user-adjustable interval.
- User-adjustable window opacity.
- On first run, ask the user to sign in; after that, start showing usage. Sign-in persists
  across restarts until it expires or the user signs out.
- Support every login method claude.ai supports (email code, Google, Apple, SSO).
- Single portable `.exe`, no installer, no admin rights.
- Lightweight: target exe ≤ 5 MB, steady-state memory ~5–15 MB, near-zero CPU when idle.
- No memory leaks; quitting releases all resources and exits the process cleanly.

## Non-goals

- Token/cost accounting per conversation or per project.
- Notifications, sounds, history charts (could come later; not now).
- macOS/Linux support.
- Auto-update.

## Anti-goals (failure even if it "works")

- Using Claude Code's OAuth identity/client ID, or reading/reusing the CLI's
  `~/.claude/.credentials.json` token. The user explicitly chose not to go near this.
- Asking for a username/password in our own form (claude.ai has no passwords, and
  handling credentials ourselves is unsafe).
- Memory that grows over hours/days of running.
- Covering meaningful screen space or stealing focus.

## Constraints

- Windows 10/11 x64. Relies on the Microsoft Edge WebView2 Runtime, which ships with
  Windows 11 and current Windows 10 (present on the dev machine: 157.x). If missing, show
  a message with the download link instead of crashing.
- There is no official public API for subscription usage. The data comes from the same
  internal endpoint claude.ai's own Usage page calls. It can change without notice; the
  widget must fail gracefully ("Couldn't read usage — retrying") and the parsing code
  should be isolated in one module so it's easy to fix.
- Google sometimes refuses to sign in inside embedded browsers. Google-account users can
  use the email-code login instead; the sign-in window should say so in a small hint.

## Approaches considered

1. **Embedded claude.ai sign-in (CHOSEN).** Show the real claude.ai login page in a
   WebView2 window, capture the resulting web session, then read the usage endpoint the
   claude.ai Usage page uses. Acts as the user's own browser session; does not pose as
   Claude Code. Trade-off: Google login may be refused in embedded windows; relies on an
   undocumented web endpoint.
2. **Browser OAuth flow impersonating Claude Code.** Supports every login, mirrors the CLI
   exactly, but uses Claude Code's client identity — against Anthropic's consumer terms
   for subscription tokens and easiest to get blocked. Rejected by the user.
3. **Hybrid: reuse CLI credentials when present, else option 1.** Most convenient but
   carries the same terms problem for CLI users and doubles the auth code. Rejected.

**Terms risk — accepted by the user.** Anthropic's consumer terms prohibit accessing the
services "through automated or non-human means" except via the API. Polling claude.ai with
the user's own session is arguably in that grey area. The user was told the consequences
(widget can break without notice; accounts could in principle be warned/suspended; a public
repo makes it more visible) and chose to proceed with approach 1 only (no Claude Code
status-line data source). Mitigations that are mandatory:

- Read-only; minimum refresh interval 30 s; default 60 s.
- No Cloudflare/bot-protection circumvention: no copying `cf_clearance`, no TLS
  fingerprint spoofing, no fake browser User-Agent on the plain HTTP client. Requests use
  an honest User-Agent (e.g. `ClaudeUsageWidget/<version>`).
- README states the risk plainly so every user opts in knowingly.

Language/stack: **Rust** (user's choice, for lightweight and leak-free). Python and .NET
were considered and rejected for size (30–70 MB) and memory footprint.

## Design

### Architecture

One Rust binary, Win32 via the `windows` crate. Three parts:

1. **Widget window** — native Win32 layered window (`WS_EX_LAYERED | WS_EX_TOPMOST |
   WS_EX_TOOLWINDOW` so it has no taskbar button), drawn with GDI (or Direct2D if GDI
   text looks poor). No WebView in the widget itself.
2. **Sign-in window** — created only when needed. Hosts WebView2 (`webview2-com` crate)
   pointed at `https://claude.ai/login`. When the `sessionKey` cookie appears (via the
   WebView2 cookie manager after each navigation completes), capture it plus the
   `lastActiveOrg` cookie, then destroy the WebView2 controller, environment, and window
   so the Edge helper processes exit. Steady-state memory then has no browser cost.
3. **Usage fetcher** — one reused HTTPS client (`ureq` with rustls, or WinHTTP), called
   on a timer from a single background thread; results posted to the UI thread with
   `PostMessage`. Never more than one request in flight.

### Data

- Org (designer's call): after sign-in, `GET https://claude.ai/api/organizations`. Default
  to the `lastActiveOrg` cookie's org if it has usage limits, else the first org that does.
  If the user belongs to more than one org, the right-click menu gets an
  "Organization ▸" submenu; the chosen org id is saved in settings and kept until changed.
- Numbers are per org, not account-wide.
- If the chosen org's plan returns no `five_hour`/`seven_day` data (null/absent), show
  "No usage limits on this plan" — not an error. "Usage format changed" is only for
  responses that don't parse at all.
- Usage: `GET https://claude.ai/api/organizations/{org_id}/usage` with the `sessionKey`
  cookie and browser-like headers. Expected response includes `five_hour` and
  `seven_day` objects with `utilization` (percent) and `resets_at` (ISO timestamp);
  may also include per-model weekly limits (e.g. `seven_day_opus`, `seven_day_sonnet`).
- Show the two core rows always. Show an extra weekly-per-model row only if the field is
  present and non-null (builder's call on naming).
- **Known risk — verify first:** claude.ai is behind Cloudflare. A plain HTTP client may
  get a 403 challenge even with a valid session. The first task of the plan is a spike that
  calls this endpoint from Rust with a real session (the user signs in live during the
  spike). No circumvention tricks (see Terms risk). Fallback if blocked: perform the
  fetch inside a hidden, long-lived WebView2 (`fetch()` via `ExecuteScriptAsync`), at the
  cost of ~40–80 MB memory. The user accepts this fallback. Design the fetcher behind one
  interface (`UsageSource`) so the plain-HTTP and hidden-WebView2 backends are
  interchangeable; build only the one the spike shows is needed. If the WebView2 backend is
  used, keep its user-data folder instead of deleting it after sign-in, and relax the
  memory goal to ≤ 100 MB steady state (still must stay flat over time).

### Sign-in persistence and security

- Store the `sessionKey` and org id encrypted with Windows DPAPI (`CryptProtectData`,
  current-user scope) in `%APPDATA%\ClaudeUsageWidget\session.bin`.
- Settings (opacity, interval, window position) in
  `%APPDATA%\ClaudeUsageWidget\settings.json`.
- WebView2 user-data folder in `%LOCALAPPDATA%\ClaudeUsageWidget\WebView2`, deleted after a
  successful sign-in and on sign-out (best effort; Edge may hold locks briefly).
- On HTTP 401/403-with-auth-error: clear the stored session, show "Signed out — click to
  sign in". Distinguish auth failures from Cloudflare/network errors; the latter just retry.
- Never log the session key.

### Widget UI

Approx. 230×56 px at 100% DPI, DPI-aware (per-monitor v2).

```
┌──────────────────────────────── ✕ ┐
│ Session  ███████░░░  68%   1h 52m │
│ Weekly   ███░░░░░░░  31%   4d 6h  │
└───────────────────────────────────┘
```

- Dark background, light text; bar green < 75%, amber 75–90%, red ≥ 90%.
- Drag anywhere to move; position saved; clamped to a visible monitor on startup.
- ✕ quits. Right-click menu:
  - Refresh now
  - Refresh every ▸ 30 s / 60 s (default) / 2 min / 5 min (30 s is the floor)
  - Organization ▸ (only when the user has more than one org)
  - Opacity ▸ 100% / 85% / 70% / 50% (window alpha via `SetLayeredWindowAttributes`)
  - Sign out
  - Quit
- Countdown text updates locally each minute between fetches (no extra network calls).
- States: "Signing in…", "Loading…", data, "Offline — retrying", "Signed out — click to
  sign in", "Usage format changed — check for an update". Stale data stays visible with a
  dimmed look if a refresh fails.
- Never takes focus on refresh. Single instance (named mutex); a second launch exits.

### Resource discipline

- One UI thread, one fetch thread; the fetch thread exits on quit via a channel/event, and
  the main thread joins it before exit.
- All GDI objects created once and reused, or released in the same paint call; RAII
  wrappers for HANDLE/HFONT/HBRUSH so nothing is forgotten.
- WebView2 objects fully released after sign-in; verify Edge helper processes exit.
- Quit path: stop timer → signal fetch thread → join → destroy window → `PostQuitMessage`.
- Verification: run for several hours at a 30 s interval while sampling private bytes and
  GDI/handle counts; they must stay flat.

### Build and distribution

- `cargo build --release` with `opt-level="z"`, `lto=true`, `panic="abort"`, `strip=true`,
  static CRT (`+crt-static`) so no VC++ redistributable is needed.
- Embed an app manifest (DPI awareness, common controls v6) and an icon.
- `WebView2Loader` statically linked (webview2-com supports this) so the exe is truly
  single-file.
- Toolchain to install after approval: rustup (stable, x86_64-pc-windows-msvc) and Visual
  Studio Build Tools (MSVC + Windows SDK).

### Testing scope and distribution (designer's calls)

- Sign-in is tested with the methods the user's own account has (likely email code and
  Google). Apple and SSO sign-in are untested; README says so.
- The exe is unsigned. README explains the SmartScreen prompt ("More info → Run anyway").
  Code signing is out of scope.
- When done and committed, the project is pushed to a new GitHub repo under the user's
  account (visibility to be confirmed with the user at that time).

## Left to the builder

- Exact crate choices (ureq vs WinHTTP; GDI vs Direct2D), module layout, colors and fonts.
- Exact header set needed to satisfy claude.ai.
- Naming/visibility of per-model weekly rows.
- Testing approach: unit tests for response parsing and countdown formatting; manual
  test for sign-in and the long-run memory check.
