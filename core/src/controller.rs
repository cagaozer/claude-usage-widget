//! The widget's state machine, shared by both platforms.
//!
//! `Controller` owns the app state, the settings, the background fetcher and the stored
//! session. Platform code forwards user actions and fetcher events to it and performs
//! the `Effect`s it returns (resize/repaint, change opacity, clean up sign-in data).
//! It never touches an OS API itself, so all of its behavior is unit-tested here.

use crate::fetcher::{Command, Event, Fetcher};
use crate::menu::Action;
use crate::session::{Session, SessionStore};
use crate::settings::Settings;
use crate::source::{FetchError, HttpSource, UsageSource};
use crate::usage::{Org, UsageState};
use crate::view::{build_view, AppState, ViewModel};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::Arc;

/// Something the platform must do after a controller call.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// What's shown changed: resize the window to `layout::content_height` and repaint.
    Relayout,
    /// Apply this opacity (percent) to the window.
    Opacity(u8),
    /// The user signed out: delete any leftover sign-in browser data.
    SignInCleanup,
}

/// Delivers fetcher events to the UI thread. Called on the fetcher thread with the
/// generation that fetcher was started with; the platform must hand `(gen, event)` to
/// `Controller::handle_posted` on the UI thread.
pub type Sink = Arc<dyn Fn(u64, Event) + Send + Sync>;

pub struct Controller {
    pub settings: Settings,
    pub state: AppState,
    /// All orgs of the signed-in account, and the one being shown.
    pub orgs: Vec<Org>,
    pub current_org: Option<String>,
    pub session: Option<Session>,
    pub quitting: bool,
    /// Folder holding settings.json.
    dir: PathBuf,
    /// Bumped whenever the fetcher is replaced or stopped. Events still queued from an
    /// older fetcher carry an older number and are dropped by `handle_posted`.
    gen: u64,
    fetcher: Option<Fetcher>,
    store: Box<dyn SessionStore>,
    sink: Sink,
    /// Builds the data source for a session. Always `HttpSource` in the app; tests swap
    /// in an offline source so they never contact claude.ai.
    pub(crate) make_source: fn(Session) -> Box<dyn UsageSource>,
}

impl Controller {
    pub fn new(dir: PathBuf, settings: Settings, store: Box<dyn SessionStore>, sink: Sink) -> Controller {
        Controller {
            settings,
            state: AppState::Loading,
            orgs: Vec::new(),
            current_org: None,
            session: None,
            quitting: false,
            dir,
            gen: 0,
            fetcher: None,
            store,
            sink,
            make_source: |s| Box::new(HttpSource::new(s)),
        }
    }

    /// Current fetcher generation.
    pub fn gen(&self) -> u64 {
        self.gen
    }

    /// True while a fetcher is running (i.e. signed in, or in demo mode).
    pub fn is_fetching(&self) -> bool {
        self.fetcher.is_some()
    }

    pub fn view(&self, now: DateTime<Utc>) -> ViewModel {
        build_view(&self.state, now)
    }

    pub fn set_state(&mut self, s: AppState) -> Vec<Effect> {
        self.state = s;
        vec![Effect::Relayout]
    }

    /// Start polling `source`, replacing (and joining) any previous fetcher.
    pub fn start(&mut self, source: Box<dyn UsageSource>, last_active: Option<String>) -> Vec<Effect> {
        self.fetcher = None; // joins the previous thread before the new one starts
        self.gen += 1;
        let (gen, sink) = (self.gen, self.sink.clone());
        self.fetcher = Some(Fetcher::spawn(
            source,
            self.settings.interval_secs,
            self.settings.org_id.clone(),
            last_active,
            move |ev| sink(gen, ev),
        ));
        Vec::new()
    }

    /// Start fetching with a session restored from storage.
    pub fn start_with_session(&mut self, session: Session) -> Vec<Effect> {
        let last = session.last_active_org.clone();
        self.session = Some(session.clone());
        let fx = self.set_state(AppState::Loading);
        self.start((self.make_source)(session), last);
        fx
    }

    /// A sign-in just succeeded: remember the session, then start fetching.
    pub fn signed_in(&mut self, session: Session) -> Vec<Effect> {
        let _ = self.store.save(&session); // if saving fails, the user just signs in again next run
        self.start_with_session(session)
    }

    /// Stop fetching and forget the session (user signed out, or it expired).
    pub fn signed_out(&mut self) -> Vec<Effect> {
        self.fetcher = None;
        self.gen += 1; // anything still queued from the old fetcher is now stale
        self.session = None;
        self.orgs.clear();
        self.current_org = None;
        self.store.clear();
        self.set_state(AppState::SignedOut)
    }

    /// Apply an event delivered by the sink for fetcher generation `gen`.
    pub fn handle_posted(&mut self, gen: u64, ev: Event) -> Vec<Effect> {
        if gen != self.gen || self.quitting {
            return Vec::new();
        }
        match ev {
            Event::Orgs(orgs, chosen) => {
                self.orgs = orgs;
                self.current_org = Some(chosen);
                Vec::new()
            }
            Event::Usage(UsageState::Limits(limits)) => self.set_state(AppState::Data { limits, stale: false }),
            Event::Usage(UsageState::NoLimits) => self.set_state(AppState::NoLimits),
            Event::Error(FetchError::Auth) => self.signed_out(),
            Event::Error(FetchError::Format(_)) => self.set_state(AppState::FormatChanged),
            Event::Error(FetchError::Network(_) | FetchError::Blocked(_)) => {
                // Keep showing the last numbers, greyed out, rather than blanking the widget.
                let next = match std::mem::replace(&mut self.state, AppState::Loading) {
                    AppState::Data { limits, .. } => AppState::Data { limits, stale: true },
                    _ => AppState::Offline,
                };
                self.set_state(next)
            }
        }
    }

    /// Apply a menu action. `SignIn` and `Quit` need platform UI and are handled by the
    /// caller; here they do nothing.
    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Refresh => {
                self.send(Command::RefreshNow);
                Vec::new()
            }
            Action::Interval(secs) => {
                self.settings.interval_secs = secs;
                self.send(Command::SetInterval(secs));
                self.save_settings();
                Vec::new()
            }
            Action::Opacity(pct) => {
                self.settings.opacity_pct = pct;
                self.save_settings();
                vec![Effect::Opacity(pct)]
            }
            Action::Org(id) => {
                self.settings.org_id = Some(id.clone());
                self.current_org = Some(id.clone());
                self.save_settings();
                let fx = if self.is_fetching() { self.set_state(AppState::Loading) } else { Vec::new() };
                self.send(Command::SetOrg(id));
                fx
            }
            Action::SignOut => {
                let mut fx = self.signed_out();
                fx.push(Effect::SignInCleanup);
                fx
            }
            Action::None | Action::SignIn | Action::Quit => Vec::new(),
        }
    }

    fn send(&self, c: Command) {
        if let Some(f) = &self.fetcher {
            f.send(c);
        }
    }

    pub fn save_settings(&self) {
        let _ = self.settings.save(&self.dir);
    }

    /// Stop the fetcher (joining its thread) and save settings. Returns false if quit
    /// is already under way, so re-entrant quit requests are ignored.
    pub fn prepare_quit(&mut self) -> bool {
        if self.quitting {
            return false;
        }
        self.quitting = true;
        self.fetcher = None;
        self.save_settings();
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::session::MemoryStore;
    use crate::source::FetchError;
    use crate::usage::{Limits, Window};

    pub(crate) fn controller() -> (tempfile::TempDir, Controller) {
        let (dir, c, _) = controller_with_store();
        (dir, c)
    }

    fn controller_with_store() -> (tempfile::TempDir, Controller, MemoryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::default();
        let mut c = Controller::new(
            dir.path().to_path_buf(),
            Settings::default(),
            Box::new(store.clone()),
            std::sync::Arc::new(|_, _| {}),
        );
        c.make_source = |_| Box::new(OfflineSource);
        (dir, c, store)
    }

    /// Always fails with a network error, without any I/O.
    struct OfflineSource;

    impl UsageSource for OfflineSource {
        fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError> {
            Err(FetchError::Network("offline (test)".into()))
        }
        fn fetch_usage(&mut self, _org: &str) -> Result<UsageState, FetchError> {
            Err(FetchError::Network("offline (test)".into()))
        }
    }

    fn session() -> Session {
        Session { session_key: "sk-test".into(), last_active_org: None }
    }

    fn limits() -> UsageState {
        let w = Window { utilization: 10.0, resets_at: None };
        UsageState::Limits(Limits { session: w.clone(), weekly: w, extra: vec![] })
    }

    #[test]
    fn late_event_after_sign_out_is_ignored() {
        let (_d, mut c) = controller();
        let old_gen = c.gen();
        let _ = c.signed_out();
        // A fetch that was in flight when the user signed out arrives afterwards.
        let _ = c.handle_posted(old_gen, Event::Usage(limits()));
        assert_eq!(c.state, AppState::SignedOut);
        let _ = c.handle_posted(old_gen, Event::Error(FetchError::Network("x".into())));
        assert_eq!(c.state, AppState::SignedOut);
    }

    #[test]
    fn current_generation_event_is_applied() {
        let (_d, mut c) = controller();
        let gen = c.gen();
        let fx = c.handle_posted(gen, Event::Usage(limits()));
        assert!(matches!(c.state, AppState::Data { stale: false, .. }));
        assert!(fx.contains(&Effect::Relayout));
    }

    #[test]
    fn signed_out_clears_store() {
        let (_d, mut c, store) = controller_with_store();
        store.save(&session()).unwrap();
        let fx = c.signed_out();
        assert_eq!(store.load(), None);
        assert_eq!(c.state, AppState::SignedOut);
        assert!(fx.contains(&Effect::Relayout));
    }

    #[test]
    fn signed_in_saves_store() {
        let (_d, mut c, store) = controller_with_store();
        let _ = c.signed_in(session());
        assert_eq!(store.load(), Some(session()));
        assert_eq!(c.state, AppState::Loading);
        assert!(c.is_fetching());
    }

    #[test]
    fn network_error_marks_data_stale() {
        let (_d, mut c) = controller();
        let gen = c.gen();
        let _ = c.handle_posted(gen, Event::Error(FetchError::Network("x".into())));
        assert_eq!(c.state, AppState::Offline);
        let _ = c.handle_posted(gen, Event::Usage(limits()));
        let _ = c.handle_posted(gen, Event::Error(FetchError::Blocked(429)));
        assert!(matches!(c.state, AppState::Data { stale: true, .. }));
    }

    #[test]
    fn auth_error_signs_out() {
        let (_d, mut c) = controller();
        let gen = c.gen();
        let _ = c.handle_posted(gen, Event::Error(FetchError::Auth));
        assert_eq!(c.state, AppState::SignedOut);
    }

    #[test]
    fn format_error_shows_format_changed() {
        let (_d, mut c) = controller();
        let gen = c.gen();
        let _ = c.handle_posted(gen, Event::Error(FetchError::Format("x".into())));
        assert_eq!(c.state, AppState::FormatChanged);
    }

    #[test]
    fn opacity_action_saves_and_returns_effect() {
        let (d, mut c) = controller();
        let fx = c.apply(Action::Opacity(70));
        assert_eq!(fx, vec![Effect::Opacity(70)]);
        assert_eq!(Settings::load(d.path()).opacity_pct, 70);
    }

    #[test]
    fn sign_out_action_requests_cleanup() {
        let (_d, mut c) = controller();
        let fx = c.apply(Action::SignOut);
        assert!(fx.contains(&Effect::SignInCleanup));
        assert_eq!(c.state, AppState::SignedOut);
    }

    #[test]
    fn quit_is_once_and_stops_events() {
        let (_d, mut c) = controller();
        let gen = c.gen();
        assert!(c.prepare_quit());
        assert!(!c.prepare_quit());
        let _ = c.handle_posted(gen, Event::Usage(limits()));
        assert_eq!(c.state, AppState::Loading);
    }
}
