//! Price alerts: a threshold crossed, a tray balloon, and an optional outbound POST.
//!
//! Firing rule: an alert fires when the quote is fresh, the price is on the alert's side of its
//! level, and `cooldown_s` has passed since it last fired. Two consequences are deliberate:
//! - A **stale** quote never fires. Otherwise a market closed at a level would retrigger on every
//!   cooldown for as long as the app runs.
//! - Starting the app already past a level fires once. That is what a user who set a level expects.
//!
//! The webhook is a *bearer secret*: anyone holding the URL can post to that channel. It is never
//! written to a note, tooltip, `--probe` line or error message — `redact` sees to the last one.
//!
//! Outbound posts run on a detached thread, one per alert, so a slow or unreachable endpoint can
//! never stall the message loop. The cooldown is the rate limit: an alert cannot post more than
//! once per `cooldown_s` however many times the price wobbles across the level.

use crate::config::{Alert, Config};
use crate::model::{fmt_price, Board};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_INFO, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW};

/// One alert that fired on this pass. The label is also inside `title`; it is kept separately so
/// the log and the menu never parse the title.
pub struct Fired {
    #[allow(dead_code)] // read by the later per-symbol work (P6); kept so nothing parses `title`
    pub label: String,
    pub title: String,
    pub body: String,
    /// UTC seconds, the `now` that `check` was given.
    pub at: f64,
}

/// How long to say nothing rather than repeat the same alert.
const POST_TIMEOUT_S: u64 = 10;

/// Which kind of level an alert tripped on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Above,
    Below,
    DayMove,
    /// A fast move (`FAST_MOVES`): this many percent inside the window.
    Fast,
}

/// The title used for the "Send a test alert" log entry and note.
pub const TEST_TITLE: &str = "Test alert";

/// The fast-move choices: percent and the window in seconds. `Alert::fast` is an index plus one.
pub const FAST_MOVES: [(f64, f64); 3] = [(1.0, 300.0), (2.0, 900.0), (3.0, 3_600.0)];

/// The percent and window of an alert's fast move, or `None` (off, or a stored number out of range).
pub fn fast_of(a: &Alert) -> Option<(f64, f64)> {
    FAST_MOVES.get((a.fast as usize).checked_sub(1)?).copied()
}

/// `1 % in 5 min`, for the menu and the notices.
pub fn fast_label(k: usize) -> String {
    let (pct, secs) = FAST_MOVES[k];
    format!("{pct} % in {} min", secs / 60.0)
}

/// Does this alert have anything set that can fire? The poll governor's "armed" and the menu's count.
pub fn has_level(a: &Alert) -> bool {
    a.above.is_some_and(f64::is_finite)
        || a.below.is_some_and(f64::is_finite)
        || a.day_move.is_some_and(|d| d.is_finite() && d > 0.0)
        || fast_of(a).is_some()
}

/// Is any alert armed?
pub fn armed(alerts: &[Alert]) -> bool {
    alerts.iter().any(has_level)
}

/// What the gap tracker (`watch`) needs from one feed tick: `(armed, fresh)`. Armed means some alert
/// with a level is set on a shown symbol (a level on a hidden symbol is not checked and is the
/// overview's warning, not a gap). Fresh means at least one of those symbols has a fresh CNBC quote,
/// i.e. an alert could be checked this tick.
pub fn checkable(cfg: &Config, board: &Board) -> (bool, bool) {
    let mut armed = false;
    for a in cfg.alerts.iter().filter(|a| has_level(a) && cfg.rows.iter().any(|r| r.label == a.label)) {
        armed = true;
        let fresh = board
            .cells
            .iter()
            .any(|c| c.label == a.label && !c.stale && c.quote.as_ref().is_some_and(|q| q.source.is_empty() && q.price.is_finite()));
        if fresh {
            return (true, true);
        }
    }
    (armed, false)
}

/// The share of a level the price must come back past before an *each crossing* alert is armed again.
pub const CROSSING_BAND: f64 = 0.001;
/// The same for a day move, in percentage points.
pub const DAY_BAND_PTS: f64 = 0.1;
/// A fast move re-arms once it has fallen to this share of its threshold.
pub const FAST_REARM: f64 = 0.5;

/// A fast move needs samples no further apart than this many polls.
pub const FAST_MAX_GAP_POLLS: f64 = 3.0;

/// How far the price has moved over the last `window_s`, in percent, if the window is covered: the
/// samples (oldest first, `(utc seconds, price)`) must reach back to the window's start, and no two
/// neighbours, from that first one to `now`, may be more than `max_gap_s` apart. `price` is the quote
/// being judged and counts as the sample at `now`. `None` for an uncovered window, a gap, a price that
/// is not finite, or a start price that is not positive.
pub fn fast_move(samples: &[(f64, f64)], now: f64, price: f64, window_s: f64, max_gap_s: f64) -> Option<f64> {
    if !(price.is_finite() && window_s > 0.0 && max_gap_s > 0.0 && now.is_finite()) {
        return None;
    }
    let start = now - window_s;
    let good = |s: &&(f64, f64)| s.0.is_finite() && s.1.is_finite();
    let held: Vec<&(f64, f64)> = samples.iter().filter(good).collect();
    let i = held.iter().rposition(|s| s.0 <= start)?;
    let mut prev = held[i].0;
    for s in &held[i + 1..] {
        if s.0 - prev > max_gap_s {
            return None;
        }
        prev = s.0;
    }
    if now - prev > max_gap_s {
        return None;
    }
    let from = held[i].1;
    (from > 0.0).then(|| (price - from) / from * 100.0)
}

/// Only a fresh CNBC price may establish a CNBC alert threshold. Fresh means what the fire path means:
/// the governed stale limit (`governor::stale_after` with the board's `governed_s`).
pub fn reference_price(cell: &crate::model::Cell, cfg: &Config, governed_s: f64, now: f64) -> Option<f64> {
    let q = cell.quote.as_ref()?;
    let stale_after = crate::governor::stale_after(cfg.stale_after_s, governed_s);
    if cell.stale || !q.source.is_empty() || crate::model::is_stale(q, now, stale_after, cfg.delayed_grace_s) {
        return None;
    }
    Some(q.price)
}

/// Has the cooldown passed? `last` is the wall-clock second of the last fire. A clock set back (or a
/// `last` written by a machine whose clock ran ahead) puts `last` after `now`: that reads as "cooled",
/// not as a silence that lasts until the clock catches up, and the next fire writes a fresh `last`,
/// so a jump costs at most one early fire and never a muted alert. A cooldown of 0 or NaN is 30 s.
pub fn cooled(now: f64, last: f64, cooldown_s: f64) -> bool {
    let cd = if cooldown_s.is_finite() { cooldown_s.max(30.0) } else { 30.0 };
    now < last || now - last >= cd
}

/// One level of an alert as the current quote sees it.
struct Cond {
    kind: Kind,
    /// Bit in `Alert::latched`.
    bit: u8,
    level: f64,
    /// The quote is on the firing side of the level.
    beyond: bool,
    /// The quote is back past the level by the band: an *each crossing* alert is armed again.
    clear: bool,
}

/// The levels `a` has set, in firing order (a price level wins over a day move, which wins over a fast
/// move), each judged against this quote. A level that is not a finite number is not listed, so NaN
/// never fires and never re-arms.
fn conds(a: &Alert, price: f64, change_pct: Option<f64>, fast: Option<f64>) -> Vec<Cond> {
    let mut out = Vec::new();
    if !price.is_finite() {
        return out;
    }
    if let Some(l) = a.above.filter(|l| l.is_finite()) {
        out.push(Cond { kind: Kind::Above, bit: 1, level: l, beyond: price >= l, clear: price <= l - l.abs() * CROSSING_BAND });
    }
    if let Some(l) = a.below.filter(|l| l.is_finite()) {
        out.push(Cond { kind: Kind::Below, bit: 2, level: l, beyond: price <= l, clear: price >= l + l.abs() * CROSSING_BAND });
    }
    if let (Some(d), Some(c)) = (a.day_move.filter(|d| d.is_finite() && *d > 0.0), change_pct.filter(|c| c.is_finite())) {
        out.push(Cond { kind: Kind::DayMove, bit: 4, level: d, beyond: c.abs() >= d, clear: c.abs() <= d - DAY_BAND_PTS });
    }
    if let (Some((pct, _)), Some(m)) = (fast_of(a), fast.filter(|m| m.is_finite())) {
        out.push(Cond { kind: Kind::Fast, bit: 8, level: pct, beyond: m.abs() >= pct - 1e-9, clear: m.abs() <= pct * FAST_REARM });
    }
    out
}

/// Check every alert and mark the ones that fire, with no price history (so a fast move never fires).
/// See [`check_with`]. The tests' short form.
#[cfg(test)]
pub fn check(cfg: &mut Config, board: &Board, now: f64) -> Vec<Fired> {
    check_with(cfg, board, now, &|_| Vec::new())
}

/// Check every alert and mark the ones that fire. The caller saves the config.
///
/// Contract: fresh CNBC quotes only, in every repeat mode (a stale quote or a fallback quote is
/// skipped before anything is judged or re-armed). One `Fired` per alert per pass.
/// - Every cooldown (the default, 1.8's rule): fires when a level or a day move is reached and
///   `cooldown_s` (at least 30 s) has passed.
/// - Once: the same, and the level that fired is cleared; an alert left with nothing set is removed.
/// - Each crossing (`Alert::crossing`): fires when a level is reached and it is not already latched;
///   latches it; re-arms only when the price has gone back past the level by the band. No cooldown:
///   the band is the rate limit. A restart has nothing latched, so the first fresh quote fires once if
///   it is already beyond.
///
/// `series(label)` gives the held samples of a symbol, oldest first, for the fast-move alert, which
/// needs the window covered with no gap longer than three polls (`FAST_MAX_GAP_POLLS`). Silencing is
/// NOT decided here: a muted alert still fires, burns its cooldown and loses its `once` level, and the
/// caller asks [`silence`].
pub fn check_with(cfg: &mut Config, board: &Board, now: f64, series: &dyn Fn(&str) -> Vec<(f64, f64)>) -> Vec<Fired> {
    let mut out = Vec::new();
    let mut spent = Vec::new();
    // The poll interval in force: the governor's, else the user's.
    let poll = if board.governed_s > 0.0 { board.governed_s } else { board.poll_s };
    let stale_after = crate::governor::stale_after(cfg.stale_after_s, board.governed_s);
    for cell in &board.cells {
        let Some(q) = cell.quote.as_ref() else { continue };
        if cell.stale || !q.source.is_empty() || crate::model::is_stale(q, now, stale_after, cfg.delayed_grace_s) {
            continue;
        }
        for (i, a) in cfg.alerts.iter_mut().enumerate().filter(|(_, a)| a.label == cell.label) {
            let fast = fast_of(a).and_then(|(_, window)| {
                fast_move(&series(&cell.label), now, q.price, window, FAST_MAX_GAP_POLLS * poll.max(1.0))
            });
            let list = conds(a, q.price, q.change_pct, fast);
            let crossing = a.crossing && !a.once;
            if crossing {
                for c in &list {
                    if c.clear {
                        a.latched &= !c.bit;
                    }
                }
            }
            let Some(c) = list.iter().find(|c| c.beyond && !(crossing && a.latched & c.bit != 0)) else { continue };
            if !crossing && !cooled(now, a.last, a.cooldown_s) {
                continue;
            }
            a.last = now;
            if crossing {
                a.latched |= c.bit;
            }
            let (kind, level) = (c.kind, c.level);
            let price = fmt_price(q.price, cell.digits);
            let (title, body) = match kind {
                Kind::Above | Kind::Below => {
                    let dir = if kind == Kind::Above { "above" } else { "below" };
                    let lv = fmt_price(level, cell.digits);
                    (format!("{} {dir} {lv}", cell.label), format!("{} is {price} {dir} {lv}", cell.label))
                }
                Kind::DayMove => {
                    let c = q.change_pct.unwrap_or(0.0);
                    (
                        format!("{} day {c:+.1} %", cell.label),
                        format!("{} is {price}, {c:+.2} % on the day (alert at \u{b1}{level} %)", cell.label),
                    )
                }
                Kind::Fast => {
                    let m = fast.unwrap_or(0.0);
                    let k = (a.fast as usize).saturating_sub(1).min(FAST_MOVES.len() - 1);
                    (
                        format!("{} {m:+.1} % in {} min", cell.label, FAST_MOVES[k].1 / 60.0),
                        format!("{} is {price}, {m:+.2} % in {} min (alert at {})", cell.label, FAST_MOVES[k].1 / 60.0, fast_label(k)),
                    )
                }
            };
            out.push(Fired { label: cell.label.clone(), title, body, at: now });
            if a.once {
                match kind {
                    Kind::Above => a.above = None,
                    Kind::Below => a.below = None,
                    Kind::DayMove => a.day_move = None,
                    Kind::Fast => a.fast = 0,
                }
                if !has_level(a) {
                    spent.push(i);
                }
            }
        }
    }
    if !spent.is_empty() {
        let mut i = 0;
        cfg.alerts.retain(|_| {
            i += 1;
            !spent.contains(&(i - 1))
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------------------------

/// What a menu preset asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    /// Signed percent away from the reference price (positive = above).
    Pct(f64),
    DayHigh,
    DayLow,
    /// The previous close, on whichever side the reference price is not.
    PrevClose,
    /// Absolute percent move on the day, either sign.
    DayMove(f64),
}

/// One menu entry. `group` changes where the menu draws a separator.
pub struct PresetDef {
    pub label: &'static str,
    pub group: u8,
    pub preset: Preset,
}

const fn pd(label: &'static str, group: u8, preset: Preset) -> PresetDef {
    PresetDef { label, group, preset }
}

/// The Alerts submenu entries, in menu order. `Action::Alert(label, k)` indexes this table.
pub const PRESETS: [PresetDef; 15] = [
    pd("above 0.5 %", 0, Preset::Pct(0.5)),
    pd("above 1 %", 0, Preset::Pct(1.0)),
    pd("above 2 %", 0, Preset::Pct(2.0)),
    pd("above 5 %", 0, Preset::Pct(5.0)),
    pd("below 0.5 %", 0, Preset::Pct(-0.5)),
    pd("below 1 %", 0, Preset::Pct(-1.0)),
    pd("below 2 %", 0, Preset::Pct(-2.0)),
    pd("below 5 %", 0, Preset::Pct(-5.0)),
    pd("past today's high", 1, Preset::DayHigh),
    pd("past today's low", 1, Preset::DayLow),
    pd("back to the previous close", 1, Preset::PrevClose),
    pd("day move \u{b1}1 %", 2, Preset::DayMove(1.0)),
    pd("day move \u{b1}2 %", 2, Preset::DayMove(2.0)),
    pd("day move \u{b1}3 %", 2, Preset::DayMove(3.0)),
    pd("day move \u{b1}5 %", 2, Preset::DayMove(5.0)),
];

/// What a preset sets on an alert.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Above(f64),
    Below(f64),
    /// Absolute percent.
    DayMove(f64),
}

/// The level a preset means for this cell, or `None` when it cannot be known: the field is
/// missing, zero, negative or not finite (CNBC sends `0.00` for symbols with no high or low), or
/// `reference` is not a positive finite price. `reference` comes from [`reference_price`].
/// "Past today's high" is `Above(high + tick)` and "past today's low" is `Below(low - tick)`, where
/// `tick` is one unit of the last displayed digit (`10^-digits`, so 1.0 for digits 0). The price has
/// to go past the day's extreme, not just reach it: a price exactly at the high does not trip the
/// level, one tick above does. `None` when `low - tick` is not positive. The previous close is `Above`
/// when `reference` is below it, `Below` when above it, and `None` when equal.
pub fn preset_level(preset: Preset, cell: &crate::model::Cell, reference: f64) -> Option<Level> {
    let good = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
    let q = cell.quote.as_ref();
    let reference = good(Some(reference));
    let tick = 10f64.powi(-(cell.digits as i32));
    match preset {
        Preset::Pct(p) => {
            let r = reference?;
            if !p.is_finite() || p == 0.0 {
                return None;
            }
            let level = r * (1.0 + p / 100.0);
            Some(if p > 0.0 { Level::Above(level) } else { Level::Below(level) })
        }
        Preset::DayHigh => good(q?.high).map(|h| Level::Above(h + tick)),
        Preset::DayLow => good(q?.low).and_then(|l| Some(l - tick).filter(|v| *v > 0.0)).map(Level::Below),
        Preset::PrevClose => {
            let (pc, r) = (good(q?.prev_close)?, reference?);
            if r < pc {
                Some(Level::Above(pc))
            } else if r > pc {
                Some(Level::Below(pc))
            } else {
                None
            }
        }
        Preset::DayMove(n) => (n.is_finite() && n > 0.0).then_some(Level::DayMove(n)),
    }
}

/// Set `level` on `a` and re-arm it (`last = 0`). Nothing else changes.
pub fn apply_level(a: &mut Alert, level: Level) {
    match level {
        Level::Above(v) => a.above = Some(v),
        Level::Below(v) => a.below = Some(v),
        Level::DayMove(v) => a.day_move = Some(v),
    }
    a.last = 0.0;
    a.latched = 0;
}

/// A price level typed by hand (*at a price...*). After trimming, accepts digits with an optional
/// decimal point and fraction (`4200`, `4200.5`, `.5`), and commas as thousands separators only
/// when they group: the first group is one to three digits and every later group is exactly three
/// (`4,200.50`, `1,234,567`). Refuses everything else: empty, a sign (`-5`, `+5`), an exponent
/// (`1e3`), `inf`, `NaN`, a second point, a comma after the point, a leading, trailing or doubled
/// comma, a comma that does not group (`4200,5` and `4,20` could be a decimal comma, so they are
/// never read as 42,005 or 420), spaces inside, and any result that is zero or not finite.
#[cfg(test)]
pub fn parse_level(text: &str) -> Option<f64> {
    parse_level_why(text).ok()
}

const NOT_A_PRICE: &str = "that is not a price";
const COMMA_RULE: &str = "a comma must be followed by exactly three digits, so type a point for decimals";

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// [`parse_level`] with the reason for a refusal, as a phrase for the message box.
pub fn parse_level_why(text: &str) -> Result<f64, &'static str> {
    let t = text.trim();
    let mut halves = t.split('.');
    let whole = halves.next().unwrap_or("");
    let frac = halves.next().unwrap_or("");
    if halves.next().is_some() || !(frac.is_empty() || all_digits(frac)) {
        return Err(if t.contains(',') && t.contains('.') { COMMA_RULE } else { NOT_A_PRICE });
    }
    let digits: String = if whole.contains(',') {
        let mut groups = whole.split(',');
        let first = groups.next().unwrap_or("");
        if !all_digits(first) {
            return Err(NOT_A_PRICE);
        }
        if first.len() > 3 {
            return Err(COMMA_RULE);
        }
        let mut out = first.to_string();
        for g in groups {
            if !all_digits(g) {
                return Err(NOT_A_PRICE);
            }
            if g.len() != 3 {
                return Err(COMMA_RULE);
            }
            out.push_str(g);
        }
        out
    } else if whole.is_empty() || all_digits(whole) {
        whole.to_string()
    } else {
        return Err(NOT_A_PRICE);
    };
    if digits.is_empty() && frac.is_empty() {
        return Err(NOT_A_PRICE);
    }
    let number = format!("{}.{}", if digits.is_empty() { "0" } else { &digits }, frac);
    let v: f64 = number.parse().map_err(|_| NOT_A_PRICE)?;
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(NOT_A_PRICE) }
}

/// How far from the reference price a typed level may be before the prompt asks first.
pub const FAR_LEVEL: f64 = 0.20;

/// The question to ask before setting a typed level that is more than 20 % from the fresh price
/// (exactly 20 % is not asked), or `None`. It echoes the parsed level and the price in the symbol's
/// own digits, so a misread shows before it is saved.
pub fn far_level_question(label: &str, level: f64, reference: f64, digits: u32) -> Option<String> {
    if !(level.is_finite() && reference.is_finite() && reference > 0.0) {
        return None;
    }
    let away = (level - reference).abs() / reference;
    (away > FAR_LEVEL).then(|| {
        format!(
            "You typed a level of {}, which is {:.0} % {} the price now ({}). Set the alert for {label} at {}?",
            fmt_price(level, digits),
            away * 100.0,
            if level > reference { "above" } else { "below" },
            fmt_price(reference, digits),
            fmt_price(level, digits),
        )
    })
}

/// The side rule for a typed or pointed level: `Above(level)` when it is above the fresh `reference`
/// price, `Below(level)` when below, compared after rounding both to `digits` decimals (so accepting
/// the prompt's initial text, the current price, is refused). `Err` with the reason, as a phrase that
/// fits "No alert set: <reason>.": `"that is the current price"` when equal, `"that is not a price"`
/// when `level` is not positive and finite, `"there is no fresh price"` when `reference` is not.
pub fn side_of(level: f64, reference: f64, digits: u32) -> Result<Level, &'static str> {
    if !(level.is_finite() && level > 0.0) {
        return Err("that is not a price");
    }
    if !(reference.is_finite() && reference > 0.0) {
        return Err("there is no fresh price");
    }
    let (l, r) = (round_level(level, digits), round_level(reference, digits));
    if l == r {
        Err("that is the current price")
    } else if l > r {
        Ok(Level::Above(level))
    } else {
        Ok(Level::Below(level))
    }
}

/// `price` rounded to `digits` decimals (half away from zero), as the pane's *Alert at* offers it.
pub fn round_level(price: f64, digits: u32) -> f64 {
    let k = 10f64.powi(digits.min(12) as i32);
    (price * k).round() / k
}

/// One line for the menu, greyed: what is set on this alert. Levels are formatted with
/// `fmt_price(level, digits)`, joined by " \u{b7} ", in the order above, below, day move, once:
/// `above 4,200.00 \u{b7} below 3,900.00 \u{b7} day \u{b1}2 % \u{b7} once`. An alert with nothing set
/// gives `nothing set`.
pub fn describe(alert: &Alert, digits: u32) -> String {
    // Only levels that can actually trip are listed: a NaN or infinite level, or a day move that
    // is not a positive finite number, never fires in `tripped`, so it is not "set" either.
    let mut parts = Vec::new();
    if let Some(v) = alert.above.filter(|v| v.is_finite()) {
        parts.push(format!("above {}", fmt_price(v, digits)));
    }
    if let Some(v) = alert.below.filter(|v| v.is_finite()) {
        parts.push(format!("below {}", fmt_price(v, digits)));
    }
    if let Some(d) = alert.day_move.filter(|d| d.is_finite() && *d > 0.0) {
        parts.push(format!("day \u{b1}{d} %"));
    }
    if let Some(k) = (alert.fast as usize).checked_sub(1).filter(|k| *k < FAST_MOVES.len()) {
        parts.push(format!("fast {}", fast_label(k)));
    }
    if parts.is_empty() {
        return "nothing set".into();
    }
    if alert.once {
        parts.push("once".into());
    } else if alert.crossing {
        parts.push("each crossing".into());
    }
    parts.join(" \u{b7} ")
}

// ---------------------------------------------------------------------------------------------
// Silence
// ---------------------------------------------------------------------------------------------

/// Why an alert is not shown or posted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Silence {
    /// Muted until the user turns it back on.
    Muted,
    /// Muted until this UTC time.
    MutedUntil(f64),
    Quiet,
}

/// The quiet-hours presets in the menu, `[from, to)` in minutes of the local day.
pub const QUIET_CHOICES: [[u16; 2]; 3] = [[22 * 60, 7 * 60], [23 * 60, 8 * 60], [0, 9 * 60]];

/// Is `local_minute` (0..1440) inside `window = [from, to)`? `from == to` is off (never inside);
/// `from > to` wraps midnight; `to` itself is outside.
pub fn quiet(local_minute: u16, window: [u16; 2]) -> bool {
    let [from, to] = window;
    if from == to {
        false
    } else if from < to {
        from <= local_minute && local_minute < to
    } else {
        local_minute >= from || local_minute < to
    }
}

/// Whether an alert firing now should stay quiet. Order: `alerts_muted` is `Muted`; then a
/// `alerts_mute_until` still ahead of `now_utc` is `MutedUntil(t)` (a timed mute ends by the
/// clock, nothing resets it); then `quiet_hours` containing `local_minute` is `Quiet`.
pub fn silence(cfg: &Config, now_utc: f64, local_minute: u16) -> Option<Silence> {
    if let Some(m) = mute_state(cfg, now_utc) {
        return Some(m);
    }
    cfg.quiet_hours.filter(|w| quiet(local_minute, *w)).map(|_| Silence::Quiet)
}

/// The mute part of [`silence`] only: `Muted`, `MutedUntil(t)` or `None`. Never `Quiet`, so the
/// menu can show the mute state without needing the local clock.
pub fn mute_state(cfg: &Config, now_utc: f64) -> Option<Silence> {
    if cfg.alerts_muted {
        return Some(Silence::Muted);
    }
    if cfg.alerts_mute_until.is_finite() && cfg.alerts_mute_until > now_utc {
        return Some(Silence::MutedUntil(cfg.alerts_mute_until));
    }
    None
}

/// `HH:MM` for a minute of the day (24-hour). Out-of-range values wrap.
pub fn hhmm(minute: u16) -> String {
    let m = minute % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// The local clock time of the UTC instant `at`, read on the offset in force AT that instant
/// (`offset_at(t)` is seconds east of UTC at `t`), so a time before or after a daylight-saving change
/// is shown on its own clock instead of today's. A time that is not a number is read as `now_utc`.
pub fn local_hhmm(offset_at: &dyn Fn(f64) -> i32, now_utc: f64, at: f64) -> String {
    let t = if at.is_finite() { at } else { now_utc };
    let t = if t.is_finite() { t.clamp(-1e12, 1e12) } else { 0.0 };
    let local = ((t + offset_at(t) as f64) / 60.0).floor() as i64;
    hhmm(local.rem_euclid(1440) as u16)
}

// ---------------------------------------------------------------------------------------------
// Recent alerts
// ---------------------------------------------------------------------------------------------

/// What became of an alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Balloon shown (and a POST started, if a webhook is set).
    Shown,
    Muted,
    Quiet,
    Sent,
    /// The first POST failed on the network or with a 5xx and the second went through.
    SentOnRetry,
    SendFailed,
    /// Not an alert: a span in which alerts could not be checked (`watch`). The entry's title is the
    /// whole line, times included, and its outcome word is the title's own.
    Gap,
}

/// One line of the log. `seq` is unique for the life of the process and is what a webhook result
/// carries back, so a late answer updates the right line (or nothing, if it was cleared).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub seq: u64,
    /// UTC seconds.
    pub at: f64,
    pub title: String,
    pub outcome: Outcome,
}

/// The last [`LOG_CAP`] (50) alerts and gaps, in memory only.
#[derive(Default)]
pub struct Log {
    entries: std::collections::VecDeque<Entry>,
    next: u64,
}

pub const LOG_CAP: usize = 50;

impl Log {
    /// Add an entry (oldest dropped beyond `LOG_CAP`) and return its sequence number (from 1).
    pub fn push(&mut self, at: f64, title: String, outcome: Outcome) -> u64 {
        self.next += 1;
        self.entries.push_back(Entry { seq: self.next, at, title, outcome });
        while self.entries.len() > LOG_CAP {
            self.entries.pop_front();
        }
        self.next
    }

    /// Change the outcome of entry `seq`; false when it is gone (cleared or pushed out).
    pub fn set_outcome(&mut self, seq: u64, outcome: Outcome) -> bool {
        match self.entries.iter_mut().find(|e| e.seq == seq) {
            Some(e) => {
                e.outcome = outcome;
                true
            }
            None => false,
        }
    }

    /// Newest first.
    pub fn newest_first(&self) -> Vec<Entry> {
        self.entries.iter().rev().cloned().collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Empty the list. Sequence numbers keep counting, so a late webhook result cannot land on a
    /// new entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// The menu text for one log entry: `14:30  XAU above 4,200.00 \u{b7} muted`. Time via
/// [`local_hhmm`]; the outcome words are `shown`, `muted`, `quiet hours`, `sent`, `send failed`.
pub fn entry_line(e: &Entry, offset_at: &dyn Fn(f64) -> i32, now_utc: f64) -> String {
    let word = match e.outcome {
        Outcome::Shown => "shown",
        Outcome::Muted => "muted",
        Outcome::Quiet => "quiet hours",
        Outcome::Sent => "sent",
        Outcome::SentOnRetry => "sent on retry",
        Outcome::SendFailed => "send failed",
        Outcome::Gap => return e.title.clone(),
    };
    format!("{}  {} \u{b7} {word}", local_hhmm(offset_at, now_utc, e.at), e.title)
}

/// What a mute or quiet hours swallowed since `since` (UTC seconds): the balloon text for the end of
/// it, or `None` when nothing was (a gap line and a delivered alert are not swallowed). One request
/// per ending, never one per alert.
pub fn digest(log: &[Entry], since: f64) -> Option<String> {
    let count = |o: Outcome| log.iter().filter(|e| e.outcome == o && e.at >= since).count();
    crate::watch::digest_text(count(Outcome::Muted), count(Outcome::Quiet))
}

/// The whole Recent list as text, newest first, one entry per line, for the clipboard.
pub fn log_text(log: &[Entry], offset_at: &dyn Fn(f64) -> i32, now_utc: f64) -> String {
    log.iter().map(|e| entry_line(e, offset_at, now_utc)).collect::<Vec<_>>().join("\r\n")
}

/// One line of *Alerts at a glance*.
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewLine {
    pub text: String,
    /// The level cannot fire as things stand (its symbol is not on the strip).
    pub warn: bool,
}

/// Everything that is set, one line per level, with how far the price is from it:
/// `XAU above 4,200.00, 1.2 % away`. A level whose symbol is not on the strip is marked
/// `(not on the strip, cannot fire)` and flagged, because an alert is only checked for a shown
/// symbol. Without a fresh price a line says `no fresh price` rather than guess a distance.
pub fn overview(cfg: &Config, board: &Board, now: f64) -> Vec<OverviewLine> {
    let stale_after = crate::governor::stale_after(cfg.stale_after_s, board.governed_s);
    let mut out = Vec::new();
    for a in cfg.alerts.iter().filter(|a| has_level(a)) {
        let shown = cfg.rows.iter().any(|r| r.label == a.label);
        let digits = cfg
            .rows
            .iter()
            .chain(cfg.catalog.iter())
            .find(|r| r.label == a.label)
            .map_or(2, |r| r.digits);
        let quote = board
            .cells
            .iter()
            .find(|c| c.label == a.label && !c.stale)
            .and_then(|c| c.quote.as_ref())
            .filter(|q| q.source.is_empty() && q.price.is_finite() && !crate::model::is_stale(q, now, stale_after, cfg.delayed_grace_s));
        let tail = |s: String| if shown { s } else { format!("{s} (not on the strip, cannot fire)") };
        let mut push = |text: String| out.push(OverviewLine { text: tail(text), warn: !shown });
        let away = |level: f64, above: bool| match quote {
            None => "no fresh price".to_string(),
            Some(q) if q.price <= 0.0 => "no fresh price".to_string(),
            Some(q) => {
                let gap = (level - q.price) / q.price * 100.0;
                if (above && gap <= 0.0) || (!above && gap >= 0.0) {
                    format!("reached, {:.1} % past", gap.abs())
                } else {
                    format!("{:.1} % away", gap.abs())
                }
            }
        };
        if let Some(l) = a.above.filter(|l| l.is_finite()) {
            push(format!("{} above {}, {}", a.label, fmt_price(l, digits), away(l, true)));
        }
        if let Some(l) = a.below.filter(|l| l.is_finite()) {
            push(format!("{} below {}, {}", a.label, fmt_price(l, digits), away(l, false)));
        }
        if let Some(d) = a.day_move.filter(|d| d.is_finite() && *d > 0.0) {
            let now_pct = quote.and_then(|q| q.change_pct).filter(|c| c.is_finite());
            push(match now_pct {
                Some(c) => format!("{} day move \u{b1}{d} %, now {c:+.1} %", a.label),
                None => format!("{} day move \u{b1}{d} %, no fresh price", a.label),
            });
        }
        if let Some(k) = (a.fast as usize).checked_sub(1).filter(|k| *k < FAST_MOVES.len()) {
            push(format!("{} fast move {}", a.label, fast_label(k)));
        }
    }
    out
}

/// Validate a webhook URL taken from the clipboard. Trimmed; `http` or `https` only (via
/// `webhook_host::redact`, which must not answer "invalid webhook URL"); no whitespace and no
/// control characters inside; at most 2,048 bytes. `Ok` is the trimmed URL. `Err` is a fixed
/// sentence that never contains any part of the input.
pub fn accept_webhook(clipboard_text: &str) -> Result<String, &'static str> {
    // Every error is a fixed sentence. None of them may ever quote `clipboard_text`: it is a secret.
    const NOT_A_URL: &str = "The clipboard does not hold a web address.";
    const MAX_BYTES: usize = 2_048;
    let url = clipboard_text.trim();
    if url.is_empty() {
        return Err("The clipboard is empty.");
    }
    if url.len() > MAX_BYTES {
        return Err("The address is longer than 2,048 bytes.");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The address has spaces or control characters in it.");
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(NOT_A_URL);
    }
    // `redact` answers with this sentinel for anything it cannot parse as an http(s) URL with a host.
    if redact(url) == "invalid webhook URL" {
        return Err(NOT_A_URL);
    }
    Ok(url.to_string())
}

/// Show a balloon on the tray icon. Focus Assist silently swallows these, which is why the panel
/// also carries the last fired alert in its footer note.
pub fn balloon(hwnd: HWND, id: u32, title: &str, body: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = id;
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    let copy = |dst: &mut [u16], src: &str| {
        for (slot, ch) in dst.iter_mut().zip(src.encode_utf16()) {
            *slot = ch;
        }
    };
    copy(&mut nid.szInfoTitle, &title.chars().take(63).collect::<String>());
    copy(&mut nid.szInfo, &body.chars().take(255).collect::<String>());
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

// URL parsing/redaction is pure and tested separately from Win32 alert delivery.
use crate::webhook_host::redact;

/// What to POST for this endpoint. Discord and Slack both take `{"content"}`/`{"text"}`; ntfy takes
/// a plain body with the title in a header. Anything else gets both keys, which most receivers
/// ignore harmlessly.
fn payload(url: &str, title: &str, body: &str) -> (String, &'static str) {
    let host = redact(url);
    let text = format!("{title}: {body}");
    if host.contains("discord") {
        (serde_json::json!({ "content": text, "allowed_mentions": { "parse": [] } }).to_string(), "application/json")
    } else if host.contains("ntfy") {
        (format!("{title}\n{body}"), "text/plain")
    } else {
        (serde_json::json!({ "text": text, "title": title, "body": body }).to_string(), "application/json")
    }
}

/// Posts started and not yet finished (including the one more try and its wait). Read by the quit path.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
/// The app is quitting: a waiting retry goes at once instead of after its pause.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// How long quitting waits for a POST in flight.
pub const QUIT_WAIT: Duration = Duration::from_secs(2);

/// Counts one post while it lives, so a thread that panics still lets go.
struct Flight;

impl Drop for Flight {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Sleep `secs` in short slices, ending early when the app is quitting.
fn nap(secs: f64) {
    let until = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
    while Instant::now() < until && !QUITTING.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(50));
    }
}

/// Wait until `counter` reads 0 or `max` has passed; true when it did reach 0. Polls every 20 ms.
fn wait_idle(counter: &AtomicUsize, max: Duration) -> bool {
    let until = Instant::now() + max;
    while counter.load(Ordering::SeqCst) > 0 {
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Quitting: give a POST that is in flight up to [`QUIT_WAIT`] to finish (its retry, if any, goes at
/// once). Returns immediately when none is. Blocks the caller, so call it only from the quit path and
/// outside `with_ui`.
pub fn wait_for_posts() {
    if IN_FLIGHT.load(Ordering::SeqCst) == 0 {
        return;
    }
    QUITTING.store(true, Ordering::SeqCst);
    wait_idle(&IN_FLIGHT, QUIT_WAIT);
}

/// POST one alert on a detached thread, and once more if the first try failed on the network or with
/// a 5xx (`retry`). `on_done` gets a redacted error, or nothing, and whether it took the second try,
/// so the UI can say "webhook failed" or "sent on retry" without ever holding the URL.
///
/// Returns false when the thread could not be started; `on_done` is then never called.
pub fn spawn_post(url: String, title: String, body: String, on_done: Box<dyn Fn(Option<String>, bool) + Send>) -> bool {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let started = thread::Builder::new().name("alert-post".into()).spawn(move || {
        let _flight = Flight;
        let (json, ctype) = payload(&url, &title, &body);
        let (result, retried) = crate::retry::run(|| crate::source::post(&url, &json, ctype, POST_TIMEOUT_S), nap);
        on_done(result.err().map(|e| format!("{}: {e}", redact(&url))), retried);
    });
    if started.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
    started.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cell, Link, Quote};

    fn quote(price: f64) -> Quote {
        Quote { symbol: "XAU=".into(), price, ts: Some(100.0), realtime: true, ..Quote::default() }
    }

    fn board(price: f64, stale: bool) -> Board {
        let mut b = Board::new(&[]);
        b.cells = vec![Cell { label: "XAU".into(), digits: 2, quote: Some(quote(price)), stale }];
        b.link = Link::Live;
        b
    }

    fn alert(above: Option<f64>, below: Option<f64>) -> Alert {
        Alert { label: "XAU".into(), above, below, ..Alert::default() }
    }

    #[test]
    fn fires_once_across_the_level_then_respects_the_cooldown() {
        let mut cfg = Config { alerts: vec![alert(Some(4100.0), None)], stale_after_s: 1_000_000.0, ..Config::default() };
        assert_eq!(check(&mut cfg, &board(4200.0, false), 10_000.0).len(), 1);
        // still above, but inside the cooldown
        assert!(check(&mut cfg, &board(4200.0, false), 10_100.0).is_empty());
        // below the level: nothing to say
        assert!(check(&mut cfg, &board(4000.0, false), 20_000.0).is_empty());
        // crossed back up after the cooldown
        assert_eq!(check(&mut cfg, &board(4200.0, false), 30_000.0).len(), 1);
        assert_eq!(cfg.alerts[0].last, 30_000.0);
    }

    #[test]
    fn fallback_or_stale_quote_never_sets_alert_reference() {
        let cfg = Config::default();
        let mut b = Board::new(&cfg.rows);
        b.cells[0].stale = false;
        b.cells[0].quote = Some(crate::model::Quote { source: "Gold API", price: 100.0, ts: Some(1000.0), realtime: true, ..Default::default() });
        assert_eq!(reference_price(&b.cells[0], &cfg, 0.0, 1000.0), None);
        b.cells[0].quote.as_mut().unwrap().source = "";
        assert_eq!(reference_price(&b.cells[0], &cfg, 0.0, 1000.0), Some(100.0));
        b.cells[0].stale = true;
        assert_eq!(reference_price(&b.cells[0], &cfg, 0.0, 1000.0), None);
    }

    #[test]
    fn a_stale_quote_never_fires() {
        let mut cfg = Config { alerts: vec![alert(Some(4100.0), None)], stale_after_s: 1_000_000.0, ..Config::default() };
        assert!(check(&mut cfg, &board(4200.0, true), 10_000.0).is_empty());
        assert_eq!(cfg.alerts[0].last, 0.0, "a stale quote must not burn the cooldown");
    }

    #[test]
    fn quote_that_aged_stale_since_publication_never_fires() {
        let mut cfg = Config { alerts: vec![alert(Some(4100.0), None)], stale_after_s: 90.0, ..Config::default() };
        let b = board(4200.0, false); // timestamp 100; the snapshot was fresh when published
        assert!(check(&mut cfg, &b, 10_000.0).is_empty());
        assert_eq!(cfg.alerts[0].last, 0.0);
    }

    #[test]
    fn below_alerts_and_a_minimum_cooldown() {
        let mut cfg = Config { alerts: vec![alert(None, Some(4000.0))], stale_after_s: 1_000_000.0, ..Config::default() };
        assert_eq!(check(&mut cfg, &board(3900.0, false), 10_000.0).len(), 1);
        // the cooldown was configured as 900, but the floor keeps a 10 s repeat out
        assert!(check(&mut cfg, &board(3900.0, false), 10_010.0).is_empty());
        // no level at all: never fires
        cfg.alerts = vec![alert(None, None)];
        assert!(check(&mut cfg, &board(3900.0, false), 50_000.0).is_empty());
    }

    fn fresh_cfg(alerts: Vec<Alert>) -> Config {
        Config { alerts, stale_after_s: 1_000_000.0, ..Config::default() }
    }

    fn board_chg(price: f64, chg: Option<f64>) -> Board {
        let mut b = board(price, false);
        b.cells[0].quote.as_mut().unwrap().change_pct = chg;
        b
    }

    #[test]
    fn day_move_fires_on_both_signs_and_only_on_a_fresh_quote() {
        let mut cfg = fresh_cfg(vec![Alert { day_move: Some(2.0), ..alert(None, None) }]);
        assert!(check(&mut cfg, &board_chg(4000.0, Some(1.9)), 10_000.0).is_empty());
        assert!(check(&mut cfg, &board_chg(4000.0, None), 10_000.0).is_empty());
        let up = check(&mut cfg, &board_chg(4000.0, Some(2.0)), 10_000.0);
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].label, "XAU");
        assert_eq!(up[0].at, 10_000.0);
        assert!(up[0].title.contains("+2.0"), "{}", up[0].title);
        let down = check(&mut cfg, &board_chg(4000.0, Some(-3.5)), 20_000.0);
        assert_eq!(down.len(), 1);
        assert!(down[0].title.contains("-3.5"), "{}", down[0].title);
        // inside the cooldown: silent
        assert!(check(&mut cfg, &board_chg(4000.0, Some(-3.5)), 20_100.0).is_empty());
        // stale: silent, and the cooldown is not burnt
        cfg.alerts[0].last = 0.0;
        let mut stale = board_chg(4000.0, Some(5.0));
        stale.cells[0].stale = true;
        assert!(check(&mut cfg, &stale, 30_000.0).is_empty());
        assert_eq!(cfg.alerts[0].last, 0.0);
    }

    #[test]
    fn a_price_level_and_a_day_move_together_fire_once() {
        let mut cfg = fresh_cfg(vec![Alert { day_move: Some(1.0), ..alert(Some(4100.0), None) }]);
        assert_eq!(check(&mut cfg, &board_chg(4200.0, Some(3.0)), 10_000.0).len(), 1);
    }

    #[test]
    fn once_clears_the_level_that_fired_and_prunes_an_empty_alert() {
        let mut cfg = fresh_cfg(vec![Alert { once: true, ..alert(Some(4100.0), Some(3000.0)) }]);
        assert_eq!(check(&mut cfg, &board(4200.0, false), 10_000.0).len(), 1);
        assert_eq!((cfg.alerts[0].above, cfg.alerts[0].below), (None, Some(3000.0)));
        // the other level fires later and takes the alert with it
        assert_eq!(check(&mut cfg, &board(2900.0, false), 20_000.0).len(), 1);
        assert!(cfg.alerts.is_empty());
        // an alert that is not `once` keeps its level
        let mut cfg = fresh_cfg(vec![alert(Some(4100.0), None)]);
        check(&mut cfg, &board(4200.0, false), 10_000.0);
        assert_eq!(cfg.alerts[0].above, Some(4100.0));
        // `once` on a day move, and another symbol's alert is untouched
        let other = Alert { label: "BTC".into(), ..alert(Some(1.0), None) };
        let mut cfg = fresh_cfg(vec![other.clone(), Alert { once: true, day_move: Some(1.0), ..alert(None, None) }]);
        assert_eq!(check(&mut cfg, &board_chg(4000.0, Some(1.5)), 10_000.0).len(), 1);
        assert_eq!(cfg.alerts, vec![other]);
    }

    #[test]
    fn silence_order_and_the_clock_ending_a_timed_mute() {
        let mut cfg = Config::default();
        assert_eq!(silence(&cfg, 1000.0, 600), None);
        cfg.quiet_hours = Some([22 * 60, 7 * 60]);
        assert_eq!(silence(&cfg, 1000.0, 60), Some(Silence::Quiet), "01:00 is inside 22:00 to 07:00");
        assert_eq!(silence(&cfg, 1000.0, 600), None, "10:00 is outside it");
        cfg.quiet_hours = None;
        cfg.alerts_mute_until = 2000.0;
        assert_eq!(silence(&cfg, 1000.0, 600), Some(Silence::MutedUntil(2000.0)));
        assert_eq!(silence(&cfg, 2000.0, 600), None, "the mute ends at its time, no timer");
        assert_eq!(silence(&cfg, 3000.0, 600), None);
        cfg.alerts_muted = true;
        assert_eq!(silence(&cfg, 3000.0, 600), Some(Silence::Muted));
        cfg.alerts_mute_until = f64::NAN;
        cfg.alerts_muted = false;
        assert_eq!(silence(&cfg, 1.0, 0), None);
    }

    #[test]
    fn a_silenced_alert_still_burns_its_cooldown() {
        let mut cfg = fresh_cfg(vec![alert(Some(4100.0), None)]);
        cfg.alerts_muted = true;
        let f = check(&mut cfg, &board(4200.0, false), 10_000.0);
        assert_eq!(f.len(), 1, "check still reports it; the caller logs it as silenced");
        assert_eq!(silence(&cfg, 10_000.0, 0), Some(Silence::Muted));
        // lifting the mute does not release a burst
        cfg.alerts_muted = false;
        assert!(check(&mut cfg, &board(4200.0, false), 10_100.0).is_empty());
    }

    fn cell_with(high: Option<f64>, low: Option<f64>, prev: Option<f64>) -> Cell {
        let mut c = Cell { label: "XAU".into(), digits: 2, quote: Some(quote(4000.0)), stale: false };
        let q = c.quote.as_mut().unwrap();
        (q.high, q.low, q.prev_close) = (high, low, prev);
        c
    }

    #[test]
    fn presets_refuse_missing_and_zero_fields() {
        let none = cell_with(None, None, None);
        let zero = cell_with(Some(0.0), Some(0.0), Some(0.0));
        let nan = cell_with(Some(f64::NAN), Some(f64::INFINITY), Some(-1.0));
        for c in [&none, &zero, &nan] {
            for p in [Preset::DayHigh, Preset::DayLow, Preset::PrevClose] {
                assert_eq!(preset_level(p, c, 4000.0), None);
            }
        }
        assert_eq!(preset_level(Preset::Pct(1.0), &none, 0.0), None);
        assert_eq!(preset_level(Preset::Pct(1.0), &none, f64::NAN), None);
        let c = cell_with(Some(4100.0), Some(3900.0), Some(3950.0));
        // "past" the extremes: one tick beyond it (digits 2, so a tick is 0.01)
        assert_eq!(preset_level(Preset::DayHigh, &c, 4000.0), Some(Level::Above(4100.0 + 0.01)));
        assert_eq!(preset_level(Preset::DayLow, &c, 4000.0), Some(Level::Below(3900.0 - 0.01)));
        assert_eq!(preset_level(Preset::PrevClose, &c, 4000.0), Some(Level::Below(3950.0)));
        assert_eq!(preset_level(Preset::PrevClose, &c, 3900.0), Some(Level::Above(3950.0)));
        assert_eq!(preset_level(Preset::PrevClose, &c, 3950.0), None);
        assert_eq!(preset_level(Preset::DayMove(2.0), &none, 4000.0), Some(Level::DayMove(2.0)));
        match preset_level(Preset::Pct(-1.0), &none, 4000.0) {
            Some(Level::Below(v)) => assert!((v - 3960.0).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
        // every table entry is usable on a complete cell
        for p in &PRESETS {
            assert!(preset_level(p.preset, &c, 4000.0).is_some(), "{}", p.label);
        }
    }

    #[test]
    fn the_log_caps_keeps_order_and_routes_late_results() {
        let mut log = Log::default();
        let n = LOG_CAP + 2;
        let seqs: Vec<u64> = (0..n).map(|i| log.push(i as f64, format!("a{i}"), Outcome::Shown)).collect();
        assert_eq!(log.len(), LOG_CAP);
        let titles: Vec<String> = log.newest_first().into_iter().map(|e| e.title).collect();
        assert_eq!(titles.first().map(String::as_str), Some(format!("a{}", n - 1).as_str()));
        assert_eq!(titles.last().map(String::as_str), Some("a2"));
        assert!(!log.set_outcome(seqs[0], Outcome::Sent), "pushed out");
        let last = n - 1;
        assert!(log.set_outcome(seqs[last], Outcome::SendFailed));
        assert_eq!(log.newest_first()[0].outcome, Outcome::SendFailed);
        log.clear();
        assert_eq!(log.len(), 0);
        let fresh = log.push(0.0, "x".into(), Outcome::Shown);
        assert!(fresh > seqs[last], "sequence numbers survive a clear");
    }

    /// A fixed clock: the offset that makes `now_utc` read `minute_now` of the local day.
    fn clock_at(minute_now: u16, now_utc: f64) -> impl Fn(f64) -> i32 {
        let offset = minute_now as i32 * 60 - now_utc.rem_euclid(86_400.0) as i32;
        move |_| offset
    }

    #[test]
    fn local_clock_labels() {
        assert_eq!(hhmm(0), "00:00");
        assert_eq!(hhmm(14 * 60 + 5), "14:05");
        assert_eq!(local_hhmm(&clock_at(13 * 60 + 30, 1000.0), 1000.0, 1000.0 + 3600.0), "14:30");
        assert_eq!(local_hhmm(&clock_at(23 * 60 + 30, 1000.0), 1000.0, 1000.0 + 3600.0), "00:30");
        assert_eq!(local_hhmm(&clock_at(0, 1000.0), 1000.0, 1000.0 - 60.0), "23:59");
    }

    #[test]
    fn apply_level_sets_one_side_and_rearms() {
        let mut a = Alert { last: 5.0, below: Some(1.0), ..alert(None, None) };
        apply_level(&mut a, Level::Above(9.0));
        assert_eq!((a.above, a.below, a.last), (Some(9.0), Some(1.0), 0.0));
        apply_level(&mut a, Level::DayMove(2.0));
        assert_eq!(a.day_move, Some(2.0));
    }

    #[test]
    fn round_level_keeps_the_symbols_digits() {
        assert_eq!(round_level(4200.126, 2), 4200.13);
        assert_eq!(round_level(98_765.4, 0), 98_765.0);
    }

    #[test]
    fn parse_level_accepts_plain_and_grouped_prices() {
        for (text, want) in [
            ("4200", 4200.0),
            (" 4,200.50 ", 4200.5),
            ("4200.5", 4200.5),
            (".5", 0.5),
            ("1,234,567", 1_234_567.0),
            ("999", 999.0),
            ("12,345.678", 12_345.678),
            ("0.5", 0.5),
        ] {
            assert_eq!(parse_level(text), Some(want), "{text:?}");
        }
        for text in ["0", "0.00", "-5", "+5", "1e3", "inf", "NaN", ",4200", "4200,", "4,,200", "4.2.0", "4.200,5", "4 200", "", "."] {
            assert_eq!(parse_level(text), None, "{text:?} is refused");
        }
    }

    #[test]
    fn a_comma_that_does_not_group_is_refused_with_a_reason() {
        // seen failing first: `4200,5` was read as 42,005 and `4,20` as 420.
        for text in ["4200,5", "4,20", "1,2345", ",500", "4,20,0", "4.200,5"] {
            assert!(parse_level_why(text).is_err(), "{text:?}");
            assert_eq!(parse_level(text), None, "{text:?}");
        }
        assert!(parse_level_why("4200,5").unwrap_err().contains("exactly three digits"));
        assert_eq!(parse_level_why("4,200.50"), Ok(4200.5));
        assert_eq!(parse_level_why("abc"), Err("that is not a price"));
    }

    #[test]
    fn the_far_level_question_asks_above_twenty_percent_only() {
        // exactly 20 % is not asked, a hair more is.
        assert_eq!(far_level_question("XAU", 1200.0, 1000.0, 2), None);
        assert_eq!(far_level_question("XAU", 800.0, 1000.0, 2), None);
        let q = far_level_question("XAU", 1200.01, 1000.0, 2).expect("asks");
        assert!(q.contains("1,200.01") && q.contains("1,000.00") && q.contains("above"), "{q}");
        let q = far_level_question("XAU", 799.0, 1000.0, 0).expect("asks");
        assert!(q.contains("below") && q.contains("799"), "{q}");
        assert_eq!(far_level_question("XAU", f64::NAN, 1000.0, 2), None);
        assert_eq!(far_level_question("XAU", 5.0, 0.0, 2), None);
    }

    #[test]
    fn a_clock_set_back_does_not_silence_an_alert() {
        // seen failing first: with `last` in the future the old test `now - last < cooldown` held
        // for as long as the clock was behind, so the alert never fired.
        let mut cfg = fresh_cfg(vec![Alert { last: 20_000.0, ..alert(Some(4100.0), None) }]);
        assert_eq!(check(&mut cfg, &board(4200.0, false), 10_000.0).len(), 1);
        assert_eq!(cfg.alerts[0].last, 10_000.0, "the fire writes a fresh stamp");
        assert!(check(&mut cfg, &board(4200.0, false), 10_010.0).is_empty(), "and the cooldown runs again");
        assert!(cooled(100.0, 0.0, 30.0) && !cooled(10.0, 0.0, 30.0) && cooled(30.0, 0.0, 0.0));
        assert!(!cooled(29.0, 0.0, f64::NAN) && cooled(30.0, 0.0, f64::NAN));
    }

    #[test]
    fn the_side_rule_follows_the_fresh_price() {
        assert_eq!(side_of(4300.0, 4200.4, 2), Ok(Level::Above(4300.0)));
        assert_eq!(side_of(4100.0, 4200.0, 2), Ok(Level::Below(4100.0)));
        assert_eq!(side_of(4200.004, 4200.001, 2), Err("that is the current price"));
        for bad in [f64::NAN, 0.0, -1.0] {
            assert_eq!(side_of(bad, 4200.0, 2), Err("that is not a price"), "level {bad}");
        }
        for bad in [f64::NAN, 0.0] {
            assert_eq!(side_of(4300.0, bad, 2), Err("there is no fresh price"), "reference {bad}");
        }
        // One tick above the reference: at digits 0 the tick is 1, at digits 4 it is 0.0001.
        assert_eq!(side_of(4201.0, 4200.0, 0), Ok(Level::Above(4201.0)));
        assert_eq!(side_of(4200.4, 4200.0, 0), Err("that is the current price"), "rounds to the same whole price");
        assert_eq!(side_of(4200.0001, 4200.0, 4), Ok(Level::Above(4200.0001)));
        assert_eq!(side_of(4200.00004, 4200.0, 4), Err("that is the current price"));
    }

    #[test]
    fn edge_cases_for_check_presets_clock_and_log() {
        // check: nothing configured, nothing fires.
        let mut empty = fresh_cfg(vec![]);
        assert!(check(&mut empty, &Board::new(&[]), 10_000.0).is_empty());
        // check: a NaN price fires nothing and burns no cooldown.
        let mut cfg = fresh_cfg(vec![alert(Some(4100.0), Some(3900.0))]);
        assert!(check(&mut cfg, &board(f64::NAN, false), 10_000.0).is_empty());
        assert_eq!(cfg.alerts[0].last, 0.0);
        // check: a price exactly on a level fires (both sides are inclusive).
        assert_eq!(check(&mut cfg, &board(4100.0, false), 10_000.0).len(), 1);
        let mut cfg = fresh_cfg(vec![alert(None, Some(3900.0))]);
        assert_eq!(check(&mut cfg, &board(3900.0, false), 10_000.0).len(), 1);
        // check: a NaN or zero day move, or a NaN change, never fires.
        let mut cfg = fresh_cfg(vec![Alert { day_move: Some(f64::NAN), ..alert(None, None) }]);
        assert!(check(&mut cfg, &board_chg(4000.0, Some(50.0)), 10_000.0).is_empty());
        let mut cfg = fresh_cfg(vec![Alert { day_move: Some(0.0), ..alert(None, None) }]);
        assert!(check(&mut cfg, &board_chg(4000.0, Some(0.0)), 10_000.0).is_empty());
        let mut cfg = fresh_cfg(vec![Alert { day_move: Some(2.0), ..alert(None, None) }]);
        assert!(check(&mut cfg, &board_chg(4000.0, Some(f64::NAN)), 10_000.0).is_empty());
        // check: a cooldown of 0 or NaN is floored to 30 s.
        for cd in [0.0, f64::NAN] {
            let mut cfg = fresh_cfg(vec![Alert { cooldown_s: cd, ..alert(Some(4100.0), None) }]);
            assert_eq!(check(&mut cfg, &board(4200.0, false), 10_000.0).len(), 1, "cooldown {cd}");
            assert!(check(&mut cfg, &board(4200.0, false), 10_029.0).is_empty(), "cooldown {cd}");
            assert_eq!(check(&mut cfg, &board(4200.0, false), 10_030.0).len(), 1, "cooldown {cd}");
        }
        // presets: a negative or NaN reference gives no percentage level. Today's high and low do
        // not depend on the reference, and a day move needs no level at all.
        let c = cell_with(Some(4100.0), Some(3900.0), Some(3950.0));
        assert_eq!(preset_level(Preset::Pct(1.0), &c, -4000.0), None);
        assert_eq!(preset_level(Preset::PrevClose, &c, f64::NAN), None);
        assert_eq!(preset_level(Preset::DayHigh, &c, -4000.0), Some(Level::Above(4100.0 + 0.01)));
        assert_eq!(preset_level(Preset::DayMove(2.0), &c, -4000.0), Some(Level::DayMove(2.0)));
        assert_eq!(preset_level(Preset::Pct(0.0), &c, 4000.0), None, "zero percent is no level");
        assert_eq!(preset_level(Preset::Pct(f64::NAN), &c, 4000.0), None);
        assert_eq!(preset_level(Preset::DayMove(0.0), &c, 4000.0), None);
        assert_eq!(preset_level(Preset::DayMove(f64::NAN), &c, 4000.0), None);
        // clock: a NaN time is treated as "now"; minute 1440 wraps to midnight.
        assert_eq!(local_hhmm(&clock_at(600, 1000.0), 1000.0, f64::NAN), "10:00");
        assert_eq!(hhmm(1440), "00:00");
        assert_eq!(hhmm(1439), "23:59");
        // log: an empty log is empty; exactly LOG_CAP entries keep all of them; the next drops the oldest.
        let titles = |log: &Log| -> Vec<String> { log.newest_first().into_iter().map(|e| e.title).collect() };
        let mut log = Log::default();
        assert!(log.newest_first().is_empty());
        for i in 0..LOG_CAP {
            log.push(i as f64, format!("a{i}"), Outcome::Shown);
        }
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(titles(&log).first().map(String::as_str), Some(format!("a{}", LOG_CAP - 1).as_str()));
        assert_eq!(titles(&log).last().map(String::as_str), Some("a0"));
        let eleventh = log.push(LOG_CAP as f64, format!("a{LOG_CAP}"), Outcome::Shown);
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(titles(&log).first().map(String::as_str), Some(format!("a{LOG_CAP}").as_str()));
        assert_eq!(titles(&log).last().map(String::as_str), Some("a1"));
        // set_outcome on sequence 0, on the dropped first entry (seq 1), or on a live one.
        assert!(!log.set_outcome(0, Outcome::Sent));
        assert!(!log.set_outcome(1, Outcome::Sent), "the first entry was dropped");
        assert!(log.set_outcome(eleventh, Outcome::Sent));
        // a single entry is listed and can be changed.
        let mut one = Log::default();
        let s = one.push(0.0, "only".into(), Outcome::Quiet);
        assert_eq!(one.newest_first().len(), 1);
        assert!(one.set_outcome(s, Outcome::Muted));
        assert_eq!(one.newest_first()[0].outcome, Outcome::Muted);
    }

    #[test]
    fn mute_state_never_reports_quiet() {
        let cfg = Config { quiet_hours: Some([1320, 420]), ..Config::default() };
        // inside the quiet window, nothing muted: no state at all, not Quiet
        assert_eq!(mute_state(&cfg, 1000.0), None);
        assert_eq!(silence(&cfg, 1000.0, 60), Some(Silence::Quiet));
        let timed = Config { alerts_mute_until: 2000.0, ..cfg.clone() };
        assert_eq!(mute_state(&timed, 1000.0), Some(Silence::MutedUntil(2000.0)));
        assert_eq!(mute_state(&timed, 2000.0), None, "the mute ends at its time");
        let muted = Config { alerts_muted: true, ..cfg };
        assert_eq!(mute_state(&muted, 1000.0), Some(Silence::Muted));
    }

    #[test]
    fn past_the_day_extremes_needs_one_tick_more() {
        let c = cell_with(Some(4100.0), Some(3900.0), None);
        // High: a price exactly at the high does not trip the level; the level itself (one tick
        // above the high) does.
        let Some(Level::Above(hi)) = preset_level(Preset::DayHigh, &c, 4000.0) else { panic!("no high level") };
        assert!((hi - 4100.01).abs() < 1e-9, "{hi}");
        let mut cfg = fresh_cfg(vec![alert(Some(hi), None)]);
        assert!(check(&mut cfg, &board(4100.0, false), 10_000.0).is_empty(), "at the high is not past it");
        assert_eq!(check(&mut cfg, &board(hi, false), 10_000.0).len(), 1, "one tick past fires");
        // Low: the mirror image.
        let Some(Level::Below(lo)) = preset_level(Preset::DayLow, &c, 4000.0) else { panic!("no low level") };
        assert!((lo - 3899.99).abs() < 1e-9, "{lo}");
        let mut cfg = fresh_cfg(vec![alert(None, Some(lo))]);
        assert!(check(&mut cfg, &board(3900.0, false), 10_000.0).is_empty(), "at the low is not past it");
        assert_eq!(check(&mut cfg, &board(lo, false), 10_000.0).len(), 1, "one tick past fires");
        // Digits 0: a tick is one whole unit.
        let mut c0 = cell_with(Some(4100.0), Some(3900.0), None);
        c0.digits = 0;
        assert_eq!(preset_level(Preset::DayHigh, &c0, 4000.0), Some(Level::Above(4101.0)));
        assert_eq!(preset_level(Preset::DayLow, &c0, 4000.0), Some(Level::Below(3899.0)));
        // A low so small that one tick below it is not positive has no level.
        let tiny = cell_with(Some(4100.0), Some(0.01), None);
        assert_eq!(preset_level(Preset::DayLow, &tiny, 4000.0), None);
        let zero_digits_tiny = Cell { digits: 0, ..cell_with(Some(4100.0), Some(1.0), None) };
        assert_eq!(preset_level(Preset::DayLow, &zero_digits_tiny, 4000.0), None, "1 - 1 is not positive");
    }

    #[test]
    fn quiet_window_inside_outside_wrapping_and_empty() {
        // A window that does not wrap midnight: `from` is inside, `to` is outside.
        assert!(quiet(600, [540, 1020]));
        assert!(quiet(540, [540, 1020]), "from is inside");
        assert!(!quiet(1020, [540, 1020]), "to is outside");
        assert!(!quiet(539, [540, 1020]));
        // A window that wraps midnight.
        assert!(quiet(0, [1320, 420]));
        assert!(quiet(1380, [1320, 420]), "from is inside");
        assert!(quiet(419, [1320, 420]));
        assert!(!quiet(420, [1320, 420]), "to is outside");
        assert!(!quiet(720, [1320, 420]));
        assert!(!quiet(1319, [1320, 420]));
        // Empty: from == to is off, whatever the time.
        for x in [0u16, 600, 1439] {
            assert!(!quiet(x, [600, 600]), "minute {x}");
        }
        // The last minute of the day, and a window that ends exactly at midnight.
        assert!(quiet(1439, [1320, 0]), "the last minute before the window ends is inside");
        assert!(!quiet(0, [1320, 0]), "midnight is the end, so it is outside");
        // silence() with quiet hours: inside is Quiet, outside is None, and a mute wins over quiet.
        let mut cfg = Config { quiet_hours: Some([1320, 420]), ..Config::default() };
        assert_eq!(silence(&cfg, 1000.0, 60), Some(Silence::Quiet));
        assert_eq!(silence(&cfg, 1000.0, 720), None);
        cfg.alerts_muted = true;
        assert_eq!(silence(&cfg, 1000.0, 60), Some(Silence::Muted), "a mute wins over quiet hours");
        cfg.alerts_muted = false;
        cfg.alerts_mute_until = 2000.0;
        assert_eq!(silence(&cfg, 1000.0, 60), Some(Silence::MutedUntil(2000.0)));
    }

    #[test]
    fn describe_lists_what_is_set() {
        assert_eq!(describe(&alert(Some(4200.0), None), 2), "above 4,200.00");
        let day = |d: f64| Alert { day_move: Some(d), ..alert(Some(4200.0), None) };
        assert_eq!(describe(&day(2.0), 2), "above 4,200.00 \u{b7} day \u{b1}2 %");
        let all = Alert { once: true, day_move: Some(2.0), ..alert(Some(4200.0), Some(3900.0)) };
        assert_eq!(describe(&all, 2), "above 4,200.00 \u{b7} below 3,900.00 \u{b7} day \u{b1}2 % \u{b7} once");
        assert_eq!(describe(&alert(None, None), 2), "nothing set");
        assert_eq!(describe(&alert(Some(4200.0), None), 0), "above 4,200");
        assert_eq!(describe(&day(2.5), 2), "above 4,200.00 \u{b7} day \u{b1}2.5 %");
        // Edge cases: a level or day move that can never fire is not listed as set.
        assert_eq!(describe(&alert(Some(f64::NAN), None), 2), "nothing set");
        assert_eq!(describe(&alert(Some(f64::INFINITY), None), 2), "nothing set");
        assert_eq!(describe(&Alert { day_move: Some(0.0), ..alert(None, None) }, 2), "nothing set");
        assert_eq!(describe(&Alert { day_move: Some(f64::NAN), ..alert(None, None) }, 2), "nothing set");
        // `once` on its own says nothing about a level, so it is not a setting either.
        assert_eq!(describe(&Alert { once: true, ..alert(None, None) }, 2), "nothing set");
        // A negative level is written with its sign.
        assert_eq!(describe(&alert(None, Some(-5.0)), 2), "below -5.00");
    }

    /// A made-up rule: UTC+1 until `change`, UTC+2 from it on (a clock change in the middle).
    fn spring_forward(change: f64) -> impl Fn(f64) -> i32 {
        move |t| if t >= change { 7_200 } else { 3_600 }
    }

    #[test]
    fn a_time_across_a_clock_change_shows_the_hour_it_had() {
        // seen failing first: one offset (today's) was applied to a time on the other side of the change.
        let change = 1_000_000.0;
        let rule = spring_forward(change);
        let (before, after) = (change - 1_800.0, change + 1_800.0);
        // 1,000,000 s = day 11, 13:46:40 UTC, so 30 minutes before is 13:16 UTC = 14:16 at +1, and
        // 30 minutes after is 14:16 UTC = 16:16 at +2.
        assert_eq!(local_hhmm(&rule, after, before), "14:16");
        assert_eq!(local_hhmm(&rule, after, after), "16:16");
        // The mute label after a change: a mute set to end at `after`, read now (after) and earlier.
        assert_eq!(local_hhmm(&rule, before, after), "16:16");
        let e = Entry { seq: 1, at: before, title: "XAU".into(), outcome: Outcome::Shown };
        assert!(entry_line(&e, &rule, after).starts_with("14:16  "));
    }

    #[test]
    fn entry_line_names_time_title_and_outcome() {
        let e = |outcome| Entry { seq: 1, at: 1000.0 + 3600.0, title: "XAU above 4,200.00".into(), outcome };
        assert_eq!(entry_line(&e(Outcome::Muted), &clock_at(13 * 60 + 30, 1000.0), 1000.0), "14:30  XAU above 4,200.00 \u{b7} muted");
        assert_eq!(entry_line(&e(Outcome::Shown), &clock_at(13 * 60 + 30, 1000.0), 1000.0), "14:30  XAU above 4,200.00 \u{b7} shown");
        assert_eq!(entry_line(&e(Outcome::Quiet), &clock_at(13 * 60 + 30, 1000.0), 1000.0), "14:30  XAU above 4,200.00 \u{b7} quiet hours");
        assert_eq!(entry_line(&e(Outcome::Sent), &clock_at(13 * 60 + 30, 1000.0), 1000.0), "14:30  XAU above 4,200.00 \u{b7} sent");
        assert_eq!(entry_line(&e(Outcome::SendFailed), &clock_at(13 * 60 + 30, 1000.0), 1000.0), "14:30  XAU above 4,200.00 \u{b7} send failed");
        // An entry from before now, and a NaN time, fall back to safe text.
        let old = Entry { at: 1000.0 - 60.0, ..e(Outcome::Sent) };
        assert!(entry_line(&old, &clock_at(0, 1000.0), 1000.0).starts_with("23:59  "));
        let nan = Entry { at: f64::NAN, ..e(Outcome::Sent) };
        assert!(entry_line(&nan, &clock_at(600, 1000.0), 1000.0).starts_with("10:00  "));
    }

    #[test]
    fn accept_webhook_takes_good_urls_and_refuses_bad_ones_silently() {
        assert_eq!(
            accept_webhook("  https://discord.com/api/webhooks/1/SECRET\r\n"),
            Ok("https://discord.com/api/webhooks/1/SECRET".to_string())
        );
        assert_eq!(accept_webhook("http://localhost:8080/x"), Ok("http://localhost:8080/x".to_string()));
        assert_eq!(accept_webhook("HTTPS://a.com/x"), Ok("HTTPS://a.com/x".to_string()));
        // Exactly 2,048 bytes is accepted, 2,049 is not.
        let at_cap = format!("https://a.com/{}", "S".repeat(2048 - 14));
        assert_eq!(at_cap.len(), 2048);
        assert!(accept_webhook(&at_cap).is_ok());
        let over = format!("https://a.com/{}", "S".repeat(2049 - 14));
        assert_eq!(over.len(), 2049);
        // Every refusal is a fixed, short sentence that never contains the input.
        let bad = [
            "",
            "   ",
            "ftp://x/SECRET",
            "https:SECRET",
            "https://a.com/SE CRET",
            "https://a.com/SE\tCRET",
            "https://a.com/\u{7}SECRET",
            "javascript:SECRET",
            "https://",
            "SECRET",
            over.as_str(),
        ];
        for raw in bad {
            match accept_webhook(raw) {
                Ok(v) => panic!("accepted {raw:?} as {v:?}"),
                Err(e) => {
                    assert!(!e.contains("SECRET"), "error echoed the secret: {e}");
                    assert!(e.len() < 100, "{e}");
                }
            }
        }
    }

    #[test]
    fn messages_never_leak_the_webhook_secret() {
        let url = "https://discord.com/api/webhooks/1234/abcdefSECRET";
        assert_eq!(redact(url), "discord.com");
        assert!(!redact(url).contains("SECRET"));
        assert_eq!(redact("https://user:SECRET@example.com?token=SECRET#fragment"), "example.com");
        assert_eq!(redact("https://example.com#SECRET"), "example.com");
        assert_eq!(redact("https://user:SECRET@[::1]:443/path"), "[::1]");
        let (body, ct) = payload(url, "XAU above 4,100", "spot");
        assert!((ct, body.contains("SECRET")) == ("application/json", false));
        assert!(body.contains("XAU above 4,100"));
        let (ntfy, ct) = payload("https://ntfy.sh/my-topic", "t", "b");
        assert_eq!((ct, ntfy.as_str()), ("text/plain", "t\nb"));
    }

    // --- W2: repeat modes, fast move, the governed stale limit ---

    fn cfg_with(a: Alert) -> Config {
        Config { alerts: vec![a], stale_after_s: 1_000_000.0, ..Config::default() }
    }

    fn crossing(above: Option<f64>, below: Option<f64>) -> Alert {
        Alert { crossing: true, ..alert(above, below) }
    }

    #[test]
    fn each_crossing_fires_once_per_crossing_and_rearms_only_past_the_band() {
        let mut cfg = cfg_with(crossing(Some(100.0), None));
        let mut at = 1_000.0;
        let mut step = |cfg: &mut Config, price: f64| {
            at += 1.0; // a second: far inside the 900 s cooldown, which this mode does not use
            check(cfg, &board(price, false), at).len()
        };
        assert_eq!(step(&mut cfg, 101.0), 1, "the first fresh quote, already beyond, fires once");
        assert_eq!(step(&mut cfg, 102.0), 0, "still beyond: nothing more");
        assert_eq!(step(&mut cfg, 99.95), 0, "back under the level but inside the band: still latched");
        assert_eq!(step(&mut cfg, 101.0), 0, "so a wobble across the level does not fire again");
        assert_eq!(step(&mut cfg, 99.9), 0, "exactly at the band edge re-arms, and 99.9 is not beyond");
        assert_eq!(step(&mut cfg, 100.0), 1, "exactly on the level fires");
        assert_eq!(step(&mut cfg, 100.0), 0);
        assert_eq!(step(&mut cfg, 90.0), 0);
        assert_eq!(step(&mut cfg, 105.0), 1, "a new crossing after a full re-arm");
    }

    #[test]
    fn each_crossing_works_downward_and_for_a_day_move() {
        let mut cfg = cfg_with(crossing(None, Some(50.0)));
        assert_eq!(check(&mut cfg, &board(49.0, false), 1.0).len(), 1);
        assert!(check(&mut cfg, &board(49.0, false), 2.0).is_empty());
        assert!(check(&mut cfg, &board(50.04, false), 3.0).is_empty(), "inside the band");
        assert!(check(&mut cfg, &board(50.06, false), 4.0).is_empty(), "past the band: armed, not beyond");
        assert_eq!(check(&mut cfg, &board(49.0, false), 5.0).len(), 1);

        let mut cfg = cfg_with(Alert { crossing: true, day_move: Some(1.0), ..alert(None, None) });
        assert_eq!(check(&mut cfg, &board_chg(10.0, Some(1.2)), 1.0).len(), 1);
        assert!(check(&mut cfg, &board_chg(10.0, Some(1.5)), 2.0).is_empty());
        assert!(check(&mut cfg, &board_chg(10.0, Some(0.95)), 3.0).is_empty(), "inside the 0.1 band");
        assert!(check(&mut cfg, &board_chg(10.0, Some(0.8)), 4.0).is_empty(), "re-armed");
        assert_eq!(check(&mut cfg, &board_chg(10.0, Some(-1.1)), 5.0).len(), 1, "down is a crossing too");
    }

    #[test]
    fn each_crossing_never_fires_or_rearms_on_a_nan_price() {
        let mut cfg = cfg_with(crossing(Some(100.0), None));
        assert!(check(&mut cfg, &board(f64::NAN, false), 1.0).is_empty());
        assert_eq!(check(&mut cfg, &board(101.0, false), 2.0).len(), 1);
        assert!(check(&mut cfg, &board(f64::NAN, false), 3.0).is_empty());
        assert!(check(&mut cfg, &board(101.0, false), 4.0).is_empty(), "NaN did not re-arm it");
        let mut nan_level = cfg_with(crossing(Some(f64::NAN), None));
        assert!(check(&mut nan_level, &board(101.0, false), 1.0).is_empty());
    }

    #[test]
    fn setting_a_level_again_re_arms_a_latched_crossing() {
        let mut cfg = cfg_with(crossing(Some(100.0), None));
        assert_eq!(check(&mut cfg, &board(101.0, false), 1.0).len(), 1);
        apply_level(&mut cfg.alerts[0], Level::Above(100.0));
        assert_eq!(check(&mut cfg, &board(101.0, false), 2.0).len(), 1);
    }

    #[test]
    fn once_wins_over_each_crossing_when_both_are_set() {
        let mut a = crossing(Some(100.0), None);
        a.once = true;
        let mut cfg = cfg_with(a);
        assert_eq!(check(&mut cfg, &board(101.0, false), 10_000.0).len(), 1);
        assert!(cfg.alerts.is_empty(), "a spent once alert is removed");
    }

    /// One hour of samples, a tick every `step` seconds, price from `f(t)`.
    fn samples(from: f64, to: f64, step: f64, f: impl Fn(f64) -> f64) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut t = from;
        while t <= to {
            out.push((t, f(t)));
            t += step;
        }
        out
    }

    #[test]
    fn a_fast_move_needs_the_window_covered_and_no_gap_over_three_polls() {
        let now = 10_000.0;
        let flat = |_t: f64| 100.0;
        // Covered, every 3 s: a 1 % rise over 5 min is exactly the threshold and fires.
        let s = samples(now - 600.0, now, 3.0, flat);
        let m = fast_move(&s, now, 101.0, 300.0, 9.0).expect("covered");
        assert!((m - 1.0).abs() < 1e-9);
        // Under the threshold: the number is there, the caller compares.
        assert!(fast_move(&s, now, 100.5, 300.0, 9.0).unwrap() < 1.0);
        // Uncovered: the oldest sample is newer than the window start.
        let short = samples(now - 200.0, now, 3.0, flat);
        assert_eq!(fast_move(&short, now, 110.0, 300.0, 9.0), None);
        // A gap of 12 s (four polls) inside the window: nothing.
        let mut gappy = samples(now - 600.0, now - 100.0, 3.0, flat);
        gappy.extend(samples(now - 88.0, now, 3.0, flat));
        assert_eq!(fast_move(&gappy, now, 110.0, 300.0, 9.0), None);
        // Three polls exactly is allowed.
        let mut ok = samples(now - 600.0, now - 100.0, 3.0, flat);
        ok.extend(samples(now - 93.0, now, 3.0, flat));
        assert!(fast_move(&ok, now, 110.0, 300.0, 9.0).is_some());
        // The newest sample too old: the app was asleep until just now.
        let asleep = samples(now - 900.0, now - 100.0, 3.0, flat);
        assert_eq!(fast_move(&asleep, now, 110.0, 300.0, 9.0), None);
        assert_eq!(fast_move(&s, now, f64::NAN, 300.0, 9.0), None);
        assert_eq!(fast_move(&[], now, 101.0, 300.0, 9.0), None);
    }

    fn fast_alert(k: u8) -> Alert {
        Alert { fast: k, ..alert(None, None) }
    }

    #[test]
    fn a_fast_move_fires_at_the_exact_threshold_and_not_below_it() {
        let now = 10_000.0;
        let hist = samples(now - 699.0, now, 3.0, |_| 100.0);
        let mut cfg = cfg_with(fast_alert(1)); // 1 % in 5 min
        let mut b = board(101.0, false);
        b.poll_s = 3.0;
        b.cells[0].quote.as_mut().unwrap().ts = Some(now);
        let fired = check_with(&mut cfg, &b, now, &|_| hist.clone());
        assert_eq!(fired.len(), 1, "exactly 1 %");
        assert!(fired[0].title.contains("+1.0 % in 5 min"), "{}", fired[0].title);
        let mut cfg = cfg_with(fast_alert(1));
        let mut b = board(100.99, false);
        b.poll_s = 3.0;
        b.cells[0].quote.as_mut().unwrap().ts = Some(now);
        assert!(check_with(&mut cfg, &b, now, &|_| hist.clone()).is_empty());
        // Falling counts as well, and the 2 % in 15 min choice reads its own window.
        let mut cfg = cfg_with(fast_alert(2));
        let mut b = board(97.0, false);
        b.poll_s = 3.0;
        b.cells[0].quote.as_mut().unwrap().ts = Some(now);
        let long = samples(now - 1_000.0, now, 3.0, |_| 100.0);
        assert_eq!(check_with(&mut cfg, &b, now, &|_| long.clone()).len(), 1);
        // Without history (`check`) a fast move can never fire.
        let mut cfg = cfg_with(fast_alert(1));
        assert!(check(&mut cfg, &b, now).is_empty());
    }

    #[test]
    fn a_fast_move_gap_limit_follows_the_governed_interval() {
        let now = 10_000.0;
        // Samples every 40 s (a governed 15 s poll cannot make that; a 60 s one can).
        let hist = samples(now - 1_000.0, now, 40.0, |_| 100.0);
        let mut b = board(102.0, false);
        b.cells[0].quote.as_mut().unwrap().ts = Some(now);
        b.poll_s = 3.0;
        let mut cfg = cfg_with(fast_alert(1));
        assert!(check_with(&mut cfg, &b, now, &|_| hist.clone()).is_empty(), "40 s apart is a gap at 3 s polls");
        b.governed_s = 60.0;
        assert_eq!(check_with(&mut cfg, &b, now, &|_| hist.clone()).len(), 1);
    }

    #[test]
    fn fast_move_once_clears_and_crossing_latches_until_it_calms() {
        let now = 10_000.0;
        let hist = samples(now - 699.0, now, 3.0, |_| 100.0);
        let mut b = board(102.0, false);
        b.poll_s = 3.0;
        b.cells[0].quote.as_mut().unwrap().ts = Some(now);
        let mut once = cfg_with(Alert { once: true, ..fast_alert(1) });
        assert_eq!(check_with(&mut once, &b, now, &|_| hist.clone()).len(), 1);
        assert!(once.alerts.is_empty());
        let mut cross = cfg_with(Alert { crossing: true, ..fast_alert(1) });
        assert_eq!(check_with(&mut cross, &b, now, &|_| hist.clone()).len(), 1);
        assert!(check_with(&mut cross, &b, now + 3.0, &|_| hist.clone()).is_empty(), "latched");
        let mut calm = board(100.2, false);
        calm.poll_s = 3.0;
        calm.cells[0].quote.as_mut().unwrap().ts = Some(now + 6.0);
        assert!(check_with(&mut cross, &calm, now + 6.0, &|_| hist.clone()).is_empty());
        let mut again = board(102.0, false);
        again.poll_s = 3.0;
        again.cells[0].quote.as_mut().unwrap().ts = Some(now + 9.0);
        assert_eq!(check_with(&mut cross, &again, now + 9.0, &|_| hist.clone()).len(), 1, "re-armed at half the threshold");
    }

    /// The four repeat modes, each as an alert that WOULD fire on a fresh quote at 4200.
    fn every_mode() -> Vec<(&'static str, Alert)> {
        let hit = |a: Alert| Alert { above: Some(4100.0), ..a };
        vec![
            ("every cooldown", hit(Alert::default())),
            ("once", hit(Alert { once: true, ..Alert::default() })),
            ("each crossing", hit(Alert { crossing: true, ..Alert::default() })),
            ("fast move", Alert { fast: 1, ..Alert::default() }),
        ]
    }

    fn sure_board(price: f64, ts: f64) -> Board {
        let mut b = board(price, false);
        b.poll_s = 3.0;
        b.cells[0].quote.as_mut().unwrap().ts = Some(ts);
        b
    }

    #[test]
    fn in_every_repeat_mode_a_stale_quote_fires_nothing() {
        let now = 10_000.0;
        let hist = samples(now - 700.0, now, 3.0, |_| 4000.0);
        for (name, a) in every_mode() {
            let mut cfg = Config { alerts: vec![Alert { label: "XAU".into(), ..a.clone() }], ..Config::default() };
            // Sanity: with a fresh quote this alert does fire, so the empty results below mean something.
            let fresh = sure_board(4200.0, now);
            assert_eq!(check_with(&mut cfg.clone(), &fresh, now, &|_| hist.clone()).len(), 1, "{name}: fresh");
            // The cell is marked stale.
            let mut marked = sure_board(4200.0, now);
            marked.cells[0].stale = true;
            assert!(check_with(&mut cfg, &marked, now, &|_| hist.clone()).is_empty(), "{name}: stale cell");
            // The quote is older than the stale limit (90 s).
            let old = sure_board(4200.0, now - 120.0);
            assert!(check_with(&mut cfg, &old, now, &|_| hist.clone()).is_empty(), "{name}: old quote");
            // No time at all.
            let mut unknown = sure_board(4200.0, now);
            unknown.cells[0].quote.as_mut().unwrap().ts = None;
            assert!(check_with(&mut cfg, &unknown, now, &|_| hist.clone()).is_empty(), "{name}: no stamp");
            // A fallback quote, even with a fresh stamp and a fresh flag.
            let mut fallback = sure_board(4200.0, now);
            fallback.cells[0].quote.as_mut().unwrap().source = "Gold API";
            assert!(check_with(&mut cfg, &fallback, now, &|_| hist.clone()).is_empty(), "{name}: fallback");
            // And none of that spent the alert: it is still armed and still fires on a fresh quote.
            assert_eq!(check_with(&mut cfg, &fresh, now, &|_| hist.clone()).len(), 1, "{name}: still armed");
        }
    }

    #[test]
    fn the_stale_limit_in_check_follows_the_governed_interval() {
        let now = 10_000.0;
        let mut cfg = cfg_with(alert(Some(4100.0), None));
        cfg.stale_after_s = 90.0;
        // A quote 250 s old at a governed 300 s interval is one poll old: fresh. Ungoverned it is stale.
        let mut b = sure_board(4200.0, now - 250.0);
        assert!(check(&mut cfg, &b, now).is_empty());
        b.governed_s = 300.0;
        assert_eq!(check(&mut cfg, &b, now).len(), 1);
        // Three intervals old is stale however it is governed.
        let b3 = {
            let mut b = sure_board(4200.0, now - 900.0);
            b.governed_s = 300.0;
            b
        };
        cfg.alerts[0].last = 0.0;
        assert!(check(&mut cfg, &b3, now).is_empty());
    }

    #[test]
    fn has_level_and_armed_see_every_kind_of_level() {
        assert!(!has_level(&Alert::default()));
        assert!(has_level(&alert(Some(1.0), None)));
        assert!(has_level(&Alert { day_move: Some(2.0), ..Alert::default() }));
        assert!(has_level(&fast_alert(3)));
        assert!(!has_level(&fast_alert(4)), "an out-of-range fast move is none");
        assert!(!has_level(&alert(Some(f64::NAN), None)));
        assert!(armed(&[Alert::default(), fast_alert(1)]));
        assert!(!armed(&[]));
        assert_eq!(fast_label(0), "1 % in 5 min");
        assert_eq!(fast_label(2), "3 % in 60 min");
    }

    #[test]
    fn the_log_holds_fifty_and_drops_the_oldest_at_fifty_one() {
        let mut log = Log::default();
        for i in 0..50 {
            log.push(i as f64, format!("a{i}"), Outcome::Shown);
        }
        assert_eq!(log.len(), 50);
        assert_eq!(log.newest_first().last().unwrap().title, "a0", "fifty entries keep the first");
        log.push(50.0, "a50".into(), Outcome::Shown);
        assert_eq!(log.len(), 50);
        let all = log.newest_first();
        assert_eq!(all.first().unwrap().title, "a50");
        assert_eq!(all.last().unwrap().title, "a1", "the fifty-first pushed the oldest out");
    }

    #[test]
    fn the_new_outcomes_read_as_words_and_a_gap_is_its_own_line() {
        let clock = clock_at(13 * 60 + 30, 1000.0);
        let e = |outcome, title: &str| Entry { seq: 1, at: 1000.0 + 3600.0, title: title.into(), outcome };
        assert_eq!(entry_line(&e(Outcome::SentOnRetry, "XAU above 4,200.00"), &clock, 1000.0), "14:30  XAU above 4,200.00 \u{b7} sent on retry");
        let gap = "02:10 to 02:31 not checked (no fresh price)";
        assert_eq!(entry_line(&e(Outcome::Gap, gap), &clock, 1000.0), gap);
    }

    #[test]
    fn the_digest_counts_what_a_silence_swallowed_and_nothing_else() {
        let e = |at: f64, outcome| Entry { seq: 1, at, title: "XAU above 1".into(), outcome };
        assert_eq!(digest(&[], 0.0), None, "none for zero");
        // Delivered alerts, gap lines and anything before the silence began are not counted.
        let log = vec![e(5.0, Outcome::Shown), e(6.0, Outcome::Gap), e(7.0, Outcome::Sent), e(1.0, Outcome::Muted)];
        assert_eq!(digest(&log, 2.0), None);
        let log = vec![e(10.0, Outcome::Muted), e(11.0, Outcome::Muted), e(12.0, Outcome::Muted), e(1.0, Outcome::Muted)];
        assert_eq!(digest(&log, 2.0).unwrap(), "3 alerts while muted. See Alerts > Recent.");
        assert_eq!(digest(&[e(10.0, Outcome::Quiet)], 2.0).unwrap(), "1 alert while in quiet hours. See Alerts > Recent.");
        assert!(digest(&[e(2.0, Outcome::Muted)], 2.0).is_some(), "an alert at the very tick counts");
    }

    #[test]
    fn the_recent_list_copies_as_one_line_per_entry() {
        let clock = clock_at(13 * 60 + 30, 1000.0);
        let log = vec![
            Entry { seq: 2, at: 1000.0 + 3600.0, title: "B".into(), outcome: Outcome::Shown },
            Entry { seq: 1, at: 1000.0, title: "A".into(), outcome: Outcome::Muted },
        ];
        assert_eq!(log_text(&log, &clock, 1000.0), "14:30  B \u{b7} shown\r\n13:30  A \u{b7} muted");
        assert_eq!(log_text(&[], &clock, 1000.0), "");
    }

    fn overview_cfg(alerts: Vec<Alert>) -> Config {
        let rows = vec![crate::config::Row { label: "XAU".into(), symbol: "XAU=".into(), digits: 2, name: String::new(), strip: true }];
        Config { alerts, rows, stale_after_s: 1_000_000.0, ..Config::default() }
    }

    #[test]
    fn the_overview_says_how_far_each_level_is() {
        let cfg = overview_cfg(vec![
            Alert { label: "XAU".into(), above: Some(4200.0), below: Some(3900.0), day_move: Some(2.0), fast: 1, ..Alert::default() },
            Alert { label: "XAU".into(), ..Alert::default() },
        ]);
        let mut b = board(4150.0, false);
        b.cells[0].quote.as_mut().unwrap().change_pct = Some(0.4);
        let lines: Vec<String> = overview(&cfg, &b, 100.0).into_iter().map(|l| l.text).collect();
        assert_eq!(
            lines,
            vec![
                "XAU above 4,200.00, 1.2 % away",
                "XAU below 3,900.00, 6.0 % away",
                "XAU day move \u{b1}2 %, now +0.4 %",
                "XAU fast move 1 % in 5 min",
            ],
            "an alert with nothing set is not listed"
        );
        // Past the level: say so rather than a negative distance.
        let past = overview(&cfg, &board(4300.0, false), 100.0);
        assert_eq!(past[0].text, "XAU above 4,200.00, reached, 2.3 % past");
        assert!(overview(&overview_cfg(vec![]), &b, 100.0).is_empty());
    }

    #[test]
    fn the_overview_warns_about_a_level_whose_symbol_is_not_on_the_strip() {
        let cfg = overview_cfg(vec![
            Alert { label: "XAU".into(), above: Some(4200.0), ..Alert::default() },
            Alert { label: "BTC".into(), below: Some(70_000.0), ..Alert::default() },
        ]);
        let lines = overview(&cfg, &board(4150.0, false), 100.0);
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].warn);
        assert!(lines[1].warn);
        assert_eq!(lines[1].text, "BTC below 70,000, no fresh price (not on the strip, cannot fire)", "digits come from the catalogue (BTC shows none) when the symbol is not on the strip");
    }

    #[test]
    fn the_overview_never_guesses_a_distance_from_a_stale_or_fallback_price() {
        let cfg = overview_cfg(vec![Alert { label: "XAU".into(), above: Some(4200.0), ..Alert::default() }]);
        assert_eq!(overview(&cfg, &board(4150.0, true), 100.0)[0].text, "XAU above 4,200.00, no fresh price");
        let mut fb = board(4150.0, false);
        fb.cells[0].quote.as_mut().unwrap().source = "Gold API";
        assert_eq!(overview(&cfg, &fb, 100.0)[0].text, "XAU above 4,200.00, no fresh price");
        assert_eq!(overview(&cfg, &board(f64::NAN, false), 100.0)[0].text, "XAU above 4,200.00, no fresh price");
    }

    #[test]
    fn describe_names_a_fast_move_and_the_crossing_mode() {
        let a = Alert { above: Some(4200.0), fast: 2, crossing: true, ..Alert::default() };
        assert_eq!(describe(&a, 2), "above 4,200.00 \u{b7} fast 2 % in 15 min \u{b7} each crossing");
        let once = Alert { once: true, ..a };
        assert!(describe(&once, 2).ends_with("\u{b7} once"));
        assert_eq!(describe(&Alert { fast: 9, ..Alert::default() }, 2), "nothing set");
    }

    #[test]
    fn quitting_waits_for_a_post_in_flight_but_never_past_the_cap() {
        let idle = AtomicUsize::new(0);
        let t = Instant::now();
        assert!(wait_idle(&idle, Duration::from_secs(2)));
        assert!(t.elapsed() < Duration::from_millis(100), "nothing in flight: no wait");
        // A post that finishes while we wait ends the wait early.
        let busy = std::sync::Arc::new(AtomicUsize::new(1));
        let b = busy.clone();
        let h = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            b.fetch_sub(1, Ordering::SeqCst);
        });
        let t = Instant::now();
        assert!(wait_idle(&busy, Duration::from_secs(2)));
        assert!(t.elapsed() >= Duration::from_millis(100) && t.elapsed() < Duration::from_secs(1));
        h.join().unwrap();
        // One that never finishes costs exactly the cap and no more.
        let stuck = AtomicUsize::new(1);
        let t = Instant::now();
        assert!(!wait_idle(&stuck, Duration::from_millis(200)));
        assert!(t.elapsed() >= Duration::from_millis(200) && t.elapsed() < Duration::from_millis(600));
        assert_eq!(QUIT_WAIT, Duration::from_secs(2));
    }

    #[test]
    fn checkable_is_armed_on_a_shown_symbol_and_fresh_when_one_could_be_checked() {
        let cfg = overview_cfg(vec![Alert { label: "XAU".into(), above: Some(1.0), ..Alert::default() }]);
        assert_eq!(checkable(&cfg, &board(4150.0, false)), (true, true));
        assert_eq!(checkable(&cfg, &board(4150.0, true)), (true, false), "stale: armed but blind");
        let mut fb = board(4150.0, false);
        fb.cells[0].quote.as_mut().unwrap().source = "Gold API";
        assert_eq!(checkable(&cfg, &fb), (true, false), "a fallback price checks nothing");
        let hidden = overview_cfg(vec![Alert { label: "BTC".into(), above: Some(1.0), ..Alert::default() }]);
        assert_eq!(checkable(&hidden, &board(4150.0, false)), (false, false), "a hidden symbol is not a gap");
        assert_eq!(checkable(&overview_cfg(vec![]), &board(1.0, false)), (false, false));
        // One fresh symbol among stale ones is enough: the alerts that can be checked are being checked.
        let mut two = overview_cfg(vec![
            Alert { label: "XAU".into(), above: Some(1.0), ..Alert::default() },
            Alert { label: "XAG".into(), above: Some(1.0), ..Alert::default() },
        ]);
        two.rows.push(crate::config::Row { label: "XAG".into(), symbol: "XAG=".into(), digits: 3, name: String::new(), strip: true });
        assert_eq!(checkable(&two, &board(4150.0, false)), (true, true));
    }

    #[test]
    fn a_preset_and_the_fire_path_agree_on_what_is_fresh() {
        let now = 10_000.0;
        let cfg = Config { alerts: vec![alert(Some(4100.0), None)], stale_after_s: 90.0, ..Config::default() };
        for (age, governed, fresh) in [(60.0, 0.0, true), (250.0, 0.0, false), (250.0, 300.0, true), (700.0, 300.0, false)] {
            let mut b = sure_board(4200.0, now - age);
            b.governed_s = governed;
            let sets = reference_price(&b.cells[0], &cfg, governed, now).is_some();
            let fires = !check(&mut cfg.clone(), &b, now).is_empty();
            assert_eq!((sets, fires), (fresh, fresh), "age {age}, governed {governed}");
        }
    }
}
