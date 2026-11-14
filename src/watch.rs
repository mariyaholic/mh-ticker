//! Telling the user when alerts could not be checked (H7) and what a mute swallowed (H8).
//!
//! Pure and portable (std only). Both machines are stepped by the feed tick; neither has a timer.
//! A sleep shows up as a clock jump between two ticks, so the same rule that catches a dead
//! network catches a closed lid.

/// A gap shorter than this is not worth a line, however fast the polls are.
pub const MIN_GAP_S: f64 = 300.0;
/// A gap shorter than this many polls is not worth a line either (the larger of the two wins).
pub const GAP_POLLS: f64 = 10.0;

/// The longest span without a good check that goes unmentioned: the larger of five minutes and ten
/// polls at the interval in force.
pub fn threshold(interval_s: f64) -> f64 {
    if interval_s.is_finite() && interval_s > 0.0 { MIN_GAP_S.max(GAP_POLLS * interval_s) } else { MIN_GAP_S }
}

/// What a tick found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The span without a check has just passed the threshold and goes on: show it while it lasts.
    Blind { since: f64 },
    /// Checking works again after a span longer than the threshold. `jump` is true when no tick saw
    /// the trouble (the machine slept, or the app was frozen): the clock simply jumped.
    Back { from: f64, to: f64, jump: bool },
}

/// The gap tracker. One per process; it forgets everything when no alert is armed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    last_ok: Option<f64>,
    saw_blind: bool,
    announced: bool,
}

impl Watch {
    /// One feed tick. `fresh` is true when at least one armed alert's symbol has a fresh price (an
    /// alert could be checked), `armed` whether any alert is set, `threshold_s` from [`threshold`].
    /// The first armed tick starts the watch whether or not it was fresh, so a feed that never worked
    /// is reported from the moment watching began.
    pub fn step(&mut self, now: f64, fresh: bool, armed: bool, threshold_s: f64) -> Option<Event> {
        if !armed || !now.is_finite() {
            self.reset();
            return None;
        }
        let Some(last) = self.last_ok else {
            self.last_ok = Some(now);
            self.saw_blind = !fresh;
            return None;
        };
        if now < last {
            // A clock set back: start again from here rather than report a negative span.
            self.last_ok = Some(now);
            self.saw_blind = !fresh;
            self.announced = false;
            return None;
        }
        let gap = now - last;
        if fresh {
            let event = (gap > threshold_s).then_some(Event::Back { from: last, to: now, jump: !self.saw_blind });
            self.last_ok = Some(now);
            self.saw_blind = false;
            self.announced = false;
            event
        } else {
            self.saw_blind = true;
            if gap > threshold_s && !self.announced {
                self.announced = true;
                return Some(Event::Blind { since: last });
            }
            None
        }
    }

    /// Since when alerts have not been checked, while a gap that is past the threshold lasts.
    pub fn blind_since(&self) -> Option<f64> {
        if self.announced { self.last_ok } else { None }
    }

    /// Forget the span: nothing is armed, or the alerts were cleared.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The Recent line for a gap: `02:10 to 02:31 not checked (no fresh price)`. The times are the
/// caller's local clock readings. It never carries a price.
pub fn gap_line(from: &str, to: &str, jump: bool) -> String {
    let why = if jump { "no checks ran, the PC slept" } else { "no fresh price" };
    format!("{from} to {to} not checked ({why})")
}

/// The tooltip line while a gap lasts.
pub fn blind_tip(since: &str) -> String {
    format!("alerts not checked since {since} (no fresh price)")
}

/// The optional webhook notice, once per gap, sent when checking is back. Times only.
pub fn gap_notice(from: &str, to: &str) -> String {
    format!("Alerts could not be checked from {from} to {to}.")
}

/// Has a mute or quiet hours just ended? Stepped by the feed tick with whether alerts are silenced.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hush {
    since: Option<f64>,
}

impl Hush {
    /// `Some(since)` on the tick that finds the silence over, `since` being the first tick that found
    /// it on. Anything swallowed in between is counted from `since`.
    pub fn step(&mut self, now: f64, silenced: bool) -> Option<f64> {
        match (self.since, silenced) {
            (None, true) => {
                self.since = Some(now);
                None
            }
            (Some(_), false) => self.since.take(),
            _ => None,
        }
    }
}

/// The balloon text for what a silence swallowed: `3 alerts while muted. See Alerts > Recent.`
/// Nothing for zero.
pub fn digest_text(muted: usize, quiet: usize) -> Option<String> {
    let n = muted + quiet;
    if n == 0 {
        return None;
    }
    let what = match (muted > 0, quiet > 0) {
        (true, true) => "muted or in quiet hours",
        (true, false) => "muted",
        _ => "in quiet hours",
    };
    let alerts = if n == 1 { "alert" } else { "alerts" };
    Some(format!("{n} {alerts} while {what}. See Alerts > Recent."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ticks: &[(f64, bool)]) -> Vec<(f64, Event)> {
        let mut w = Watch::default();
        ticks.iter().filter_map(|&(t, fresh)| w.step(t, fresh, true, threshold(3.0)).map(|e| (t, e))).collect()
    }

    #[test]
    fn the_threshold_is_the_larger_of_five_minutes_and_ten_polls() {
        assert_eq!(threshold(3.0), 300.0);
        assert_eq!(threshold(30.0), 300.0);
        assert_eq!(threshold(60.0), 600.0);
        assert_eq!(threshold(f64::NAN), 300.0);
    }

    #[test]
    fn a_short_blip_gives_no_event() {
        let ticks: Vec<_> = (0..100).map(|i| (i as f64 * 3.0, i % 2 == 0)).collect();
        assert!(run(&ticks).is_empty());
        // Just under and exactly at the threshold: still nothing; one second over is a gap.
        assert!(run(&[(0.0, true), (299.0, false), (300.0, true)]).is_empty());
        let over = run(&[(0.0, true), (300.0, false), (301.0, true)]);
        assert_eq!(over.len(), 1);
    }

    #[test]
    fn a_stale_spell_gives_blind_once_then_back_with_both_ends() {
        let mut ticks = vec![(1_000.0, true)];
        ticks.extend((1..=30).map(|i| (1_000.0 + i as f64 * 60.0, false)));
        ticks.push((1_000.0 + 31.0 * 60.0, true));
        ticks.push((1_000.0 + 32.0 * 60.0, true));
        let ev = run(&ticks);
        assert_eq!(ev.len(), 2, "{ev:?}");
        assert_eq!(ev[0].1, Event::Blind { since: 1_000.0 });
        assert_eq!(ev[1].1, Event::Back { from: 1_000.0, to: 1_000.0 + 31.0 * 60.0, jump: false });
    }

    #[test]
    fn a_clock_jump_gives_one_line_with_both_ends() {
        let ev = run(&[(0.0, true), (3.0, true), (3.0 + 8_000.0, true), (3.0 + 8_003.0, true)]);
        assert_eq!(ev, vec![(8_003.0, Event::Back { from: 3.0, to: 8_003.0, jump: true })]);
    }

    #[test]
    fn a_jump_landing_on_a_stale_tick_is_blind_then_back() {
        let ev = run(&[(0.0, true), (9_000.0, false), (9_003.0, false), (9_100.0, true)]);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].1, Event::Blind { since: 0.0 });
        assert_eq!(ev[1].1, Event::Back { from: 0.0, to: 9_100.0, jump: false });
    }

    #[test]
    fn nothing_armed_means_nothing_watched_and_state_is_forgotten() {
        let mut w = Watch::default();
        assert_eq!(w.step(0.0, true, true, 300.0), None);
        assert_eq!(w.step(10_000.0, false, false, 300.0), None, "disarmed during the gap");
        assert_eq!(w.blind_since(), None);
        assert_eq!(w.step(20_000.0, true, true, 300.0), None, "watching starts again from here");
    }

    #[test]
    fn a_feed_that_never_worked_is_reported_from_when_watching_began() {
        let ev = run(&[(100.0, false), (200.0, false), (500.0, false), (501.0, true)]);
        assert_eq!(ev[0].1, Event::Blind { since: 100.0 });
        assert_eq!(ev[1].1, Event::Back { from: 100.0, to: 501.0, jump: false });
    }

    #[test]
    fn blind_since_lasts_only_while_the_gap_does() {
        let mut w = Watch::default();
        w.step(0.0, true, true, 300.0);
        w.step(100.0, false, true, 300.0);
        assert_eq!(w.blind_since(), None, "under the threshold nothing is shown");
        w.step(400.0, false, true, 300.0);
        assert_eq!(w.blind_since(), Some(0.0));
        w.step(401.0, true, true, 300.0);
        assert_eq!(w.blind_since(), None);
    }

    #[test]
    fn a_clock_set_back_starts_over_without_an_event() {
        let ev = run(&[(10_000.0, true), (100.0, true), (103.0, true)]);
        assert!(ev.is_empty());
    }

    #[test]
    fn the_texts_carry_times_and_never_a_price() {
        let line = gap_line("02:10", "02:31", false);
        assert_eq!(line, "02:10 to 02:31 not checked (no fresh price)");
        let notice = gap_notice("02:10", "02:31");
        assert_eq!(notice, "Alerts could not be checked from 02:10 to 02:31.");
        assert_eq!(blind_tip("02:10"), "alerts not checked since 02:10 (no fresh price)");
        for text in [line, notice, blind_tip("02:10"), gap_line("02:10", "02:31", true)] {
            // The only digits are the two clock readings.
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            assert!(digits.len() <= 8, "{text}");
            assert!(!text.contains('$') && !text.contains(".0"), "{text}");
        }
    }

    #[test]
    fn a_hush_reports_its_start_when_it_ends() {
        let mut h = Hush::default();
        assert_eq!(h.step(1.0, false), None);
        assert_eq!(h.step(2.0, true), None);
        assert_eq!(h.step(3.0, true), None);
        assert_eq!(h.step(4.0, false), Some(2.0));
        assert_eq!(h.step(5.0, false), None, "once");
        h.step(6.0, true);
        assert_eq!(h.step(7.0, false), Some(6.0), "the way out is the silence ending");
    }

    #[test]
    fn the_digest_says_nothing_for_zero_and_one_line_otherwise() {
        assert_eq!(digest_text(0, 0), None);
        assert_eq!(digest_text(3, 0).unwrap(), "3 alerts while muted. See Alerts > Recent.");
        assert_eq!(digest_text(1, 0).unwrap(), "1 alert while muted. See Alerts > Recent.");
        assert_eq!(digest_text(0, 2).unwrap(), "2 alerts while in quiet hours. See Alerts > Recent.");
        assert_eq!(digest_text(1, 1).unwrap(), "2 alerts while muted or in quiet hours. See Alerts > Recent.");
    }
}
