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

    /// Swap in a new symbol list, keeping data only for the same label and symbol.
    /// Candle and backfill vectors are indexed like `rows`; remap them atomically with the cells
    /// so an in-flight fetch cannot briefly put one instrument's history under another label.
    pub fn set_rows(&mut self, rows: &[Row]) {
        let indices: Vec<_> = rows.iter().map(|r| {
            self.rows.iter().position(|old| old.label == r.label && old.symbol == r.symbol)
        }).collect();
        self.cells = rows.iter().zip(&indices).map(|(r, i)| {
            i.and_then(|i| self.cells.get(i))
                .map(|c| Cell { digits: r.digits, ..c.clone() })
                .unwrap_or_else(|| empty_cell(r))
        }).collect();
        self.bars = indices.iter().map(|i| i.and_then(|i| self.bars.get(i)).cloned().unwrap_or_default()).collect();
        self.bars_t0 = indices.iter().map(|i| i.and_then(|i| self.bars_t0.get(i)).copied().unwrap_or_default()).collect();
        self.backfill = indices.iter().map(|i| i.and_then(|i| self.backfill.get(i)).cloned().unwrap_or_default()).collect();
        self.rows = rows.to_vec();
        self.rows_gen += 1;
    }
}

pub fn is_stale(q: &Quote, now: f64, stale_after: f64, delayed_grace: f64) -> bool {
    let limit = stale_after + if q.realtime { 0.0 } else { delayed_grace };
    q.ts.map(|t| now - t > limit).unwrap_or(true)
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
            Row { label: "XAU".into(), symbol: "XAU=".into(), digits: 2, name: String::new() },
            Row { label: "DOW".into(), symbol: "@DJ.1".into(), digits: 0, name: String::new() },
        ];
        let quotes: HashMap<_, _> = [("XAU=".to_string(), q("XAU=", Some(100.0), true))].into_iter().collect();
        let cells = resolve(&rows, &quotes, 110.0, 90.0, 900.0);
        assert_eq!(cells[0].label, "XAU");
        assert!(!cells[0].stale);
        assert!(cells[1].quote.is_none() && cells[1].stale);
    }

    #[test]
    fn row_changes_keep_chart_data_attached_to_their_symbols() {
        let a = Row { label: "A".into(), symbol: "AAA".into(), digits: 2, name: String::new() };
        let b = Row { label: "B".into(), symbol: "BBB".into(), digits: 2, name: String::new() };
        let c = Row { label: "C".into(), symbol: "CCC".into(), digits: 2, name: String::new() };
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
        let old = Row { label: "A".into(), symbol: "OLD".into(), digits: 2, name: String::new() };
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
}
