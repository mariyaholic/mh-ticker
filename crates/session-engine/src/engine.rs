// SPDX-License-Identifier: MIT
//! Session intervals, statuses, the London/New York overlap and broker reopen.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

use crate::{
    coverage_contains, BrokerRule, SessionRule, BROKER_DEFAULT, COVERAGE_FROM, COVERAGE_TO,
    OVERLAP_PAIR,
};

/// A UTC interval, half-open: `start <= t < end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Interval {
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.start <= at && at < self.end
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boundary {
    pub at: DateTime<Utc>,
    pub kind: BoundaryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStatus {
    pub id: &'static str,
    pub active: Option<Interval>,
    pub next: Option<Boundary>,
    pub in_coverage: bool,
}

impl SessionStatus {
    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }
}

/// Resolve one local wall-clock minute according to the pinned policies:
/// ambiguous -> earliest; nonexistent -> move forward an hour until it exists.
pub fn resolve_local(
    zone: Tz,
    date: NaiveDate,
    minute_of_day: u16,
    nonexistent_shift: bool,
) -> Option<DateTime<Utc>> {
    let naive = date.and_hms_opt(minute_of_day as u32 / 60, minute_of_day as u32 % 60, 0)?;
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(a, _b) => Some(a.with_timezone(&Utc)),
        LocalResult::None if nonexistent_shift => {
            let mut shifted = naive;
            for _ in 0..4 {
                shifted += Duration::hours(1);
                match zone.from_local_datetime(&shifted) {
                    LocalResult::Single(dt) => return Some(dt.with_timezone(&Utc)),
                    LocalResult::Ambiguous(a, _b) => return Some(a.with_timezone(&Utc)),
                    LocalResult::None => {}
                }
            }
            None
        }
        LocalResult::None => None,
    }
}

fn date_days(from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    let mut d = from;
    std::iter::from_fn(move || {
        if d > to {
            None
        } else {
            let out = d;
            d += Duration::days(1);
            Some(out)
        }
    })
}

/// All intervals of `rule` that could touch `[from, to)`, in UTC.
pub fn intervals_between(
    rule: &SessionRule,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<Interval> {
    if to <= from {
        return Vec::new();
    }
    let zone = rule.zone();
    // Two local days either side cover overnight sessions and the date-line shift.
    let first = from.with_timezone(&zone).date_naive() - Duration::days(2);
    let last = to.with_timezone(&zone).date_naive() + Duration::days(2);
    let mut out = Vec::new();
    for date in date_days(first, last) {
        if !rule.is_day(date.weekday()) {
            continue;
        }
        let Some(start) = resolve_local(zone, date, rule.open_min, true) else {
            continue;
        };
        let close_date = if rule.close_min <= rule.open_min {
            date + Duration::days(1)
        } else {
            date
        };
        let Some(end) = resolve_local(zone, close_date, rule.close_min, true) else {
            continue;
        };
        if start < end && start < to && end > from {
            out.push(Interval { start, end });
        }
    }
    out.sort_by_key(|i| i.start);
    out.dedup();
    out
}

/// The session's state at `now`, and its next open or close boundary.
pub fn status_at(rule: &SessionRule, now: DateTime<Utc>) -> SessionStatus {
    let intervals = intervals_between(rule, now - Duration::days(3), now + Duration::days(10));
    let active = intervals.iter().copied().find(|i| i.contains(now));
    let next = if let Some(i) = active {
        Some(Boundary {
            at: i.end,
            kind: BoundaryKind::Close,
        })
    } else {
        intervals
            .iter()
            .filter(|i| i.start > now)
            .min_by_key(|i| i.start)
            .map(|i| Boundary {
                at: i.start,
                kind: BoundaryKind::Open,
            })
    };
    SessionStatus {
        id: rule.id,
        active,
        next,
        in_coverage: coverage_contains(now),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlapStatus {
    pub first: &'static str,
    pub second: &'static str,
    pub active: Option<Interval>,
    pub next: Option<Interval>,
    pub in_coverage: bool,
}

pub fn overlap_at(
    first: &'static SessionRule,
    second: &'static SessionRule,
    now: DateTime<Utc>,
) -> OverlapStatus {
    let a = intervals_between(first, now - Duration::days(3), now + Duration::days(10));
    let b = intervals_between(second, now - Duration::days(3), now + Duration::days(10));
    let mut both = Vec::new();
    for x in &a {
        for y in &b {
            let start = x.start.max(y.start);
            let end = x.end.min(y.end);
            if start < end {
                both.push(Interval { start, end });
            }
        }
    }
    both.sort_by_key(|i| i.start);
    let active = both.iter().copied().find(|i| i.contains(now));
    let next = both
        .iter()
        .copied()
        .find(|i| i.start > now)
        .or_else(|| active.filter(|i| i.end > now));
    OverlapStatus {
        first: first.id,
        second: second.id,
        active,
        next,
        in_coverage: coverage_contains(now),
    }
}

pub fn default_overlap(now: DateTime<Utc>) -> OverlapStatus {
    let first = crate::session(OVERLAP_PAIR[0]).expect("generated overlap session");
    let second = crate::session(OVERLAP_PAIR[1]).expect("generated overlap session");
    overlap_at(first, second, now)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReopenConfidence {
    /// The rule was approved/verified and the instant is inside generated coverage.
    Verified,
    /// The rule is a preset/provenance note that has not been confirmed.
    Unverified,
    /// The calculation falls outside the generated tzdb range.
    OutsideCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokerStatus {
    pub offset_min: i32,
    /// The server clock as an instant: UTC plus the server offset. Format it as UTC.
    pub server_now: DateTime<Utc>,
    /// Only present when the rule is verified and inside coverage.
    pub next_reopen: Option<DateTime<Utc>>,
    /// Always present when a rule can be evaluated, for transparency.
    pub estimate: Option<DateTime<Utc>>,
    pub confidence: ReopenConfidence,
    pub note: &'static str,
}

impl BrokerRule {
    /// The next local reopen instant, independent of the verification state.
    pub fn next_reopen_estimate(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Server-local dates around now. Use the current offset as the first guess.
        let guess = self.offset_at(now);
        let local_now = now + Duration::minutes(guess as i64);
        let first = local_now.date_naive() - Duration::days(2);
        let last = local_now.date_naive() + Duration::days(10);
        let mut best: Option<DateTime<Utc>> = None;
        for date in date_days(first, last) {
            if !self.reopen_days[date.weekday().num_days_from_monday() as usize] {
                continue;
            }
            let Some(naive) =
                date.and_hms_opt(self.reopen_min as u32 / 60, self.reopen_min as u32 % 60, 0)
            else {
                continue;
            };
            let naive_utc = Utc.from_utc_datetime(&naive);
            // Try the candidate offsets; accept only one that maps back to the same wall clock.
            let mut offsets = vec![self.winter_offset_min, self.summer_offset_min, guess];
            offsets.sort_unstable();
            offsets.dedup();
            for off in offsets {
                let candidate = naive_utc - Duration::minutes(off as i64);
                if candidate <= now
                    || !coverage_contains(candidate)
                    || self.offset_at(candidate) != off
                {
                    continue;
                }
                if best.is_none_or(|b| candidate < b) {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    pub fn status_at(&self, now: DateTime<Utc>) -> BrokerStatus {
        let offset = self.offset_at(now);
        let estimate = self.next_reopen_estimate(now);
        let in_coverage = coverage_contains(now) && estimate.is_some_and(coverage_contains);
        let (next_reopen, confidence, note) = if !in_coverage {
            (
                None,
                ReopenConfidence::OutsideCoverage,
                "outside generated tzdb coverage",
            )
        } else if !self.verified {
            (None, ReopenConfidence::Unverified, "broker rule unverified")
        } else {
            (estimate, ReopenConfidence::Verified, "verified rule")
        };
        BrokerStatus {
            offset_min: offset,
            server_now: now + Duration::minutes(offset as i64),
            next_reopen,
            estimate,
            confidence,
            note,
        }
    }
}

pub fn default_broker(now: DateTime<Utc>) -> BrokerStatus {
    BROKER_DEFAULT.status_at(now)
}

/// The generated coverage range, for documentation and tests.
pub fn coverage() -> (i64, i64) {
    (COVERAGE_FROM, COVERAGE_TO)
}

/// A readable short zone name: the last IANA component, underscores spaced.
pub fn short_zone(iana: &str) -> String {
    if iana == "Etc/UTC" || iana == "UTC" {
        return "UTC".into();
    }
    iana.rsplit('/').next().unwrap_or(iana).replace('_', " ")
}

/// `UTC+2`, `UTC-5`, `UTC+5:30`.
pub fn offset_label(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.abs();
    if m % 60 == 0 {
        format!("UTC{sign}{}", m / 60)
    } else {
        format!("UTC{sign}{}:{:02}", m / 60, m % 60)
    }
}

/// Parse the day-of-week list used by the spec (1 = Monday .. 7 = Sunday).
pub fn weekday_from_iso(d: u8) -> Option<Weekday> {
    Some(match d {
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        6 => Weekday::Sat,
        7 => Weekday::Sun,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn half_open_interval_at_open_and_close() {
        let ny = session("new_york").unwrap();
        let open = utc("2025-01-15T13:00:00Z");
        let close = utc("2025-01-15T22:00:00Z");
        let at_open = status_at(ny, open);
        assert_eq!(at_open.active.unwrap().start, open);
        assert_eq!(at_open.next.unwrap().at, close);
        assert_eq!(at_open.next.unwrap().kind, BoundaryKind::Close);
        let at_close = status_at(ny, close);
        assert!(!at_close.is_open(), "close is excluded from [open, close)");
        assert_eq!(at_close.next.unwrap().at, utc("2025-01-16T13:00:00Z"));
        assert_eq!(at_close.next.unwrap().kind, BoundaryKind::Open);
    }

    #[test]
    fn weekend_reopen_is_monday_local() {
        let ny = session("new_york").unwrap();
        let sat = status_at(ny, utc("2025-01-18T12:00:00Z"));
        assert!(!sat.is_open());
        assert_eq!(sat.next.unwrap().at, utc("2025-01-20T13:00:00Z"));
        let sydney = session("sydney").unwrap();
        let sat = status_at(sydney, utc("2025-01-18T12:00:00Z"));
        assert_eq!(
            sat.next.unwrap().at,
            utc("2025-01-19T20:00:00Z"),
            "Monday 07:00 AEDT"
        );
    }

    #[test]
    fn dst_transitions_for_all_four_zones() {
        let london = session("london").unwrap();
        let before = status_at(london, utc("2025-03-28T09:00:00Z"));
        assert!(before.is_open());
        assert_eq!(before.next.unwrap().at, utc("2025-03-28T17:00:00Z"));
        let after = status_at(london, utc("2025-03-31T09:00:00Z"));
        assert!(
            after.is_open(),
            "Monday 10:00 BST is still inside 08:00-17:00 local"
        );
        assert_eq!(after.next.unwrap().at, utc("2025-03-31T16:00:00Z"));

        let ny = session("new_york").unwrap();
        let before = status_at(ny, utc("2025-03-07T14:00:00Z"));
        assert!(before.is_open());
        assert_eq!(before.next.unwrap().at, utc("2025-03-07T22:00:00Z"));
        let after = status_at(ny, utc("2025-03-10T13:30:00Z"));
        assert!(after.is_open(), "Monday 09:30 EDT is inside 08:00-17:00");
        assert_eq!(after.next.unwrap().at, utc("2025-03-10T21:00:00Z"));

        let sydney = session("sydney").unwrap();
        let before = status_at(sydney, utc("2025-04-04T00:00:00Z"));
        assert!(before.is_open());
        assert_eq!(before.next.unwrap().at, utc("2025-04-04T05:00:00Z"));
        let after = status_at(sydney, utc("2025-04-07T00:00:00Z"));
        assert!(after.is_open());
        assert_eq!(after.next.unwrap().at, utc("2025-04-07T06:00:00Z"));

        let tokyo = session("tokyo").unwrap();
        let winter = status_at(tokyo, utc("2025-01-15T08:00:00Z"));
        let summer = status_at(tokyo, utc("2025-07-15T08:00:00Z"));
        assert_eq!(winter.next.unwrap().at, utc("2025-01-15T09:00:00Z"));
        assert_eq!(
            summer.next.unwrap().at,
            utc("2025-07-15T09:00:00Z"),
            "Tokyo has no DST"
        );
    }

    #[test]
    fn ambiguous_and_nonexistent_local_times_follow_the_policy() {
        let ny = chrono_tz::America::New_York;
        // 2025-11-02 01:30 occurs twice (EDT then EST); earliest = 05:30Z.
        let amb =
            resolve_local(ny, NaiveDate::from_ymd_opt(2025, 11, 2).unwrap(), 90, true).unwrap();
        assert_eq!(amb, utc("2025-11-02T05:30:00Z"));
        // 2025-03-09 02:30 does not exist; move forward to 03:30 EDT = 07:30Z.
        let gap =
            resolve_local(ny, NaiveDate::from_ymd_opt(2025, 3, 9).unwrap(), 150, true).unwrap();
        assert_eq!(gap, utc("2025-03-09T07:30:00Z"));
    }

    #[test]
    fn london_new_york_overlap_handles_both_mismatch_weeks() {
        let active = default_overlap(utc("2025-01-15T14:00:00Z"));
        assert!(active.active.is_some());
        assert_eq!(active.active.unwrap().end, utc("2025-01-15T17:00:00Z"));

        // US DST starts 9 March, UK 30 March: the overlap starts at 12:00 UTC.
        let march = default_overlap(utc("2025-03-12T12:00:00Z"));
        assert!(march.active.is_some());
        assert_eq!(march.active.unwrap().start, utc("2025-03-12T12:00:00Z"));
        assert_eq!(march.active.unwrap().end, utc("2025-03-12T17:00:00Z"));

        // UK ends 26 October, US 2 November: same one-hour mismatch.
        let october = default_overlap(utc("2025-10-28T12:00:00Z"));
        assert_eq!(october.active.unwrap().start, utc("2025-10-28T12:00:00Z"));

        let weekend = default_overlap(utc("2025-01-18T12:00:00Z"));
        assert!(weekend.active.is_none());
        assert_eq!(weekend.next.unwrap().start, utc("2025-01-20T13:00:00Z"));
    }

    #[test]
    fn broker_default_is_unverified_and_never_invents_a_verified_reopen() {
        let b = default_broker(utc("2025-01-15T12:00:00Z"));
        assert_eq!(b.confidence, ReopenConfidence::Unverified);
        assert!(b.next_reopen.is_none());
        assert!(b.estimate.is_some());
        assert_eq!(b.offset_min, 0);
        assert_eq!(b.server_now, utc("2025-01-15T12:00:00Z"));
    }

    #[test]
    fn verified_broker_rule_handles_us_dst_and_weekends() {
        let rule = BrokerRule {
            winter_offset_min: 120,
            summer_offset_min: 180,
            seasonal_us_dst: true,
            reopen_min: 0,
            reopen_days: [true, true, true, true, true, false, false],
            verified: true,
            ..BROKER_DEFAULT
        };
        let winter = rule.status_at(utc("2025-01-15T12:00:00Z"));
        assert_eq!(winter.offset_min, 120);
        assert_eq!(winter.server_now, utc("2025-01-15T14:00:00Z"));
        assert_eq!(winter.next_reopen.unwrap(), utc("2025-01-15T22:00:00Z"));

        let summer = rule.status_at(utc("2025-03-12T12:00:00Z"));
        assert_eq!(summer.offset_min, 180);
        assert_eq!(summer.next_reopen.unwrap(), utc("2025-03-12T21:00:00Z"));

        let friday = rule.status_at(utc("2025-01-17T21:00:00Z"));
        assert_eq!(
            friday.next_reopen.unwrap(),
            utc("2025-01-19T22:00:00Z"),
            "next Monday 00:00 server"
        );
    }

    #[test]
    fn outside_coverage_is_reported() {
        let ny = session("new_york").unwrap();
        assert!(!status_at(ny, utc("2040-01-15T14:00:00Z")).in_coverage);
        assert!(!default_overlap(utc("2040-01-15T14:00:00Z")).in_coverage);
        let b = default_broker(utc("2040-01-15T12:00:00Z"));
        assert_eq!(b.confidence, ReopenConfidence::OutsideCoverage);
        assert!(b.next_reopen.is_none());
    }
}
