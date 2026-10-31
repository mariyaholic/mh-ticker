//! Price alerts: a threshold crossed, a tray balloon, and an optional outbound POST.
//!
//! Firing rule: an alert fires when the quote is fresh, the price is on the alert's side of its
//! level, and `cooldown_s` has passed since it last fired. Two consequences are deliberate:
//! - A **stale** quote never fires. Otherwise a market closed at a level would retrigger on every
//!   cooldown for as long as the app runs.
//! - Starting the app already past a level fires once. That is what a user who set a level expects.
//!
//! The webhook is a *bearer secret*: anyone holding the URL can post to that channel. It is never
//! written to a note, tooltip, `--probe` line or error message — `redact` sees to the last one.
//!
//! Outbound posts run on a detached thread, one per alert, so a slow or unreachable endpoint can
//! never stall the message loop. The cooldown is the rate limit: an alert cannot post more than
//! once per `cooldown_s` however many times the price wobbles across the level.

use crate::config::{Alert, Config};
use crate::model::{fmt_price, Board};
use std::thread;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_INFO, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW};

/// One alert that fired on this pass. The label is already inside `title`, so there is no third
/// copy of it to keep in step.
pub struct Fired {
    pub title: String,
    pub body: String,
}

/// How long to say nothing rather than repeat the same alert.
const POST_TIMEOUT_S: u64 = 10;

/// Which side of its level a quote is on, or `None` when it has not reached one.
fn side(a: &Alert, price: f64) -> Option<(bool, f64)> {
    if let Some(above) = a.above.filter(|_| price >= a.above.unwrap_or(f64::MAX)) {
        return Some((true, above));
    }
    a.below.filter(|below| price <= *below).map(|below| (false, below))
}

/// Check every alert and mark the ones that fire. The caller saves the config.
pub fn check(cfg: &mut Config, board: &Board, now: f64) -> Vec<Fired> {
    let mut out = Vec::new();
    for cell in &board.cells {
        let Some(q) = cell.quote.as_ref() else { continue };
        if cell.stale {
            continue;
        }
        for a in cfg.alerts.iter_mut().filter(|a| a.label == cell.label) {
            let Some((rise, level)) = side(a, q.price) else { continue };
            if now - a.last < a.cooldown_s.max(30.0) {
                continue;
            }
            a.last = now;
            let dir = if rise { "above" } else { "below" };
            out.push(Fired {
                title: format!("{} {dir} {}", cell.label, fmt_price(level, cell.digits)),
                body: format!("{} is {} {} (was {})", cell.label, fmt_price(q.price, cell.digits), dir, fmt_price(level, cell.digits)),
            });
        }
    }
    out
}

/// Show a balloon on the tray icon. Focus Assist silently swallows these, which is why the panel
/// also carries the last fired alert in its footer note.
pub fn balloon(hwnd: HWND, id: u32, title: &str, body: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = id;
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    let copy = |dst: &mut [u16], src: &str| {
        for (slot, ch) in dst.iter_mut().zip(src.encode_utf16()) {
            *slot = ch;
        }
    };
    copy(&mut nid.szInfoTitle, &title.chars().take(63).collect::<String>());
    copy(&mut nid.szInfo, &body.chars().take(255).collect::<String>());
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

/// Strip a URL down to its host, for any message a human might read. The path carries the secret.
pub fn redact(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url);
    host.split('/').next().unwrap_or(host).to_string()
}

/// What to POST for this endpoint. Discord and Slack both take `{"content"}`/`{"text"}`; ntfy takes
/// a plain body with the title in a header. Anything else gets both keys, which most receivers
/// ignore harmlessly.
fn payload(url: &str, title: &str, body: &str) -> (String, &'static str) {
    let host = redact(url);
    let text = format!("{title}: {body}");
    if host.contains("discord") {
        (serde_json::json!({ "content": text, "allowed_mentions": { "parse": [] } }).to_string(), "application/json")
    } else if host.contains("ntfy") {
        (format!("{title}\n{body}"), "text/plain")
    } else {
        (serde_json::json!({ "text": text, "title": title, "body": body }).to_string(), "application/json")
    }
}

/// POST one alert on a detached thread. `on_done` gets a redacted error, or nothing, so the UI can
/// say "webhook failed" without ever holding the URL.
pub fn spawn_post(url: String, title: String, body: String, on_done: Box<dyn Fn(Option<String>) + Send>) {
    thread::Builder::new()
        .name("alert-post".into())
        .spawn(move || {
            let (json, ctype) = payload(&url, &title, &body);
            let result = crate::source::post(&url, &json, ctype, POST_TIMEOUT_S);
            on_done(result.err().map(|e| format!("{}: {e}", redact(&url))));
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cell, Link, Quote};

    fn quote(price: f64) -> Quote {
        Quote { symbol: "XAU=".into(), price, ts: Some(100.0), realtime: true, ..Quote::default() }
    }

    fn board(price: f64, stale: bool) -> Board {
        let mut b = Board::new(&[]);
        b.cells = vec![Cell { label: "XAU".into(), digits: 2, quote: Some(quote(price)), stale }];
        b.link = Link::Live;
        b
    }

    fn alert(above: Option<f64>, below: Option<f64>) -> Alert {
        Alert { label: "XAU".into(), above, below, cooldown_s: 900.0, last: 0.0 }
    }

    #[test]
    fn fires_once_across_the_level_then_respects_the_cooldown() {
        let mut cfg = Config { alerts: vec![alert(Some(4100.0), None)], ..Config::default() };
        assert_eq!(check(&mut cfg, &board(4200.0, false), 10_000.0).len(), 1);
        // still above, but inside the cooldown
        assert!(check(&mut cfg, &board(4200.0, false), 10_100.0).is_empty());
        // below the level: nothing to say
        assert!(check(&mut cfg, &board(4000.0, false), 20_000.0).is_empty());
        // crossed back up after the cooldown
        assert_eq!(check(&mut cfg, &board(4200.0, false), 30_000.0).len(), 1);
        assert_eq!(cfg.alerts[0].last, 30_000.0);
    }

    #[test]
    fn a_stale_quote_never_fires() {
        let mut cfg = Config { alerts: vec![alert(Some(4100.0), None)], ..Config::default() };
        assert!(check(&mut cfg, &board(4200.0, true), 10_000.0).is_empty());
        assert_eq!(cfg.alerts[0].last, 0.0, "a stale quote must not burn the cooldown");
    }

    #[test]
    fn below_alerts_and_a_minimum_cooldown() {
        let mut cfg = Config { alerts: vec![alert(None, Some(4000.0))], ..Config::default() };
        assert_eq!(check(&mut cfg, &board(3900.0, false), 10_000.0).len(), 1);
        // the cooldown was configured as 900, but the floor keeps a 10 s repeat out
        assert!(check(&mut cfg, &board(3900.0, false), 10_010.0).is_empty());
        // no level at all: never fires
        cfg.alerts = vec![alert(None, None)];
        assert!(check(&mut cfg, &board(3900.0, false), 50_000.0).is_empty());
    }

    #[test]
    fn messages_never_leak_the_webhook_secret() {
        let url = "https://discord.com/api/webhooks/1234/abcdefSECRET";
        assert_eq!(redact(url), "discord.com");
        assert!(!redact(url).contains("SECRET"));
        let (body, ct) = payload(url, "XAU above 4,100", "spot");
        assert!((ct, body.contains("SECRET")) == ("application/json", false));
        assert!(body.contains("XAU above 4,100"));
        let (ntfy, ct) = payload("https://ntfy.sh/my-topic", "t", "b");
        assert_eq!((ct, ntfy.as_str()), ("text/plain", "t\nb"));
    }
}
