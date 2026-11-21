//! claude.ai sign-in in a WebView2 window. Created on demand and fully torn down afterwards.
//!
//! The user signs in on the real claude.ai page with any method it offers. We poll the
//! WebView's cookie store once a second; when `sessionKey` appears we keep it (plus
//! `lastActiveOrg`), close everything, and delete the WebView2 profile folder.

use crate::session::Session;
use crate::settings::webview_dir;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, GetCookiesCompletedHandler, NewWindowRequestedEventHandler,
};
use windows::core::{w, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug)]
pub enum SignInError {
    WebView2Missing,
    Cancelled,
    Failed(String),
}

pub const HINT: &str = "Google sign-in blocked? Use 'Continue with email' with your Google address.";
const LOGIN_URL: &str = "https://claude.ai/login";
const COOKIE_URL: &str = "https://claude.ai";
const TIMER_POLL: usize = 1;
const HINT_H: i32 = 30;

#[derive(Default)]
struct State {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    result: Option<Session>,
    cancelled: bool,
    poll_in_flight: bool,
}

thread_local! {
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
}

fn state() -> Option<Rc<RefCell<State>>> {
    STATE.with(|s| s.borrow().clone())
}

/// Debug builds only: append a line to %LOCALAPPDATA%\ClaudeUsageWidget\signin-debug.log.
/// Never logs cookie values or URL query strings.
#[cfg(debug_assertions)]
fn dlog(line: &str) {
    use std::io::Write;
    let path = webview_dir().parent().unwrap().join("signin-debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {line}", chrono::Utc::now().format("%H:%M:%S%.3f"));
    }
}
#[cfg(not(debug_assertions))]
fn dlog(_line: &str) {}

fn strip_query(u: &str) -> &str {
    u.split(['?', '#']).next().unwrap_or(u)
}

fn err(e: impl std::fmt::Debug) -> SignInError {
    SignInError::Failed(format!("{e:?}"))
}

pub fn runtime_available() -> bool {
    let mut version = PWSTR::null();
    let ok = unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) }.is_ok();
    let v = take_pwstr(version);
    ok && !v.is_empty()
}

/// Runs the sign-in window with a nested message loop. The owner is disabled meanwhile.
pub fn sign_in(owner: HWND) -> Result<Session, SignInError> {
    if !runtime_available() {
        return Err(SignInError::WebView2Missing);
    }
    let st = Rc::new(RefCell::new(State::default()));
    STATE.with(|s| *s.borrow_mut() = Some(st.clone()));

    let hwnd = create_window(owner)?;
    unsafe {
        let _ = EnableWindow(owner, false);
    }
    let result = run(hwnd, &st);

    // Teardown: stop polling, close the WebView, release every COM pointer, destroy window.
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_POLL);
        let (controller, webview) = {
            let mut s = st.borrow_mut();
            (s.controller.take(), s.webview.take())
        };
        drop(webview);
        if let Some(c) = controller {
            let _ = c.Close();
        }
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
    }
    STATE.with(|s| *s.borrow_mut() = None);
    drop(st);
    cleanup_profile();
    result
}

fn run(hwnd: HWND, st: &Rc<RefCell<State>>) -> Result<Session, SignInError> {
    let folder = webview_dir();
    let _ = std::fs::create_dir_all(&folder);
    let folder_w = CoTaskMemPWSTR::from(folder.to_string_lossy().as_ref());

    // Environment.
    let env = {
        let (tx, rx) = std::sync::mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), *folder_w.as_ref().as_pcwstr(), None, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |hr, env| {
                hr?;
                let _ = tx.send(env);
                Ok(())
            }),
        )
        .map_err(err)?;
        rx.recv().map_err(err)?.ok_or_else(|| err("no environment"))?
    };

    // Controller hosted in our window.
    let controller = {
        let (tx, rx) = std::sync::mpsc::channel();
        let env2 = env.clone();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                env2.CreateCoreWebView2Controller(hwnd, &handler).map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |hr, c| {
                hr?;
                let _ = tx.send(c);
                Ok(())
            }),
        )
        .map_err(err)?;
        rx.recv().map_err(err)?.ok_or_else(|| err("no controller"))?
    };
    drop(env);

    let webview = unsafe { controller.CoreWebView2() }.map_err(err)?;
    unsafe {
        // Let SSO / Google pop-ups open as real pop-up windows (WebView2's default). They
        // share this profile, and their `window.opener` hand-back to claude.ai keeps
        // working. Loading them in place broke Google sign-in (white page).
        let handler = NewWindowRequestedEventHandler::create(Box::new(|_sender, args| {
            if let Some(args) = args {
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                dlog(&format!("NewWindowRequested {}", strip_query(&take_pwstr(uri))));
            }
            Ok(())
        }));
        let mut token = 0i64;
        webview.add_NewWindowRequested(&handler, &mut token).map_err(err)?;
        let nav = webview2_com::NavigationCompletedEventHandler::create(Box::new(|sender, args| {
            if let Some(sender) = sender {
                let mut src = PWSTR::null();
                sender.Source(&mut src)?;
                let mut ok = windows::core::BOOL(0);
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                if let Some(a) = args {
                    a.IsSuccess(&mut ok)?;
                    a.WebErrorStatus(&mut status)?;
                }
                dlog(&format!("NavigationCompleted ok={} status={} {}", ok.as_bool(), status.0, strip_query(&take_pwstr(src))));
            }
            Ok(())
        }));
        webview.add_NavigationCompleted(&nav, &mut token).map_err(err)?;
        let src_changed = webview2_com::SourceChangedEventHandler::create(Box::new(|sender, _| {
            if let Some(sender) = sender {
                let mut src = PWSTR::null();
                sender.Source(&mut src)?;
                dlog(&format!("SourceChanged {}", strip_query(&take_pwstr(src))));
            }
            Ok(())
        }));
        webview.add_SourceChanged(&src_changed, &mut token).map_err(err)?;
        let _ = controller.SetIsVisible(true);
    }
    {
        let mut s = st.borrow_mut();
        s.controller = Some(controller);
        s.webview = Some(webview.clone());
    }
    layout(hwnd);
    unsafe {
        let url = CoTaskMemPWSTR::from(LOGIN_URL);
        webview.Navigate(*url.as_ref().as_pcwstr()).map_err(err)?;
        SetTimer(Some(hwnd), TIMER_POLL, 1000, None);
    }
    drop(webview);

    // Nested loop until we have a session or the user closes the window.
    let mut msg = MSG::default();
    loop {
        {
            let mut s = st.borrow_mut();
            if let Some(sess) = s.result.take() {
                return Ok(sess);
            }
            if s.cancelled {
                return Err(SignInError::Cancelled);
            }
        }
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 == 0 {
            // WM_QUIT: re-post for the outer loop and bail out.
            unsafe { PostQuitMessage(msg.wParam.0 as i32) };
            return Err(SignInError::Cancelled);
        }
        if got.0 == -1 {
            return Err(err("GetMessage failed"));
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn poll_cookies() {
    let Some(st) = state() else { return };
    let webview = {
        let mut s = st.borrow_mut();
        if s.poll_in_flight || s.result.is_some() {
            return;
        }
        let Some(w) = s.webview.clone() else { return };
        s.poll_in_flight = true;
        w
    };
    let st2 = st.clone();
    let started = (|| -> windows::core::Result<()> {
        let manager = unsafe { webview.cast::<ICoreWebView2_2>()?.CookieManager()? };
        let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
            st2.borrow_mut().poll_in_flight = false;
            hr?;
            let Some(list) = list else { return Ok(()) };
            let (mut key, mut org) = (None, None);
            let mut names: Vec<String> = Vec::new();
            unsafe {
                let mut n = 0u32;
                list.Count(&mut n)?;
                for i in 0..n {
                    let c = list.GetValueAtIndex(i)?;
                    let (mut name, mut value) = (PWSTR::null(), PWSTR::null());
                    c.Name(&mut name)?;
                    c.Value(&mut value)?;
                    let (name, value) = (take_pwstr(name), take_pwstr(value));
                    names.push(name.clone());
                    match name.as_str() {
                        "sessionKey" if !value.is_empty() => key = Some(value),
                        "lastActiveOrg" if !value.is_empty() => org = Some(value),
                        _ => {}
                    }
                }
            }
            dlog(&format!("cookies: {}", names.join(",")));
            if let Some(session_key) = key {
                st2.borrow_mut().result = Some(Session { session_key, last_active_org: org });
            }
            Ok(())
        }));
        let url = CoTaskMemPWSTR::from(COOKIE_URL);
        unsafe { manager.GetCookies(*url.as_ref().as_pcwstr(), &handler) }
    })();
    if started.is_err() {
        st.borrow_mut().poll_in_flight = false;
    }
}

fn layout(hwnd: HWND) {
    let Some(st) = state() else { return };
    let s = st.borrow();
    let Some(c) = &s.controller else { return };
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let hint = HINT_H * GetDpiForWindow(hwnd).max(96) as i32 / 96;
        rc.top = hint.min(rc.bottom);
        let _ = c.SetBounds(rc);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_POLL => {
                poll_cookies();
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                rc.bottom = HINT_H * dpi / 96;
                let bg = crate::gdi::Brush::solid(crate::gdi::rgb(0xF5, 0xF4, 0xEE));
                FillRect(hdc, &rc, bg.0);
                let font = crate::gdi::Font::segoe(12 * dpi / 96, false);
                let _sel = crate::gdi::Selected::font(hdc, &font);
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, crate::gdi::rgb(0x3D, 0x3D, 0x3A));
                rc.left += 10 * dpi / 96;
                let mut text: Vec<u16> = HINT.encode_utf16().collect();
                DrawTextW(hdc, &mut text, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CLOSE => {
                if let Some(st) = state() {
                    st.borrow_mut().cancelled = true;
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn create_window(owner: HWND) -> Result<HWND, SignInError> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).map_err(err)?.into();
        let class = w!("ClaudeUsageWidgetSignIn");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(Some(instance), PCWSTR(1 as _)).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc); // may already be registered from an earlier sign-in

        let dpi = GetDpiForWindow(owner).max(96) as i32;
        let (w, h) = (480 * dpi / 96, 720 * dpi / 96);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Sign in to Claude"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            (sw - w) / 2,
            (sh - h) / 2,
            w,
            h,
            Some(owner),
            None,
            Some(instance),
            None,
        )
        .map_err(err)?;
        let _ = SetForegroundWindow(hwnd);
        Ok(hwnd)
    }
}

/// Best-effort delete of the WebView2 profile folder. Edge helper processes may hold
/// files for a moment after Close(), so retry for up to 2 s.
pub fn cleanup_profile() {
    let dir = webview_dir();
    let deadline = Instant::now() + Duration::from_secs(2);
    while dir.exists() {
        if std::fs::remove_dir_all(&dir).is_ok() || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
