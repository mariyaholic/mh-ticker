//! Chart ranges (1D .. 5Y): which file serves each, the series cache, and the request state machine.
//!
//! Pure and portable: `crate::model::Bar` and `std` only. No clock (`now` is a parameter), no window
//! handle, no thread: the worker thread lives in `source.rs`, and talks to the UI through [`Slots`].
//!
//! How the pieces fit:
//!
//! ```text
//!   UI thread                         worker thread ("ranges", source.rs)
//!   Book::want(key, now) -> Request
//!   Slots::submit(request)  ------->  Slots::take_request()   (polls every ROWS_CHECK)
//!                                     ... fetch + parse (no lock held) ...
//!   WM_RANGE  <-------------------    Slots::deliver(Done), PostMessageW
//!   Slots::take_done() -> Book::arrived(generation, key, result, now)
//! ```
//!
//! Two rules decide every race, and both are enforced here, not by the caller:
//!
//! * A request replaced in the slot before the worker picked it up is gone. [`Slots::submit`] returns
//!   it so the caller can [`Book::cancel`] it; its key is then free to be asked for again.
//! * A result is cached only if its generation is the one the book has in flight *for that key*. The
//!   user having moved to another range or card meanwhile does not matter (the data is good and
//!   cached); a request the book forgot (rows changed, cancelled, lost) is dropped.

use crate::model::Bar;
use std::cell::Cell;
use std::sync::Arc;

/// The six ranges the pane offers, in tab order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    /// The day of 1-minute bars the board already holds: no file, no request.
    #[default]
    D1,
    D5,
    M1,
    M6,
    Y1,
    Y5,
}

const DAY_S: f64 = 86_400.0;

impl Range {
    /// Tab order, left to right.
    pub const ALL: [Range; 6] = [Range::D1, Range::D5, Range::M1, Range::M6, Range::Y1, Range::Y5];

    /// The tab text: "1D" "5D" "1M" "6M" "1Y" "5Y".
    pub fn label(self) -> &'static str {
        match self {
            Range::D1 => "1D",
            Range::D5 => "5D",
            Range::M1 => "1M",
            Range::M6 => "6M",
            Range::Y1 => "1Y",
            Range::Y5 => "5Y",
        }
    }

    /// The inverse of [`Range::label`] (case-insensitive); `None` for anything else.
    pub fn parse(s: &str) -> Option<Range> {
        Range::ALL.into_iter().find(|r| r.label().eq_ignore_ascii_case(s.trim()))
    }

    /// The range a saved `chart_range` names: `1D` when there is none or it names no range.
    pub fn from_saved(saved: Option<&str>) -> Range {
        saved.and_then(Range::parse).unwrap_or_default()
    }

    /// What `chart_range` holds for this range: `None` for `1D` (the default is never written).
    pub fn to_saved(self) -> Option<String> {
        (self != Range::D1).then(|| self.label().to_string())
    }

    /// The file under `.../charts/` that serves this range. `M1` and `M6` share `1M.json`, which
    /// carries about a year of daily bars: the range only trims it. `D1` has no file.
    pub fn file(self) -> Option<&'static str> {
        match self {
            Range::D1 => None,
            Range::D5 => Some("5D.json"),
            Range::M1 | Range::M6 => Some("1M.json"),
            Range::Y1 => Some("1Y.json"),
            Range::Y5 => Some("5Y.json"),
        }
    }

    /// How far back the range shows, in seconds: 1 day, 5, 31, 183, 366 and 5 x 366 days.
    pub fn span_s(self) -> f64 {
        DAY_S
            * match self {
                Range::D1 => 1.0,
                Range::D5 => 5.0,
                Range::M1 => 31.0,
                Range::M6 => 183.0,
                Range::Y1 => 366.0,
                Range::Y5 => 5.0 * 366.0,
            }
    }

    /// How long a fetched file is good for: 10 minutes for the intraday file, an hour for the daily
    /// ones, six hours for `5Y`. `D1` is the board's own business: 10 minutes, its refresh cadence.
    #[allow(dead_code)] // the book reads `ttl_of(file)`; kept as the range's own answer
    pub fn ttl_s(self) -> f64 {
        self.file().map_or(600.0, ttl_of)
    }

    /// The cache key for `symbol` at this range; `None` for `D1`.
    pub fn key(self, symbol: &str) -> Option<Key> {
        self.file().map(|file| Key { symbol: symbol.to_string(), file })
    }
}

/// The time to live of a fetched file, by file name: 10 minutes for `5D.json` (and anything unknown),
/// an hour for the daily files, six hours for `5Y.json`.
pub fn ttl_of(file: &str) -> f64 {
    match file {
        "1M.json" | "1Y.json" => 3_600.0,
        "5Y.json" => 21_600.0,
        _ => 600.0,
    }
}

/// What a fetch is for: one symbol's one file. `M1` and `M6` of one symbol are one key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub symbol: String,
    pub file: &'static str,
}

/// Do the stamps of a daily file sit on a day boundary? A daily bar is stamped at midnight on its
/// exchange's clock, so each stamp's time of day (UTC) is a whole quarter hour (every zone offset
/// is), and all of them agree to within the hour a daylight-saving change moves it. `stamps` are UTC
/// seconds. `true` for an empty list. Tolerance 30 s each way: a stamp five years out has passed
/// through an `f32` offset, which is 16 s coarse. This is the assumption `--probe` and the pane's
/// *check dates* note judge; nothing live has verified it.
pub fn stamps_on_day_boundary(stamps: &[f64]) -> bool {
    const TOL: f64 = 30.0;
    let Some(first) = stamps.iter().copied().find(|t| t.is_finite()) else {
        return stamps.iter().all(|t| t.is_finite());
    };
    let tod = |t: f64| t.rem_euclid(DAY_S);
    stamps.iter().all(|&t| {
        if !t.is_finite() {
            return false;
        }
        let r = tod(t) % 900.0;
        let off_grid = r.min(900.0 - r) > TOL;
        let apart = (tod(t) - tod(first)).abs();
        let apart = apart.min(DAY_S - apart);
        !off_grid && apart <= 3_600.0 + TOL
    })
}

/// How long past its time to live a series may be before the pane says so: a refresh that is on its
/// way (a request in flight) is not worth a note, one that has failed is.
pub const STALE_GRACE_S: f64 = RETRY_AFTER_S;

/// `as of 14:05` when the series on screen was fetched more than its time to live (plus
/// [`STALE_GRACE_S`]) ago, on the clock `tz_offset_s` east of UTC; `as of Nov 12 14:05` when that is a
/// day or more back; `None` while it is fresh or when a time is not a number.
pub fn age_label(now: f64, fetched: f64, ttl_s: f64, tz_offset_s: i32) -> Option<String> {
    if !(now.is_finite() && fetched.is_finite() && ttl_s.is_finite()) || now - fetched <= ttl_s + STALE_GRACE_S {
        return None;
    }
    let clock = crate::chartview::clock(fetched, tz_offset_s);
    Some(if now - fetched >= DAY_S * 0.9 {
        format!("as of {} {clock}", crate::chartview::date_label(fetched, tz_offset_s, crate::chartview::DateStyle::DayMonth))
    } else {
        format!("as of {clock}")
    })
}

/// What a series says about itself that the reader should know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Check {
    /// A daily file whose stamps are not on a day boundary: its candles may be dated a day off.
    pub dates_off: bool,
}

/// One fetched file, parsed. Bar times are offsets (`Bar::dt`) from `t0`, and each is the START of
/// its bar (unlike the 1D path, which stamps bars at their end).
#[derive(Clone, Debug)]
pub struct Series {
    /// UTC seconds of the first bar's start.
    pub t0: f64,
    pub bars: Arc<[Bar]>,
    /// Seconds one bar lasts: the median spacing of consecutive bars, inferred, never assumed.
    /// 0.0 when it cannot be told (fewer than two bars).
    pub bar_s: f64,
    /// UTC seconds the file was fetched; the time to live counts from here.
    pub fetched: f64,
}

impl Series {
    /// A series from parsed bars (in time order, `dt` relative to `t0`).
    pub fn new(t0: f64, bars: Vec<Bar>, fetched: f64) -> Series {
        let bar_s = median_spacing(&bars);
        Series { t0, bars: bars.into(), bar_s, fetched }
    }

    /// Flags computed from the bars: nothing is stored. Only a daily or longer series (`bar_s` of 0.95
    /// day or more) can have its dates off; an intraday one never does.
    pub fn check(&self) -> Check {
        if self.bar_s < DAY_S * 0.95 || self.bars.is_empty() {
            return Check::default();
        }
        let stamps: Vec<f64> = self.bars.iter().map(|b| b.t(self.t0)).collect();
        Check { dates_off: !stamps_on_day_boundary(&stamps) }
    }

    /// The note for the pane's header, or `""`: `as of 14:05`, `check dates`, both joined by `" \u{b7} "`.
    pub fn note(&self, now: f64, ttl_s: f64, tz_offset_s: i32) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(age_label(now, self.fetched, ttl_s, tz_offset_s));
        if self.check().dates_off {
            parts.push("check dates".to_string());
        }
        parts.join(" \u{b7} ")
    }

    /// The bars whose start is at or after `now - span_s`, as a slice of `bars` (the tail).
    /// A bar exactly `span_s` old is inside. Everything is inside when `span_s` is huge; nothing
    /// when `span_s` is negative or NaN. Never panics, never reorders.
    pub fn window(&self, now: f64, span_s: f64) -> &[Bar] {
        // Negative or NaN: nothing is inside.
        if span_s.is_nan() || span_s < 0.0 {
            return &[];
        }
        let cutoff = now - span_s;
        let first = self.bars.partition_point(|b| b.t(self.t0) < cutoff);
        &self.bars[first..]
    }
}

/// The median of the gaps between consecutive bar starts (`dt` differences), in seconds. For an even
/// count of gaps the mean of the two middle ones. `0.0` for fewer than two bars. Gaps that are not
/// positive and finite (duplicates, unsorted input) are ignored. Median, not mean: a weekend or a
/// holiday is one long gap among hundreds of ordinary ones and must not move the answer.
pub(crate) fn median_spacing(bars: &[Bar]) -> f64 {
    let mut gaps: Vec<f64> = bars
        .windows(2)
        .map(|w| w[1].dt as f64 - w[0].dt as f64)
        .filter(|g| g.is_finite() && *g > 0.0)
        .collect();
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_by(|a, b| a.total_cmp(b));
    let n = gaps.len();
    if n % 2 == 1 {
        gaps[n / 2]
    } else {
        (gaps[n / 2 - 1] + gaps[n / 2]) / 2.0
    }
}

/// What the pane should say about one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Cached, bars present. May be older than its time to live while a refresh is on its way.
    Ready,
    /// Not cached and a request is in flight (or about to be made).
    Loading,
    /// Not cached and the last attempt failed.
    Failed,
    /// Cached, but the file held no bars.
    Empty,
}

/// A fetch the UI asks the worker for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub key: Key,
}

/// A fetch the worker finished.
#[derive(Clone, Debug)]
pub struct Done {
    pub generation: u64,
    pub key: Key,
    pub result: Result<Series, String>,
}

/// At most this many series are kept; the least recently used goes first. 48 is two ranges for each of
/// the 24 symbols the strip can hold, so coming back to a symbol and a range already seen is instant.
/// Only the series on screen is ever asked for (`Book::want` is called for that key alone), so more
/// room buys no traffic.
pub const MAX_SERIES: usize = 48;
/// What a "full" series is sized at, in bars: about 1,100. A daily file is 260 bars to the year and an
/// intraday one a few hundred; the parser's own ceiling (`SERIES_BARS`) is a guard against a payload
/// that is not a chart, not a size to plan for.
#[cfg_attr(not(test), allow(dead_code))] // the size arithmetic's input; only tests read it
pub const FULL_SERIES_BARS: usize = 1_100;
/// A bar in memory: six `f32`.
pub const BAR_BYTES: usize = std::mem::size_of::<Bar>();
/// The cache's other ceiling, in bar bytes: 2 MB. [`MAX_SERIES`] full series come to 1.27 MB, so this
/// only bites when files are far bigger than planned for (a 4,000-bar series is 96 KB); then the least
/// recently used series go until the rest fit, and the newest always stays.
pub const MAX_CACHE_BYTES: usize = 2_000_000;
/// After a failure, the same key is not asked for again for this long.
pub const RETRY_AFTER_S: f64 = 30.0;
/// A request that has been in flight this long is presumed lost (the worker's own timeout is 45 s),
/// so the key can be asked for again. A late answer to it is then dropped (its generation is gone).
pub const LOST_AFTER_S: f64 = 120.0;

struct Cached {
    key: Key,
    series: Series,
    /// `Book::tick` at the last read; a `Cell` so `get(&self)` can mark use while the painter reads.
    used: Cell<u64>,
}

struct Flying {
    key: Key,
    generation: u64,
    since: f64,
}

/// The cache and the request state machine. One per `Ui`; lives as long as the process (the cache
/// outlives the pane).
///
/// Transitions, for one key:
///
/// ```text
///  absent --want--> in flight --arrived(Ok)--> cached --(ttl passes)--want--> cached + in flight
///     ^                 |  `----arrived(Err)--> failed(at) --(30 s)--want--> in flight
///     `-- forget / cancel / lost (120 s) ------'
/// ```
#[derive(Default)]
pub struct Book {
    cache: Vec<Cached>,
    flying: Vec<Flying>,
    failed: Vec<(Key, f64)>,
    tick: Cell<u64>,
    next_generation: u64,
}

impl Book {
    pub fn new() -> Book {
        Book::default()
    }

    /// Is a fetch for `key` due, and if so the request to send (the key is then in flight).
    ///
    /// Due when: the key is not cached or its series is older than the file's time to live
    /// ([`ttl_of`]); and nothing is in flight
    /// for it (an in-flight entry older than [`LOST_AFTER_S`] is presumed lost and replaced); and it
    /// did not fail within the last [`RETRY_AFTER_S`]. Generations start at 1 and only grow.
    pub fn want(&mut self, key: &Key, now: f64) -> Option<Request> {
        let ttl_s = ttl_of(key.file);
        self.flying.retain(|f| now - f.since < LOST_AFTER_S);
        if self.flying.iter().any(|f| &f.key == key) {
            return None;
        }
        if let Some((_, at)) = self.failed.iter().find(|(k, _)| k == key) {
            if now - at < RETRY_AFTER_S {
                return None;
            }
        }
        if let Some(c) = self.cache.iter().find(|c| &c.key == key) {
            if now - c.series.fetched < ttl_s {
                return None;
            }
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        self.flying.push(Flying { key: key.clone(), generation, since: now });
        Some(Request { generation, key: key.clone() })
    }

    /// A request was dropped before the worker took it (see [`Slots::submit`]): its key is free again.
    pub fn cancel(&mut self, generation: u64) {
        self.flying.retain(|f| f.generation != generation);
    }

    /// The worker's answer. Accepted (and `true`) only when `generation` is the one in flight for
    /// `key`. Then: `Ok` is cached (an empty series too, so the pane can say "no data" and the time
    /// to live stops the file being asked for in a loop) and clears the key's failure; `Err` records
    /// the failure at `now`. The 49th cached series evicts the least recently used one. Anything else
    /// is dropped and `false`: a stale generation, a forgotten key, a repeat delivery.
    pub fn arrived(&mut self, generation: u64, key: &Key, result: Result<Series, String>, now: f64) -> bool {
        let Some(i) = self.flying.iter().position(|f| f.generation == generation && &f.key == key) else {
            return false;
        };
        self.flying.remove(i);
        match result {
            Ok(series) => {
                self.failed.retain(|(k, _)| k != key);
                self.cache.retain(|c| &c.key != key);
                if self.cache.len() >= MAX_SERIES {
                    if let Some(lru) = self.cache.iter().enumerate().min_by_key(|(_, c)| c.used.get()).map(|(i, _)| i) {
                        self.cache.remove(lru);
                    }
                }
                self.tick.set(self.tick.get() + 1);
                self.cache.push(Cached { key: key.clone(), series, used: Cell::new(self.tick.get()) });
                // Over the byte ceiling: drop the least recently used, never the series just stored.
                while self.cache.len() > 1 && self.bytes() > MAX_CACHE_BYTES {
                    let newest = self.cache.len() - 1;
                    let lru = self.cache[..newest].iter().enumerate().min_by_key(|(_, c)| c.used.get()).map(|(i, _)| i);
                    match lru {
                        Some(i) => {
                            self.cache.remove(i);
                        }
                        None => break,
                    }
                }
            }
            Err(_) => {
                self.failed.retain(|(k, _)| k != key);
                self.failed.push((key.clone(), now));
            }
        }
        true
    }

    /// What to say about `key`: see [`Status`]. Cached wins (even past its time to live, even with a
    /// newer failure on record: old data beats a spinner); then a recorded failure; else loading.
    pub fn status(&self, key: &Key) -> Status {
        if let Some(c) = self.cache.iter().find(|c| &c.key == key) {
            return if c.series.bars.is_empty() { Status::Empty } else { Status::Ready };
        }
        if self.failed.iter().any(|(k, _)| k == key) {
            Status::Failed
        } else {
            Status::Loading
        }
    }

    /// The cached series, marking it most recently used.
    pub fn get(&self, key: &Key) -> Option<&Series> {
        let c = self.cache.iter().find(|c| &c.key == key)?;
        self.tick.set(self.tick.get() + 1);
        c.used.set(self.tick.get());
        Some(&c.series)
    }

    /// The rows changed: keep only what belongs to a symbol still shown. Cached series, failures and
    /// in-flight requests of every other symbol are removed (their late answers will be dropped).
    pub fn forget(&mut self, symbols_still_shown: &[&str]) {
        let keep = |k: &Key| symbols_still_shown.contains(&k.symbol.as_str());
        self.cache.retain(|c| keep(&c.key));
        self.flying.retain(|f| keep(&f.key));
        self.failed.retain(|(k, _)| keep(k));
    }

    /// *Refresh now*: forget every recorded failure, so the next `want` for a failed key asks again at
    /// once instead of waiting out [`RETRY_AFTER_S`]. The cache and the requests in flight are kept
    /// (a key in flight is not asked for twice). Without this the menu's refresh woke the quote and
    /// day-chart loops but left a long range saying "failed" for up to 30 s.
    pub fn retry_now(&mut self) {
        self.failed.clear();
    }

    /// The bars held, in bytes.
    fn bytes(&self) -> usize {
        self.cache.iter().map(|c| c.series.bars.len() * BAR_BYTES).sum()
    }

    /// How many series are cached.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

/// The two slots the UI and the worker share, behind one `Arc<Mutex<_>>` (the lock is held only to
/// move a value in or out, never across I/O).
///
/// * `request`: at most one request waiting for the worker. A new one **replaces** it: the user
///   clicked 5D, then 1Y, and the 5D fetch that never started is simply not made.
/// * `done`: results the UI has not read yet, in order. The worker never waits for the UI.
#[derive(Default)]
pub struct Slots {
    request: Option<Request>,
    done: Vec<Done>,
}

impl Slots {
    /// Put `req` in the request slot and return the request it replaced, if the worker had not
    /// picked it up yet. The caller passes the replaced generation to [`Book::cancel`].
    pub fn submit(&mut self, req: Request) -> Option<Request> {
        self.request.replace(req)
    }

    /// The worker takes the waiting request, if any. A request being fetched is no longer here: it
    /// finishes and is cached when it arrives.
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    /// The worker hands over a result.
    pub fn deliver(&mut self, done: Done) {
        self.done.push(done);
    }

    /// The UI drains the results, oldest first.
    pub fn take_done(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.done)
    }

    /// Take back the waiting request (the rows changed, so it may be for a symbol that is gone).
    /// A fetch already running still delivers; the book then drops it if it was forgotten.
    pub fn clear_request(&mut self) -> Option<Request> {
        self.request.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sym: &str, file: &'static str) -> Key {
        Key { symbol: sym.into(), file }
    }

    fn series(fetched: f64, n: usize) -> Series {
        let bars: Vec<Bar> = (0..n).map(|i| Bar { dt: i as f32 * 86_400.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }).collect();
        Series { t0: 0.0, bars: bars.into(), bar_s: 86_400.0, fetched }
    }

    #[test]
    fn a_full_select_arrive_expire_refresh_cycle() {
        let mut b = Book::new();
        let k = key("@SP.1", "5D.json");
        assert_eq!(b.status(&k), Status::Loading);
        let r = b.want(&k, 1_000.0).expect("due at once");
        assert_eq!(r.generation, 1);
        assert!(b.want(&k, 1_001.0).is_none(), "one in flight per key");
        assert!(b.arrived(r.generation, &k, Ok(series(1_005.0, 3)), 1_005.0));
        assert_eq!(b.status(&k), Status::Ready);
        assert!(b.want(&k, 1_300.0).is_none(), "inside the time to live");
        let again = b.want(&k, 1_606.0).expect("expired: refresh");
        assert_eq!(again.generation, 2);
        assert_eq!(b.status(&k), Status::Ready, "old data stays up while it refreshes");
        assert!(b.arrived(2, &k, Ok(series(1_610.0, 4)), 1_610.0));
        assert_eq!(b.get(&k).map(|s| s.bars.len()), Some(4));
        assert_eq!(b.len(), 1);
    }

    /// Audit 1.8, finding 1: *Refresh now* must reach a long range that just failed, not leave the
    /// pane saying "failed" until the 30 s back-off ends. Cache and in-flight state are untouched.
    #[test]
    fn refresh_now_asks_again_for_a_failed_key_and_keeps_the_rest() {
        let mut b = Book::new();
        let (bad, good, busy) = (key("A", "5D.json"), key("B", "1Y.json"), key("C", "5Y.json"));
        let r = b.want(&bad, 0.0).unwrap();
        assert!(b.arrived(r.generation, &bad, Err("network".into()), 1.0));
        let g = b.want(&good, 2.0).unwrap();
        assert!(b.arrived(g.generation, &good, Ok(series(3.0, 2)), 3.0));
        let flying = b.want(&busy, 4.0).unwrap();
        assert!(b.want(&bad, 5.0).is_none(), "inside the back-off");
        b.retry_now();
        assert_eq!(b.status(&good), Status::Ready, "the cache stays");
        assert!(b.want(&good, 6.0).is_none(), "a fresh cached series is not refetched");
        assert!(b.want(&busy, 6.0).is_none(), "a key in flight is not asked for twice");
        let again = b.want(&bad, 6.0).expect("the failed key is due at once");
        assert!(again.generation > flying.generation);
        assert_eq!(b.status(&bad), Status::Loading);
    }

    #[test]
    fn a_stale_generation_is_dropped_and_a_late_answer_for_a_left_key_is_cached() {
        let mut b = Book::new();
        let (a, c) = (key("A", "5D.json"), key("A", "1Y.json"));
        let ra = b.want(&a, 0.0).unwrap();
        let rc = b.want(&c, 1.0).unwrap(); // the user moved on; A is still being fetched
        assert!(b.arrived(ra.generation, &a, Ok(series(5.0, 2)), 5.0), "late but good: cached");
        assert_eq!(b.status(&a), Status::Ready);
        assert!(!b.arrived(99, &c, Ok(series(5.0, 2)), 6.0), "a generation nobody asked for");
        assert_eq!(b.status(&c), Status::Loading);
        assert!(b.arrived(rc.generation, &c, Ok(series(7.0, 2)), 7.0));
        assert!(!b.arrived(rc.generation, &c, Ok(series(8.0, 9)), 8.0), "a repeat delivery");
        assert_eq!(b.get(&c).map(|s| s.bars.len()), Some(2));
    }

    #[test]
    fn a_replaced_request_is_cancelled_and_may_be_asked_for_again() {
        let mut b = Book::new();
        let mut slots = Slots::default();
        let (a, c) = (key("A", "5D.json"), key("A", "1Y.json"));
        let ra = b.want(&a, 0.0).unwrap();
        assert!(slots.submit(ra.clone()).is_none());
        let rc = b.want(&c, 1.0).unwrap();
        let replaced = slots.submit(rc.clone()).expect("the worker had not taken it");
        assert_eq!(replaced, ra);
        b.cancel(replaced.generation);
        assert_eq!(slots.take_request(), Some(rc));
        assert!(slots.take_request().is_none());
        assert!(b.want(&a, 2.0).is_some(), "A is free again");
    }

    #[test]
    fn a_failure_backs_off_for_thirty_seconds() {
        let mut b = Book::new();
        let k = key("A", "5Y.json");
        let r = b.want(&k, 100.0).unwrap();
        assert!(b.arrived(r.generation, &k, Err("HTTP 500".into()), 110.0));
        assert_eq!(b.status(&k), Status::Failed);
        assert!(b.want(&k, 139.0).is_none(), "29 s after the failure");
        let r2 = b.want(&k, 140.0).expect("30 s after the failure");
        assert_eq!(b.status(&k), Status::Failed, "still failed while the retry runs");
        assert!(b.arrived(r2.generation, &k, Ok(series(141.0, 2)), 141.0));
        assert_eq!(b.status(&k), Status::Ready);
    }

    #[test]
    fn the_saved_range_round_trips_and_an_unknown_one_reads_as_one_day() {
        for r in Range::ALL {
            assert_eq!(Range::from_saved(r.to_saved().as_deref()), r, "{}", r.label());
        }
        assert_eq!(Range::D1.to_saved(), None, "the default is never written");
        assert_eq!(Range::from_saved(None), Range::D1);
        for bad in ["", "2Y", "5y!", "week", "1D1", "\u{0}"] {
            assert_eq!(Range::from_saved(Some(bad)), Range::D1, "{bad:?}");
        }
        assert_eq!(Range::from_saved(Some(" 5y ")), Range::Y5, "case and spaces are forgiven, as parse does");
    }

    #[test]
    fn the_series_past_the_cap_evicts_the_least_recently_used() {
        let mut b = Book::new();
        let keys: Vec<Key> = (0..=MAX_SERIES).map(|i| key(&format!("S{i}"), "1Y.json")).collect();
        for (i, k) in keys.iter().take(MAX_SERIES).enumerate() {
            let r = b.want(k, i as f64).unwrap();
            assert!(b.arrived(r.generation, k, Ok(series(i as f64, 2)), i as f64));
        }
        assert_eq!(b.len(), MAX_SERIES);
        b.get(&keys[0]); // S0 is now the most recently used; S1 the least
        let r = b.want(&keys[MAX_SERIES], 100.0).unwrap();
        assert!(b.arrived(r.generation, &keys[MAX_SERIES], Ok(series(100.0, 2)), 100.0));
        assert_eq!(b.len(), MAX_SERIES);
        assert!(b.get(&keys[1]).is_none(), "S1 went");
        assert!(b.get(&keys[0]).is_some() && b.get(&keys[MAX_SERIES]).is_some());
    }

    #[test]
    fn the_cache_holds_forty_eight_series_and_a_fresh_book_still_asks_once_per_key() {
        assert_eq!(MAX_SERIES, 48, "L3: two ranges for each of 24 symbols");
        let mut b = Book::new();
        for i in 0..MAX_SERIES {
            let k = key(&format!("S{i}"), "5Y.json");
            let r = b.want(&k, i as f64).unwrap();
            assert!(b.arrived(r.generation, &k, Ok(series(i as f64, 3)), i as f64));
        }
        assert_eq!(b.len(), 48, "none evicted before the 49th");
    }

    #[test]
    fn a_series_off_screen_is_never_due() {
        // 48 series, all long past their time to live. Nothing the book does on its own asks for one:
        // reading, status checks and `forget` leave nothing in flight, and only `want(key)` (called for
        // the series on screen) puts that one key in flight.
        let mut b = Book::new();
        let keys: Vec<Key> = (0..MAX_SERIES).map(|i| key(&format!("S{i}"), "5D.json")).collect();
        for k in &keys {
            let r = b.want(k, 0.0).unwrap();
            assert!(b.arrived(r.generation, k, Ok(series(1.0, 2)), 1.0));
        }
        let shown: Vec<&str> = keys.iter().map(|k| k.symbol.as_str()).collect();
        for k in &keys {
            assert_eq!(b.status(k), Status::Ready);
            assert!(b.get(k).is_some());
        }
        b.forget(&shown);
        b.retry_now();
        assert!(b.flying.is_empty(), "no refresh is started by looking");
        let on_screen = &keys[7];
        assert!(b.want(on_screen, 1_000_000.0).is_some(), "the one on screen refreshes");
        assert_eq!(b.flying.len(), 1, "and no other");
    }

    #[test]
    fn forty_eight_full_series_are_under_one_point_three_megabytes() {
        assert_eq!(BAR_BYTES, 24, "six f32");
        let bytes = MAX_SERIES * FULL_SERIES_BARS * BAR_BYTES;
        assert_eq!(bytes, 1_267_200);
        assert!(bytes < 1_300_000);
        assert!(bytes < MAX_CACHE_BYTES, "the byte ceiling is above the planned load");
    }

    #[test]
    fn bigger_files_than_planned_for_cost_series_not_memory() {
        // 4,000-bar series (the parser's ceiling) are 96 KB each: the byte ceiling keeps about 20.
        let mut b = Book::new();
        let mut last = None;
        for i in 0..MAX_SERIES {
            let k = key(&format!("S{i}"), "5Y.json");
            let r = b.want(&k, i as f64).unwrap();
            assert!(b.arrived(r.generation, &k, Ok(series(i as f64, 4_000)), i as f64));
            assert!(b.bytes() <= MAX_CACHE_BYTES);
            last = Some(k);
        }
        assert!(b.len() < MAX_SERIES && b.len() >= 20, "{}", b.len());
        assert!(b.get(&last.unwrap()).is_some(), "the newest always stays");
        assert!(b.get(&key("S0", "5Y.json")).is_none(), "the oldest went first");
    }

    #[test]
    fn forget_keeps_only_the_symbols_still_shown() {
        let mut b = Book::new();
        let (a, c) = (key("A", "1Y.json"), key("B", "1Y.json"));
        let ra = b.want(&a, 0.0).unwrap();
        b.arrived(ra.generation, &a, Ok(series(1.0, 2)), 1.0);
        let rc = b.want(&c, 2.0).unwrap();
        b.forget(&["A"]);
        assert!(b.get(&a).is_some());
        assert!(!b.arrived(rc.generation, &c, Ok(series(3.0, 2)), 3.0), "B's fetch was in flight: dropped");
        assert_eq!(b.status(&c), Status::Loading);
    }

    #[test]
    fn a_request_lost_for_two_minutes_may_be_asked_for_again() {
        let mut b = Book::new();
        let k = key("A", "1Y.json");
        let r = b.want(&k, 0.0).unwrap();
        assert!(b.want(&k, 119.0).is_none());
        let r2 = b.want(&k, 121.0).expect("presumed lost");
        assert!(!b.arrived(r.generation, &k, Ok(series(122.0, 2)), 122.0), "the lost one is dropped");
        assert!(b.arrived(r2.generation, &k, Ok(series(123.0, 2)), 123.0));
    }

    #[test]
    fn an_empty_file_is_cached_as_empty() {
        let mut b = Book::new();
        let k = key("A", "1Y.json");
        let r = b.want(&k, 0.0).unwrap();
        b.arrived(r.generation, &k, Ok(series(1.0, 0)), 1.0);
        assert_eq!(b.status(&k), Status::Empty);
        assert!(b.want(&k, 2.0).is_none(), "not asked for in a loop");
    }

    #[test]
    fn the_ranges_name_their_files_and_share_one() {
        assert_eq!(Range::D1.file(), None);
        assert_eq!(Range::D1.key("X"), None);
        assert_eq!(Range::M1.key("X"), Range::M6.key("X"));
        assert_ne!(Range::Y1.key("X"), Range::Y5.key("X"));
        assert_eq!(Range::Y5.span_s(), 5.0 * 366.0 * 86_400.0);
        assert_eq!(Range::parse("5y"), Some(Range::Y5));
        assert_eq!(Range::parse("2D"), None);
        let labels: Vec<&str> = Range::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(labels, ["1D", "5D", "1M", "6M", "1Y", "5Y"]);
    }

    fn bars_at(dts: &[f32]) -> Vec<Bar> {
        dts.iter().map(|&dt| Bar { dt, ..Bar::default() }).collect()
    }

    fn req(generation: u64, sym: &str) -> Request {
        Request { generation, key: key(sym, "5D.json") }
    }

    #[test]
    fn submit_returns_the_request_it_replaced() {
        let mut s = Slots::default();
        assert!(s.submit(req(1, "A")).is_none(), "an empty slot replaces nothing");
        let replaced = s.submit(req(2, "B")).expect("the waiting request is replaced");
        assert_eq!(replaced.generation, 1);
        assert_eq!(replaced.key.symbol, "A");
        assert_eq!(s.take_request().map(|r| r.generation), Some(2));
        assert!(s.take_request().is_none(), "taken once");
    }

    #[test]
    fn take_done_drains_in_delivery_order() {
        let mut s = Slots::default();
        assert!(s.take_done().is_empty());
        for g in [7, 8, 9] {
            s.deliver(Done { generation: g, key: key("A", "5D.json"), result: Err("x".into()) });
        }
        let got: Vec<u64> = s.take_done().iter().map(|d| d.generation).collect();
        assert_eq!(got, [7, 8, 9], "oldest first");
        assert!(s.take_done().is_empty(), "drained");
    }

    #[test]
    fn clear_request_takes_back_only_the_waiting_request() {
        let mut s = Slots::default();
        assert!(s.clear_request().is_none(), "nothing to take back");
        s.submit(req(3, "A"));
        s.deliver(Done { generation: 2, key: key("B", "1Y.json"), result: Err("x".into()) });
        let taken = s.clear_request().expect("the waiting request");
        assert_eq!(taken.generation, 3);
        assert!(s.take_request().is_none(), "the slot is empty now");
        assert_eq!(s.take_done().len(), 1, "results are not touched");
    }

    #[test]
    fn bar_s_is_the_median_spacing() {
        // Minute bars.
        let minutes: Vec<f32> = (0..30).map(|i| i as f32 * 60.0).collect();
        assert_eq!(median_spacing(&bars_at(&minutes)), 60.0);
        // Daily bars over three weeks of weekdays: one 3-day gap per weekend.
        let mut days = Vec::new();
        let mut d = 0.0f32;
        for i in 0..21 {
            days.push(d);
            d += if i % 5 == 4 { 3.0 } else { 1.0 } * 86_400.0;
        }
        assert_eq!(median_spacing(&bars_at(&days)), 86_400.0);
        // Weekly bars.
        let weeks: Vec<f32> = (0..10).map(|i| i as f32 * 604_800.0).collect();
        assert_eq!(median_spacing(&bars_at(&weeks)), 604_800.0);
        // Fewer than two bars.
        assert_eq!(median_spacing(&[]), 0.0);
        assert_eq!(median_spacing(&bars_at(&[5.0])), 0.0);
        // Two bars: their gap.
        assert_eq!(median_spacing(&bars_at(&[0.0, 300.0])), 300.0);
        // A duplicate dt is ignored, not averaged in as zero.
        assert_eq!(median_spacing(&bars_at(&[0.0, 60.0, 60.0, 120.0])), 60.0);
        // Even count of gaps: mean of the two middle ones (gaps 10, 20, 30, 40 -> 25).
        assert_eq!(median_spacing(&bars_at(&[0.0, 10.0, 30.0, 60.0, 100.0])), 25.0);
    }

    #[test]
    fn window_takes_the_tail_inside_the_span() {
        let t0 = 1_000_000.0;
        // Bars at t0, t0+60, ..., t0+600 (11 bars).
        let bars = bars_at(&(0..11).map(|i| i as f32 * 60.0).collect::<Vec<_>>());
        let s = Series::new(t0, bars, t0);
        let now = t0 + 600.0;
        // Span 120 s: cutoff is now - 120 = t0 + 480, a bar exactly that old is inside.
        let w = s.window(now, 120.0);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].dt, 480.0);
        // One second less drops the boundary bar (119 s span: cutoff t0+481); one more keeps it.
        assert_eq!(s.window(now, 119.0).len(), 2);
        assert_eq!(s.window(now, 121.0).len(), 3);
        // Span 0 keeps only bars at or after now.
        assert_eq!(s.window(now, 0.0).len(), 1);
        // A huge span gives every bar; infinity too.
        assert_eq!(s.window(now, 1.0e12).len(), 11);
        assert_eq!(s.window(now, f64::INFINITY).len(), 11);
        // Negative or NaN span: empty.
        assert!(s.window(now, -1.0).is_empty());
        assert!(s.window(now, f64::NAN).is_empty());
        // Empty series: empty, no panic.
        let empty = Series::new(t0, Vec::new(), t0);
        assert!(empty.window(now, 3600.0).is_empty());
        assert!(empty.window(now, f64::NAN).is_empty());
    }

    #[test]
    fn window_with_one_bar_and_the_boundary_itself() {
        let t0 = 50.0;
        let s = Series::new(t0, bars_at(&[0.0]), t0);
        // The only bar is exactly 10 s old: inside for a 10 s span, outside for 9 s.
        assert_eq!(s.window(60.0, 10.0).len(), 1);
        assert!(s.window(60.0, 9.0).is_empty());
    }

    /// A daily file of `n` bars stamped at `tod` seconds after UTC midnight, from 2026-01-05.
    fn daily_at(tod: f64, n: usize) -> Series {
        let t0 = crate::chartview::days_from_civil(2026, 1, 5) as f64 * DAY_S + tod;
        let bars = (0..n).map(|i| Bar { dt: (i as f64 * DAY_S) as f32, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }).collect();
        Series::new(t0, bars, 1_000_000.0)
    }

    #[test]
    fn daily_stamps_on_a_boundary_pass_and_off_it_are_flagged() {
        // New York midnight in winter is 05:00 UTC; Tokyo midnight is 15:00 UTC the day before.
        for tod in [0.0, 5.0 * 3_600.0, 15.0 * 3_600.0, 19_800.0] {
            assert!(!daily_at(tod, 30).check().dates_off, "tod {tod}");
        }
        // 13:07 is no zone's midnight.
        assert!(daily_at(13.0 * 3_600.0 + 7.0 * 60.0, 30).check().dates_off);
        // Stamps that agree with each other only within an hour of a clock change pass; a two-hour
        // spread does not.
        assert!(stamps_on_day_boundary(&[5.0 * 3_600.0, 4.0 * 3_600.0 + DAY_S]));
        assert!(!stamps_on_day_boundary(&[5.0 * 3_600.0, 7.0 * 3_600.0 + DAY_S]));
        assert!(!stamps_on_day_boundary(&[f64::NAN]));
        // An intraday series is never flagged, however its stamps fall.
        let intraday = Series::new(1_000.0, (0..10).map(|i| Bar { dt: i as f32 * 300.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }).collect(), 0.0);
        assert_eq!(intraday.check(), Check::default());
        assert_eq!(Series::new(0.0, vec![], 0.0).check(), Check::default());
    }

    #[test]
    fn the_age_label_appears_only_past_the_time_to_live() {
        let fetched = 14.0 * 3_600.0 + 5.0 * 60.0;
        let ttl = 600.0;
        assert_eq!(age_label(fetched + 100.0, fetched, ttl, 0), None, "fresh");
        assert_eq!(age_label(fetched + ttl + STALE_GRACE_S, fetched, ttl, 0), None, "a refresh on its way");
        assert_eq!(age_label(fetched + ttl + STALE_GRACE_S + 1.0, fetched, ttl, 0).as_deref(), Some("as of 14:05"));
        assert_eq!(age_label(fetched + 3_600.0, fetched, ttl, 3_600).as_deref(), Some("as of 15:05"), "on the viewer's clock");
        // A day or more back it carries the date.
        let old = age_label(fetched + 2.0 * DAY_S, fetched, ttl, 0).unwrap();
        assert!(old.starts_with("as of Jan ") && old.ends_with(" 14:05"), "{old}");
        assert_eq!(age_label(f64::NAN, fetched, ttl, 0), None);
    }

    #[test]
    fn the_note_joins_what_is_suspect() {
        let s = daily_at(13.0 * 3_600.0 + 420.0, 30);
        assert_eq!(s.note(s.fetched + 10.0, 3_600.0, 0), "check dates");
        let both = s.note(s.fetched + 9_000.0, 3_600.0, 0);
        assert!(both.starts_with("as of ") && both.ends_with(" \u{b7} check dates"), "{both}");
        assert_eq!(daily_at(0.0, 30).note(1_000_001.0, 3_600.0, 0), "");
    }
}
