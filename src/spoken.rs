//! The sentence a screen reader gets, `std` only: the window's name, which Windows also shows in the
//! taskbar's thumbnail and in Alt+Tab.
//!
//! With a card locked the name is that card as a sentence, `Gold 4,012.30, up 0.4 percent`, with
//! `, closed` added when its market is closed, `, quiet` while it is calm but not yet closed
//! (`model::calm_stale`), and `, stale` when its price is old for another reason. With none locked it is one line about the board. The name ends with the app's own, so
//! the window is still recognisable. Classic never gets a price in its window name, so `window_name`
//! answers `None` for it and the title stays as it always was.

use crate::model::{calm_stale, fmt_price, reads_closed, Cell, Link};

/// A change this small reads as no move: `0.00` is how the card shows it.
const FLAT: f64 = 0.005;

/// The percent as spoken: at most two decimals, none that are only zeros (`0.4`, `0.04`, `12`).
fn percent(p: f64) -> String {
    let s = format!("{:.2}", p.abs());
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One card as a sentence. No quote reads `Gold, no price`; a quote with no day change reads
/// `Gold 4,012.30, no day change`; zero reads `unchanged`; a price of a closed market ends with
/// `, closed`, a calm price that is not yet closed with `, quiet`, and any other stale price with
/// `, stale`. `link` and `now` (UTC seconds) tell them apart.
pub fn spoken_line(cell: &Cell, link: Link, now: f64) -> String {
    let Some(q) = cell.quote.as_ref().filter(|q| q.price.is_finite()) else {
        return format!("{}, no price", cell.label);
    };
    let mut out = format!("{} {}", cell.label, fmt_price(q.price, cell.digits));
    match q.change_pct.filter(|p| p.is_finite()) {
        Some(p) if p.abs() < FLAT => out.push_str(", unchanged"),
        Some(p) if p > 0.0 => out.push_str(&format!(", up {} percent", percent(p))),
        Some(p) => out.push_str(&format!(", down {} percent", percent(p))),
        None => out.push_str(", no day change"),
    }
    if reads_closed(cell, link, now) {
        out.push_str(", closed");
    } else if calm_stale(cell, link, now) {
        out.push_str(", quiet");
    } else if cell.stale {
        out.push_str(", stale");
    }
    out
}

/// The board in one line: `6 symbols, 4 up, 2 down, 1 closed, 1 quiet, 1 stale` (a count that is zero
/// is left out; no cards is `no symbols`).
pub fn summary_line(cells: &[Cell], link: Link, now: f64) -> String {
    if cells.is_empty() {
        return "no symbols".into();
    }
    let change = |c: &Cell| c.quote.as_ref().and_then(|q| q.change_pct).filter(|p| p.is_finite());
    let up = cells.iter().filter(|c| change(c).is_some_and(|p| p >= FLAT)).count();
    let down = cells.iter().filter(|c| change(c).is_some_and(|p| p <= -FLAT)).count();
    let closed = cells.iter().filter(|c| reads_closed(c, link, now)).count();
    let quiet = cells.iter().filter(|c| calm_stale(c, link, now) && !reads_closed(c, link, now)).count();
    let stale = cells.iter().filter(|c| c.stale && c.quote.is_some() && !calm_stale(c, link, now)).count();
    let mut out = format!("{} symbol{}", cells.len(), if cells.len() == 1 { "" } else { "s" });
    for (n, word) in [(up, "up"), (down, "down"), (closed, "closed"), (quiet, "quiet"), (stale, "stale")] {
        if n > 0 {
            out.push_str(&format!(", {n} {word}"));
        }
    }
    out
}

/// The window's name: `None` for Classic (its title never changes, and never holds a price), else the
/// locked card's sentence or the board's line, then the app's own name.
pub fn window_name(ultra: bool, app: &str, locked: Option<&Cell>, cells: &[Cell], link: Link, now: f64) -> Option<String> {
    if !ultra {
        return None;
    }
    Some(match locked {
        Some(c) => format!("{} - {app}", spoken_line(c, link, now)),
        None => format!("{app}, {}", summary_line(cells, link, now)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Quote;

    fn line(c: &Cell) -> String {
        spoken_line(c, Link::Live, 0.0)
    }

    fn summary(cells: &[Cell]) -> String {
        summary_line(cells, Link::Live, 0.0)
    }

    fn cell(label: &str, price: Option<f64>, change: Option<f64>, stale: bool) -> Cell {
        let quote = price.map(|price| Quote {
            symbol: String::new(),
            source: "",
            received_at: None,
            name: String::new(),
            price,
            change_pct: change,
            prev_close: None,
            ts: None,
            moved_at: None,
            realtime: true,
            open: None,
            high: None,
            low: None,
            volume: None,
            yr_hi: None,
            yr_lo: None,
            yr_hi_date: None,
            currency: String::new(),
        });
        Cell { label: label.into(), digits: 2, quote, stale }
    }

    #[test]
    fn a_rise_a_fall_and_a_flat_card_each_read_as_one_sentence() {
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(0.4), false)), "Gold 4,012.30, up 0.4 percent");
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(-1.25), false)), "Gold 4,012.30, down 1.25 percent");
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(0.0), false)), "Gold 4,012.30, unchanged");
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(0.003), false)), "Gold 4,012.30, unchanged");
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(-0.004), false)), "Gold 4,012.30, unchanged");
        assert_eq!(line(&cell("BTC", Some(112480.0), Some(12.0), false)), "BTC 112,480.00, up 12 percent");
        assert_eq!(line(&cell("VIX", Some(14.5), Some(0.04), false)), "VIX 14.50, up 0.04 percent");
    }

    #[test]
    fn a_stale_price_says_so_and_still_says_which_way() {
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(0.4), true)), "Gold 4,012.30, up 0.4 percent, stale");
        assert_eq!(line(&cell("Gold", Some(4012.3), None, true)), "Gold 4,012.30, no day change, stale");
    }

    #[test]
    fn no_quote_and_no_change_are_said_plainly() {
        assert_eq!(line(&cell("Gold", None, None, false)), "Gold, no price");
        assert_eq!(line(&cell("Gold", Some(f64::NAN), Some(1.0), false)), "Gold, no price");
        assert_eq!(line(&cell("Gold", Some(4012.3), None, false)), "Gold 4,012.30, no day change");
        assert_eq!(line(&cell("Gold", Some(4012.3), Some(f64::NAN), false)), "Gold 4,012.30, no day change");
    }

    #[test]
    fn the_board_line_counts_what_moved() {
        let cells = vec![
            cell("A", Some(1.0), Some(1.0), false),
            cell("B", Some(1.0), Some(-1.0), false),
            cell("C", Some(1.0), Some(0.0), true),
            cell("D", None, None, false),
        ];
        assert_eq!(summary(&cells), "4 symbols, 1 up, 1 down, 1 stale");
        assert_eq!(summary(&cells[..1]), "1 symbol, 1 up");
        assert_eq!(summary(&[]), "no symbols");
    }

    #[test]
    fn the_window_name_follows_the_lock_and_classic_gets_none() {
        let cells = vec![cell("Gold", Some(4012.3), Some(0.4), false), cell("BTC", Some(1.0), Some(-1.0), false)];
        assert_eq!(window_name(true, "MH Ticker Ultra", Some(&cells[0]), &cells, Link::Live, 0.0).as_deref(), Some("Gold 4,012.30, up 0.4 percent - MH Ticker Ultra"));
        assert_eq!(window_name(true, "MH Ticker Ultra", None, &cells, Link::Live, 0.0).as_deref(), Some("MH Ticker Ultra, 2 symbols, 1 up, 1 down"));
        // Classic: the title is never touched, so a price never lands in it.
        assert_eq!(window_name(false, "MH Ticker", Some(&cells[0]), &cells, Link::Live, 0.0), None);
        assert_eq!(window_name(false, "MH Ticker", None, &cells, Link::Live, 0.0), None);
        for n in [window_name(true, "MH Ticker Ultra", Some(&cells[1]), &cells, Link::Live, 0.0).unwrap(), summary(&cells)] {
            assert!(!n.contains('\u{2014}') && !n.contains('\u{2013}'), "{n}");
        }
    }

    /// A card whose quote is `age_s` old at `now`, marked stale.
    fn old_cell(label: &str, age_s: f64, now: f64) -> Cell {
        let mut c = cell(label, Some(4012.3), Some(0.4), true);
        c.quote.as_mut().unwrap().ts = Some(now - age_s);
        c
    }

    #[test]
    fn a_closed_market_says_closed_and_a_fault_says_stale() {
        let now = 1_000_000.0;
        let closed = old_cell("Gold", 20.0 * 3600.0, now);
        assert_eq!(spoken_line(&closed, Link::Partial, now), "Gold 4,012.30, up 0.4 percent, closed");
        assert_eq!(spoken_line(&closed, Link::Down, now), "Gold 4,012.30, up 0.4 percent, stale", "a dead feed is a fault");
        let no_time = cell("Gold", Some(4012.3), Some(0.4), true);
        assert_eq!(spoken_line(&no_time, Link::Partial, now), "Gold 4,012.30, up 0.4 percent, stale");
        let cells = vec![closed, no_time, cell("BTC", Some(1.0), Some(-1.0), false)];
        assert_eq!(summary_line(&cells, Link::Partial, now), "3 symbols, 2 up, 1 down, 1 closed, 1 stale");
        let name = window_name(true, "MH Ticker Ultra", None, &cells, Link::Partial, now).unwrap();
        assert_eq!(name, "MH Ticker Ultra, 3 symbols, 2 up, 1 down, 1 closed, 1 stale");
    }
}
