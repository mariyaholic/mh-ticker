// SPDX-License-Identifier: Apache-2.0
//! Shared UTC golden vectors: the same JSON is shipped beside the indicators.

use chrono::{DateTime, TimeZone, Utc};
use mh_session::engine::{default_overlap, status_at, BoundaryKind, ReopenConfidence};
use mh_session::{session, BrokerRule, BROKER_DEFAULT, SPEC_VERSION, TZDB_VERSION};
use serde_json::Value;

const VECTORS: &str = include_str!("../../../spec/golden/vectors.json");

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn vectors_match_the_generated_spec_versions() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    assert_eq!(v["spec_version"], SPEC_VERSION);
    assert_eq!(v["tzdb_version"], TZDB_VERSION);
}

#[test]
fn session_and_overlap_vectors() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        let name = text(case, "name");
        let now = utc(text(case, "utc"));
        let sessions = case["sessions"].as_object().unwrap();
        assert_eq!(sessions.len(), 4, "{name}");
        for (id, want) in sessions {
            let rule = session(id).unwrap_or_else(|| panic!("{name}: unknown session {id}"));
            let got = status_at(rule, now);
            assert_eq!(got.is_open(), want["open"].as_bool().unwrap(), "{name}/{id} open");
            let next = got.next.unwrap_or_else(|| panic!("{name}/{id}: no next boundary"));
            let kind = text(want, "next");
            let want_kind = match kind {
                "open" => BoundaryKind::Open,
                "close" => BoundaryKind::Close,
                other => panic!("{name}/{id}: bad kind {other}"),
            };
            assert_eq!(next.kind, want_kind, "{name}/{id} kind");
            assert_eq!(next.at, utc(text(want, "next_utc")), "{name}/{id} next instant");
        }
        let ov = default_overlap(now);
        let want = &case["overlap"];
        assert_eq!(ov.active.is_some(), want["open"].as_bool().unwrap(), "{name} overlap open");
        if want["open"].as_bool().unwrap() {
            let active = ov.active.unwrap();
            assert_eq!(active.start, utc(text(want, "current_start")), "{name} overlap start");
            assert_eq!(active.end, utc(text(want, "current_end")), "{name} overlap end");
        } else {
            let next = ov.next.unwrap_or_else(|| panic!("{name}: no next overlap"));
            assert_eq!(next.start, utc(text(want, "next_start")), "{name} next overlap start");
            assert_eq!(next.end, utc(text(want, "next_end")), "{name} next overlap end");
        }
    }
}

#[test]
fn broker_vectors() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    for case in v["broker_cases"].as_array().unwrap() {
        let name = text(case, "name");
        let now = utc(text(case, "utc"));
        let r = &case["rule"];
        let mut days = [false; 7];
        for d in r["reopen_days"].as_array().unwrap() {
            days[(d.as_u64().unwrap() - 1) as usize] = true;
        }
        let (h, m) = text(r, "reopen_local").split_once(':').unwrap();
        let rule = BrokerRule {
            verified: r["verified"].as_bool().unwrap(),
            winter_offset_min: r["winter_offset_minutes"].as_i64().unwrap() as i32,
            summer_offset_min: r["summer_offset_minutes"].as_i64().unwrap() as i32,
            seasonal_us_dst: text(r, "seasonal") == "us_dst",
            reopen_min: h.parse::<u16>().unwrap() * 60 + m.parse::<u16>().unwrap(),
            reopen_days: days,
            ..BROKER_DEFAULT
        };
        let got = rule.status_at(now);
        assert_eq!(got.confidence, ReopenConfidence::Verified, "{name}");
        assert_eq!(got.offset_min, case["offset_minutes"].as_i64().unwrap() as i32, "{name} offset");
        assert_eq!(got.server_now, utc(text(case, "server_utc")), "{name} server clock");
        assert_eq!(got.next_reopen, Some(utc(text(case, "next_reopen"))), "{name} reopen");
    }
}

#[test]
fn default_broker_preset_is_never_presented_as_verified() {
    let now = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
    let got = BROKER_DEFAULT.status_at(now);
    assert!(!BROKER_DEFAULT.verified);
    assert_eq!(got.confidence, ReopenConfidence::Unverified);
    assert!(got.next_reopen.is_none());
    assert!(got.estimate.is_some());
}
