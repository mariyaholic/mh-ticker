//! Settings a person can type, `std` only: a time span for quiet hours, a time of day for a timed
//! mute, a cooldown, and the sentences that refuse a bad entry.
//!
//! Every parser returns `Err` with one fixed sentence (the same text for every kind of mistake in
//! that field), and the caller changes nothing on `Err`. No input is echoed back into a sentence, so
//! a pasted secret never reaches a dialog. The hotkey is not parsed here: `hotkey::parse` is the
//! one grammar for it and is used as it is; only the sentence for refusing it lives here.

pub const QUIET_REFUSED: &str = "Type quiet hours as start-end in 24-hour time, such as 22:30-06:45. Nothing was changed.";
pub const TIME_REFUSED: &str = "Type a time of day in 24-hour time, such as 17:30. Nothing was changed.";
pub const COOLDOWN_REFUSED: &str =
    "Type a cooldown such as 90s, 15m or 2h, from 30 seconds to 24 hours. Nothing was changed.";
pub const HOTKEY_REFUSED: &str =
    "Type a hotkey as Ctrl, Alt or Win (and Shift if you like) plus one key, such as Ctrl+Alt+M. Nothing was changed.";

/// `H:MM` or `HH:MM`, 00:00 to 23:59, as a minute of the day.
pub fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let digits = |s: &str, lo: usize, hi: usize| (lo..=hi).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h, 1, 2) || !digits(m, 2, 2) {
        return None;
    }
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Quiet hours as typed, `22:30-06:45`: `Ok(Some([from, to]))` in minutes of the local day (it wraps
/// midnight when `from > to`), `Ok(None)` when the two ends are equal, which means off. The dash may
/// be `-`, an en dash, or the word `to`.
pub fn parse_span(text: &str) -> Result<Option<[u16; 2]>, &'static str> {
    let t = text.trim().replace('\u{2013}', "-");
    let (a, b) = match t.split_once('-') {
        Some(parts) => parts,
        None => t.split_once(" to ").ok_or(QUIET_REFUSED)?,
    };
    let (from, to) = (parse_clock(a).ok_or(QUIET_REFUSED)?, parse_clock(b).ok_or(QUIET_REFUSED)?);
    Ok((from != to).then_some([from, to]))
}

/// A span as it is typed back into the prompt: `22:30-06:45`.
pub fn span_text(span: [u16; 2]) -> String {
    format!("{}-{}", clock_text(span[0]), clock_text(span[1]))
}

pub fn clock_text(minute: u16) -> String {
    let m = minute % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// A time of day for a timed mute, `17:30`.
pub fn parse_time(text: &str) -> Result<u16, &'static str> {
    parse_clock(text).ok_or(TIME_REFUSED)
}

/// The UTC instant a mute typed as `minute` of the local day ends: the next time the local clock
/// reads it, strictly after now (typing the minute it is already is tomorrow). `local_secs_of_day`
/// is how many seconds of the local day have passed at `now_utc`. A clock change in between moves the
/// real end by the size of the change; a mute is not worth a calendar for that.
pub fn mute_until(now_utc: f64, local_secs_of_day: u32, minute: u16) -> f64 {
    let target = minute as i64 * 60;
    let mut delta = target - local_secs_of_day as i64;
    if delta <= 0 {
        delta += 86_400;
    }
    now_utc + delta as f64
}

/// The shortest and longest cooldown that can be typed, in seconds. The firing rule never goes below
/// 30 s (`alerts::cooled`), so a typed value under that would not do what it says.
pub const COOLDOWN_MIN_S: f64 = 30.0;
pub const COOLDOWN_MAX_S: f64 = 86_400.0;

/// A cooldown as typed: a whole number with `s`, `m` or `h` (a bare number is minutes), 30 seconds to
/// 24 hours.
pub fn parse_cooldown(text: &str) -> Result<f64, &'static str> {
    let t = text.trim().to_ascii_lowercase();
    let (num, unit) = match t.chars().last() {
        Some(c @ ('s' | 'm' | 'h')) => (&t[..t.len() - 1], c),
        _ => (t.as_str(), 'm'),
    };
    let num = num.trim_end();
    if num.is_empty() || num.len() > 6 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(COOLDOWN_REFUSED);
    }
    let n: f64 = num.parse().map_err(|_| COOLDOWN_REFUSED)?;
    let secs = n * match unit {
        's' => 1.0,
        'm' => 60.0,
        _ => 3_600.0,
    };
    (COOLDOWN_MIN_S..=COOLDOWN_MAX_S).contains(&secs).then_some(secs).ok_or(COOLDOWN_REFUSED)
}

/// A cooldown as it is typed back into the prompt: `90s`, `15m`, `2h`.
pub fn cooldown_text(secs: f64) -> String {
    let s = if secs.is_finite() { secs.round().max(0.0) as u64 } else { 900 };
    if s % 3_600 == 0 && s >= 3_600 {
        format!("{}h", s / 3_600)
    } else if s % 60 == 0 && s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_parses_wraps_midnight_and_equal_ends_mean_off() {
        assert_eq!(parse_span("22:30-06:45"), Ok(Some([22 * 60 + 30, 6 * 60 + 45])));
        assert_eq!(parse_span(" 9:05 \u{2013} 17:00 "), Ok(Some([9 * 60 + 5, 17 * 60])));
        assert_eq!(parse_span("22:00 to 07:00"), Ok(Some([22 * 60, 7 * 60])));
        // Wraps: from after to, which `alerts::quiet` already reads across midnight (tested there).
        let [from, to] = parse_span("22:30-06:45").unwrap().unwrap();
        assert!(from > to);
        // Equal ends: off.
        assert_eq!(parse_span("07:00-07:00"), Ok(None));
        assert_eq!(parse_span("00:00-00:00"), Ok(None));
        assert_eq!(parse_span("00:00-23:59"), Ok(Some([0, 23 * 60 + 59])));
    }

    #[test]
    fn a_bad_span_is_refused_with_the_one_sentence() {
        for bad in ["", "22:30", "22:30-", "-06:45", "24:00-06:00", "22:60-06:00", "22.30-06.45", "ab:cd-01:00", "22:30-06:45-08:00", "1:5-2:00", "22:30 06:45", "7pm-7am", "22:30;06:45"] {
            assert_eq!(parse_span(bad), Err(QUIET_REFUSED), "{bad:?}");
        }
    }

    #[test]
    fn a_time_of_day_is_a_minute_and_a_bad_one_is_refused() {
        assert_eq!(parse_time("17:30"), Ok(17 * 60 + 30));
        assert_eq!(parse_time(" 0:00 "), Ok(0));
        assert_eq!(parse_time("23:59"), Ok(23 * 60 + 59));
        for bad in ["", "24:00", "12:60", "1730", "5pm", "12:5", "-1:30", "12:30:00"] {
            assert_eq!(parse_time(bad), Err(TIME_REFUSED), "{bad:?}");
        }
    }

    #[test]
    fn a_mute_ends_at_the_next_time_the_local_clock_reads_it() {
        let now = 1_000_000.0;
        // 10:00:00 local; mute until 17:30 is 7.5 hours away.
        assert_eq!(mute_until(now, 10 * 3600, 17 * 60 + 30), now + 7.5 * 3600.0);
        // Already past: tomorrow.
        assert_eq!(mute_until(now, 18 * 3600, 17 * 60 + 30), now + 23.5 * 3600.0);
        // The minute it is right now (any second into it): tomorrow, never zero.
        assert_eq!(mute_until(now, 17 * 3600 + 30 * 60, 17 * 60 + 30), now + 86_400.0);
        assert!(mute_until(now, 17 * 3600 + 30 * 60 + 20, 17 * 60 + 30) > now + 86_000.0);
        // Never more than a day away.
        for sod in [0, 1, 3_599, 43_200, 86_399] {
            for min in [0, 1, 720, 1_439] {
                let d = mute_until(now, sod, min) - now;
                assert!(d > 0.0 && d <= 86_400.0, "{sod} {min} {d}");
            }
        }
    }

    #[test]
    fn a_cooldown_has_units_a_range_and_one_refusal() {
        assert_eq!(parse_cooldown("90s"), Ok(90.0));
        assert_eq!(parse_cooldown("15m"), Ok(900.0));
        assert_eq!(parse_cooldown("15"), Ok(900.0), "a bare number is minutes");
        assert_eq!(parse_cooldown(" 2H "), Ok(7_200.0));
        assert_eq!(parse_cooldown("30s"), Ok(30.0));
        assert_eq!(parse_cooldown("24h"), Ok(86_400.0));
        for bad in ["", "m", "29s", "0", "25h", "1441m", "1.5h", "-5m", "5 minutes", "1e3s", "999999999h", "15mm"] {
            assert_eq!(parse_cooldown(bad), Err(COOLDOWN_REFUSED), "{bad:?}");
        }
    }

    #[test]
    fn what_the_prompt_shows_parses_back_to_the_same_value() {
        for secs in [30.0, 90.0, 900.0, 3_600.0, 7_200.0, 5_400.0, 86_400.0] {
            assert_eq!(parse_cooldown(&cooldown_text(secs)), Ok(secs), "{secs}");
        }
        for span in [[1_350u16, 405], [0, 540], [1_320, 420]] {
            assert_eq!(parse_span(&span_text(span)), Ok(Some(span)));
        }
        assert_eq!(cooldown_text(f64::NAN), "15m");
    }

    #[test]
    fn the_sentences_name_no_input_and_end_with_nothing_changed() {
        for s in [QUIET_REFUSED, TIME_REFUSED, COOLDOWN_REFUSED, HOTKEY_REFUSED] {
            assert!(s.ends_with("Nothing was changed."), "{s}");
            assert!(!s.contains('\u{2014}'), "{s}");
        }
    }
}
