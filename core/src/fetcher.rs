//! Background thread that polls a `UsageSource` and reports `Event`s through a sink.

use crate::source::{FetchError, UsageSource};
use crate::usage::{Org, UsageState};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    RefreshNow,
    SetInterval(u32),
    SetOrg(String),
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// All orgs, and the uuid of the one being shown.
    Orgs(Vec<Org>, String),
    Usage(UsageState),
    Error(FetchError),
}

pub struct Fetcher {
    tx: Sender<Command>,
    handle: Option<JoinHandle<()>>,
}

impl Fetcher {
    pub fn spawn(
        source: Box<dyn UsageSource>,
        interval_secs: u32,
        preferred_org: Option<String>,
        last_active_org: Option<String>,
        sink: impl Fn(Event) + Send + 'static,
    ) -> Fetcher {
        Self::spawn_inner(
            source,
            Duration::from_secs(interval_secs as u64),
            Duration::from_secs(crate::settings::MIN_INTERVAL_SECS as u64),
            preferred_org,
            last_active_org,
            Box::new(sink),
        )
    }

    fn spawn_inner(
        source: Box<dyn UsageSource>,
        interval: Duration,
        min_interval: Duration,
        preferred_org: Option<String>,
        last_active_org: Option<String>,
        sink: Box<dyn Fn(Event) + Send>,
    ) -> Fetcher {
        let (tx, rx) = mpsc::channel();
        let mut worker = Worker {
            source,
            sink,
            org: None,
            preferred_org,
            last_active_org,
            dead: false,
        };
        let handle = std::thread::Builder::new()
            .name("usage-fetcher".into())
            .spawn(move || {
                let mut interval = interval.max(min_interval);
                let mut next = Instant::now(); // first fetch immediately
                loop {
                    let cmd = if worker.dead {
                        rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    } else {
                        rx.recv_timeout(next.saturating_duration_since(Instant::now()))
                    };
                    match cmd {
                        Err(RecvTimeoutError::Timeout) | Ok(Command::RefreshNow) => {
                            if !worker.dead {
                                worker.tick();
                                next = Instant::now() + interval;
                            }
                        }
                        Ok(Command::SetInterval(secs)) => {
                            interval = Duration::from_secs(secs as u64).max(min_interval);
                            next = Instant::now() + interval;
                        }
                        Ok(Command::SetOrg(id)) => {
                            worker.org = Some(id);
                            if !worker.dead {
                                worker.tick();
                                next = Instant::now() + interval;
                            }
                        }
                        Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                // `worker` (and its source/sink) drops here, on the fetch thread.
            })
            .expect("spawn fetch thread");
        Fetcher { tx, handle: Some(handle) }
    }

    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }

    /// Ask the thread to stop without waiting for it. A request already in flight (up to
    /// the 15 s HTTP timeout on a stalled network) finishes in the background, then the
    /// thread exits; whatever it reports meanwhile carries an outdated generation and is
    /// ignored by the controller. Used so sign-out and quit never freeze the UI.
    pub fn detach(mut self) {
        let _ = self.tx.send(Command::Stop);
        drop(self.handle.take()); // dropping a JoinHandle detaches the thread
    }
}

impl Drop for Fetcher {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Worker {
    source: Box<dyn UsageSource>,
    sink: Box<dyn Fn(Event) + Send>,
    org: Option<String>,
    preferred_org: Option<String>,
    last_active_org: Option<String>,
    /// Set after an auth failure: no more requests until a new Fetcher is made.
    dead: bool,
}

impl Worker {
    fn tick(&mut self) {
        let result = match self.org.clone() {
            Some(org) => self.source.fetch_usage(&org),
            None => self.choose_org(),
        };
        let event = match result {
            Ok(state) => Event::Usage(state),
            Err(e) => {
                if e == FetchError::Auth {
                    self.dead = true;
                }
                Event::Error(e)
            }
        };
        (self.sink)(event);
    }

    /// Pick an org (preferred → last-active with limits → first with limits → first),
    /// emit `Orgs`, and return the chosen org's usage so it isn't fetched twice.
    fn choose_org(&mut self) -> Result<UsageState, FetchError> {
        let orgs = self.source.list_orgs()?;
        if orgs.is_empty() {
            return Err(FetchError::Format("no organizations".into()));
        }
        let has = |id: &Option<String>| id.as_ref().filter(|id| orgs.iter().any(|o| &o.uuid == *id)).cloned();

        let (chosen, state) = if let Some(p) = has(&self.preferred_org) {
            let s = self.source.fetch_usage(&p)?;
            (p, s)
        } else {
            let first = has(&self.last_active_org);
            let order: Vec<String> = first
                .clone()
                .into_iter()
                .chain(orgs.iter().map(|o| o.uuid.clone()).filter(|u| Some(u) != first.as_ref()))
                .collect();
            let mut fallback = None;
            let mut found = None;
            for id in order {
                let s = self.source.fetch_usage(&id)?;
                if matches!(s, UsageState::Limits(_)) {
                    found = Some((id, s));
                    break;
                }
                fallback.get_or_insert((id, s));
            }
            found.or(fallback).expect("orgs non-empty")
        };
        self.org = Some(chosen.clone());
        (self.sink)(Event::Orgs(orgs, chosen));
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    type Script = VecDeque<Result<UsageState, FetchError>>;

    struct FakeSource {
        orgs: Vec<Org>,
        /// Per-org queue of results; the last result repeats once the queue has one left.
        usage: HashMap<String, Script>,
        fetches: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
    }

    impl UsageSource for FakeSource {
        fn list_orgs(&mut self) -> Result<Vec<Org>, FetchError> {
            Ok(self.orgs.clone())
        }
        fn fetch_usage(&mut self, org_id: &str) -> Result<UsageState, FetchError> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            let q = self.usage.get_mut(org_id).expect("unknown org");
            if q.len() > 1 { q.pop_front().unwrap() } else { q[0].clone() }
        }
    }

    impl Drop for FakeSource {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    fn org(id: &str) -> Org {
        Org { uuid: id.into(), name: id.into() }
    }

    fn limits(pct: f64) -> UsageState {
        use crate::usage::{Limits, Window};
        let w = Window { utilization: pct, resets_at: None };
        UsageState::Limits(Limits { session: w.clone(), weekly: w, extra: vec![] })
    }

    struct Harness {
        fetcher: Option<Fetcher>,
        events: Arc<Mutex<Vec<Event>>>,
        fetches: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
    }

    fn start(
        orgs: &[&str],
        usage: Vec<(&str, Vec<Result<UsageState, FetchError>>)>,
        interval: Duration,
        preferred: Option<&str>,
        last_active: Option<&str>,
    ) -> Harness {
        let fetches = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let src = FakeSource {
            orgs: orgs.iter().map(|o| org(o)).collect(),
            usage: usage.into_iter().map(|(k, v)| (k.to_string(), v.into())).collect(),
            fetches: fetches.clone(),
            dropped: dropped.clone(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let fetcher = Fetcher::spawn_inner(
            Box::new(src),
            interval,
            Duration::ZERO,
            preferred.map(String::from),
            last_active.map(String::from),
            Box::new(move |e| ev.lock().unwrap().push(e)),
        );
        Harness { fetcher: Some(fetcher), events, fetches, dropped }
    }

    impl Harness {
        fn usage_events(&self) -> Vec<Event> {
            self.events.lock().unwrap().iter().filter(|e| !matches!(e, Event::Orgs(..))).cloned().collect()
        }
        fn wait_usage_events(&self, n: usize) -> Vec<Event> {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let ev = self.usage_events();
                if ev.len() >= n || Instant::now() > deadline {
                    return ev;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fn send(&self, c: Command) {
            self.fetcher.as_ref().unwrap().send(c);
        }
    }

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn picks_org_with_limits() {
        let h = start(&["A", "B"], vec![("A", vec![Ok(UsageState::NoLimits)]), ("B", vec![Ok(limits(10.0))])], LONG, None, Some("A"));
        assert_eq!(h.wait_usage_events(1), vec![Event::Usage(limits(10.0))]);
        let orgs = h.events.lock().unwrap()[0].clone();
        assert_eq!(orgs, Event::Orgs(vec![org("A"), org("B")], "B".into()));
    }

    #[test]
    fn last_active_used_when_it_has_limits() {
        let h = start(&["A", "B"], vec![("A", vec![Ok(limits(1.0))]), ("B", vec![Ok(limits(2.0))])], LONG, None, Some("B"));
        assert_eq!(h.wait_usage_events(1), vec![Event::Usage(limits(2.0))]);
    }

    #[test]
    fn preferred_org_wins() {
        let h = start(&["A", "B"], vec![("A", vec![Ok(limits(1.0))]), ("B", vec![Ok(limits(2.0))])], LONG, Some("B"), Some("A"));
        assert_eq!(h.wait_usage_events(1), vec![Event::Usage(limits(2.0))]);
        assert!(matches!(&h.events.lock().unwrap()[0], Event::Orgs(_, c) if c == "B"));
    }

    #[test]
    fn no_org_with_limits_reports_no_limits() {
        let h = start(&["A"], vec![("A", vec![Ok(UsageState::NoLimits)])], LONG, None, None);
        assert_eq!(h.wait_usage_events(1), vec![Event::Usage(UsageState::NoLimits)]);
    }

    #[test]
    fn refresh_now_fetches_once_more() {
        let h = start(&["A"], vec![("A", vec![Ok(limits(1.0)), Ok(limits(2.0))])], LONG, None, None);
        h.wait_usage_events(1);
        h.send(Command::RefreshNow);
        assert_eq!(h.wait_usage_events(2), vec![Event::Usage(limits(1.0)), Event::Usage(limits(2.0))]);
    }

    #[test]
    fn set_org_switches() {
        let h = start(&["A", "B"], vec![("A", vec![Ok(limits(1.0))]), ("B", vec![Ok(limits(2.0))])], LONG, Some("A"), None);
        h.wait_usage_events(1);
        h.send(Command::SetOrg("B".into()));
        assert_eq!(h.wait_usage_events(2)[1], Event::Usage(limits(2.0)));
    }

    #[test]
    fn polls_on_interval() {
        let h = start(&["A"], vec![("A", vec![Ok(limits(1.0))])], Duration::from_millis(20), None, None);
        assert!(h.wait_usage_events(3).len() >= 3);
    }

    #[test]
    fn network_error_then_recovers() {
        let net = Err(FetchError::Network("down".into()));
        let h = start(&["A"], vec![("A", vec![net.clone(), Ok(limits(5.0))])], LONG, None, None);
        assert_eq!(h.wait_usage_events(1), vec![Event::Error(FetchError::Network("down".into()))]);
        h.send(Command::RefreshNow);
        assert_eq!(h.wait_usage_events(2)[1], Event::Usage(limits(5.0)));
    }

    #[test]
    fn auth_error_stops_polling() {
        let h = start(&["A"], vec![("A", vec![Err(FetchError::Auth)])], Duration::from_millis(20), Some("A"), None);
        assert_eq!(h.wait_usage_events(1), vec![Event::Error(FetchError::Auth)]);
        std::thread::sleep(Duration::from_millis(200));
        h.send(Command::RefreshNow);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(h.fetches.load(Ordering::SeqCst), 1);
        assert_eq!(h.usage_events().len(), 1);
    }

    #[test]
    fn drop_joins_thread() {
        let mut h = start(&["A"], vec![("A", vec![Ok(limits(1.0))])], LONG, None, None);
        h.wait_usage_events(1);
        let t = Instant::now();
        drop(h.fetcher.take());
        assert!(t.elapsed() < Duration::from_secs(1));
        assert!(h.dropped.load(Ordering::SeqCst), "source not freed after drop");
    }
}
