//! The expanded panel: one card per symbol (price, day change, day open, source and age, and a
//! sparkline of the last hour). Drawn with GDI into whatever DC `ui.rs` hands it.
//!
//! The sparkline is a rolling one-hour window: seeded from CNBC's 1-minute bars (so a fresh launch
//! already shows the past hour), then extended by live samples, and scrolling left as time passes.

use crate::model::{fmt_change, fmt_price, Board, Cell};

use crate::ui::{link_color, link_name, BG, DIM, DOWN, LABEL, PRICE, UP, AMBER};
use std::collections::VecDeque;
use windows_sys::Win32::Foundation::{COLORREF, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;

const COLS: usize = 3;
/// The sparkline window, and a hard cap on points per row (an hour of 1 s samples).
const WINDOW_S: f64 = 3600.0;
const HIST_CAP: usize = 3600;
const SAMPLE_S: f64 = 1.0;
/// A bar point closer than this to a live sample is redundant.
const BAR_DEDUP_S: f64 = 45.0;

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const CARD: COLORREF = rgb(28, 31, 37);
const BTN_HOVER: COLORREF = rgb(52, 57, 66);
const GUIDE: COLORREF = rgb(52, 56, 64);

pub struct Fonts {
    big: HFONT,
    mid: HFONT,
    small: HFONT,
}

fn font(face: &[u16], pt: i32, weight: u32, dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -(pt * dpi as i32 / 72), 0, 0, 0, weight as _, 0, 0, 0, DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _,
            CLIP_DEFAULT_PRECIS as _, CLEARTYPE_QUALITY as _, (DEFAULT_PITCH | FF_DONTCARE) as _, face.as_ptr(),
        )
    }
}

impl Fonts {
    pub fn new(face: &str, dpi: u32) -> Self {
        let f: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
        Self { big: font(&f, 18, FW_SEMIBOLD, dpi), mid: font(&f, 11, FW_SEMIBOLD, dpi), small: font(&f, 9, FW_NORMAL, dpi) }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.big);
            DeleteObject(self.mid);
            DeleteObject(self.small);
        }
    }
}

/// (utc_s, price) points, oldest first.
type Series = VecDeque<(f64, f64)>;

/// The last hour of prices per row. Live samples are kept at most one per second; 1-minute bar
/// closes fill in wherever no live sample sits within 45 s (before launch, or across a sleep).
/// Series are kept by label, so a symbol that stays keeps its line when the Symbols menu adds or
/// removes others.
pub struct History {
    labels: Vec<String>,
    rows: Vec<Series>,
    gen: u64,
    pub backfilled: bool,
}

/// Merge bar closes into a sorted series, skipping any within `BAR_DEDUP_S` of a point already there.
fn seed(row: &mut VecDeque<(f64, f64)>, bars: &[(f64, f64)]) {
    let have: Vec<f64> = row.iter().map(|p| p.0).collect();
    let mut merged: Vec<(f64, f64)> = row.iter().copied().collect();
    for &(t, v) in bars {
        let i = have.partition_point(|&x| x < t);
        let near = |j: usize| have.get(j).is_some_and(|&x| (x - t).abs() < BAR_DEDUP_S);
        if !(near(i) || (i > 0 && near(i - 1))) {
            merged.push((t, v));
        }
    }
    merged.sort_by(|a, b| a.0.total_cmp(&b.0));
    *row = merged.into();
}

impl History {
    pub fn new() -> Self {
        Self { labels: Vec::new(), rows: Vec::new(), gen: 0, backfilled: false }
    }

    /// Re-key the series when the board's rows changed (by label; new rows start empty).
    fn align(&mut self, board: &Board) {
        if self.labels.len() == board.cells.len() && self.labels.iter().zip(&board.cells).all(|(l, c)| *l == c.label) {
            return;
        }
        let mut old: Vec<(String, Series)> = self.labels.drain(..).zip(self.rows.drain(..)).collect();
        for c in &board.cells {
            let row = match old.iter().position(|(l, _)| *l == c.label) {
                Some(i) => old.swap_remove(i).1,
                None => VecDeque::new(),
            };
            self.labels.push(c.label.clone());
            self.rows.push(row);
        }
    }

    pub fn record(&mut self, board: &Board, now: f64) {
        self.align(board);
        let reseed = board.backfill_gen != self.gen;
        self.gen = board.backfill_gen;
        for (i, c) in board.cells.iter().enumerate() {
            let row = &mut self.rows[i];
            if reseed {
                if let Some(bars) = board.backfill.get(i).filter(|b| !b.is_empty()) {
                    seed(row, bars);
                    self.backfilled = true;
                }
            }
            if let (Some(q), false) = (&c.quote, c.stale) {
                match row.back_mut() {
                    Some(last) if now - last.0 < SAMPLE_S => last.1 = q.price,
                    _ => row.push_back((now, q.price)),
                }
            }
            while row.front().is_some_and(|p| p.0 < now - WINDOW_S) || row.len() > HIST_CAP {
                row.pop_front();
            }
        }
    }

    fn row(&self, i: usize) -> Option<&VecDeque<(f64, f64)>> {
        self.rows.get(i)
    }
}

pub struct Geo {
    pub w: i32,
    pub h: i32,
    pub header: i32,
    pub btn: RECT,
    cols: usize,
    pad: i32,
    gap: i32,
    card_w: i32,
    card_h: i32,
}

pub fn geometry(n: usize, dpi: u32) -> Geo {
    let s = |v: i32| v * dpi as i32 / 96;
    let cols = n.clamp(1, COLS);
    let rows = n.div_ceil(COLS).max(1) as i32;
    let (pad, gap, card_w, card_h, header) = (s(16), s(10), s(224), s(132), s(48));
    let w = 2 * pad + cols as i32 * card_w + (cols as i32 - 1) * gap;
    let h = header + rows * card_h + (rows - 1) * gap + s(36);
    let btn = RECT { left: w - s(12) - s(38), top: s(10), right: w - s(12), bottom: s(10) + s(30) };
    Geo { w, h, header, btn, cols, pad, gap, card_w, card_h }
}

pub fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Draw `s` with its top-left at (x, y); returns its width.
fn text(dc: HDC, font: HFONT, x: i32, y: i32, s: &str, color: COLORREF) -> i32 {
    unsafe {
        let old = SelectObject(dc, font);
        SetTextColor(dc, color);
        let t = utf16(s);
        TextOutW(dc, x, y, t.as_ptr(), t.len() as i32);
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        SelectObject(dc, old);
        sz.cx
    }
}

/// Draw `s` with its top-right at (right, y).
fn text_right(dc: HDC, font: HFONT, right: i32, y: i32, s: &str, color: COLORREF) {
    unsafe {
        let old = SelectObject(dc, font);
        let t = utf16(s);
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        SetTextColor(dc, color);
        TextOutW(dc, right - sz.cx, y, t.as_ptr(), t.len() as i32);
        SelectObject(dc, old);
    }
}

fn line_height(dc: HDC, font: HFONT) -> i32 {
    unsafe {
        let old = SelectObject(dc, font);
        let t = utf16("Xg");
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        SelectObject(dc, old);
        sz.cy
    }
}

fn fill_round(dc: HDC, r: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_b = SelectObject(dc, brush);
        let old_p = SelectObject(dc, GetStockObject(NULL_PEN));
        // A NULL pen draws one pixel short on the right and bottom.
        RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, radius, radius);
        SelectObject(dc, old_p);
        SelectObject(dc, old_b);
        DeleteObject(brush);
    }
}

fn fill_ellipse(dc: HDC, x: i32, y: i32, d: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_b = SelectObject(dc, brush);
        let old_p = SelectObject(dc, GetStockObject(NULL_PEN));
        Ellipse(dc, x, y, x + d + 1, y + d + 1);
        SelectObject(dc, old_p);
        SelectObject(dc, old_b);
        DeleteObject(brush);
    }
}

fn fill_rect(dc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, &r, brush);
        DeleteObject(brush);
    }
}

pub fn fmt_age(s: f64) -> String {
    let s = s.max(0.0);
    if s < 60.0 {
        format!("{:.0}s", s.floor())
    } else if s < 3600.0 {
        format!("{:.0}m", (s / 60.0).floor())
    } else {
        format!("{:.0}h", (s / 3600.0).floor())
    }
}

/// The day's open and absolute change implied by a price and its change %.
pub fn day_open(price: f64, pct: f64) -> (f64, f64) {
    let open = price / (1.0 + pct / 100.0);
    (open, price - open)
}

/// The last hour on a fixed time axis (now - 1 h .. now), so the line scrolls left as time passes.
fn spark(dc: HDC, f: &Fonts, r: RECT, pts: Option<&VecDeque<(f64, f64)>>, dpi: u32, now: f64) {
    let s = |v: i32| v * dpi as i32 / 96;
    let pts = match pts {
        Some(p) if p.len() >= 2 => p,
        _ => {
            text(dc, f.small, r.left, r.top + s(4), "collecting\u{2026}", DIM);
            return;
        }
    };
    let (t0, tspan) = (now - WINDOW_S, WINDOW_S);
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let (w, h) = ((r.right - r.left) as f64, (r.bottom - r.top) as f64);
    let span = hi - lo;
    let y_of = |v: f64| if span > 0.0 { r.bottom as f64 - (v - lo) / span * h } else { r.top as f64 + h / 2.0 };
    let line: Vec<POINT> =
        pts.iter().map(|&(t, v)| POINT { x: r.left + ((t - t0) / tspan * w).clamp(0.0, w).round() as i32, y: y_of(v).round() as i32 }).collect();
    let first = pts.front().unwrap().1;
    let last = pts.back().unwrap().1;
    let color = if last >= first { UP } else { DOWN };
    unsafe {
        // reference line at the first sample
        let gy = y_of(first).round() as i32;
        fill_rect(dc, RECT { left: r.left, top: gy, right: r.right, bottom: gy + 1 }, GUIDE);
        let pen = CreatePen(PS_SOLID, s(2).max(1), color);
        let old = SelectObject(dc, pen);
        Polyline(dc, line.as_ptr(), line.len() as i32);
        SelectObject(dc, old);
        DeleteObject(pen);
        let d = s(5);
        let end = line.last().unwrap();
        fill_ellipse(dc, end.x - d / 2, end.y - d / 2, d, color);
    }
}

fn card(dc: HDC, f: &Fonts, dpi: u32, r: RECT, c: &Cell, hist: Option<&VecDeque<(f64, f64)>>, now: f64) {
    let s = |v: i32| v * dpi as i32 / 96;
    fill_round(dc, r, s(12), CARD);
    let (ix, rx) = (r.left + s(14), r.right - s(14));
    text(dc, f.mid, ix, r.top + s(10), &c.label, LABEL);
    match &c.quote {
        None => {
            text(dc, f.big, ix, r.top + s(30), "--", DIM);
            text(dc, f.small, ix, r.top + s(68), "no price from any source", DIM);
        }
        Some(q) => {
            let src = if q.realtime { "CNBC" } else { "CNBC delayed" };
            let tag = match q.ts {
                Some(t) => format!("{src} \u{b7} {}", fmt_age(now - t)),
                None => src.to_string(),
            };
            text_right(dc, f.small, rx, r.top + s(13), &tag, if c.stale { AMBER } else { DIM });
            text(dc, f.big, ix, r.top + s(30), &fmt_price(q.price, c.digits), if c.stale { DIM } else { PRICE });
            match q.change_pct {
                Some(p) => {
                    let col = if c.stale { DIM } else if p >= 0.0 { UP } else { DOWN };
                    let (open, abs) = match q.prev_close {
                        Some(prev) => (prev, q.price - prev),
                        None => day_open(q.price, p),
                    };
                    let abs_s = format!("{}{}", if abs >= 0.0 { "+" } else { "-" }, fmt_price(abs.abs(), c.digits));
                    text(dc, f.mid, ix, r.top + s(66), &format!("{}   {}", fmt_change(p), abs_s), col);
                    text_right(dc, f.small, rx, r.top + s(68), &format!("prev {}", fmt_price(open, c.digits)), DIM);
                }
                None => {
                    text(dc, f.small, ix, r.top + s(68), "no day change", DIM);
                }
            }
        }
    }
    spark(dc, f, RECT { left: ix, top: r.top + s(94), right: rx, bottom: r.bottom - s(12) }, hist, dpi, now);
}

/// Paint the whole panel into `dc` (w × h is the panel's size from `geometry`).
#[allow(clippy::too_many_arguments)]
pub fn draw(dc: HDC, w: i32, h: i32, board: &Board, hist: &History, f: &Fonts, dpi: u32, hover: bool, now: f64) {
    let s = |v: i32| v * dpi as i32 / 96;
    let g = geometry(board.cells.len(), dpi);
    unsafe {
        fill_rect(dc, RECT { left: 0, top: 0, right: w, bottom: h }, BG);
        SetBkMode(dc, TRANSPARENT as _);
    }
    // header: title, link dot + source, minimise button
    let ty = s(15);
    let tw = text(dc, f.mid, g.pad + s(2), ty, "MH Ticker", PRICE);
    let lh = line_height(dc, f.mid);
    let d = s(8);
    let dx = g.pad + s(2) + tw + s(14);
    fill_ellipse(dc, dx, ty + (lh - d) / 2, d, link_color(board.link));
    let sh = line_height(dc, f.small);
    text(dc, f.small, dx + d + s(7), ty + (lh - sh) / 2, link_name(board.link), LABEL);
    if hover {
        fill_round(dc, g.btn, s(8), BTN_HOVER);
    }
    let (cx, cy) = ((g.btn.left + g.btn.right) / 2, (g.btn.top + g.btn.bottom) / 2);
    let (bar, thick) = (s(12), s(2).max(2));
    fill_rect(dc, RECT { left: cx - bar / 2, top: cy - thick / 2, right: cx + bar / 2, bottom: cy - thick / 2 + thick }, if hover { PRICE } else { LABEL });

    for (i, c) in board.cells.iter().enumerate() {
        let (col, row) = ((i % g.cols) as i32, (i / g.cols) as i32);
        let x = g.pad + col * (g.card_w + g.gap);
        let y = g.header + row * (g.card_h + g.gap);
        card(dc, f, dpi, RECT { left: x, top: y, right: x + g.card_w, bottom: y + g.card_h }, c, hist.row(i), now);
    }

    let fy = h - s(27);
    let note: String = board.note.chars().take(70).collect();
    let lines = if hist.backfilled { "lines: last hour (1-min bars, then live)" } else { "lines: last hour, filling live" };
    let left = if note.is_empty() { lines.to_string() } else { format!("{note}   \u{b7}   {lines}") };
    text(dc, f.small, g.pad + s(2), fy, &left, DIM);
    text_right(dc, f.small, w - g.pad - s(2), fy, "Esc or \u{2014} to dock", DIM);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_open_and_age() {
        let (open, abs) = day_open(101.0, 1.0);
        assert!((open - 100.0).abs() < 1e-9 && (abs - 1.0).abs() < 1e-9);
        assert_eq!(fmt_age(4.7), "4s");
        assert_eq!(fmt_age(125.0), "2m");
        assert_eq!(fmt_age(7300.0), "2h");
    }

    #[test]
    fn history_seeds_from_bars_keeps_live_points_and_rolls_an_hour() {
        let cell = Cell { label: "XAU".into(), digits: 2, quote: None, stale: true };
        let mut b = Board::new(&[]);
        b.cells = vec![cell];
        let now = 10_000.0;
        // bars every 60 s over the past hour; one live point already at now - 100
        let mut h = History::new();
        h.labels = vec!["XAU".into()];
        h.rows = vec![VecDeque::from(vec![(now - 100.0, 9.0)])];
        b.backfill = vec![(0..61).map(|i| (now - 3600.0 + 60.0 * i as f64, i as f64)).collect()];
        b.backfill_gen = 1;
        h.record(&b, now);
        let row = h.row(0).unwrap();
        assert!(row.iter().any(|p| p.1 == 9.0));
        // the bar closing at now-120 is 20 s from the live point at now-100, so it is skipped
        assert!(!row.iter().any(|p| (p.0 - (now - 120.0)).abs() < 1e-9));
        assert!(row.iter().zip(row.iter().skip(1)).all(|(a, b)| a.0 <= b.0));
        assert!(h.backfilled);
        // an hour later everything has rolled out
        h.record(&b, now + 3700.0);
        assert!(h.row(0).unwrap().iter().all(|p| p.0 >= now + 100.0));
    }

    #[test]
    fn geometry_fits_six_cards_in_two_rows() {
        let g = geometry(6, 96);
        assert_eq!(g.w, 2 * 16 + 3 * 224 + 2 * 10);
        assert_eq!(g.h, 48 + 2 * 132 + 10 + 36);
        assert!(g.btn.right < g.w && g.btn.top > 0);
        assert_eq!(geometry(2, 96).w, 2 * 16 + 2 * 224 + 10);
    }

    #[test]
    fn history_follows_labels_when_rows_change() {
        let rows = crate::config::default_rows();
        let mut b = Board::new(&rows[..2]);
        let mut h = History::new();
        b.cells[1].quote = Some(crate::model::Quote {
            symbol: "XAG=".into(),
            name: String::new(),
            price: 56.0,
            change_pct: None,
            prev_close: None,
            ts: Some(100.0),
            realtime: true,
        });
        b.cells[1].stale = false;
        h.record(&b, 100.0);
        assert_eq!(h.row(1).unwrap().len(), 1);
        // XAU dropped, BTC added: XAG moves to index 0 with its line, BTC starts empty
        b.set_rows(&[rows[1].clone(), rows[2].clone()]);
        h.align(&b);
        assert_eq!(h.labels, ["XAG", "BTC"]);
        assert_eq!(h.row(0).unwrap().len(), 1);
        assert!(h.row(1).unwrap().is_empty());
    }
}
