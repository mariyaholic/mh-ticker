//! The poll governor: how long the quote feed waits between polls.
//!
//! Pure and portable (std only). The feed asks [`interval`] after every poll, and the stale rule asks
//! [`stale_after`], so a slower poll can never make a quote look stale just because the app chose to
//! look less often.
//!
//! The rules, in the order they apply:
//! - Quiet: no shown quote changed for 10 minutes, poll every 15 s; for 30 minutes, every 60 s.
//! - Locked session: one poll per 5 minutes.
//! - Armed: while any alert is armed the answer is never slower than 15 s (or the user's own base,
//!   when that is already slower). This rule wins over the two above.
//! - Both settings off (the Classic default) the answer is the base, for every input.
//!
//! A change, an expand, the hotkey, an unlock and *Refresh now* all end the wait through the
//! feed's `kick` and restart the quiet clock, so the first poll after any of them is at the base.

/// Quiet this long and the feed polls every [`QUIET_SLOW_S`] seconds.
pub const QUIET_AFTER_S: f64 = 600.0;
/// Quiet this long and the feed polls every [`QUIET_SLOWER_S`] seconds.
pub const QUIET_LONG_AFTER_S: f64 = 1_800.0;
pub const QUIET_SLOW_S: f64 = 15.0;
pub const QUIET_SLOWER_S: f64 = 60.0;
/// One poll per this many seconds while the session is locked and nothing is armed.
pub const LOCKED_S: f64 = 300.0;
/// The slowest an armed alert is ever polled (unless the user's base is already slower).
pub const ARMED_CAP_S: f64 = 15.0;

/// The two menu ticks. Ultra turns both on by default, Classic neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub slow_when_quiet: bool,
    pub rest_when_locked: bool,
}

impl Settings {
    pub fn any(self) -> bool {
        self.slow_when_quiet || self.rest_when_locked
    }
}

/// Seconds to wait before the next poll. `base` is the user's `Poll every` choice, `quiet_for` the
/// seconds since any shown quote last changed, `locked` whether the session is locked, `armed`
/// whether any alert is set.
pub fn interval(base: f64, quiet_for: f64, locked: bool, armed: bool, set: Settings) -> f64 {
    let base = if base.is_finite() && base > 0.0 { base } else { 3.0 };
    if !set.any() {
        return base;
    }
    let mut t = base;
    if set.slow_when_quiet && quiet_for.is_finite() {
        if quiet_for >= QUIET_LONG_AFTER_S {
            t = t.max(QUIET_SLOWER_S);
        } else if quiet_for >= QUIET_AFTER_S {
            t = t.max(QUIET_SLOW_S);
        }
    }
    if set.rest_when_locked && locked {
        t = t.max(LOCKED_S);
    }
    if armed {
        t = t.min(base.max(ARMED_CAP_S));
    }
    t
}

/// The age at which a quote is stale, given the governed interval in force (`0` or less means the
/// governor is not in charge: the configured value is returned unchanged). A quote one interval old
/// is never stale and one three intervals old always is: the limit is twice the interval, or the
/// configured value when that is larger.
pub fn stale_after(stale_after_s: f64, governed_s: f64) -> f64 {
    if governed_s.is_finite() && governed_s > 0.0 {
        stale_after_s.max(2.0 * governed_s)
    } else {
        stale_after_s
    }
}

/// Watches whether any shown price changed, on the feed's own monotonic seconds.
#[derive(Clone, Debug, Default)]
pub struct Quiet {
    prices: Vec<f64>,
    since: Option<f64>,
}

impl Quiet {
    /// Feed one poll's prices (NaN for a row with none). Returns how long nothing has changed.
    /// A different list, a different length or any different price restarts the clock at `now`.
    pub fn observe(&mut self, now: f64, prices: &[f64]) -> f64 {
        let same = self.since.is_some()
            && self.prices.len() == prices.len()
            && self.prices.iter().zip(prices).all(|(a, b)| a == b || (a.is_nan() && b.is_nan()));
        if !same {
            self.prices = prices.to_vec();
            self.since = Some(now);
        }
        self.quiet_for(now)
    }

    /// Seconds since the last change, 0 before the first poll. A clock that went backwards reads as 0.
    pub fn quiet_for(&self, now: f64) -> f64 {
        self.since.map_or(0.0, |s| (now - s).max(0.0))
    }

    /// Back to base: a change of mind, an expand, the hotkey, an unlock or *Refresh now*.
    pub fn reset(&mut self, now: f64) {
        self.since = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Settings = Settings { slow_when_quiet: true, rest_when_locked: true };
    const OFF: Settings = Settings { slow_when_quiet: false, rest_when_locked: false };
    const QUIET_TIMES: [f64; 9] = [0.0, 1.0, 599.0, 600.0, 1_000.0, 1_799.0, 1_800.0, 86_400.0, f64::NAN];

    #[test]
    fn the_governor_never_exceeds_fifteen_seconds_while_armed() {
        for base in [1.0, 2.0, 3.0, 5.0, 10.0] {
            for q in QUIET_TIMES {
                for locked in [false, true] {
                    for set in [ON, OFF, Settings { slow_when_quiet: true, ..OFF }, Settings { rest_when_locked: true, ..OFF }] {
                        let t = interval(base, q, locked, true, set);
                        assert!(t <= 15.0, "{t} s with base {base}, quiet {q}, locked {locked}");
                        assert!(t >= base);
                    }
                }
            }
        }
        // A base the user chose above 15 s is theirs: the governor adds nothing to it.
        assert_eq!(interval(30.0, 5_000.0, true, true, ON), 30.0);
        assert_eq!(interval(60.0, 0.0, false, true, ON), 60.0);
    }

    #[test]
    fn quiet_time_steps_the_poll_to_15_then_60_seconds() {
        let at = |q| interval(3.0, q, false, false, ON);
        assert_eq!(at(0.0), 3.0);
        assert_eq!(at(599.9), 3.0);
        assert_eq!(at(600.0), 15.0);
        assert_eq!(at(1_799.9), 15.0);
        assert_eq!(at(1_800.0), 60.0);
        assert_eq!(at(f64::NAN), 3.0, "a clock that makes no sense is the base");
        assert_eq!(at(-5.0), 3.0);
        // Only the quiet tick: a locked session is not rested.
        let only_quiet = Settings { slow_when_quiet: true, rest_when_locked: false };
        assert_eq!(interval(3.0, 0.0, true, false, only_quiet), 3.0);
    }

    #[test]
    fn a_locked_session_with_nothing_armed_polls_once_in_five_minutes() {
        assert_eq!(interval(3.0, 0.0, true, false, ON), 300.0);
        assert_eq!(interval(3.0, 5_000.0, true, false, ON), 300.0, "locked and quiet: the longer rest");
        assert_eq!(interval(3.0, 0.0, false, false, ON), 3.0, "unlocked is the base at once");
        let only_lock = Settings { slow_when_quiet: false, rest_when_locked: true };
        assert_eq!(interval(3.0, 5_000.0, false, false, only_lock), 3.0);
        assert_eq!(interval(3.0, 0.0, true, true, ON), 15.0, "locked but armed");
    }

    #[test]
    fn with_both_settings_off_every_input_gives_the_base() {
        for base in [1.0, 2.0, 3.0, 5.0, 10.0, 30.0, 60.0] {
            for q in QUIET_TIMES {
                for locked in [false, true] {
                    for armed in [false, true] {
                        assert_eq!(interval(base, q, locked, armed, OFF), base);
                    }
                }
            }
        }
    }

    #[test]
    fn the_stale_limit_follows_the_governed_interval() {
        use crate::model::{is_stale, Quote};
        let quote = |age: f64| Quote { ts: Some(1_000.0 - age), realtime: true, ..Quote::default() };
        let stale = |iv: f64, age: f64| is_stale(&quote(age), 1_000.0, stale_after(90.0, iv), 900.0);
        for iv in [60.0, 300.0] {
            assert!(!stale(iv, iv), "one interval old at {iv} s");
            assert!(stale(iv, 3.0 * iv), "three intervals old at {iv} s");
        }
        // Not governed: the configured 90 s, exactly as in 1.8.
        assert!(!stale(0.0, 90.0));
        assert!(stale(0.0, 91.0));
        // The configured value is a floor: at 15 s the limit stays 90 s.
        assert_eq!(stale_after(90.0, 15.0), 90.0);
        assert_eq!(stale_after(90.0, f64::NAN), 90.0);
    }

    #[test]
    fn the_quiet_clock_restarts_on_any_change_and_on_a_wake() {
        let mut q = Quiet::default();
        assert_eq!(q.observe(0.0, &[1.0, 2.0]), 0.0);
        assert_eq!(q.observe(100.0, &[1.0, 2.0]), 100.0);
        assert_eq!(q.observe(700.0, &[1.0, 2.0]), 700.0);
        assert_eq!(q.observe(710.0, &[1.0, 2.5]), 0.0, "one price moved: back to base");
        assert_eq!(q.observe(720.0, &[1.0, 2.5, 3.0]), 0.0, "a new row is a change");
        assert_eq!(q.observe(800.0, &[f64::NAN]), 0.0);
        assert_eq!(q.observe(900.0, &[f64::NAN]), 100.0, "NaN equals NaN here");
        q.reset(950.0);
        assert_eq!(q.quiet_for(960.0), 10.0);
        assert_eq!(q.quiet_for(900.0), 0.0, "a clock set back reads as 0");
    }

    /// A scripted feed: a market that moves for 5 minutes, then goes quiet for two hours. Counts the
    /// polls the governor makes against the base, the figure the notes quote.
    #[test]
    fn a_quiet_two_hours_costs_far_fewer_polls() {
        let simulate = |set: Settings, armed: bool| {
            let (mut t, mut polls, mut q) = (0.0f64, 0u32, Quiet::default());
            while t < 2.0 * 3600.0 {
                polls += 1;
                // The price moves on every poll for the first 300 s, then never again.
                let price = if t < 300.0 { t } else { 300.0 };
                let quiet = q.observe(t, &[price]);
                t += interval(3.0, quiet, false, armed, set);
            }
            polls
        };
        let base = simulate(OFF, false);
        let governed = simulate(ON, false);
        let armed = simulate(ON, true);
        println!("polls in 2 h of a quiet market: base {base}, governed {governed}, governed and armed {armed}");
        assert_eq!(base, 2_400);
        assert!(governed < 500, "{governed}");
        assert!(armed > governed && armed < base, "{armed}");
    }
}
