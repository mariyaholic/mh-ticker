// SPDX-License-Identifier: Apache-2.0
//! Platform-independent core for MH Ticker's session timers.
//!
//! The generated rules in [`SESSION_RULES`] come from `spec/sessions.json`; see
//! `docs/SESSIONS.md`. Everything here builds and tests on Linux. The Windows
//! device-zone adapter lives in the application crate behind `cfg(windows)`.
//!
//! Conventions:
//! - intervals are half-open `[open, close)`, so a close instant is already closed;
//! - ambiguous local times use the earliest occurrence;
//! - nonexistent local times move forward an hour until they exist;
//! - no holiday calendar is applied (v1 is weekdays only);
//! - all instants crossing the public API are UTC.

pub mod engine;
pub mod tables;
pub mod view;
pub mod zones;

use chrono::{DateTime, Duration, TimeZone, Utc, Weekday};
use chrono_tz::{OffsetComponents, Tz};

/// One ordered session rule, generated from the shared JSON spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRule {
    pub id: &'static str,
    pub label: &'static str,
    pub iana: &'static str,
    /// Local minutes after midnight.
    pub open_min: u16,
    pub close_min: u16,
    /// Monday-first weekdays.
    pub days: [bool; 7],
    pub overnight: bool,
    pub provenance: &'static str,
}

/// The configurable broker-server rule. `reopen_days` is Monday-first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrokerRule {
    pub id: &'static str,
    pub label: &'static str,
    pub verified: bool,
    pub provenance: &'static str,
    pub winter_offset_min: i32,
    pub summer_offset_min: i32,
    pub seasonal_us_dst: bool,
    pub reopen_min: u16,
    pub reopen_days: [bool; 7],
    pub reopen_note: &'static str,
}

include!("generated_rules.rs");

impl SessionRule {
    pub fn zone(&self) -> Tz {
        self.iana.parse().expect("generated IANA zone is valid")
    }

    pub fn is_day(&self, weekday: Weekday) -> bool {
        self.days[weekday.num_days_from_monday() as usize]
    }

    pub fn open_label(&self) -> String {
        hhmm(self.open_min)
    }

    pub fn close_label(&self) -> String {
        hhmm(self.close_min)
    }

    pub fn window_label(&self) -> String {
        format!("{}-{}", self.open_label(), self.close_label())
    }
}

impl BrokerRule {
    /// Apply the user's config overrides. An explicit offset always means a fixed
    /// clock; it never claims to be the broker's seasonal rule.
    pub fn with_overrides(&self, offset_min: Option<i32>, reopen_min: Option<u16>, verified: bool) -> BrokerRule {
        let mut r = *self;
        if let Some(off) = offset_min {
            r.winter_offset_min = off;
            r.summer_offset_min = off;
            r.seasonal_us_dst = false;
        }
        if let Some(m) = reopen_min {
            r.reopen_min = m;
        }
        if verified {
            r.verified = true;
        }
        r
    }

    /// The server offset in minutes at a UTC instant.
    pub fn offset_at(&self, at: DateTime<Utc>) -> i32 {
        if !self.seasonal_us_dst {
            return self.winter_offset_min;
        }
        let ny = chrono_tz::America::New_York;
        let local = at.with_timezone(&ny);
        if local.offset().dst_offset() != Duration::zero() {
            self.summer_offset_min
        } else {
            self.winter_offset_min
        }
    }
}

pub fn session(id: &str) -> Option<&'static SessionRule> {
    SESSION_RULES.iter().find(|s| s.id == id)
}

pub fn hhmm(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub fn coverage_contains(at: DateTime<Utc>) -> bool {
    let t = at.timestamp();
    (COVERAGE_FROM..COVERAGE_TO).contains(&t)
}

/// `2024–2036`, for the panel caveat and docs.
pub fn coverage_years() -> String {
    let from = Utc.timestamp_opt(COVERAGE_FROM, 0).single();
    let to = Utc.timestamp_opt(COVERAGE_TO, 0).single();
    match (from, to) {
        (Some(f), Some(t)) => format!("{}\u{2013}{}", f.format("%Y"), t.format("%Y")),
        _ => "unknown".into(),
    }
}
