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
use crate::chartview::{self, candle_metrics, clock, nice_ticks, tick_interval, Layout, Pointer};
use crate::ranges::{Range, Status};
use crate::model::{fmt_change, fmt_price, Bar, Board, Cell};
use crate::theme::{Gradient, Theme};
use crate::workspace;

use std::collections::VecDeque;
use windows_sys::Win32::Foundation::{COLORREF, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;

/// The sparkline window, and a hard cap on points per row (an hour of 1 s samples).
const WINDOW_S: f64 = 3600.0;
const HIST_CAP: usize = 3600;
const SAMPLE_S: f64 = 1.0;
/// A bar point closer than this to a live sample is redundant.
const BAR_DEDUP_S: f64 = 45.0;
/// Symbol-picker chips per row.
const CHIP_COLS: usize = 4;

pub struct Fonts {
    pub(crate) big: HFONT,
    pub(crate) mid: HFONT,
    pub(crate) small: HFONT,
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
    symbols: Vec<String>,
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
        Self { labels: Vec::new(), symbols: Vec::new(), rows: Vec::new(), gen: 0, backfilled: false }
    }

    /// Re-key by label and symbol; a renamed/replaced instrument starts with no old prices.
    fn align(&mut self, board: &Board) {
        let keys: Vec<_> = board.cells.iter().enumerate().map(|(i, c)| {
            let symbol = board.rows.get(i).map(|r| r.symbol.as_str()).unwrap_or("");
            let provider = c.quote.as_ref().map(|q| q.source).unwrap_or("");
            let key = if provider.is_empty() { symbol.to_string() } else { format!("{symbol}\0{provider}") };
            (c.label.clone(), key)
        }).collect();
        if self.labels.len() == keys.len() && self.symbols.len() == keys.len()
            && keys.iter().enumerate().all(|(i, (l, s))| self.labels[i] == *l && self.symbols[i] == *s) {
            return;
        }
        let labels = std::mem::take(&mut self.labels);
        let symbols = std::mem::take(&mut self.symbols);
        let series = std::mem::take(&mut self.rows);
        let mut old: Vec<_> = labels.into_iter().zip(series).enumerate().map(|(i, (l, row))| {
            (l, symbols.get(i).cloned().unwrap_or_default(), row)
        }).collect();
        for (label, symbol) in keys {
            let row = match old.iter().position(|(l, s, _)| *l == label && *s == symbol) {
                Some(i) => old.swap_remove(i).2,
                None => VecDeque::new(),
            };
            self.labels.push(label);
            self.symbols.push(symbol);
            self.rows.push(row);
        }
    }

    pub fn record(&mut self, board: &Board, now: f64) {
        self.align(board);
        let reseed = board.backfill_gen != self.gen;
        self.gen = board.backfill_gen;
        for (i, c) in board.cells.iter().enumerate() {
            let row = &mut self.rows[i];
            if reseed && c.quote.as_ref().is_none_or(|q| q.source.is_empty()) {
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
    footer: i32,
    scroll: i32,
    pub scroll_max: i32,
    pub btn: RECT,
    /// The symbol-picker toggle, beside the minimise button.
    pub pick_btn: RECT,
    /// The chart toggle, beside the picker.
    pub chart_btn: RECT,
    /// The settings button, at the left end of the header buttons. Ultra only: it opens the same menu
    /// the tray icon does, so settings no longer live in one place nobody looks. It goes on the far
    /// side of the chart toggle rather than beside the cross so the buttons that were already there
    /// keep their pixels (the theme stills are pinned byte for byte).
    pub gear_btn: RECT,
    /// The candle pane, when it is open. Its bottom is the caller's to set: the pane grows with a
    /// resized window, and `geometry` does not know how tall the frame is yet.
    pub chart: Option<RECT>,
    /// How many card columns the current width holds. Public because the window asks before it
    /// resizes: opening the chart must not cost the cards a column.
    pub cols: usize,
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

/// The panel at its content size: the width its cards ask for. Screenshots and the strip's "expand"
/// both want this, and it must never drift — `shots/*.png` pins it.
pub fn geometry(n: usize, catalog: usize, dpi: u32, picker: bool) -> Geo {
    geometry_in(workspace::grid(n, dpi as i32).w, n, catalog, dpi, picker, false)
}

/// The panel in a window `w` wide. Columns come from the width, so a dragged edge reflows the grid
/// instead of clipping it; everything below the header is derived from those columns.
pub fn geometry_in(w: i32, n: usize, catalog: usize, dpi: u32, picker: bool, chart: bool) -> Geo {
    let s = |v: i32| v * dpi as i32 / 96;
    // The cards come first: the chart takes the width left over once they can hold their natural
    // grid, between 360 and 720. Deriving it from the frame instead left the cards a column short at
    // 1,488 px — two pixels under a bucket edge — which is a reflow the user did not ask for.
    let natural_w = workspace::grid(n, dpi as i32).w;
    let chart_w = if chart { (w - natural_w - s(workspace::GAP)).clamp(s(360), s(720)) } else { 0 };
    let cards_w = w - chart_w - if chart { s(workspace::GAP) } else { 0 };
    // The card grid's size and column count are `workspace`'s business: the panel is one pane, and
    // the pane model has to own the numbers before it can own the window.
    let g = workspace::grid_in(cards_w, n, dpi as i32);
    let cols = g.cols;
    let (pad, gap, card_w, card_h, header) = (
        s(workspace::PAD),
        s(workspace::GAP),
        s(workspace::CARD_W),
        s(workspace::CARD_H),
        s(workspace::HEADER),
    );
    let (w, cards_h) = (w.max(g.w), g.h);
    let (chip_h, chip_gap) = (s(26), s(6));
    let chip_rows = if picker { catalog.div_ceil(CHIP_COLS).max(1) as i32 } else { 0 };
    let pick_h = if picker { s(22) + chip_rows * (chip_h + chip_gap) } else { 0 };
    // Header buttons, right to left: minimise, picker, chart, settings. All the same size and the
    // same gap, so the row reads as one control. Each new button goes on the left end, which leaves
    // every existing button where it was.
    let (bw, bh, bx) = (s(30), s(30), w - s(12));
    let btn = RECT { left: bx - bw, top: s(10), right: bx, bottom: s(10) + bh };
    let pick_btn = RECT { left: btn.left - s(6) - bw, top: s(10), right: btn.left - s(6), bottom: s(10) + bh };
    let chart_btn = RECT { left: pick_btn.left - s(6) - bw, top: s(10), right: pick_btn.left - s(6), bottom: s(10) + bh };
    let gear_btn = RECT { left: chart_btn.left - s(6) - bw, top: s(10), right: chart_btn.left - s(6), bottom: s(10) + bh };
    let chart_rect = chart.then(|| RECT { left: w - pad - chart_w, top: header, right: w - pad, bottom: cards_h });
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
    Geo { w, h, header, footer: s(36), scroll: 0, scroll_max: 0, btn, pick_btn, chart_btn, gear_btn, chart: chart_rect, cols, pad, gap, card_w, card_h, pick_top: cards_h, chips }
}

/// Clip and scroll the panel body while keeping header and footer fixed.
pub fn viewport(g: &mut Geo, height: i32, offset: i32) {
    g.scroll_max = (g.h - height).max(0);
    g.scroll = offset.clamp(0, g.scroll_max);
    g.pick_top -= g.scroll;
    for chip in &mut g.chips { chip.top -= g.scroll; chip.bottom -= g.scroll; }
    g.h = height;
}

/// The smallest frame this content can be drawn in: the cards' natural grid, plus a chart pane when
/// one is open. Below this the painter would have to fold a column, which is a reflow the user did
/// not ask for, so the window's own minimum track size comes from here.
pub fn minimum(n: usize, catalog: usize, dpi: u32, picker: bool, chart: bool) -> (i32, i32) {
    let s = |v: i32| v * dpi as i32 / 96;
    let g = geometry(n, catalog, dpi, picker);
    if chart {
        (g.w + s(360) + s(workspace::GAP), g.h)
    } else {
        (g.w, g.h)
    }
}

/// The body scroll offset that brings card `i` fully into the body (between header and footer), or
/// the current offset when it already is. A card taller than the body is aligned to its top.
pub fn reveal(g: &Geo, i: usize) -> i32 {
    let r = card_rect(g, i);
    let bottom = g.h - g.footer;
    let want = if r.top < g.header {
        g.scroll - (g.header - r.top)
    } else if r.bottom > bottom {
        g.scroll + (r.bottom - bottom).min(r.top - g.header)
    } else {
        g.scroll
    };
    want.clamp(0, g.scroll_max)
}

pub fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// A card's rect, given its index. `draw` and the hit tests share this, so a click lands on the
/// card the pointer is over.
pub fn card_rect(g: &Geo, i: usize) -> RECT {
    let (col, row) = ((i % g.cols) as i32, (i / g.cols) as i32);
    let x = g.pad + col * (g.card_w + g.gap);
    let y = g.header + row * (g.card_h + g.gap) - g.scroll;
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
    if y < g.header || y >= g.h - g.footer { return None; }
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
    if y < g.header || y >= g.h - g.footer { return None; }
    g.chips.iter().position(|r| in_rect(r, x, y))
}

/// Which card a pointer is over. The card is the click target (it opens the symbol on CNBC), and
/// the chips sit below the cards, so the two never overlap.
pub fn card_at(g: &Geo, n: usize, x: i32, y: i32) -> Option<usize> {
    if y < g.header || y >= g.h - g.footer { return None; }
    (0..n).find(|i| in_rect(&card_rect(g, *i), x, y))
}

/// The "Open on CNBC" button on a locked card: the top-right corner, where the source tag normally
/// sits. A fixed size in device-independent pixels, so the painter and the click handler agree on it
/// without either needing a font to measure with.
pub fn open_btn_rect(card: RECT, dpi: u32) -> RECT {
    let s = |v: i32| v * dpi as i32 / 96;
    RECT { left: card.right - s(14) - s(96), top: card.top + s(6), right: card.right - s(14), bottom: card.top + s(6) + s(19) }
}

/// Whether a click lands on the locked card's Open button. Only the locked card has one.
pub fn open_btn_at(g: &Geo, selected: Option<usize>, dpi: u32, x: i32, y: i32) -> bool {
    if y < g.header || y >= g.h - g.footer {
        return false;
    }
    selected.is_some_and(|i| in_rect(&open_btn_rect(card_rect(g, i), dpi), x, y))
}

/// Everything a mouse move can change on the open panel, as flags and indices, so two of them can be
/// compared and the difference turned into the rectangle that has to be repainted ([`hot_dirty`]).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Hot {
    pub btn: bool,
    pub pick: bool,
    pub chart_btn: bool,
    pub gear: bool,
    pub open: bool,
    /// The candle under the pointer (and the crosshair, tags and legend that follow it).
    pub aim: Option<Pointer>,
    /// The card whose sparkline the pointer is on (and its crosshair plate). The snapped moment is
    /// part of the comparison but not of the region: the plate is inside the card either way.
    pub card: Option<Hover>,
}

/// The smallest rectangle that has to be repainted when the hover state goes from `was` to `now`:
/// the union of what each differing part draws. `pane` is the candle pane's rectangle from the last
/// frame's layout. The answer is `None`, which stands for "repaint everything", when a part changed
/// that has no rectangle to name (an aimed pointer with no pane, an Open button with no locked card).
/// When nothing differs it is `Some` of a rectangle with no area.
///
/// Every part's own rectangle is padded by two device pixels because `fill_round` and the dotted
/// pen run one pixel past the rectangle they are given.
pub fn hot_dirty(g: &Geo, dpi: u32, selected: Option<usize>, pane: Option<RECT>, was: &Hot, now: &Hot) -> Option<RECT> {
    let mut out: Option<RECT> = None;
    let mut add = |r: RECT| {
        let r = RECT { left: r.left - 2, top: r.top - 2, right: r.right + 2, bottom: r.bottom + 2 };
        out = Some(match out {
            None => r,
            Some(o) => RECT { left: o.left.min(r.left), top: o.top.min(r.top), right: o.right.max(r.right), bottom: o.bottom.max(r.bottom) },
        });
    };
    if was.btn != now.btn {
        add(g.btn);
    }
    if was.pick != now.pick {
        add(g.pick_btn);
    }
    if was.chart_btn != now.chart_btn {
        add(g.chart_btn);
    }
    if was.gear != now.gear {
        add(g.gear_btn);
    }
    if was.open != now.open {
        add(open_btn_rect(card_rect(g, selected?), dpi));
    }
    if was.aim != now.aim {
        add(pane?);
    }
    if was.card != now.card {
        for row in [was.card, now.card].into_iter().flatten().map(|h| h.row) {
            add(card_rect(g, row));
        }
    }
    Some(out.unwrap_or(RECT { left: 0, top: 0, right: 0, bottom: 0 }))
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

/// What the candle pane draws. `bars` is the row's day in order, `t0` the time its first bar carries.
pub struct Chart<'a> {
    pub label: &'a str,
    pub digits: u32,
    pub t0: f64,
    pub bars: &'a [Bar],
    pub last: Option<f64>,
    pub unavailable: bool,
    /// The range shown. With `D1` the bars are the board's day; otherwise the series' window.
    pub range: Range,
    /// What to say while `bars` is empty: loading, failed, or no data. `Ready` otherwise.
    pub status: Status,
    /// Seconds one bar lasts (the series' inferred spacing; 60 for 1D). The fold never goes finer.
    pub bar_s: f64,
    /// Draw the range tabs and store them in the layout. False reproduces the pane from before
    /// ranges (the shot fixture's default).
    pub tabs: bool,
    /// Seconds east of UTC the pane's clock runs on (0 = UTC). Candles are cut and labelled on it.
    pub tz_offset_s: i32,
    /// The zone's name as the header shows it ("local" or "UTC"); it is the click target that flips it.
    pub tz_label: &'a str,
    /// Previous close, for the dashed reference line. `None` when the quote has none (or sends 0).
    pub prev: Option<f64>,
    /// The day's high and low, drawn as short ticks on the axis. `None` as above.
    pub day_hi: Option<f64>,
    pub day_lo: Option<f64>,
    /// The day's change in percent, drawn beside the last price in the up or down colour.
    pub change_pct: Option<f64>,
    /// The charted symbol's alert levels (above and below, in any order), on every range. Inside the
    /// price window a dashed amber line with the price at the plot's right end; outside it a marker on
    /// the top or bottom edge (`chartview::level_marks`). Empty draws nothing.
    pub levels: &'a [f64],
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
    /// The chart toggle is under the pointer.
    pub chart_hover: bool,
    /// The settings button is under the pointer.
    pub gear_hover: bool,
    /// Cards are selectable (Ultra). A click locks a card instead of opening CNBC; the page is one
    /// explicit button on the locked card. The classic build keeps its one-click behaviour.
    pub select: bool,
    /// Ultra's footer names the keys (arrows, Enter, Del, Esc). False keeps the older hint, which is
    /// what every catalogue still shows.
    pub keys_hint: bool,
    /// The locked card, drawn with an accent border and the Open button. The chart follows it.
    pub selected: Option<usize>,
    /// The Open button on the locked card is under the pointer.
    pub open_hover: bool,
    /// The pointer is on a chart.
    pub hover: Option<Hover>,
    /// The pointer is on a candle of the pane: it draws a crosshair, the price tag and the legend.
    pub chart_at: Option<Pointer>,
    /// Where the pane stores the `Layout` it drew with (cleared when it drew nothing with bars). The
    /// `Ui` owns the cell and the pointer code reads it back, so a hit test answers from the pixels
    /// last painted.
    pub layout: &'a std::cell::Cell<Option<Layout>>,
    /// `Some` when the picker is open. `None` also hides the toggle: the classic build has no
    /// picker, so it must not draw a dead button.
    pub picker: Option<Picker<'a>>,
    pub picker_scroll: i32,
    /// This build supports a picker at all.
    pub picker_btn: bool,
    /// The candle pane's data, when it is open. The bars are the row's whole day; the pane picks
    /// what to show.
    pub chart: Option<Chart<'a>>,
    /// The chart toggle is drawn (Ultra only: the classic build has one window, one look).
    pub chart_btn: bool,
    /// The settings button is drawn (Ultra only, for the same reason).
    pub settings_btn: bool,
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
    let radius = if t.is_pixel() { 0 } else { s(12) };
    // A locked card is outlined in the accent: that is the whole of "this is the one the chart shows".
    // The border is a second, inset fill rather than a pen, so it is the same two pixels on every
    // edge and the corners stay round at any dpi.
    let locked = sc.select && sc.selected == Some(i);
    if locked {
        let edge = s(2).max(2);
        fill_round(dc, r, radius, t.accent);
        let inner = RECT { left: r.left + edge, top: r.top + edge, right: r.right - edge, bottom: r.bottom - edge };
        fill_round(dc, inner, (radius - edge).max(0), t.card);
    } else {
        fill_round(dc, r, radius, t.card);
    }
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
            let src = if !q.source.is_empty() { q.source } else if q.realtime { "CNBC" } else { "CNBC delayed" };
            let tag = if !q.source.is_empty() { format!("{src} \u{b7} age unknown") } else { match q.ts {
                Some(ts) => format!("{src} \u{b7} {}", fmt_age(sc.now - ts)),
                None => src.to_string(),
            }};
            let room = rx - ix - text_width(dc, sc.fonts.mid, &c.label) - s(10);
            let tag = match q.volume {
                Some(v) => {
                    let with = format!("{tag} \u{b7} {}", fmt_vol(v));
                    if text_width(dc, sc.fonts.small, &with) <= room { with } else { tag }
                }
                None => tag,
            };
            // The Open button takes the tag's corner on a locked card; the tag is the first thing to
            // give way, as it already is for the volume.
            if !locked {
                text_right(dc, sc.fonts.small, rx, r.top + s(10), &tag, if c.stale { t.amber } else { t.dim });
            }
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
    if c.quote.as_ref().is_some_and(|q| !q.source.is_empty()) {
        let area = spark_rect(r, sc.dpi);
        text(dc, sc.fonts.small, area.left, area.top + s(4), "chart unavailable", t.dim);
    } else {
        spark(dc, sc, spark_rect(r, sc.dpi), i, sc.hist.row(i), day_baseline(c));
    }
    if locked {
        open_button(dc, sc, r);
    }
}

/// The "Open on CNBC" button: the card's one outward action, shown only on the locked card so a
/// stray click on a price can no longer throw a browser window at the user.
fn open_button(dc: HDC, sc: &Scene, card: RECT) {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    let r = open_btn_rect(card, sc.dpi);
    // Idle it is a chip (the picker's "on" look); under the pointer it fills with the accent, so the
    // one clickable thing on the card announces itself.
    fill_round(dc, r, if t.is_pixel() { 0 } else { s(8) }, if sc.open_hover { t.accent } else { t.btn });
    let label = "Open on CNBC";
    let (tw, th) = (text_width(dc, sc.fonts.small, label), line_height(dc, sc.fonts.small));
    text(
        dc,
        sc.fonts.small,
        r.left + (r.right - r.left - tw) / 2,
        r.top + (r.bottom - r.top - th) / 2,
        label,
        if sc.open_hover { t.card } else { t.accent },
    );
}

/// A cog centred on `c`, `r` pixels to the tip of a tooth. One filled polygon with the hub cut out:
/// `PolyPolygon` fills with the alternate rule, so the inner ring is a hole and the header's
/// gradient shows through it, which a hub painted in a flat colour would not on a ramp theme.
fn gear(dc: HDC, c: (i32, i32), r: i32, color: COLORREF) {
    const TEETH: usize = 8;
    let (root, hole) = (r as f64 * 0.74, r as f64 * 0.32);
    let at = |rad: f64, a: f64| POINT { x: c.0 + (rad * a.cos()).round() as i32, y: c.1 + (rad * a.sin()).round() as i32 };
    let mut pts: Vec<POINT> = Vec::with_capacity(TEETH * 4 + 16);
    let step = std::f64::consts::TAU / TEETH as f64;
    for k in 0..TEETH {
        // Each tooth is a trapezoid: it leaves the root circle, runs out to the tip, and returns.
        let a = k as f64 * step;
        pts.push(at(root, a - step * 0.30));
        pts.push(at(r as f64, a - step * 0.16));
        pts.push(at(r as f64, a + step * 0.16));
        pts.push(at(root, a + step * 0.30));
    }
    let outer = pts.len() as i32;
    for k in 0..16 {
        pts.push(at(hole, k as f64 * std::f64::consts::TAU / 16.0));
    }
    let counts = [outer, 16];
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_b = SelectObject(dc, brush);
        let old_p = SelectObject(dc, GetStockObject(NULL_PEN));
        PolyPolygon(dc, pts.as_ptr(), counts.as_ptr(), 2);
        SelectObject(dc, old_p);
        SelectObject(dc, old_b);
        DeleteObject(brush);
    }
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
    // Columns come from the width we were asked to draw in, so the frame and its content agree:
    // the window is the composition, and a dragged edge reflows the grid instead of leaving a gutter.
    let mut g = geometry_in(w, sc.board.cells.len(), sc.picker.as_ref().map(|p| p.list.len()).unwrap_or(0), sc.dpi, sc.picker.is_some(), sc.chart.is_some());
    viewport(&mut g, h, sc.picker_scroll);
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
    if sc.settings_btn {
        if sc.gear_hover {
            fill_round(dc, g.gear_btn, radius(8), t.btn);
        }
        let c = ((g.gear_btn.left + g.gear_btn.right) / 2, (g.gear_btn.top + g.gear_btn.bottom) / 2);
        gear(dc, c, s(8), if sc.gear_hover { t.price } else { t.label });
    }
    if sc.chart_btn {
        // Lit while the pane is open, and under the pointer: the toggle is the way back to the compact
        // panel, so it has to look pressable in both states.
        let hot = sc.chart.is_some();
        if hot || sc.chart_hover {
            fill_round(dc, g.chart_btn, radius(8), t.btn);
        }
        // Three candles, tallest in the middle: a chart, at 12 px.
        let (px, py) = ((g.chart_btn.left + g.chart_btn.right) / 2, (g.chart_btn.top + g.chart_btn.bottom) / 2);
        let col = if hot || sc.chart_hover { t.price } else { t.label };
        for (k, len) in [(-1, s(7)), (0, s(12)), (1, s(9))] {
            let x = px + k * s(5);
            fill_rect(dc, RECT { left: x, top: py - len / 2, right: x + s(2).max(2), bottom: py + len / 2 }, col);
        }
    }
    let (cx, cy) = ((g.btn.left + g.btn.right) / 2, (g.btn.top + g.btn.bottom) / 2);
    let (bar, thick) = (s(12), s(2).max(2));
    fill_rect(dc, RECT { left: cx - bar / 2, top: cy - thick / 2, right: cx + bar / 2, bottom: cy - thick / 2 + thick }, if sc.btn_hover { t.price } else { t.label });

    let clip = if sc.picker.is_some() || g.scroll_max > 0 {
        unsafe { let saved = SaveDC(dc); IntersectClipRect(dc, 0, g.header, w, (h - g.footer).max(g.header)); saved }
    } else { 0 };
    for (i, c) in sc.board.cells.iter().enumerate() {
        card(dc, sc, card_rect(&g, i), i, c);
    }
    if let Some(p) = &sc.picker {
        picker(dc, sc, &g, p);
    }
    if clip != 0 { unsafe { RestoreDC(dc, clip); } }
    if g.scroll_max > 0 {
        text_right(dc, sc.fonts.small, w - g.pad, h - s(27), "Wheel / PgUp / PgDn: scroll", t.label);
    }
    // Whatever the pane stores below is this frame's; a frame without the pane leaves no layout.
    sc.layout.set(None);
    if let (Some(c), Some(mut r)) = (&sc.chart, g.chart) {
        // The pane grows with the frame: its bottom is the footer, not the last card row.
        r.bottom = (h - s(34)).max(r.top + s(40));
        candles(dc, sc, r, c);
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
    let left_w = text(dc, sc.fonts.small, g.pad + s(2), fy, &left, t.dim);
    if g.scroll_max == 0 {
        // Where a click goes depends on the build: Ultra locks the card (and offers the page as a
        // button), the classic build opens CNBC straight away, as it always has.
        let mut hint = if sc.select { "click a card: lock it \u{b7} Esc: dock" } else { "click a card: CNBC \u{b7} Esc: dock" };
        // Ultra names the keys, when the line fits beside what the left side says.
        const KEYS: &str = "arrows: pick \u{b7} Enter: CNBC \u{b7} Del: remove \u{b7} Esc: dock";
        if sc.keys_hint && g.pad + s(2) + left_w + s(24) + text_width(dc, sc.fonts.small, KEYS) <= w - g.pad - s(2) {
            hint = KEYS;
        }
        text_right(dc, sc.fonts.small, w - g.pad - s(2), fy, hint, t.dim);
    }
}

/// The candle pane.
///
/// Bars arrive in time order, one minute each. The pane owns the price window, the bucket and the
/// axes: a day of one-minute bars in 650 px is a comb, not a chart, so when a minute is narrower
/// than a candle the same session is folded into 5-, 15-, 30- or 60-minute candles instead of being
/// squeezed. Flat bars still draw a one-pixel body rather than being skipped — spot symbols return
/// `open == high == low == close` for most minutes, and a chart with holes reads as missing data.
///
/// What makes a candle chart look right is mostly arithmetic on whole pixels, and that is where the
/// first version looked wrong: a fractional step rounded per candle gave bodies 5 and 6 px wide in
/// the same row, and a 1 px wick cannot sit in the middle of an even-width body. So the step is a
/// whole, odd number of pixels, every body is the same odd width, and the wick is the middle pixel.
/// The axes follow the same rule: gridlines sit on round prices and round clock times, not on
/// quarters of whatever the day's range happened to be.
fn candles(dc: HDC, sc: &Scene, r: RECT, c: &Chart) {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    let radius = if t.is_pixel() { 0 } else { s(12) };
    fill_round(dc, r, radius, t.card);
    let lh = line_height(dc, sc.fonts.small);
    // The range tabs are on the header's second line in every state that names a card, so the way out
    // of "loading" or "could not load" is always a tab that still works. A pane with no tabs
    // (`Chart::tabs` false: the shot fixture's default) is exactly the pane from before ranges.
    let show_tabs = c.tabs && !c.label.is_empty();
    let tabs = if show_tabs { range_tabs(dc, sc, r, c, lh) } else { [chartview::Rect::default(); 6] };
    if show_tabs {
        // Until the plot is laid out below, the tabs are all there is to hit.
        sc.layout.set(Some(Layout {
            pane: chartview::Rect::new(r.left, r.top, r.right, r.bottom),
            plot: chartview::Rect::default(),
            step: 1,
            body: 1,
            wick: 1,
            bars: 0,
            lo: 0.0,
            hi: 0.0,
            bucket_s: 60.0,
            zone_btn: chartview::Rect::default(),
            tabs,
            vol: None,
        }));
    }
    if c.bars.is_empty() {
        // Named even when empty: a bare rectangle could be any pane, and the reason it is empty is
        // the one thing the reader wants to know.
        text(dc, sc.fonts.small, r.left + s(12), r.top + s(8), c.label, t.label);
        let why = if c.label.is_empty() {
            "select a card to chart it".to_string()
        } else if c.unavailable {
            "chart unavailable for this source".to_string()
        } else {
            match (c.range, c.status) {
                (Range::D1, _) => "waiting for the day's bars".to_string(),
                (rg, Status::Loading) => format!("loading {}", rg.label()),
                (rg, Status::Failed) => format!("could not load {}, retrying", rg.label()),
                (_, _) => "no data for this range".to_string(),
            }
        };
        // Clear of the tabs when there are any; where it always was when there are none.
        let wy = if show_tabs { r.top + s(56) } else { r.top + s(28) };
        text(dc, sc.fonts.small, r.left + s(12), wy, &why, t.dim);
        return;
    }
    // The price axis is as wide as the widest label it will carry, so a six-figure price is not
    // clipped and a two-figure one does not waste a quarter of the pane.
    let last = c.last.unwrap_or_else(|| c.bars[c.bars.len() - 1].c as f64);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for b in c.bars {
        lo = lo.min(b.l as f64);
        hi = hi.max(b.h as f64);
    }
    let widest = [lo, hi, last].iter().map(|v| text_width(dc, sc.fonts.small, &fmt_price(*v, c.digits))).max().unwrap_or(0);
    // The header is two lines: the pane's name, the day's change and the last price on the first, and
    // a line of its own for the hover legend (and the range tabs) on the second, so the legend never
    // has to share a row with (or replace) the label, the zone button or the price. The line is
    // reserved even while nothing is hovered, so the plot does not jump when the pointer arrives.
    let full = RECT { left: r.left + s(12), top: r.top + s(50), right: r.right - widest - s(22), bottom: r.bottom - s(24) };
    if full.right - full.left < s(40) || full.bottom - full.top < s(30) {
        return;
    }
    // The clock the buckets, the axis and the legend are read on: the viewer's for intraday bars, the
    // exchange's own date for daily and longer ones (see `chartview::date_zone`).
    let zone = chartview::date_zone(c.bar_s, c.tz_offset_s);
    // 1D folds minutes into at most hours, as it always has; the long ranges start at the series' own
    // spacing and climb to a month.
    let coarsest = if c.range == Range::D1 { 3_600.0 } else { chartview::MONTH_S };
    let (bars, bucket) = chartview::fitted_to(c.bars, full.right - full.left, c.t0, zone, c.bar_s, coarsest);
    let span = chartview::bucket_name(bucket);
    // Volume: when any drawn candle has some, the bottom fifth of the plot is a strip for it and the
    // price plot is that much shorter. No volume anywhere: no strip, and `plot == full`.
    let has_volume = bars.iter().any(|b| b.v > 0.0);
    let (price_plot, vol) = chartview::split_plot(chartview::Rect::new(full.left, full.top, full.right, full.bottom), has_volume, s(3));
    let plot = RECT { left: price_plot.left, top: price_plot.top, right: price_plot.right, bottom: price_plot.bottom };
    if hi <= lo {
        // Every bar identical: give it a window anyway, or the division below is by zero. (A bar
        // price is parsed from JSON, so it cannot be NaN; `<=` and `!(> )` mean the same here.)
        let m = (hi.abs() * 0.001).max(0.5);
        lo -= m;
        hi += m;
    }
    let pad = (hi - lo) * 0.08;
    let (lo, hi) = (lo - pad, hi + pad);

    // Header, first line. What is charted, at what resolution, in which time zone (the zone name is
    // a button: it is underlined, and its rectangle goes into the layout for the click), the day's
    // change, and where it last traded.
    let (hx, hy) = (r.left + s(12), r.top + s(8));
    // Daily and longer candles are trading days, not instants: they have no zone to name or to switch,
    // so the header ends at the candle size and there is no button (an empty rectangle is never hit).
    let zone_btn = if chartview::daily(c.bar_s) {
        text(dc, sc.fonts.small, hx, hy, &format!("{} \u{b7} {span}", c.label), t.label);
        chartview::Rect::default()
    } else {
        let hw = text(dc, sc.fonts.small, hx, hy, &format!("{} \u{b7} {span} \u{b7} ", c.label), t.label);
        let zw = text(dc, sc.fonts.small, hx + hw, hy, c.tz_label, t.accent);
        fill_rect(dc, RECT { left: hx + hw, top: hy + lh, right: hx + hw + zw, bottom: hy + lh + 1 }, t.accent);
        chartview::Rect::new(hx + hw - s(4), r.top + s(4), hx + hw + zw + s(4), hy + lh + s(4))
    };
    let price_text = fmt_price(last, c.digits);
    let pw = text_width(dc, sc.fonts.mid, &price_text);
    text_right(dc, sc.fonts.mid, r.right - s(12), r.top + s(6), &price_text, t.price);
    if let Some(p) = c.change_pct.filter(|p| p.is_finite()) {
        let txt = fmt_change(p);
        let (right, w) = (r.right - s(12) - pw - s(10), text_width(dc, sc.fonts.small, &txt));
        // Only when it clears the zone button: a narrow pane drops the change, not the zone.
        if right - w > zone_btn.right + s(10) {
            text_right(dc, sc.fonts.small, right, hy + s(1), &txt, if p >= 0.0 { t.up } else { t.down });
        }
    }

    // The one layout of this frame. Everything in the plot below is placed with it, and it is stored
    // at the end for the pointer.
    let n = bars.len();
    let (step, body, wick) = candle_metrics(plot.right - plot.left, n as i32, sc.dpi);
    let lay = Layout {
        pane: chartview::Rect::new(r.left, r.top, r.right, r.bottom),
        plot: chartview::Rect::new(plot.left, plot.top, plot.right, plot.bottom),
        step,
        body,
        wick,
        bars: n,
        lo,
        hi,
        bucket_s: bucket,
        zone_btn,
        tabs,
        vol,
    };
    // Coloured by the day's direction, the same one the header and the card report. Only when the
    // day's change is unknown does it fall back to the bars: the last close against the first open.
    let up = bars.last().map(|b| b.c).unwrap_or(0.0) as f64 >= bars[0].o as f64;
    let tone = match c.change_pct.filter(|p| p.is_finite()) {
        Some(p) if p >= 0.0 => t.up,
        Some(_) => t.down,
        None if up => t.up,
        None => t.down,
    };
    let (ylast, tag_h) = (lay.y_of(last).clamp(plot.top, plot.bottom), lh + s(4));

    // Price gridlines on round values. A label that would sit under the last-price tag is skipped:
    // two numbers on one line is how a chart gets read wrong.
    let (ticks, decimals) = nice_ticks(lo, hi, 4);
    for v in ticks {
        let y = lay.y_of(v);
        fill_rect(dc, RECT { left: plot.left, top: y, right: plot.right, bottom: y + 1 }, t.guide);
        if (y - ylast).abs() >= tag_h {
            text_right(dc, sc.fonts.small, r.right - s(10), y - lh / 2, &fmt_price(v, decimals), t.label);
        }
    }

    // Time ticks hang from the bottom of the plot, or of the volume strip when there is one. Every
    // tick gets its mark; a label is placed only where it clears the labels already placed, and the
    // date labels (a new day on the long intraday range) are placed first, so a clock label beside a
    // day boundary gives way to the date and not the other way round.
    let base = lay.base();
    let tz = zone as f64;
    let mut ticks: Vec<(i32, String, bool)> = Vec::new();
    if bucket >= chartview::DAY_S {
        // Candles of a day or more: month starts ("Oct"), or year starts ("2025") when the months
        // would not fit, or dates when the window is under two months.
        let measure = |label: &str| text_width(dc, sc.fonts.small, label);
        for (i, label) in chartview::date_ticks(&bars, c.t0, zone, step, &measure, s(8)) {
            ticks.push((lay.x_of(i), label, false));
        }
    } else {
        // Round clock times, short marks under the plot and a label centred on each. On the long
        // intraday range the first tick of a local day is labelled with the date instead.
        let label_w = text_width(dc, sc.fonts.small, "00:00");
        let every = tick_interval(step as f64 / bucket, (label_w + s(28)) as f64);
        let (mut prev_key, mut prev_day) = (None, None);
        for (i, b) in bars.iter().enumerate() {
            let tb = b.t(c.t0);
            let key = ((tb + tz) / every).floor();
            let crossed = prev_key.is_some_and(|k| k != key);
            prev_key = Some(key);
            let day = chartview::local_day(tb, zone);
            let new_day = c.range != Range::D1 && prev_day.is_some_and(|d| d != day);
            if !crossed {
                continue;
            }
            prev_day = Some(day);
            // The bar's own start, which is the boundary itself except after a break in the session
            // (a market reopening at 23:15 is labelled 23:15, not a 23:00 it did not trade at).
            let label = if new_day { chartview::date_label(tb, zone, chartview::DateStyle::DayMonth) } else { clock(tb, zone) };
            ticks.push((lay.x_of(i), label, new_day));
        }
    }
    let mut placed: Vec<(i32, i32)> = Vec::new();
    for date_pass in [true, false] {
        for (x, label, is_date) in &ticks {
            if *is_date != date_pass {
                continue;
            }
            let w = text_width(dc, sc.fonts.small, label);
            let lx = (*x - w / 2).clamp(plot.left, (plot.right - w).max(plot.left));
            if placed.iter().all(|(a, b)| lx > b + s(8) || lx + w + s(8) < *a) {
                text(dc, sc.fonts.small, lx, base + s(6), label, t.label);
                placed.push((lx, lx + w));
            }
        }
    }
    for (x, _, _) in &ticks {
        fill_rect(dc, RECT { left: *x, top: base, right: *x + 1, bottom: base + s(4) }, t.guide);
    }

    // Everything below up to the candles is under them, so nothing cuts a body.
    // Session breaks: a faint dotted line on the boundary between the last candle before the break
    // and the first after it. Only for candles shorter than a day: every weekend is not a break.
    if chartview::marks_breaks(bucket) {
        for i in chartview::gaps(&bars, bucket) {
            let x = lay.x_of(i) - step / 2;
            pen_line(dc, PS_DOT, t.guide, (x, plot.top), (x, plot.bottom));
        }
    }
    // Previous close: a dashed line with a small label at the plot's left edge, above the line (below
    // it when the line is at the very top). The label is inside the plot and the last-price tag is on
    // the axis, so the two can never touch. Reference lines belong to today's session: 1D only.
    let refs = c.range == Range::D1;
    if let Some(p) = chartview::in_window(chartview::positive(c.prev).filter(|_| refs), lo, hi) {
        let y = lay.y_of(p);
        pen_line(dc, PS_DASH, t.dim, (plot.left, y), (plot.right, y));
        let ly = if y - lh > plot.top { y - lh - 1 } else { y + 2 };
        text(dc, sc.fonts.small, plot.left + s(4), ly, "prev", t.dim);
    }
    // Where it last traded: a dotted line across the plot, running into a tag on the price axis.
    // Coloured by the day's direction, the same one the header and the card report.
    pen_line(dc, PS_DOT, tone, (plot.left, ylast), (plot.right, ylast));

    // Candles: a whole, odd number of pixels per bar, so every body is the same width and the wick
    // is exactly the middle pixel. The leftover (under a bar's width per candle) is the open right
    // edge of a session still in progress.
    let colour = |b: &Bar| {
        if (b.c - b.o).abs() <= f32::EPSILON {
            t.dim
        } else if b.c > b.o {
            t.up
        } else {
            t.down
        }
    };
    for (i, b) in bars.iter().enumerate() {
        let x = lay.x_of(i);
        let col = colour(b);
        let (y0, y1) = (lay.y_of(b.h as f64), lay.y_of(b.l as f64));
        let wx = x - wick / 2;
        fill_rect(dc, RECT { left: wx, top: y0.min(y1), right: wx + wick, bottom: y1.max(y0 + 1) }, col);
        let (b0, b1) = (lay.y_of(b.o.max(b.c) as f64), lay.y_of(b.o.min(b.c) as f64));
        let bx = x - body / 2;
        fill_rect(dc, RECT { left: bx, top: b0.min(b1), right: bx + body, bottom: b1.max(b0 + 1) }, col);
    }
    // The volume strip: one bar per candle, in the candle's colour, bottom-aligned.
    if let Some(v) = vol {
        let max = bars.iter().map(|b| b.v).fold(0.0f32, f32::max);
        for (i, b) in bars.iter().enumerate() {
            let h = chartview::volume_px(b.v, max, v.bottom - v.top);
            if h > 0 {
                let bx = lay.x_of(i) - body / 2;
                fill_rect(dc, RECT { left: bx, top: v.bottom - h, right: bx + body, bottom: v.bottom }, colour(b));
            }
        }
    }

    // The day's high and low: short ticks on the axis edge of the plot, over the candles, only when
    // they are inside the window the bars set (and only on 1D).
    for v in [c.day_hi, c.day_lo] {
        if let Some(p) = chartview::in_window(chartview::positive(v).filter(|_| refs), lo, hi) {
            let y = lay.y_of(p);
            fill_rect(dc, RECT { left: plot.right - s(8), top: y, right: plot.right, bottom: y + s(2).max(2) }, t.label);
        }
    }

    // Alert levels, over the candles and under the crosshair: a dashed amber line with its price at the
    // right end of the plot (above the line, below it at the very top); a level outside the window is a
    // small triangle on the top or bottom edge with the nearest price, and "+n" for the others there.
    let marks = chartview::level_marks(&lay, c.levels);
    for &(p, y) in &marks.lines {
        pen_line(dc, PS_DASH, t.amber, (plot.left, y), (plot.right, y));
        let ly = if y - lh > plot.top { y - lh - 1 } else { y + 2 };
        text_right(dc, sc.fonts.small, plot.right - s(4), ly, &fmt_price(p, c.digits), t.amber);
    }
    for (mark, up) in [(marks.above, true), (marks.below, false)] {
        let Some((p, n)) = mark else { continue };
        let half = s(4).max(3);
        let cx = plot.right - s(4) - half;
        // The triangle points out of the window: its tip on the edge, its base one half-width inside.
        for i in 0..half {
            let y = if up { plot.top + i } else { plot.bottom - 1 - i };
            fill_rect(dc, RECT { left: cx - i, top: y, right: cx + i + 1, bottom: y + 1 }, t.amber);
        }
        let more = if n > 1 { format!(" +{}", n - 1) } else { String::new() };
        let ly = if up { plot.top + s(1) } else { plot.bottom - lh - s(1) };
        text_right(dc, sc.fonts.small, cx - half - s(4), ly, &format!("{}{more}", fmt_price(p, c.digits)), t.amber);
    }

    // The crosshair, when the pointer is on a candle: a vertical line through it and a horizontal at
    // the pointer's row. `bar` is clamped because the pointer was taken from the previous frame and
    // a resize can have refolded the bars since.
    let hover = sc.chart_at.filter(|p| p.y >= plot.top && p.y < plot.bottom).map(|p| (p.bar.min(n - 1), p.y));
    if let Some((bar, y)) = hover {
        let x = lay.x_of(bar);
        pen_line(dc, PS_DOT, t.accent, (x, plot.top), (x, plot.bottom));
        pen_line(dc, PS_DOT, t.accent, (plot.left, y), (plot.right, y));
    }

    // The last-price tag goes on over the axis, so nothing can paint across the number.
    let tag = |price: f64, y: i32, fill: COLORREF| {
        let tag = fmt_price(price, c.digits);
        let tag_w = text_width(dc, sc.fonts.small, &tag) + s(10);
        let tag_r = RECT { left: r.right - s(6) - tag_w, top: y - tag_h / 2, right: r.right - s(6), bottom: y - tag_h / 2 + tag_h };
        fill_round(dc, tag_r, if t.is_pixel() { 0 } else { s(4) }, fill);
        text(dc, sc.fonts.small, tag_r.left + s(5), tag_r.top + (tag_h - lh) / 2, &tag, t.card);
    };
    tag(last, ylast, tone);
    if let Some((bar, y)) = hover {
        // The hover tag is drawn last and may cover the last-price tag while the pointer is on its
        // row: it is the transient one, and it is the number being asked about.
        if let Some(price) = lay.price_at(y) {
            tag(price, y, t.accent);
        }
        // The legend: the header's second line, the whole candle. If the line is too narrow for the
        // full legend (the tabs take the right end of it) the compact one (time, close, change) is
        // used instead, and if even that would touch the tabs it is left out.
        let fmt = |v: f64| fmt_price(v, c.digits);
        let avail = if show_tabs { tabs[0].left - (r.left + s(12)) - s(10) } else { r.right - r.left - s(24) };
        let stamp = chartview::stamp_for(c.range == Range::D1, bucket);
        let mut line = chartview::readout_with(&bars[bar], c.t0, zone, &fmt, false, stamp);
        if text_width(dc, sc.fonts.small, &line) > avail {
            line = chartview::readout_with(&bars[bar], c.t0, zone, &fmt, true, stamp);
        }
        if !show_tabs || text_width(dc, sc.fonts.small, &line) <= avail {
            text(dc, sc.fonts.small, r.left + s(12), r.top + s(30), &line, t.price);
        }
    }
    sc.layout.set(Some(lay));
}

/// The six range tabs, right-aligned on the header's second line, the current one lit. Returns their
/// rectangles in `Range::ALL` order for the layout (the click is answered from them).
fn range_tabs(dc: HDC, sc: &Scene, r: RECT, c: &Chart, lh: i32) -> [chartview::Rect; 6] {
    let t = sc.theme;
    let s = |v: i32| v * sc.dpi as i32 / 96;
    let (top, bottom) = (r.top + s(28), r.top + s(28) + lh + s(4));
    let mut out = [chartview::Rect::default(); 6];
    let mut right = r.right - s(12);
    for (k, rg) in Range::ALL.iter().enumerate().rev() {
        let w = text_width(dc, sc.fonts.small, rg.label()) + s(10);
        let rc = RECT { left: right - w, top, right, bottom };
        let lit = *rg == c.range;
        if lit {
            fill_round(dc, rc, if t.is_pixel() { 0 } else { s(4) }, t.btn);
        }
        text(dc, sc.fonts.small, rc.left + s(5), top + (bottom - top - lh) / 2, rg.label(), if lit { t.price } else { t.label });
        out[k] = chartview::Rect::new(rc.left, rc.top, rc.right, rc.bottom);
        right = rc.left - s(2);
    }
    out
}

/// A one-pixel straight line in a pen style (dotted, dashed), in `color`.
fn pen_line(dc: HDC, style: PEN_STYLE, color: COLORREF, a: (i32, i32), b: (i32, i32)) {
    unsafe {
        let pen = CreatePen(style, 1, color);
        let old = SelectObject(dc, pen);
        let seg = [POINT { x: a.0, y: a.1 }, POINT { x: b.0, y: b.1 }];
        Polyline(dc, seg.as_ptr(), 2);
        SelectObject(dc, old);
        DeleteObject(pen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chart_pane_takes_half_the_frame_and_leaves_the_cards_whole_columns() {
        let g = geometry_in(1_490, 6, 0, 96, false, true);
        let chart = g.chart.expect("the pane is open");
        assert_eq!(chart.right - chart.left, 720, "capped so the eye can hold it");
        assert_eq!(chart.left, 1_490 - g.pad - 720);
        // 1,490 - 720 - 10 = 760: exactly the natural panel, so the cards keep three columns.
        assert_eq!(g.cols, 3);
        assert_eq!(g.h, 394);
    }

    #[test]
    fn the_chart_never_costs_the_cards_a_column() {
        // 1,488 was the frame the old half-the-window rule produced, and it left two columns where
        // 1,490 left three. The pane now takes the leftover after the cards keep their grid.
        let natural = geometry(6, 0, 96, false);
        assert_eq!(geometry_in(1_488, 6, 0, 96, false, true).cols, 3, "1,488 must hold three");
        for w in [1_130, 1_200, 1_400, 1_488, 1_490, 1_600, 1_920, 2_560] {
            let split = geometry_in(w, 6, 0, 96, false, true);
            let cw = split.chart.map(|c| c.right - c.left).unwrap_or(0);
            // Once there is room for the cards' own grid plus a chart, the chart never costs a
            // column. Above that the extra width still goes to the cards, up to a 720 px pane.
            assert!(split.cols >= natural.cols, "w={w} cols={}", split.cols);
            assert!((360..=720).contains(&cw), "w={w} chart={cw}");
        }
        // A frame too narrow for both: the chart still gets its 360 and the cards give up a column,
        // which is the one case where opening a pane costs the grid something.
        let tight = geometry_in(1_000, 6, 0, 96, false, true);
        assert_eq!(tight.cols, 2);
        assert_eq!(tight.chart.map(|c| c.right - c.left), Some(360));
    }

    #[test]
    fn an_open_chart_raises_the_window_floor_to_two_whole_panes() {
        let (w, h) = minimum(6, 0, 96, false, true);
        assert_eq!((w, h), (760 + 360 + 10, 394), "the cards' grid plus a whole pane");
        // At that floor the cards still have all three columns: the pane never costs a column.
        let g = geometry_in(w, 6, 0, 96, false, true);
        assert_eq!(g.chart.map(|c| c.right - c.left), Some(360));
        assert_eq!(g.cols, 3);
        // Closed, the floor is the panel it has always been.
        assert_eq!(minimum(6, 0, 96, false, false), (geometry(6, 0, 96, false).w, 394));
    }

    #[test]
    fn a_closed_chart_leaves_the_panel_exactly_as_it_was() {
        let plain = geometry(6, 0, 96, false);
        let same = geometry_in(plain.w, 6, 0, 96, false, false);
        assert!(same.chart.is_none());
        assert_eq!((same.cols, same.h), (plain.cols, plain.h));
    }

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
        assert!(g.gear_btn.right < g.chart_btn.left, "settings sits at the left end of the group");
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
    fn picker_hit_does_not_activate_a_chip_below_the_visible_frame() {
        let mut g = geometry(6, 24, 96, true);
        let chip = g.chips[0];
        g.h = chip.top + 4;
        assert_eq!(pick_hit(&g, chip.left + 2, chip.top + 5), None);
    }

    #[test]
    fn scrolling_reaches_the_last_chip_without_changing_its_identity() {
        for dpi in [96, 144, 192] {
            let mut g = geometry(6, 24, dpi, true);
            let height = 394 * dpi as i32 / 96;
            viewport(&mut g, height, i32::MAX);
            assert!(g.scroll_max > 0);
            let last = g.chips[23];
            let x = (last.left + last.right) / 2;
            let y = (last.top + last.bottom) / 2;
            assert!(y >= g.header && y < height - g.footer);
            assert_eq!(pick_hit(&g, x, y), Some(23));
            assert_eq!(pick_hit(&g, x, height - 1), None);
        }
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
    fn history_resets_when_symbol_changes_under_same_label() {
        let old = crate::config::Row { label: "X".into(), symbol: "OLD".into(), digits: 2, name: String::new() };
        let new = crate::config::Row { symbol: "NEW".into(), ..old.clone() };
        let mut board = Board::new(&[old]);
        board.cells[0].quote = Some(crate::model::Quote { symbol: "OLD".into(), price: 42.0, ..Default::default() });
        board.cells[0].stale = false;
        let mut history = History::new();
        history.record(&board, 100.0);
        board.set_rows(&[new]);
        history.record(&board, 101.0);
        assert!(history.row(0).unwrap().is_empty(), "old symbol's line must not survive");
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

    #[test]
    fn the_four_header_buttons_sit_in_one_row_without_touching() {
        for dpi in [96, 144, 192] {
            let g = geometry(6, 0, dpi, false);
            let row = [g.gear_btn, g.chart_btn, g.pick_btn, g.btn];
            for pair in row.windows(2) {
                assert!(pair[0].right < pair[1].left, "dpi {dpi}: buttons overlap or touch");
            }
            assert!(row.iter().all(|b| b.top == g.btn.top && b.bottom == g.btn.bottom), "one row");
            assert!(g.gear_btn.left > 0, "the leftmost button is on the panel");
        }
    }

    #[test]
    fn only_the_locked_cards_open_button_is_a_target() {
        let g = geometry(6, 0, 96, false);
        let card = card_rect(&g, 2);
        let btn = open_btn_rect(card, 96);
        let (cx, cy) = ((btn.left + btn.right) / 2, (btn.top + btn.bottom) / 2);
        // The button lives inside its own card, in the corner, clear of the price below it.
        assert!(btn.left > card.left + 100 && btn.right <= card.right && btn.top >= card.top && btn.bottom <= card.top + 26);
        assert!(open_btn_at(&g, Some(2), 96, cx, cy));
        // The same pixel on an unlocked card, or with nothing locked, is just a card.
        assert!(!open_btn_at(&g, Some(1), 96, cx, cy));
        assert!(!open_btn_at(&g, None, 96, cx, cy));
        // And the card is still a card around it: a click beside the button locks, it does not open.
        assert_eq!(card_at(&g, 6, card.left + 20, card.top + 90), Some(2));
        assert!(!open_btn_at(&g, Some(2), 96, card.left + 20, card.top + 90));
    }
}
