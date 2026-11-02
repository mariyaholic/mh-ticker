// SPDX-License-Identifier: MIT
//! Rust port of `MhNextBoundary` from the generated MQL5 boilerplate (02-H1).
//!
//! The MQL5 indicator cannot be executed on this box (no MT5), so the port
//! drives the same algorithm with the session-engine rules and asserts the
//! result against the shared golden vectors. The generated artifact itself is
//! guarded structurally: it must zero the clock fields before `StructToTime`.

use chrono::{DateTime, Offset, TimeZone, Utc};
use serde_json::Value;

/// `MhOffsetByZone` equivalent: seconds east of UTC for the rule zone at `utc`.
fn offset_sec(rule: &mh_session::SessionRule, utc: i64) -> i64 {
    let at: DateTime<Utc> = Utc.timestamp_opt(utc, 0).single().expect("valid utc");
    at.with_timezone(&rule.zone())
        .offset()
        .fix()
        .local_minus_utc() as i64
}

/// `MhLocalToUtc`: iterative guess `local - offset(guess)`, three refinement
/// rounds, exactly as generated for MQL5.
fn local_to_utc(rule: &mh_session::SessionRule, local: i64) -> i64 {
    let mut guess = local;
    for _ in 0..3 {
        guess = local - offset_sec(rule, guess);
    }
    guess
}

fn epoch_rfc3339(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}

fn next_boundary(rule: &mh_session::SessionRule, utc_secs: i64) -> i64 {
    let open_min = rule.open_min as i64;
    let close_min = rule.close_min as i64;
    let offset = offset_sec(rule, utc_secs);
    let local_now = utc_secs + offset;
    // Midnight in the zone's local coordinate: floor to the day with every
    // clock field zeroed. The unfixed MQL5 used `StructToTime(dt)` with the
    // current time still inside `dt`, so `midnight` equalled "now".
    let midnight = local_now - local_now.rem_euclid(86_400);
    let mut best = i64::MAX;
    for d in -1..=10 {
        let local_open = midnight + d * 86_400 + open_min * 60;
        let local_close =
            midnight + d * 86_400 + close_min * 60 + if close_min <= open_min { 86_400 } else { 0 };
        // MQL5 weekday filter on the candidate's local day (`(dow + 6) % 7`
        // is Monday-first; 1970-01-01 was a Thursday, hence `+ 3`).
        let day_bits = local_open.div_euclid(86_400);
        let mon_first = (day_bits + 3).rem_euclid(7);
        if !rule.days[mon_first as usize] {
            continue;
        }
        let open_utc = local_to_utc(rule, local_open);
        let close_utc = local_to_utc(rule, local_close);
        if open_utc > utc_secs && open_utc < best {
            best = open_utc;
        }
        if close_utc > utc_secs && close_utc < best {
            best = close_utc;
        }
    }
    if best == i64::MAX {
        0
    } else {
        best
    }
}

const VECTORS: &str = include_str!("../../../spec/golden/vectors.json");

#[test]
fn mql5_boundary_port_matches_the_golden_vectors() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let cases = v["cases"].as_array().unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let now = epoch_rfc3339(case["utc"].as_str().unwrap());
        let sessions = case["sessions"].as_object().unwrap();
        for (id, want) in sessions {
            let rule = mh_session::session(id).unwrap_or_else(|| panic!("{name}: {id}"));
            let got = next_boundary(rule, now);
            let want_at = epoch_rfc3339(want["next_utc"].as_str().unwrap());
            assert_eq!(got, want_at, "{name}/{id} boundary");
        }
    }
}

/// Port of `MhBrokerNextReopen` with the 02-H1 + 02-M2 fix: midnight-anchored
/// and driven by the *effective* offset the indicator displays.
fn next_reopen(offset_min: i32, reopen_min: i32, days_monfirst: u32, utc_secs: i64) -> i64 {
    let effective = offset_min;
    let server_now = utc_secs + (effective as i64) * 60;
    let midnight = server_now - server_now.rem_euclid(86_400);
    let reopen_min = reopen_min as i64;
    let mut best = 0i64;
    for d in -1..=10 {
        let local_reopen = midnight + d * 86_400 + reopen_min * 60;
        let day_bits = local_reopen.div_euclid(86_400);
        if days_monfirst >> ((day_bits + 3).rem_euclid(7)) & 1 == 0 {
            continue;
        }
        let guess = local_reopen - (effective as i64) * 60;
        if guess > utc_secs && (best == 0 || guess < best) {
            best = guess;
        }
    }
    best
}

const BROKER_VECTORS: &str = include_str!("../../../spec/golden/vectors.json");

#[test]
fn mql5_reopen_port_matches_the_broker_vectors() {
    let v: Value = serde_json::from_str(BROKER_VECTORS).unwrap();
    let mut checked = 0;
    for case in v["broker_cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        // Only the cases the MQL5 can model: verified, fixed offset override.
        if case["rule"]["seasonal"].as_str() != Some("fixed") {
            continue;
        }
        let offset = case["rule"]["winter_offset_minutes"].as_i64().unwrap() as i32;
        let (h, m) = case["rule"]["reopen_local"]
            .as_str()
            .unwrap()
            .split_once(':')
            .unwrap();
        let reopen_min = h.parse::<i64>().unwrap() * 60 + m.parse::<i64>().unwrap();
        let mut days = 0u32;
        for d in case["rule"]["reopen_days"].as_array().unwrap() {
            days |= 1 << (d.as_u64().unwrap() as u32 - 1);
        }
        let now = epoch_rfc3339(case["utc"].as_str().unwrap());
        let want = epoch_rfc3339(case["next_reopen"].as_str().unwrap());
        let got = next_reopen(offset, reopen_min as i32, days, now);
        assert_eq!(got, want, "{name} reopen");
        checked += 1;
    }
    assert!(checked >= 2, "{checked} broker vectors exercised");
}

const MQL5: &str = include_str!("../../../mql5/MH_Sessions.mq5");

#[test]
fn generated_mql5_reopen_uses_the_effective_offset() {
    // 02-M2: the signature takes the displayed offset, the caller passes it,
    // and the candidate conversion uses the same offset instead of re-deriving
    // the seasonal default per iteration.
    let text = MQL5.replace("\r\n", "\n");
    for needle in [
        "long MhBrokerNextReopen(long utc, int brokerOffset)",
        "MhBrokerNextReopen(utc, brokerOffset)",
        "long guess = (long)localReopen - offset * 60;",
    ] {
        assert!(text.contains(needle), "missing: {needle}");
    }
}

#[test]
fn generated_mql5_zeroes_the_midnight_fields() {
    // Both `StructToTime(dt)` call sites (MhNextBoundary, MhBrokerNextReopen)
    // must zero hour/min/sec first, or `midnight` is "now" (02-H1).
    let text = MQL5.replace("\r\n", "\n");
    let zeroing = text
        .matches("dt.hour = 0;\n   dt.min = 0;\n   dt.sec = 0;")
        .count();
    assert_eq!(zeroing, 2, "midnight must be zeroed in both call sites");
}
