//! The Kuis page's one call: a question from ihz.app (`poni_core::kuis`).
//!
//! Nothing here exists at rest: each question is a thread of its own
//! (`poni-kuis`) that asks, reports and ends. The call carries the PC's
//! ihz.app key, read from its sealed file on that thread (`services::secret`),
//! like the IELTS worker's: it never reaches the UI thread or the log.
//!
//! `PONI_KUIS_URL` asks another server instead (a local ihz-web, a preview),
//! with the same key.

use crate::events::{AppEvent, EventSink};
use crate::services::http::{Client, MtaGuard};
use poni_core::kuis::{self, Problem};
use poni_core::oyenza;
use std::time::Duration;

const USER_AGENT: &str = concat!("ihzland/", env!("CARGO_PKG_VERSION"), " (+https://github.com/ihzaskip/ihz-land)");
/// ihz.app draws the formulas first (~1 ms each), so a question is quick.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Ask for a question; the answer comes as `AppEvent::Kuis`.
pub fn fetch(sink: EventSink) {
    let spawned = std::thread::Builder::new().name("poni-kuis".into()).spawn({
        let sink = sink.clone();
        move || {
            let _com = MtaGuard::new();
            let got = ask();
            if let Err(p) = &got {
                crate::pwarn!("kuis: no question ({p:?})");
            }
            sink.send(AppEvent::Kuis(Box::new(got)));
        }
    });
    if let Err(e) = spawned {
        crate::pwarn!("kuis: no thread ({e})");
        sink.send(AppEvent::Kuis(Box::new(Err(Problem::Server))));
    }
}

fn ask() -> Result<kuis::Question, Problem> {
    let key = crate::services::secret::load(&crate::paths::oyenza_link_path()).and_then(|s| oyenza::parse_link(&s)).map(|l| l.key).ok_or(Problem::SignedOut)?;
    let url = std::env::var("PONI_KUIS_URL").unwrap_or_else(|_| kuis::url(oyenza::API));
    // Private: no redirect takes the key to another host, nothing cached.
    let client = Client::private(USER_AGENT).map_err(|_| Problem::Offline)?;
    let auth = format!("Bearer {key}");
    match client.get_with_headers(&url, &[("Authorization", &auth)], TIMEOUT) {
        Ok(r) if r.status == 200 => kuis::parse(&r.body).ok_or(Problem::Server),
        Ok(r) => Err(Problem::from_status(r.status)),
        Err(_) => Err(Problem::Offline),
    }
}
