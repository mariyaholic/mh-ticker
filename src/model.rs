//! Quotes, staleness, the board the UI draws, and number formatting. Pure code: no I/O, unit-tested.

use crate::config::Row;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quote {
    pub symbol: String,
    /// Empty for CNBC; fallback provider name otherwise.
    pub source: &'static str,
    /// When a fallback response was received, not the market observation time.
    pub received_at: Option<f64>,
    /// CNBC's name for it, e.g. "Dow Jones Fut (Dec'26)".
    pub name: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub prev_close: Option<f64>,
    /// UTC epoch seconds of the price; None = unknown (treated as stale).
    pub ts: Option<f64>,
    /// UTC seconds when the feed last saw this CNBC price change (set by the feed, not by CNBC;
    /// its first sight of a price counts as a move). None for a quote the feed has not stored. See [`restamped`].
    pub moved_at: Option<f64>,
    /// False when CNBC marks the quote as delayed (its index futures, ~10 min).
    pub realtime: bool,
    // --- Fields the 3-second response already carries. Every one of them is `None`/empty when
    // CNBC omits it, and a few are sent as `0.00` for symbols that have no such value (see
    // `positive`), so nothing below may be assumed present.
    /// The session's open, when CNBC sends one.
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    /// Day volume; absent for FX, yields and spot metals.
    pub volume: Option<f64>,
    pub yr_hi: Option<f64>,
    pub yr_lo: Option<f64>,
    /// The day the 52-week high was set, as CNBC sends it (a bare date).
    pub yr_hi_date: Option<String>,
    /// ISO code, when CNBC names one (`USD`, `EUR`, …).
    pub currency: String,
}

/// Overall state, shown by the dot on the strip and the tray icon colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// Every row fresh.
    Live,
    /// Answering, but some row is stale or missing (a market closed, a symbol gone).
    Partial,
    /// The last request failed.
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub label: String,
    pub digits: u32,
    pub quote: Option<Quote>,
    pub stale: bool,
}

/// One bar, as CNBC sends it. The time is an offset from the row's `bars_t0` and the prices and the
/// volume are `f32`: 24 bytes a bar, and a 400 px chart cannot tell the difference.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bar {
    pub dt: f32,
    pub o: f32,
    pub h: f32,
    pub l: f32,
    pub c: f32,
    /// Volume of the bar; 0 when the feed sends none (FX, yields, spot metals).
    pub v: f32,
}

impl Bar {
    pub fn t(&self, t0: f64) -> f64 {
        t0 + self.dt as f64
    }
}

#[derive(Clone, Debug)]
pub struct Board {
    pub fallback_enabled: bool,
    pub fallback_gen: u64,
    /// The symbols shown. The UI changes them (Symbols menu) and bumps `rows_gen`; the feed
    /// thread picks the new list up on its next publish.
    pub rows: Vec<Row>,
    pub rows_gen: u64,
    pub cells: Vec<Cell>,
    pub link: Link,
    /// One-line reason shown in the tray tooltip / menu when something is off.
    pub note: String,
    /// Past-hour (utc_s, close) points per row from 1-minute bars, for the panel's sparklines;
    /// `backfill_gen` bumps each time a fresh set arrives.
    pub backfill: Vec<Vec<(f64, f64)>>,
    pub backfill_gen: u64,
    /// The whole day's 1-minute bars per row, for the candle chart. `bars_gen` bumps with
    /// `backfill_gen`.
    pub bars: Vec<std::sync::Arc<[Bar]>>,
    /// Each row's first bar's utc seconds; the bars themselves carry offsets from it.
    pub bars_t0: Vec<f64>,
    pub bars_gen: u64,
    /// Seconds between quote requests. The UI owns it (`Poll every` in the menu) and the feed
    /// thread reads it each pass, so a change takes effect on the next tick without a restart.
    pub poll_s: f64,
    /// *Refresh now* counter. The UI bumps it with `wrapping_add(1)`; `Feed::wait` captures it on
    /// entry and returns when it differs, and the chart thread makes rows with no bars due at once.
    pub kick: u64,
    /// The poll governor's inputs, written by the UI (`governor` has the rules). Both ticks off is
    /// the Classic default: the feed then waits exactly `poll_s`.
    pub gov: crate::governor::Settings,
    /// The session is locked (written by the UI on the session notification).
    pub locked: bool,
    /// Some alert has a level or a fast-move set (written by the UI).
    pub armed: bool,
    /// The interval the feed is waiting out now, written by the feed; 0 while the governor is off.
    /// The stale rule follows it (`governor::stale_after`).
    pub governed_s: f64,
    /// The feed is waiting longer than `poll_s` because of the governor, so a wake is worth a kick.
    pub slowed: bool,
}

fn empty_cell(r: &Row) -> Cell {
    Cell { label: r.label.clone(), digits: r.digits, quote: None, stale: true }
}

impl Board {
    pub fn new(rows: &[Row]) -> Self {
        Self {
            fallback_enabled: false,
            fallback_gen: 0,
            rows: rows.to_vec(),
            rows_gen: 0,
            cells: rows.iter().map(empty_cell).collect(),
            link: Link::Down,
            note: "starting".into(),
            backfill: Vec::new(),
            backfill_gen: 0,
            bars: Vec::new(),
            bars_t0: Vec::new(),
            bars_gen: 0,
            poll_s: 3.0,
            kick: 0,
            gov: crate::governor::Settings::default(),
            locked: false,
            armed: false,
            governed_s: 0.0,
            slowed: false,
        }
    }

    pub fn set_fallback(&mut self, enabled: bool) {
        if self.fallback_enabled == enabled { return; }
        self.fallback_enabled = enabled;
        self.fallback_gen = self.fallback_gen.wrapping_add(1);
        if !enabled {
            for c in &mut self.cells {
                if c.quote.as_ref().is_some_and(|q| !q.source.is_empty()) {
                    c.quote = None;
                    c.stale = true;
                }
            }
        }
    }

    /// Expire fallback independently of either provider or CNBC request completion.
    pub fn expire_fallback(&mut self, now: f64) -> bool {
        let mut changed = false;
        for cell in &mut self.cells {
            let expired = cell.quote.as_ref().is_some_and(|q| !q.source.is_empty()
                && q.received_at.is_none_or(|t| !(0.0..90.0).contains(&(now - t))));
            if expired { cell.quote = None; cell.stale = true; changed = true; }
        }
        changed
    }

    pub fn publish_fallback(&mut self, generation: u64, rows_gen: u64, row: &Row, quote: Quote) -> bool {
        if !self.fallback_enabled || self.fallback_gen != generation || self.rows_gen != rows_gen
            || quote.symbol != row.symbol || quote.source.is_empty() { return false; }
        let Some(i) = self.rows.iter().position(|r| r.label == row.label && r.symbol == row.symbol) else { return false; };
        if !self.cells[i].stale && self.link != Link::Down { return false; }
        self.cells[i].quote = Some(quote);
        // No trustworthy market observation timestamp; never permit alert use.
        self.cells[i].stale = true;
        true
    }

    /// Swap in a new symbol list, keeping data only for the same symbol (a row is its symbol: a rename
    /// or a new decimal count keeps its quote, bars and line, and takes the new label and digits).
    /// Candle and backfill vectors are indexed like `rows`; remap them atomically with the cells
    /// so an in-flight fetch cannot briefly put one instrument's history under another label.
    pub fn set_rows(&mut self, rows: &[Row]) {
        let indices: Vec<_> = rows.iter().map(|r| {
            self.rows.iter().position(|old| old.symbol == r.symbol)
        }).collect();
        self.cells = rows.iter().zip(&indices).map(|(r, i)| {
            i.and_then(|i| self.cells.get(i))
                .map(|c| Cell { label: r.label.clone(), digits: r.digits, ..c.clone() })
                .unwrap_or_else(|| empty_cell(r))
        }).collect();
        self.bars = indices.iter().map(|i| i.and_then(|i| self.bars.get(i)).cloned().unwrap_or_default()).collect();
        self.bars_t0 = indices.iter().map(|i| i.and_then(|i| self.bars_t0.get(i)).copied().unwrap_or_default()).collect();
        self.backfill = indices.iter().map(|i| i.and_then(|i| self.backfill.get(i)).cloned().unwrap_or_default()).collect();
        self.rows = rows.to_vec();
        self.rows_gen += 1;
    }
}

/// A quote time this far ahead of the clock counts as clock skew. Further ahead, the time is wrong
/// and the quote has no known age (`is_stale` is true and `market_closed` is false for it).
pub const FUTURE_SKEW_S: f64 = 300.0;

pub fn is_stale(q: &Quote, now: f64, stale_after: f64, delayed_grace: f64) -> bool {
    let limit = stale_after + if q.realtime { 0.0 } else { delayed_grace };
    q.ts.map(|t| now - t > limit || now - t < -FUTURE_SKEW_S).unwrap_or(true) || restamped(q, now)
}

/// How old a CNBC quote must be, with the feed answering, before the card calls its market closed.
/// CNBC's own `curmktstatus` field reads `REG_MKT` on a Saturday for gold, silver, FX, futures and
/// bitcoin alike, so the state comes from the age of the quote. 15 minutes is past the 10-minute
/// delay of the index futures. A delayed quote is stale only after the stale limit plus the delayed
/// grace (990 s with the defaults), so it reads closed from that age on, not from 15 minutes.
pub const CLOSED_AFTER_S: f64 = 900.0;

/// Has CNBC re-stamped a price that did not move? True for a CNBC quote whose price has been the
/// same for [`CLOSED_AFTER_S`] on the feed's own observations (`Quote::moved_at`), whatever time the
/// quote carries. Futures print thin re-stamps after the session ends; a fresh-looking time on a
/// frozen price must not count as a fresh price.
pub fn restamped(q: &Quote, now: f64) -> bool {
    q.source.is_empty() && q.moved_at.is_some_and(|m| now - m >= CLOSED_AFTER_S)
}

/// When this CNBC quote's market stopped, as UTC seconds: the quote's time when it is at least
/// [`CLOSED_AFTER_S`] old, else the moment the price last moved when the quote is [`restamped`].
/// `None` for a quote with no time, a time ahead of the clock, or a price that is still moving.
pub fn closed_since(q: &Quote, now: f64) -> Option<f64> {
    if !q.source.is_empty() {
        return None;
    }
    q.ts.filter(|t| now - t >= CLOSED_AFTER_S).or_else(|| q.moved_at.filter(|_| restamped(q, now)))
}

/// Is this card's market closed, as opposed to its feed dead? True for a stale CNBC quote whose
/// market has stopped ([`closed_since`]) while the last request was answered (`link` is not `Down`):
/// the service is up and this instrument has stopped printing. False with the link down (the age
/// says nothing then), with no quote, with a fallback-provider quote (its age is unknown), with a
/// quote time ahead of the clock (unknown age, never age 0), and while the quote is fresh.
///
/// The test cannot tell a closed market from one feed that stopped for a single symbol while the
/// others keep updating: that symbol also reads closed after 15 minutes. A request that fails, or a
/// service that stops answering, is the only fault this rule separates from a closed market.
pub fn market_closed(cell: &Cell, link: Link, now: f64) -> bool {
    link != Link::Down && cell.stale && cell.quote.as_ref().is_some_and(|q| closed_since(q, now).is_some())
}
/// How much of the board is closed, when closed is the only reason a price is not fresh. Every
/// surface that names the link (the panel header, the strip's dot, the tray icon and tooltip) asks
/// [`Board::calm`], so a closed market looks the same everywhere and a fault never does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calm {
    /// Nothing is closed, or some stale cell is a fault (no quote, a fallback quote, a quote of
    /// unknown age, the link down): the link's own look applies.
    No,
    /// Every stale cell is a closed market and the other `n` rows are live.
    Some(usize),
    /// Every row is a closed market (`n` of them).
    All(usize),
}

impl Board {
    /// See [`Calm`]. `now` is UTC seconds.
    pub fn calm(&self, now: f64) -> Calm {
        if self.link == Link::Down {
            return Calm::No;
        }
        let stale = self.cells.iter().filter(|c| c.stale).count();
        let closed = self.cells.iter().filter(|c| market_closed(c, self.link, now)).count();
        match (stale, closed) {
            (0, _) | (_, 0) => Calm::No,
            (s, c) if s != c => Calm::No,
            (_, c) if c == self.cells.len() => Calm::All(c),
            (_, c) => Calm::Some(c),
        }
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The local day and clock of the UTC instant `at`, as `Fri 17:40`; a time six days or more before
/// `now` reads `Nov 6 17:40`, because a day name would then name a day that is not the last one.
/// `offset_at(t)` is seconds east of UTC at `t`, so a time on the other side of a daylight-saving
/// change is shown on its own clock.
pub fn fmt_since(offset_at: &dyn Fn(f64) -> i32, now: f64, at: f64) -> String {
    let t = if at.is_finite() { at.clamp(-1e12, 1e12) } else { now };
    let local = t + offset_at(t) as f64;
    let days = (local / 86_400.0).floor() as i64;
    let minute = ((local / 60.0).floor() as i64).rem_euclid(1440);
    let clock = format!("{:02}:{:02}", minute / 60, minute % 60);
    if now - t >= 6.0 * 86_400.0 {
        let (_, m, d) = crate::chartview::civil_from_days(days);
        return format!("{} {d} {clock}", MONTHS[(m as usize - 1).min(11)]);
    }
    format!("{} {clock}", WEEKDAYS[(days + 4).rem_euclid(7) as usize])
}

/// The first of `options` (longest first) that `fits`, else the last one.
pub fn first_fitting(options: &[String], fits: &dyn Fn(&str) -> bool) -> String {
    options.iter().find(|o| fits(o)).or(options.last()).cloned().unwrap_or_default()
}

/// The words a closed card carries, longest first: `closed since Fri 17:40` with the source before
/// it (`CNBC delayed · closed since Fri 17:40`) when `src` is given, then the words alone, then
/// `closed` alone. `since` is [`closed_since`] and may be unknown, which leaves `closed`.
pub fn closed_words(src: &str, since: Option<f64>, offset_at: &dyn Fn(f64) -> i32, now: f64) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(t) = since {
        let words = format!("closed since {}", fmt_since(offset_at, now, t));
        if !src.is_empty() {
            out.push(format!("{src} \u{b7} {words}"));
        }
        out.push(words);
    }
    out.push("closed".into());
    out
}

/// One cell per configured row, from the latest quotes keyed by symbol.
pub fn resolve(rows: &[Row], quotes: &HashMap<String, Quote>, now: f64, stale_after: f64, delayed_grace: f64) -> Vec<Cell> {
    rows.iter()
        .map(|r| {
            let quote = quotes.get(&r.symbol).cloned();
            let stale = quote.as_ref().map(|q| is_stale(q, now, stale_after, delayed_grace)).unwrap_or(true);
            Cell { label: r.label.clone(), digits: r.digits, quote, stale }
        })
        .collect()
}

/// `81176.4, 0` -> "81,176"; `4154.456, 2` -> "4,154.46".
pub fn fmt_price(v: f64, digits: u32) -> String {
    if !v.is_finite() {
        return "--".to_string();
    }
    let s = format!("{:.*}", digits as usize, v.abs());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let sign = if v < 0.0 { "-" } else { "" };
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

pub fn fmt_change(pct: f64) -> String {
    if !pct.is_finite() {
        return "--".to_string();
    }
    format!("{}{:.2}%", if pct >= 0.0 { "+" } else { "" }, pct)
}

/// The whole board as text for the clipboard: one line per cell, `label<TAB>price<TAB>change`,
/// joined with `\n`, no trailing newline. A stale cell (`Cell::stale`) with a quote gets a fourth
/// field, `stale`, so a pasted list cannot pass an old price off as current. Price is `fmt_price(price, digits)`, change is
/// `fmt_change(change_pct)`; a cell with no quote is `label<TAB>--<TAB>--`, and a quote with no
/// `change_pct` has `--` for the change only. No cells = empty string.
pub fn board_text(cells: &[Cell]) -> String {
    cells
        .iter()
        .map(|cell| match &cell.quote {
            None => format!("{}\t--\t--", cell.label),
            Some(q) => {
                let change = q.change_pct.map_or_else(|| "--".to_string(), fmt_change);
                let line = format!("{}\t{}\t{}", cell.label, fmt_price(q.price, cell.digits), change);
                if cell.stale { format!("{line}\tstale") } else { line }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_finite_number_prints_as_dashes() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(fmt_price(v, 2), "--", "{v}");
            assert_eq!(fmt_change(v), "--", "{v}");
            let q = Quote { price: v, change_pct: Some(v), ..Quote::default() };
            let cell = Cell { label: "X".into(), digits: 2, quote: Some(q), stale: false };
            assert_eq!(board_text(&[cell]), "X\t--\t--");
        }
        assert_eq!(fmt_price(-1234.5, 1), "-1,234.5");
        assert_eq!(fmt_change(1.5), "+1.50%");
    }

    #[test]
    fn board_text_is_tab_separated_with_dashes_for_gaps() {
        let cell = |label: &str, digits: u32, quote: Option<Quote>| Cell { label: label.into(), digits, quote, stale: false };
        let xau = Quote { price: 4154.456, change_pct: Some(0.32), ..Quote::default() };
        let btc = Quote { price: 81176.4, ..Quote::default() };
        let cells = [cell("XAU", 2, Some(xau)), cell("DOW", 0, None), cell("BTC", 0, Some(btc))];
        assert_eq!(board_text(&cells), "XAU\t4,154.46\t+0.32%\nDOW\t--\t--\nBTC\t81,176\t--");
        assert!(!board_text(&cells).ends_with('\n'));
        // No cells is the empty string.
        assert_eq!(board_text(&[]), "");
        // One cell, with a quote and without one.
        assert_eq!(board_text(&cells[..1]), "XAU\t4,154.46\t+0.32%");
        assert_eq!(board_text(&[cell("DOW", 0, None)]), "DOW\t--\t--");
        // A zero change uses fmt_change's rule: a plus sign, as elsewhere in the app.
        let flat = Quote { price: 2.0, change_pct: Some(0.0), ..Quote::default() };
        assert_eq!(board_text(&[cell("X", 2, Some(flat.clone()))]), "X\t2.00\t+0.00%");
        let old = Cell { stale: true, ..cell("Y", 2, Some(flat)) };
        assert_eq!(board_text(&[old]), "Y\t2.00\t+0.00%\tstale");
        let gone = Cell { stale: true, ..cell("Z", 2, None) };
        assert_eq!(board_text(&[gone]), "Z\t--\t--");
    }

    fn q(sym: &str, ts: Option<f64>, realtime: bool) -> Quote {
        Quote { symbol: sym.into(), price: 1.0, ts, realtime, ..Quote::default() }
    }

    #[test]
    fn fallback_expiry_does_not_need_network_completion() {
        let rows = crate::config::default_rows();
        let mut b = Board::new(&rows);
        b.cells[0].quote = Some(Quote { symbol: rows[0].symbol.clone(), source: "Gold API", received_at: Some(100.0), price: 2.0, ..Quote::default() });
        assert!(!b.expire_fallback(189.99));
        assert!(b.expire_fallback(190.0));
        assert!(b.cells[0].quote.is_none());
        assert!(!b.expire_fallback(200.0));
    }

    #[test]
    fn disabling_invalidates_inflight_fallback_and_clears_only_fallback() {
        let rows = crate::config::default_rows();
        let mut b = Board::new(&rows);
        b.set_fallback(true);
        let gen = b.fallback_gen;
        let row = rows[0].clone();
        let q = Quote { symbol: row.symbol.clone(), source: "Gold API", price: 5.0, ..Quote::default() };
        assert!(b.publish_fallback(gen, b.rows_gen, &row, q.clone()));
        assert_eq!(b.cells[0].quote.as_ref().unwrap().source, "Gold API");
        b.set_fallback(false);
        assert!(b.cells[0].quote.is_none());
        b.set_fallback(true);
        assert!(!b.publish_fallback(gen, b.rows_gen, &row, q));
        assert!(b.cells[0].quote.is_none());
    }

    #[test]
    fn stress_toggle_rejects_all_obsolete_inflight_results() {
        let rows = crate::config::default_rows();
        let row = rows[0].clone();
        let mut b = Board::new(&rows);
        for _ in 0..10_000 {
            b.set_fallback(true);
            let old = b.fallback_gen;
            b.set_fallback(false);
            b.set_fallback(true);
            let q = Quote { symbol: row.symbol.clone(), source: "Gold API", price: 1.0, ..Quote::default() };
            assert!(!b.publish_fallback(old, b.rows_gen, &row, q.clone()));
            assert!(b.publish_fallback(b.fallback_gen, b.rows_gen, &row, q));
            b.set_fallback(false);
            assert!(b.cells[0].quote.is_none());
        }
    }

    #[test]
    fn an_old_quote_on_a_live_link_is_a_closed_market() {
        let cell = |ts: Option<f64>, source: &'static str, stale: bool| Cell {
            label: "XAG".into(),
            digits: 3,
            quote: Some(Quote { symbol: "XAG=".into(), source, price: 48.72, ts, realtime: true, ..Quote::default() }),
            stale,
        };
        let now = 100_000.0;
        let old = cell(Some(now - 2.0 * 86_400.0), "", true);
        assert!(market_closed(&old, Link::Partial, now), "two days old, the service answering");
        assert!(market_closed(&old, Link::Live, now));
        assert!(!market_closed(&old, Link::Down, now), "a dead feed is not a closed market");
        assert!(!market_closed(&cell(Some(now - CLOSED_AFTER_S + 1.0), "", true), Link::Live, now), "stale, but only just");
        assert!(market_closed(&cell(Some(now - CLOSED_AFTER_S), "", true), Link::Live, now), "the limit itself counts");
        assert!(!market_closed(&cell(Some(now - 5.0), "", false), Link::Live, now), "fresh");
        assert!(!market_closed(&cell(Some(now - 86_400.0), "", false), Link::Live, now), "not stale by the board's rule");
        assert!(!market_closed(&cell(None, "", true), Link::Live, now), "no time, no age");
        assert!(!market_closed(&cell(Some(now - 86_400.0), "Metals.dev", true), Link::Live, now), "a fallback quote has no age");
        let none = Cell { quote: None, ..old };
        assert!(!market_closed(&none, Link::Live, now), "no quote");
    }

    #[test]
    fn a_quote_time_ahead_of_the_clock_has_no_age() {
        let now = 100_000.0;
        let ahead = |s: f64| q("@SP.1", Some(now + s), false);
        assert!(!is_stale(&ahead(FUTURE_SKEW_S), now, 90.0, 900.0), "a few minutes of skew is fresh");
        assert!(is_stale(&ahead(FUTURE_SKEW_S + 1.0), now, 90.0, 900.0), "never age 0");
        assert!(is_stale(&ahead(19_000.0), now, 90.0, 900.0), "the Saturday futures stamp, hours ahead");
        let cell = Cell { label: "S&P".into(), digits: 2, quote: Some(ahead(19_000.0)), stale: true };
        assert!(!market_closed(&cell, Link::Live, now), "unknown age is not a closed market");
        assert_eq!(closed_since(cell.quote.as_ref().unwrap(), now), None);
    }

    #[test]
    fn a_price_that_stopped_moving_under_a_fresh_stamp_is_a_closed_market() {
        let now = 100_000.0;
        let mut quote = q("@SP.1", Some(now - 4.0), true);
        quote.moved_at = Some(now - CLOSED_AFTER_S + 1.0);
        assert!(!restamped(&quote, now) && !is_stale(&quote, now, 90.0, 900.0), "unchanged for under 15 minutes");
        quote.moved_at = Some(now - 3_600.0);
        assert!(restamped(&quote, now), "the stamp is fresh, the price is an hour old");
        assert!(is_stale(&quote, now, 90.0, 900.0), "so an alert cannot use it");
        assert_eq!(closed_since(&quote, now), Some(now - 3_600.0));
        let cell = Cell { label: "S&P".into(), digits: 2, quote: Some(quote.clone()), stale: true };
        assert!(market_closed(&cell, Link::Live, now));
        assert!(!market_closed(&cell, Link::Down, now));
        let fallback = Quote { source: "Gold API", ..quote };
        assert!(!restamped(&fallback, now), "a fallback quote has no observed history");
    }

    fn closed_cell(label: &str, age_s: f64, now: f64) -> Cell {
        Cell { label: label.into(), digits: 2, quote: Some(q("@SP.1", Some(now - age_s), false)), stale: true }
    }

    #[test]
    fn calm_is_closed_only_when_every_stale_cell_is_closed() {
        let now = 1_000_000.0;
        let mut b = Board::new(&[]);
        b.link = Link::Partial;
        let live = Cell { label: "BTC".into(), digits: 0, quote: Some(q("BTC", Some(now - 2.0), true)), stale: false };
        b.cells = vec![closed_cell("S&P", 20.0 * 3600.0, now), live.clone()];
        assert_eq!(b.calm(now), Calm::Some(1));
        b.cells = vec![closed_cell("S&P", 20.0 * 3600.0, now), closed_cell("DOW", 20.0 * 3600.0, now)];
        assert_eq!(b.calm(now), Calm::All(2));
        // One stale cell that is not closed (a fault) keeps the amber look for the whole board.
        let mut stuck = closed_cell("NAS", 100.0, now);
        stuck.quote.as_mut().unwrap().ts = None;
        b.cells.push(stuck);
        assert_eq!(b.calm(now), Calm::No);
        b.cells = vec![closed_cell("S&P", 20.0 * 3600.0, now), live];
        b.link = Link::Down;
        assert_eq!(b.calm(now), Calm::No, "a dead feed is a fault");
        b.link = Link::Live;
        b.cells = vec![];
        assert_eq!(b.calm(now), Calm::No);
    }

    #[test]
    fn closed_since_reads_as_a_local_day_and_clock() {
        let sat = crate::probe::parse_time("2026-11-14T17:18:36.000+0000").unwrap();
        let fri = crate::probe::parse_time("2026-11-13T17:00:00.000-0500").unwrap();
        let est = |_: f64| -5 * 3600;
        assert_eq!(fmt_since(&est, sat, fri), "Fri 17:00");
        assert_eq!(fmt_since(&|_| 0, sat, fri), "Fri 22:00");
        // Seven days back a day name would name the wrong day, so the date is given.
        assert_eq!(fmt_since(&est, sat + 7.0 * 86_400.0, fri), "Nov 13 17:00");
        assert_eq!(fmt_since(&est, sat, f64::NAN), "Sat 12:18");
        let q = q("@SP.1", Some(fri), false);
        let words = closed_words("CNBC delayed", closed_since(&q, sat), &est, sat);
        assert_eq!(words, ["CNBC delayed \u{b7} closed since Fri 17:00", "closed since Fri 17:00", "closed"]);
        assert_eq!(closed_words("", closed_since(&q, sat), &est, sat), ["closed since Fri 17:00", "closed"]);
        assert_eq!(closed_words("CNBC", None, &est, sat), ["closed"]);
    }

    #[test]
    fn the_first_option_that_fits_wins_and_the_last_is_the_fallback() {
        let words = closed_words("CNBC", Some(0.0), &|_| 0, 100_000.0);
        assert_eq!(first_fitting(&words, &|w| w.len() <= 40), words[0]);
        assert_eq!(first_fitting(&words, &|w| w.len() <= 25), words[1]);
        assert_eq!(first_fitting(&words, &|_| false), "closed");
        assert_eq!(first_fitting(&[], &|_| true), "");
    }

    #[test]
    fn delayed_quotes_get_grace_and_missing_time_is_stale() {
        assert!(!is_stale(&q("A", Some(1000.0), true), 1080.0, 90.0, 900.0));
        assert!(is_stale(&q("A", Some(1000.0), true), 1100.0, 90.0, 900.0));
        assert!(!is_stale(&q("F", Some(1000.0), false), 1600.0, 90.0, 900.0));
        assert!(is_stale(&q("F", Some(1000.0), false), 2100.0, 90.0, 900.0));
        assert!(is_stale(&q("A", None, true), 0.0, 90.0, 900.0));
    }

    #[test]
    fn resolve_keeps_row_order_and_marks_missing() {
        let rows = vec![
            Row { label: "XAU".into(), symbol: "XAU=".into(), digits: 2, name: String::new(), strip: true },
            Row { label: "DOW".into(), symbol: "@DJ.1".into(), digits: 0, name: String::new(), strip: true },
        ];
        let quotes: HashMap<_, _> = [("XAU=".to_string(), q("XAU=", Some(100.0), true))].into_iter().collect();
        let cells = resolve(&rows, &quotes, 110.0, 90.0, 900.0);
        assert_eq!(cells[0].label, "XAU");
        assert!(!cells[0].stale);
        assert!(cells[1].quote.is_none() && cells[1].stale);
    }

    #[test]
    fn row_changes_keep_chart_data_attached_to_their_symbols() {
        let a = Row { label: "A".into(), symbol: "AAA".into(), digits: 2, name: String::new(), strip: true };
        let b = Row { label: "B".into(), symbol: "BBB".into(), digits: 2, name: String::new(), strip: true };
        let c = Row { label: "C".into(), symbol: "CCC".into(), digits: 2, name: String::new(), strip: true };
        let mut board = Board::new(&[a, b.clone()]);
        let bar = |v| Bar { dt: 0.0, o: v, h: v, l: v, c: v, v: 0.0 };
        board.bars = vec![vec![bar(1.0)].into(), vec![bar(2.0)].into()];
        board.bars_t0 = vec![100.0, 200.0];
        board.backfill = vec![vec![(100.0, 1.0)], vec![(200.0, 2.0)]];
        board.set_rows(&[b, c]);
        assert_eq!(board.bars[0][0].c, 2.0);
        assert_eq!(board.bars_t0[0], 200.0);
        assert_eq!(board.backfill[0], vec![(200.0, 2.0)]);
        assert!(board.bars[1].is_empty());
        assert!(board.backfill[1].is_empty());
    }

    #[test]
    fn row_with_same_label_but_new_symbol_does_not_keep_old_quote_or_chart() {
        let old = Row { label: "A".into(), symbol: "OLD".into(), digits: 2, name: String::new(), strip: true };
        let new = Row { symbol: "NEW".into(), ..old.clone() };
        let mut board = Board::new(&[old]);
        board.cells[0].quote = Some(q("OLD", Some(100.0), true));
        board.bars = vec![vec![Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }].into()];
        board.set_rows(&[new]);
        assert!(board.cells[0].quote.is_none());
        assert!(board.bars[0].is_empty());
    }

    #[test]
    fn formats_prices_and_changes() {
        assert_eq!(fmt_price(81176.4, 0), "81,176");
        assert_eq!(fmt_price(4154.456, 2), "4,154.46");
        assert_eq!(fmt_price(-1234.5, 1), "-1,234.5");
        assert_eq!(fmt_price(56.0312, 3), "56.031");
        assert_eq!(fmt_change(0.3214), "+0.32%");
        assert_eq!(fmt_change(-1.2), "-1.20%");
    }

    #[test]
    fn a_renamed_row_keeps_its_quote_bars_and_cell() {
        let old = Row { label: "XAU".into(), symbol: "XAU=".into(), digits: 2, name: String::new(), strip: true };
        let new = Row { label: "Gold".into(), digits: 3, ..old.clone() };
        let mut board = Board::new(&[old]);
        board.cells[0].quote = Some(q("XAU=", Some(100.0), true));
        board.cells[0].stale = false;
        board.bars = vec![vec![Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }].into()];
        board.backfill = vec![vec![(1.0, 1.0)]];
        board.set_rows(&[new]);
        assert_eq!((board.cells[0].label.as_str(), board.cells[0].digits), ("Gold", 3));
        assert!(board.cells[0].quote.is_some() && !board.cells[0].stale, "the price is still there");
        assert_eq!(board.bars[0].len(), 1);
        assert_eq!(board.backfill[0].len(), 1);
    }
}
