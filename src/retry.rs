//! The webhook's one more try (A13).
//!
//! Pure and portable (std only). A POST that failed on the network or with a 5xx is tried once more
//! after a short wait; a 4xx never is (the URL or the payload is wrong and a second try says the same,
//! and a rate limit should not be hammered). The loop takes its sender and its sleep as arguments so
//! the tests run it without a network or a clock.

/// Why a POST failed, from `source::post`'s fixed error texts (`network`, `HTTP <code>`, `tls`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Network,
    Status(u16),
    /// Anything else (the TLS stack could not start): a second try would fail the same way.
    Other,
}

/// Read a failure from the error text. A webhook URL is never in these texts.
pub fn classify(err: &str) -> Failure {
    let err = err.trim();
    if err == "network" {
        return Failure::Network;
    }
    match err.strip_prefix("HTTP ").and_then(|c| c.trim().parse::<u16>().ok()) {
        Some(code) => Failure::Status(code),
        None => Failure::Other,
    }
}

/// Seconds to wait before the one more try.
pub const RETRY_AFTER_S: f64 = 3.0;
/// The most attempts after the first.
pub const MAX_RETRIES: u32 = 1;

/// After `attempt` (0 for the first try) failed with `why`: wait this long and try again, or `None`.
/// Retried: a network error and a 5xx, once. Never: a 4xx, a 3xx, anything else, or a second failure.
pub fn next(why: Failure, attempt: u32) -> Option<f64> {
    if attempt >= MAX_RETRIES {
        return None;
    }
    match why {
        Failure::Network => Some(RETRY_AFTER_S),
        Failure::Status(code) if (500..600).contains(&code) => Some(RETRY_AFTER_S),
        _ => None,
    }
}

/// Whether a post that started under webhook generation `started` may still send its retry when the
/// generation is `now`: the address it was for has not been removed or replaced since.
pub fn still_wanted(started: u64, now: u64) -> bool {
    started == now
}

/// Send, and send once more if [`next`] says so. Returns the last result and whether it took the
/// second try (`sent on retry`). `sleep` gets the seconds to wait. `wanted` is asked after each wait:
/// when it says no, the failure that caused the wait is the result and nothing more is sent (a webhook
/// removed during the wait must not get the request).
pub fn run_while(mut send: impl FnMut() -> Result<(), String>, mut sleep: impl FnMut(f64), mut wanted: impl FnMut() -> bool) -> (Result<(), String>, bool) {
    let mut attempt = 0;
    loop {
        match send() {
            Ok(()) => return (Ok(()), attempt > 0),
            Err(e) => match next(classify(&e), attempt) {
                Some(wait) => {
                    sleep(wait);
                    if !wanted() {
                        return (Err(e), attempt > 0);
                    }
                    attempt += 1;
                }
                None => return (Err(e), attempt > 0),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_text_is_read_into_a_failure() {
        assert_eq!(classify("network"), Failure::Network);
        assert_eq!(classify("HTTP 503"), Failure::Status(503));
        assert_eq!(classify("HTTP 404"), Failure::Status(404));
        assert_eq!(classify("tls"), Failure::Other);
        assert_eq!(classify("HTTP x"), Failure::Other);
    }

    #[test]
    fn never_on_a_4xx_once_on_a_5xx_or_a_network_error() {
        for code in [400u16, 401, 403, 404, 410, 429, 499, 301] {
            assert_eq!(next(Failure::Status(code), 0), None, "{code}");
        }
        for code in [500u16, 502, 503, 504, 599] {
            assert_eq!(next(Failure::Status(code), 0), Some(RETRY_AFTER_S), "{code}");
            assert_eq!(next(Failure::Status(code), 1), None, "only once: {code}");
        }
        assert_eq!(next(Failure::Network, 0), Some(RETRY_AFTER_S));
        assert_eq!(next(Failure::Network, 1), None);
        assert_eq!(next(Failure::Other, 0), None);
    }

    fn script(answers: &[&str]) -> (Result<(), String>, bool, usize, usize) {
        let mut i = 0;
        let mut waits = 0;
        let (r, retried) = run_while(
            || {
                let a = answers[i];
                i += 1;
                if a == "ok" { Ok(()) } else { Err(a.to_string()) }
            },
            |_| waits += 1,
            || true,
        );
        (r, retried, i, waits)
    }

    #[test]
    fn the_loop_sends_once_on_success_and_marks_a_retry_only_when_it_took_one() {
        assert_eq!(script(&["ok"]), (Ok(()), false, 1, 0));
        assert_eq!(script(&["HTTP 503", "ok"]), (Ok(()), true, 2, 1), "sent on retry");
        assert_eq!(script(&["network", "ok"]), (Ok(()), true, 2, 1));
    }

    /// Audit 2026-11, webhook-retry-after-remove: a retry waiting for its pause does not go to an
    /// address that was removed or replaced meanwhile, and the result is the first failure, never "sent".
    #[test]
    fn a_retry_is_skipped_when_the_webhook_changed_during_the_wait() {
        assert!(still_wanted(4, 4));
        assert!(!still_wanted(4, 5), "removed or replaced");
        let mut sends = 0;
        let generation = std::cell::Cell::new(7u64);
        let (r, retried) = run_while(
            || {
                sends += 1;
                Err("HTTP 503".to_string())
            },
            // The user removes the webhook while the post sleeps.
            |_| generation.set(generation.get() + 1),
            || still_wanted(7, generation.get()),
        );
        assert_eq!(sends, 1, "the removed address got nothing more");
        assert_eq!(r, Err("HTTP 503".to_string()));
        assert!(!retried, "not reported as sent on retry");
        // Unchanged: the retry goes and succeeds, as before.
        let mut sends = 0;
        let (r, retried) = run_while(
            || {
                sends += 1;
                if sends == 1 { Err("network".to_string()) } else { Ok(()) }
            },
            |_| {},
            || still_wanted(7, 7),
        );
        assert_eq!((r, retried, sends), (Ok(()), true, 2));
    }

    #[test]
    fn the_loop_gives_up_after_one_more_try_and_never_retries_a_4xx() {
        let (r, retried, sends, _) = script(&["HTTP 502", "HTTP 502", "ok"]);
        assert_eq!((r, retried, sends), (Err("HTTP 502".into()), true, 2));
        let (r, retried, sends, waits) = script(&["HTTP 404", "ok"]);
        assert_eq!((r, retried, sends, waits), (Err("HTTP 404".into()), false, 1, 0));
        let (_, _, sends, _) = script(&["tls", "ok"]);
        assert_eq!(sends, 1);
    }
}
