//! The expanded panel: one card per symbol (price, day change, day open, source and age, and a
//! sparkline of the last hour), plus — in the Ultra build — a symbol picker strip.
//! Drawn with GDI into whatever DC `ui.rs` hands it.
//!
//! The sparkline is a rolling one-hour window: seeded from CNBC's 1-minute bars (so a fresh launch
//! already shows the past hour), then extended by live samples, and scrolling left as time passes.
//!
//! Two things this module owns that used to be split across it and `ui.rs`:
//! - **The palette.** Colours come from `theme::Theme`, so a theme change is a data change and not
//!   an edit in two files.
//! - **The geometry.** `Geo` is the single description of where every card, chip and button sits;
//!   `draw` and the mouse hit tests both read it, so a click can never disagree with the pixels.
//!
//! The line colour is the *day's* direction, not the rolling hour's: a symbol up 1.2 % on the day
//! draws green even while the last hour ticks down. That is the same baseline the card's change
//! text uses (`day_open`), so the number and the line can no longer disagree.

use crate::config::Row;
use crate::model::{fmt_change, fmt_price, Board, Cell};
use crate::theme::{Gradient, Theme};

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
/// Symbol-picker chips per row.
const CHIP_COLS: usize = 4;

pub struct Fonts {
    big: HFONT,
    mid: HFONT,
    small: HFONT,
}

fn font(face: &[u16], pt: i32, weight: u32, dpi: u32, pixel: bool) -> HFONT {
    // A bitmap face is only crisp without antialiasing: ClearType smears a raster glyph into a
    // grey fringe and the "pixel" theme stops looking like one. Everything else keeps ClearType.
    let (quality, pitch) = if pixel {
        (NONANTIALIASED_QUALITY as u32, FF_MODERN | FIXED_PITCH)
    } else {
        (CLEARTYPE_QUALITY as u32, DEFAULT_PITCH | FF_DONTCARE)
    };
    unsafe {
        CreateFontW(
            -(pt * dpi as i32 / 72), 0, 0, 0, weight as _, 0, 0, 0, DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _,
            CLIP_DEFAULT_PRECIS as _, quality as _, pitch as _, face.as_ptr(),
        )
    }
}

impl Fonts {
    pub fn new(face: &str, dpi: u32, pixel: bool) -> Self {
        let f: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
        Self {
            big: font(&f, if pixel { 16 } else { 18 }, FW_SEMIBOLD, dpi, pixel),
            mid: font(&f, if pixel { 10 } else { 11 }, FW_SEMIBOLD, dpi, pixel),
            small: font(&f, if pixel { 8 } else { 9 }, FW_NORMAL, dpi, pixel),
        }
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

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
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

    /// The sample a crosshair at time `t` points at: the newest point at or before `t`, or `None`
    /// when `t` falls before the series starts (the left edge of a freshly seeded chart).
    pub fn sample(&self, i: usize, t: f64) -> Option<(f64, f64)> {
        let row = self.rows.get(i)?;
        match row.binary_search_by(|p| p.0.total_cmp(&t)) {
            Ok(k) => row.get(k).copied(),
            // `k` is the first point after `t`, so the answer is the one before it.
            Err(0) => None,
            Err(k) => row.get(k - 1).copied(),
        }
    }
}

/// Where the pointer is on the chart: which card, and at what time (UTC seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hover {
    pub row: usize,
    pub t: f64,
}

/// The catalogue, and which of its entries are currently on the strip.
pub struct Picker<'a> {
    pub list: &'a [Row],
    pub shown: &'a [bool],
}

pub struct Geo {
    pub w: i32,
    pub h: i32,
    pub header: i32,
    pub btn: RECT,
    /// The symbol-picker toggle, beside the minimise button.
    pub pick_btn: RECT,
    cols: usize,
    pad: i32,
    gap: i32,
    card_w: i32,
    card_h: i32,
    /// Top of the picker strip = the bottom of the last card row. The picker draws from here down
    /// to `h - footer`, so `draw` never has to re-derive it.
    pick_top: i32,
    /// One rect per catalogue entry, in `config::menu_list` order.
    pub chips: Vec<RECT>,
}

pub fn geometry(n: usize, catalog: usize, dpi: u32, picker: bool) -> Geo {
    let s = |v: i32| v * dpi as i32 / 96;
    let cols = n.clamp(1, COLS);
    let rows = n.div_ceil(COLS).max(1) as i32;
    // 150 tall, not 132: the day-range bar and the lo/hi + 52-week line need the extra strip, and
    // squeezing them into 132 would overlap the last hour's line. 236 wide, not 224: the range, the
    // 52-week badge and its date are three numbers on one 9 pt line.
    let (pad, gap, card_w, card_h, header) = (s(16), s(10), s(236), s(150), s(48));
    let w = 2 * pad + cols as i32 * card_w + (cols as i32 - 1) * gap;
    let cards_h = header + rows * card_h + (rows - 1) * gap;
    let (chip_h, chip_gap) = (s(26), s(6));
    let chip_rows = if picker { catalog.div_ceil(CHIP_COLS).max(1) as i32 } else { 0 };
    let pick_h = if picker { s(22) + chip_rows * (chip_h + chip_gap) } else { 0 };
    // Two header buttons: the picker toggle then the minimise cross, both the same size.
    let (bw, bh, bx) = (s(30), s(30), w - s(12));
    let btn = RECT { left: bx - bw, top: s(10), right: bx, bottom: s(10) + bh };
    let pick_btn = RECT { left: btn.left - s(6) - bw, top: s(10), right: btn.left - s(6), bottom: s(10) + bh };
    let mut chips = Vec::new();
    if picker {
        let chip_w = (w - 2 * pad - (CHIP_COLS as i32 - 1) * chip_gap) / CHIP_COLS as i32;
        for i in 0..catalog {
            let (col, row) = ((i % CHIP_COLS) as i32, (i / CHIP_COLS) as i32);
            let x = pad + col * (chip_w + chip_gap);
            let y = cards_h + s(22) + row * (chip_h + chip_gap);
            chips.push(RECT { left: x, top: y, right: x + chip_w, bottom: y + chip_h });
        }
    }
    let h = cards_h + pick_h + s(36);
    Geo { w, h, header, btn, pick_btn, cols, pad, gap, card_w, card_h, pick_top: cards_h, chips }
}

pub fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// A card's rect, given its index. `draw` and the hit tests share this, so a click lands on the
/// card the pointer is over.
pub fn card_rect(g: &Geo, i: usize) -> RECT {
    let (col, row) = ((i % g.cols) as i32, (i / g.cols) as i32);
    let x = g.pad + col * (g.card_w + g.gap);
    let y = g.header + row * (g.card_h + g.gap);
    RECT { left: x, top: y, right: x + g.card_w, bottom: y + g.card_h }
}

/// The sparkline's plot area inside a card. The bottom leaves room for the day-range bar and the
/// lo/hi line under it, so the line and its numbers never overlap.
fn spark_rect(card: RECT, dpi: u32) -> RECT {
    let s = |v: i32| v * dpi as i32 / 96;
    RECT { left: card.left + s(14), top: card.top + s(86), right: card.right - s(14), bottom: card.bottom - s(38) }
}

/// The card (and time) under a pointer, if it is over a sparkline.
pub fn hit(g: &Geo, n: usize, dpi: u32, x: i32, y: i32, now: f64) -> Option<Hover> {
    (0..n).find_map(|i| {
        let r = spark_rect(card_rect(g, i), dpi);
        if !in_rect(&r, x, y) {
            return None;
        }
        let span = (r.right - r.left).max(1) as f64;
        let f = ((x - r.left) as f64 / span).clamp(0.0, 1.0);
        Some(Hover { row: i, t: now - WINDOW_S + f * WINDOW_S })
    })
}

/// Which catalogue entry a pointer is over, when the picker is open.
pub fn pick_hit(g: &Geo, x: i32, y: i32) -> Option<usize> {
    g.chips.iter().position(|r| in_rect(r, x, y))
}

/// Which card a pointer is over. The card is the click target (it opens the symbol on CNBC), and
/// the chips sit below the cards, so the two never overlap.
pub fn card_at(g: &Geo, n: usize, x: i32, y: i32) -> Option<usize> {
    (0..n).find(|i| in_rect(&card_rect(g, *i), x, y))
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

fn text_width(dc: HDC, font: HFONT, s: &str) -> i32 {
    unsafe {
        let old = SelectObject(dc, font);
        let t = utf16(s);
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
        SelectObject(dc, old);
        sz.cx
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

/// The price the day is measured from: the previous close when CNBC sends one, else the open
/// implied by the change %. `None` when the quote has neither, so the line falls back to neutral.
pub fn day_baseline(c: &Cell) -> Option<f64> {
    let q = c.quote.as_ref()?;
    match q.prev_close {
        Some(p) if p > 0.0 => Some(p),
        _ => q.change_pct.filter(|p| *p > -99.0).map(|p| day_open(q.price, p).0),
    }
}

/// What the panel needs to draw one frame. Bundled so `draw` has one argument that can grow
/// without every caller changing.
pub struct Scene<'a> {
    pub title: &'a str,
    pub board: &'a Board,
    pub hist: &'a History,
    pub fonts: &'a Fonts,
    pub theme: &'a Theme,
    pub grad: &'a Gradient,
    pub dpi: u32,
    pub now: f64,
    /// The minimise button is under the pointer.
    pub btn_hover: bool,
    /// The picker toggle is under the pointer.
    pub pick_hover: bool,
    /// The pointer is on a chart.
    pub hover: Option<Hover>,
    /// `Some` when the picker is open. `None` also hides the toggle: the classic build has no
    /// picker, so it must not draw a dead button.
    pub picker: Option<Picker<'a>>,
    /// This build supports a picker at all.
    pub picker_btn: bool,
    /// The last alert outcome. Replaces the footer hint, because Focus Assist can swallow the
    /// tray balloon and the panel is then the only place the alert is visible.
    pub alert: Option<&'a str>,
    /// Position in an animated background's cycle, 0..1. Ignored by every other theme, so the caller
    /// can always pass a clock.
    pub phase: f64,
}

/// The last hour on a fixed time axis (now - 1 h .. now), so the line scrolls left as time passes.
/// `baseline` is the day's reference price: the line and the guide are drawn against it, so the
/// sparkline agrees with the change % printed above it.
fn spark(dc: HDC, sc: &Scene, r: RECT, i: usize, pts: Option<&VecDeque<(f64, f64)>>, baseline: Option<f64>) {
    let t = sc.theme;
    let dpi = sc.dpi;
    let s = |v: i32| v * dpi as i32 / 96;
    let pts = match pts {
        Some(p) if p.len() >= 2 => p,
        _ => {
            text(dc, sc.fonts.small, r.left, r.top + s(4), "collecting\u{2026}", t.dim);
            return;
        }
    };
    let now = sc.now;
    let (t0, tspan) = (now - WINDOW_S, WINDOW_S);
    let (mut lo, mut hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    // Keep the day's baseline inside the plot, or a flat day would draw the guide on the edge.
    if let Some(b) = baseline {
        lo = lo.min(b);
        hi = hi.max(b);
    }
    let (w, h) = ((r.right - r.left) as f64, (r.bottom - r.top) as f64);
    let span = hi - lo;
    let y_of = |v: f64| if span > 0.0 { r.bottom as f64 - (v - lo) / span * h } else { r.top as f64 + h / 2.0 };
    let x_of = |time: f64| r.left + ((time - t0) / tspan * w).clamp(0.0, w).round() as i32;
    let line: Vec<POINT> =
        pts.iter().map(|&(time, v)| POINT { x: x_of(time), y: y_of(v).round() as i32 }).collect();
    let last = pts.back().unwrap().1;
    // The day's direction, not the rolling hour's.
    let direction = match baseline {
        Some(b) => last >= b,
        None => last >= pts.front().unwrap().1,
    };
    let color = if direction { t.up } else { t.down };
    // The session's extremes, as short dashes where the line actually turned. Cheap (two more
    // fills) and it answers "was that the high of the hour?" without a crosshair.
    let (mut hi_i, mut lo_i) = (0usize, 0usize);
    for (k, p) in pts.iter().enumerate() {
        if p.1 > pts[hi_i].1 {
            hi_i = k;
        }
        if p.1 < pts[lo_i].1 {
            lo_i = k;
        }
    }
    unsafe {
        if let Some(b) = baseline {
            let gy = y_of(b).round() as i32;
            fill_rect(dc, RECT { left: r.left, top: gy, right: r.right, bottom: gy + 1 }, t.guide);
            // The day-open tick, so the guide has a value attached to it at the left edge.
            fill_rect(dc, RECT { left: r.left, top: gy - s(1), right: r.left + s(6), bottom: gy + s(2) }, t.dim);
        }
        let dash = s(7).max(3);
        for k in [hi_i, lo_i] {
            if pts[hi_i].1 == pts[lo_i].1 {
                break;
            }
            let p = line[k];
            let x0 = (p.x - dash / 2).clamp(r.left, (r.right - dash).max(r.left));
            fill_rect(dc, RECT { left: x0, top: p.y, right: x0 + dash, bottom: p.y + s(2).max(1) }, t.dim);
        }
        let pen = CreatePen(PS_SOLID, if t.is_pixel() { 1 } else { s(2).max(1) }, color);
        let old = SelectObject(dc, pen);
        Polyline(dc, line.as_ptr(), line.len() as i32);
        SelectObject(dc, old);
        DeleteObject(pen);
        let d = s(5);
        let end = line.last().unwrap();
        fill_ellipse(dc, end.x - d / 2, end.y - d / 2, d, color);
    }
    // Crosshair: only on the card the pointer is on, and only if there is a sample to name.
    if let Some(hv) = sc.hover.filter(|hv| hv.row == i) {
        if let Some((time, price)) = sc.hist.sample(i, hv.t) {
            let hx = x_of(time);
            unsafe {
                // A dotted pen through a two-point Polyline: no MoveToEx/LineTo pair to get wrong.
                let pen = CreatePen(PS_DOT, 1, t.accent);
                let old = SelectObject(dc, pen);
                let seg = [POINT { x: hx, y: r.top }, POINT { x: hx, y: r.bottom }];
                Polyline(dc, seg.as_ptr(), 2);
                SelectObject(dc, old);
                DeleteObject(pen);
            }
            let dot = s(5);
            fill_ellipse(dc, hx - dot / 2, y_of(price).round() as i32 - dot / 2, dot, t.accent);
            let label = format!("-{}  {}", fmt_age(now - time), fmt_price(price, 2));
            let (lw, lh) = (text_width(dc, sc.fonts.small, &label), line_height(dc, sc.fonts.small));
            let box_w = lw + s(10);
            let bx = (hx - box_w / 2).clamp(r.left, (r.right - box_w).max(r.left));
            let plate = RECT { left: bx, top: r.top, right: bx + box_w, bottom: r.top + lh + s(4) };
            fill_round(dc, plate, s(4), t.card);
            text(dc, sc.fonts.small, plate.left + s(5), plate.top + s(2), &label, t.accent);
        }
    }
}

/// `128400` -> "128.4K", `1.2e9` -> "1.2B". A trailing `.0` is trimmed: "1M", not "1.0M".
pub fn fmt_vol(v: f64) -> String {
    let (n, unit) = if v >= 1e9 {
        (v / 1e9, "B")
    } else if v >= 1e6 {
        (v / 1e6, "M")
    } else if v >= 1e3 {
        (v / 1e3, "K")
    } else {
        (v, "")
    };
    let s = if unit.is_empty() { format!("{n:.0}") } else { format!("{n:.1}") };
    format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
}

/// `2026-09-22` or `09/22/2026` -> "22 Sep". Anything else is dropped rather than shown raw: the
/// card has room for a date — not for a format string.
fn short_date(s: &str) -> Option<String> {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = if let Some((y, rest)) = s.split_once('-') {
        let mut r = rest.split('-');
        (y, r.next()?, r.next()?)
    } else {
        let mut r = s.split('/');
        let (m, d, y) = (r.next()?, r.next()?, r.next()?);
        (y, m, d)
    };
    let m: usize = m.parse().ok()?;
    let d: u32 = d.get(0..2).and_then(|d| d.parse().ok())?;
    let _ = y;
    Some(format!("{d} {}", MON.get(m.checked_sub(1)?)?))
}

fn card(dc: HDC, sc: &Scene, r: RECT, i: usize, c: &Cell) {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    fill_round(dc, r, if t.is_pixel() { 0 } else { s(12) }, t.card);
    let (ix, rx) = (r.left + s(14), r.right - s(14));
    text(dc, sc.fonts.mid, ix, r.top + s(8), &c.label, t.label);
    match &c.quote {
        None => {
            text(dc, sc.fonts.big, ix, r.top + s(28), "--", t.dim);
            text(dc, sc.fonts.small, ix, r.top + s(64), "no price from any source", t.dim);
        }
        Some(q) => {
            // Header: source, age, and volume when CNBC sends one and it still fits beside the
            // label. Volume is the first thing to go, because it is the least load-bearing number
            // on the card and "CNBC delayed" already uses the width.
            let src = if q.realtime { "CNBC" } else { "CNBC delayed" };
            let tag = match q.ts {
                Some(ts) => format!("{src} \u{b7} {}", fmt_age(sc.now - ts)),
                None => src.to_string(),
            };
            let room = rx - ix - text_width(dc, sc.fonts.mid, &c.label) - s(10);
            let tag = match q.volume {
                Some(v) => {
                    let with = format!("{tag} \u{b7} {}", fmt_vol(v));
                    if text_width(dc, sc.fonts.small, &with) <= room { with } else { tag }
                }
                None => tag,
            };
            text_right(dc, sc.fonts.small, rx, r.top + s(10), &tag, if c.stale { t.amber } else { t.dim });
            let pw = text(dc, sc.fonts.big, ix, r.top + s(26), &fmt_price(q.price, c.digits), if c.stale { t.dim } else { t.price });
            if !q.currency.is_empty() {
                text(dc, sc.fonts.small, ix + pw + s(5), r.top + s(40), &q.currency, t.dim);
            }
            match q.change_pct {
                Some(p) => {
                    let col = if c.stale { t.dim } else if p >= 0.0 { t.up } else { t.down };
                    let (open, abs) = match q.prev_close {
                        Some(prev) => (prev, q.price - prev),
                        None => day_open(q.price, p),
                    };
                    let abs_s = format!("{}{}", if abs >= 0.0 { "+" } else { "-" }, fmt_price(abs.abs(), c.digits));
                    let change = format!("{}   {abs_s}", fmt_change(p));
                    text(dc, sc.fonts.mid, ix, r.top + s(62), &change, col);
                    // The previous close is right-aligned on the same line, and dropped when the
                    // change text would run into it: a clipped "rev 29,638" is worse than no prev.
                    let prev = format!("prev {}", fmt_price(open, c.digits));
                    let used = text_width(dc, sc.fonts.mid, &change);
                    if text_width(dc, sc.fonts.small, &prev) + used + s(8) <= rx - ix {
                        text_right(dc, sc.fonts.small, rx, r.top + s(64), &prev, t.dim);
                    }
                }
                None => {
                    text(dc, sc.fonts.small, ix, r.top + s(64), "no day change", t.dim);
                }
            }
            // Day range: a track from the session low to the high with the price as a marker, so
            // "where in today's range is this?" is one glance instead of two numbers.
            let bar_y = r.bottom - s(34);
            let mut left_w = 0;
            if let (Some(lo), Some(hi)) = (q.low, q.high) {
                // No "L"/"H" letters: the bar directly above is the legend, and the two words cost
                // 20 px of the 208 a card has.
                let label = format!("{}\u{2013}{}", fmt_price(lo, c.digits), fmt_price(hi, c.digits));
                text(dc, sc.fonts.small, ix, r.bottom - s(26), &label, t.dim);
                left_w = text_width(dc, sc.fonts.small, &label);
                if hi > lo {
                    let thick = s(4).max(2);
                    fill_rect(dc, RECT { left: ix, top: bar_y, right: rx, bottom: bar_y + thick }, t.guide);
                    let f = ((q.price - lo) / (hi - lo)).clamp(0.0, 1.0);
                    let mx = ix + ((rx - ix) as f64 * f).round() as i32;
                    let mw = s(3).max(2);
                    let x0 = (mx - mw / 2).clamp(ix, (rx - mw).max(ix));
                    let col = if c.stale { t.dim } else if q.price >= lo + (hi - lo) / 2.0 { t.up } else { t.down };
                    fill_rect(
                        dc,
                        RECT { left: x0, top: bar_y - s(2), right: x0 + mw, bottom: bar_y + thick + s(2) },
                        col,
                    );
                }
            }
            // 52-week range, right-aligned on the same line, with the date of the high when it
            // fits. Dropping the date is better than overlapping the lo/hi text.
            if let (Some(lo), Some(hi)) = (q.yr_lo, q.yr_hi) {
                // Whole units: a 52-week range is context, not a price to trade from, and the cents
                // are what push the date off the line.
                let base = format!("52w {}\u{2013}{}", fmt_price(lo, 0), fmt_price(hi, 0));
                let dated = q.yr_hi_date.as_deref().and_then(short_date).map(|d| format!("{base} \u{b7} {d}"));
                let room = (rx - ix) - left_w - s(6);
                let fits = |text: &str| text_width(dc, sc.fonts.small, text) <= room;
                let chosen = dated.filter(|d| fits(d)).or_else(|| fits(&base).then_some(base));
                if let Some(text) = chosen {
                    text_right(dc, sc.fonts.small, rx, r.bottom - s(26), &text, t.dim);
                }
            }
        }
    }
    spark(dc, sc, spark_rect(r, sc.dpi), i, sc.hist.row(i), day_baseline(c));
}

/// The catalogue as clickable chips: ticked entries are on the strip, the rest are one click away.
fn picker(dc: HDC, sc: &Scene, g: &Geo, p: &Picker) {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    let shown = p.shown.iter().filter(|v| **v).count();
    let head = format!("Symbols \u{b7} {shown} of {} on the strip", p.list.len());
    text(dc, sc.fonts.small, g.pad + s(2), g.pick_top + s(4), &head, t.label);
    for (i, r) in g.chips.iter().enumerate() {
        let Some(row) = p.list.get(i) else { continue };
        let on = p.shown.get(i).copied().unwrap_or(false);
        fill_round(dc, *r, if t.is_pixel() { 0 } else { s(8) }, if on { t.btn } else { t.card });
        let label = &row.label;
        let (tw, th) = (text_width(dc, sc.fonts.small, label), line_height(dc, sc.fonts.small));
        text(
            dc,
            sc.fonts.small,
            r.left + (r.right - r.left - tw) / 2,
            r.top + (r.bottom - r.top - th) / 2,
            label,
            if on { t.accent } else { t.dim },
        );
        if on {
            let uy = r.bottom - s(4);
            fill_rect(dc, RECT { left: r.left + s(6), top: uy, right: r.right - s(6), bottom: uy + s(2) }, t.accent);
        }
    }
}

/// Paint the whole panel into `dc` (w × h is the panel's size from `geometry`).
pub fn draw(dc: HDC, w: i32, h: i32, sc: &Scene) {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    let radius = |v: i32| if t.is_pixel() { 0 } else { s(v) };
    let g = geometry(sc.board.cells.len(), sc.picker.as_ref().map(|p| p.list.len()).unwrap_or(0), sc.dpi, sc.picker.is_some());
    unsafe {
        SetBkMode(dc, TRANSPARENT as _);
    }
    sc.grad.paint(dc, RECT { left: 0, top: 0, right: w, bottom: h }, sc.phase);
    // header: title, link dot + source, symbol-picker toggle, minimise button
    let ty = s(15);
    let tw = text(dc, sc.fonts.mid, g.pad + s(2), ty, sc.title, t.price);
    let lh = line_height(dc, sc.fonts.mid);
    let d = s(8);
    let dx = g.pad + s(2) + tw + s(14);
    fill_ellipse(dc, dx, ty + (lh - d) / 2, d, crate::theme::link_color(t, sc.board.link));
    let sh = line_height(dc, sc.fonts.small);
    text(dc, sc.fonts.small, dx + d + s(7), ty + (lh - sh) / 2, crate::theme::link_name(sc.board.link), t.label);

    if sc.picker_btn {
        if sc.pick_hover {
            fill_round(dc, g.pick_btn, radius(8), t.btn);
        }
        // three stacked bars: the catalogue
        let (px, py) = ((g.pick_btn.left + g.pick_btn.right) / 2, (g.pick_btn.top + g.pick_btn.bottom) / 2);
        for k in -1..=1 {
            let (bw, bh) = (s(12), s(2).max(2));
            let by = py + k * s(6) - bh / 2;
            fill_rect(dc, RECT { left: px - bw / 2, top: by, right: px + bw / 2, bottom: by + bh }, if sc.pick_hover { t.price } else { t.label });
        }
    }
    if sc.btn_hover {
        fill_round(dc, g.btn, radius(8), t.btn);
    }
    let (cx, cy) = ((g.btn.left + g.btn.right) / 2, (g.btn.top + g.btn.bottom) / 2);
    let (bar, thick) = (s(12), s(2).max(2));
    fill_rect(dc, RECT { left: cx - bar / 2, top: cy - thick / 2, right: cx + bar / 2, bottom: cy - thick / 2 + thick }, if sc.btn_hover { t.price } else { t.label });

    for (i, c) in sc.board.cells.iter().enumerate() {
        card(dc, sc, card_rect(&g, i), i, c);
    }
    if let Some(p) = &sc.picker {
        picker(dc, sc, &g, p);
    }

    let fy = h - s(27);
    let note: String = sc.board.note.chars().take(70).collect();
    let lines = if sc.hist.backfilled { "lines: last hour (1-min bars, then live)" } else { "lines: last hour, filling live" };
    // An alert outranks the hint about how the lines are built.
    let left: String = match (note.is_empty(), sc.alert) {
        (true, Some(a)) => format!("{a}   \u{b7}   {lines}"),
        (true, None) => lines.to_string(),
        (false, Some(a)) => format!("{note}   \u{b7}   {a}"),
        (false, None) => format!("{note}   \u{b7}   {lines}"),
    };
    let left: String = left.chars().take(70).collect();
    text(dc, sc.fonts.small, g.pad + s(2), fy, &left, t.dim);
    text_right(dc, sc.fonts.small, w - g.pad - s(2), fy, "click a card: CNBC \u{b7} Esc: dock", t.dim);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(label: &str, price: f64, pct: Option<f64>) -> Cell {
        Cell {
            label: label.into(),
            digits: 2,
            quote: Some(crate::model::Quote {
                symbol: label.into(),
                price,
                change_pct: pct,
                ts: Some(100.0),
                realtime: true,
                ..Default::default()
            }),
            stale: false,
        }
    }

    #[test]
    fn the_card_is_tall_enough_for_its_own_line() {
        // The card grew for the range bar and the lo/hi + 52-week row; the sparkline must still
        // sit above them with a gap, or the last hour's line runs through the numbers.
        let g = geometry(3, 0, 96, false);
        let card = card_rect(&g, 0);
        let spark = spark_rect(card, 96);
        assert!(spark.top > card.top + 70, "price and change rows come first");
        let labels_top = card.bottom - 26;
        assert!(spark.bottom <= labels_top - 6, "spark {} overlaps labels {labels_top}", spark.bottom);
        assert!(spark.bottom > spark.top, "the plot has height");
    }

    #[test]
    fn volumes_and_dates_fit_on_a_card() {
        assert_eq!(fmt_vol(128_400.0), "128.4K");
        assert_eq!(fmt_vol(1_000_000.0), "1M");
        assert_eq!(fmt_vol(2_640_000_000.0), "2.6B");
        assert_eq!(fmt_vol(842.0), "842");
        assert_eq!(short_date("2026-09-22").as_deref(), Some("22 Sep"));
        assert_eq!(short_date("09/22/2026").as_deref(), Some("22 Sep"));
        assert_eq!(short_date("yesterday"), None, "an unknown format is dropped, not shown raw");
        assert_eq!(short_date("2026-13-01"), None);
    }

    #[test]
    fn day_open_and_age() {
        let (open, abs) = day_open(101.0, 1.0);
        assert!((open - 100.0).abs() < 1e-9 && (abs - 1.0).abs() < 1e-9);
        assert_eq!(fmt_age(4.7), "4s");
        assert_eq!(fmt_age(125.0), "2m");
        assert_eq!(fmt_age(7300.0), "2h");
    }

    #[test]
    fn line_direction_follows_the_day_not_the_hour() {
        // Up 1 % on the day, so the baseline sits below the price: the line is a rise even if the
        // last hour ticked down from a high.
        let up = cell("XAU", 4155.0, Some(1.0));
        let base = day_baseline(&up).unwrap();
        assert!(base < 4155.0, "baseline {base} should sit under the price");
        // A previous close wins over the implied open.
        let mut q = up.clone();
        q.quote.as_mut().unwrap().prev_close = Some(4100.0);
        assert_eq!(day_baseline(&q), Some(4100.0));
        // No day change at all: no baseline, so the spark falls back to its own first sample.
        assert_eq!(day_baseline(&cell("X", 1.0, None)), None);
        // A nonsensical -100 % change must not divide by zero.
        assert_eq!(day_baseline(&cell("X", 1.0, Some(-100.0))), None);
    }

    #[test]
    fn history_seeds_from_bars_keeps_live_points_and_rolls_an_hour() {
        let c = Cell { label: "XAU".into(), digits: 2, quote: None, stale: true };
        let mut b = Board::new(&[]);
        b.cells = vec![c];
        let now = 10_000.0;
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
    fn sample_answers_the_newest_point_at_or_before_the_crosshair() {
        let mut h = History::new();
        h.labels = vec!["X".into()];
        h.rows = vec![VecDeque::from(vec![(10.0, 1.0), (20.0, 2.0), (30.0, 3.0)])];
        assert_eq!(h.sample(0, 25.0), Some((20.0, 2.0)));
        assert_eq!(h.sample(0, 20.0), Some((20.0, 2.0)));
        assert_eq!(h.sample(0, 5.0), None, "before the first sample");
        assert_eq!(h.sample(0, 99.0), Some((30.0, 3.0)));
        assert_eq!(h.sample(9, 99.0), None);
    }

    #[test]
    fn geometry_fits_six_cards_in_two_rows() {
        let g = geometry(6, 0, 96, false);
        assert_eq!(g.w, 2 * 16 + 3 * 236 + 2 * 10);
        assert_eq!(g.h, 48 + 2 * 150 + 10 + 36);
        assert_eq!(g.pick_top, 48 + 2 * 150 + 10, "no picker, so the cards end the panel");
        assert!(g.chips.is_empty());
        assert!(g.btn.right < g.w && g.btn.top > 0);
        assert!(g.pick_btn.right < g.btn.left, "the picker toggle sits left of the cross");
        assert_eq!(geometry(2, 0, 96, false).w, 2 * 16 + 2 * 236 + 10);
    }

    #[test]
    fn picker_adds_a_strip_of_chips_without_moving_the_cards() {
        let closed = geometry(6, 23, 96, false);
        let open = geometry(6, 23, 96, true);
        assert_eq!(open.w, closed.w);
        assert!(open.h > closed.h);
        assert_eq!(open.chips.len(), 23);
        let corners = |r: RECT| (r.left, r.top, r.right, r.bottom);
        assert_eq!(corners(card_rect(&open, 5)), corners(card_rect(&closed, 5)), "cards stay put");
        for c in &open.chips {
            assert!(c.bottom <= open.h, "a chip must not fall off the panel");
            assert!(c.right <= open.w);
        }
        // the last chip of the first row is inside the panel width
        assert!(open.chips[CHIP_COLS - 1].right <= open.w - open.pad + 1);
    }

    #[test]
    fn cards_and_chips_are_separate_click_targets() {
        let g = geometry(6, 8, 96, true);
        let card = card_rect(&g, 2);
        assert_eq!(card_at(&g, 6, (card.left + card.right) / 2, (card.top + card.bottom) / 2), Some(2));
        // The gap between cards belongs to no card, and neither does the header or the chips.
        assert_eq!(card_at(&g, 6, card.right + 3, card.top + 10), None);
        assert_eq!(card_at(&g, 6, 40, 12), None);
        let chip = g.chips[0];
        assert_eq!(card_at(&g, 6, (chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2), None);
    }

    #[test]
    fn hit_testing_lands_on_the_card_and_chip_under_the_pointer() {
        let g = geometry(6, 8, 96, true);
        let spark = spark_rect(card_rect(&g, 4), 96);
        let h = hit(&g, 6, 96, (spark.left + spark.right) / 2, (spark.top + spark.bottom) / 2, 10_000.0).unwrap();
        assert_eq!(h.row, 4);
        // The plot spans [now - 1 h, now], so the middle of it is half an hour back.
        assert!((h.t - 8_200.0).abs() < 1e-6, "middle of the chart is now - 30 min, got {}", h.t);
        assert!((hit(&g, 6, 96, spark.right - 1, spark.top, 10_000.0).unwrap().t - 10_000.0).abs() < 20.0);
        assert!(hit(&g, 6, 96, spark.left, spark.top, 10_000.0).unwrap().t < 10_000.0);
        // the header is not a chart
        assert_eq!(hit(&g, 6, 96, 40, 12, 10_000.0), None);
        let chip = g.chips[3];
        assert_eq!(pick_hit(&g, (chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2), Some(3));
        assert_eq!(pick_hit(&g, 40, 12), None);
    }

    #[test]
    fn history_follows_labels_when_rows_change() {
        let rows = crate::config::default_rows();
        let mut b = Board::new(&rows[..2]);
        let mut h = History::new();
        b.cells[1].quote = Some(crate::model::Quote {
            symbol: "XAG=".into(),
            price: 56.0,
            ts: Some(100.0),
            realtime: true,
            ..Default::default()
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
