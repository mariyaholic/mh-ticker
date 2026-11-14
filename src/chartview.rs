//! The candle pane's arithmetic: what is drawn where, and which bar a pointer is over.
//!
//! Pure and portable. This module depends on `crate::model::Bar` and `std` and on nothing else: no
//! `windows_sys`, no device context, no clock. `panel::candles` measures its text, builds ONE
//! [`Layout`] per paint, draws everything in the plot from it, and stores it (`Scene::layout`);
//! `WM_MOUSEMOVE` then answers "which bar, which price" from that stored layout, so the pointer can
//! never disagree with the pixels. Nothing here is recomputed from a second formula.
//!
//! Also here, moved from `panel.rs` unchanged apart from the time-zone argument: `candle_metrics`,
//! `nice_ticks`, `tick_interval`, `fitted`, `fold`, `clock`.

use crate::model::Bar;

/// A pixel rectangle, `right` and `bottom` exclusive (the same convention as Win32 `RECT`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect { left, top, right, bottom }
    }

    /// `(x, y)` is inside: `left <= x < right` and `top <= y < bottom`.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Where the pointer is on the pane: the candle (index into the bars as drawn, after folding) and a
/// y in pane pixels already quantised by [`Layout::pointer_at`], so equality means "nothing to repaint".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub bar: usize,
    pub y: i32,
}

/// Everything the painter and the pointer share about one frame of the pane.
///
/// Built by the painter after it has measured its text (the axis width decides `plot`), used for
/// every coordinate it draws in the plot, and stored so the pointer code reads the same numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole pane (the card the candles sit on).
    pub pane: Rect,
    /// Where candles are drawn. Price gridlines span exactly its width; ticks hang under `bottom`.
    pub plot: Rect,
    /// Whole, odd pixels from `candle_metrics`: the slot each candle owns, its body, its wick.
    pub step: i32,
    pub body: i32,
    pub wick: i32,
    /// How many candles are drawn (after folding), left-aligned from `plot.left`.
    pub bars: usize,
    /// The padded price window the plot spans: `lo` at `plot.bottom`, `hi` at `plot.top`.
    pub lo: f64,
    pub hi: f64,
    /// Seconds per candle (60, 300, 900, 1800 or 3600).
    pub bucket_s: f64,
    /// The zone name in the header, which is a button (flips `Config::chart_utc`).
    pub zone_btn: Rect,
    /// The six range tabs on the header's second line, in `Range::ALL` order. `Rect::default()`
    /// (which contains nothing) for a tab that was not drawn.
    pub tabs: [Rect; 6],
    /// The volume strip under the price plot, when a displayed candle has volume. Its `left` and
    /// `right` are the plot's; time ticks hang under its `bottom`, not under `plot.bottom`.
    /// `plot` is then the PRICE plot (shortened), so `y_of` and `price_at` mean what they always did.
    pub vol: Option<Rect>,
}

impl Layout {
    /// The x of candle `i`'s wick (the middle pixel of its slot).
    pub fn x_of(&self, i: usize) -> i32 {
        self.plot.left + i as i32 * self.step + self.step / 2
    }

    /// The candle whose slot holds `x`. `None` left of the plot, right of the last candle (the open
    /// edge of a session in progress, and any leftover under one slot), and when there are no bars.
    pub fn index_at(&self, x: i32) -> Option<usize> {
        if self.step < 1 || self.bars == 0 || x < self.plot.left {
            return None;
        }
        let i = ((x - self.plot.left) / self.step) as usize;
        (i < self.bars).then_some(i)
    }

    /// The pixel row of `price`: `plot.bottom` at `lo`, `plot.top` at `hi`, rounded to a whole pixel.
    /// Not clamped: a price outside the window lands outside the plot, and the painter decides.
    pub fn y_of(&self, price: f64) -> i32 {
        let span_px = (self.plot.bottom - self.plot.top) as f64;
        let span = (self.hi - self.lo).max(f64::EPSILON);
        self.plot.bottom - (((price - self.lo) / span) * span_px).round() as i32
    }

    /// The price at pixel row `y`. `None` unless `plot.top <= y <= plot.bottom`.
    /// Inverse of [`Layout::y_of`] to within one pixel of price.
    pub fn price_at(&self, y: i32) -> Option<f64> {
        if y < self.plot.top || y > self.plot.bottom {
            return None;
        }
        let span_px = ((self.plot.bottom - self.plot.top) as f64).max(1.0);
        Some(self.lo + (self.plot.bottom - y) as f64 / span_px * (self.hi - self.lo))
    }

    /// The range tab under `(x, y)`, as an index into `Range::ALL`.
    pub fn tab_at(&self, x: i32, y: i32) -> Option<usize> {
        self.tabs.iter().position(|t| t.contains(x, y))
    }

    /// The pixel row the time axis hangs from: the bottom of the volume strip when there is one,
    /// else the bottom of the plot.
    pub fn base(&self) -> i32 {
        self.vol.map_or(self.plot.bottom, |v| v.bottom)
    }

    /// The pointer for client position `(x, y)`, or `None` when it is not over a candle's slot inside
    /// the plot. `y` is snapped down to a multiple of `quant` pixels counted from `plot.top`
    /// (`quant < 1` is treated as 1), so a mouse move inside one band does not change the `Pointer`.
    pub fn pointer_at(&self, x: i32, y: i32, quant: i32) -> Option<Pointer> {
        let bar = self.index_at(x)?;
        if !self.plot.contains(x, y) {
            return None;
        }
        let q = quant.max(1);
        Some(Pointer { bar, y: self.plot.top + (y - self.plot.top) / q * q })
    }
}

/// The name of the pane's clock, as the header shows it.
pub fn zone_label(utc: bool) -> &'static str {
    if utc {
        "UTC"
    } else {
        "local"
    }
}

/// A quote field the pane may draw: `Some(v)` only when `v` is finite and above zero. CNBC sends
/// `0.00` for a value a symbol does not have, and a reference line at zero is a lie.
pub fn positive(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// `price` if it lies inside the window `[lo, hi]` (both ends inclusive) and is finite, else `None`.
/// The painter draws a reference line (previous close, day high, day low) only for `Some`.
pub fn in_window(price: Option<f64>, lo: f64, hi: f64) -> Option<f64> {
    price.filter(|p| p.is_finite() && *p >= lo && *p <= hi)
}

/// Where the locked symbol's alert levels go on one frame of the pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelMarks {
    /// The levels inside the price window (`in_window`), each with its pixel row (`Layout::y_of`), in
    /// the order given. Drawn as a dashed line across the plot with the price at its right end.
    pub lines: Vec<(f64, i32)>,
    /// The nearest level above the window and how many levels lie above it in all. Drawn as a small
    /// marker on the plot's top edge.
    pub above: Option<(f64, usize)>,
    /// The nearest level below the window and how many lie below it. A marker on the bottom edge.
    pub below: Option<(f64, usize)>,
}

/// Place alert `levels` on the frame `lay` describes. A level that is not positive and finite is
/// skipped (it can never trip). The painter draws exactly what this returns.
pub fn level_marks(lay: &Layout, levels: &[f64]) -> LevelMarks {
    let mut out = LevelMarks::default();
    for &p in levels.iter().filter(|p| p.is_finite() && **p > 0.0) {
        if in_window(Some(p), lay.lo, lay.hi).is_some() {
            out.lines.push((p, lay.y_of(p)));
        } else if p > lay.hi {
            let (near, n) = out.above.unwrap_or((p, 0));
            out.above = Some((near.min(p), n + 1));
        } else {
            let (near, n) = out.below.unwrap_or((p, 0));
            out.below = Some((near.max(p), n + 1));
        }
    }
    out
}

/// Indices of the drawn candles that start a new session: `i >= 1` where
/// `bars[i].dt - bars[i - 1].dt > 1.5 * bucket_s`. Bars are in time order; `bars` are the folded
/// candles, so a candle's `dt` is already on a bucket boundary. Empty for fewer than two bars or a
/// non-positive/NaN bucket. The painter draws a faint dotted line half a slot left of each index.
pub fn gaps(bars: &[Bar], bucket_s: f64) -> Vec<usize> {
    // A zero, negative or NaN bucket would make every step look like a break: refuse it.
    if !(bucket_s > 0.0 && bucket_s.is_finite()) {
        return Vec::new();
    }
    let limit = 1.5 * bucket_s;
    bars.windows(2)
        .enumerate()
        .filter(|(_, w)| (w[1].dt as f64 - w[0].dt as f64) > limit)
        .map(|(i, _)| i + 1)
        .collect()
}

/// The candle's own change in percent, `(c - o) / o * 100`; `None` when `o` is zero or not finite.
pub fn bar_change_pct(b: &Bar) -> Option<f64> {
    let (o, c) = (b.o as f64, b.c as f64);
    if !o.is_finite() || o == 0.0 {
        return None;
    }
    // A NaN close gives no change either, rather than a NaN in the legend.
    Some((c - o) / o * 100.0).filter(|p| p.is_finite())
}

/// The legend line shown in the pane's header while a candle is hovered.
///
/// `full` (`compact == false`): `"13:35  O 4,150.20  H 4,151.00  L 4,149.50  C 4,150.80  +0.01%"`:
/// the candle's start time on the pane's clock (`clock(b.t(t0), tz_offset_s)`), two spaces, then
/// O, H, L, C each as a letter, a space and `fmt(value)`, separated by two spaces, then two spaces and
/// the change from [`bar_change_pct`] formatted `{:+.2}%` (`+0.01%`, `-1.30%`; a zero change is
/// `+0.00%`), or nothing when the change is `None`.
/// `compact`: `"13:35  C 4,150.80  +0.01%"` (time, close, change), for a pane too narrow for the full
/// line. `fmt` formats a price the way the axis does (the caller passes `fmt_price` with its digits).
#[cfg(test)]
pub fn readout(b: &Bar, t0: f64, tz_offset_s: i32, fmt: &dyn Fn(f64) -> String, compact: bool) -> String {
    readout_with(b, t0, tz_offset_s, fmt, compact, Stamp::Clock)
}

/// How the legend names a candle's moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    /// `13:35`: the 1D pane, as it always was.
    Clock,
    /// `Nov 12 13:35`: intraday candles over several days (5D).
    DayClock,
    /// `Nov 12 2026`: candles of a day or more, which have no time.
    Day,
}

/// [`readout`] with the moment named by `stamp` instead of always the clock.
pub fn readout_with(b: &Bar, t0: f64, tz_offset_s: i32, fmt: &dyn Fn(f64) -> String, compact: bool, stamp: Stamp) -> String {
    let change = bar_change_pct(b).map(|p| format!("  {p:+.2}%")).unwrap_or_default();
    let t = b.t(t0);
    let time = match stamp {
        Stamp::Clock => clock(t, tz_offset_s),
        Stamp::DayClock => format!("{} {}", date_label(t, tz_offset_s, DateStyle::DayMonth), clock(t, tz_offset_s)),
        // A bucket of a day or more starts on a boundary that f32 may have put a few seconds early.
        Stamp::Day => date_label(t + BIAS_S, tz_offset_s, DateStyle::Full),
    };
    if compact {
        return format!("{time}  C {}{change}", fmt(b.c as f64));
    }
    format!(
        "{time}  O {}  H {}  L {}  C {}{change}",
        fmt(b.o as f64),
        fmt(b.h as f64),
        fmt(b.l as f64),
        fmt(b.c as f64)
    )
}

/// `(step, body, wick)` in whole pixels for `n` candles across `plot_w`: the slot each candle owns,
/// the width of its body and the width of its wick. The step and body are odd and the wick is odd, so
/// the wick is exactly the middle pixel of the body and of the slot; and they are the same for every
/// candle in the row, which is the whole of the difference between a chart and a bar code.
pub(crate) fn candle_metrics(plot_w: i32, n: i32, dpi: u32) -> (i32, i32, i32) {
    let s = |v: i32| v * dpi as i32 / 96;
    let mut step = (plot_w / n.max(1)).clamp(1, s(15) | 1);
    if step >= MIN_BAR && step % 2 == 0 {
        step -= 1;
    }
    let wick = s(1) | 1;
    // About 70 % of the step, made odd so it is symmetric about the wick, and never thinner than it.
    let mut body = ((step as f32 * 0.7).round() as i32).max(wick);
    if body % 2 == 0 {
        body -= 1;
    }
    (step, body.clamp(1, step.max(1)), wick)
}

/// Round gridline values inside `[lo, hi]`, about `want` of them, and how many decimals they need.
/// The step is 1, 2, 2.5 or 5 times a power of ten — the spacings a reader can subtract in their
/// head — so a day that runs 6,655 to 6,733 is gridded at 6,680 / 6,700 / 6,720, not at the
/// arbitrary quarters of its own range.
pub(crate) fn nice_ticks(lo: f64, hi: f64, want: usize) -> (Vec<f64>, u32) {
    let span = hi - lo;
    if span <= 0.0 || !span.is_finite() || want == 0 {
        return (Vec::new(), 0);
    }
    let raw = span / want as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag * [1.0, 2.0, 2.5, 5.0, 10.0].iter().copied().find(|m| *m >= norm - 1e-9).unwrap_or(10.0);
    let mut out = Vec::new();
    let mut v = (lo / step).ceil() * step;
    while v <= hi + step * 1e-9 && out.len() < 16 {
        // Strip the float drift (0.30000000000000004) so the label and the line agree.
        out.push((v / step).round() * step);
        v += step;
    }
    let decimals = (0..=6u32).find(|d| {
        let f = step * 10f64.powi(*d as i32);
        (f - f.round()).abs() < 1e-6
    });
    (out, decimals.unwrap_or(6))
}

/// The smallest round clock interval (15 min .. 12 h) whose ticks are at least `min_px` apart when a
/// second is `px_per_s` wide. A day of 5-minute candles ticks hourly; a pane stretched wide enough
/// to hold the minute ticks every half hour.
pub(crate) fn tick_interval(px_per_s: f64, min_px: f64) -> f64 {
    const LADDER: [f64; 8] = [900.0, 1_800.0, 3_600.0, 7_200.0, 10_800.0, 14_400.0, 21_600.0, 43_200.0];
    LADDER.iter().copied().find(|s| s * px_per_s >= min_px).unwrap_or(86_400.0)
}

/// The narrowest candle that still reads as one. Below five pixels a body is a scratch between two
/// wicks, whatever the colour, and an odd width is what lets the wick be exactly the middle pixel.
pub(crate) const MIN_BAR: i32 = 5;

/// A week and a calendar month as bucket sizes. A month is not a fixed length: `MONTH_S` is only the
/// nominal value that NAMES a calendar-month bucket (`fold` cuts it on the calendar), and `gaps` and
/// the axis treat anything of a day or more as "no session breaks".
pub(crate) const DAY_S: f64 = 86_400.0;
pub(crate) const WEEK_S: f64 = 604_800.0;
pub(crate) const MONTH_S: f64 = 2_629_800.0;
/// Candle starts are stored as `f32` offsets, which are 16 s coarse five years out. A bucket of a day
/// or more is therefore decided 30 s after the instant it names, so a bar stamped exactly on a local
/// midnight is not filed under the day before.
const BIAS_S: f64 = 30.0;

/// The finest bucket on the ladder whose candles still have `MIN_BAR` pixels each, at one-minute
/// bars and no coarser than an hour: the 1D pane. Buckets are cut on the clock `tz_offset_s` seconds
/// east of UTC.
#[cfg(test)]
pub(crate) fn fitted(bars: &[Bar], plot_w: i32, t0: f64, tz_offset_s: i32) -> (Vec<Bar>, f64) {
    fitted_to(bars, plot_w, t0, tz_offset_s, 60.0, 3_600.0)
}

/// The bucket ladder: 1, 5, 15, 30, 60 minutes, 4 hours, a day, a week, a calendar month.
pub(crate) const LADDER: [f64; 9] = [60.0, 300.0, 900.0, 1_800.0, 3_600.0, 14_400.0, DAY_S, WEEK_S, MONTH_S];

/// [`fitted`] for data of `bar_s` seconds a bar (the series' inferred spacing; 0 when unknown),
/// climbing no higher than `coarsest`. The ladder starts at the first rung that is not finer than the
/// data (a rung within 5 % under `bar_s` counts as equal), so a daily series starts at a day. The
/// finest rung that fits wins; if none does, `coarsest` is used rather than a comb. If `coarsest` is
/// finer than the starting rung the starting rung is used.
pub(crate) fn fitted_to(bars: &[Bar], plot_w: i32, t0: f64, tz_offset_s: i32, bar_s: f64, coarsest: f64) -> (Vec<Bar>, f64) {
    let from = LADDER.iter().position(|r| *r >= bar_s * 0.95).unwrap_or(LADDER.len() - 1);
    let (mut best, mut bucket) = (fold(bars, LADDER[from], t0, tz_offset_s), LADDER[from]);
    // The finest bucket that fits wins; if none does, the coarsest is still better than a comb.
    if best.len() as i32 * MIN_BAR > plot_w {
        for b in LADDER[from + 1..].iter().take_while(|b| **b <= coarsest) {
            best = fold(bars, *b, t0, tz_offset_s);
            bucket = *b;
            if best.len() as i32 * MIN_BAR <= plot_w {
                break;
            }
        }
    }
    (best, bucket)
}

/// Which bucket of size `bucket_s` the local instant `local` (seconds on the zone's clock) is in:
/// `floor(local / bucket_s)`, except a week (cut on Monday) and a calendar month (`year * 12 + month`).
fn bucket_key(local: f64, bucket_s: f64) -> i64 {
    if bucket_s >= MONTH_S - 1.0 {
        let (y, m, _) = civil_from_days(((local + BIAS_S) / DAY_S).floor() as i64);
        y * 12 + (m as i64 - 1)
    } else if bucket_s == WEEK_S {
        // Day 0 (1970-01-01) was a Thursday, so Monday is day 4 and weeks turn over at 7k - 3.
        let days = ((local + BIAS_S) / DAY_S).floor() as i64;
        (days + 3).div_euclid(7)
    } else if bucket_s >= DAY_S {
        ((local + BIAS_S) / bucket_s).floor() as i64
    } else {
        (local / bucket_s).floor() as i64
    }
}

/// The local instant a bucket starts at: the inverse of [`bucket_key`] on the boundary itself.
fn bucket_start(key: i64, bucket_s: f64) -> f64 {
    if bucket_s >= MONTH_S - 1.0 {
        days_from_civil(key.div_euclid(12), key.rem_euclid(12) as u32 + 1, 1) as f64 * DAY_S
    } else if bucket_s == WEEK_S {
        (key * 7 - 3) as f64 * DAY_S
    } else {
        key as f64 * bucket_s
    }
}

/// Fold bars into `bucket_s`-second candles: the open of the first bar in a bucket, the extreme high
/// and low of all of them, the close of the last, the volumes added. Buckets are cut on the UTC clock
/// (`t0` is the absolute time of `dt == 0`), shifted by `tz_offset_s` so a +05:30 zone gets its
/// candles on local half-hours (0 is the UTC clock, and the output is then exactly what it was before
/// zones), not from the first bar and not by counting bars: a day that opens at 13:31 still has its
/// five-minute candles start at 13:30, which is what the time axis labels say, and a break in the
/// session stays a gap instead of being papered over. A day is cut on local midnight, a week
/// ([`WEEK_S`]) on local Monday 00:00, a month ([`MONTH_S`]) on the 1st.
pub(crate) fn fold(bars: &[Bar], bucket_s: f64, t0: f64, tz_offset_s: i32) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::new();
    let mut key: Option<i64> = None;
    let tz = tz_offset_s as f64;
    for b in bars {
        let k = bucket_key(t0 + b.dt as f64 + tz, bucket_s);
        if key == Some(k) {
            if let Some(last) = out.last_mut() {
                last.h = last.h.max(b.h);
                last.l = last.l.min(b.l);
                last.c = b.c;
                last.v += b.v;
                continue;
            }
        }
        key = Some(k);
        out.push(Bar { dt: (bucket_start(k, bucket_s) - tz - t0) as f32, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v });
    }
    out
}

/// Days since 1970-01-01 of the civil date `y-m-d` (proleptic Gregorian; `m` 1..=12).
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month 1..=12, day 1..=31)` of `days` since 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The date forms the axis and the legend use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStyle {
    /// `Nov 12`
    DayMonth,
    /// `Oct`
    Month,
    /// `2025`
    Year,
    /// `Nov 12 2026`
    Full,
}

/// The local date of the absolute time `t` (UTC seconds) on the clock `tz_offset_s` east of UTC.
pub fn date_label(t: f64, tz_offset_s: i32, style: DateStyle) -> String {
    let (y, m, d) = civil_from_days(((t + tz_offset_s as f64) / DAY_S).floor() as i64);
    let mon = MONTHS[(m as usize - 1).min(11)];
    match style {
        DateStyle::DayMonth => format!("{mon} {d}"),
        DateStyle::Month => mon.to_string(),
        DateStyle::Year => y.to_string(),
        DateStyle::Full => format!("{mon} {d} {y}"),
    }
}

/// The local calendar day number (days since 1970-01-01) of `t`; two instants on the same local
/// date have the same number.
pub fn local_day(t: f64, tz_offset_s: i32) -> i64 {
    ((t + tz_offset_s as f64) / DAY_S).floor() as i64
}

/// Is a series of `bar_s` seconds a bar made of trading DAYS (daily bars or longer) rather than of
/// instants? The same 5 % tolerance as the bucket ladder.
pub fn daily(bar_s: f64) -> bool {
    bar_s >= DAY_S * 0.95
}

/// The clock offset a series' buckets, axis and legend are read on.
///
/// Intraday bars are instants and are read on the viewer's clock (`tz_offset_s`). A daily or longer
/// bar is a trading day, stamped at midnight on its exchange's clock. That instant is within 12 hours
/// of the UTC midnight of the same calendar date wherever the exchange is (UTC-12 to UTC+12), so read
/// 12 hours east of UTC it comes out as the exchange's own date for every viewer. Read on the
/// viewer's clock instead, every candle would be dated a day early for anyone west of the exchange
/// (a Tokyo session seen from Manila, a New York one from Los Angeles) and weeks would be cut on the
/// wrong day. The midnight stamp is an assumption, unverified live: `--probe` prints the stamps.
pub fn date_zone(bar_s: f64, tz_offset_s: i32) -> i32 {
    if daily(bar_s) {
        43_200
    } else {
        tz_offset_s
    }
}

/// Axis ticks for candles of a day or more: `(candle index, label)` at each candle that starts a new
/// local month, labelled `Oct` (`2026` for a January, which starts a year). When the month labels
/// would not fit (two consecutive ticks closer than the widest label plus `gap_px`, with candles
/// `step_px` apart and `width` measuring a label), only year starts are labelled (`2025`), thinned
/// from the left so no two are closer than their width plus `gap_px`. With fewer than two ticks
/// either way (a one-month window) it falls back to `Nov 12` style labels on candles spaced as far
/// as the labels need. Never labels index 0 as a month start. Candle starts may be a few seconds
/// early (f32); the date is read 30 s later.
pub fn date_ticks(bars: &[Bar], t0: f64, tz_offset_s: i32, step_px: i32, width: &dyn Fn(&str) -> i32, gap_px: i32) -> Vec<(usize, String)> {
    let ymd = |i: usize| civil_from_days(local_day(bars[i].t(t0) + BIAS_S, tz_offset_s));
    // Keep a tick only when it clears the previous kept one.
    let thin = |cands: Vec<(usize, String)>| -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> = Vec::new();
        for (i, label) in cands {
            let clear = out.last().is_none_or(|(j, prev)| {
                ((i - j) as i32 * step_px) >= width(prev).max(width(&label)) + gap_px
            });
            if clear {
                out.push((i, label));
            }
        }
        out
    };
    let mut months = Vec::new();
    let mut years = Vec::new();
    #[allow(clippy::needless_range_loop)] // `i` also names the candle in the result
    for i in 1..bars.len() {
        let ((py, pm, _), (y, m, _)) = (ymd(i - 1), ymd(i));
        if (y, m) != (py, pm) {
            let t = bars[i].t(t0) + BIAS_S;
            months.push((i, date_label(t, tz_offset_s, if m == 1 { DateStyle::Year } else { DateStyle::Month })));
        }
        if y != py {
            years.push((i, date_label(bars[i].t(t0) + BIAS_S, tz_offset_s, DateStyle::Year)));
        }
    }
    let kept = thin(months.clone());
    if kept.len() == months.len() && months.len() >= 2 {
        return kept;
    }
    let yearly = thin(years);
    if yearly.len() >= 2 {
        return yearly;
    }
    if months.len() >= 2 {
        return kept;
    }
    thin((0..bars.len()).map(|i| (i, date_label(bars[i].t(t0) + BIAS_S, tz_offset_s, DateStyle::DayMonth))).collect())
}

/// Split a plot into the price plot and, when `has_volume`, a volume strip under it: the strip is the
/// bottom fifth of `plot`'s height, the price plot is the rest above it, and `gap_px` pixels are left
/// between them. Without volume the price plot is `plot` itself and there is no strip, so a series
/// with no volume lays out exactly as before volume existed. A plot too short for both (a strip
/// under 4 px, or a price plot under 3 times the strip) gets no strip.
pub fn split_plot(plot: Rect, has_volume: bool, gap_px: i32) -> (Rect, Option<Rect>) {
    let h = plot.bottom - plot.top;
    let strip = h / 5;
    if !has_volume || strip < 4 || h - strip - gap_px < 3 * strip {
        return (plot, None);
    }
    let price = Rect { bottom: plot.bottom - strip - gap_px, ..plot };
    (price, Some(Rect { top: plot.bottom - strip, ..plot }))
}

/// The pixel height of a volume bar: 0 for `v` that is not positive and finite (or `max` that is
/// not), at least 1 for any positive volume, at most `strip_h`, linear in between.
pub fn volume_px(v: f32, max: f32, strip_h: i32) -> i32 {
    if !(v > 0.0 && v.is_finite() && max > 0.0 && max.is_finite()) || strip_h < 1 {
        return 0;
    }
    (((v / max) as f64 * strip_h as f64).round() as i32).clamp(1, strip_h)
}

/// The header's name for a candle size: `1 m` .. `1 h`, `4 h`, `1 d`, `1 w`, `1 mo`.
pub fn bucket_name(bucket_s: f64) -> &'static str {
    match bucket_s as i64 {
        60 => "1 m",
        300 => "5 m",
        900 => "15 m",
        1_800 => "30 m",
        3_600 => "1 h",
        14_400 => "4 h",
        86_400 => "1 d",
        604_800 => "1 w",
        b if b >= MONTH_S as i64 - 1 => "1 mo",
        _ => "1 h",
    }
}

/// Does a pane at candles of `bucket_s` mark session breaks? Only for candles shorter than a day:
/// every weekend is not a break.
pub fn marks_breaks(bucket_s: f64) -> bool {
    bucket_s < DAY_S
}

/// What the legend names a candle with: the date alone for candles of a day or more, the clock alone
/// on the 1D pane (`is_1d`), date and clock on the other intraday ranges.
pub fn stamp_for(is_1d: bool, bucket_s: f64) -> Stamp {
    if bucket_s >= DAY_S {
        Stamp::Day
    } else if is_1d {
        Stamp::Clock
    } else {
        Stamp::DayClock
    }
}

/// HH:MM on the clock `tz_offset_s` seconds east of UTC, for the absolute time `t` (UTC seconds).
/// Wraps at midnight in both directions. Offset 0 is plain UTC, as before zones existed.
pub(crate) fn clock(t: f64, tz_offset_s: i32) -> String {
    let m = ((((t as i64) + tz_offset_s as i64) % 86_400) + 86_400) % 86_400 / 60;
    format!("{:02}:{:02}", m / 60, m % 60)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_of_minutes_folds_into_candles_that_read() {
        let bars: Vec<Bar> = (0..390)
            .map(|i| {
                let o = 100.0 + (i as f32 % 7.0);
                Bar { dt: (i * 60) as f32, o, h: o + 1.0, l: o - 1.0, c: o + 0.5, v: 0.0 }
            })
            .collect();
        // 390 bars in 650 px is 1.6 px a bar: fold to five minutes, 78 of them at 8 px.
        let (folded, bucket) = fitted(&bars, 650, 0.0, 0);
        assert_eq!(bucket, 300.0);
        assert_eq!(folded.len(), 78);
        // Open from the first bar of the bucket, extremes across the bucket, close from the last.
        assert_eq!(folded[0].o, bars[0].o);
        assert_eq!(folded[0].c, bars[4].c);
        assert_eq!(folded[0].h, bars[0..5].iter().fold(f32::MIN, |m, b| m.max(b.h)));
        assert_eq!(folded[0].l, bars[0..5].iter().fold(f32::MAX, |m, b| m.min(b.l)));
        // A pane wide enough for the minute keeps the minute.
        let (fine, b1) = fitted(&bars, 2_000, 0.0, 0);
        assert_eq!((b1, fine.len()), (60.0, 390));
    }

    #[test]
    fn a_break_in_the_session_is_not_closed_by_folding() {
        let bars = vec![
            Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 },
            Bar { dt: 60.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 },
            // An hour later: the market was shut, and five-minute buckets must not merge across it.
            Bar { dt: 3_660.0, o: 2.0, h: 2.0, l: 2.0, c: 2.0, v: 0.0 },
        ];
        let folded = fold(&bars, 300.0, 0.0, 0);
        assert_eq!(folded.len(), 2);
        assert_eq!((folded[0].c, folded[1].o), (1.0, 2.0));
    }

    #[test]
    fn candles_start_on_the_utc_clock_even_when_the_day_opens_off_it() {
        // A session that opens at 13:31:00 UTC: `dt` counts from there, `t0` places it on the clock.
        let t0 = 13.0 * 3_600.0 + 31.0 * 60.0 + 86_400.0 * 20_000.0;
        let bars: Vec<Bar> = (0..14)
            .map(|i| Bar { dt: (i * 60) as f32, o: i as f32, h: i as f32 + 1.0, l: i as f32 - 1.0, c: i as f32, v: 0.0 })
            .collect();
        let folded = fold(&bars, 300.0, t0, 0);
        // 13:31-13:34 | 13:35-13:39 | 13:40-13:44: three candles, cut at :30 / :35 / :40, not at :31.
        assert_eq!(folded.len(), 3);
        for b in &folded {
            assert_eq!(((t0 + b.dt as f64) % 300.0).abs(), 0.0, "bucket starts on a five-minute boundary");
        }
        assert_eq!(clock(folded[0].t(t0), 0), "13:30");
        assert_eq!(clock(folded[1].t(t0), 0), "13:35");
        // The first candle holds only the four minutes the session actually traded.
        assert_eq!(folded[0].o, bars[0].o);
        assert_eq!(folded[0].c, bars[3].c);
    }

    #[test]
    fn candles_are_whole_pixels_with_the_wick_in_the_middle() {
        for dpi in [96, 120, 144, 192] {
            for plot_w in [296, 400, 650, 720, 1_200, 2_000] {
                for n in [1, 4, 8, 24, 78, 96, 390] {
                    let (step, body, wick) = candle_metrics(plot_w, n, dpi);
                    assert!(step >= 1 && body >= 1 && wick >= 1);
                    // Same parity all round: the wick is the middle pixel of the body and the slot.
                    assert_eq!(body % 2, 1, "dpi {dpi} w {plot_w} n {n}: body {body} not odd");
                    assert_eq!(wick % 2, 1, "wick {wick} not odd");
                    assert!(body <= step, "a body wider than its slot overlaps the next candle");
                    if step >= MIN_BAR {
                        assert_eq!(step % 2, 1, "step {step} not odd");
                        assert!(body < step, "candles that fit keep a gap between bodies");
                        assert!(n * step <= plot_w, "{n} candles of {step}px overflow {plot_w}");
                    }
                }
            }
        }
    }

    #[test]
    fn gridlines_land_on_round_prices() {
        // The case from the screenshot: S&P over 6,655 .. 6,733 used to grid at 6,709.47 / 6,686.35.
        let (ticks, decimals) = nice_ticks(6_655.0, 6_733.0, 4);
        assert_eq!(ticks, [6_660.0, 6_680.0, 6_700.0, 6_720.0]);
        assert_eq!(decimals, 0);
        // A narrow range steps in quarters and says so in the label.
        let (q, d) = nice_ticks(0.0, 1.0, 4);
        assert_eq!((q.len(), d), (5, 2));
        assert!((q[1] - 0.25).abs() < 1e-12 && (q[4] - 1.0).abs() < 1e-12);
        // Spot gold: two decimals only when the step needs them.
        let (g, gd) = nice_ticks(4_150.0, 4_162.0, 4);
        assert!(g.iter().all(|v| *v >= 4_150.0 && *v <= 4_162.0), "{g:?}");
        assert_eq!(gd, 0, "a 2-dollar step prints whole dollars");
        // Nothing to grid: no span, no ticks, and no panic.
        assert_eq!(nice_ticks(5.0, 5.0, 4).0.len(), 0);
        assert_eq!(nice_ticks(f64::NAN, 1.0, 4).0.len(), 0);
    }

    #[test]
    fn time_ticks_are_round_clock_times_that_do_not_collide() {
        // Five-minute candles 9 px wide: 0.03 px a second, so an hourly tick is ~108 px apart.
        assert_eq!(tick_interval(9.0 / 300.0, 80.0), 3_600.0);
        // A wide pane of one-minute candles can afford every quarter hour.
        assert_eq!(tick_interval(15.0 / 60.0, 80.0), 900.0);
        // A hopelessly narrow one falls back to a day rather than dividing by nothing.
        assert_eq!(tick_interval(0.0, 80.0), 86_400.0);
        // The clock labels are what they say, in UTC, wrapping past midnight.
        assert_eq!(clock(13.0 * 3_600.0, 0), "13:00");
        assert_eq!(clock(86_400.0 + 30.0 * 60.0, 0), "00:30");
    }

    fn lay() -> Layout {
        // 40 candles of a 7 px slot in a 280 x 100 plot at (20, 50); window 100..200.
        Layout {
            pane: Rect::new(10, 10, 400, 200),
            plot: Rect::new(20, 50, 300, 150),
            step: 7,
            body: 5,
            wick: 1,
            bars: 40,
            lo: 100.0,
            hi: 200.0,
            bucket_s: 300.0,
            zone_btn: Rect::new(60, 12, 100, 28),
            tabs: [Rect::default(); 6],
            vol: None,
        }
    }

    #[test]
    fn index_at_inverts_x_of() {
        let l = lay();
        for i in 0..l.bars {
            assert_eq!(l.index_at(l.x_of(i)), Some(i));
            // Anywhere in the slot is the same candle.
            assert_eq!(l.index_at(l.plot.left + i as i32 * l.step), Some(i));
            assert_eq!(l.index_at(l.plot.left + (i as i32 + 1) * l.step - 1), Some(i));
        }
        assert_eq!(l.index_at(l.plot.left - 1), None);
        assert_eq!(l.index_at(l.plot.left + l.bars as i32 * l.step), None, "the open edge is no candle");
        assert_eq!(Layout { bars: 0, ..l }.index_at(l.plot.left), None);
    }

    #[test]
    fn price_at_inverts_y_of_within_a_pixel() {
        let l = lay();
        let per_px = (l.hi - l.lo) / (l.plot.bottom - l.plot.top) as f64;
        for p in [100.0, 112.5, 150.0, 163.3, 199.9, 200.0] {
            let back = l.price_at(l.y_of(p)).unwrap();
            assert!((back - p).abs() <= per_px, "{p} came back as {back}");
        }
        assert_eq!(l.y_of(100.0), l.plot.bottom);
        assert_eq!(l.y_of(200.0), l.plot.top);
        assert_eq!(l.price_at(l.plot.top - 1), None);
        assert_eq!(l.price_at(l.plot.bottom + 1), None);
    }

    #[test]
    fn alert_levels_are_lines_inside_and_markers_outside() {
        let l = lay();
        let m = level_marks(&l, &[150.0, 260.0, 230.0]);
        assert_eq!(m.lines, vec![(150.0, l.y_of(150.0))]);
        assert_eq!(m.above, Some((230.0, 2)), "the nearest above, and how many");
        assert_eq!(m.below, None);
        // The window's own ends are lines, at the bottom and top of the plot.
        let e = level_marks(&l, &[100.0, 200.0]);
        assert_eq!(e.lines, vec![(100.0, l.plot.bottom), (200.0, l.plot.top)]);
        assert_eq!((e.above, e.below), (None, None));
        // One tick below the window is a bottom marker and no line.
        let b = level_marks(&l, &[99.0]);
        assert!(b.lines.is_empty());
        assert_eq!(b.below, Some((99.0, 1)));
        // Levels that cannot trip are skipped, the rest are kept.
        let s = level_marks(&l, &[f64::NAN, f64::INFINITY, 0.0, -150.0, 150.0]);
        assert_eq!(s.lines, vec![(150.0, l.y_of(150.0))]);
        assert_eq!((s.above, s.below), (None, None));
        assert_eq!(level_marks(&l, &[]), LevelMarks::default());
        // The lines keep the order given, not the price order.
        let o = level_marks(&l, &[180.0, 120.0]);
        assert_eq!(o.lines.iter().map(|(p, _)| *p).collect::<Vec<_>>(), [180.0, 120.0]);
    }

    #[test]
    fn the_pointer_is_a_bar_and_a_quantised_row_or_nothing() {
        let l = lay();
        let a = l.pointer_at(l.x_of(3), 71, 2).unwrap();
        let b = l.pointer_at(l.x_of(3) + 1, 70, 2).unwrap();
        assert_eq!(a, b, "one band, one pointer: no repaint");
        assert_eq!((a.bar, a.y), (3, 70));
        assert_ne!(Some(a), l.pointer_at(l.x_of(3), 72, 2));
        assert_eq!(l.pointer_at(l.x_of(3), l.plot.top - 1, 2), None, "above the plot");
        assert_eq!(l.pointer_at(l.x_of(3), l.plot.bottom, 2), None, "bottom is exclusive");
        assert_eq!(l.pointer_at(l.plot.right + 50, 70, 2), None, "past the last candle");
        assert_eq!(l.pointer_at(l.x_of(3), 71, 0).map(|p| p.y), Some(71), "quant < 1 is 1");
    }

    fn bar(dt: f32, o: f32, h: f32, l: f32, c: f32) -> Bar {
        Bar { dt, o, h, l, c, v: 0.0 }
    }

    fn two_decimals(v: f64) -> String {
        format!("{v:.2}")
    }

    /// The same shape as `a_day_of_minutes_folds_into_candles_that_read`: 390 one-minute bars.
    fn minute_day() -> Vec<Bar> {
        (0..390)
            .map(|i| {
                let o = 100.0 + (i as f32 % 7.0);
                bar((i * 60) as f32, o, o + 1.0, o - 1.0, o + 0.5)
            })
            .collect()
    }

    /// The fold as it was before zones existed: buckets cut on the UTC clock and nothing else.
    fn fold_before_zones(bars: &[Bar], bucket_s: f64, t0: f64) -> Vec<Bar> {
        let mut groups: Vec<(f64, Vec<&Bar>)> = Vec::new();
        for b in bars {
            let k = ((t0 + b.dt as f64) / bucket_s).floor();
            match groups.last_mut() {
                Some((gk, g)) if *gk == k => g.push(b),
                _ => groups.push((k, vec![b])),
            }
        }
        groups
            .iter()
            .map(|(k, g)| Bar {
                dt: (k * bucket_s - t0) as f32,
                o: g[0].o,
                h: g.iter().fold(f32::MIN, |m, b| m.max(b.h)),
                l: g.iter().fold(f32::MAX, |m, b| m.min(b.l)),
                c: g[g.len() - 1].c,
                v: g.iter().map(|b| b.v).sum(),
            })
            .collect()
    }

    #[test]
    fn a_reference_outside_the_window_is_not_drawn() {
        let (lo, hi) = (100.0, 200.0);
        assert_eq!(in_window(Some(150.0), lo, hi), Some(150.0));
        assert_eq!(in_window(Some(lo), lo, hi), Some(lo), "the lower end is inside");
        assert_eq!(in_window(Some(hi), lo, hi), Some(hi), "the upper end is inside");
        assert_eq!(in_window(Some(99.99), lo, hi), None);
        assert_eq!(in_window(Some(200.01), lo, hi), None);
        assert_eq!(in_window(None, lo, hi), None);
        assert_eq!(in_window(Some(f64::NAN), lo, hi), None);
        assert_eq!(in_window(Some(f64::INFINITY), lo, hi), None);
        assert_eq!(in_window(Some(f64::NEG_INFINITY), lo, hi), None);
        for p in [lo, 150.0, hi] {
            assert_eq!(in_window(Some(p), hi, lo), None, "an inverted window holds no price");
        }
    }

    #[test]
    fn positive_drops_zero_and_non_finite() {
        assert_eq!(positive(Some(0.0)), None);
        assert_eq!(positive(Some(-0.0)), None);
        assert_eq!(positive(Some(-1.0)), None);
        assert_eq!(positive(Some(f64::NAN)), None);
        assert_eq!(positive(Some(f64::INFINITY)), None);
        assert_eq!(positive(Some(0.01)), Some(0.01));
        assert_eq!(positive(None), None);
    }

    #[test]
    fn gaps_mark_the_candle_after_a_session_break() {
        let at = |dts: &[f32]| -> Vec<Bar> { dts.iter().map(|d| bar(*d, 1.0, 1.0, 1.0, 1.0)).collect() };
        assert_eq!(gaps(&at(&[0.0, 300.0, 600.0, 4_200.0, 4_500.0]), 300.0), vec![3]);
        // Exactly 1.5 buckets later is not a break; a little more is.
        assert!(gaps(&at(&[0.0, 450.0]), 300.0).is_empty());
        assert_eq!(gaps(&at(&[0.0, 451.0]), 300.0), vec![1]);
        // A contiguous day has none.
        let contiguous: Vec<f32> = (0..100).map(|i| i as f32 * 300.0).collect();
        assert!(gaps(&at(&contiguous), 300.0).is_empty());
        // Empty input, one bar, and a bucket that cannot mean anything: nothing, and no panic.
        assert!(gaps(&[], 300.0).is_empty());
        assert!(gaps(&at(&[0.0]), 300.0).is_empty());
        let breaks = at(&[0.0, 300.0, 4_200.0]);
        for bucket in [0.0, -300.0, f64::NAN, f64::INFINITY] {
            assert!(gaps(&breaks, bucket).is_empty(), "bucket {bucket}");
        }
        // Two breaks come back in order.
        assert_eq!(gaps(&at(&[0.0, 300.0, 3_000.0, 3_300.0, 9_000.0]), 300.0), vec![2, 4]);
        // A day with an hour missing: the raw minutes break once, and so does the folded day, at the
        // same candle.
        let holed: Vec<Bar> = (0..390)
            .filter(|i| !(120..180).contains(i))
            .map(|i| bar((i * 60) as f32, 100.0, 100.0, 100.0, 100.0))
            .collect();
        assert_eq!(gaps(&holed, 60.0), vec![120]);
        let folded = fold(&holed, 300.0, 0.0, 0);
        assert_eq!(folded.len(), 66);
        assert_eq!(gaps(&folded, 300.0), vec![24]);
    }

    #[test]
    fn a_candles_change_is_close_over_open() {
        let c = |o: f32, close: f32| bar(0.0, o, o.max(close), o.min(close), close);
        assert!((bar_change_pct(&c(100.0, 101.0)).unwrap() - 1.0).abs() < 1e-9);
        assert!((bar_change_pct(&c(100.0, 99.0)).unwrap() + 1.0).abs() < 1e-9);
        assert_eq!(bar_change_pct(&c(100.0, 100.0)), Some(0.0));
        assert_eq!(bar_change_pct(&c(0.0, 5.0)), None, "a zero open has no change");
        assert_eq!(bar_change_pct(&c(-0.0, 5.0)), None, "nor has a negative zero open");
        assert_eq!(bar_change_pct(&c(f32::NAN, 5.0)), None);
        assert_eq!(bar_change_pct(&c(f32::INFINITY, 5.0)), None);
        assert_eq!(bar_change_pct(&c(100.0, f32::NAN)), None, "a NaN close gives no change either");
    }

    #[test]
    fn the_readout_names_time_ohlc_and_change() {
        let t0 = 20_000.0 * 86_400.0 + 13.0 * 3_600.0 + 35.0 * 60.0;
        let b = bar(0.0, 4_150.2, 4_151.0, 4_149.5, 4_150.8);
        assert_eq!(
            readout(&b, t0, 0, &two_decimals, false),
            "13:35  O 4150.20  H 4151.00  L 4149.50  C 4150.80  +0.01%"
        );
        assert!(readout(&b, t0, 19_800, &two_decimals, false).starts_with("19:05  O 4150.20"));
        // A falling candle takes a minus; a flat one reads +0.00%.
        let falling = bar(0.0, 100.0, 100.0, 98.7, 98.7);
        assert_eq!(
            readout(&falling, t0, 0, &two_decimals, false),
            "13:35  O 100.00  H 100.00  L 98.70  C 98.70  -1.30%"
        );
        let flat = bar(0.0, 100.0, 100.0, 100.0, 100.0);
        assert!(readout(&flat, t0, 0, &two_decimals, false).ends_with("  C 100.00  +0.00%"));
        // A zero open has no change: no percent and no trailing spaces.
        let no_open = bar(0.0, 0.0, 5.0, 0.0, 5.0);
        let line = readout(&no_open, t0, 0, &two_decimals, false);
        assert!(!line.contains('%') && line.ends_with("C 5.00"), "{line}");
        // A NaN close shows as NaN and no change.
        let nan_close = bar(0.0, 100.0, 100.0, 100.0, f32::NAN);
        assert!(!readout(&nan_close, t0, 0, &two_decimals, false).contains('%'));
    }

    #[test]
    fn the_compact_readout_keeps_time_close_and_change() {
        let t0 = 20_000.0 * 86_400.0 + 13.0 * 3_600.0 + 35.0 * 60.0;
        let b = bar(0.0, 4_150.2, 4_151.0, 4_149.5, 4_150.8);
        let full = readout(&b, t0, 0, &two_decimals, false);
        let compact = readout(&b, t0, 0, &two_decimals, true);
        assert_eq!(compact, "13:35  C 4150.80  +0.01%");
        assert!(compact.len() < full.len());
        assert!(!compact.contains(" O ") && !compact.contains(" H ") && !compact.contains(" L "));
        assert!(readout(&b, t0, 19_800, &two_decimals, true).starts_with("19:05  C 4150.80"));
        // No change to show: time and close only.
        let no_open = bar(0.0, 0.0, 5.0, 0.0, 5.0);
        assert_eq!(readout(&no_open, t0, 0, &two_decimals, true), "13:35  C 5.00");
    }

    #[test]
    fn folding_lands_candles_on_local_boundaries() {
        let t0 = 20_000.0 * 86_400.0; // 00:00 UTC
        // Bar i opens at i, so a candle's open and close name its first and last bar.
        let bars: Vec<Bar> = (0..300)
            .map(|i| {
                let v = i as f32;
                bar(v * 60.0, v, v + 0.5, v - 0.5, v)
            })
            .collect();
        // +05:30: the day starts at 05:30 local. The first candle is 05:00 and holds only 30 bars.
        let plus = fold(&bars, 3_600.0, t0, 19_800);
        assert_eq!(plus.len(), 6, "05, 06, 07, 08, 09 and 10 o'clock");
        assert_eq!(clock(plus[0].t(t0), 19_800), "05:00");
        assert_eq!((plus[0].o, plus[0].c), (0.0, 29.0));
        assert_eq!(plus[1].o, 30.0, "06:00 local is bar 30");
        assert_eq!(clock(plus[5].t(t0), 19_800), "10:00");
        assert_eq!((plus[5].o, plus[5].c), (270.0, 299.0));
        // -03:30: the day starts at 20:30 the evening before. The first candle is 20:00.
        let minus = fold(&bars, 3_600.0, t0, -12_600);
        assert_eq!(minus.len(), 6, "20, 21, 22, 23, 00 and 01 o'clock");
        assert_eq!(clock(minus[0].t(t0), -12_600), "20:00");
        assert_eq!((minus[0].o, minus[0].c), (0.0, 29.0));
        assert_eq!(clock(minus[4].t(t0), -12_600), "00:00");
        assert_eq!(clock(minus[5].t(t0), -12_600), "01:00");
        assert_eq!((minus[5].o, minus[5].c), (270.0, 299.0));
        for (tz, candles) in [(19_800, &plus), (-12_600, &minus)] {
            for b in candles.iter() {
                assert_eq!((t0 + b.dt as f64 + tz as f64) % 3_600.0, 0.0, "zone {tz}");
            }
        }
        // Offset 0 is exactly the output from before zones, on a day that opens off the clock too.
        for start in [t0, t0 + 13.0 * 3_600.0 + 31.0 * 60.0] {
            let day = minute_day();
            for bucket in [300.0, 900.0, 3_600.0] {
                assert_eq!(fold(&day, bucket, start, 0), fold_before_zones(&day, bucket, start), "bucket {bucket}");
            }
        }
    }

    #[test]
    fn the_clock_reads_the_zone_and_wraps_midnight() {
        assert_eq!(clock(13.0 * 3_600.0, 19_800), "18:30");
        assert_eq!(clock(23.0 * 3_600.0, 7_200), "01:00", "wraps forward past midnight");
        assert_eq!(clock(1.0 * 3_600.0, -7_200), "23:00", "wraps back past midnight");
        assert_eq!(clock(0.0, -12_600), "20:30");
        // Offset 0 is the old UTC clock: the edges of the day, and a negative time.
        assert_eq!(clock(0.0, 0), "00:00");
        assert_eq!(clock(86_399.0, 0), "23:59");
        assert_eq!(clock(-60.0, 0), "23:59");
        assert_eq!(clock(86_400.0 * 20_000.0 + 45.0 * 60.0, 0), "00:45");
    }

    #[test]
    fn fitted_with_an_offset_chooses_the_same_bucket() {
        let bars = minute_day();
        let (zero, bucket0) = fitted(&bars, 650, 0.0, 0);
        assert_eq!((bucket0, zero.len()), (300.0, 78));
        let (plus, bucket_plus) = fitted(&bars, 650, 0.0, 19_800);
        assert_eq!(bucket_plus, bucket0);
        // The count can differ by one when the day's first bar falls mid-bucket, never by more.
        assert!((plus.len() as i64 - 78).abs() <= 1, "{}", plus.len());
        // A session that opens at 13:31 UTC and is shown on a half-hour zone: still five-minute candles.
        let t0 = 20_000.0 * 86_400.0 + 13.0 * 3_600.0 + 31.0 * 60.0;
        for tz in [19_800, -12_600, 0] {
            let (candles, bucket) = fitted(&bars, 650, t0, tz);
            assert_eq!(bucket, 300.0, "zone {tz}");
            assert!((candles.len() as i64 - 78).abs() <= 1, "zone {tz}: {}", candles.len());
            for b in &candles {
                assert_eq!((t0 + b.dt as f64 + tz as f64) % 300.0, 0.0, "zone {tz}");
            }
        }
    }

    #[test]
    fn fitting_handles_no_bars_one_bar_and_a_pane_too_narrow_for_anything() {
        // No bars: no candles, and the finest bucket is the default.
        assert_eq!(fitted(&[], 650, 0.0, 0), (Vec::new(), 60.0));
        assert!(fold(&[], 300.0, 0.0, 19_800).is_empty());
        // One bar: one candle, whatever the zone.
        let one = [bar(0.0, 1.0, 2.0, 0.5, 1.5)];
        assert_eq!(fitted(&one, 650, 0.0, -12_600).0.len(), 1);
        assert!(gaps(&fold(&one, 300.0, 0.0, 0), 300.0).is_empty());
        // A pane with no width: nothing fits, so the coarsest bucket is used rather than a comb.
        assert_eq!(fitted(&minute_day(), 0, 0.0, 0).1, 3_600.0);
    }

    #[test]
    fn identical_bars_fold_flat_with_no_break_and_no_change() {
        let flat: Vec<Bar> = (0..390).map(|i| bar((i * 60) as f32, 50.0, 50.0, 50.0, 50.0)).collect();
        let (candles, bucket) = fitted(&flat, 650, 0.0, 0);
        assert_eq!((candles.len(), bucket), (78, 300.0));
        assert!(gaps(&candles, bucket).is_empty());
        for c in &candles {
            assert!(c.o == c.c && c.h == c.l && c.o == 50.0);
            assert_eq!(bar_change_pct(c), Some(0.0));
        }
        assert!(readout(&candles[0], 0.0, 0, &two_decimals, false).ends_with("+0.00%"));
    }

    #[test]
    fn fold_and_fitted_keep_the_same_bucket_for_every_zone_count_scale() {
        // The count decides the bucket: with the same bars, every zone picks the same one.
        for n in [1, 5, 60, 390, 1_000] {
            let bars: Vec<Bar> = (0..n).map(|i| bar((i * 60) as f32, 1.0, 1.0, 1.0, 1.0)).collect();
            let buckets: Vec<f64> = [0, 19_800, -12_600].iter().map(|tz| fitted(&bars, 650, 0.0, *tz).1).collect();
            assert!(buckets.iter().all(|b| *b == buckets[0]), "n {n}: {buckets:?}");
        }
    }

    /// A layout built the way the painter builds one: metrics from `candle_metrics` for `n` candles
    /// across a `plot_w` pixel plot.
    fn layout_for(plot_w: i32, n: usize, dpi: u32) -> Layout {
        let (step, body, wick) = candle_metrics(plot_w, n as i32, dpi);
        Layout {
            pane: Rect::new(0, 0, plot_w + 40, 200),
            plot: Rect::new(20, 50, 20 + plot_w, 150),
            step,
            body,
            wick,
            bars: n,
            lo: 6_655.0,
            hi: 6_733.0,
            bucket_s: 300.0,
            zone_btn: Rect::default(),
            tabs: [Rect::default(); 6],
            vol: None,
        }
    }

    #[test]
    fn every_candle_round_trips_through_x_at_several_widths_and_dpis() {
        for dpi in [96, 120, 144, 192] {
            for plot_w in [296, 400, 650, 720, 1_200, 2_000] {
                for n in [1, 4, 8, 24, 78, 96, 390] {
                    let l = layout_for(plot_w, n, dpi);
                    for i in 0..n {
                        assert_eq!(l.index_at(l.x_of(i)), Some(i), "dpi {dpi} w {plot_w} n {n} candle {i}");
                    }
                    assert_eq!(l.index_at(l.plot.left - 1), None, "one pixel left of the plot");
                    // When the candles fit, the plot's last pixel is the last candle and the next is nothing.
                    if n as i32 * l.step <= plot_w {
                        let end = l.plot.left + n as i32 * l.step;
                        assert_eq!(l.index_at(end - 1), Some(n - 1), "dpi {dpi} w {plot_w} n {n}");
                        assert_eq!(l.index_at(end), None, "dpi {dpi} w {plot_w} n {n}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_pointer_on_the_plot_edges_and_one_pixel_outside() {
        // 40 candles of 7 px fill the 280 px plot exactly.
        let l = lay();
        assert_eq!(l.index_at(l.plot.left), Some(0));
        assert_eq!(l.index_at(l.plot.left - 1), None);
        assert_eq!(l.index_at(l.plot.right - 1), Some(39));
        assert_eq!(l.index_at(l.plot.right), None);
        assert_eq!(l.pointer_at(l.plot.left, l.plot.top, 2), Some(Pointer { bar: 0, y: l.plot.top }));
        assert_eq!(l.pointer_at(l.plot.left - 1, 70, 2), None);
        assert_eq!(l.pointer_at(l.plot.right - 1, l.plot.bottom - 1, 2).map(|p| p.bar), Some(39));
        assert_eq!(l.pointer_at(l.plot.right, 70, 2), None);
        assert_eq!(l.pointer_at(l.plot.left, l.plot.bottom - 1, 1).map(|p| p.y), Some(l.plot.bottom - 1));
        assert_eq!(l.pointer_at(l.plot.left, l.plot.bottom, 1), None);
    }

    #[test]
    fn price_round_trips_through_y_within_a_pixel_at_several_sizes() {
        for (lo, hi) in [(6_655.0, 6_733.0), (0.5, 0.75), (4_150.0, 4_162.0)] {
            for px in [40, 100, 333] {
                let mut l = layout_for(650, 78, 96);
                l.lo = lo;
                l.hi = hi;
                l.plot = Rect::new(20, 50, 670, 50 + px);
                let per_px = (hi - lo) / px as f64;
                for k in 0..=50 {
                    let p = lo + (hi - lo) * k as f64 / 50.0;
                    let y = l.y_of(p);
                    assert!(y >= l.plot.top && y <= l.plot.bottom, "{p} at {y}");
                    let back = l.price_at(y).unwrap();
                    assert!((back - p).abs() <= per_px + 1e-9, "{p} came back as {back} at {px} px");
                }
            }
        }
    }

    #[test]
    fn a_flat_window_and_an_empty_layout_do_not_divide_by_zero() {
        let mut l = lay();
        l.lo = 100.0;
        l.hi = 100.0;
        assert_eq!(l.y_of(100.0), l.plot.bottom);
        assert_eq!(l.price_at(l.plot.top), Some(100.0));
        let empty = Layout { bars: 0, ..lay() };
        assert_eq!(empty.index_at(empty.plot.left), None);
        assert_eq!(empty.pointer_at(empty.plot.left, 70, 2), None);
        let no_step = Layout { step: 0, ..lay() };
        assert_eq!(no_step.index_at(no_step.plot.left), None);
    }

    // The zone button is part of the layout, so a click can be answered from it.
    #[test]
    fn the_zone_button_is_a_hit_rectangle() {
        let z = lay().zone_btn;
        assert!(z.contains(z.left, z.top) && !z.contains(z.right, z.top) && !z.contains(z.left, z.bottom));
    }

    // ---- ranges and volume (P4) ----

    #[test]
    fn civil_dates_round_trip_and_know_the_calendar() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "2000 was a leap year");
        assert_eq!(civil_from_days(days_from_civil(2100, 3, 1) - 1), (2100, 2, 28), "2100 was not");
        for d in (-800_000..800_000).step_by(997) {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn days_weeks_and_months_are_cut_on_the_local_calendar() {
        // 2026-10-15 (a Thursday) 12:00 UTC, one bar a day for 70 days.
        let t0 = days_from_civil(2026, 10, 15) as f64 * DAY_S + 12.0 * 3_600.0;
        let bars: Vec<Bar> = (0..70).map(|i| bar((i as f32) * 86_400.0, i as f32, i as f32 + 1.0, i as f32 - 1.0, i as f32 + 0.5)).collect();
        // Weeks begin on Monday: the first bucket is Mon 2026-10-12 and holds Thu, Fri, Sat, Sun.
        let weeks = fold(&bars, WEEK_S, t0, 0);
        let start = weeks[0].t(t0);
        assert_eq!(civil_from_days(local_day(start, 0)), (2026, 10, 12));
        assert_eq!(((local_day(start, 0) + 3) % 7), 0, "a Monday");
        assert_eq!((weeks[0].o, weeks[0].c), (0.0, 3.5));
        assert_eq!(weeks[1].o, 4.0, "Monday 2026-10-19 is bar 4");
        // Months are cut on the 1st: October holds bars 0..=16, November starts at bar 17.
        let months = fold(&bars, MONTH_S, t0, 0);
        assert_eq!(civil_from_days(local_day(months[0].t(t0), 0)), (2026, 10, 1));
        assert_eq!(civil_from_days(local_day(months[1].t(t0), 0)), (2026, 11, 1));
        assert_eq!(months[1].o, 17.0);
        assert_eq!(months.len(), 3, "Oct, Nov and Dec");
        // A +05:30 zone moves local midnight: 12:00 UTC is 17:30 local, still the same day, but a bar
        // at 20:00 UTC would be the next local day.
        let days = fold(&bars, DAY_S, t0, 19_800);
        assert_eq!(days.len(), 70);
        assert_eq!((t0 + days[0].dt as f64 + 19_800.0) % DAY_S, 0.0, "on a local midnight");
        // 17:00 and 19:00 UTC are 22:30 and 00:30 at +05:30: two local days.
        let late = [bar(5.0 * 3_600.0, 1.0, 1.0, 1.0, 1.0), bar(7.0 * 3_600.0, 2.0, 2.0, 2.0, 2.0)];
        assert_eq!(fold(&late, DAY_S, t0, 19_800).len(), 2);
        assert_eq!(fold(&late, DAY_S, t0, 0).len(), 1);
    }

    #[test]
    fn a_bar_exactly_on_a_local_midnight_stays_on_that_day_despite_f32() {
        // Five years out the f32 offset is 16 s coarse; the day must still be the right one.
        let t0 = days_from_civil(2021, 11, 15) as f64 * DAY_S;
        let dt = (5.0 * 365.0 * DAY_S) as f32;
        let b = [Bar { dt: dt - 8.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }];
        let day = fold(&b, DAY_S, t0, 0);
        assert_eq!(local_day(day[0].t(t0) + 30.0, 0), local_day(t0 + dt as f64, 0));
    }

    #[test]
    fn the_ladder_never_goes_finer_than_the_data() {
        let daily: Vec<Bar> = (0..250).map(|i| bar(i as f32 * 86_400.0, 1.0, 2.0, 0.5, 1.5)).collect();
        // 250 daily bars at 5 px each is 1,250 px: a 650 px plot gets weeks (36 x 5 = 180).
        let (c, b) = fitted_to(&daily, 650, 0.0, 0, 86_400.0, MONTH_S);
        assert_eq!(b, WEEK_S);
        assert!(c.len() <= 130);
        let (_, b) = fitted_to(&daily, 2_000, 0.0, 0, 86_400.0, MONTH_S);
        assert_eq!(b, DAY_S, "room for days: days, not minutes");
        // Weekly data starts at a week; a narrow plot climbs to a month.
        let weekly: Vec<Bar> = (0..260).map(|i| bar(i as f32 * WEEK_S as f32, 1.0, 2.0, 0.5, 1.5)).collect();
        assert_eq!(fitted_to(&weekly, 2_000, 0.0, 0, WEEK_S, MONTH_S).1, WEEK_S);
        assert_eq!(fitted_to(&weekly, 650, 0.0, 0, WEEK_S, MONTH_S).1, MONTH_S);
        // The 1D wrapper keeps its old ladder: nothing coarser than an hour.
        assert_eq!(fitted(&minute_day(), 0, 0.0, 0).1, 3_600.0);
    }

    #[test]
    fn volume_is_summed_when_candles_fold() {
        let bars: Vec<Bar> = (0..10).map(|i| Bar { dt: i as f32 * 60.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 10.0 }).collect();
        let folded = fold(&bars, 300.0, 0.0, 0);
        assert_eq!(folded.iter().map(|b| b.v).collect::<Vec<_>>(), [10.0 * 5.0, 10.0 * 5.0]);
    }

    #[test]
    fn the_volume_strip_is_the_bottom_fifth_and_price_still_round_trips() {
        let plot = Rect::new(20, 50, 320, 250);
        let (price, vol) = split_plot(plot, true, 3);
        let vol = vol.expect("a strip");
        assert_eq!((vol.top, vol.bottom, vol.left, vol.right), (210, 250, 20, 320));
        assert_eq!(price, Rect::new(20, 50, 320, 207));
        assert_eq!(split_plot(plot, false, 3), (plot, None), "no volume, no change");
        assert_eq!(split_plot(Rect::new(0, 0, 100, 15), true, 3).1, None, "too short for both");
        let mut l = lay();
        l.plot = price;
        l.vol = Some(vol);
        assert_eq!(l.base(), 250);
        let per_px = (l.hi - l.lo) / (price.bottom - price.top) as f64;
        for p in [100.0, 133.3, 199.9] {
            assert!((l.price_at(l.y_of(p)).unwrap() - p).abs() <= per_px);
        }
        assert_eq!(l.price_at(vol.top), None, "the strip is not the price plot");
        assert_eq!(volume_px(0.0, 10.0, 40), 0);
        assert_eq!(volume_px(f32::NAN, 10.0, 40), 0);
        assert_eq!(volume_px(0.001, 10.0, 40), 1, "any volume shows");
        assert_eq!(volume_px(10.0, 10.0, 40), 40);
        assert_eq!(volume_px(5.0, 10.0, 40), 20);
    }

    #[test]
    fn date_ticks_take_months_then_years_then_days() {
        let t0 = days_from_civil(2026, 1, 1) as f64 * DAY_S;
        let at = |days: &[i64]| -> Vec<Bar> { days.iter().map(|d| bar((*d as f64 * DAY_S) as f32, 1.0, 1.0, 1.0, 1.0)).collect() };
        let w = |s: &str| s.len() as i32 * 7;
        // Daily bars across Oct-Dec 2026 at 4 px a bar: month starts, labelled by name.
        let t0b = days_from_civil(2026, 10, 1) as f64 * DAY_S;
        let all: Vec<i64> = (0..92).collect();
        let ticks = date_ticks(&at(&all), t0b, 0, 8, &w, 8);
        assert_eq!(ticks.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(), ["Nov", "Dec"]);
        assert_eq!(ticks[0].0, 31);
        // A January is labelled with its year.
        let t0c = days_from_civil(2026, 12, 1) as f64 * DAY_S;
        let ticks = date_ticks(&at(&(0..70).collect::<Vec<_>>()), t0c, 0, 8, &w, 8);
        assert_eq!(ticks.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(), ["2027", "Feb"]);
        // Months too close for their labels: years only.
        let weekly: Vec<i64> = (0..260).map(|i| i * 7).collect();
        let ticks = date_ticks(&at(&weekly), t0, 0, 1, &w, 8);
        assert!(ticks.iter().all(|(_, l)| l.len() == 4), "{ticks:?}");
        // A window inside one month: dates, spaced by their width.
        let ticks = date_ticks(&at(&(0..20).collect::<Vec<_>>()), t0, 0, 10, &w, 8);
        assert!(ticks.len() >= 2 && ticks[0].1 == "Jan 1", "{ticks:?}");
        assert!(date_ticks(&[], t0, 0, 8, &w, 8).is_empty());
    }

    #[test]
    fn dates_read_in_the_zone() {
        let t = days_from_civil(2026, 11, 12) as f64 * DAY_S + 23.0 * 3_600.0;
        assert_eq!(date_label(t, 0, DateStyle::DayMonth), "Nov 12");
        assert_eq!(date_label(t, 7_200, DateStyle::DayMonth), "Nov 13", "23:00 UTC is already tomorrow at +02:00");
        assert_eq!(date_label(t, 0, DateStyle::Month), "Nov");
        assert_eq!(date_label(t, 0, DateStyle::Year), "2026");
        assert_eq!(date_label(t, 0, DateStyle::Full), "Nov 12 2026");
        assert_eq!(bucket_name(DAY_S), "1 d");
        assert_eq!(bucket_name(WEEK_S), "1 w");
        assert_eq!(bucket_name(MONTH_S), "1 mo");
        assert_eq!(bucket_name(300.0), "5 m");
        assert!(marks_breaks(3_600.0) && !marks_breaks(DAY_S));
    }

    #[test]
    fn the_legend_dates_long_candles() {
        // 12 Nov 2026 13:35 UTC.
        let t0 = days_from_civil(2026, 11, 12) as f64 * DAY_S + 13.0 * 3_600.0 + 35.0 * 60.0;
        let b = bar(0.0, 4_150.2, 4_151.0, 4_149.5, 4_150.8);
        let dm = readout_with(&b, t0, 0, &two_decimals, false, Stamp::DayClock);
        assert!(dm.starts_with("Nov 12 13:35  O 4150.20"), "{dm}");
        let day = readout_with(&b, t0, 0, &two_decimals, false, Stamp::Day);
        assert!(day.starts_with("Nov 12 2026  O 4150.20"), "{day}");
        assert!(!day.contains(':'), "a day has no clock: {day}");
        assert_eq!(readout_with(&b, t0, 0, &two_decimals, false, Stamp::Clock), readout(&b, t0, 0, &two_decimals, false));
        // The zone moves the date too: 13:35 UTC at +11:00 is 00:35 on the 13th.
        assert!(readout_with(&b, t0, 11 * 3_600, &two_decimals, false, Stamp::DayClock).starts_with("Nov 13 00:35"));
        // Which stamp a range gets.
        assert_eq!(stamp_for(true, 300.0), Stamp::Clock);
        assert_eq!(stamp_for(false, 900.0), Stamp::DayClock);
        assert_eq!(stamp_for(false, DAY_S), Stamp::Day);
        assert_eq!(stamp_for(false, WEEK_S), Stamp::Day);
    }

    #[test]
    fn a_series_with_no_volume_lays_out_as_before() {
        let plot = Rect::new(20, 50, 320, 150);
        // Without volume the plot is untouched, whatever the gap.
        assert_eq!(split_plot(plot, false, 3), (plot, None));
        let mut l = layout_for(300, 20, 96);
        l.plot = plot;
        l.vol = None;
        assert_eq!(l.base(), plot.bottom, "the time axis hangs from the plot");
        // Volume-less bars fold to zero volume everywhere.
        let bars: Vec<Bar> = (0..10).map(|i| bar(i as f32 * 60.0, 1.0, 2.0, 0.5, 1.5)).collect();
        let folded = fold(&bars, 300.0, 0.0, 0);
        assert!(!folded.is_empty());
        assert!(folded.iter().all(|b| b.v == 0.0));
        // And with volume the strip takes the bottom fifth, and base() follows it.
        let (price, strip) = split_plot(plot, true, 3);
        let strip = strip.expect("a strip when there is volume");
        assert_eq!(strip.bottom, plot.bottom);
        assert_eq!(price.bottom, strip.top - 3);
        l.plot = price;
        l.vol = Some(strip);
        assert_eq!(l.base(), plot.bottom);
    }

    #[test]
    fn tab_at_finds_the_tab_under_the_pointer() {
        let mut l = layout_for(300, 20, 96);
        // Six adjacent rectangles, 10 px wide.
        for i in 0..6 {
            l.tabs[i] = Rect::new(i as i32 * 10, 0, i as i32 * 10 + 10, 10);
        }
        for i in 0..6 {
            let x = i as i32 * 10;
            assert_eq!(l.tab_at(x, 0), Some(i), "left edge of tab {i}");
            assert_eq!(l.tab_at(x + 9, 9), Some(i), "last pixel of tab {i}");
        }
        assert_eq!(l.tab_at(60, 5), None, "right of the last tab");
        assert_eq!(l.tab_at(5, 10), None, "below the tabs");
        assert_eq!(l.tab_at(-1, 5), None, "left of the first tab");
        // With a gap between them, the gap belongs to no tab.
        for i in 0..6 {
            l.tabs[i] = Rect::new(i as i32 * 12, 0, i as i32 * 12 + 10, 10);
        }
        assert_eq!(l.tab_at(10, 5), None, "the gap");
        assert_eq!(l.tab_at(12, 5), Some(1));
        // A default Rect (a tab not drawn) hits nothing.
        l.tabs = [Rect::default(); 6];
        assert_eq!(l.tab_at(0, 0), None);
    }

    #[test]
    fn daily_bars_are_dated_by_their_exchange_not_by_the_viewer() {
        let nov12 = days_from_civil(2026, 11, 12) as f64 * DAY_S;
        // The same trading day stamped at midnight in New York (05:00 UTC), Tokyo (15:00 UTC the day
        // before), London (00:00 UTC) and Auckland in winter (12:00 UTC the day before).
        for stamp in [nov12 + 5.0 * 3_600.0, nov12 - 9.0 * 3_600.0, nov12, nov12 - 12.0 * 3_600.0] {
            for viewer in [-8 * 3_600, 0, 8 * 3_600] {
                let zone = date_zone(DAY_S, viewer);
                assert_eq!(date_label(stamp, zone, DateStyle::Full), "Nov 12 2026", "stamp {stamp} viewer {viewer}");
            }
        }
        // Weekly bars count as daily-or-longer; a daily series a shade under 24 h (the tolerance) too.
        assert_eq!(date_zone(WEEK_S, -8 * 3_600), 43_200);
        assert!(daily(DAY_S * 0.96) && !daily(DAY_S * 0.9));
        // Intraday bars are instants: the viewer's clock, whatever it is.
        for viewer in [-8 * 3_600, 0, 19_800] {
            assert_eq!(date_zone(60.0, viewer), viewer);
            assert_eq!(date_zone(14_400.0, viewer), viewer);
        }
        assert_eq!(date_zone(0.0, 3_600), 3_600, "unknown spacing is treated as intraday");
    }

    #[test]
    fn a_week_of_daily_bars_is_one_candle_for_a_viewer_west_of_the_exchange() {
        // Two weeks of New York daily bars (Mon Nov 2 .. Fri Nov 13 2026), each stamped 05:00 UTC.
        let mon = days_from_civil(2026, 11, 2);
        assert_eq!((mon + 3).rem_euclid(7), 0, "Nov 2 2026 is a Monday");
        let t0 = mon as f64 * DAY_S + 5.0 * 3_600.0;
        let bars: Vec<Bar> = (0..12)
            .filter(|d| d % 7 < 5)
            .map(|d| Bar { dt: d as f32 * 86_400.0, o: 1.0, h: 2.0, l: 0.5, c: 1.5, v: 10.0 })
            .collect();
        assert_eq!(bars.len(), 10);
        let viewer = -8 * 3_600; // Los Angeles: 05:00 UTC is 21:00 the evening before
        let weeks = fold(&bars, WEEK_S, t0, date_zone(DAY_S, viewer));
        assert_eq!(weeks.len(), 2, "Monday to Friday is one candle");
        assert_eq!(weeks[0].v, 50.0);
        assert_eq!(weeks[1].v, 50.0);
        // The viewer's own clock would have filed each Monday under the week before: three candles.
        assert_eq!(fold(&bars, WEEK_S, t0, viewer).len(), 3);
        // And each daily candle keeps its exchange date.
        let days = fold(&bars, DAY_S, t0, date_zone(DAY_S, viewer));
        assert_eq!(days.len(), 10);
        assert_eq!(date_label(days[0].t(t0) + BIAS_S, date_zone(DAY_S, viewer), DateStyle::Full), "Nov 2 2026");
        assert_eq!(date_label(days[9].t(t0) + BIAS_S, date_zone(DAY_S, viewer), DateStyle::Full), "Nov 13 2026");
    }
}
