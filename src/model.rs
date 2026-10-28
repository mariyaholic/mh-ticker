//! Quotes, staleness, the board the UI draws, and number formatting. Pure code: no I/O, unit-tested.

use crate::config::Row;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub symbol: String,
    /// CNBC's name for it, e.g. "Dow Jones Fut (Dec'26)".
    pub name: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub prev_close: Option<f64>,
    /// UTC epoch seconds of the price; None = unknown (treated as stale).
    pub ts: Option<f64>,
    /// False when CNBC marks the quote as delayed (its index futures, ~10 min).
    pub realtime: bool,
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

#[derive(Clone, Debug)]
pub struct Board {
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
}

fn empty_cell(r: &Row) -> Cell {
    Cell { label: r.label.clone(), digits: r.digits, quote: None, stale: true }
}

impl Board {
    pub fn new(rows: &[Row]) -> Self {
        Self {
            rows: rows.to_vec(),
            rows_gen: 0,
            cells: rows.iter().map(empty_cell).collect(),
            link: Link::Down,
            note: "starting".into(),
            backfill: Vec::new(),
            backfill_gen: 0,
        }
    }

    /// Swap in a new symbol list. A symbol that stays keeps its last quote, so the strip does not
    /// blink; a new one is empty until the feed has its price.
    pub fn set_rows(&mut self, rows: &[Row]) {
        self.cells = rows
            .iter()
            .map(|r| {
                self.cells
                    .iter()
                    .find(|c| c.label == r.label)
                    .map(|c| Cell { digits: r.digits, ..c.clone() })
                    .unwrap_or_else(|| empty_cell(r))
            })
            .collect();
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
    format!("{}{:.2}%", if pct >= 0.0 { "+" } else { "" }, pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(sym: &str, ts: Option<f64>, realtime: bool) -> Quote {
        Quote { symbol: sym.into(), name: String::new(), price: 1.0, change_pct: None, prev_close: None, ts, realtime }
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
    fn formats_prices_and_changes() {
        assert_eq!(fmt_price(81176.4, 0), "81,176");
        assert_eq!(fmt_price(4154.456, 2), "4,154.46");
        assert_eq!(fmt_price(-1234.5, 1), "-1,234.5");
        assert_eq!(fmt_price(56.0312, 3), "56.031");
        assert_eq!(fmt_change(0.3214), "+0.32%");
        assert_eq!(fmt_change(-1.2), "-1.20%");
    }
}
