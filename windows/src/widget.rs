//! The always-on-top widget window: painting, dragging, menu, and event handling.

use usage_core::fetcher::{Command, Event, Fetcher};
use usage_core::format::Level;
use crate::gdi::{rgb, Brush, Font, MemDc, Selected};
use usage_core::geometry::{clamp_to_work_areas, default_position, Rect};
use crate::dpapi_store::DpapiStore;
use usage_core::session::{Session, SessionStore};
use crate::paths::app_dir;
use usage_core::settings::{Settings, INTERVAL_CHOICES, OPACITY_CHOICES};
use usage_core::source::{FetchError, HttpSource, UsageSource};
use usage_core::usage::{Org, UsageState};
use usage_core::view::{build_view, AppState, Status, ViewModel};
use std::process::ExitCode;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_APP_EVENT: u32 = WM_APP + 1;
const WM_APP_SIGNIN: u32 = WM_APP + 2;
const TIMER_COUNTDOWN: usize = 1;

// Logical (96-DPI) layout.
const W: i32 = 252;
const PAD: i32 = 8;
const ROW_H: i32 = 20;
const NOTE_H: i32 = 16;
const CLOSE: i32 = 14;
// Row columns as (left, right) in logical px.
const COL_LABEL: (i32, i32) = (PAD, 74);
const COL_BAR: (i32, i32) = (76, 124);
const COL_PCT: (i32, i32) = (126, 160);
const COL_REMAIN: (i32, i32) = (166, W - CLOSE - 6);

const BG: COLORREF = rgb(0x1F, 0x1F, 0x1E);
const TEXT: COLORREF = rgb(0xEC, 0xEC, 0xEA);
const DIM: COLORREF = rgb(0x8A, 0x8A, 0x85);
const TRACK: COLORREF = rgb(0x3A, 0x3A, 0x38);
const GREEN: COLORREF = rgb(0x5C, 0xB8, 0x5C);
const AMBER: COLORREF = rgb(0xE0, 0xA0, 0x30);
const RED: COLORREF = rgb(0xE0, 0x55, 0x45);

// Menu command ids.
const ID_REFRESH: usize = 100;
const ID_INTERVAL: usize = 110;
const ID_OPACITY: usize = 120;
const ID_ORG: usize = 200;
const ID_SIGN_OUT: usize = 300;
const ID_QUIT: usize = 301;

const ID_SIGN_IN: usize = 302;

/// What a menu click means, decided from the menu exactly as it was shown.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    None,
    Refresh,
    Interval(u32),
    Opacity(u8),
    Org(String),
    SignOut,
    SignIn,
    Quit,
}

/// The org uuids listed in the menu, in menu order.
struct MenuSnapshot {
    orgs: Vec<String>,
}

fn menu_action(id: usize, snap: &MenuSnapshot) -> Action {
    let in_range = |base: usize, len: usize| (base..base + len).contains(&id);
    match id {
        ID_REFRESH => Action::Refresh,
        ID_SIGN_OUT => Action::SignOut,
        ID_SIGN_IN => Action::SignIn,
        ID_QUIT => Action::Quit,
        _ if in_range(ID_INTERVAL, INTERVAL_CHOICES.len()) => Action::Interval(INTERVAL_CHOICES[id - ID_INTERVAL]),
        _ if in_range(ID_OPACITY, OPACITY_CHOICES.len()) => Action::Opacity(OPACITY_CHOICES[id - ID_OPACITY]),
        _ if in_range(ID_ORG, snap.orgs.len()) => Action::Org(snap.orgs[id - ID_ORG].clone()),
        _ => Action::None,
    }
}

pub enum StartMode {
    NeedSignIn,
    Session(Session),
    #[cfg(debug_assertions)]
    Demo,
}

/// Hook used by Task 7: performs sign-in and returns a session, or None if cancelled.
pub type SignIn = fn(HWND) -> Option<Session>;

struct App {
    hwnd: HWND,
    /// Folder for settings.json / session.bin (a temp dir in tests).
    dir: std::path::PathBuf,
    /// Bumped whenever the fetcher is replaced or stopped; events from older
    /// fetchers are ignored.
    gen: u64,
    settings: Settings,
    state: AppState,
    orgs: Vec<Org>,
    current_org: Option<String>,
    fetcher: Option<Fetcher>,
    session: Option<Session>,
    sign_in: SignIn,
    dpi: u32,
    font: Font,
    font_bold: Font,
    quitting: bool,
}

impl App {
    fn s(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    fn rebuild_fonts(&mut self) {
        self.font = Font::segoe(self.s(12), false);
        self.font_bold = Font::segoe(self.s(12), true);
    }

    fn view(&self) -> ViewModel {
        build_view(&self.state, chrono::Utc::now())
    }

    /// Logical height for the current view.
    fn logical_height(&self) -> i32 {
        match self.view() {
            ViewModel::Message(_) => PAD * 2 + ROW_H * 2,
            ViewModel::Rows { rows, note, .. } => {
                PAD * 2 + ROW_H * rows.len() as i32 + if note.is_some() { NOTE_H } else { 0 }
            }
        }
    }

    fn resize_to_content(&self) {
        unsafe {
            let mut rc = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut rc);
            let (w, h) = (self.s(W), self.s(self.logical_height()));
            if rc.right - rc.left != w || rc.bottom - rc.top != h {
                let _ = SetWindowPos(self.hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn set_state(&mut self, s: AppState) {
        self.state = s;
        self.resize_to_content();
    }

    fn start_fetcher(&mut self, source: Box<dyn UsageSource>, last_active: Option<String>) {
        self.fetcher = None; // joins any previous thread first
        self.gen += 1;
        let gen = self.gen;
        let hwnd = self.hwnd.0 as isize;
        self.fetcher = Some(Fetcher::spawn(
            source,
            self.settings.interval_secs,
            self.settings.org_id.clone(),
            last_active,
            move |ev| post_event(hwnd, gen, ev),
        ));
    }

    fn start_with_session(&mut self, session: Session) {
        let last = session.last_active_org.clone();
        self.session = Some(session.clone());
        self.set_state(AppState::Loading);
        self.start_fetcher(Box::new(HttpSource::new(session)), last);
    }


    fn signed_out(&mut self) {
        self.fetcher = None;
        self.gen += 1; // anything still queued from the old fetcher is now stale
        self.session = None;
        self.orgs.clear();
        self.current_org = None;
        DpapiStore { dir: self.dir.clone() }.clear();
        self.set_state(AppState::SignedOut);
    }

    /// Apply an event posted by fetcher generation `gen`.
    fn handle_posted(&mut self, gen: u64, ev: Event) {
        if gen == self.gen && !self.quitting {
            self.on_event(ev);
        }
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Orgs(orgs, chosen) => {
                self.orgs = orgs;
                self.current_org = Some(chosen);
            }
            Event::Usage(UsageState::Limits(limits)) => self.set_state(AppState::Data { limits, stale: false }),
            Event::Usage(UsageState::NoLimits) => self.set_state(AppState::NoLimits),
            Event::Error(FetchError::Auth) => self.signed_out(),
            Event::Error(FetchError::Format(_)) => self.set_state(AppState::FormatChanged),
            Event::Error(FetchError::Network(_) | FetchError::Blocked(_)) => {
                let next = match std::mem::replace(&mut self.state, AppState::Loading) {
                    AppState::Data { limits, .. } => AppState::Data { limits, stale: true },
                    _ => AppState::Offline,
                };
                self.set_state(next);
            }
        }
    }

    fn save_settings(&self) {
        let _ = self.settings.save(&self.dir);
    }

    fn apply_opacity(&self) {
        let alpha = (self.settings.opacity_pct as u32 * 255 / 100) as u8;
        unsafe {
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    fn close_rect(&self) -> RECT {
        let (w, c, p) = (self.s(W), self.s(CLOSE), self.s(4));
        RECT { left: w - c - p, top: p, right: w - p, bottom: p + c }
    }

    /// Release everything; the caller then destroys the window (which frees `App`).
    /// Returns false if already quitting.
    fn prepare_quit(&mut self) -> bool {
        if self.quitting {
            return false;
        }
        self.quitting = true;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = KillTimer(Some(self.hwnd), TIMER_COUNTDOWN);
        }
        self.fetcher = None; // signals Stop and joins the fetch thread
        drain_events(self.hwnd);
        self.save_settings();
        true
    }

    fn paint(&self, hdc: HDC) {
        let (w, h) = (self.s(W), self.s(self.logical_height()));
        let mem = MemDc::new(hdc, w, h);
        let dc = mem.dc;
        unsafe {
            let bg = Brush::solid(BG);
            FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg.0);
            SetBkMode(dc, TRANSPARENT);
            let _sel = Selected::font(dc, &self.font);

            // Close button.
            SetTextColor(dc, DIM);
            draw_text(dc, "✕", self.close_rect(), DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            match self.view() {
                ViewModel::Message(m) => {
                    SetTextColor(dc, TEXT);
                    let rc = RECT { left: self.s(PAD), top: 0, right: w - self.s(CLOSE + 8), bottom: h };
                    draw_text(dc, &m, rc, DT_LEFT | DT_VCENTER | DT_WORDBREAK | DT_END_ELLIPSIS);
                }
                ViewModel::Rows { rows, status, note } => {
                    let text = if status == Status::Stale { DIM } else { TEXT };
                    let track = Brush::solid(TRACK);
                    for (i, row) in rows.iter().enumerate() {
                        let top = self.s(PAD + ROW_H * i as i32);
                        let bottom = top + self.s(ROW_H);
                        let cell = |l: i32, r: i32| RECT { left: self.s(l), top, right: self.s(r), bottom };

                        SetTextColor(dc, text);
                        {
                            let _b = Selected::font(dc, &self.font_bold);
                            draw_text(dc, &row.label, cell(COL_LABEL.0, COL_LABEL.1), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                        }
                        // Bar.
                        let mid = (top + bottom) / 2;
                        let bar = RECT { left: self.s(COL_BAR.0), top: mid - self.s(3), right: self.s(COL_BAR.1), bottom: mid + self.s(3) };
                        FillRect(dc, &bar, track.0);
                        let filled = RECT {
                            right: bar.left + ((bar.right - bar.left) as f64 * row.fill).round() as i32,
                            ..bar
                        };
                        let color = if status == Status::Stale {
                            DIM
                        } else {
                            match row.level {
                                Level::Green => GREEN,
                                Level::Amber => AMBER,
                                Level::Red => RED,
                            }
                        };
                        let fill = Brush::solid(color);
                        FillRect(dc, &filled, fill.0);

                        draw_text(dc, &row.pct_text, cell(COL_PCT.0, COL_PCT.1), DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
                        SetTextColor(dc, DIM);
                        draw_text(dc, &row.remaining, cell(COL_REMAIN.0, COL_REMAIN.1), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    }
                    if let Some(n) = note {
                        SetTextColor(dc, DIM);
                        let top = self.s(PAD + ROW_H * rows.len() as i32);
                        let rc = RECT { left: self.s(PAD), top, right: w - self.s(PAD), bottom: top + self.s(NOTE_H) };
                        draw_text(dc, &n, rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    }
                }
            }
            let _ = BitBlt(hdc, 0, 0, w, h, Some(dc), 0, 0, SRCCOPY);
        }
    }

    /// Build the popup menu for the current state. Caller tracks and destroys it.
    fn build_menu(&self) -> Option<(HMENU, MenuSnapshot)> {
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            let add = |m: HMENU, id: usize, text: &str, checked: bool, enabled: bool| {
                let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
                let mut flags = MF_STRING;
                if checked {
                    flags |= MF_CHECKED;
                }
                if !enabled {
                    flags |= MF_GRAYED;
                }
                let _ = AppendMenuW(m, flags, id, PCWSTR(wide.as_ptr()));
            };
            let sub = |parent: HMENU, child: HMENU, text: &str| {
                let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
                let _ = AppendMenuW(parent, MF_POPUP, child.0 as usize, PCWSTR(wide.as_ptr()));
            };
            let signed_in = self.fetcher.is_some();
            let mut snap = MenuSnapshot { orgs: Vec::new() };

            add(menu, ID_REFRESH, "Refresh now", false, signed_in);
            if let Ok(m) = CreatePopupMenu() {
                for (i, s) in INTERVAL_CHOICES.iter().enumerate() {
                    let label = if *s <= 60 { format!("{s} s") } else { format!("{} min", s / 60) };
                    add(m, ID_INTERVAL + i, &label, self.settings.interval_secs == *s, true);
                }
                sub(menu, m, "Refresh every");
            }
            if let Ok(m) = CreatePopupMenu() {
                for (i, o) in OPACITY_CHOICES.iter().enumerate() {
                    add(m, ID_OPACITY + i, &format!("{o}%"), self.settings.opacity_pct == *o, true);
                }
                sub(menu, m, "Opacity");
            }
            if self.orgs.len() > 1 {
                if let Ok(m) = CreatePopupMenu() {
                    for (i, o) in self.orgs.iter().enumerate() {
                        add(m, ID_ORG + i, &o.name, self.current_org.as_deref() == Some(&o.uuid), true);
                        snap.orgs.push(o.uuid.clone());
                    }
                    sub(menu, m, "Organization");
                }
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            if signed_in || self.session.is_some() {
                add(menu, ID_SIGN_OUT, "Sign out", false, true);
            } else {
                add(menu, ID_SIGN_IN, "Sign in", false, true);
            }
            add(menu, ID_QUIT, "Quit", false, true);
            Some((menu, snap))
        }
    }

    /// Apply a non-modal menu action. Sign-in and Quit are handled by the caller.
    fn apply(&mut self, action: Action) {
        match action {
            Action::Refresh => {
                if let Some(f) = &self.fetcher {
                    f.send(Command::RefreshNow);
                }
            }
            Action::Interval(secs) => {
                self.settings.interval_secs = secs;
                if let Some(f) = &self.fetcher {
                    f.send(Command::SetInterval(secs));
                }
                self.save_settings();
            }
            Action::Opacity(pct) => {
                self.settings.opacity_pct = pct;
                self.apply_opacity();
                self.save_settings();
            }
            Action::Org(id) => {
                self.settings.org_id = Some(id.clone());
                self.current_org = Some(id.clone());
                self.save_settings();
                if self.fetcher.is_some() {
                    self.set_state(AppState::Loading);
                }
                if let Some(f) = &self.fetcher {
                    f.send(Command::SetOrg(id));
                }
            }
            Action::SignOut => {
                self.signed_out();
                crate::app::signin_cleanup();
            }
            Action::None | Action::SignIn | Action::Quit => {}
        }
    }
}

// Modal operations. These take the HWND, not `&mut App`: the menu and the sign-in
// window run nested message loops that re-enter `wndproc`, so no reference to `App`
// may be held across them. `App` is looked up again afterwards.

fn show_menu(hwnd: HWND, x: i32, y: i32) {
    let Some((menu, snap)) = (unsafe { app(hwnd) }).and_then(|a| a.build_menu()) else { return };
    let cmd = unsafe {
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, x, y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // also destroys submenus
        cmd.0 as usize
    };
    match menu_action(cmd, &snap) {
        Action::SignIn => begin_sign_in(hwnd),
        Action::Quit => quit(hwnd),
        action => {
            if let Some(a) = unsafe { app(hwnd) } {
                a.apply(action);
            }
        }
    }
}

fn begin_sign_in(hwnd: HWND) {
    let sign_in = {
        let Some(a) = (unsafe { app(hwnd) }) else { return };
        if matches!(a.state, AppState::SigningIn) || a.quitting {
            return; // already open (nested message loop)
        }
        a.set_state(AppState::SigningIn);
        a.sign_in
    };
    unsafe {
        let _ = UpdateWindow(hwnd);
    }
    let result = sign_in(hwnd);
    let Some(a) = (unsafe { app(hwnd) }) else { return };
    match result {
        Some(session) => a.start_with_session(session),
        None => a.set_state(AppState::SignedOut),
    }
}

fn quit(hwnd: HWND) {
    let proceed = unsafe { app(hwnd) }.map(|a| a.prepare_quit()).unwrap_or(false);
    if proceed {
        unsafe {
            let _ = DestroyWindow(hwnd); // frees App in WM_NCDESTROY
        }
    }
}

fn draw_text(dc: HDC, s: &str, mut rc: RECT, fmt: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        DrawTextW(dc, &mut wide, &mut rc, fmt | DT_NOPREFIX);
    }
}

/// Called on the fetch thread. Ownership of the boxed event passes to the window;
/// if the post fails (window gone), the box is reclaimed and dropped here.
fn post_event(hwnd: isize, gen: u64, ev: Event) {
    let raw = Box::into_raw(Box::new((gen, ev)));
    let ok = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_EVENT, WPARAM(0), LPARAM(raw as isize)) };
    if ok.is_err() {
        drop(unsafe { Box::from_raw(raw) });
    }
}

/// Free any events still queued for this window.
fn drain_events(hwnd: HWND) {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, Some(hwnd), WM_APP_EVENT, WM_APP_EVENT, PM_REMOVE).as_bool() {
            drop(Box::from_raw(msg.lParam.0 as *mut (u64, Event)));
        }
    }
}

/// Work areas of all monitors, plus the primary one, as core `Rect`s.
fn work_areas() -> (Vec<Rect>, Rect) {
    unsafe extern "system" fn cb(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let v = &mut *(data.0 as *mut Vec<RECT>);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            v.push(mi.rcWork);
        }
        TRUE
    }
    let mut areas: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut areas as *mut _ as isize));
        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(primary, &mut mi);
        (areas.iter().map(to_rect).collect(), to_rect(&mi.rcWork))
    }
}

fn to_rect(r: &RECT) -> Rect {
    Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

unsafe fn app<'a>(hwnd: HWND) -> Option<&'a mut App> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    p.as_mut()
}

fn lparam_point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(a) = app(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_APP_EVENT => {
                let (gen, ev) = *Box::from_raw(lp.0 as *mut (u64, Event));
                a.handle_posted(gen, ev);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                a.paint(hdc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_NCHITTEST => {
                let (x, y) = lparam_point(lp);
                let mut pt = POINT { x, y };
                let _ = ScreenToClient(hwnd, &mut pt);
                let c = a.close_rect();
                let on_close = pt.x >= c.left && pt.x < c.right && pt.y >= c.top && pt.y < c.bottom;
                let clickable = matches!(a.state, AppState::SignedOut);
                LRESULT(if on_close || clickable { HTCLIENT } else { HTCAPTION } as isize)
            }
            WM_LBUTTONUP => {
                let (x, y) = lparam_point(lp);
                let c = a.close_rect();
                let on_close = x >= c.left && x < c.right && y >= c.top && y < c.bottom;
                let signed_out = matches!(a.state, AppState::SignedOut);
                if on_close {
                    quit(hwnd);
                } else if signed_out {
                    begin_sign_in(hwnd);
                }
                LRESULT(0)
            }
            WM_APP_SIGNIN => {
                begin_sign_in(hwnd);
                LRESULT(0)
            }
            WM_NCLBUTTONDBLCLK => LRESULT(0),
            WM_NCRBUTTONUP | WM_RBUTTONUP => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                show_menu(hwnd, pt.x, pt.y);
                LRESULT(0)
            }
            WM_CONTEXTMENU => LRESULT(0),
            WM_EXITSIZEMOVE => {
                let mut rc = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rc);
                a.settings.pos = Some((rc.left, rc.top));
                a.save_settings();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_COUNTDOWN => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                a.dpi = (wp.0 & 0xFFFF) as u32;
                a.rebuild_fonts();
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    a.s(W),
                    a.s(a.logical_height()),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                LRESULT(0)
            }
            WM_CLOSE => {
                quit(hwnd);
                LRESULT(0)
            }
            WM_ENDSESSION => {
                a.save_settings();
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(a as *mut App));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub fn run(settings: Settings, start: StartMode, sign_in: SignIn) -> ExitCode {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return ExitCode::FAILURE };
        let instance: HINSTANCE = module.into();
        let class = w!("ClaudeUsageWidget");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(Some(instance), PCWSTR(1 as _)).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return ExitCode::FAILURE;
        }

        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Claude Usage"),
            WS_POPUP,
            0,
            0,
            W,
            PAD * 2 + ROW_H * 2,
            None,
            None,
            Some(instance),
            None,
        ) else {
            return ExitCode::FAILURE;
        };

        let dpi = GetDpiForWindow(hwnd).max(96);
        let mut a = Box::new(App {
            hwnd,
            dir: app_dir(),
            gen: 0,
            settings,
            state: AppState::Loading,
            orgs: Vec::new(),
            current_org: None,
            fetcher: None,
            session: None,
            sign_in,
            dpi,
            font: Font::segoe(12, false),
            font_bold: Font::segoe(12, true),
            quitting: false,
        });
        a.rebuild_fonts();
        let pref = DWMWCP_ROUNDSMALL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );
        a.apply_opacity();

        let size = (a.s(W), a.s(a.logical_height()));
        let (areas, primary) = work_areas();
        let (x, y) = match a.settings.pos {
            Some(p) => clamp_to_work_areas(p, size, &areas, primary),
            None => default_position(size, primary),
        };
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, size.0, size.1, SWP_NOACTIVATE);
        let raw = Box::into_raw(a);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
        let a = &mut *raw;

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), TIMER_COUNTDOWN, 60_000, None);

        match start {
            StartMode::Session(s) => a.start_with_session(s),
            StartMode::NeedSignIn => {
                // Open sign-in once the message loop is running.
                a.set_state(AppState::SignedOut);
                let _ = PostMessageW(Some(hwnd), WM_APP_SIGNIN, WPARAM(0), LPARAM(0));
            }
            #[cfg(debug_assertions)]
            StartMode::Demo => {
                a.set_state(AppState::Loading);
                a.start_fetcher(Box::new(demo::DemoSource::default()), None);
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnregisterClassW(class, Some(instance));
        ExitCode::SUCCESS
    }
}

#[cfg(debug_assertions)]
mod demo {
    use usage_core::source::{FetchError, UsageSource};
    use usage_core::usage::{Limits, Org, UsageState, Window};
    use chrono::{Duration, Utc};

    /// Fake data for `--demo` (debug builds only): numbers creep up on each refresh.
    #[derive(Default)]
    pub struct DemoSource {
        n: u32,
    }

    impl UsageSource for DemoSource {
        fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError> {
            Ok(vec![
                Org { uuid: "demo-1".into(), name: "Personal (demo)".into() },
                Org { uuid: "demo-2".into(), name: "Team (demo)".into() },
            ])
        }
        fn fetch_usage(&mut self, _org: &str) -> Result<UsageState, FetchError> {
            self.n += 1;
            let now = Utc::now();
            Ok(UsageState::Limits(Limits {
                session: Window {
                    utilization: (60.0 + 8.0 * self.n as f64).min(120.0),
                    resets_at: Some(now + Duration::minutes(112)),
                },
                weekly: Window { utilization: 31.0, resets_at: Some(now + Duration::hours(4 * 24 + 6)) },
                extra: vec![],
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usage_core::usage::{Limits, Window};
    use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC};

    fn text_width(font: &Font, s: &str) -> i32 {
        unsafe {
            let dc = GetDC(None);
            let _sel = Selected::font(dc, font);
            let wide: Vec<u16> = s.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            drop(_sel);
            ReleaseDC(None, dc);
            size.cx
        }
    }

    #[test]
    fn columns_fit_longest_strings() {
        // Logical px at 96 DPI, same fonts the widget paints with.
        let (normal, bold) = (Font::segoe(12, false), Font::segoe(12, true));
        let width = |c: (i32, i32)| c.1 - c.0;
        for s in ["Session", "Weekly", "Opus wk", "Sonnet wk"] {
            assert!(text_width(&bold, s) <= width(COL_LABEL), "label {s:?} clipped");
        }
        for s in ["resetting…", "23h 59m", "6d 23h", "59m"] {
            assert!(text_width(&normal, s) <= width(COL_REMAIN), "countdown {s:?} clipped");
        }
        for s in ["100%", "130%"] {
            assert!(text_width(&normal, s) <= width(COL_PCT), "percent {s:?} clipped");
        }
        // Columns don't overlap each other or the close button, and fit the window.
        assert!(COL_LABEL.1 <= COL_BAR.0 && COL_BAR.1 <= COL_PCT.0 && COL_PCT.1 <= COL_REMAIN.0);
        assert!(COL_REMAIN.1 <= W - CLOSE - 4);
    }

    fn no_sign_in(_: HWND) -> Option<Session> {
        None
    }

    fn test_app(dir: &std::path::Path) -> App {
        App {
            hwnd: HWND::default(),
            dir: dir.to_path_buf(),
            gen: 1,
            settings: Settings::default(),
            state: AppState::Loading,
            orgs: Vec::new(),
            current_org: None,
            fetcher: None,
            session: None,
            sign_in: no_sign_in,
            dpi: 96,
            font: Font::segoe(12, false),
            font_bold: Font::segoe(12, true),
            quitting: false,
        }
    }

    fn limits() -> UsageState {
        let w = Window { utilization: 10.0, resets_at: None };
        UsageState::Limits(Limits { session: w.clone(), weekly: w, extra: vec![] })
    }

    #[test]
    fn late_event_after_sign_out_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = test_app(dir.path());
        let old_gen = a.gen;
        a.signed_out();
        // A fetch that was in flight when the user signed out arrives afterwards.
        a.handle_posted(old_gen, Event::Usage(limits()));
        assert_eq!(a.state, AppState::SignedOut);
        a.handle_posted(old_gen, Event::Error(FetchError::Network("x".into())));
        assert_eq!(a.state, AppState::SignedOut);
    }

    #[test]
    fn current_generation_event_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = test_app(dir.path());
        let gen = a.gen;
        a.handle_posted(gen, Event::Usage(limits()));
        assert!(matches!(a.state, AppState::Data { stale: false, .. }));
    }

    #[test]
    fn menu_action_uses_snapshot_not_live_state() {
        let snap = MenuSnapshot { orgs: vec!["org-a".into(), "org-b".into()] };
        assert_eq!(menu_action(ID_ORG + 1, &snap), Action::Org("org-b".into()));
        assert_eq!(menu_action(ID_ORG + 2, &snap), Action::None);
        assert_eq!(menu_action(ID_SIGN_OUT, &snap), Action::SignOut);
        assert_eq!(menu_action(ID_SIGN_IN, &snap), Action::SignIn);
        assert_eq!(menu_action(ID_INTERVAL + 2, &snap), Action::Interval(INTERVAL_CHOICES[2]));
        assert_eq!(menu_action(ID_OPACITY + 3, &snap), Action::Opacity(OPACITY_CHOICES[3]));
        assert_eq!(menu_action(ID_REFRESH, &snap), Action::Refresh);
        assert_eq!(menu_action(ID_QUIT, &snap), Action::Quit);
        assert_eq!(menu_action(0, &snap), Action::None);
    }
}
