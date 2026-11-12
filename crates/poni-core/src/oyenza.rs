//! Signing in to ihz.app, optional: a device code, the way a TV signs in.
//! The island shows 8 letters, the person approves them at ihz.app/device
//! (signed in with Google there anyway), and the island gets a key of its
//! own (`ihzd_…`). The protocol is the one ihz.app documents in
//! `ihz-web/docs/device-login.md`: `POST /connect/start`, `/connect/token`
//! and `/connect/disconnect`.
//! No password is ever typed into the app. The key is what other services
//! (the IELTS coach) are asked with; this module only gets it, keeps it and
//! gives it back.
//!
//! Everything here is pure: request bodies and URLs and parsing the answers.
//! The HTTP and the sealed key live in the app crate (`services/oyenza.rs`).
//! (The module keeps the upstream project's name, to keep merges small.)

use crate::datetime::{days_from_civil, LocalDateTime};
use serde::{Deserialize, Serialize};

pub const API: &str = "https://ihz.app";
pub const WEB: &str = "https://ihz.app";
pub const APP: &str = "poni";
/// The account's name as people see it, and the page where a code is approved.
pub const NAME: &str = "ihz.app";
pub const APPROVE_PAGE: &str = "ihz.app/device";
/// Every device key starts so (48 characters in all).
pub const KEY_PREFIX: &str = "ihzd_";

/// Errors back off from a minute, doubling, up to this.
pub const MAX_BACKOFF_S: u64 = 15 * 60;

// ---------------------------------------------------------------- the link

/// What the PC keeps after signing in, sealed with Windows (DPAPI) under
/// `%LOCALAPPDATA%\Poni\` (on a Mac, a file only its account can read, in
/// `~/Library/Application Support/Poni/`). The key is never logged or shown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub key: String,
    /// The account, masked ("a•••@example.com"), to show which one it joined.
    pub account: String,
    pub device_id: String,
    pub device_name: String,
}

pub fn parse_link(s: &str) -> Option<Link> {
    let l: Link = serde_json::from_str(s).ok()?;
    (l.key.starts_with(KEY_PREFIX) && l.key.len() == 48).then_some(l)
}

pub fn link_json(l: &Link) -> String {
    serde_json::to_string(l).unwrap_or_default()
}

// ---------------------------------------------------------------- requests

pub fn start_url() -> String {
    format!("{API}/connect/start")
}

pub fn token_url() -> String {
    format!("{API}/connect/token")
}

pub fn disconnect_url() -> String {
    format!("{API}/connect/disconnect")
}

/// `platform`: [`PLATFORM`], so ihz.app shows a Mac as one.
pub fn start_body(pc_name: &str, version: &str, platform: &str) -> String {
    serde_json::json!({ "app": APP, "name": pc_name, "version": version, "platform": platform }).to_string()
}

/// The system this app runs on, as the sign-in names it.
pub const PLATFORM: &str = if cfg!(target_os = "macos") { "macos" } else { "windows" };

pub fn token_body(device_code: &str) -> String {
    serde_json::json!({ "deviceCode": device_code }).to_string()
}

/// What ihz.app lists a computer as when the system gives no name.
const UNNAMED: &str = if cfg!(target_os = "macos") { "Mac" } else { "Windows PC" };

/// The PC's name as ihz.app will list it: one printable line, 40 characters
/// at most ([`UNNAMED`] when the system gives none).
pub fn pc_name(raw: &str) -> String {
    let clean: String = raw.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = clean.chars().take(40).collect();
    if cut.trim().is_empty() {
        UNNAMED.into()
    } else {
        cut.trim().to_string()
    }
}

/// The local zone as a UTC offset, the way the server reads days in it
/// ("+07:00", "-03:30"). Windows names zones its own way; an offset is
/// unambiguous for "today" and is all the server needs.
pub fn tz_param(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let m = offset_minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Local wall time minus UTC, in minutes, rounded to the quarter hour.
pub fn offset_minutes(local: &LocalDateTime, unix: i64) -> i32 {
    let local_s = days_from_civil(local.year, local.month, local.day) * 86_400
        + local.hour as i64 * 3600
        + local.minute as i64 * 60
        + local.second as i64;
    let raw = (local_s - unix) as f64 / 60.0;
    ((raw / 15.0).round() * 15.0) as i32
}

// ---------------------------------------------------------------- answers

/// The server's error code (`{"error": "…"}`), if the body has one.
pub fn error_code(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct E {
        error: String,
    }
    serde_json::from_str::<E>(body).ok().map(|e| e.error)
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub device_code: String,
    pub user_code: String,
    pub verify_url_complete: String,
    pub expires_in: u64,
    #[serde(default = "five")]
    pub interval: u64,
}

fn five() -> u64 {
    5
}

pub fn parse_started(body: &str) -> Option<Started> {
    serde_json::from_str(body).ok()
}

/// What asking for the key answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Claim {
    Waiting,
    Connected(Link),
    SlowDown,
    Expired,
    Denied,
    /// The server no longer knows the code (used, or never made).
    Invalid,
}

pub fn parse_claim(status: u16, body: &str) -> Option<Claim> {
    #[derive(Deserialize)]
    struct Device {
        id: String,
        name: String,
    }
    #[derive(Deserialize)]
    struct Ok {
        status: String,
        key: Option<String>,
        device: Option<Device>,
        #[serde(default)]
        account: String,
    }
    if status == 200 {
        let ok: Ok = serde_json::from_str(body).ok()?;
        return match (ok.status.as_str(), ok.key, ok.device) {
            ("waiting", _, _) => Some(Claim::Waiting),
            ("connected", Some(key), Some(d)) => {
                let link = Link { key, account: ok.account, device_id: d.id, device_name: d.name };
                parse_link(&link_json(&link)).map(Claim::Connected)
            }
            _ => None,
        };
    }
    match error_code(body).as_deref() {
        Some("slow_down") => Some(Claim::SlowDown),
        Some("expired") => Some(Claim::Expired),
        Some("denied") => Some(Claim::Denied),
        Some("invalid_code") => Some(Claim::Invalid),
        _ => None,
    }
}

// ---------------------------------------------------------------- cadence

/// Wait after `fails` failed calls in a row: 1, 2, 4 … 15 minutes, never
/// shorter than the server's `Retry-After`.
pub fn backoff_s(fails: u32, retry_after_s: Option<u64>) -> u64 {
    let doubling = 60u64.saturating_mul(1u64 << fails.saturating_sub(1).min(10));
    doubling.min(MAX_BACKOFF_S).max(retry_after_s.unwrap_or(0).min(MAX_BACKOFF_S))
}

// ---------------------------------------------------------------- words

/// Title and subtitle when signing in stops without a key.
pub fn connect_failure_text(code: &str) -> (&'static str, &'static str) {
    match code {
        "expired" | "invalid_code" => ("Sign-in code expired", "Start again from the tray"),
        "denied" => ("Not connected", "The code was refused on ihz.app"),
        "offline" => ("Can't reach ihz.app", "Check the connection and try again"),
        _ => ("Couldn't sign in to ihz.app", "Try again from the tray"),
    }
}

/// "9:41" left on a sign-in code.
pub fn code_time_left(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_bodies() {
        assert_eq!(start_url(), "https://ihz.app/connect/start");
        assert_eq!(token_url(), "https://ihz.app/connect/token");
        assert_eq!(disconnect_url(), "https://ihz.app/connect/disconnect");
        let b: serde_json::Value = serde_json::from_str(&start_body("DESKTOP-7Q2K", "0.1.5", "windows")).unwrap();
        assert_eq!(b, serde_json::json!({ "app": "poni", "name": "DESKTOP-7Q2K", "version": "0.1.5", "platform": "windows" }));
        assert_eq!(PLATFORM, if cfg!(target_os = "macos") { "macos" } else { "windows" });
        assert_eq!(pc_name("  DESKTOP-7Q2K\u{0}\n "), "DESKTOP-7Q2K");
        assert_eq!(pc_name(""), UNNAMED);
        assert_eq!(pc_name(&"x".repeat(60)).len(), 40);
    }

    #[test]
    fn zone_offsets() {
        assert_eq!(tz_param(420), "+07:00");
        assert_eq!(tz_param(-210), "-03:30");
        assert_eq!(tz_param(0), "+00:00");
        let local = LocalDateTime { year: 2026, month: 10, day: 24, hour: 14, minute: 38, second: 20 };
        // 2026-10-24 07:38:20 UTC
        let unix = days_from_civil(2026, 10, 24) * 86_400 + 7 * 3600 + 38 * 60 + 20;
        assert_eq!(offset_minutes(&local, unix), 420);
        // A second off either way still rounds to the zone.
        assert_eq!(offset_minutes(&local, unix + 1), 420);
        let india = LocalDateTime { hour: 13, minute: 8, ..local };
        assert_eq!(offset_minutes(&india, unix), 330);
    }

    #[test]
    fn claims() {
        assert_eq!(parse_claim(200, r#"{"status":"waiting"}"#), Some(Claim::Waiting));
        let key = format!("ihzd_{}", "A".repeat(43));
        let body = format!(r#"{{"status":"connected","key":"{key}","device":{{"id":"d1","name":"DESKTOP-7Q2K"}},"account":"a•••@example.com"}}"#);
        assert_eq!(
            parse_claim(200, &body),
            Some(Claim::Connected(Link { key, account: "a•••@example.com".into(), device_id: "d1".into(), device_name: "DESKTOP-7Q2K".into() }))
        );
        assert_eq!(parse_claim(200, r#"{"status":"connected","key":"nope","device":{"id":"d","name":"x"}}"#), None);
        assert_eq!(parse_claim(429, r#"{"error":"slow_down"}"#), Some(Claim::SlowDown));
        assert_eq!(parse_claim(410, r#"{"error":"expired"}"#), Some(Claim::Expired));
        assert_eq!(parse_claim(403, r#"{"error":"denied"}"#), Some(Claim::Denied));
        assert_eq!(parse_claim(404, r#"{"error":"invalid_code"}"#), Some(Claim::Invalid));
        assert_eq!(parse_claim(500, "oops"), None);
        assert_eq!(parse_link(r#"{"key":"oyzc_x","account":"","device_id":"","device_name":""}"#), None);
        // An Oyenza key from before the fork is not one of ours.
        let old = format!(r#"{{"key":"oyzd_{}","account":"","device_id":"","device_name":""}}"#, "A".repeat(43));
        assert_eq!(parse_link(&old), None);
    }

    #[test]
    fn claims_and_backoff() {
        assert_eq!((1..=6).map(|n| backoff_s(n, None)).collect::<Vec<_>>(), [60, 120, 240, 480, 900, 900]);
        assert_eq!(backoff_s(1, Some(300)), 300);
        assert_eq!(backoff_s(1, Some(99_999)), 900);
        assert_eq!(backoff_s(40, None), 900);
        assert_eq!(error_code(r#"{"error":"denied"}"#).as_deref(), Some("denied"));
        assert_eq!(error_code("<html>"), None);
        assert_eq!(code_time_left(581.2), "9:42");
    }
}
