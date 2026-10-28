// SPDX-License-Identifier: MIT
//! Generated, bounded IANA offset tables used by the indicator boilerplates.
//!
//! The session engine itself asks `chrono-tz` (the full compiled database). The
//! MQL5 indicator cannot, so it embeds these bounded one-offset-per-transition
//! tables instead and fails visibly outside `[TABLE_FROM, TABLE_TO)`.

/// One DST transition: from `at` onward the zone offset is `offset_s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub at: i64,
    pub offset_s: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ZoneTable {
    pub iana: &'static str,
    pub base_offset_s: i32,
    pub transitions: &'static [Transition],
}

include!("generated_tables.rs");

pub fn table_for(iana: &str) -> Option<&'static ZoneTable> {
    ZONE_TABLES.iter().find(|t| t.iana == iana)
}

/// Offset in seconds east of UTC at `unix`, or `None` outside coverage.
pub fn offset_at(table: &ZoneTable, unix: i64) -> Option<i32> {
    if !(TABLE_FROM..TABLE_TO).contains(&unix) {
        return None;
    }
    let idx = table.transitions.partition_point(|t| t.at <= unix);
    if idx == 0 {
        Some(table.base_offset_s)
    } else {
        Some(table.transitions[idx - 1].offset_s)
    }
}

/// UTC unix seconds -> wall clock for a fixed-offset display.
pub fn wall_clock(table: &ZoneTable, unix: i64) -> Option<i64> {
    offset_at(table, unix).map(|off| unix + off as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn chrono_offset(iana: &str, unix: i64) -> i32 {
        use chrono::Offset;
        let tz: chrono_tz::Tz = iana.parse().unwrap();
        let t = Utc.timestamp_opt(unix, 0).single().unwrap();
        tz.offset_from_utc_datetime(&t.naive_utc())
            .fix()
            .local_minus_utc()
    }

    #[test]
    fn generated_tables_match_chrono_tz_at_every_transition() {
        for table in ZONE_TABLES {
            let mut points = vec![TABLE_FROM];
            for tr in table.transitions {
                points.push(tr.at - 1);
                points.push(tr.at);
                points.push(tr.at + 1);
            }
            points.push(TABLE_TO - 1);
            for t in points {
                assert_eq!(
                    offset_at(table, t),
                    Some(chrono_offset(table.iana, t)),
                    "{} at {}",
                    table.iana,
                    t
                );
            }
        }
    }

    #[test]
    fn tables_are_bounded_and_ordered() {
        assert!(TABLE_FROM < TABLE_TO);
        for table in ZONE_TABLES {
            assert_eq!(offset_at(table, TABLE_FROM - 1), None);
            assert_eq!(offset_at(table, TABLE_TO), None);
            assert!(table.transitions.windows(2).all(|w| w[0].at < w[1].at));
            for tr in table.transitions {
                assert!(TABLE_FROM <= tr.at && tr.at < TABLE_TO);
            }
        }
    }

    #[test]
    fn generated_mql5_embeds_the_same_transition_numbers() {
        let text = include_str!("../../../mql5/MH_Sessions.mq5");
        assert!(text.contains("MhOffsetByZone"));
        assert!(!text.contains("[] = {};"));
        for table in ZONE_TABLES {
            for tr in table.transitions {
                assert!(
                    text.contains(&tr.at.to_string()),
                    "{} transition {} missing from MQL5",
                    table.iana,
                    tr.at
                );
                assert!(
                    text.contains(&tr.offset_s.to_string()),
                    "{} offset {} missing from MQL5",
                    table.iana,
                    tr.offset_s
                );
            }
        }
    }

    #[test]
    fn generated_pine_uses_the_shared_session_strings_and_a_manual_zone_input() {
        let text = include_str!("../../../pine/MH_Sessions.pine");
        assert!(text.contains("indicator("));
        assert!(text.contains("MH_SESSION_SYDNEY = \"0700-1600:23456\""));
        assert!(text.contains("input.string"));
        assert!(text.contains("syminfo.timezone"));
        assert!(text.contains("not auto-detected"));
        assert!(!text.contains("chart.timezone"));
    }

    #[test]
    fn wall_clock_examples() {
        let ny = table_for("America/New_York").unwrap();
        // 2025-01-15 13:00 UTC = 08:00 EST
        assert_eq!(
            wall_clock(ny, 1_736_946_000),
            Some(1_736_946_000 - 5 * 3600)
        );
        let london = table_for("Europe/London").unwrap();
        // 2025-07-15 07:00 UTC = 08:00 BST
        assert_eq!(
            wall_clock(london, 1_752_562_800),
            Some(1_752_562_800 + 3600)
        );
    }
}
