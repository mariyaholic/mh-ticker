//! The judging probe (`--probe`, the diagnostics report): raw answers in, findings out. Pure and
//! portable: `std` and `serde_json`, plus the other pure modules of the crate. No request is made
//! here; `source::probe` collects the bodies and hands them to [`judge`].
//!
//! Each assumption the app makes about what CNBC and GitHub answer is one finding with a verdict:
//! `PASS` (seen to hold in these answers), `WARN` (seen NOT to hold, or the answer could not be
//! had) or `NOTE` (not testable from these answers, for instance whether a bar is stamped at its
//! start while the market is closed). A run exits non-zero on any `WARN` and never on a `NOTE`.
//!
//! Text in a finding is made of symbols, numbers and field names (cleaned and cut short), never of
//! a body, a URL or an error message from the network.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// A symbol that does not exist, asked for to see what an unknown symbol is answered with.
pub const UNKNOWN_SYMBOL: &str = "MHTICKER.NOSUCH";

/// The quote fields the app reads (`source::parse_quotes`).
pub const QUOTE_READ: [&str; 15] = [
    "symbol", "last", "name", "change_pct", "previous_day_closing", "last_time", "realTime", "open", "high", "low",
    "volume", "yrhiprice", "yrloprice", "yrhidate", "currencyCode",
];
/// The bar fields the app reads (`source::parse_chart` and `parse_series`).
pub const BAR_READ: [&str; 7] = ["tradeTimeinMills", "tradeTimeInMills", "close", "open", "high", "low", "volume"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Warn,
    Note,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Warn => "WARN",
            Verdict::Note => "NOTE",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The assumption's name: `quote-fields`, `unknown-symbol`, `time-spelling`, `bar-start`,
    /// `daily-boundary`, `volume`, `release-tag`, and `unread-fields` / `*-request` for the rest.
    pub id: &'static str,
    pub verdict: Verdict,
    pub text: String,
}

fn f(id: &'static str, verdict: Verdict, text: impl Into<String>) -> Finding {
    Finding { id, verdict, text: text.into() }
}

/// What the probe fetched. An `Err` is a fixed reason (`"network"`, `"HTTP 429"`, ...), never the
/// network's own text.
#[derive(Clone, Debug, Default)]
pub struct Bodies {
    /// UTC seconds now.
    pub now: f64,
    /// This app's version, to compare with the release.
    pub current: String,
    /// The symbols shown, as sent.
    pub shown: Vec<String>,
    /// One quote request for every shown symbol.
    pub quotes: Option<Result<String, String>>,
    /// One quote request for [`UNKNOWN_SYMBOL`].
    pub unknown: Option<Result<String, String>>,
    /// The 1D bar file of each probed row: `(symbol, answer)`.
    pub day: Vec<(String, Result<String, String>)>,
    /// The range files of the first row: `(file, answer)`.
    pub ranges: Vec<(String, Result<String, String>)>,
    /// The GitHub "latest release" answer.
    pub release: Option<Result<String, String>>,
}

/// Judge every assumption these answers can speak to, in a fixed order.
pub fn judge(b: &Bodies) -> Vec<Finding> {
    let mut out = Vec::new();
    let quotes = parsed("quotes-request", &b.quotes, &mut out);
    quote_fields(b, quotes.as_ref(), &mut out);
    let unknown = parsed("unknown-request", &b.unknown, &mut out);
    unknown_symbol(b.unknown.is_some(), unknown.as_ref(), &mut out);
    // Every bar file that answered, with a name for it.
    let mut files: Vec<(String, Value)> = Vec::new();
    for (sym, r) in &b.day {
        if let Some(v) = parsed("day-request", &Some(r.clone()), &mut out) {
            files.push((format!("1D {}", clean(sym, 24)), v));
        }
    }
    for (file, r) in &b.ranges {
        if let Some(v) = parsed("range-request", &Some(r.clone()), &mut out) {
            files.push((clean(file, 24), v));
        }
    }
    time_spelling(quotes.as_ref(), &files, &mut out);
    bar_start(b, quotes.as_ref(), &files, &mut out);
    daily_boundary(&files, &mut out);
    volume(&files, &mut out);
    unread_fields(quotes.as_ref(), &files, &mut out);
    release_tag(b, &mut out);
    out
}

/// `1` when any finding is a `WARN`, else `0`.
pub fn exit_code(findings: &[Finding]) -> i32 {
    i32::from(findings.iter().any(|x| x.verdict == Verdict::Warn))
}

/// One line per finding: `PASS  quote-fields  3 symbols ...`.
pub fn render(findings: &[Finding]) -> String {
    findings.iter().map(|x| format!("{}  {:<15} {}", x.verdict.word(), x.id, x.text)).collect::<Vec<_>>().join("\n")
}

/// The body parsed as JSON. A missing request is nothing; a failed one or a body that is not JSON is
/// a `WARN` under `id`.
fn parsed(id: &'static str, r: &Option<Result<String, String>>, out: &mut Vec<Finding>) -> Option<Value> {
    match r {
        None => None,
        Some(Err(why)) => {
            out.push(f(id, Verdict::Warn, format!("no answer: {}", clean(why, 40))));
            None
        }
        Some(Ok(body)) => match serde_json::from_str::<Value>(body) {
            Ok(v) => Some(v),
            Err(_) => {
                out.push(f(id, Verdict::Warn, "the answer is not JSON"));
                None
            }
        },
    }
}

/// A name or reason from the network made safe to print: ASCII letters, digits and `_-.=@` and spaces only,
/// cut to `max` characters.
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || "_-.=@ ".contains(*c)).take(max).collect()
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('%').replace([',', '+'], "").parse().ok(),
        _ => None,
    }
}

fn quote_rows(v: &Value) -> Option<&Vec<Value>> {
    v.pointer("/FormattedQuoteResult/FormattedQuote").and_then(Value::as_array)
}

fn bar_rows(v: &Value) -> Option<&Vec<Value>> {
    v.pointer("/barData/priceBars").and_then(Value::as_array)
}

fn quote_fields(b: &Bodies, quotes: Option<&Value>, out: &mut Vec<Finding>) {
    let Some(v) = quotes else { return };
    let Some(rows) = quote_rows(v) else {
        out.push(f("quote-fields", Verdict::Warn, "no FormattedQuoteResult.FormattedQuote list in the answer"));
        return;
    };
    let mut problems: Vec<String> = Vec::new();
    for sym in &b.shown {
        match rows.iter().find(|r| r.get("symbol").and_then(Value::as_str) == Some(sym.as_str())) {
            None => problems.push(format!("{} has no quote", clean(sym, 24))),
            Some(r) => {
                if !num(r.get("last")).is_some_and(|p| p > 0.0) {
                    problems.push(format!("{} has no usable last", clean(sym, 24)));
                }
                if r.get("last_time").and_then(Value::as_str).is_none() {
                    problems.push(format!("{} has no last_time", clean(sym, 24)));
                }
            }
        }
    }
    if problems.is_empty() {
        out.push(f("quote-fields", Verdict::Pass, format!("{} shown symbols, each with symbol, last and last_time", b.shown.len())));
    } else {
        problems.truncate(8);
        out.push(f("quote-fields", Verdict::Warn, problems.join("; ")));
    }
}

fn unknown_symbol(asked: bool, v: Option<&Value>, out: &mut Vec<Finding>) {
    if !asked {
        return;
    }
    let Some(v) = v else { return };
    let rows = quote_rows(v).cloned().unwrap_or_default();
    let priced = rows.iter().any(|r| {
        r.get("symbol").and_then(Value::as_str).is_some_and(|s| s.eq_ignore_ascii_case(UNKNOWN_SYMBOL))
            && num(r.get("last")).is_some_and(|p| p > 0.0)
    });
    if priced {
        out.push(f("unknown-symbol", Verdict::Warn, "an unknown symbol was answered with a price; Add symbol would accept anything"));
    } else {
        out.push(f("unknown-symbol", Verdict::Pass, format!("an unknown symbol gets no quote ({} rows, none priced for it)", rows.len())));
    }
}

/// A bar's time as seconds: either spelling of the key, a number or a numeric string, in milliseconds.
fn bar_ms(b: &Value) -> Option<f64> {
    ["tradeTimeinMills", "tradeTimeInMills"].iter().find_map(|k| num(b.get(k))).map(|ms| ms / 1000.0).filter(|t| t.is_finite())
}

fn time_spelling(quotes: Option<&Value>, files: &[(String, Value)], out: &mut Vec<Finding>) {
    let mut bars = 0usize;
    let mut unspelt: Vec<String> = Vec::new();
    let mut spellings: Vec<&str> = Vec::new();
    for (name, v) in files {
        let Some(rows) = bar_rows(v) else { continue };
        let bad = rows.iter().filter(|r| bar_ms(r).is_none()).count();
        bars += rows.len();
        for k in ["tradeTimeinMills", "tradeTimeInMills"] {
            if rows.iter().any(|r| r.get(k).is_some()) && !spellings.contains(&k) {
                spellings.push(k);
            }
        }
        if bad > 0 {
            unspelt.push(format!("{name}: {bad} of {} bars have no tradeTimeinMills or tradeTimeInMills", rows.len()));
        }
    }
    // The quote times are read too: a full stamp parses, a bare date is a closed market's last close.
    let (mut full, mut bare) = (0usize, 0usize);
    if let Some(rows) = quotes.and_then(quote_rows) {
        for r in rows {
            if let Some(t) = r.get("last_time").and_then(Value::as_str) {
                if parse_time(t).is_some() { full += 1 } else { bare += 1 }
            }
        }
    }
    if bars == 0 && full + bare == 0 {
        out.push(f("time-spelling", Verdict::Note, "no bar file or quote time came back to read"));
    } else if !unspelt.is_empty() {
        unspelt.truncate(4);
        out.push(f("time-spelling", Verdict::Warn, unspelt.join("; ")));
    } else {
        let spelt = if spellings.is_empty() { "no bars".to_string() } else { spellings.join(" and ") };
        out.push(f("time-spelling", Verdict::Pass, format!("{bars} bars stamped with {spelt}; {full} quote times parse, {bare} are bare dates")));
    }
}

fn median_gap(times: &[f64]) -> f64 {
    let mut gaps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > 0.0).collect();
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_by(|a, b| a.total_cmp(b));
    gaps[gaps.len() / 2]
}

/// Is a bar stamped at its start? Read from the first 1D file against the quote of the same symbol:
/// the trade the quote names falls inside the last bar when the stamp is the bar's start, and just
/// before it when the stamp is its end. Only possible while the last bar still closes at the quote's
/// price, so a closed market gives a `NOTE`.
fn bar_start(b: &Bodies, quotes: Option<&Value>, files: &[(String, Value)], out: &mut Vec<Finding>) {
    let Some((sym, _)) = b.day.first() else { return };
    let Some((_, v)) = files.iter().find(|(n, _)| n.starts_with("1D ")) else { return };
    fn not_now(out: &mut Vec<Finding>, why: &str) {
        out.push(f("bar-start", Verdict::Note, format!("not testable in these answers: {why}")));
    }
    let mut bars: Vec<(f64, f64)> = bar_rows(v)
        .map(|rows| rows.iter().filter_map(|r| Some((bar_ms(r)?, num(r.get("close"))?))).collect())
        .unwrap_or_default();
    bars.sort_by(|a, c| a.0.total_cmp(&c.0));
    let times: Vec<f64> = bars.iter().map(|x| x.0).collect();
    let (Some(&(t, close)), s) = (bars.last(), median_gap(&times)) else { return not_now(out, "no bars") };
    let s = if s >= 30.0 { s } else { 60.0 };
    let quote = quotes.and_then(quote_rows).and_then(|rows| rows.iter().find(|r| r.get("symbol").and_then(Value::as_str) == Some(sym.as_str())));
    let (Some(price), Some(tq)) = (
        quote.and_then(|r| num(r.get("last"))),
        quote.and_then(|r| r.get("last_time")).and_then(Value::as_str).and_then(parse_time),
    ) else {
        return not_now(out, "the quote has no price or time");
    };
    if t > b.now + 120.0 {
        out.push(f("bar-start", Verdict::Warn, "the last bar is stamped in the future: its stamp is its end, or the clock is wrong"));
    } else if (close - price).abs() > price.abs() * 1e-9 + 1e-9 || tq - t > s + 5.0 {
        not_now(out, "no bar closes at the quote's price (the market is probably closed)");
    } else if tq + 1.0 < t {
        out.push(f("bar-start", Verdict::Warn, format!("the quote's trade is {:.0} s before the last bar's stamp: stamps look like bar ends", t - tq)));
    } else {
        out.push(f("bar-start", Verdict::Pass, format!("the quote's trade falls {:.0} s into the last bar: stamps are bar starts", tq - t)));
    }
}

fn daily_boundary(files: &[(String, Value)], out: &mut Vec<Finding>) {
    let mut seen = 0;
    for (name, v) in files {
        if name.starts_with("1D ") {
            continue;
        }
        let Some(rows) = bar_rows(v) else { continue };
        let mut times: Vec<f64> = rows.iter().filter_map(bar_ms).collect();
        times.sort_by(|a, c| a.total_cmp(c));
        if times.len() < 2 || median_gap(&times) < 86_400.0 * 0.95 {
            continue;
        }
        seen += 1;
        let tod = times.last().map_or(0.0, |t| t.rem_euclid(86_400.0));
        if crate::ranges::stamps_on_day_boundary(&times) {
            out.push(f("daily-boundary", Verdict::Pass, format!("{name}: {} daily stamps sit on a day boundary (about {:02}:{:02} UTC)", times.len(), (tod / 3600.0) as u32, (tod % 3600.0 / 60.0) as u32)));
        } else {
            out.push(f("daily-boundary", Verdict::Warn, format!("{name}: daily stamps are not on a day boundary (last at {:02}:{:02} UTC); candles may be dated a day off", (tod / 3600.0) as u32, (tod % 3600.0 / 60.0) as u32)));
        }
    }
    if seen == 0 {
        out.push(f("daily-boundary", Verdict::Note, "no daily file came back to read"));
    }
}

fn volume(files: &[(String, Value)], out: &mut Vec<Finding>) {
    let (mut bars, mut with) = (0usize, 0usize);
    for (_, v) in files {
        if let Some(rows) = bar_rows(v) {
            bars += rows.len();
            with += rows.iter().filter(|r| num(r.get("volume")).is_some_and(|x| x.is_finite() && x > 0.0)).count();
        }
    }
    if bars == 0 {
        out.push(f("volume", Verdict::Note, "no bars came back to read"));
    } else if with > 0 {
        out.push(f("volume", Verdict::Pass, format!("{with} of {bars} bars carry a volume")));
    } else {
        out.push(f("volume", Verdict::Warn, format!("none of {bars} bars carries a volume; the volume strip will not draw")));
    }
}

fn unread_fields(quotes: Option<&Value>, files: &[(String, Value)], out: &mut Vec<Finding>) {
    let mut q: Vec<String> = Vec::new();
    if let Some(rows) = quotes.and_then(quote_rows) {
        for r in rows {
            for k in r.as_object().into_iter().flat_map(|o| o.keys()) {
                if !QUOTE_READ.contains(&k.as_str()) && !q.contains(&clean(k, 32)) {
                    q.push(clean(k, 32));
                }
            }
        }
    }
    let mut bar: Vec<String> = Vec::new();
    for (_, v) in files {
        for r in bar_rows(v).into_iter().flatten().take(50) {
            for k in r.as_object().into_iter().flat_map(|o| o.keys()) {
                if !BAR_READ.contains(&k.as_str()) && !bar.contains(&clean(k, 32)) {
                    bar.push(clean(k, 32));
                }
            }
        }
    }
    q.sort();
    bar.sort();
    q.truncate(30);
    bar.truncate(30);
    if q.is_empty() && bar.is_empty() {
        out.push(f("unread-fields", Verdict::Note, "every field in the answers is one the parser reads"));
    } else {
        out.push(f("unread-fields", Verdict::Note, format!("quote fields not read: {}; bar fields not read: {}", join_or_none(&q), join_or_none(&bar))));
    }
}

fn join_or_none(v: &[String]) -> String {
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

fn release_tag(b: &Bodies, out: &mut Vec<Finding>) {
    match &b.release {
        None => {}
        Some(Err(why)) => out.push(f("release-tag", Verdict::Warn, format!("no answer: {}", clean(why, 40)))),
        Some(Ok(body)) => match crate::update::decide(&b.current, body) {
            crate::update::Outcome::Failed(_) => out.push(f("release-tag", Verdict::Warn, "the answer has no tag_name the app can read as a version")),
            crate::update::Outcome::UpToDate => out.push(f("release-tag", Verdict::Pass, "the latest release tag reads as a version; this build is up to date")),
            crate::update::Outcome::Newer(v) => out.push(f("release-tag", Verdict::Pass, format!("the latest release tag reads as a version ({v}), newer than this build"))),
        },
    }
}

/// "2026-10-15T04:29:41.313-0400" -> UTC epoch seconds. A bare date (a closed cash market's
/// last close) has no time of day and returns None, so the row reads as stale.
pub fn parse_time(s: &str) -> Option<f64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let split = rest.rfind(['+', '-']).filter(|&i| i >= 8)?;
    let (clock, zone) = rest.split_at(split);
    let mut c = clock.split(':');
    let h: f64 = c.next()?.parse().ok()?;
    let mi: f64 = c.next()?.parse().ok()?;
    let se: f64 = c.next().unwrap_or("0").parse().ok()?;
    let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
    let z = zone[1..].replace(':', "");
    let zh: f64 = z.get(0..2)?.parse().ok()?;
    let zm: f64 = z.get(2..4).unwrap_or("0").parse().ok()?;
    let local = crate::chartview::days_from_civil(y, mo as u32, da as u32) as f64 * 86_400.0 + h * 3600.0 + mi * 60.0 + se;
    Some(local - sign * (zh * 3600.0 + zm * 60.0))
}

// ---------------------------------------------------------------------------------------------
// --dump
// ---------------------------------------------------------------------------------------------

/// The folder named by `--dump <dir>` in `args` (the whole command line, program name first).
/// `Ok(None)`: no `--dump`. `Err` with a sentence when `--dump` has no folder after it, when what
/// follows is another flag, or when it is given without `--probe` (the running app never dumps).
pub fn dump_dir(args: &[String]) -> Result<Option<PathBuf>, &'static str> {
    let Some(i) = args.iter().position(|a| a == "--dump") else { return Ok(None) };
    if !args.iter().any(|a| a == "--probe") {
        return Err("--dump works only together with --probe");
    }
    match args.get(i + 1) {
        Some(dir) if !dir.is_empty() && !dir.starts_with("--") => Ok(Some(PathBuf::from(dir))),
        _ => Err("--dump needs a folder: --probe --dump <folder>"),
    }
}

/// A file name from a part of a symbol or file name: ASCII letters, digits, `-`, `_` and `.`; every
/// other character becomes `_`.
fn file_part(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).take(40).collect()
}

/// The raw answers that came back, as `(file name, body)` for a folder of fixtures:
/// `quotes.json`, `quote-unknown.json`, `day-01-<symbol>.json`, `range-<file>` and `release.json`.
/// An answer that failed has no file.
pub fn dump_files(b: &Bodies) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |name: String, r: &Option<Result<String, String>>| {
        if let Some(Ok(body)) = r {
            out.push((name, body.clone()));
        }
    };
    add("quotes.json".into(), &b.quotes);
    add("quote-unknown.json".into(), &b.unknown);
    for (i, (sym, r)) in b.day.iter().enumerate() {
        add(format!("day-{:02}-{}.json", i + 1, file_part(sym)), &Some(r.clone()));
    }
    for (file, r) in &b.ranges {
        add(format!("range-{}", file_part(file)), &Some(r.clone()));
    }
    add("release.json".into(), &b.release);
    out
}

/// Write [`dump_files`] into `dir` (created if missing) and return how many files were written.
/// Nothing but those files: not the settings, not the webhook.
pub fn write_dump(dir: &Path, b: &Bodies) -> std::io::Result<usize> {
    std::fs::create_dir_all(dir)?;
    let files = dump_files(b);
    for (name, body) in &files {
        std::fs::write(dir.join(name), body)?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_792_000_000.0;

    /// A quote answer for the symbols, each priced `last`, stamped `time`.
    fn quotes_body(symbols: &[&str], last: f64, time: &str) -> String {
        let rows: Vec<String> = symbols
            .iter()
            .map(|s| format!(r#"{{"symbol":"{s}","last":"{last}","last_time":"{time}","change_pct":"+0.5%","name":"{s}","extraField":"x"}}"#))
            .collect();
        format!(r#"{{"FormattedQuoteResult":{{"FormattedQuote":[{}]}}}}"#, rows.join(","))
    }

    fn bars_body(key: &str, stamps_ms: &[f64], close: f64, volume: f64) -> String {
        let rows: Vec<String> = stamps_ms
            .iter()
            .map(|t| format!(r#"{{"{key}":"{t:.0}","open":{close},"high":{close},"low":{close},"close":{close},"volume":{volume}}}"#))
            .collect();
        format!(r#"{{"barData":{{"priceBars":[{}]}}}}"#, rows.join(","))
    }

    /// `n` one-minute bar stamps (ms) ending at `last_s`.
    fn minutes(last_s: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| (last_s - (n - 1 - i) as f64 * 60.0) * 1000.0).collect()
    }

    /// `n` daily stamps (ms) at `tod` seconds after UTC midnight, ending 2026-10-14.
    fn days(tod: f64, n: usize) -> Vec<f64> {
        let end = crate::chartview::days_from_civil(2026, 10, 14) as f64 * 86_400.0 + tod;
        (0..n).map(|i| (end - (n - 1 - i) as f64 * 86_400.0) * 1000.0).collect()
    }

    const RELEASE: &str = r#"{"tag_name":"v1.8.0","name":"1.8"}"#;

    /// Answers in which every assumption holds. The quote is stamped 30 s into the last bar.
    fn good() -> Bodies {
        let last_bar = NOW - 100.0;
        let tq = last_bar + 30.0;
        // The quote's time as CNBC spells it: local time with an offset.
        let secs = tq as i64;
        let (h, m, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
        let (y, mo, d) = crate::chartview::civil_from_days(secs / 86_400);
        let time = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.000+0000");
        Bodies {
            now: NOW,
            current: "1.8.0".into(),
            shown: vec!["XAU=".into(), "@SP.1".into()],
            quotes: Some(Ok(quotes_body(&["XAU=", "@SP.1"], 4150.0, &time))),
            unknown: Some(Ok(r#"{"FormattedQuoteResult":{"FormattedQuote":[]}}"#.into())),
            day: vec![("XAU=".into(), Ok(bars_body("tradeTimeinMills", &minutes(last_bar, 30), 4150.0, 12.0)))],
            ranges: vec![
                ("5D.json".into(), Ok(bars_body("tradeTimeinMills", &minutes(last_bar, 20), 4150.0, 5.0))),
                ("1M.json".into(), Ok(bars_body("tradeTimeInMills", &days(5.0 * 3600.0, 40), 4150.0, 100.0))),
                ("5Y.json".into(), Ok(bars_body("tradeTimeinMills", &days(5.0 * 3600.0, 60), 4150.0, 100.0))),
            ],
            release: Some(Ok(RELEASE.into())),
        }
    }

    fn verdict(findings: &[Finding], id: &str) -> Vec<Verdict> {
        findings.iter().filter(|x| x.id == id).map(|x| x.verdict).collect()
    }

    #[test]
    fn good_answers_pass_every_assumption_and_exit_zero() {
        let found = judge(&good());
        for id in ["quote-fields", "unknown-symbol", "time-spelling", "bar-start", "volume", "release-tag"] {
            assert_eq!(verdict(&found, id), vec![Verdict::Pass], "{id}: {}", render(&found));
        }
        assert_eq!(verdict(&found, "daily-boundary"), vec![Verdict::Pass, Verdict::Pass]);
        assert_eq!(exit_code(&found), 0, "{}", render(&found));
        // The unread field is named, as a note, not a warning.
        let unread = found.iter().find(|x| x.id == "unread-fields").unwrap();
        assert_eq!(unread.verdict, Verdict::Note);
        assert!(unread.text.contains("extraField"), "{}", unread.text);
    }

    #[test]
    fn quote_fields_warn_on_a_missing_symbol_price_or_time() {
        let mut b = good();
        b.quotes = Some(Ok(r#"{"FormattedQuoteResult":{"FormattedQuote":[{"symbol":"XAU=","last":"0"}]}}"#.into()));
        let found = judge(&b);
        let w = found.iter().find(|x| x.id == "quote-fields").unwrap();
        assert_eq!(w.verdict, Verdict::Warn);
        assert!(w.text.contains("XAU= has no usable last") && w.text.contains("XAU= has no last_time") && w.text.contains("@SP.1 has no quote"), "{}", w.text);
        assert_eq!(exit_code(&found), 1);
        b.quotes = Some(Ok(r#"{"other":1}"#.into()));
        assert_eq!(verdict(&judge(&b), "quote-fields"), vec![Verdict::Warn]);
        // A request that failed is a warning under its own name, and quotes are then not judged.
        b.quotes = Some(Err("HTTP 429".into()));
        let found = judge(&b);
        assert_eq!(verdict(&found, "quotes-request"), vec![Verdict::Warn]);
        assert!(verdict(&found, "quote-fields").is_empty());
    }

    #[test]
    fn an_unknown_symbol_must_get_no_quote() {
        let mut b = good();
        assert_eq!(verdict(&judge(&b), "unknown-symbol"), vec![Verdict::Pass]);
        // Another symbol's row in the answer is still no quote for this one.
        b.unknown = Some(Ok(quotes_body(&["XAU="], 1.0, "2026-10-15T04:29:41.000+0000")));
        assert_eq!(verdict(&judge(&b), "unknown-symbol"), vec![Verdict::Pass]);
        b.unknown = Some(Ok(quotes_body(&[UNKNOWN_SYMBOL], 12.5, "2026-10-15T04:29:41.000+0000")));
        let found = judge(&b);
        assert_eq!(verdict(&found, "unknown-symbol"), vec![Verdict::Warn]);
        assert_eq!(exit_code(&found), 1);
    }

    #[test]
    fn either_spelling_of_the_bar_time_passes_and_a_third_warns() {
        let mut b = good();
        b.ranges = vec![("5D.json".into(), Ok(bars_body("tradeTimeInMills", &minutes(NOW - 100.0, 10), 1.0, 1.0)))];
        assert_eq!(verdict(&judge(&b), "time-spelling"), vec![Verdict::Pass]);
        b.ranges = vec![("5D.json".into(), Ok(bars_body("tradeTime", &minutes(NOW - 100.0, 10), 1.0, 1.0)))];
        let found = judge(&b);
        let w = found.iter().find(|x| x.id == "time-spelling").unwrap();
        assert_eq!(w.verdict, Verdict::Warn);
        assert!(w.text.contains("10 of 10 bars"), "{}", w.text);
        // The unknown key is reported among the unread fields.
        assert!(found.iter().any(|x| x.id == "unread-fields" && x.text.contains("tradeTime")));
    }

    #[test]
    fn a_bar_stamped_at_its_end_or_in_the_future_warns_and_a_closed_market_is_a_note() {
        // Stamped at its end: the trade the quote names is before the last stamp.
        let mut b = good();
        let end_stamped = minutes(NOW - 100.0 + 60.0, 30);
        b.day = vec![("XAU=".into(), Ok(bars_body("tradeTimeinMills", &end_stamped, 4150.0, 12.0)))];
        assert_eq!(verdict(&judge(&b), "bar-start"), vec![Verdict::Warn]);
        // Stamped in the future.
        let mut b = good();
        b.day = vec![("XAU=".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW + 3_600.0, 30), 4150.0, 12.0)))];
        assert_eq!(verdict(&judge(&b), "bar-start"), vec![Verdict::Warn]);
        // Market closed: the last bar closed long before the quote's trade.
        let mut b = good();
        b.day = vec![("XAU=".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW - 86_400.0, 30), 4140.0, 12.0)))];
        let found = judge(&b);
        assert_eq!(verdict(&found, "bar-start"), vec![Verdict::Note]);
        assert_eq!(exit_code(&found), 0, "a note is not a warning");
    }

    #[test]
    fn daily_stamps_off_a_day_boundary_warn() {
        let mut b = good();
        b.ranges = vec![("1M.json".into(), Ok(bars_body("tradeTimeinMills", &days(13.0 * 3600.0 + 420.0, 40), 1.0, 1.0)))];
        let found = judge(&b);
        let w = found.iter().find(|x| x.id == "daily-boundary").unwrap();
        assert_eq!(w.verdict, Verdict::Warn);
        assert!(w.text.contains("13:07 UTC") && w.text.contains("a day off"), "{}", w.text);
        assert_eq!(exit_code(&found), 1);
        // No daily file at all: a note.
        b.ranges = vec![("5D.json".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW - 100.0, 10), 1.0, 1.0)))];
        assert_eq!(verdict(&judge(&b), "daily-boundary"), vec![Verdict::Note]);
    }

    #[test]
    fn no_volume_anywhere_warns() {
        let mut b = good();
        b.day = vec![("XAU=".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW - 70.0, 30), 4150.0, 0.0)))];
        b.ranges = vec![("5D.json".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW - 100.0, 20), 4150.0, 0.0)))];
        let found = judge(&b);
        assert_eq!(verdict(&found, "volume"), vec![Verdict::Warn]);
        assert!(found.iter().find(|x| x.id == "volume").unwrap().text.contains("will not draw"));
        // One bar with a volume is enough to pass.
        b.ranges = vec![("5D.json".into(), Ok(bars_body("tradeTimeinMills", &minutes(NOW - 100.0, 20), 4150.0, 3.0)))];
        assert_eq!(verdict(&judge(&b), "volume"), vec![Verdict::Pass]);
    }

    #[test]
    fn the_release_tag_must_read_as_a_version() {
        let mut b = good();
        b.release = Some(Ok(r#"{"tag_name":"v1.9.0"}"#.into()));
        let found = judge(&b);
        assert_eq!(verdict(&found, "release-tag"), vec![Verdict::Pass]);
        assert!(found.iter().find(|x| x.id == "release-tag").unwrap().text.contains("1.9.0"));
        for bad in [r#"{"tag_name":"nightly"}"#, r#"{"name":"x"}"#, "not json"] {
            b.release = Some(Ok(bad.into()));
            let found = judge(&b);
            assert!(verdict(&found, "release-tag").contains(&Verdict::Warn) || verdict(&found, "release-tag-request").contains(&Verdict::Warn), "{bad}");
            assert_eq!(exit_code(&found), 1, "{bad}");
        }
        b.release = Some(Err("HTTP 403".into()));
        assert_eq!(verdict(&judge(&b), "release-tag"), vec![Verdict::Warn]);
    }

    #[test]
    fn nothing_from_the_network_is_printed_beyond_names_and_numbers() {
        let mut b = good();
        b.quotes = Some(Err("https://secret.example/hook?token=abc <b>".into()));
        b.shown = vec!["EVIL<script>=".into()];
        let text = render(&judge(&b));
        for bad in ['/', '?', '<', '>', '"'] {
            assert!(!text.contains(bad), "{bad}: {text}");
        }
    }

    #[test]
    fn the_dump_refuses_a_missing_folder_and_writes_only_the_answers() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dump_dir(&args(&["x", "--probe"])), Ok(None));
        assert!(dump_dir(&args(&["x", "--probe", "--dump"])).is_err());
        assert!(dump_dir(&args(&["x", "--probe", "--dump", "--other"])).is_err());
        assert!(dump_dir(&args(&["x", "--probe", "--dump", ""])).is_err());
        assert!(dump_dir(&args(&["x", "--dump", "out"])).is_err(), "the running app never dumps");
        assert_eq!(dump_dir(&args(&["x", "--probe", "--dump", "out"])), Ok(Some(PathBuf::from("out"))));

        let root = std::env::var_os("TICKER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("mh-ticker-dump-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut b = good();
        b.day.push(("@SP.1".into(), Err("network".into())));
        let n = write_dump(&dir, &b).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(n, names.len());
        assert_eq!(
            names,
            ["day-01-XAU_.json", "quote-unknown.json", "quotes.json", "range-1M.json", "range-5D.json", "range-5Y.json", "release.json"]
        );
        assert!(names.iter().all(|n| !n.contains("settings") && !n.contains("webhook") && !n.ends_with("config.json")));
        assert_eq!(std::fs::read_to_string(dir.join("quotes.json")).unwrap(), b.quotes.clone().unwrap().unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parse_time_reads_cnbc_stamps() {
        let utc = parse_time("2026-10-15T04:29:41.313-0400").unwrap();
        assert_eq!(parse_time("2026-10-15T08:29:41.313+0000"), Some(utc));
        assert_eq!(parse_time("2026-10-14"), None);
    }
}
