// SPDX-License-Identifier: MIT
//! Generates the checked-in Rust, Pine v6 and MQL5 artifacts from `spec/sessions.json`.
//!
//!     cargo run -p mh-specgen -- --check
//!
//! `--check` writes nothing and exits non-zero when a generated file has drifted.
//! The generated files are build inputs: never edit them by hand.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Spec {
    spec_version: String,
    tzdb_version: String,
    effective_from: String,
    effective_to: String,
    interval: String,
    ambiguous_local: String,
    nonexistent_local: String,
    sessions: Vec<SessionSpec>,
    overlap: Vec<String>,
    holidays: Holidays,
    broker: BrokerSpec,
}

#[derive(Debug, Deserialize)]
struct SessionSpec {
    id: String,
    label: String,
    iana: String,
    open: String,
    close: String,
    days: Vec<u8>,
    overnight: bool,
    provenance: String,
}

#[derive(Debug, Deserialize)]
struct Holidays {
    policy: String,
    provenance: String,
}

#[derive(Debug, Deserialize)]
struct BrokerSpec {
    id: String,
    label: String,
    verified: bool,
    provenance: String,
    winter_offset_minutes: i32,
    summer_offset_minutes: i32,
    seasonal: String,
    reopen_local: String,
    reopen_days: Vec<u8>,
    reopen_note: String,
}

#[derive(Debug)]
struct Transition {
    at: i64,
    offset_s: i32,
}

struct Generated {
    path: PathBuf,
    text: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let root = args
        .iter()
        .position(|a| a == "--root")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match run(&root, check) {
        Ok(files) => {
            for f in &files {
                if check {
                    println!("ok  {}", f.display());
                } else {
                    println!("wrote {}", f.display());
                }
            }
            if check {
                println!("generated artifacts are up to date");
            }
        }
        Err(e) => {
            eprintln!("gen-spec: {e}");
            std::process::exit(1);
        }
    }
}

fn run(root: &Path, check: bool) -> Result<Vec<PathBuf>, String> {
    let spec_path = root.join("spec/sessions.json");
    let text =
        std::fs::read_to_string(&spec_path).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    let spec: Spec =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    validate(&spec)?;

    let from = parse_utc(&spec.effective_from)?;
    let to = parse_utc(&spec.effective_to)?;
    if from >= to {
        return Err("effective_from must be before effective_to".into());
    }

    let mut files = Vec::new();
    files.push(Generated {
        path: root.join("crates/session-engine/src/generated_rules.rs"),
        text: rust_rules(&spec, from, to)?,
    });
    files.push(Generated {
        path: root.join("crates/session-engine/src/generated_tables.rs"),
        text: rust_tables(&spec, from, to)?,
    });

    let pine = root.join("crates/specgen/templates/MH_Sessions.pine.in");
    if pine.exists() {
        let template =
            std::fs::read_to_string(&pine).map_err(|e| format!("{}: {e}", pine.display()))?;
        files.push(Generated {
            path: root.join("pine/MH_Sessions.pine"),
            text: fill(&template, &spec, from, to)?,
        });
    }
    let mq5 = root.join("crates/specgen/templates/MH_Sessions.mq5.in");
    if mq5.exists() {
        let template =
            std::fs::read_to_string(&mq5).map_err(|e| format!("{}: {e}", mq5.display()))?;
        files.push(Generated {
            path: root.join("mql5/MH_Sessions.mq5"),
            text: fill(&template, &spec, from, to)?,
        });
    }

    let mut changed = Vec::new();
    for f in &files {
        let old = std::fs::read_to_string(&f.path).unwrap_or_default();
        if old != f.text {
            changed.push(f.path.clone());
            if !check {
                if let Some(dir) = f.path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(&f.path, &f.text)
                    .map_err(|e| format!("{}: {e}", f.path.display()))?;
            }
        }
    }
    if check && !changed.is_empty() {
        return Err(format!(
            "generated files are stale (run `cargo run -p mh-specgen`): {}",
            changed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(files.into_iter().map(|f| f.path).collect())
}

fn validate(spec: &Spec) -> Result<(), String> {
    if spec.sessions.len() != 4 {
        return Err(format!(
            "v1 needs exactly four sessions, found {}",
            spec.sessions.len()
        ));
    }
    if spec.interval != "half-open" {
        return Err("only the half-open [open, close) interval is supported".into());
    }
    if spec.ambiguous_local != "earliest" {
        return Err("only the 'earliest' ambiguous-local policy is supported".into());
    }
    if spec.nonexistent_local != "shift_forward_1h" {
        return Err("only the 'shift_forward_1h' nonexistent-local policy is supported".into());
    }
    if spec.holidays.policy != "none" {
        return Err("v1 supports no holiday calendar; set holidays.policy to 'none'".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for s in &spec.sessions {
        if !ids.insert(s.id.as_str()) {
            return Err(format!("duplicate session id {}", s.id));
        }
        s.iana
            .parse::<Tz>()
            .map_err(|_| format!("{}: unknown IANA zone {}", s.id, s.iana))?;
        parse_hm(&s.open)?;
        parse_hm(&s.close)?;
        if s.days.is_empty() || s.days.iter().any(|d| !(1..=7).contains(d)) {
            return Err(format!("{}: days must be 1..7 (Mon..Sun)", s.id));
        }
        if s.open == s.close {
            return Err(format!("{}: open and close must differ", s.id));
        }
    }
    if spec.overlap.len() != 2 {
        return Err("exactly one overlap pair is supported in v1".into());
    }
    for id in &spec.overlap {
        if !ids.contains(id.as_str()) {
            return Err(format!("overlap id {id} is not a session"));
        }
    }
    let broker = &spec.broker;
    parse_hm(&broker.reopen_local)?;
    if !matches!(broker.seasonal.as_str(), "fixed" | "us_dst") {
        return Err("broker.seasonal must be 'fixed' or 'us_dst'".into());
    }
    if broker.reopen_days.is_empty() || broker.reopen_days.iter().any(|d| !(1..=7).contains(d)) {
        return Err("broker.reopen_days must be 1..7 (Mon..Sun)".into());
    }
    Ok(())
}

fn parse_utc(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| format!("{s}: {e}"))
}

fn parse_hm(s: &str) -> Result<(u16, u16), String> {
    let t = NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| format!("{s}: {e}"))?;
    Ok((
        t.format("%H").to_string().parse().unwrap(),
        t.format("%M").to_string().parse().unwrap(),
    ))
}

fn minutes(s: &str) -> Result<u16, String> {
    let (h, m) = parse_hm(s)?;
    Ok(h * 60 + m)
}

fn days_array(days: &[u8]) -> [bool; 7] {
    let mut out = [false; 7];
    for &d in days {
        out[(d - 1) as usize] = true;
    }
    out
}

fn rust_bool(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

fn rust_rules(spec: &Spec, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "// SPDX-License-Identifier: MIT").unwrap();
    writeln!(
        out,
        "// @generated by `cargo run -p mh-specgen` from spec/sessions.json. Do not edit."
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "pub const SPEC_VERSION: &str = {};",
        rust_str(&spec.spec_version)
    )
    .unwrap();
    writeln!(
        out,
        "pub const TZDB_VERSION: &str = {};",
        rust_str(&spec.tzdb_version)
    )
    .unwrap();
    writeln!(out, "pub const COVERAGE_FROM: i64 = {};", from.timestamp()).unwrap();
    writeln!(out, "pub const COVERAGE_TO: i64 = {};", to.timestamp()).unwrap();
    writeln!(
        out,
        "pub const HOLIDAY_POLICY: &str = {};",
        rust_str(&spec.holidays.policy)
    )
    .unwrap();
    writeln!(
        out,
        "pub const HOLIDAY_PROVENANCE: &str = {};",
        rust_str(&spec.holidays.provenance)
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "pub static SESSION_RULES: [SessionRule; {}] = [",
        spec.sessions.len()
    )
    .unwrap();
    for s in &spec.sessions {
        let days = days_array(&s.days);
        let d: Vec<&str> = days.iter().map(|b| rust_bool(*b)).collect();
        writeln!(out, "    SessionRule {{").unwrap();
        writeln!(out, "        id: {},", rust_str(&s.id)).unwrap();
        writeln!(out, "        label: {},", rust_str(&s.label)).unwrap();
        writeln!(out, "        iana: {},", rust_str(&s.iana)).unwrap();
        writeln!(out, "        open_min: {},", minutes(&s.open)?).unwrap();
        writeln!(out, "        close_min: {},", minutes(&s.close)?).unwrap();
        writeln!(out, "        days: [{}],", d.join(", ")).unwrap();
        writeln!(out, "        overnight: {},", rust_bool(s.overnight)).unwrap();
        writeln!(out, "        provenance: {},", rust_str(&s.provenance)).unwrap();
        writeln!(out, "    }},").unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();
    let pair: Vec<String> = spec.overlap.iter().map(|s| rust_str(s)).collect();
    writeln!(
        out,
        "pub static OVERLAP_PAIR: [&str; 2] = [{}];",
        pair.join(", ")
    )
    .unwrap();
    writeln!(out).unwrap();
    let b = &spec.broker;
    let bdays = days_array(&b.reopen_days);
    let bd: Vec<&str> = bdays.iter().map(|x| rust_bool(*x)).collect();
    writeln!(out, "pub static BROKER_DEFAULT: BrokerRule = BrokerRule {{").unwrap();
    writeln!(out, "    id: {},", rust_str(&b.id)).unwrap();
    writeln!(out, "    label: {},", rust_str(&b.label)).unwrap();
    writeln!(out, "    verified: {},", rust_bool(b.verified)).unwrap();
    writeln!(out, "    provenance: {},", rust_str(&b.provenance)).unwrap();
    writeln!(out, "    winter_offset_min: {},", b.winter_offset_minutes).unwrap();
    writeln!(out, "    summer_offset_min: {},", b.summer_offset_minutes).unwrap();
    writeln!(
        out,
        "    seasonal_us_dst: {},",
        rust_bool(b.seasonal == "us_dst")
    )
    .unwrap();
    writeln!(out, "    reopen_min: {},", minutes(&b.reopen_local)?).unwrap();
    writeln!(out, "    reopen_days: [{}],", bd.join(", ")).unwrap();
    writeln!(out, "    reopen_note: {},", rust_str(&b.reopen_note)).unwrap();
    writeln!(out, "}};").unwrap();
    Ok(out)
}

fn offset_at(tz: Tz, t: DateTime<Utc>) -> i32 {
    use chrono::Offset;
    tz.offset_from_utc_datetime(&t.naive_utc())
        .fix()
        .local_minus_utc()
}

fn transitions(tz: Tz, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Transition> {
    let mut out = Vec::new();
    let mut day = from;
    while day < to {
        let next = (day + Duration::days(1)).min(to);
        let o0 = offset_at(tz, day);
        let o1 = offset_at(tz, next);
        if o0 != o1 {
            // First whole second in the window whose offset differs from o0.
            let (mut lo, mut hi) = (day.timestamp(), next.timestamp());
            while lo + 1 < hi {
                let mid = lo + (hi - lo) / 2;
                let t = Utc.timestamp_opt(mid, 0).single().unwrap();
                if offset_at(tz, t) == o0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let at = Utc.timestamp_opt(hi, 0).single().unwrap();
            out.push(Transition {
                at: hi,
                offset_s: offset_at(tz, at),
            });
        }
        day = next;
    }
    out
}

fn unique_zones(spec: &Spec) -> Vec<(String, Vec<Transition>)> {
    let from = parse_utc(&spec.effective_from).unwrap();
    let to = parse_utc(&spec.effective_to).unwrap();
    let mut zones = BTreeMap::new();
    for s in &spec.sessions {
        zones
            .entry(s.iana.clone())
            .or_insert_with(|| transitions(s.iana.parse().unwrap(), from, to));
    }
    zones.into_iter().collect()
}

fn rust_tables(spec: &Spec, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "// SPDX-License-Identifier: MIT").unwrap();
    writeln!(
        out,
        "// @generated by `cargo run -p mh-specgen` from spec/sessions.json. Do not edit."
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "pub const TABLE_FROM: i64 = {};", from.timestamp()).unwrap();
    writeln!(out, "pub const TABLE_TO: i64 = {};", to.timestamp()).unwrap();
    writeln!(out).unwrap();
    for (i, (iana, ts)) in unique_zones(spec).iter().enumerate() {
        let tz: Tz = iana.parse().unwrap();
        writeln!(out, "static T{i}_BASE: i32 = {};", offset_at(tz, from)).unwrap();
        writeln!(out, "static T{i}_TRANSITIONS: &[Transition] = &[").unwrap();
        for t in ts {
            writeln!(
                out,
                "    Transition {{ at: {}, offset_s: {} }},",
                t.at, t.offset_s
            )
            .unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "pub static ZONE_TABLES: &[ZoneTable] = &[").unwrap();
    for (i, (iana, _)) in unique_zones(spec).iter().enumerate() {
        writeln!(out, "    ZoneTable {{ iana: {}, base_offset_s: T{i}_BASE, transitions: T{i}_TRANSITIONS }},", rust_str(iana)).unwrap();
    }
    writeln!(out, "];").unwrap();
    Ok(out)
}

fn fill(
    template: &str,
    spec: &Spec,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<String, String> {
    let mut s = template.to_string();
    let mut pine_sessions = String::new();
    let mut mq5_sessions = String::new();
    for x in &spec.sessions {
        let up = x.id.to_uppercase();
        writeln!(pine_sessions, "// {} ({})", x.label, x.iana).unwrap();
        writeln!(
            pine_sessions,
            "const string MH_SESSION_{up} = \"{}\"",
            pine_session(x)?
        )
        .unwrap();
        writeln!(pine_sessions, "const string MH_ZONE_{up} = \"{}\"", x.iana).unwrap();
        writeln!(
            pine_sessions,
            "const string MH_WINDOW_{up} = \"{}-{}\"",
            x.open, x.close
        )
        .unwrap();
        writeln!(pine_sessions).unwrap();
        writeln!(
            mq5_sessions,
            "const string MH_LABEL_{up} = \"{}\";",
            x.label
        )
        .unwrap();
        writeln!(mq5_sessions, "const string MH_ZONE_{up} = \"{}\";", x.iana).unwrap();
        writeln!(
            mq5_sessions,
            "const int MH_OPEN_{up} = {};",
            minutes(&x.open)?
        )
        .unwrap();
        writeln!(
            mq5_sessions,
            "const int MH_CLOSE_{up} = {};",
            minutes(&x.close)?
        )
        .unwrap();
        writeln!(mq5_sessions, "const int MH_DAYS_{up} = {};", mask(&x.days)).unwrap();
        writeln!(
            mq5_sessions,
            "const bool MH_OVERNIGHT_{up} = {};",
            rust_bool(x.overnight)
        )
        .unwrap();
        writeln!(mq5_sessions).unwrap();
    }
    let broker = &spec.broker;
    let pine_reopen_day = if broker.reopen_days[0] == 7 {
        1
    } else {
        broker.reopen_days[0] + 1
    };
    let pine_broker = format!(
        "const bool MH_BROKER_VERIFIED = {}\nconst int MH_BROKER_OFFSET_MIN = {}\nconst int MH_BROKER_REOPEN_MIN = {}\nconst int MH_BROKER_REOPEN_DAY = {}",
        rust_bool(broker.verified),
        broker.winter_offset_minutes,
        minutes(&broker.reopen_local)?,
        pine_reopen_day
    );
    let mut mq5_constants = String::new();
    writeln!(
        mq5_constants,
        "const bool MH_BROKER_VERIFIED = {};",
        rust_bool(broker.verified)
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const int MH_BROKER_WINTER_OFFSET = {};",
        broker.winter_offset_minutes
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const int MH_BROKER_SUMMER_OFFSET = {};",
        broker.summer_offset_minutes
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const bool MH_BROKER_US_DST = {};",
        rust_bool(broker.seasonal == "us_dst")
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const int MH_BROKER_REOPEN = {};",
        minutes(&broker.reopen_local)?
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const int MH_BROKER_DAYS = {};",
        mask(&broker.reopen_days)
    )
    .unwrap();
    writeln!(mq5_constants).unwrap();
    for (i, (iana, ts)) in unique_zones(spec).iter().enumerate() {
        let tz: Tz = iana.parse().unwrap();
        let base = offset_at(tz, from);
        writeln!(mq5_constants, "// {iana}").unwrap();
        let ats = if ts.is_empty() {
            "0".to_string()
        } else {
            ts.iter()
                .map(|t| t.at.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let offs = if ts.is_empty() {
            base.to_string()
        } else {
            ts.iter()
                .map(|t| t.offset_s.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        writeln!(mq5_constants, "long MH_AT_{i}[] = {{{ats}}};").unwrap();
        writeln!(mq5_constants, "int MH_OFF_{i}[] = {{{offs}}};").unwrap();
        writeln!(mq5_constants, "const int MH_BASE_{i} = {base};").unwrap();
        writeln!(mq5_constants).unwrap();
    }
    writeln!(
        mq5_constants,
        "const long MH_TABLE_FROM = {};",
        from.timestamp()
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const long MH_TABLE_TO = {};",
        to.timestamp()
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const string MH_SPEC_VERSION = \"{}\";",
        spec.spec_version
    )
    .unwrap();
    writeln!(
        mq5_constants,
        "const string MH_TZDB_VERSION = \"{}\";",
        spec.tzdb_version
    )
    .unwrap();
    let mut mq5_functions = String::new();
    writeln!(mq5_functions, "int MhOffsetByZone(string iana, long utc)").unwrap();
    writeln!(mq5_functions, "{{").unwrap();
    for (i, (iana, _)) in unique_zones(spec).iter().enumerate() {
        writeln!(
            mq5_functions,
            "   if(iana == \"{iana}\") return MhOffsetAt(utc, MH_AT_{i}, MH_OFF_{i}, MH_BASE_{i});"
        )
        .unwrap();
    }
    writeln!(mq5_functions, "   return MH_UNKNOWN;").unwrap();
    writeln!(mq5_functions, "}}").unwrap();
    let common = format!(
        "const string MH_SPEC_VERSION = \"{}\"\nconst string MH_TZDB_VERSION = \"{}\"",
        spec.spec_version, spec.tzdb_version
    );
    s = s.replace("{{SESSION_CONSTANTS}}", &pine_sessions);
    s = s.replace("{{PINE_SESSION_CONSTANTS}}", &pine_sessions);
    s = s.replace("{{PINE_CONSTANTS}}", &format!("{pine_broker}\n{common}"));
    s = s.replace("{{MQ5_SESSION_CONSTANTS}}", &mq5_sessions);
    s = s.replace("{{MQ5_CONSTANTS}}", &mq5_constants);
    s = s.replace("{{MQ5_FUNCTIONS}}", &mq5_functions);
    s = s.replace("{{SPEC_VERSION}}", &spec.spec_version);
    s = s.replace("{{TZDB_VERSION}}", &spec.tzdb_version);
    s = s.replace("{{EFFECTIVE_FROM}}", &spec.effective_from);
    s = s.replace("{{EFFECTIVE_TO}}", &spec.effective_to);
    Ok(s)
}

fn pine_session(s: &SessionSpec) -> Result<String, String> {
    // Pine session day numbers: 1=Sunday .. 7=Saturday; spec days are 1=Monday .. 7=Sunday.
    let days: Vec<String> = s
        .days
        .iter()
        .map(|d| {
            let pine = if *d == 7 { 1 } else { *d + 1 };
            pine.to_string()
        })
        .collect();
    let (oh, om) = parse_hm(&s.open)?;
    let (ch, cm) = parse_hm(&s.close)?;
    Ok(format!("{oh:02}{om:02}-{ch:02}{cm:02}:{}", days.join("")))
}

fn mask(days: &[u8]) -> u32 {
    days.iter().fold(0, |m, d| m | (1 << (d - 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pine_days_are_sunday_first() {
        let s = SessionSpec {
            id: "x".into(),
            label: "X".into(),
            iana: "Europe/London".into(),
            open: "08:00".into(),
            close: "17:00".into(),
            days: vec![1, 2, 3, 4, 5],
            overnight: false,
            provenance: String::new(),
        };
        // Mon..Fri in the spec is 2..6 in Pine's Sunday-first numbering.
        assert_eq!(pine_session(&s).unwrap(), "0800-1700:23456");
    }

    #[test]
    fn masks_are_monday_first() {
        assert_eq!(mask(&[1, 7]), 0b100_0001);
        assert_eq!(mask(&[1, 2, 3, 4, 5]), 0b001_1111);
    }

    #[test]
    fn checked_in_artifacts_are_current() {
        run(&PathBuf::from("../.."), true)
            .expect("run `cargo run -p mh-specgen` and commit the result");
    }

    #[test]
    fn generated_indicators_have_no_template_leftovers_or_empty_mql5_arrays() {
        let pine = std::fs::read_to_string("../../pine/MH_Sessions.pine").unwrap();
        let mq5 = std::fs::read_to_string("../../mql5/MH_Sessions.mq5").unwrap();
        for (name, text) in [("pine", &pine), ("mql5", &mq5)] {
            assert!(
                !text.contains("{{"),
                "{name} still has a template placeholder"
            );
            assert!(text.contains("MH_SESSION_SYDNEY") || text.contains("MH_OPEN_SYDNEY"));
        }
        assert!(!mq5.contains("[] = {};"), "MQL5 empty arrays are invalid");
        assert!(!pine
            .lines()
            .filter(|l| l.trim_start().starts_with("const "))
            .any(|l| l.ends_with(';')));
        assert!(pine.contains("0700-1600:23456"));
        assert!(
            pine.contains("MH_BROKER_REOPEN_DAY = 1"),
            "spec day 7 (Sunday) is Pine day 1"
        );
        assert!(pine.contains("syminfo.timezone"));
        assert!(pine.contains("not auto-detected"));
        assert!(mq5.contains("NOT AFFILIATED WITH METAMOQUOTES"));
    }
}
