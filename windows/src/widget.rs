//! The always-on-top widget window: painting, dragging, menu, and Win32 event handling.
//!
//! App logic (state, fetching, menu contents, settings) lives in `usage_core::Controller`;
//! this file only renders it with GDI and turns window messages into controller calls.

use crate::dpapi_store::DpapiStore;
use crate::gdi::{rgb, Brush, Font, MemDc, Selected};
use crate::paths::app_dir;
use std::process::ExitCode;
use std::sync::Arc;
use usage_core::controller::{Controller, Effect};
use usage_core::fetcher::Event;
use usage_core::geometry::{clamp_to_work_areas, default_position, Rect};
use usage_core::layout::{self, content_height, level_color, CLOSE, COL_BAR, COL_LABEL, COL_PCT, COL_REMAIN, NOTE_H, PAD, ROW_H, W};
use usage_core::menu::{build_menu, menu_action, Action, MenuEntry, MenuItem, MenuSnapshot};
use usage_core::session::Session;
use usage_core::settings::Settings;
use usage_core::view::{AppState, Status, ViewModel};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Posted by the fetch thread; lParam is a `Box<(u64, Event)>` (generation, event).
pub const WM_APP_EVENT: u32 = WM_APP + 1;
/// Posted to open the sign-in window once the message loop is running.
const WM_APP_SIGNIN: u32 = WM_APP + 2;
/// Repaints once a minute so the countdowns stay current between fetches.
const TIMER_COUNTDOWN: usize = 1;

fn color(c: layout::Rgb) -> COLORREF {
    rgb(c.0, c.1, c.2)
}

pub enum StartMode {
    NeedSignIn,
    Session(Session),
    #[cfg(debug_assertions)]
    Demo,
}

/// Performs sign-in (shows the WebView2 window) and returns a session, or None if
/// cancelled or failed. Supplied by `app.rs`.
pub type SignIn = fn(HWND) -> Option<Session>;

struct App {
    hwnd: HWND,
    ctl: Controller,
    sign_in: SignIn,
    dpi: u32,
    font: Font,
    font_bold: Font,
}

impl App {
    /// Scale a logical (96-DPI) length to device pixels for this window's monitor.
    fn s(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    fn rebuild_fonts(&mut self) {
        self.font = Font::segoe(self.s(12), false);
        self.font_bold = Font::segoe(self.s(12), true);
    }

    fn view(&self) -> ViewModel {
        self.ctl.view(chrono::Utc::now())
    }

    fn logical_height(&self) -> i32 {
        content_height(&self.view())
    }

    /// Carry out what the controller asked for.
    fn run_effects(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Relayout => self.resize_to_content(),
                Effect::Opacity(pct) => self.apply_opacity(pct),
                Effect::SignInCleanup => crate::app::signin_cleanup(),
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

    fn apply_opacity(&self, pct: u8) {
        let alpha = (pct as u32 * 255 / 100) as u8;
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
        if self.ctl.quitting {
            return false;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = KillTimer(Some(self.hwnd), TIMER_COUNTDOWN);
        }
        self.ctl.prepare_quit(); // stops the fetch thread (without waiting), saves settings
        drain_events(self.hwnd);
        true
    }

    fn paint(&self, hdc: HDC) {
        let (w, h) = (self.s(W), self.s(self.logical_height()));
        // Draw into an off-screen bitmap, then copy it in one go (no flicker).
        let mem = MemDc::new(hdc, w, h);
        let dc = mem.dc;
        let (text_c, dim_c) = (color(layout::TEXT), color(layout::DIM));
        unsafe {
            let bg = Brush::solid(color(layout::BG));
            FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg.0);
            SetBkMode(dc, TRANSPARENT);
            let _sel = Selected::font(dc, &self.font);

            // Close button.
            SetTextColor(dc, dim_c);
            draw_text(dc, "✕", self.close_rect(), DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            match self.view() {
                ViewModel::Message(m) => {
                    SetTextColor(dc, text_c);
                    let rc = RECT { left: self.s(PAD), top: 0, right: w - self.s(CLOSE + 8), bottom: h };
                    draw_text(dc, &m, rc, DT_LEFT | DT_VCENTER | DT_WORDBREAK | DT_END_ELLIPSIS);
                }
                ViewModel::Rows { rows, status, note } => {
                    let stale = status == Status::Stale;
                    let text = if stale { dim_c } else { text_c };
                    let track = Brush::solid(color(layout::TRACK));
                    for (i, row) in rows.iter().enumerate() {
                        let top = self.s(PAD + ROW_H * i as i32);
                        let bottom = top + self.s(ROW_H);
                        let cell = |l: i32, r: i32| RECT { left: self.s(l), top, right: self.s(r), bottom };

                        SetTextColor(dc, text);
                        {
                            let _b = Selected::font(dc, &self.font_bold);
                            draw_text(dc, &row.label, cell(COL_LABEL.0, COL_LABEL.1), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                        }
                        // Bar: a 6 px track, filled up to the usage fraction.
                        let mid = (top + bottom) / 2;
                        let bar = RECT { left: self.s(COL_BAR.0), top: mid - self.s(3), right: self.s(COL_BAR.1), bottom: mid + self.s(3) };
                        FillRect(dc, &bar, track.0);
                        let filled = RECT {
                            right: bar.left + ((bar.right - bar.left) as f64 * row.fill).round() as i32,
                            ..bar
                        };
                        let fill = Brush::solid(color(level_color(row.level, stale)));
                        FillRect(dc, &filled, fill.0);

                        draw_text(dc, &row.pct_text, cell(COL_PCT.0, COL_PCT.1), DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
                        SetTextColor(dc, dim_c);
                        draw_text(dc, &row.remaining, cell(COL_REMAIN.0, COL_REMAIN.1), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    }
                    if let Some(n) = note {
                        SetTextColor(dc, dim_c);
                        let top = self.s(PAD + ROW_H * rows.len() as i32);
                        let rc = RECT { left: self.s(PAD), top, right: w - self.s(PAD), bottom: top + self.s(NOTE_H) };
                        draw_text(dc, &n, rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    }
                }
            }
            let _ = BitBlt(hdc, 0, 0, w, h, Some(dc), 0, 0, SRCCOPY);
        }
    }

    /// Build the native popup menu from the shared menu model. Caller tracks and destroys it.
    fn build_menu(&self) -> Option<(HMENU, MenuSnapshot)> {
        fn add(m: HMENU, it: &MenuItem) {
            let wide: Vec<u16> = it.label.encode_utf16().chain([0]).collect();
            let mut flags = MF_STRING;
            if it.checked {
                flags |= MF_CHECKED;
            }
            if !it.enabled {
                flags |= MF_GRAYED;
            }
            unsafe {
                let _ = AppendMenuW(m, flags, it.id, PCWSTR(wide.as_ptr()));
            }
        }
        let (entries, snap) = build_menu(&self.ctl);
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            for entry in &entries {
                match entry {
                    MenuEntry::Item(it) => add(menu, it),
                    MenuEntry::Separator => {
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                    }
                    MenuEntry::Submenu(title, items) => {
                        let Ok(sub) = CreatePopupMenu() else { continue };
                        for it in items {
                            add(sub, it);
                        }
                        let wide: Vec<u16> = title.encode_utf16().chain([0]).collect();
                        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(wide.as_ptr()));
                    }
                }
            }
            Some((menu, snap))
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
                let fx = a.ctl.apply(action);
                a.run_effects(fx);
            }
        }
    }
}

fn begin_sign_in(hwnd: HWND) {
    let sign_in = {
        let Some(a) = (unsafe { app(hwnd) }) else { return };
        if matches!(a.ctl.state, AppState::SigningIn) || a.ctl.quitting {
            return; // already open (nested message loop)
        }
        let fx = a.ctl.set_state(AppState::SigningIn);
        a.run_effects(fx);
        a.sign_in
    };
    unsafe {
        let _ = UpdateWindow(hwnd);
    }
    let result = sign_in(hwnd);
    let Some(a) = (unsafe { app(hwnd) }) else { return };
    let fx = match result {
        Some(session) => a.ctl.signed_in(session),
        None => a.ctl.set_state(AppState::SignedOut),
    };
    a.run_effects(fx);
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

/// The `App` stored in the window's user data, if the window is still alive.
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
                let fx = a.ctl.handle_posted(gen, ev);
                a.run_effects(fx);
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
            // Report "caption" everywhere except the close button (and the whole widget
            // when signed out, where a click means "sign in"), so dragging moves the window.
            WM_NCHITTEST => {
                let (x, y) = lparam_point(lp);
                let mut pt = POINT { x, y };
                let _ = ScreenToClient(hwnd, &mut pt);
                let c = a.close_rect();
                let on_close = pt.x >= c.left && pt.x < c.right && pt.y >= c.top && pt.y < c.bottom;
                let clickable = matches!(a.ctl.state, AppState::SignedOut);
                LRESULT(if on_close || clickable { HTCLIENT } else { HTCAPTION } as isize)
            }
            WM_LBUTTONUP => {
                let (x, y) = lparam_point(lp);
                let c = a.close_rect();
                let on_close = x >= c.left && x < c.right && y >= c.top && y < c.bottom;
                let signed_out = matches!(a.ctl.state, AppState::SignedOut);
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
            WM_NCLBUTTONDBLCLK => LRESULT(0), // no maximize on double-click
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
                a.ctl.settings.pos = Some((rc.left, rc.top));
                a.ctl.save_settings();
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
                a.ctl.save_settings();
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

        // Layered (for opacity), topmost, no taskbar button, never takes focus.
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

        // The controller is created after the window exists: its sink posts to it.
        let raw_hwnd = hwnd.0 as isize;
        let ctl = Controller::new(
            app_dir(),
            settings,
            Box::new(DpapiStore { dir: app_dir() }),
            Arc::new(move |gen, ev| post_event(raw_hwnd, gen, ev)),
        );
        let dpi = GetDpiForWindow(hwnd).max(96);
        let mut a = Box::new(App {
            hwnd,
            ctl,
            sign_in,
            dpi,
            font: Font::segoe(12, false),
            font_bold: Font::segoe(12, true),
        });
        a.rebuild_fonts();
        let pref = DWMWCP_ROUNDSMALL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );
        a.apply_opacity(a.ctl.settings.opacity_pct);

        let size = (a.s(W), a.s(a.logical_height()));
        let (areas, primary) = work_areas();
        let (x, y) = match a.ctl.settings.pos {
            Some(p) => clamp_to_work_areas(p, size, &areas, primary),
            None => default_position(size, primary),
        };
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, size.0, size.1, SWP_NOACTIVATE);
        let raw = Box::into_raw(a);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
        let a = &mut *raw;

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), TIMER_COUNTDOWN, 60_000, None);

        let fx = match start {
            StartMode::Session(s) => a.ctl.start_with_session(s),
            StartMode::NeedSignIn => {
                // Open sign-in once the message loop is running.
                let _ = PostMessageW(Some(hwnd), WM_APP_SIGNIN, WPARAM(0), LPARAM(0));
                a.ctl.set_state(AppState::SignedOut)
            }
            #[cfg(debug_assertions)]
            StartMode::Demo => {
                let mut fx = a.ctl.set_state(AppState::Loading);
                fx.extend(a.ctl.start(Box::new(usage_core::demo::DemoSource::default()), None));
                fx
            }
        };
        a.run_effects(fx);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnregisterClassW(class, Some(instance));
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
