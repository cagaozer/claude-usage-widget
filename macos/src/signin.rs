//! macOS sign-in: claude.ai's real login page in a WKWebView, shown in a modal window.
//!
//! - The web view uses a non-persistent data store, so cookies and cache live only in
//!   memory and vanish when the window closes (nothing to clean up on disk).
//! - It keeps WebKit's default user agent; it never pretends to be Safari.
//! - Sign-in pop-ups (Google, Apple, SSO) are hosted in a real second window created
//!   with the configuration WebKit supplies, so `window.opener` works and the pop-up
//!   can hand the result back to the login page. At most one pop-up exists at a time.
//! - Once a second the cookie store is checked; when a claude.ai `sessionKey` appears,
//!   the window closes and the session is returned. Cookie values are never logged.

use crate::app::schedule_timer;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSBackingStoreType, NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask};
use objc2_foundation::{NSArray, NSError, NSHTTPCookie, NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString, NSTimer, NSURLRequest, NSURL};
use objc2_web_kit::{WKNavigation, WKNavigationAction, WKNavigationDelegate, WKUIDelegate, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore, WKWindowFeatures};
use std::cell::RefCell;
use std::ptr::NonNull;
use usage_core::session::{session_from_cookies, Session, SIGN_IN_HINT};

const LOGIN_URL: &str = "https://claude.ai/login";
const WIDTH: f64 = 480.0;
const HEIGHT: f64 = 720.0;
/// Height of the hint line above the web view.
const HINT_H: f64 = 30.0;

/// Everything the open sign-in window needs. Lives in a thread_local while the window
/// is up, because AppKit/WebKit callbacks (delegate methods, timers) need to reach it.
struct State {
    window: Retained<NSWindow>,
    web: Retained<WKWebView>,
    /// WebKit and AppKit hold delegates weakly; this keeps ours alive.
    _delegate: Retained<SignInDelegate>,
    popup: Option<(Retained<NSWindow>, Retained<WKWebView>)>,
    timer: Option<Retained<NSTimer>>,
    result: Option<Session>,
    finished: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| cell.try_borrow_mut().ok().and_then(|mut slot| slot.as_mut().map(f)))
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn is_same(a: &AnyObject, b: &NSWindow) -> bool {
    std::ptr::eq(a as *const AnyObject, b as *const NSWindow as *const AnyObject)
}

define_class!(
    // Receives WebKit callbacks (pop-ups, page loads) and window-close notifications.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CUWSignInDelegate"]
    struct SignInDelegate;

    unsafe impl NSObjectProtocol for SignInDelegate {}

    unsafe impl WKUIDelegate for SignInDelegate {
        /// The page called `window.open` (e.g. "Continue with Google"). Host it in a new
        /// window, using WebKit's configuration so it shares the session and can talk
        /// back to its opener.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _web: &WKWebView,
            config: &WKWebViewConfiguration,
            _action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            close_popup();
            let mtm = mtm();
            let window = new_window(mtm, "Sign in", 480.0, 640.0);
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 640.0));
            let web = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, config) };
            unsafe {
                web.setUIDelegate(Some(ProtocolObject::from_ref(self)));
                web.setNavigationDelegate(Some(ProtocolObject::from_ref(self)));
                window.setDelegate(Some(ProtocolObject::from_ref(self)));
            }
            dlog("pop-up opened");
            window.setContentView(Some(&web));
            window.makeKeyAndOrderFront(None);
            with_state(|s| s.popup = Some((window, web.clone())));
            Some(web)
        }

        /// The pop-up called `window.close()` after handing back the result.
        #[unsafe(method(webViewDidClose:))]
        fn web_view_did_close(&self, _web: &WKWebView) {
            close_popup();
        }
    }

    // Page-load results, logged in debug builds only (see `dlog`).
    unsafe impl WKNavigationDelegate for SignInDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web: &WKWebView, _nav: Option<&WKNavigation>) {
            dlog(&format!("loaded {}", page(web)));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, web: &WKWebView, _nav: Option<&WKNavigation>, error: &NSError) {
            dlog(&format!("failed to start loading {}: {}", page(web), error.localizedDescription()));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, web: &WKWebView, _nav: Option<&WKNavigation>, error: &NSError) {
            dlog(&format!("failed loading {}: {}", page(web), error.localizedDescription()));
        }
    }

    unsafe impl NSWindowDelegate for SignInDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, notification: &NSNotification) {
            let Some(closing) = notification.object() else { return };
            let main_closed = with_state(|s| {
                if s.popup.as_ref().is_some_and(|(w, _)| is_same(&closing, w)) {
                    s.popup = None; // user closed the pop-up; keep signing in
                    false
                } else {
                    is_same(&closing, &s.window)
                }
            });
            if main_closed == Some(true) {
                finish(None); // user closed the sign-in window: cancelled
            }
        }
    }
);

/// Debug builds: print sign-in progress to stderr. Only page addresses without the
/// query string are logged, never cookies. Release builds log nothing.
fn dlog(msg: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[signin] {msg}");
    }
}

/// Scheme, host and path of the web view's current page (no query or fragment).
fn page(web: &WKWebView) -> String {
    let Some(url) = (unsafe { web.URL() }) else { return "(no page)".into() };
    let full = url.absoluteString().map(|s| s.to_string()).unwrap_or_default();
    full.split(['?', '#']).next().unwrap_or_default().to_string()
}

fn new_window(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Retained<NSWindow> {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
    let window: Retained<NSWindow> = unsafe {
        msg_send![
            NSWindow::alloc(mtm),
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.center();
    window
}

fn close_popup() {
    if let Some((window, web)) = with_state(|s| s.popup.take()).flatten() {
        unsafe {
            web.stopLoading();
            web.setUIDelegate(None);
            window.setDelegate(None);
        }
        window.close();
    }
}

/// End the modal session with `result` (None = cancelled).
fn finish(result: Option<Session>) {
    let first = with_state(|s| {
        if s.finished {
            return false;
        }
        s.finished = true;
        s.result = result;
        true
    });
    if first == Some(true) {
        // abortModal (not stopModal) also works from timers and completion handlers.
        NSApplication::sharedApplication(mtm()).abortModal();
    }
}

/// Look for the session cookie; called once a second.
fn poll_cookies() {
    let Some(store) = with_state(|s| unsafe { s.web.configuration().websiteDataStore().httpCookieStore() }) else {
        return;
    };
    let block = RcBlock::new(|cookies: NonNull<NSArray<NSHTTPCookie>>| {
        let cookies = unsafe { cookies.as_ref() };
        let triples: Vec<(String, String, String)> = cookies
            .iter()
            .map(|c| (c.name().to_string(), c.domain().to_string(), c.value().to_string()))
            .collect();
        if let Some(session) = session_from_cookies(triples.iter().map(|(n, d, v)| (n.as_str(), d.as_str(), v.as_str()))) {
            dlog("session cookie found");
            finish(Some(session));
        }
    });
    unsafe { store.getAllCookies(&block) };
}

/// True while the sign-in window is open.
pub fn is_active() -> bool {
    STATE.with(|cell| cell.try_borrow().map(|s| s.is_some()).unwrap_or(true))
}

/// Close the sign-in window as if the user cancelled (used when quitting).
pub fn cancel() {
    finish(None);
}

/// Show the sign-in window and block (in a modal loop) until the user signs in or
/// closes it. Returns the session, or None if cancelled.
pub fn sign_in(mtm: MainThreadMarker) -> Option<Session> {
    if is_active() {
        return None;
    }
    let window = new_window(mtm, "Sign in to Claude", WIDTH, HEIGHT);

    // Content: the hint line on top, the web view below (AppKit's y grows upwards).
    let content = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)));
    let hint = NSTextField::labelWithString(&NSString::from_str(SIGN_IN_HINT), mtm);
    // 11 pt so the whole tip fits on one line in the 480 pt window.
    hint.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(11.0)));
    hint.setFrame(NSRect::new(NSPoint::new(10.0, HEIGHT - HINT_H + 7.0), NSSize::new(WIDTH - 20.0, 16.0)));
    content.addSubview(&hint);

    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
    let web_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT - HINT_H));
    let web = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), web_frame, &config) };
    content.addSubview(&web);
    window.setContentView(Some(&content));

    let delegate: Retained<SignInDelegate> = unsafe { msg_send![SignInDelegate::alloc(mtm), init] };
    unsafe {
        web.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(LOGIN_URL)) {
        let _ = unsafe { web.loadRequest(&NSURLRequest::requestWithURL(&url)) };
    }

    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            window: window.clone(),
            web,
            _delegate: delegate,
            popup: None,
            timer: None,
            result: None,
            finished: false,
        })
    });
    // The timer runs in common modes, so it keeps firing inside the modal loop below.
    let timer = schedule_timer(1.0, true, poll_cookies);
    with_state(|s| s.timer = Some(timer));

    // Our app is an accessory and normally never active; activate so the window gets focus.
    let ns_app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    ns_app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
    ns_app.runModalForWindow(&window); // returns after finish() calls abortModal

    // Tear down everything the sign-in created.
    let state = STATE.with(|cell| cell.borrow_mut().take());
    close_popup_of(&state);
    let state = state?;
    if let Some(t) = &state.timer {
        t.invalidate();
    }
    unsafe {
        state.web.stopLoading();
        state.web.setUIDelegate(None);
        state.web.setNavigationDelegate(None);
        state.window.setDelegate(None);
    }
    state.window.close();
    state.result
}

/// Close a pop-up still referenced by `state` (the thread_local is already emptied).
fn close_popup_of(state: &Option<State>) {
    if let Some((window, web)) = state.as_ref().and_then(|s| s.popup.as_ref()) {
        unsafe {
            web.stopLoading();
            web.setUIDelegate(None);
            window.setDelegate(None);
        }
        window.close();
    }
}
