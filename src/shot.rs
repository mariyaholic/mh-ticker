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
use crate::theme::{self, Gradient, Theme};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Graphics::Gdi::*;

/// Six symbols, because six is two rows of three and shows the panel at the size most people run.
const ROWS: usize = 6;

fn row(label: &str, symbol: &str, digits: u32) -> Row {
    Row { label: label.into(), symbol: symbol.into(), digits, name: String::new() }
}

/// Plausible values rather than the same number six times: a card with no 52-week range, one with a
/// volume and one without, and a delayed quote among live ones, so the screenshot exercises the
/// layout's awkward cases instead of its easy one.
type QuoteSpec<'a> = (f64, f64, f64, f64, f64, Option<f64>, (f64, f64), &'a str, bool);

fn quote(row: &Row, spec: QuoteSpec<'_>, now: f64) -> Quote {
    let (price, pct, prev, high, low, vol, yr, date, realtime) = spec;
    Quote {
        symbol: row.symbol.clone(),
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
        hover: None,
        picker: None,
        picker_scroll: 0,
        picker_btn: true,
        // Stills are the resting panel: the chart is a live pane, and a shot of it belongs to the
        // window test, not to the theme catalogue.
        chart: None,
        chart_btn: false,
        alert: None,
        phase,
    };
    capture(w, hh, &sc)
}

/// Paint a scene into a top-down RGBA buffer.
fn capture(w: i32, hh: i32, sc: &Scene) -> (u32, u32, Vec<u8>) {
    let mut px = vec![0u8; (w * hh * 4) as usize];
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, hh);
        let old = SelectObject(dc, bmp);
        panel::draw(dc, w, hh, sc);
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
    write_chart_named_at(dir, name, dpi, w, None)
}

/// `write_chart_named` with a card held under the pointer, so a review can watch the pane follow it.
pub fn write_chart_named_at(
    dir: &Path,
    name: &str,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
) -> Result<PathBuf, String> {
    let t = theme::ALL.iter().find(|t| t.name == name).ok_or_else(|| format!("no theme named {name}"))?;
    write_chart_at(dir, t, dpi, w, hover)
}

/// One rendered panel with the candle pane open, from deterministic bars.
///
/// This exists because the live feed can be closed (a weekend) or blocked (rate limited), and the
/// pane still has to be reviewable. The bars are a seeded walk, not a market: the point is the
/// painter, the axes and the layout. The six cards are the catalogue's own fixture, so the pane is
/// always seen beside the prices it will be read with.
pub fn write_chart(dir: &Path, t: &Theme, dpi: u32, w: i32) -> Result<PathBuf, String> {
    write_chart_at(dir, t, dpi, w, None)
}

/// `write_chart` with a card held under the pointer.
pub fn write_chart_at(
    dir: &Path,
    t: &Theme,
    dpi: u32,
    w: i32,
    hover: Option<usize>,
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
    let charted = bars.get(row).cloned().unwrap_or_default();
    // The sparklines seed from the hour of bars, exactly as they do on a live board.
    let mut hist = History::new();
    hist.record(&b, now);
    let g = panel::geometry_in(w, b.cells.len(), 0, dpi, false, true);
    let fonts = Fonts::new(theme::font_of(t, None), dpi, t.is_pixel());
    let grad = Gradient::new(t);
    let sc = Scene {
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
        hover: hover.map(|row| panel::Hover { row, t: snap_t(now) }),
        picker: None,
        picker_scroll: 0,
        picker_btn: true,
        chart: Some(Chart { label: &label, digits, t0, bars: &charted, last }),
        chart_btn: true,
        alert: None,
        phase: 0.0,
    };
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
    let mut seed = 0x2545_f491_4f6c_dd1d ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let amp = last.abs() * 0.0004;
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
        });
    }
    bars
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
