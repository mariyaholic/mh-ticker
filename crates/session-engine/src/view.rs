// SPDX-License-Identifier: MIT
//! Presentation-neutral rows for the ticker panel and (where useful) the
//! indicator boilerplates. This module is deliberately pure: it takes a UTC
//! instant and already-detected device-zone information, so it is fully
//! testable on Linux.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use crate::engine::{default_overlap, offset_label, short_zone, status_at, BrokerStatus, ReopenConfidence};
use crate::zones::WINDOWS_ZONE_DATA_VERSION;
use crate::{coverage_years, hhmm, BrokerRule, SessionRule, BROKER_DEFAULT, SESSION_RULES, SPEC_VERSION, TZDB_VERSION};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Open,
    Closed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSource {
    /// The Windows adapter mapped the current dynamic time-zone key.
    Auto,
    /// The user supplied an IANA zone in config.
    Configured,
    /// Detection or mapping failed.
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub iana: Option<String>,
    pub source: DeviceSource,
    pub windows_id: Option<String>,
    pub territory: Option<String>,
}

impl DeviceInfo {
    pub fn unavailable(windows_id: Option<String>, territory: Option<String>) -> Self {
        Self { iana: None, source: DeviceSource::Unavailable, windows_id, territory }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRow {
    pub title: String,
    pub clock: String,
    pub state: String,
    pub detail: String,
    pub tone: Tone,
    /// True for the single extra device-zone clock; never used to reorder rows.
    pub device: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlapLine {
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionView {
    /// Always four sessions, then broker, then exactly one device row.
    pub rows: Vec<ViewRow>,
    pub overlap: OverlapLine,
    pub caveat: String,
    pub coverage_ok: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewOptions {
    pub broker_offset_min: Option<i32>,
    pub broker_reopen_min: Option<u16>,
    pub broker_rule_verified: bool,
    pub device: DeviceInfo,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            broker_offset_min: None,
            broker_reopen_min: None,
            broker_rule_verified: false,
            device: DeviceInfo::unavailable(None, None),
        }
    }
}

pub fn fmt_clock(zone: Tz, now: DateTime<Utc>) -> String {
    now.with_timezone(&zone).format("%H:%M").to_string()
}

pub fn fmt_seconds_clock(zone: Tz, now: DateTime<Utc>) -> String {
    now.with_timezone(&zone).format("%H:%M:%S").to_string()
}

pub fn fmt_server_clock(now: DateTime<Utc>, offset_min: i32) -> String {
    (now + Duration::minutes(offset_min as i64)).format("%H:%M").to_string()
}

/// Compact countdown: `45s`, `12m`, `2h 05m`, `3d 4h`.
pub fn countdown(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let secs = (at - now).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m:02}m")
        }
    } else {
        let d = secs / 86_400;
        let h = (secs % 86_400) / 3600;
        if h == 0 {
            format!("{d}d")
        } else {
            format!("{d}d {h}h")
        }
    }
}

fn session_row(rule: &SessionRule, now: DateTime<Utc>) -> ViewRow {
    let st = status_at(rule, now);
    let clock = fmt_clock(rule.zone(), now);
    if !st.in_coverage {
        return ViewRow {
            title: rule.label.to_uppercase(),
            clock,
            state: "unknown".into(),
            detail: "outside tzdb coverage".into(),
            tone: Tone::Unknown,
            device: false,
        };
    }
    let open = st.is_open();
    let detail = match st.next {
        Some(b) if open => format!("closes in {}", countdown(now, b.at)),
        Some(b) => format!("opens in {}", countdown(now, b.at)),
        None => "no transition".into(),
    };
    ViewRow {
        title: rule.label.to_uppercase(),
        clock,
        state: if open { "open".into() } else { "closed".into() },
        detail,
        tone: if open { Tone::Open } else { Tone::Closed },
        device: false,
    }
}

fn broker_row(rule: &BrokerRule, status: &BrokerStatus, now: DateTime<Utc>, offset_set: bool) -> ViewRow {
    let detail = match status.confidence {
        ReopenConfidence::Verified => match status.next_reopen {
            Some(at) => format!("reopens in {}", countdown(now, at)),
            None => "reopen unknown".into(),
        },
        ReopenConfidence::Unverified => match status.estimate {
            Some(at) => {
                let local = at + Duration::minutes(status.offset_min as i64);
                format!("unknown · est {} {}", local.format("%a"), local.format("%H:%M"))
            }
            None => "reopen unknown".into(),
        },
        ReopenConfidence::OutsideCoverage => "no coverage".into(),
    };
    ViewRow {
        title: rule.label.to_uppercase(),
        clock: fmt_server_clock(now, status.offset_min),
        state: if offset_set { format!("{} set", offset_label(status.offset_min)) } else { offset_label(status.offset_min) },
        detail,
        tone: if status.confidence == ReopenConfidence::Verified { Tone::Closed } else { Tone::Unknown },
        device: false,
    }
}

fn device_row(info: &DeviceInfo, now: DateTime<Utc>) -> ViewRow {
    let source = match info.source {
        DeviceSource::Auto => "auto-detected",
        DeviceSource::Configured => "configured",
        DeviceSource::Unavailable => "unavailable",
    };
    match info.iana.as_deref().and_then(|z| z.parse::<Tz>().ok()) {
        Some(zone) => ViewRow {
            title: "DEVICE".into(),
            clock: fmt_clock(zone, now),
            state: short_zone(zone.name()),
            detail: source.into(),
            tone: Tone::Closed,
            device: true,
        },
        None => ViewRow {
            title: "DEVICE".into(),
            clock: "--:--".into(),
            state: "unavailable".into(),
            detail: match &info.windows_id {
                Some(id) if !id.is_empty() => format!("zone map failed: {id}"),
                _ => "zone map failed".into(),
            },
            tone: Tone::Unknown,
            device: true,
        },
    }
}

fn overlap_line(now: DateTime<Utc>) -> (OverlapLine, bool) {
    let ov = default_overlap(now);
    let first = crate::session(ov.first).map(|s| s.label.to_uppercase()).unwrap_or_else(|| ov.first.to_uppercase());
    let second = crate::session(ov.second).map(|s| s.label.to_uppercase()).unwrap_or_else(|| ov.second.to_uppercase());
    let head = format!("{first}\u{2013}{second} OVERLAP");
    if !ov.in_coverage {
        return (
            OverlapLine { text: format!("{head} · outside tzdb coverage"), tone: Tone::Unknown },
            false,
        );
    }
    match ov.active {
        Some(i) => (
            OverlapLine { text: format!("{head} · open · ends in {}", countdown(now, i.end)), tone: Tone::Open },
            true,
        ),
        None => match ov.next {
            Some(i) => (
                OverlapLine { text: format!("{head} · closed · next in {}", countdown(now, i.start)), tone: Tone::Closed },
                true,
            ),
            None => (OverlapLine { text: format!("{head} · none this week"), tone: Tone::Closed }, true),
        },
    }
}

pub fn build_view(now: DateTime<Utc>, opts: &ViewOptions) -> SessionView {
    let broker = BROKER_DEFAULT.with_overrides(opts.broker_offset_min, opts.broker_reopen_min, opts.broker_rule_verified);
    let broker_status = broker.status_at(now);
    let mut rows: Vec<ViewRow> = SESSION_RULES.iter().map(|r| session_row(r, now)).collect();
    rows.push(broker_row(&broker, &broker_status, now, opts.broker_offset_min.is_some()));
    rows.push(device_row(&opts.device, now));
    debug_assert_eq!(rows.len(), 6);
    debug_assert_eq!(rows.iter().filter(|r| r.device).count(), 1);
    let (overlap, coverage_ok) = overlap_line(now);
    let caveat = format!(
        "Conventions, not exchange hours · no holidays · tzdb {} · {} · spec {} · broker configured · {}",
        TZDB_VERSION,
        coverage_years(),
        SPEC_VERSION,
        WINDOWS_ZONE_DATA_VERSION
    );
    SessionView { rows, overlap, caveat, coverage_ok }
}

/// Convenience for the panel: the four ordered session rules, then broker.
pub fn rule_count() -> usize {
    SESSION_RULES.len() + 2
}

pub fn hhmm_label(minutes: u16) -> String {
    hhmm(minutes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BrokerRule, BROKER_DEFAULT};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn fixed_order_and_exactly_one_extra_device_row() {
        let opts = ViewOptions {
            device: DeviceInfo {
                iana: Some("Europe/Berlin".into()),
                source: DeviceSource::Auto,
                windows_id: Some("W. Europe Standard Time".into()),
                territory: Some("DE".into()),
            },
            ..ViewOptions::default()
        };
        let v = build_view(utc("2025-01-15T14:00:00Z"), &opts);
        let titles: Vec<&str> = v.rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["SYDNEY", "TOKYO", "LONDON", "NEW YORK", "BROKER", "DEVICE"]);
        assert_eq!(v.rows.iter().filter(|r| r.device).count(), 1);
        assert_eq!(v.rows[0].clock, "01:00", "Sydney is UTC+11 in January");
        assert_eq!(v.rows[1].clock, "23:00");
        assert_eq!(v.rows[2].clock, "14:00");
        assert_eq!(v.rows[3].clock, "09:00");
        assert_eq!(v.rows[5].clock, "15:00");
        assert_eq!(v.rows[5].state, "Berlin");
        assert_eq!(v.overlap.tone, Tone::Open);
        assert!(v.overlap.text.contains("ends in 3h"), "{}", v.overlap.text);
    }

    #[test]
    fn device_row_stays_separate_even_when_it_matches_a_session_zone() {
        let opts = ViewOptions {
            device: DeviceInfo { iana: Some("Australia/Sydney".into()), source: DeviceSource::Auto, windows_id: None, territory: None },
            ..ViewOptions::default()
        };
        let v = build_view(utc("2025-01-15T14:00:00Z"), &opts);
        assert_eq!(v.rows.len(), 6);
        assert_eq!(v.rows[0].clock, v.rows[5].clock, "both show Sydney time");
        assert!(v.rows[5].device);
        assert!(!v.rows[0].device);
        assert_eq!(v.rows[5].detail, "auto-detected");
    }

    #[test]
    fn failed_mapping_shows_unavailable_not_a_guess() {
        let opts = ViewOptions {
            device: DeviceInfo::unavailable(Some("Nowhere Standard Time".into()), None),
            ..ViewOptions::default()
        };
        let v = build_view(utc("2025-01-15T14:00:00Z"), &opts);
        let d = v.rows.last().unwrap();
        assert_eq!(d.clock, "--:--");
        assert_eq!(d.state, "unavailable");
        assert_eq!(d.tone, Tone::Unknown);
    }

    #[test]
    fn broker_unverified_is_marked_unknown_with_an_estimate() {
        let v = build_view(utc("2025-01-15T12:00:00Z"), &ViewOptions::default());
        let b = &v.rows[4];
        assert_eq!(b.title, "BROKER");
        assert_eq!(b.state, "UTC+0");
        assert!(b.detail.starts_with("unknown"), "{}", b.detail);
        assert!(b.detail.contains("est"), "{}", b.detail);
    }

    #[test]
    fn verified_override_shows_a_real_countdown() {
        let rule = BrokerRule { verified: true, winter_offset_min: 120, summer_offset_min: 120, reopen_min: 0, ..BROKER_DEFAULT };
        let status = rule.status_at(utc("2025-01-15T12:00:00Z"));
        assert_eq!(status.next_reopen, Some(utc("2025-01-18T22:00:00Z")), "default preset reopens on Sunday");
        let opts = ViewOptions { broker_offset_min: Some(120), broker_reopen_min: Some(0), broker_rule_verified: true, ..ViewOptions::default() };
        let v = build_view(utc("2025-01-15T12:00:00Z"), &opts);
        assert_eq!(v.rows[4].detail, "reopens in 3d 10h");
    }

    #[test]
    fn countdown_formatting() {
        let now = utc("2025-01-15T12:00:00Z");
        assert_eq!(countdown(now, now + Duration::seconds(45)), "45s");
        assert_eq!(countdown(now, now + Duration::minutes(12)), "12m");
        assert_eq!(countdown(now, now + Duration::minutes(125)), "2h 05m");
        assert_eq!(countdown(now, now + Duration::days(2) + Duration::hours(4)), "2d 4h");
        assert_eq!(countdown(now, now - Duration::hours(1)), "0s");
    }

    #[test]
    fn offset_labels() {
        assert_eq!(offset_label(0), "UTC+0");
        assert_eq!(offset_label(120), "UTC+2");
        assert_eq!(offset_label(-300), "UTC-5");
        assert_eq!(offset_label(330), "UTC+5:30");
    }

    #[test]
    fn caveat_names_the_data_versions() {
        let v = build_view(utc("2025-01-15T12:00:00Z"), &ViewOptions::default());
        assert!(v.caveat.contains(TZDB_VERSION));
        assert!(v.caveat.contains(SPEC_VERSION));
        assert!(v.caveat.contains("no holidays"));
        assert!(v.caveat.contains("2024\u{2013}2036"));
        assert!(v.caveat.contains("CLDR 48"));
        assert!(v.caveat.contains("broker configured"));
    }

    #[test]
    fn hhmm_helpers() {
        assert_eq!(hhmm(0), "00:00");
        assert_eq!(hhmm(9 * 60 + 5), "09:05");
        assert_eq!(hhmm_label(22 * 60), "22:00");
    }
}
