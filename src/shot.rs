//! The screenshot renderer: every theme in `theme::ALL`, painted by the real `panel::draw` into an
//! off-screen bitmap and written as a PNG.
//!
//! This exists because the panel's look cannot be unit-tested — a test can check that a card's rect
//! does not overlap its labels, not that the result is worth looking at. Rendering through the same
//! painter the window uses means a screenshot cannot flatter the app: if the gradients, the
//! crosshair, the range bar or the bitmap faces are wrong, they are wrong here too.
//!
//! Behind the `shots` feature, which no binary enables, so neither shipped exe links the PNG
//! encoder:
//!
//! ```text
//! cargo run --release --example shots --features shots -- shots/
//! ```

use crate::config::Row;
use crate::model::{Bar, Board, Quote};
use crate::panel::{self, Chart, Fonts, History, Scene};
use crate::ranges::{Range, Status};
use crate::theme::{self, Gradient, Theme};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Graphics::Gdi::*;

/// Six symbols, because six is two rows of three and shows the panel at the size most people run.
const ROWS: usize = 6;

fn row(label: &str, symbol: &str, digits: u32) -> Row {
    Row { label: label.into(), symbol: symbol.into(), digits, name: String::new(), strip: true }
}

/// Plausible values rather than the same number six times: a card with no 52-week range, one with a
/// volume and one without, and a delayed quote among live ones, so the screenshot exercises the
/// layout's awkward cases instead of its easy one.
type QuoteSpec<'a> = (f64, f64, f64, f64, f64, Option<f64>, (f64, f64), &'a str, bool);

fn quote(row: &Row, spec: QuoteSpec<'_>, now: f64) -> Quote {
    let (price, pct, prev, high, low, vol, yr, date, realtime) = spec;
    Quote {
        symbol: row.symbol.clone(),
        source: "",
        received_at: None,
        name: row.name.clone(),
        price,
        change_pct: Some(pct),
        prev_close: Some(prev),
        ts: Some(if realtime { now - 4.0 } else { now - 612.0 }),
        realtime,
        open: Some(prev),
        high: Some(high),
        low: Some(low),
        volume: vol,
        yr_hi: Some(yr.1),
        yr_lo: Some(yr.0),
        yr_hi_date: Some(date.into()),
        currency: "USD".into(),
    }
}

fn board(now: f64) -> Board {
    let rows = vec![
        row("XAU", "XAU=", 2),
        row("XAG", "XAG=", 3),
        row("BTC", "BTC.CB=", 0),
        row("DOW", "@DJ.1", 0),
        row("S&P", "@SP.1", 2),
        row("NAS", "@ND.1", 2),
    ];
    let spec: [QuoteSpec<'_>; ROWS] = [
        (4155.24, 0.62, 4130.60, 4190.00, 4102.50, None, (3940.0, 4381.0), "2026-09-22", true),
        (56.590, -0.31, 56.766, 57.100, 56.020, None, (38.20, 58.90), "2025-10-30", true),
        (112_480.0, 1.94, 110_340.0, 113_200.0, 109_800.0, Some(24.8e9), (74_200.0, 126_500.0), "2025-10-06", true),
        (52_048.0, -0.42, 52_267.0, 52_410.0, 51_980.0, Some(312e6), (39_800.0, 53_120.0), "2026-09-18", false),
        (6712.30, 0.18, 6700.20, 6733.00, 6681.00, Some(1.42e9), (4810.0, 6802.0), "2026-10-08", false),
        (29_801.50, 0.55, 29_638.00, 29_910.00, 29_600.00, Some(612e6), (18_200.0, 30_120.0), "2026-10-14", false),
    ];
    let mut b = Board::new(&rows);
    b.link = crate::model::Link::Partial;
    b.note.clear();
    b.cells = rows
        .iter()
        .zip(spec)
        .map(|(r, s)| crate::model::Cell {
            label: r.label.clone(),
            digits: r.digits,
            quote: Some(quote(r, s, now)),
            stale: false,
        })
        .collect();
    // One hour of 1-minute bars, ending exactly at the quote, in a deterministic wobble: the same
    // screenshot every run, so a change in the image is a change in the code.
    b.backfill = spec
        .iter()
        .enumerate()
        .map(|(k, &(price, ..))| {
            let n = 61;
            let start = price * (1.0 - (k as f64 * 0.003 - 0.006));
            (0..n)
                .map(|i| {
                    let f = i as f64 / (n - 1) as f64;
                    let wob = ((i as f64 * 0.7) + k as f64 * 2.3).sin() * 0.55
                        + ((i as f64 * 1.9) + k as f64).sin() * 0.25;
                    let v = start + (price - start) * f + price * wob * 0.0009;
                    // The bar closing now is the current price, so the line ends on the number.
                    let v = if i + 1 == n { price } else { v };
                    (now - 3600.0 + i as f64 * 60.0, v)
                })
                .collect()
        })
        .collect();
    b.backfill_gen = 1;
    b
}

/// Paint one theme into a top-down BGRA buffer.
fn render(t: &Theme, phase: f64, now: f64, b: &Board, h: &History, dpi: u32) -> (u32, u32, Vec<u8>) {
    let g = panel::geometry(b.cells.len(), 0, dpi, false);
    let (w, hh) = (g.w, g.h);
    let fonts = Fonts::new(theme::font_of(t, None), dpi, t.is_pixel());
    let grad = Gradient::new(t);
    // Stills never draw the pane, so the cell stays empty.
    let layout = std::cell::Cell::new(None);
    let sc = Scene {
        title: "MH Ticker Ultra",
        board: b,
        hist: h,
        fonts: &fonts,
        theme: t,
        grad: &grad,
        dpi,
        now,
        btn_hover: false,
        pick_hover: false,
        chart_hover: false,
        gear_hover: false,
        open_hover: false,
        // Nothing is locked in a still, and the settings button is chrome rather than palette: the
        // catalogue pictures stay what they were, byte for byte.
        select: false,
        keys_hint: false,
        selected: None,
        hover: None,
        chart_at: None,
        layout: &layout,
        picker: None,
        picker_scroll: 0,
        picker_btn: true,
        // Stills are the resting panel: the chart is a live pane, and a shot of it belongs to the
        // window test, not to the theme catalogue.
        chart: None,
        chart_btn: false,
        settings_btn: false,
        alert: None,
        alert_dots: &[],
        phase,
    };
    capture(w, hh, &sc)
}

/// Paint a scene into a top-down RGBA buffer.
fn capture(w: i32, hh: i32, sc: &Scene) -> (u32, u32, Vec<u8>) {
    capture_with(w, hh, |dc| panel::draw(dc, w, hh, sc))
}

/// Paint whatever `draw` paints on a `w` x `hh` bitmap into a top-down RGBA buffer.
pub(crate) fn capture_with(w: i32, hh: i32, draw: impl FnOnce(HDC)) -> (u32, u32, Vec<u8>) {
    let mut px = vec![0u8; (w * hh * 4) as usize];
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, hh);
        let old = SelectObject(dc, bmp);
        draw(dc);
        // `GetDIBits` refuses a bitmap that is still selected into a DC.
        SelectObject(dc, old);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -hh; // top-down, so row 0 is the top
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        GetDIBits(dc, bmp, 0, hh as u32, px.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
    // GDI hands back BGRA and the encoder wants RGBA. Swapping in place on 285k pixels is cheaper
    // than a second buffer, and the source is dropped right after. The alpha byte has to be forced
    // opaque: a 32-bit `BI_RGB` DIB leaves it zero, and an RGBA PNG with alpha 0 renders as an
    // empty rectangle.
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        p[3] = 255;
    }
    (w as u32, hh as u32, px)
}

fn write_png(path: &Path, w: u32, h: u32, px: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| format!("{}: {e}", path.display()))?;
    w.write_image_data(px).map_err(|e| format!("{}: {e}", path.display()))
}


/// `write_chart` by palette name: the example should not need `theme` to be public.
pub fn write_chart_named(dir: &Path, name: &str, dpi: u32, w: i32) -> Result<PathBuf, String> {
    write_chart_named_at(dir, name, dpi, w, None, None, false)
}

/// `write_chart_named` with card `hover` locked, so a review can see the pane, the border and the
/// Open button on any of the six, and candle `bar` hovered (an index into the candles as drawn, 0 at
/// the left; clamped to the last), so the crosshair, the price tag and the legend can be reviewed.
/// The pointer sits at the candle's close.
pub fn write_chart_named_at(
    dir: &Path,
    name: &str,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
    bar: Option<usize>,
    refs: bool,
) -> Result<PathBuf, String> {
    write_chart_named_range(dir, name, dpi, w, hover, bar, refs, None, false)
}

/// `write_chart_named_at` for a chart range. `range` is `1D`, `5D`, `1M`, `6M`, `1Y` or `5Y`
/// (`None`: the pane exactly as it was before ranges, with no tabs). A range other than 1D charts a
/// deterministic synthetic series with the shape of that file (1-minute bars over five sessions,
/// daily bars, weekly bars) and ignores `refs`. `volume` gives the charted bars a synthetic volume,
/// which turns the volume strip on.
#[allow(clippy::too_many_arguments)]
pub fn write_chart_named_range(
    dir: &Path,
    name: &str,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
    bar: Option<usize>,
    refs: bool,
    range: Option<&str>,
    volume: bool,
) -> Result<PathBuf, String> {
    write_chart_named_levels(dir, name, dpi, w, hover, bar, refs, range, volume, false)
}

/// `write_chart_named_range` with alert levels on the charted symbol when `levels` is true: one inside
/// the price window (70 % of the way up the bars' range) and one above it (half the range over the
/// high), so both the dashed line and the edge marker can be reviewed. False is
/// `write_chart_named_range` exactly.
#[allow(clippy::too_many_arguments)]
pub fn write_chart_named_levels(
    dir: &Path,
    name: &str,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
    bar: Option<usize>,
    refs: bool,
    range: Option<&str>,
    volume: bool,
    levels: bool,
) -> Result<PathBuf, String> {
    let t = theme::ALL.iter().find(|t| t.name == name).ok_or_else(|| format!("no theme named {name}"))?;
    let range = match range {
        Some(r) => Some(Range::parse(r).ok_or_else(|| format!("no range named {r} (1D 5D 1M 6M 1Y 5Y)"))?),
        None => None,
    };
    write_chart_fixture(dir, t, dpi, w, hover, bar, refs, range, volume, levels, "")
}

/// W3's pictures, written into `dir` beside the others and never over them: the pane on 5Y with alert
/// levels, W1's two header notes (`as of`, `check dates`) and L1's header change and high/low marks
/// (`w3-5y.png`, 1,488 px, 96 dpi, the second card locked); and the strip of the
/// same 24 symbols with 6 pinned (`w3-strip-24-pinned-6.png`, two rows in a 1,800 px gap), where only
/// the six pinned cards are laid out. Returns how many files were written.
pub fn write_w3(dir: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = 1_792_052_981.0;
    let t = theme::ALL.iter().find(|t| t.name == "midnight").unwrap_or(&theme::ALL[0]);
    let scratch = dir.join(".scratch");
    let rg = Some(Range::Y5);
    let made = write_chart_fixture(&scratch, t, 96, 1488, Some(1), None, false, rg, true, true, "as of 14:05 \u{b7} check dates")?;
    std::fs::rename(&made, dir.join("w3-5y.png")).map_err(|e| format!("w3-5y.png: {e}"))?;
    let _ = std::fs::remove_dir(&scratch);
    let b24 = board_n(now, 24);
    // Six pinned of the 24, taken through the same function the strip uses (`config::strip_rows`).
    let mut cfg = crate::config::Config { rows: b24.rows.clone(), ..Default::default() };
    for (i, r) in cfg.rows.iter_mut().enumerate() {
        r.strip = [0, 2, 4, 8, 13, 20].contains(&i);
    }
    let pinned: Vec<crate::model::Cell> = crate::config::strip_rows(&cfg).into_iter().map(|i| b24.cells[i].clone()).collect();
    let (w, h, px) = crate::ui::strip_still(&pinned, t, 96, 2, 1_800);
    write_png(&dir.join("w3-strip-24-pinned-6.png"), w, h, &px)?;
    Ok(2)
}

/// The fixture's clock: UTC-4, a US equity day in summer.
const TZ_S: i32 = -4 * 3_600;

/// One rendered panel with the candle pane open, from deterministic bars.
///
/// This exists because the live feed can be closed (a weekend) or blocked (rate limited), and the
/// pane still has to be reviewable. The bars are a seeded walk, not a market: the point is the
/// painter, the axes and the layout. The six cards are the catalogue's own fixture, so the pane is
/// always seen beside the prices it will be read with.
pub fn write_chart(dir: &Path, t: &Theme, dpi: u32, w: i32) -> Result<PathBuf, String> {
    write_chart_at(dir, t, dpi, w, None, None)
}

/// `write_chart` with a card locked and, optionally, a candle hovered.
pub fn write_chart_at(
    dir: &Path,
    t: &Theme,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
    bar: Option<usize>,
) -> Result<PathBuf, String> {
    write_chart_fixture(dir, t, dpi, w, hover, bar, false, None, false, false, "")
}

/// The fixture behind `write_chart_at` and `write_chart_named_at`. With `refs`, the pane alone gets
/// the reference lines and one session break, so both can be reviewed from a picture: the previous
/// close at the midpoint of the day's range, the day's high and low at its extremes, and about forty
/// minutes cut from the middle of the bars. The cards keep the untouched bars.
#[allow(clippy::too_many_arguments)]
fn write_chart_fixture(
    dir: &Path,
    t: &Theme,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
    bar: Option<usize>,
    refs: bool,
    range: Option<Range>,
    volume: bool,
    levels: bool,
    note: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = 1_792_052_981.0;
    let mut b = board(now);
    // 390 one-minute bars is 6.5 h, the length of a US equity day.
    let (n, t0) = (390, now - 390.0 * 60.0);
    let bars: Vec<Vec<Bar>> = b
        .cells
        .iter()
        .enumerate()
        .map(|(i, c)| walk(c.quote.as_ref().map(|q| q.price).unwrap_or(100.0), n, i as u64 + 1))
        .collect();
    b.bars_t0 = vec![t0; b.cells.len()];
    b.bars = bars.iter().map(|v| std::sync::Arc::from(v.as_slice())).collect();
    let row = hover.unwrap_or_else(|| b.cells.iter().position(|c| c.label == "S&P").unwrap_or(0));
    let label = b.cells.get(row).map(|c| c.label.clone()).unwrap_or_default();
    let digits = b.cells.get(row).map(|c| c.digits).unwrap_or(2);
    let last = b.cells.get(row).and_then(|c| c.quote.as_ref()).map(|q| q.price);
    let mut charted = bars.get(row).cloned().unwrap_or_default();
    let quote = b.cells.get(row).and_then(|c| c.quote.as_ref());
    let (mut prev, mut day_hi, mut day_lo, change_pct) = (
        quote.and_then(|q| q.prev_close),
        quote.and_then(|q| q.high),
        quote.and_then(|q| q.low),
        quote.and_then(|q| q.change_pct),
    );
    if refs && charted.len() > 200 {
        let mid = charted.len() / 2;
        charted.drain(mid - 20..mid + 20);
        let hi = charted.iter().fold(f32::MIN, |m, b| m.max(b.h)) as f64;
        let lo = charted.iter().fold(f32::MAX, |m, b| m.min(b.l)) as f64;
        prev = Some((hi + lo) / 2.0);
        day_hi = Some(hi);
        day_lo = Some(lo);
    }
    // A range: the pane charts a synthetic series with the shape of that range's file (the cards keep
    // the day). With no range the pane has no tabs and is the pane from before ranges.
    let rg = range.unwrap_or(Range::D1);
    let (mut chart_t0, mut bar_s) = (t0, 60.0);
    if volume && rg == Range::D1 {
        add_volume(&mut charted, row as u64 + 1);
    }
    if rg != Range::D1 {
        let (st, sbars, sbs) = long_series(rg, now, last.unwrap_or(100.0), row as u64 + 1, volume);
        (chart_t0, charted, bar_s) = (st, sbars, sbs);
        (prev, day_hi, day_lo) = (None, None, None);
    }
    let coarsest = if rg == Range::D1 { 3_600.0 } else { crate::chartview::MONTH_S };
    // Alert levels for the charted symbol, placed from the bars so they land where the doc says.
    let alert_levels: Vec<f64> = if levels && !charted.is_empty() {
        let hi = charted.iter().fold(f32::MIN, |m, b| m.max(b.h)) as f64;
        let lo = charted.iter().fold(f32::MAX, |m, b| m.min(b.l)) as f64;
        vec![lo + (hi - lo) * 0.7, hi + (hi - lo) * 0.5]
    } else {
        Vec::new()
    };
    // The sparklines seed from the hour of bars, exactly as they do on a live board.
    let mut hist = History::new();
    hist.record(&b, now);
    let g = panel::geometry_in(w, b.cells.len(), 0, dpi, false, true);
    let fonts = Fonts::new(theme::font_of(t, None), dpi, t.is_pixel());
    let grad = Gradient::new(t);
    let layout = std::cell::Cell::new(None);
    let mut sc = Scene {
        title: "MH Ticker Ultra",
        board: &b,
        hist: &hist,
        fonts: &fonts,
        theme: t,
        grad: &grad,
        dpi,
        now,
        btn_hover: false,
        pick_hover: false,
        chart_hover: false,
        gear_hover: false,
        open_hover: false,
        // The pane charts the locked card, so the fixture locks it: the border and the Open button
        // are part of what this picture is for.
        select: true,
        keys_hint: false,
        selected: Some(row),
        hover: hover.map(|row| panel::Hover { row, t: snap_t(now) }),
        chart_at: None,
        layout: &layout,
        picker: None,
        picker_scroll: 0,
        picker_btn: true,
        // A fixed zone (UTC-4, a US equity day in summer) so the picture is the same on every machine.
        chart: Some(Chart {
            label: &label,
            digits,
            t0: chart_t0,
            bars: &charted,
            last,
            unavailable: false,
            range: rg,
            status: Status::Ready,
            bar_s,
            tabs: range.is_some(),
            zone: crate::chartview::Zone::fixed(TZ_S),
            tz_label: "local",
            note,
            prev,
            day_hi,
            day_lo,
            change_pct: change_pct.filter(|_| rg == Range::D1),
            levels: &alert_levels,
        }),
        chart_btn: true,
        settings_btn: true,
        alert: None,
        alert_dots: &[],
        phase: 0.0,
    };
    if let Some(bar) = bar {
        // The pointer comes from the layout a first paint stored, exactly as the app gets it from the
        // last frame: paint once, read the layout, aim at the candle's close, paint again.
        capture(g.w, g.h, &sc);
        if let Some(lay) = layout.get() {
            let i = bar.min(lay.bars.saturating_sub(1));
            let (drawn, _) = crate::chartview::fitted_to(&charted, lay.plot.right - lay.plot.left, chart_t0, TZ_S, bar_s, coarsest);
            let drawn = crate::chartview::newest_that_fit(drawn, lay.plot.right - lay.plot.left, sc.dpi);
            let close = drawn.get(i).map_or(last.unwrap_or(0.0), |b| b.c as f64);
            let close = lay.y_of(close).clamp(lay.plot.top, lay.plot.bottom - 1);
            sc.chart_at = lay.pointer_at(lay.x_of(i), close, 2);
        }
    }
    let (pw, ph, px8) = capture(g.w, g.h, &sc);
    let path = dir.join(format!("chart-{}.png", t.name));
    write_png(&path, pw, ph, &px8)?;
    Ok(path)
}

/// The pointer's moment on a card, snapped to five minutes the way the live hit-test does: the pane
/// reads it as the time under the cursor, and a fixture should not disagree with the app.
fn snap_t(t: f64) -> f64 {
    (t / 5.0).round() * 5.0
}

/// A session that ends exactly on `last`: a drift, a midday sell-off, a recovery, at a scale that
/// suits the price it is charting. Deterministic, so the same picture every run.
fn walk(last: f64, n: usize, salt: u64) -> Vec<Bar> {
    walk_amp(last, n, salt, 1.0)
}

/// `walk` with the step scaled by `k`: a daily or weekly candle moves a lot more than a minute does.
fn walk_amp(last: f64, n: usize, salt: u64, k: f64) -> Vec<Bar> {
    let mut seed = 0x2545_f491_4f6c_dd1d ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let amp = last.abs() * 0.0004 * k;
    let mut close = Vec::with_capacity(n);
    let mut px = 0.0;
    for i in 0..n {
        let drift = if (n * 2 / 5..n * 3 / 5).contains(&i) { -0.8 } else { 0.3 };
        px += noise(&mut seed) * amp + drift * amp * 0.4;
        close.push(px);
    }
    let shift = last - close.last().copied().unwrap_or(0.0);
    let mut bars = Vec::with_capacity(n);
    for i in 0..n {
        let c = close[i] + shift;
        let o = if i == 0 { c - noise(&mut seed) * amp } else { close[i - 1] + shift };
        let w = amp * 0.35 * noise(&mut seed).abs();
        bars.push(Bar {
            dt: (i * 60) as f32,
            o: o as f32,
            h: (o.max(c) + w) as f32,
            l: (o.min(c) - w) as f32,
            c: c as f32,
            v: 0.0,
        });
    }
    bars
}

/// A synthetic volume, bigger in the first and last quarter of the bars (an open and a close), in
/// units that read as thousands of shares. Deterministic.
fn add_volume(bars: &mut [Bar], salt: u64) {
    let mut seed = 0x9e37_79b9_7f4a_7c15 ^ salt.wrapping_mul(0x2545_f491_4f6c_dd1d);
    let n = bars.len().max(1);
    for (i, b) in bars.iter_mut().enumerate() {
        let edge = if i < n / 4 || i > n * 3 / 4 { 2.0 } else { 1.0 };
        b.v = (40_000.0 * edge * (1.0 + noise(&mut seed).abs() * 2.0)) as f32;
    }
}

/// A series shaped like `range`'s file, ending at `last`, as `(t0, bars, bar_s)`: five 6.5-hour
/// sessions of one-minute bars (5D), or daily bars on weekdays (1M and 6M, from the year the file
/// carries), or weekly bars (5Y), trimmed to the range's span exactly as `Series::window` does. Bars
/// are stamped at their START, at 13:30 UTC (an equity open), and `dt` counts from the first.
fn long_series(range: Range, now: f64, last: f64, salt: u64, volume: bool) -> (f64, Vec<Bar>, f64) {
    const DAY: f64 = 86_400.0;
    let today = (now / DAY).floor() as i64;
    // Day 0 (1970-01-01) was a Thursday: 0 = Sunday .. 6 = Saturday.
    let weekday = |d: i64| (d + 4).rem_euclid(7);
    let open = |d: i64| d as f64 * DAY + 13.5 * 3_600.0;
    let (starts, bar_s, per, k): (Vec<f64>, f64, usize, f64) = match range {
        Range::D1 | Range::D5 => {
            let days: Vec<i64> = (0..30).map(|i| today - i).filter(|d| (1..=5).contains(&weekday(*d)) && open(*d) + 390.0 * 60.0 <= now).take(5).collect();
            (days.into_iter().rev().map(open).collect(), 60.0, 390, 1.0)
        }
        Range::M1 | Range::M6 | Range::Y1 => {
            let days: Vec<i64> = (1..400).map(|i| today - i).filter(|d| (1..=5).contains(&weekday(*d))).take(260).collect();
            (days.into_iter().rev().map(open).collect(), DAY, 1, 10.0)
        }
        Range::Y5 => {
            let mondays: Vec<i64> = (1..1_900).map(|i| today - i).filter(|d| weekday(*d) == 1).take(261).collect();
            (mondays.into_iter().rev().map(open).collect(), 7.0 * DAY, 1, 25.0)
        }
    };
    let n = starts.len() * per;
    let walked = walk_amp(last, n, salt, k);
    let mut bars: Vec<Bar> = Vec::with_capacity(n);
    for (i, mut b) in walked.into_iter().enumerate() {
        // Minute bars restart at each session's open; daily and weekly bars sit on their own start.
        b.dt = (starts[i / per] + (i % per) as f64 * bar_s) as f32;
        bars.push(b);
    }
    let keep = bars.iter().position(|b| b.dt as f64 >= now - range.span_s()).unwrap_or(bars.len());
    bars.drain(..keep);
    let t0 = bars.first().map_or(now, |b| b.dt as f64);
    for b in &mut bars {
        b.dt = (b.dt as f64 - t0) as f32;
    }
    if volume {
        add_volume(&mut bars, salt);
    }
    (t0, bars, bar_s)
}

/// xorshift64 -> [-1, 1). No dependency, and the same numbers on every platform.
fn noise(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    ((*seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 2.0
}

/// Render every theme into `dir`. Animated themes get three frames a third of a cycle apart, since
/// one PNG cannot show a drift.
pub fn write_all(dir: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = 1_792_052_981.0;
    let b = board(now);
    let mut hist = History::new();
    hist.record(&b, now);
    let mut n = 0;
    for t in &theme::ALL {
        // Every palette moves now, but only the animated tier is *about* the motion, so only it
        // gets the three frames: a screenshot set that tripled in size would stop being a
        // catalogue and start being a film strip.
        let phases: &[f64] = if t.kind == theme::Kind::Animated { &[0.0, 0.34, 0.67] } else { &[0.0] };
        for (i, &phase) in phases.iter().enumerate() {
            let name = if i == 0 { format!("{}.png", t.name) } else { format!("{}-{}.png", t.name, i + 1) };
            let path = dir.join(name);
            let (w, h, px) = render(t, phase, now, &b, &hist, 96);
            write_png(&path, w, h, &px)?;
            println!("{:>4}x{:<4} {}  ({}, {})", w, h, path.display(), t.kind.label(), t.font);
            n += 1;
        }
    }
    Ok(n)
}

/// The labels of the 24-symbol board, in the order a user might add them.
const LABELS_24: [&str; 24] = [
    "XAU", "XAG", "BTC", "ETH", "DOW", "S&P", "NAS", "RUT", "VIX", "OIL", "GAS", "EUR", "GBP", "JPY", "TNX", "AAPL", "MSFT", "NVDA",
    "TSLA", "AMZN", "META", "GOOG", "AMD", "INTC",
];

/// `n` (at most 24) symbols: the fixture's six cycled, each round a little dearer, with one stale
/// cell and one with no quote among them, so the wide pictures show the awkward cards as well.
fn board_n(now: f64, n: usize) -> Board {
    let base = board(now);
    let n = n.clamp(1, LABELS_24.len());
    let rows: Vec<Row> = (0..n).map(|i| row(LABELS_24[i], &format!("SYM{i}="), base.cells[i % 6].digits)).collect();
    let mut b = Board::new(&rows);
    b.link = crate::model::Link::Partial;
    b.note.clear();
    b.cells = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (k, scale) = (i % 6, 1.0 + (i / 6) as f64 * 0.07 - (i % 5) as f64 * 0.011);
            let mut cell = base.cells[k].clone();
            cell.label = r.label.clone();
            if let Some(q) = cell.quote.as_mut() {
                q.symbol = r.symbol.clone();
                q.price *= scale;
                q.prev_close = q.prev_close.map(|v| v * scale);
                q.open = q.open.map(|v| v * scale);
                q.high = q.high.map(|v| v * scale);
                q.low = q.low.map(|v| v * scale);
                q.yr_hi = q.yr_hi.map(|v| v * scale);
                q.yr_lo = q.yr_lo.map(|v| v * scale);
                q.change_pct = q.change_pct.map(|p| p * (1.0 + (i % 4) as f64 * 0.8) * if i % 3 == 1 { -1.0 } else { 1.0 });
            }
            cell.stale = i == 7;
            if i == 13 {
                cell.quote = None;
            }
            cell
        })
        .collect();
    b.backfill = (0..n).map(|i| base.backfill[i % 6].iter().map(|&(t, v)| (t, v * (1.0 + (i / 6) as f64 * 0.07 - (i % 5) as f64 * 0.011))).collect()).collect();
    b.backfill_gen = 1;
    b
}

/// A chart still written under `file` in `dir`: the chart fixture renders to `chart-<theme>.png` in a
/// scratch folder, which is then moved to its own name.
#[allow(clippy::too_many_arguments)]
fn chart_still(dir: &Path, file: &str, dpi: u32, w: i32, bar: Option<usize>, range: &str) -> Result<(), String> {
    let scratch = dir.join(".scratch");
    let made = write_chart_named_levels(&scratch, "midnight", dpi, w, None, bar, false, Some(range), true, false)?;
    std::fs::rename(&made, dir.join(file)).map_err(|e| format!("{file}: {e}"))?;
    let _ = std::fs::remove_dir(&scratch);
    Ok(())
}

/// The wider set of pictures (W1, H14), written into `dir` beside the theme catalogue and never over
/// it: 24 cards; 24 symbols on 1, 2 and 3 strip rows in a 1,800 px taskbar gap; the narrowest panel
/// with the pane open; 192 DPI cards and pane; and the pane on each of the six ranges. All in the
/// first palette, at the catalogue's fixture time. Returns how many files were written.
pub fn write_wide(dir: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = 1_792_052_981.0;
    let t = theme::ALL.iter().find(|t| t.name == "midnight").unwrap_or(&theme::ALL[0]);
    let mut n = 0;
    let put = |n: &mut usize, file: &str, (w, h, px): (u32, u32, Vec<u8>)| -> Result<(), String> {
        write_png(&dir.join(file), w, h, &px)?;
        println!("{w:>4}x{h:<4} {file}");
        *n += 1;
        Ok(())
    };
    let b24 = board_n(now, 24);
    let mut hist = History::new();
    hist.record(&b24, now);
    put(&mut n, "wide-cards-24.png", render(t, 0.0, now, &b24, &hist, 96))?;
    for rows in 1..=3usize {
        put(&mut n, &format!("wide-strip-24-rows{rows}.png"), crate::ui::strip_still(&b24.cells, t, 96, rows, 1_800))?;
    }
    let b6 = board(now);
    let mut hist6 = History::new();
    hist6.record(&b6, now);
    put(&mut n, "wide-192dpi-cards.png", render(t, 0.0, now, &b6, &hist6, 192))?;
    put(&mut n, "wide-192dpi-strip-24-rows2.png", crate::ui::strip_still(&b24.cells, t, 192, 2, 3_600))?;
    chart_still(dir, "wide-pane-narrow.png", 96, panel::minimum(6, 0, 96, false, true).0, Some(40), "5D")?;
    chart_still(dir, "wide-192dpi-pane.png", 192, 1488 * 2, Some(40), "1D")?;
    n += 2;
    for range in ["1D", "5D", "1M", "6M", "1Y", "5Y"] {
        chart_still(dir, &format!("wide-range-{}.png", range.to_lowercase()), 96, 1488, None, range)?;
        n += 1;
    }
    Ok(n)
}

/// The chart fixture as an owned value, so a test or a timing loop can build a scene, change one hover
/// part, and draw again. Same data as `write_chart_fixture` with a locked card: the S&P row, 1488 px
/// wide at 96 dpi, the first theme, the pane open on `range` (`None` is the closed panel).
pub struct Fixture {
    board: Board,
    hist: History,
    fonts: Fonts,
    grad: Gradient,
    layout: std::cell::Cell<Option<crate::chartview::Layout>>,
    charted: Vec<Bar>,
    t0: f64,
    bar_s: f64,
    label: String,
    last: Option<f64>,
    range: Option<Range>,
    row: usize,
    now: f64,
    /// The pane's note line (`Chart::note`); empty for every existing picture.
    pub note: String,
    pub dpi: u32,
    pub w: i32,
    pub h: i32,
}

impl Fixture {
    pub fn new(range: Option<&str>) -> Result<Fixture, String> {
        Fixture::with(range, 96, 1488, None)
    }

    /// The fixture at `dpi`, `w` px wide, with the panel face `face` (any installed family name)
    /// instead of the theme's own. `w` below the content's minimum is raised to it: the narrowest
    /// panel with the pane open is `panel::minimum(..).0`.
    pub fn with(range: Option<&str>, dpi: u32, w: i32, face: Option<&str>) -> Result<Fixture, String> {
        let t = &theme::ALL[0];
        let rg = match range {
            Some(r) => Some(Range::parse(r).ok_or_else(|| format!("no range named {r}"))?),
            None => None,
        };
        let now = 1_792_052_981.0;
        let mut b = board(now);
        let (n, t0) = (390, now - 390.0 * 60.0);
        let bars: Vec<Vec<Bar>> = b.cells.iter().enumerate().map(|(i, c)| walk(c.quote.as_ref().map(|q| q.price).unwrap_or(100.0), n, i as u64 + 1)).collect();
        b.bars_t0 = vec![t0; b.cells.len()];
        b.bars = bars.iter().map(|v| std::sync::Arc::from(v.as_slice())).collect();
        let row = b.cells.iter().position(|c| c.label == "S&P").unwrap_or(0);
        let label = b.cells[row].label.clone();
        let last = b.cells[row].quote.as_ref().map(|q| q.price);
        let (mut chart_t0, mut charted, mut bar_s) = (t0, bars[row].clone(), 60.0);
        if let Some(rg) = rg.filter(|r| *r != Range::D1) {
            (chart_t0, charted, bar_s) = long_series(rg, now, last.unwrap_or(100.0), row as u64 + 1, true);
        }
        let mut hist = History::new();
        hist.record(&b, now);
        let g = panel::geometry_in(w, b.cells.len(), 0, dpi, false, rg.is_some());
        Ok(Fixture {
            hist,
            fonts: Fonts::new(face.unwrap_or_else(|| theme::font_of(t, None)), dpi, t.is_pixel()),
            grad: Gradient::new(t),
            layout: std::cell::Cell::new(None),
            board: b,
            charted,
            t0: chart_t0,
            bar_s,
            label,
            last,
            range: rg,
            row,
            now,
            note: String::new(),
            dpi,
            w: g.w,
            h: g.h,
        })
    }

    /// The panel's geometry as the app computes it for this fixture.
    pub fn geo(&self) -> panel::Geo {
        panel::geometry_in(self.w, self.board.cells.len(), 0, self.dpi, false, self.range.is_some())
    }

    /// The scene with the hover state `hot` applied.
    pub fn scene<'a>(&'a self, hot: &panel::Hot) -> Scene<'a> {
        let rg = self.range.unwrap_or(Range::D1);
        Scene {
            title: "MH Ticker Ultra",
            board: &self.board,
            hist: &self.hist,
            fonts: &self.fonts,
            theme: &theme::ALL[0],
            grad: &self.grad,
            dpi: self.dpi,
            now: self.now,
            btn_hover: hot.btn,
            pick_hover: hot.pick,
            chart_hover: hot.chart_btn,
            gear_hover: hot.gear,
            open_hover: hot.open,
            select: true,
            keys_hint: false,
            selected: Some(self.row),
            hover: hot.card,
            chart_at: hot.aim,
            layout: &self.layout,
            picker: None,
            picker_scroll: 0,
            picker_btn: true,
            chart: self.range.map(|_| Chart {
                label: &self.label,
                digits: 2,
                t0: self.t0,
                bars: &self.charted,
                last: self.last,
                unavailable: false,
                range: rg,
                status: Status::Ready,
                bar_s: self.bar_s,
                tabs: true,
                zone: crate::chartview::Zone::fixed(TZ_S),
                tz_label: "local",
                note: &self.note,
                prev: None,
                day_hi: None,
                day_lo: None,
                change_pct: None,
                levels: &[],
            }),
            chart_btn: true,
            settings_btn: true,
            alert: None,
            alert_dots: &[],
            phase: 0.0,
        }
    }

    /// The pane's rectangle from the last frame drawn (the layout the pointer is answered from).
    pub fn pane(&self) -> Option<windows_sys::Win32::Foundation::RECT> {
        self.layout.get().map(|l| windows_sys::Win32::Foundation::RECT { left: l.pane.left, top: l.pane.top, right: l.pane.right, bottom: l.pane.bottom })
    }

    /// A pointer aimed at the middle candle's middle row, from the layout the last frame stored.
    pub fn aim_middle(&self) -> Option<crate::chartview::Pointer> {
        let lay = self.layout.get()?;
        lay.pointer_at(lay.x_of(lay.bars / 2), (lay.plot.top + lay.plot.bottom) / 2, 2)
    }

    /// One frame as RGBA pixels.
    pub fn frame(&self, hot: &panel::Hot) -> Vec<u8> {
        capture(self.w, self.h, &self.scene(hot)).2
    }

    pub fn selected(&self) -> usize {
        self.row
    }
}

/// What `examples/perf.rs` times: the milliseconds one frame of the panel takes to draw (the same
/// `panel::draw` the app calls, into an off-screen bitmap, no PNG), with the candle pane open on
/// `range` (`1D`..`5Y`), or closed when `range` is `None`. `hovered` aims the crosshair at a candle,
/// as a hover repaint does; `clipped` clips the back buffer to the pane and copies only the pane, as
/// the region a hover invalidates does. Each frame ends with the copy to the screen DC, as `paint` does.
pub fn bench_draw(range: Option<&str>, hovered: bool, clipped: bool, iters: u32) -> Result<f64, String> {
    let fx = Fixture::new(range)?;
    let none = panel::Hot::default();
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, fx.w, fx.h);
        let old = SelectObject(dc, bmp);
        panel::draw(dc, fx.w, fx.h, &fx.scene(&none));
        let hot = panel::Hot { aim: if hovered { fx.aim_middle() } else { None }, ..none };
        let sc = fx.scene(&hot);
        panel::draw(dc, fx.w, fx.h, &sc);
        if clipped {
            if let Some(p) = fx.pane() {
                let rgn = CreateRectRgn(p.left, p.top, p.right, p.bottom);
                SelectClipRgn(dc, rgn);
                DeleteObject(rgn);
            }
        }
        // The paint also copies to the window: the whole of it, or the pane when the draw is clipped.
        let part = match (clipped, fx.pane()) {
            (true, Some(p)) => p,
            _ => windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: fx.w, bottom: fx.h },
        };
        let start = std::time::Instant::now();
        for _ in 0..iters {
            panel::draw(dc, fx.w, fx.h, &sc);
            BitBlt(screen, part.left, part.top, part.right - part.left, part.bottom - part.top, dc, part.left, part.top, SRCCOPY);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / iters.max(1) as f64;
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
        Ok(ms)
    }
}

/// Microseconds one GDI text measurement takes (`GetTextExtentPoint32W` on a short string, the call
/// `panel::text` makes after every `TextOutW`), and one `TextOutW`.
pub fn bench_text(iters: u32) -> (f64, f64) {
    let t = &theme::ALL[0];
    let fonts = Fonts::new(theme::font_of(t, None), 96, t.is_pixel());
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, 400, 100);
        let old = SelectObject(dc, bmp);
        let f = SelectObject(dc, fonts.small);
        let txt: Vec<u16> = "5,432.10".encode_utf16().collect();
        let mut sz = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        let a = std::time::Instant::now();
        for _ in 0..iters {
            GetTextExtentPoint32W(dc, txt.as_ptr(), txt.len() as i32, &mut sz);
        }
        let measure = a.elapsed().as_secs_f64() * 1e6 / iters as f64;
        let a = std::time::Instant::now();
        for _ in 0..iters {
            TextOutW(dc, 4, 4, txt.as_ptr(), txt.len() as i32);
        }
        let out = a.elapsed().as_secs_f64() * 1e6 / iters as f64;
        SelectObject(dc, f);
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
        (measure, out)
    }
}

/// Microseconds one `chartview::fitted_to` takes over `n` one-minute bars into a `plot_w`-pixel
/// plot (the climb through the ladder included).
pub fn bench_fold(n: usize, plot_w: i32, iters: u32) -> f64 {
    let bars = walk(5_000.0, n, 7);
    let start = std::time::Instant::now();
    let mut keep = 0;
    for _ in 0..iters {
        keep += crate::chartview::fitted_to(&bars, plot_w, 0.0, TZ_S, 60.0, crate::chartview::MONTH_S).0.len();
    }
    std::hint::black_box(keep);
    start.elapsed().as_secs_f64() * 1e6 / iters.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::Hot;

    /// Every pixel that differs between two frames, as (x, y).
    fn diff(a: &[u8], b: &[u8], w: i32) -> Vec<(i32, i32)> {
        a.chunks_exact(4)
            .zip(b.chunks_exact(4))
            .enumerate()
            .filter(|(_, (p, q))| p != q)
            .map(|(i, _)| (i as i32 % w, i as i32 / w))
            .collect()
    }

    /// The rectangle `hot_dirty` names for a move between two hover states must hold every pixel that
    /// move changes, or a partial repaint would leave a stale mark on screen. Checked against the real
    /// painter, one hover part at a time and in pairs, with the pane open and closed.
    #[test]
    fn a_hover_change_repaints_inside_the_rectangle_it_names() {
        // The pane's note line is drawn only while no candle is hovered, so it is part of what a hover
        // change repaints: run every case with and without one.
        for (range, note) in [(None, ""), (Some("5D"), ""), (Some("5D"), "as of 14:05 \u{b7} check dates")] {
            let mut fx = Fixture::new(range).unwrap();
            fx.note = note.into();
            let g = fx.geo();
            let none = Hot::default();
            let base = fx.frame(&none);
            let sel = fx.selected();
            let at = Some(panel::Hover { row: sel, t: 1_792_052_981.0 - 1_800.0 });
            let mut cases: Vec<(&str, Hot)> = vec![
                ("minimise", Hot { btn: true, ..none }),
                ("picker", Hot { pick: true, ..none }),
                ("chart toggle", Hot { chart_btn: true, ..none }),
                ("settings", Hot { gear: true, ..none }),
                ("open button", Hot { open: true, ..none }),
                ("card crosshair", Hot { card: at, ..none }),
            ];
            if range.is_some() {
                let aim = fx.aim_middle().expect("the first frame stores the pane's layout");
                cases.push(("candle crosshair", Hot { aim: Some(aim), ..none }));
                cases.push(("candle and card", Hot { aim: Some(aim), card: at, ..none }));
            }
            for (name, hot) in cases {
                let pane = fx.pane();
                let now = fx.frame(&hot);
                let r = panel::hot_dirty(&g, 96, Some(sel), pane, &none, &hot).expect("a rectangle for this change");
                let moved = diff(&base, &now, fx.w);
                // The chart toggle is already lit while the pane is open, so hovering it changes nothing then.
                let invisible = name == "chart toggle" && range.is_some();
                assert!(invisible || !moved.is_empty(), "{name}: the change must be visible, or the case tests nothing");
                for (x, y) in &moved {
                    assert!(panel::in_rect(&r, *x, *y), "{name} ({range:?}): pixel ({x},{y}) changed outside ({},{},{},{})", r.left, r.top, r.right, r.bottom);
                }
                // And back again: leaving repaints the same place.
                let r2 = panel::hot_dirty(&g, 96, Some(sel), pane, &hot, &none).unwrap();
                assert_eq!((r.left, r.top, r.right, r.bottom), (r2.left, r2.top, r2.right, r2.bottom), "{name}: enter and leave name the same place");
                // A partial repaint is only worth having when it is partial.
                let area = ((r.right - r.left) as i64) * ((r.bottom - r.top) as i64);
                // (Two parts at once are one bounding box and can be large; a pointer is over a card or
                // the pane, so that pair is only a coverage check.)
                assert!(name.contains(" and ") || area * 2 < (fx.w as i64) * (fx.h as i64), "{name}: the rectangle should be under half the window");
            }
        }
    }

    /// J5: a card with a level set carries a dot, inside that card and nowhere else; with no alerts
    /// (an empty slice, which is what every still passes) not one pixel differs.
    #[test]
    fn a_card_with_a_level_carries_a_dot_inside_that_card_only() {
        let fx = Fixture::new(None).unwrap();
        let none = Hot::default();
        let plain = capture(fx.w, fx.h, &fx.scene(&none)).2;
        let mut sc = fx.scene(&none);
        sc.alert_dots = &[];
        assert!(diff(&plain, &capture(fx.w, fx.h, &sc).2, fx.w).is_empty(), "no alerts: the picture is unchanged");
        for card in [0usize, 1] {
            let mut dots = vec![false; fx.board.cells.len()];
            dots[card] = true;
            let mut sc = fx.scene(&none);
            sc.alert_dots = &dots;
            let marked = capture(fx.w, fx.h, &sc).2;
            let moved = diff(&plain, &marked, fx.w);
            assert!(!moved.is_empty(), "card {card}: the dot is drawn");
            let r = panel::card_rect(&fx.geo(), card);
            for (x, y) in &moved {
                assert!(panel::in_rect(&r, *x, *y), "card {card}: pixel ({x},{y}) changed outside the card");
            }
            assert!(moved.len() < 120, "card {card}: a dot, not a repaint ({} px)", moved.len());
        }
    }

    /// The note is drawn on the header's second line with no pointer over the plot, and the pointer's
    /// legend takes its place (so the note never sits under the legend).
    #[test]
    fn the_chart_note_shows_until_a_candle_is_hovered() {
        let none = Hot::default();
        let mut fx = Fixture::new(Some("5D")).unwrap();
        let plain = fx.frame(&none);
        fx.note = "check dates".into();
        let noted = fx.frame(&none);
        let moved = diff(&plain, &noted, fx.w);
        assert!(!moved.is_empty(), "the note must be visible");
        let pane = fx.pane().unwrap();
        // Everything it changed is on the header's second line, inside the pane.
        for (x, y) in &moved {
            assert!(panel::in_rect(&pane, *x, *y) && *y < pane.top + 56, "({x},{y}) outside the header line");
        }
        // Hovering a candle replaces it with the legend: the frame equals the one drawn with no note.
        let aim = fx.aim_middle().unwrap();
        let hot = Hot { aim: Some(aim), ..none };
        let with = fx.frame(&hot);
        fx.note = String::new();
        let without = fx.frame(&hot);
        assert!(diff(&with, &without, fx.w).is_empty(), "a hovered pane shows the legend, not the note");
    }

    /// The width of `s` in `font` on a scratch DC, and the height of a line of it.
    fn measure(font: HFONT, s: &str) -> (i32, i32) {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            let old = SelectObject(dc, font);
            let t: Vec<u16> = s.encode_utf16().collect();
            let mut sz = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz);
            SelectObject(dc, old);
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
            (sz.cx, sz.cy)
        }
    }

    /// The faces of the catalogue's Mono and Bitmap groups that this machine draws at a fixed pitch
    /// (an absent face is substituted by a proportional one, which fails the test of `iiii` against
    /// `WWWW` and is left out: nothing here can be said about it by counting characters).
    fn fixed_pitch_faces() -> Vec<&'static str> {
        // Windows has the catalogue's own; Wine substitutes all of those with one proportional face,
        // so a family Wine does ship (DejaVu Sans Mono) stands in for them there.
        let own = theme::FACES.iter().filter(|f| f.group == "Mono" || f.group == "Bitmap").map(|f| f.name);
        own.chain(["DejaVu Sans Mono", "Liberation Mono"])
            .filter(|name| {
                let fonts = Fonts::new(name, 96, false);
                measure(fonts.small, "iiii").0 == measure(fonts.small, "WWWW").0
            })
            .collect()
    }

    /// Every string the pane and the panel draw, in the narrowest panel with the pane open, for each
    /// fixed-pitch face at 96, 120, 144 and 192 DPI, on every range, with and without a candle
    /// hovered and with the longest note: each fits by character count (a run's width is its length
    /// times the face's advance), lies inside the window, and a run that starts in the pane ends in
    /// it; and no two strings of the pane's header collide.
    #[test]
    fn every_line_fits_for_each_fixed_pitch_face_at_each_dpi() {
        let faces = fixed_pitch_faces();
        assert!(!faces.is_empty(), "this machine draws no fixed-pitch face to count characters in");
        let mut checked = 0;
        for face in &faces {
            for dpi in [96u32, 120, 144, 192] {
                let w = panel::minimum(6, 0, dpi, false, true).0;
                for range in ["1D", "5D", "1M", "6M", "1Y", "5Y"] {
                    for hovered in [false, true] {
                        let mut fx = Fixture::with(Some(range), dpi, w, Some(face)).unwrap();
                        fx.note = "as of Nov 12 14:05 \u{b7} check dates".into();
                        let none = Hot::default();
                        fx.frame(&none);
                        let hot = Hot { aim: if hovered { fx.aim_middle() } else { None }, ..none };
                        panel::trace::start();
                        fx.frame(&hot);
                        let runs = panel::trace::take();
                        let pane = fx.pane().expect("the pane is open");
                        let ctx = format!("{face} {dpi} dpi {range} hovered {hovered}");
                        assert!(runs.len() > 20, "{ctx}: the frame drew {} strings", runs.len());
                        let (_, line_h) = measure(fx.fonts.small, "Xg");
                        let header = pane.top + 56 * dpi as i32 / 96;
                        for r in &runs {
                            let n = r.text.chars().count() as i32;
                            let (adv, _) = measure(r.font as HFONT, "0");
                            assert_eq!(r.w, n * adv, "{ctx}: {:?} is not {n} characters of {adv} px", r.text);
                            assert!(r.x >= 0 && r.x + r.w <= fx.w, "{ctx}: {:?} leaves the window ({}..{} of {})", r.text, r.x, r.x + r.w, fx.w);
                            if r.x >= pane.left && r.x < pane.right && r.y >= pane.top && r.y < pane.bottom {
                                assert!(r.x + r.w <= pane.right, "{ctx}: {:?} runs past the pane's right edge ({} > {})", r.text, r.x + r.w, pane.right);
                            }
                            checked += 1;
                        }
                        let band: Vec<_> = runs.iter().filter(|r| r.y >= pane.top && r.y < header && r.x >= pane.left && r.x < pane.right).collect();
                        assert!(band.iter().any(|r| r.text == "5Y") && band.iter().any(|r| r.text.contains('\u{b7}')), "{ctx}: the header band was not read: {:?}", band.iter().map(|r| &r.text).collect::<Vec<_>>());
                        for (i, a) in band.iter().enumerate() {
                            for b in &band[i + 1..] {
                                let same_line = (a.y - b.y).abs() < line_h;
                                let overlap = a.x < b.x + b.w && b.x < a.x + a.w;
                                assert!(!(same_line && overlap), "{ctx}: {:?} and {:?} collide in the header", a.text, b.text);
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1000, "{checked} strings checked");
    }

    /// No change, no area; an aimed pointer with no pane to name has no rectangle at all.
    #[test]
    fn nothing_changed_is_an_empty_rectangle_and_an_unnameable_change_is_everything() {
        let fx = Fixture::new(Some("5D")).unwrap();
        let g = fx.geo();
        let none = Hot::default();
        let r = panel::hot_dirty(&g, 96, Some(0), None, &none, &none).unwrap();
        assert_eq!((r.right - r.left, r.bottom - r.top), (0, 0));
        let aimed = Hot { aim: Some(crate::chartview::Pointer { bar: 3, y: 100 }), ..none };
        assert!(panel::hot_dirty(&g, 96, Some(0), None, &none, &aimed).is_none());
        assert!(panel::hot_dirty(&g, 96, None, None, &none, &Hot { open: true, ..none }).is_none());
    }
}
