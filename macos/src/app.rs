//! macOS application: owns the shared `Controller`, the widget panel and its timers, and
//! connects AppKit events to the controller.
//!
//! All state lives in a main-thread `thread_local`. Handlers borrow it only briefly and
//! never across anything that runs a nested event loop (the context menu, window
//! dragging), because AppKit can call back into this module from inside those loops.

use crate::keychain::KeychainStore;
use crate::menu;
use crate::panel::{self, PanelDelegate, WidgetView};
use crate::paths::app_dir;
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSMenu, NSPanel, NSScreen};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSTimer};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;
use usage_core::controller::{Controller, Effect, Sink};
use usage_core::fetcher::Event;
use usage_core::geometry::{clamp_to_work_areas, default_position, Rect};
use usage_core::layout::{content_height, W};
use usage_core::menu::{build_menu, menu_action, Action, MenuSnapshot};
use usage_core::session::Session;
use usage_core::settings::Settings;
use usage_core::view::AppState;

pub enum StartMode {
    NeedSignIn,
    Session(Session),
    #[cfg(debug_assertions)]
    Demo,
}

struct App {
    ctl: Controller,
    panel: Retained<NSPanel>,
    view: Retained<WidgetView>,
    /// AppKit holds window delegates weakly; this keeps ours alive.
    _panel_delegate: Retained<PanelDelegate>,
    /// Repaints once a minute so countdowns stay current between fetches.
    countdown: Retained<NSTimer>,
    /// Org list as shown in the last context menu (see `MenuSnapshot`).
    menu_snapshot: MenuSnapshot,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` with the app state, unless it doesn't exist or is already borrowed further up
/// the stack (a re-entrant AppKit callback), in which case nothing happens.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut slot| slot.as_mut().map(f)))
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit code must run on the main thread")
}

/// A repeating (or one-shot) timer that also fires while a menu or modal window is
/// tracking events (common run-loop modes).
pub fn schedule_timer(seconds: f64, repeats: bool, f: impl Fn() + 'static) -> Retained<NSTimer> {
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| f());
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(seconds, repeats, &block) };
    unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
    timer
}

// --- Screen geometry. AppKit puts the origin at the bottom-left of the primary screen
// with y growing upwards; settings and the shared geometry code use top-left with y
// growing downwards, like Windows. These helpers convert between the two.

/// Height of the primary screen (the one with the menu bar), the flip axis.
fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm).iter().next().map(|s| s.frame().size.height).unwrap_or(0.0)
}

fn to_top_left(r: NSRect, primary_h: f64) -> Rect {
    Rect {
        left: r.origin.x as i32,
        top: (primary_h - r.origin.y - r.size.height) as i32,
        right: (r.origin.x + r.size.width) as i32,
        bottom: (primary_h - r.origin.y) as i32,
    }
}

/// Visible areas (minus menu bar and Dock) of all screens, and of the primary one.
fn work_areas(mtm: MainThreadMarker) -> (Vec<Rect>, Rect) {
    let h = primary_height(mtm);
    let areas: Vec<Rect> = NSScreen::screens(mtm).iter().map(|s| to_top_left(s.visibleFrame(), h)).collect();
    let primary = areas.first().copied().unwrap_or_default();
    (areas, primary)
}

impl App {
    fn height(&self) -> f64 {
        content_height(&self.ctl.view(chrono::Utc::now())) as f64
    }

    /// Top-left of the panel in settings coordinates.
    fn top_left(&self) -> (i32, i32) {
        let r = to_top_left(self.panel.frame(), primary_height(mtm()));
        (r.left, r.top)
    }

    fn run_effects(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Relayout => self.relayout(),
                Effect::Opacity(pct) => self.panel.setAlphaValue(pct as f64 / 100.0),
                // The sign-in web view uses a non-persistent data store: nothing to delete.
                Effect::SignInCleanup => {}
            }
        }
    }

    /// Resize to fit the content, keeping the top-left corner fixed, and repaint.
    fn relayout(&self) {
        let f = self.panel.frame();
        let h = self.height();
        if (f.size.height - h).abs() > 0.5 {
            let top = f.origin.y + f.size.height;
            let r = NSRect::new(NSPoint::new(f.origin.x, top - h), NSSize::new(W as f64, h));
            // display: false. Drawing synchronously here would re-enter `draw` while
            // the app state is borrowed.
            self.panel.setFrame_display(r, false);
        }
        self.view.setNeedsDisplay(true);
    }

    fn save_position(&mut self) {
        self.ctl.settings.pos = Some(self.top_left());
        self.ctl.save_settings();
    }
}

/// Create the widget and run the AppKit event loop. Returns when the app terminates.
pub fn run(start: StartMode, quit_after: Option<f64>) {
    let mtm = mtm();
    let ns_app = NSApplication::sharedApplication(mtm);
    // Accessory: no Dock icon, no menu bar (also set by LSUIElement in Info.plist).
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let dir = app_dir();
    let settings = Settings::load(&dir);
    // Fetch events arrive on the fetch thread; hop to the main queue to apply them.
    let sink: Sink = Arc::new(|gen, ev| DispatchQueue::main().exec_async(move || on_posted(gen, ev)));
    let ctl = Controller::new(dir, settings, Box::new(KeychainStore::new()), sink);

    let opacity = ctl.settings.opacity_pct;
    let height = content_height(&ctl.view(chrono::Utc::now())) as f64;
    let (panel, view, panel_delegate) = panel::create(mtm, W as f64, height);
    panel.setAlphaValue(opacity as f64 / 100.0);

    // Restore the saved position if it's still on a screen, else top-right of the main one.
    let size = (W, height as i32);
    let (areas, primary) = work_areas(mtm);
    let (x, y) = match ctl.settings.pos {
        Some(p) => clamp_to_work_areas(p, size, &areas, primary),
        None => default_position(size, primary),
    };
    panel.setFrameTopLeftPoint(NSPoint::new(x as f64, primary_height(mtm) - y as f64));

    let countdown = schedule_timer(60.0, true, || {
        with_app(|a| a.view.setNeedsDisplay(true));
    });
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            ctl,
            panel,
            view,
            _panel_delegate: panel_delegate,
            countdown,
            menu_snapshot: MenuSnapshot::default(),
        })
    });

    let need_sign_in = matches!(start, StartMode::NeedSignIn);
    with_app(|a| {
        let fx = match start {
            StartMode::Session(s) => a.ctl.start_with_session(s),
            StartMode::NeedSignIn => a.ctl.set_state(AppState::SignedOut),
            #[cfg(debug_assertions)]
            StartMode::Demo => {
                let mut fx = a.ctl.set_state(AppState::Loading);
                fx.extend(a.ctl.start(Box::new(usage_core::demo::DemoSource::default()), None));
                fx
            }
        };
        a.run_effects(fx);
    });
    // Outside the borrow: ordering front may draw synchronously, and `draw` must be able
    // to read the state.
    if let Some(panel) = with_app(|a| a.panel.clone()) {
        panel.orderFrontRegardless();
    }
    if need_sign_in {
        // Open sign-in once the event loop is running.
        DispatchQueue::main().exec_async(begin_sign_in);
    }
    if let Some(secs) = quit_after {
        // Debug-only `--quit-after`: exercises the normal quit path for automated checks.
        let _ = schedule_timer(secs, false, quit);
    }

    ns_app.run();
}

/// Apply a fetcher event on the main thread.
fn on_posted(gen: u64, ev: Event) {
    with_app(|a| {
        let fx = a.ctl.handle_posted(gen, ev);
        a.run_effects(fx);
    });
}

/// Called from the view's `drawRect:`.
pub fn draw(height: f64) {
    APP.with(|cell| {
        // Read-only borrow; if the state is mutably borrowed, skip this frame (a repaint
        // is always requested again after the change).
        if let Ok(slot) = cell.try_borrow() {
            if let Some(a) = slot.as_ref() {
                crate::draw::paint(&a.ctl.view(chrono::Utc::now()), height);
            }
        }
    });
}

fn contains(r: NSRect, p: NSPoint) -> bool {
    p.x >= r.origin.x && p.x < r.origin.x + r.size.width && p.y >= r.origin.y && p.y < r.origin.y + r.size.height
}

/// Left click: ✕ quits; when signed out (or signing in), anywhere opens sign-in;
/// otherwise drag the widget.
pub fn on_mouse_down(view: &WidgetView, event: &NSEvent, p: NSPoint) {
    if contains(crate::draw::close_rect(), p) {
        quit();
        return;
    }
    let signing = with_app(|a| matches!(a.ctl.state, AppState::SignedOut | AppState::SigningIn)).unwrap_or(false);
    if signing {
        begin_sign_in(); // opens sign-in, or brings an open sign-in window to the front
        return;
    }
    // Hand the drag to the window server. The new position is saved by
    // `on_window_moved` when the move ends. No borrow is held here.
    if let Some(window) = view.window() {
        window.performWindowDragWithEvent(event);
    }
}

/// The panel moved (`windowDidMove:`): remember where, so it reopens there even if the
/// app is ended by logout or shutdown rather than by Quit.
pub fn on_window_moved() {
    with_app(|a| {
        if !a.ctl.quitting {
            a.save_position();
        }
    });
}

/// Right click: show the context menu built from the shared menu model.
pub fn on_right_mouse_down(view: &WidgetView, event: &NSEvent) {
    let mtm = mtm();
    let Some(menu) = with_app(|a| {
        let (entries, snap) = build_menu(&a.ctl);
        a.menu_snapshot = snap;
        menu::build(mtm, &entries, view)
    }) else {
        return;
    };
    // Tracks the menu in a nested loop; the chosen item calls `on_menu_command`.
    NSMenu::popUpContextMenu_withEvent_forView(&menu, event, view);
}

pub fn on_menu_command(id: usize) {
    let Some(action) = with_app(|a| menu_action(id, &a.menu_snapshot)) else { return };
    match action {
        // Deferred to the next turn of the event loop so they don't run inside menu tracking.
        Action::SignIn => DispatchQueue::main().exec_async(begin_sign_in),
        Action::Quit => DispatchQueue::main().exec_async(quit),
        action => {
            with_app(|a| {
                let fx = a.ctl.apply(action);
                a.run_effects(fx);
            });
        }
    }
}

/// Open the sign-in window. When it finishes, start fetching, or show
/// "Signed out — click to sign in" if the user closed it.
pub fn begin_sign_in() {
    let open = with_app(|a| {
        if a.ctl.quitting {
            return false;
        }
        if matches!(a.ctl.state, AppState::SigningIn) {
            // Already open but maybe hidden behind other apps. An accessory app has no
            // Dock icon or Cmd-Tab entry, so this is the user's way back to it.
            crate::signin::bring_to_front();
            return false;
        }
        let fx = a.ctl.set_state(AppState::SigningIn);
        a.run_effects(fx);
        true
    });
    if open == Some(true) {
        crate::signin::start(mtm(), sign_in_done);
    }
}

fn sign_in_done(result: Option<Session>) {
    with_app(|a| {
        if a.ctl.quitting {
            return;
        }
        let fx = match result {
            Some(session) => a.ctl.signed_in(session),
            None => a.ctl.set_state(AppState::SignedOut),
        };
        a.run_effects(fx);
    });
}

/// Quit: stop the countdown, hide, close any sign-in window, stop the fetch
/// thread, save settings, then terminate the process. Safe to call more than once.
pub fn quit() {
    let proceed = with_app(|a| {
        if a.ctl.quitting {
            return false;
        }
        a.save_position();
        a.countdown.invalidate();
        a.panel.orderOut(None);
        a.ctl.prepare_quit()
    })
    .unwrap_or(false);
    if proceed {
        crate::signin::close_now();
        terminate();
    }
}

fn terminate() {
    NSApplication::sharedApplication(mtm()).terminate(None);
}
