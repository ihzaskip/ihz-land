//! Signing in to the account, optional: a device code, then the key the
//! other services ask with (the IELTS coach). URLs, bodies and parsing live
//! in `poni_core::oyenza`; this file does the HTTP on one worker thread and
//! keeps the key with `services::secret` at `paths::oyenza_link_path()`:
//! sealed with DPAPI on Windows, a file only the Mac account can read on a
//! Mac (which shares this file as is).
//!
//! Nothing runs until the person signs in (Settings or the tray), and even
//! then nothing is asked on a timer: the worker only polls for the key while
//! a sign-in code is on screen, and retries a disconnect that couldn't reach
//! the server (hourly, while the PC is in use). The thread sleeps on a
//! condition variable between calls, so the island's idle stays at zero CPU.
//! Calls block this thread only (8 s budget).

use crate::commands::OyenzaCmd;
use crate::events::{AppEvent, EventSink, OyenzaEvent};
use crate::services::http::{Client, FetchError, Method, MtaGuard, Response};
use poni_core::oyenza::{self, Claim, Link};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const USER_AGENT: &str = concat!("ihzland/", env!("CARGO_PKG_VERSION"), " (+https://github.com/ihzaskip/oyenza-studio)");
const TIMEOUT: Duration = Duration::from_secs(8);
/// A disconnect that couldn't reach the server is retried this often.
const REVOKE_EVERY: Duration = Duration::from_secs(3600);

/// What the account lists this computer as: the PC's name, or the Mac's as
/// System Settings › General › About shows it.
#[cfg(windows)]
fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

#[cfg(not(windows))]
fn computer_name() -> String {
    crate::services::phonedrop::computer_name().unwrap_or_default()
}

pub struct OyenzaService {
    shared: Arc<Shared>,
}

impl OyenzaService {
    pub fn start(sink: EventSink, activity: Activity) -> std::io::Result<Self> {
        let shared = Arc::new(Shared { q: Mutex::new(Queue { activity, ..Default::default() }), cv: Condvar::new() });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("poni-oyenza".into())
            .spawn(move || worker_main(worker, sink))
            .map_err(|e| std::io::Error::other(format!("could not start the sign-in thread: {e}")))?;
        Ok(Self { shared })
    }

    /// Queue a command and return at once (UI thread safe).
    pub fn send(&self, cmd: OyenzaCmd) {
        self.shared.lock().cmds.push_back(cmd);
        self.shared.cv.notify_all();
    }

    /// The PC's use and power changed: the worker paces itself by them.
    pub fn set_activity(&self, a: Activity) {
        let mut q = self.shared.lock();
        if q.activity != a {
            q.activity = a;
            drop(q);
            self.shared.cv.notify_all();
        }
    }
}

impl Drop for OyenzaService {
    fn drop(&mut self) {
        // The worker exits at its next wait; a call in flight ends on its own
        // timeout. Never join from the UI thread.
        self.shared.lock().shutdown = true;
        self.shared.cv.notify_all();
    }
}

/// What paces the calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// Unlocked with the display on.
    pub in_use: bool,
}

#[derive(Default)]
struct Queue {
    cmds: VecDeque<OyenzaCmd>,
    activity: Activity,
    shutdown: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

impl Shared {
    /// The queue holds plain data, so a panic elsewhere can't leave it
    /// inconsistent: recover from poisoning instead of panicking.
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.q.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wait_for<'a>(&self, g: MutexGuard<'a, Queue>, timeout: Option<Duration>) -> MutexGuard<'a, Queue> {
        match timeout {
            Some(d) => match self.cv.wait_timeout(g, d) {
                Ok((g, _)) => g,
                Err(p) => p.into_inner().0,
            },
            None => self.cv.wait(g).unwrap_or_else(|p| p.into_inner()),
        }
    }
}

// ----------------------------------------------------------------- worker

/// A sign-in code on screen, waiting for the person's approval.
struct Pending {
    device_code: String,
    until: Instant,
    next_ask: Instant,
    every: Duration,
}

struct Worker {
    sink: EventSink,
    client: Option<Client>,
    link: Option<Link>,
    pending: Option<Pending>,
    /// A key whose disconnect couldn't reach the server yet.
    revoke: Option<String>,
    next_revoke: Instant,
}

fn worker_main(shared: Arc<Shared>, sink: EventSink) {
    let _com = MtaGuard::new();
    let now = Instant::now();
    let mut w = Worker {
        sink,
        client: None,
        link: None,
        pending: None,
        revoke: None,
        next_revoke: now,
    };
    w.restore();
    loop {
        let (cmds, activity) = {
            let mut q = shared.lock();
            loop {
                if q.shutdown {
                    return;
                }
                if !q.cmds.is_empty() {
                    break;
                }
                let now = Instant::now();
                match w.next_due(q.activity) {
                    Some(t) if t <= now => break,
                    Some(t) => q = shared.wait_for(q, Some(t - now)),
                    None => q = shared.wait_for(q, None),
                }
            }
            (q.cmds.drain(..).collect::<Vec<_>>(), q.activity)
        };
        for c in cmds {
            w.run(c);
        }
        w.due(activity);
    }
}

impl Worker {
    fn send(&self, ev: OyenzaEvent) {
        self.sink.send(AppEvent::Oyenza(ev));
    }

    fn client(&mut self) -> Result<&Client, FetchError> {
        if self.client.is_none() {
            self.client = Some(Client::new(USER_AGENT)?);
        }
        Ok(self.client.as_ref().expect("client was just made"))
    }

    fn call(&mut self, method: Method, url: &str, key: Option<&str>, json: Option<&str>, timeout: Duration) -> Result<Response, FetchError> {
        let auth = key.map(|k| format!("Bearer {k}"));
        let headers: Vec<(&str, &str)> = auth.as_deref().map(|a| vec![("Authorization", a)]).unwrap_or_default();
        self.client()?.send(method, url, &headers, json, timeout)
    }

    /// The sign-in saved on this PC, and a disconnect still to tell the server.
    fn restore(&mut self) {
        self.link = crate::services::secret::load(&crate::paths::oyenza_link_path()).and_then(|s| oyenza::parse_link(&s));
        if let Some(l) = &self.link {
            crate::pinfo!("oyenza: signed in (this PC is {})", l.device_name);
            self.send(OyenzaEvent::Restored { account: l.account.clone() });
        }
        self.revoke = crate::services::secret::load(&crate::paths::oyenza_revoke_path()).filter(|k| k.starts_with(oyenza::KEY_PREFIX));
    }

    /// When the worker next has something to do on its own.
    fn next_due(&mut self, a: Activity) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut consider = |t: Instant| next = Some(next.map_or(t, |n| n.min(t)));
        if let Some(p) = &self.pending {
            consider(p.next_ask.min(p.until));
        }
        if self.revoke.is_some() && a.in_use {
            consider(self.next_revoke);
        }
        next
    }

    fn run(&mut self, cmd: OyenzaCmd) {
        match cmd {
            OyenzaCmd::SignIn => self.sign_in(),
            OyenzaCmd::Cancel => {
                if self.pending.take().is_some() {
                    crate::pinfo!("oyenza: sign-in cancelled");
                    self.send(OyenzaEvent::ConnectCancelled);
                }
            }
            OyenzaCmd::Disconnect => self.disconnect(),
        }
    }

    fn due(&mut self, a: Activity) {
        let now = Instant::now();
        if self.pending.as_ref().is_some_and(|p| now >= p.until) {
            self.pending = None;
            self.send(OyenzaEvent::ConnectFailed("expired".into()));
        } else if self.pending.as_ref().is_some_and(|p| now >= p.next_ask) {
            self.ask_for_key();
        }
        if self.revoke.is_some() && a.in_use && Instant::now() >= self.next_revoke {
            self.retry_revoke();
        }
    }

    // ------------------------------------------------------------ signing in

    fn sign_in(&mut self) {
        if let Some(l) = &self.link {
            // Already signed in: say so again.
            self.send(OyenzaEvent::Restored { account: l.account.clone() });
            return;
        }
        let name = oyenza::pc_name(&computer_name());
        let body = oyenza::start_body(&name, env!("CARGO_PKG_VERSION"), oyenza::PLATFORM);
        match self.call(Method::Post, &oyenza::start_url(), None, Some(&body), TIMEOUT) {
            Ok(r) if r.status == 200 => match oyenza::parse_started(&r.body) {
                Some(s) => {
                    let now = Instant::now();
                    let every = Duration::from_secs(s.interval.clamp(2, 30));
                    self.pending =
                        Some(Pending { device_code: s.device_code, until: now + Duration::from_secs(s.expires_in.clamp(30, 1800)), next_ask: now + every, every });
                    crate::pinfo!("oyenza: showing a sign-in code");
                    self.send(OyenzaEvent::Connecting { code: s.user_code, url: s.verify_url_complete, expires_in_s: s.expires_in });
                }
                None => self.sign_in_failed("other", "unreadable answer"),
            },
            Ok(r) => self.sign_in_failed("other", &format!("HTTP {}", r.status)),
            Err(e) => self.sign_in_failed("offline", &e.to_string()),
        }
    }

    fn sign_in_failed(&mut self, code: &str, detail: &str) {
        crate::pwarn!("oyenza: sign-in didn't start ({detail})");
        self.send(OyenzaEvent::ConnectFailed(code.into()));
    }

    fn ask_for_key(&mut self) {
        let Some(device_code) = self.pending.as_ref().map(|p| p.device_code.clone()) else { return };
        let body = oyenza::token_body(&device_code);
        let answer = self.call(Method::Post, &oyenza::token_url(), None, Some(&body), TIMEOUT);
        let now = Instant::now();
        let claim = match &answer {
            Ok(r) => oyenza::parse_claim(r.status, &r.body),
            Err(_) => None,
        };
        match claim {
            Some(Claim::Connected(link)) => {
                self.pending = None;
                if let Err(e) = crate::services::secret::save(&crate::paths::oyenza_link_path(), &oyenza::link_json(&link)) {
                    // Still signed in for this run; it just won't survive a restart.
                    crate::pwarn!("oyenza: cannot save the sign-in ({e})");
                }
                crate::pinfo!("oyenza: signed in (this PC is {})", link.device_name);
                self.send(OyenzaEvent::Connected { account: link.account.clone() });
                self.link = Some(link);
            }
            Some(Claim::Waiting) => self.ask_again(now, false),
            Some(Claim::SlowDown) => self.ask_again(now, true),
            Some(Claim::Expired | Claim::Invalid) => {
                self.pending = None;
                self.send(OyenzaEvent::ConnectFailed("expired".into()));
            }
            Some(Claim::Denied) => {
                self.pending = None;
                crate::pinfo!("oyenza: the sign-in code was refused on ihz.app");
                self.send(OyenzaEvent::ConnectFailed("denied".into()));
            }
            // A network hiccup or a server error: keep asking until the code runs out.
            None => self.ask_again(now, false),
        }
    }

    fn ask_again(&mut self, now: Instant, slower: bool) {
        if let Some(p) = self.pending.as_mut() {
            if slower {
                p.every += Duration::from_secs(5);
            }
            p.next_ask = now + p.every;
        }
    }

    fn disconnect(&mut self) {
        self.pending = None;
        let Some(link) = self.link.take() else {
            self.send(OyenzaEvent::Disconnected { by_server: false });
            return;
        };
        let told = matches!(self.call(Method::Post, &oyenza::disconnect_url(), Some(&link.key), Some("{}"), TIMEOUT), Ok(r) if r.status == 200 || r.status == 401);
        if !told {
            // Keep the key only to tell the server later; it's no longer used for anything else.
            crate::pwarn!("oyenza: couldn't reach ihz.app to disconnect; will retry");
            let _ = crate::services::secret::save(&crate::paths::oyenza_revoke_path(), &link.key);
            self.revoke = Some(link.key.clone());
            self.next_revoke = Instant::now() + REVOKE_EVERY;
        }
        self.forget();
        crate::pinfo!("oyenza: disconnected from this PC");
        self.send(OyenzaEvent::Disconnected { by_server: false });
    }

    fn retry_revoke(&mut self) {
        let Some(key) = self.revoke.clone() else { return };
        let done = matches!(self.call(Method::Post, &oyenza::disconnect_url(), Some(&key), Some("{}"), TIMEOUT), Ok(r) if r.status == 200 || r.status == 401);
        if done {
            let _ = crate::services::secret::delete(&crate::paths::oyenza_revoke_path());
            self.revoke = None;
            crate::pinfo!("oyenza: the earlier disconnect reached ihz.app");
        } else {
            self.next_revoke = Instant::now() + REVOKE_EVERY;
        }
    }

    /// Drop the sign-in.
    fn forget(&mut self) {
        self.link = None;
        if let Err(e) = crate::services::secret::delete(&crate::paths::oyenza_link_path()) {
            crate::pwarn!("oyenza: cannot remove the saved sign-in ({e})");
        }
    }
}
