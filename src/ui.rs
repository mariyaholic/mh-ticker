//! The strip (borderless, layered, always-on-top popup drawn with GDI) and the tray icon.
//! Plain Win32: no GUI toolkit, no GPU context, which is what keeps the process to a few MB.
//!
//! This module is the whole engine: both builds link it. What differs between them is data, not
//! code paths — `Ui::profile` decides the config file, the mutex and the window title, and
//! `Ui::theme` decides the palette. The classic build has one theme and no picker, so its menus
//! simply have fewer entries and its panel draws no dead buttons.
//!
//! Click the docked strip (double-click when free) and it morphs into a centred panel with a card
//! per symbol (`panel.rs`); the minimise button or Esc morphs it back. The morph stretches and
//! crossfades snapshots of both states while the window glides, paced by `DwmFlush` (vsync).
//!
//! Placements (right-click -> Position, `config::Placement`):
//! - **Auto** (default): a compact grid in a taskbar gap. The gap left of the Start button while
//!   it is roomy (centred icons), else the gap between the app icons and the tray (left-aligned
//!   icons, a crowded centre), else the bottom-right corner above the taskbar (auto-hide or
//!   vertical taskbar). `dock.rs` tracks the gaps.
//! - **Taskbar, left of Start** / **next to the tray**: the same grid, in that gap only.
//! - **Four corners**: one row in that corner of the work area.
//! - **Floating**: one row anywhere on screen, dragged by hand, position saved.
//!
//! On the taskbar the strip re-raises itself when the taskbar comes on top. Every placement hides
//! while a fullscreen app (a game) is in front. With several displays, *Position* also picks one.
//!
//! Re-entrancy: TrackPopupMenu, SetWindowPos and ShowWindow pump or send messages back into
//! `wndproc`, so the `UI` RefCell is never borrowed across those calls.

use crate::alerts;
use crate::autostart;
use crate::chartview;
use crate::find;
use crate::hotkey;
use crate::keys;
use crate::undo;
use crate::update;
use crate::watch;
use crate::probe;
use crate::prompt;
use crate::report;
use crate::config::{self, Config, Placement, Profile};
use crate::dock;
use crate::panel;
use crate::ranges::{self, Range};
use crate::theme::{self, Gradient, Theme};
use crate::model::{fmt_change, fmt_price, Board, Cell, Link};
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, RegisterHotKey, UnregisterHotKey, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_PRIOR, VK_NEXT, VK_HOME, VK_END};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_FEED: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
/// The free taskbar span moved (probe thread), or the display/taskbar changed.
const WM_DOCK: u32 = WM_APP + 3;
/// The foreground window changed (WinEvent hook).
const WM_FGCHANGE: u32 = WM_APP + 4;
/// A webhook post finished off-thread (`wparam` 1 = failed). Kept as a message because the posting
/// thread cannot touch `Ui` (it is thread-local).
const WM_ALERT: u32 = WM_APP + 5;
const TRAY_ID: u32 = 1;
const TIMER_DOCK: usize = 1;
/// Repaint pace for an animated background, only while the panel is open. 12 fps is enough for a
/// slow drift and keeps the cost of a moving background in the same class as a hover repaint.
const TIMER_ANIM: usize = 2;
/// One-shot re-check soon after a foreground change: a taskbar click raises the taskbar a moment
/// after the foreground event, so the immediate check can miss it.
const TIMER_RECHECK: usize = 3;
const CMD_STRIP: usize = 1001;
const CMD_TOP: usize = 1002;
const CMD_STATUS: usize = 1003;
const CMD_FOLDER: usize = 1004;
const CMD_EXIT: usize = 1005;
const CMD_PANEL: usize = 1007;
const CMD_RESET: usize = 1008;
const CMD_FALLBACK: usize = 1009;
/// Position submenu: CMD_POS + index into `Placement::ALL`.
const CMD_POS: usize = 4000;
/// Poll interval: CMD_POLL + index into `POLL_CHOICES`.
const CMD_POLL: usize = 5000;
/// Theme: CMD_THEME + index into `theme::ALL` (Ultra only).
const CMD_THEME: usize = 5100;
/// Font: CMD_FONT + index into `theme::FACES` (Ultra only).
const CMD_FONT: usize = 5150;
/// The Ultra symbol picker open/closed toggle.
const CMD_PICK: usize = 5800;
/// Alerts submenu, fixed entries (Ultra only): 5200..=5299 is this range's whole allotment.
/// The per-symbol presets are dynamic (`Action::Alert`), indexing `alerts::PRESETS`.
const CMD_ALERT_ONCE: usize = 5200;
const CMD_MUTE_1H: usize = 5201;
const CMD_MUTE_8H: usize = 5202;
const CMD_MUTE_INDEF: usize = 5203;
const CMD_UNMUTE: usize = 5204;
const CMD_QUIET_OFF: usize = 5205;
/// Quiet hours: CMD_QUIET + index into `alerts::QUIET_CHOICES`.
const CMD_QUIET: usize = 5206;
const CMD_HOOK_HOST: usize = 5210;
const CMD_HOOK_PASTE: usize = 5211;
const CMD_HOOK_TEST: usize = 5212;
const CMD_HOOK_REMOVE: usize = 5213;
const CMD_LOG_CLEAR: usize = 5214;
/// Repeat mode (W2): *Every cooldown* and *Each crossing* beside *Fire once, then clear* (`CMD_ALERT_ONCE`).
const CMD_REPEAT_COOLDOWN: usize = 5215;
const CMD_REPEAT_CROSSING: usize = 5216;
/// Tell the webhook when alerts could not be checked (default off).
const CMD_GAP_NOTICE: usize = 5217;
/// Copy the Recent list as text.
const CMD_LOG_COPY: usize = 5218;
/// Clear every alert, after asking.
const CMD_ALERTS_CLEAR_ALL: usize = 5219;
/// The last fixed entry of the Alerts submenu (the dispatch range ends here).
const CMD_ALERT_LAST: usize = CMD_ALERTS_CLEAR_ALL;
/// A second "Send a test alert" inside this many seconds is refused (one request in flight).
const TEST_ALERT_GAP_S: u64 = 15;

/// Poll intervals offered in the menu, in seconds. 1 s is aggressive; 60 s is a working-day glance.
const POLL_CHOICES: [f64; 7] = [1.0, 2.0, 3.0, 5.0, 10.0, 30.0, 60.0];
/// Auto takes a taskbar gap at least this wide (96-dpi px); a forced side needs `MIN_FORCED_GAP`.
const MIN_GAP: i32 = 140;
const MIN_FORCED_GAP: i32 = 60;
/// Not exported by windows-sys 0.59's WindowsAndMessaging.
const WM_MOUSELEAVE: u32 = 0x02A3;
const EXPAND_MS: f64 = 230.0;
const COLLAPSE_MS: f64 = 190.0;
/// The window gliding wider for the chart, or back. Shorter than a strip <-> panel morph: the content
/// is the same panel, only the frame changes.
const CHART_MS: f64 = 210.0;
/// The candle pane's crosshair moves in bands this tall (96-dpi px, scaled by DPI), so a mouse move
/// inside one band repaints nothing.
const CHART_Y_BAND: i32 = 2;
/// The `ranges` worker finished a fetch (the result is in `Ui::range_slots`; `wparam` and `lparam`
/// are unused). `WM_APP + 12` is reserved for P4 and unused: the design needs no second message and
/// no timer (the retry and refresh tick is `on_feed`).
const WM_RANGE: u32 = WM_APP + 11;
/// The fallback cycle length, and how often a moving background is advanced. Each palette names its
/// own cycle in `theme::ALL`; this only covers a theme that somehow has none.
const ANIM_CYCLE_S: f64 = 9.0;
const ANIM_MS: u32 = 100;
const TIP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// A second tray click inside this window (a double-click) is ignored.
const TRAY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

// --- P3 "Everyday": reserved menu IDs 5400..5499, WM_APP + 9 and + 10, timer 6. ---
/// The update check finished off-thread (its outcome is in `Ui::update_result`). `WM_APP + 10` and
/// timer 6 are reserved for this package and unused.
const WM_UPDATE: u32 = WM_APP + 9;
/// The one global hotkey's id for `RegisterHotKey` / `WM_HOTKEY`'s `wparam`.
const HOTKEY_ID: i32 = 1;
const CMD_REFRESH: usize = 5400;
const CMD_COPY_ALL: usize = 5401;
const CMD_AUTOSTART: usize = 5402;
const CMD_VERSION: usize = 5403;
const CMD_UPDATE: usize = 5404;
const CMD_SAVE_STATUS: usize = 5405;
/// The greyed line that says a settings file that would not parse was kept under another name.
const CMD_KEPT_STATUS: usize = 5406;
/// The greyed data meter line.
const CMD_METER: usize = 5407;
/// *Copy diagnostics report* (W1): the probe's checks on a click, as text on the clipboard.
const CMD_REPORT: usize = 5408;
/// The report's checks finished off-thread (the text is in `Ui::report_result`). `WM_APP + 16` is
/// free: P7 uses 14 and 15, and nothing else is above 15.
const WM_REPORT: u32 = WM_APP + 16;
/// Hotkey submenu: Off, `hotkey::PRESETS` at `CMD_HOTKEY + 1 + i`, and `CMD_HOTKEY_CUSTOM` for a
/// configured combo that is none of them.
const CMD_HOTKEY: usize = 5410;
const CMD_HOTKEY_CUSTOM: usize = 5415;
/// Strip submenu: rows, text size and opacity, index into `STRIP_ROWS` / `TEXT_STEPS` / `OPACITY_STEPS`.
const CMD_STRIP_ROWS: usize = 5420;
const CMD_STRIP_TEXT: usize = 5430;
const CMD_STRIP_OPACITY: usize = 5440;
/// The poll governor's two ticks (W2), in the Poll every submenu.
const CMD_SLOW_QUIET: usize = 5450;
const CMD_REST_LOCKED: usize = 5451;
/// Symbols > Order the cards (W3 D5): once, by day move, by name, or back to the default order.
const CMD_ORDER_MOVE: usize = 5460;
const CMD_ORDER_NAME: usize = 5461;
const CMD_ORDER_DEFAULT: usize = 5462;
/// Symbols > Show all on strip: pin every card (W3 J1).
const CMD_PIN_ALL: usize = 5463;
/// Edit > Undo the last removal, move, sort, pin change or alert clear (W3 J2).
const CMD_UNDO: usize = 5464;
/// The session was locked or unlocked (`wparam` 7 or 8), sent once `register_session_notice` ran.
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;

/// Rows in the taskbar grid offered by the Strip submenu (`cfg.dock_rows`).
const STRIP_ROWS: [u32; 3] = [1, 2, 3];
/// Text sizes: `(dock_font_pt, font_pt, label)`. The second step is today's default (9, 10).
const TEXT_STEPS: [(i32, i32, &str); 4] = [(8, 9, "Small"), (9, 10, "Default"), (10, 11, "Large"), (11, 12, "Larger")];
/// Opacities: `(cfg.opacity, label)`. The second step is today's default (235).
const OPACITY_STEPS: [(u8, &str); 5] = [(255, "100 %"), (235, "92 %"), (205, "80 %"), (180, "70 %"), (155, "60 %")];

// --- P6 "Type it in": reserved menu IDs 6000..6099, WM_APP + 14 and + 15, timer 9. ---
/// A symbol lookup finished off-thread (its outcome is in `Ui::lookup_result`). `WM_APP + 15` and timer
/// 9 are reserved for this package and unused.
const WM_LOOKUP: u32 = WM_APP + 14;
/// Symbols > *Add symbol...* (both builds).
const CMD_ADD_SYMBOL: usize = 6000;
/// The candle pane's own right-click menu (Ultra): alert at the price under the pointer, clear the
/// symbol's alerts, and the main menu.
const CMD_PANE_ALERT: usize = 6010;
const CMD_PANE_CLEAR: usize = 6011;
const CMD_PANE_MENU: usize = 6012;
/// Characters the *Add symbol* prompt lets you type. More than `config::MAX_SYMBOL`, so spaces around a
/// pasted symbol do not cut it; `clean_symbol` enforces the real limit.
const SYMBOL_PROMPT_LEN: usize = 40;
/// Characters the *at a price* prompt lets you type.
const LEVEL_PROMPT_LEN: usize = 32;

/// Index into `STRIP_ROWS` closest to `rows` (ties go to the lower index; 0 and anything above 3
/// clamp to the ends).
fn nearest_rows(rows: u32) -> usize {
    // Strictly closer wins, so a tie keeps the lower index.
    (1..STRIP_ROWS.len()).fold(0, |best, i| if rows.abs_diff(STRIP_ROWS[i]) < rows.abs_diff(STRIP_ROWS[best]) { i } else { best })
}

/// Index into `TEXT_STEPS` closest to the config's `(dock_font_pt, font_pt)`: the smallest sum of
/// absolute differences of both numbers; ties go to the lower index. The pair (9, 10) is index 1.
fn nearest_text(dock_pt: i32, pt: i32) -> usize {
    let distance = |i: usize| {
        let (d, p, _) = TEXT_STEPS[i];
        (i64::from(dock_pt) - i64::from(d)).abs() + (i64::from(pt) - i64::from(p)).abs()
    };
    (1..TEXT_STEPS.len()).fold(0, |best, i| if distance(i) < distance(best) { i } else { best })
}

/// Index into `OPACITY_STEPS` closest to `opacity` (ties go to the lower index, the more opaque
/// step). 235 is index 1; 0 gives the last index; 255 gives 0.
fn nearest_opacity(opacity: u8) -> usize {
    let distance = |i: usize| (i32::from(opacity) - i32::from(OPACITY_STEPS[i].0)).abs();
    (1..OPACITY_STEPS.len()).fold(0, |best, i| if distance(i) < distance(best) { i } else { best })
}

#[cfg(test)]
mod strip_step_tests {
    use super::*;

    #[test]
    fn rows_pick_the_nearest_step() {
        assert_eq!(nearest_rows(1), 0);
        assert_eq!(nearest_rows(2), 1);
        assert_eq!(nearest_rows(3), 2);
        assert_eq!(nearest_rows(0), 0);
        assert_eq!(nearest_rows(9), 2);
        assert_eq!(nearest_rows(u32::MAX), 2);
        // The default two rows is the middle step, and it is today's dock_rows.
        assert_eq!(STRIP_ROWS[1], config::Config::default().dock_rows);
        assert_eq!(STRIP_ROWS[nearest_rows(config::Config::default().dock_rows)], 2);
    }

    #[test]
    fn text_picks_the_nearest_step_and_defaults_are_the_default() {
        let default = config::Config::default();
        assert_eq!(nearest_text(9, 10), 1);
        assert_eq!((TEXT_STEPS[1].0, TEXT_STEPS[1].1), (default.dock_font_pt, default.font_pt));
        assert_eq!(nearest_text(default.dock_font_pt, default.font_pt), 1);
        assert_eq!(nearest_text(8, 9), 0);
        assert_eq!(nearest_text(11, 12), 3);
        assert_eq!(nearest_text(14, 16), 3, "past the largest step stays at the largest");
        assert_eq!(nearest_text(-5, 0), 0, "below the smallest stays at the smallest");
        // (9, 12) is equally far from steps 1, 2 and 3: the lower index wins.
        assert_eq!(nearest_text(9, 12), 1);
        for (i, &(d, p, _)) in TEXT_STEPS.iter().enumerate() {
            assert_eq!(nearest_text(d, p), i, "step {i} maps to itself");
        }
    }

    #[test]
    fn opacity_picks_the_nearest_step_and_235_is_the_default() {
        assert_eq!(nearest_opacity(config::Config::default().opacity), 1);
        assert_eq!(nearest_opacity(255), 0);
        assert_eq!(nearest_opacity(0), 4);
        assert_eq!(nearest_opacity(220), 1, "235 and 205 are both 15 away: the lower index");
        for (i, &(value, _)) in OPACITY_STEPS.iter().enumerate() {
            assert_eq!(nearest_opacity(value), i, "step {i} maps to itself");
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

static STRIP: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Run {
    x: i32,
    y: i32,
    text: Vec<u16>,
    color: COLORREF,
}

#[derive(Default)]
struct Frame {
    runs: Vec<Run>,
    w: i32,
    h: i32,
    /// Link dot: x, y, diameter.
    dot: (i32, i32, i32),
    /// Each symbol's clickable rectangle, recorded by the layout functions while they place the
    /// text (`keys::slot_at` answers the strip click from these, the painter draws the same runs).
    slots: Vec<keys::Slot>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Strip,
    Panel,
    Morph,
}

/// A running morph: both end states as bitmaps, and the eased progress 0..1.
struct Morph {
    from: HBITMAP,
    from_sz: (i32, i32),
    to: HBITMAP,
    to_sz: (i32, i32),
    t: f64,
}

struct Ui {
    hwnd: HWND,
    profile: Profile,
    font: HFONT,
    bg: HBRUSH,
    /// The palette in force. `grad` and `bg` are derived from it and rebuilt together, so a theme
    /// change can never leave a stale brush or gradient behind.
    theme: &'static Theme,
    /// The face actually in force (`cfg.theme_font` when it names a known face, else the theme's).
    /// The panel fonts are rebuilt when either this or the theme changes.
    face: &'static str,
    grad: Gradient,
    icon: HICON,
    /// What the current tray icon was drawn for: the link colour and the alert marker.
    icon_link: Option<(Link, bool)>,
    dpi: u32,
    cfg: Config,
    board: Arc<Mutex<Board>>,
    view: Board,
    frame: Frame,
    /// False until the free strip has a position (start-up, or just undocked).
    placed: bool,
    /// Hidden because a fullscreen app is in front.
    suppressed: bool,
    taskbar_created: u32,
    mode: Mode,
    morph: Option<Morph>,
    /// Where the strip sits (screen px): the morph's small end.
    strip_rect: RECT,
    /// Last open frame, including a panel not yet manually resized.
    last_panel: Option<RECT>,
    /// Panel fonts, made on first expand.
    fonts: Option<panel::Fonts>,
    hist: panel::History,
    /// The minimise button is hovered.
    hover: bool,
    /// The symbol-picker toggle is hovered.
    pick_hover: bool,
    /// The chart toggle is hovered.
    chart_hover: bool,
    /// The settings button is hovered.
    gear_hover: bool,
    /// The Open button on the locked card is hovered.
    open_hover: bool,
    /// The pointer on a candle of the pane (bar and quantised row), `None` when it is off the plot.
    /// Reset: pointer leaves, chart opens or closes (`set_chart`), the lock moves (`Act::Select`), the
    /// rows change (`apply_rows`), the zone flips (`Act::Zone`), the panel expands or collapses.
    chart_at: Option<chartview::Pointer>,
    /// The layout the pane was last painted with (`Scene::layout`); the pointer is answered from it.
    /// Cleared by the painter whenever the pane names no card, and with every reset of `chart_at`.
    /// While the pane names a card and draws no bars (loading, failed, no data) it holds the range
    /// tabs alone (`bars == 0`): the pointer finds no candle and the zone button's rectangle is empty.
    chart_layout: std::cell::Cell<Option<chartview::Layout>>,
    /// The chart range shown (session only, like the chart flag; starts at 1D). Reset: the chart
    /// closes (`set_chart` back to 1D). Kept across a lock change and a collapse/expand: the range is
    /// the user's choice of how to look, not a property of a card.
    range: Range,
    /// The series cache and the request state machine. Outlives the pane; `apply_rows` makes it
    /// forget the symbols that left the strip. Never touched off the UI thread.
    book: ranges::Book,
    /// The slots shared with the `ranges` worker (see `ranges::Slots`). The only `Ui` field another
    /// thread can reach, and only through the `Arc`.
    range_slots: Arc<Mutex<ranges::Slots>>,
    /// The one undo snapshot (W3 J2): the last removal, move, sort, pin change or alert clear. Memory
    /// only, so a quit forgets it. Replaced by the next such action, emptied by `undo_last`, and dropped
    /// by `apply_rows` when the cards change any other way (a snapshot of the cards would undo that too).
    /// Never holds the webhook (`config::Snapshot` has no field for it).
    undo: undo::Undo<config::Snapshot>,
    /// The picker strip is open (Ultra only).
    pick_open: bool,
    picker_scroll: i32,
    wheel_remainder: i32,
    /// The candle pane is open (Ultra only). It shows the locked card.
    chart: bool,
    /// The locked card, by label (so it survives the symbol list changing under it). A click locks a
    /// card and the candle pane follows it; hovering never changes it, because a chart that moves
    /// while the pointer travels to it cannot be read.
    selected: Option<String>,
    /// `(frame before, frame after)` when opening the chart widened the window. Closing the chart
    /// glides back to the frame it came from, but only while the window is still the width the chart
    /// made it: once the user has dragged an edge, the size is theirs.
    chart_grew: Option<(RECT, RECT)>,
    /// Pointer position on a chart, `None` when it is off every sparkline.
    hover_at: Option<panel::Hover>,
    /// Last alert outcome, shown in the footer so a swallowed balloon is not silent.
    alert_note: Option<String>,
    /// The title of the latest alert nobody has looked at yet: the tray icon carries a marker and
    /// the tooltip leads with it. Set by every fired alert (silenced or not) unless the panel is open
    /// and in front (`marks_tray`); cleared by `expand` and by opening any menu (`clear_pending`).
    alert_pending: Option<String>,
    /// The last 10 alerts and what became of them (memory only). Cleared by its menu entry.
    alert_log: alerts::Log,
    /// Whether alerts could be checked on each feed tick (`watch`): a span that was not is a Recent line.
    watch: watch::Watch,
    /// When a mute or quiet hours began, so their end can say what they swallowed.
    hush: watch::Hush,
    /// When "Send a test alert" last started a POST; a second click inside `TEST_ALERT_GAP_S` is
    /// refused. Time-based on purpose: no result message can leave it stuck.
    test_alert_at: Option<Instant>,
    tracking: bool,
    /// Foreground window before the panel opened; it gets focus back on collapse.
    prev_fg: HWND,
    /// Last tray tooltip refresh.
    tip_at: Option<Instant>,
    tray_click_at: Option<Instant>,
    /// The last `RegisterHotKey` for `cfg.hotkey` failed (another app owns the combo) or the
    /// configured text does not parse. Reset by every `register_hotkey` (start-up and each menu
    /// change); false when no hotkey is configured.
    hotkey_failed: bool,
    /// An update check is in flight. Set by `start_update_check`, cleared by the `WM_UPDATE`
    /// handler in every outcome (answer, failure, failed spawn).
    update_running: bool,
    /// Where the worker leaves its `update::Outcome` before posting `WM_UPDATE`; taken by the handler.
    update_result: Arc<Mutex<Option<update::Outcome>>>,
    /// The diagnostics report's checks are in flight. Set by `start_report`, cleared by the
    /// `WM_REPORT` handler in every outcome (answer, failed spawn); a second click is ignored while set.
    report_running: bool,
    /// Where the worker leaves the finished report and its warning count before posting `WM_REPORT`.
    report_result: Arc<Mutex<Option<(String, usize)>>>,
    /// The symbol *Add symbol...* is looking up while its worker runs: the menu entry then reads
    /// "Adding <symbol>..." and is greyed, and a second add is refused. Set by `add_symbol` just before
    /// the worker starts, cleared by `on_lookup` (the `WM_LOOKUP` handler) in every outcome: found, not
    /// found, failed, and a worker that could not start (which posts `WM_LOOKUP` itself).
    adding: Option<String>,
    /// Where the lookup worker leaves `(symbol, outcome)` before posting `WM_LOOKUP`; taken by the handler.
    lookup_result: Arc<Mutex<Option<(String, crate::source::Lookup)>>>,
    /// The cached back buffer, rebuilt only when the window changes size.
    back: Option<Back>,
    /// When the current background cycle started, so its phase is continuous across repaints.
    anim: Option<Instant>,
    /// False when Windows asks for no animation; every palette then holds phase 0.
    motion: bool,
}

/// The panel's back buffer, kept between frames.
///
/// `paint` used to allocate a window-sized bitmap per repaint. That was survivable while repaints
/// were rare (a poll every 3 s, a hover move), but an animated background repaints ten times a
/// second, and a fresh ~1 MB `CreateCompatibleBitmap` each time is pure GDI churn for a surface that
/// never changes size. One buffer, reused until the window resizes.
struct Back {
    dc: HDC,
    bmp: HBITMAP,
    /// What the DC held before `bmp` was selected into it, restored before the bitmap is freed: a
    /// bitmap that is still selected into a DC cannot be deleted.
    old: HGDIOBJ,
    size: (i32, i32),
}

impl Back {
    fn new(w: i32, h: i32) -> Self {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w.max(1), h.max(1));
            ReleaseDC(null_mut(), screen);
            let old = SelectObject(dc, bmp);
            Self { dc, bmp, old, size: (w, h) }
        }
    }
}

impl Drop for Back {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// How many catalogue entries the picker would draw. The classic build never draws it, so the
/// count only has to be right when the picker exists.
fn catalog_len(u: &Ui) -> usize {
    if u.profile.ultra { config::menu_list(&u.cfg).len() } else { 0 }
}

fn picker_open(u: &Ui) -> bool {
    u.profile.ultra && u.pick_open
}

/// What a click on card `clicked` does to the lock: the same card again unlocks it, any other card
/// takes the lock. A lock that cannot be undone is a trap.
fn toggle_lock(current: Option<&str>, clicked: String) -> Option<String> {
    if current == Some(clicked.as_str()) { None } else { Some(clicked) }
}

/// Every way the lock moves or clears goes through here (a click on a card, the arrow keys, Delete, the
/// card menu's Remove, a click on the strip), so the resets cannot drift apart: the Open button's hover,
/// the chart pointer and the layout it was answered from are forgotten, and the range the pane now needs
/// is asked for. `apply_rows` does the same resets when the rows change under the lock.
fn lock_card(u: &mut Ui, label: Option<String>) {
    u.selected = label;
    u.open_hover = false;
    u.chart_at = None;
    u.chart_layout.set(None);
    want_range(u);
}

/// The card that should be locked after `label` leaves the strip: the one that takes its place (the next,
/// or the previous for the last), `None` if it was the only card.
fn neighbour_of(cells: &[crate::model::Cell], label: &str) -> Option<String> {
    let i = cells.iter().position(|c| c.label == label)?;
    cells.get(i + 1).or_else(|| i.checked_sub(1).and_then(|p| cells.get(p))).map(|c| c.label.clone())
}

/// The frame the window grows to when the chart opens: `want_w` wide, centred on where it was so the
/// panel opens out in both directions instead of lurching to one side, and slid back inside the work
/// area where that would hang off an edge. The height is untouched.
fn grown_rect(before: RECT, want_w: i32, work: RECT) -> RECT {
    let w = want_w.min(work.right - work.left);
    let centre = (before.left + before.right) / 2;
    let x = (centre - w / 2).clamp(work.left, (work.right - w).max(work.left));
    RECT { left: x, top: before.top, right: x + w, bottom: before.bottom }
}

/// The frame to glide back to when the chart closes: the one it grew from, carried by however far the
/// user has moved the window since (so a panel dragged to another corner shrinks where it is, not back
/// to where it started), with the window's current height. `None` if the width is no longer the one the
/// chart made it: the user has resized the window, and the size is theirs.
fn rect_after_chart(current: RECT, grown: RECT, before: RECT) -> Option<RECT> {
    if current.right - current.left != grown.right - grown.left {
        return None;
    }
    let dx = current.left - grown.left;
    Some(RECT { left: before.left + dx, top: current.top, right: before.right + dx, bottom: current.bottom })
}

/// `frame` as it would be if the chart had not widened the window: the compact frame, in the same
/// place the user has the window now. `grew` is the `(before, after)` pair `grow_target` recorded; with
/// none (the chart never widened it, or the user has resized it) the frame is returned as it is.
/// Used wherever a frame outlives the chart (the saved panel rectangle, the one a collapsed panel
/// reopens at), because the chart is not remembered but a frame is: without this a wide frame saved
/// while the chart was open reopens wide with no chart in it, and nothing shrinks it.
fn compact_frame(frame: RECT, grew: Option<(RECT, RECT)>) -> RECT {
    grew.and_then(|(before, grown)| rect_after_chart(frame, grown, before)).unwrap_or(frame)
}

/// Which card stays locked when the rows change underneath it. The lock is by label, so it survives a
/// reorder; but a label that is no longer there must not sit invisibly in the state and light up again
/// if the symbol is re-added later. With the chart open the first card takes over (the pane is never
/// left asking for a card); with it closed there is simply no lock.
fn settle_lock(selected: Option<&str>, labels: &[&str], chart_open: bool) -> Option<String> {
    match selected {
        Some(l) if labels.contains(&l) => Some(l.to_string()),
        _ if chart_open => labels.first().map(|l| l.to_string()),
        _ => None,
    }
}

/// The locked card's row. Ultra only: the classic build has no chart to lock, so a click there still
/// opens CNBC at once. Looked up by label every time, because rows come and go with the Symbols menu.
fn selected_row(u: &Ui) -> Option<usize> {
    if !u.profile.ultra {
        return None;
    }
    let label = u.selected.as_deref()?;
    u.view.cells.iter().position(|c| c.label == label)
}

/// The panel face for a palette. The classic build ignores `theme_font` for the same reason it
/// ignores `theme`: one look, enforced here rather than by convention. A face this machine lacks is
/// refused here too — GDI would otherwise substitute one silently at paint time, which looks like a
/// theme bug rather than a missing font.
fn face_of(theme: &Theme, cfg: &Config, ultra: bool) -> &'static str {
    let over = if ultra { cfg.theme_font.as_deref() } else { None };
    let face = theme::font_of(theme, over);
    if theme::face_available(face) {
        face
    } else {
        theme::font_of(theme, None)
    }
}

/// The palette the config names and the face that goes with it. Resolved together, because a face
/// read off the palette still on screen is the previous theme's.
fn selected_look(cfg: &Config, ultra: bool) -> (&'static Theme, &'static str) {
    let theme = theme::pick(ultra, &cfg.theme);
    (theme, face_of(theme, cfg, ultra))
}

/// The panel's geometry, from one place. `draw` and every hit test call this, so a click lands
/// where the pixels are even while the picker is opening or the symbols change.
/// The panel's geometry. Content-sized unless it is open: an open panel takes its columns from the
/// window's width, so a dragged edge reflows the cards instead of clipping them, and its height from
/// the window's height, so the footer sits at the bottom of the frame.
fn panel_geo(u: &Ui) -> panel::Geo {
    let n = u.view.cells.len();
    let natural = panel::geometry(n, catalog_len(u), u.dpi, picker_open(u));
    if u.mode != Mode::Panel {
        return natural;
    }
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(u.hwnd, &mut rc) };
    if rc.right <= 0 || rc.bottom <= 0 {
        return natural;
    }
    let mut g = panel::geometry_in(rc.right, n, catalog_len(u), u.dpi, picker_open(u), u.chart);
    panel::viewport(&mut g, rc.bottom, u.picker_scroll);
    g
}

/// Snap a crosshair time to 5 s buckets. A 1 px mouse move would otherwise change the readout on
/// every event; over an hour-wide plot 5 s is well under a pixel, so nothing visible is lost.
fn snap(h: panel::Hover) -> panel::Hover {
    panel::Hover { row: h.row, t: (h.t / 5.0).round() * 5.0 }
}

/// Report an unreadable configuration without replacing it or exposing its contents.
pub fn config_error(error: &str) {
    let message = wide(&format!("{error}\n\nThe settings file was not changed. Correct it and start the app again."));
    let title = wide("MH Ticker — settings error");
    unsafe { MessageBoxW(null_mut(), message.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR); }
}

/// False when another instance of *this* build already runs (one strip, one tray icon per
/// profile). The two profiles get different mutexes so they can run side by side.
pub fn single_instance() -> bool {
    let name = wide(&format!("Local\\{}-single_instance", config::profile().id));
    unsafe {
        let h = CreateMutexW(null(), 0, name.as_ptr());
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn make_font(cfg: &Config, dpi: u32) -> HFONT {
    let face = wide(&cfg.font);
    let pt = if cfg.placement().on_taskbar() { cfg.dock_font_pt } else { cfg.font_pt };
    let height = -(pt * dpi as i32 / 72);
    unsafe {
        CreateFontW(
            height, 0, 0, 0, FW_SEMIBOLD as _, 0, 0, 0, DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _,
            CLIP_DEFAULT_PRECIS as _, CLEARTYPE_QUALITY as _, (DEFAULT_PITCH | FF_DONTCARE) as _, face.as_ptr(),
        )
    }
}

/// The tray icon: a small rising chart line in the link colour (green live, amber partly stale,
/// red offline) on a dark rounded tile, so it is easy to spot among the hidden icons. With
/// `pending`, a white disc with a dark rim sits bottom right: it reads against all three colours.
fn make_icon(color: COLORREF, pending: bool) -> HICON {
    unsafe {
        let n = GetSystemMetrics(SM_CXSMICON).max(16);
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let full = RECT { left: 0, top: 0, right: n, bottom: n };
        let radius = n / 3;
        // mask: 1 = transparent, 0 = opaque (the rounded tile)
        let mask = CreateBitmap(n, n, 1, 1, null());
        let old = SelectObject(dc, mask);
        FillRect(dc, &full, GetStockObject(WHITE_BRUSH));
        SelectObject(dc, GetStockObject(BLACK_BRUSH));
        SelectObject(dc, GetStockObject(NULL_PEN));
        RoundRect(dc, 0, 0, n + 1, n + 1, radius, radius);
        let colour = CreateCompatibleBitmap(screen, n, n);
        SelectObject(dc, colour);
        FillRect(dc, &full, GetStockObject(BLACK_BRUSH));
        let tile = CreateSolidBrush(rgb(30, 33, 40));
        SelectObject(dc, tile);
        RoundRect(dc, 0, 0, n + 1, n + 1, radius, radius);
        let f = |a: i32| a * n / 16;
        let line = [POINT { x: f(3), y: f(11) }, POINT { x: f(6), y: f(8) }, POINT { x: f(9), y: f(10) }, POINT { x: f(13), y: f(4) }];
        let pen = CreatePen(PS_SOLID, (n / 8).max(2), color);
        SelectObject(dc, pen);
        Polyline(dc, line.as_ptr(), line.len() as i32);
        if pending {
            let rim = CreatePen(PS_SOLID, 1, rgb(30, 33, 40));
            let disc = CreateSolidBrush(rgb(255, 255, 255));
            SelectObject(dc, rim);
            SelectObject(dc, disc);
            Ellipse(dc, f(8), f(8), f(15), f(15));
            SelectObject(dc, GetStockObject(BLACK_PEN));
            SelectObject(dc, GetStockObject(BLACK_BRUSH));
            DeleteObject(rim);
            DeleteObject(disc);
        }
        SelectObject(dc, old);
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: colour };
        let icon = CreateIconIndirect(&info);
        DeleteObject(pen);
        DeleteObject(tile);
        DeleteObject(mask);
        DeleteObject(colour);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        icon
    }
}

/// (label, price, price colour, change + colour) for one row of the board.
fn cell_texts(c: &Cell, t: &Theme) -> (String, String, COLORREF, Option<(String, COLORREF)>) {
    let label = match &c.quote {
        Some(q) if !q.source.is_empty() => format!("{} ({} ?)", c.label, q.source),
        _ => c.label.clone(),
    };
    match &c.quote {
        Some(q) => {
            let change = q.change_pct.map(|p| {
                let col = if c.stale { t.dim } else if p >= 0.0 { t.up } else { t.down };
                (fmt_change(p), col)
            });
            (label, fmt_price(q.price, c.digits), if c.stale { t.dim } else { t.price }, change)
        }
        None => (label, "--".into(), t.dim, None),
    }
}

fn text_size(dc: HDC, s: &str) -> (i32, i32) {
    let t: Vec<u16> = s.encode_utf16().collect();
    let mut sz = SIZE { cx: 0, cy: 0 };
    unsafe { GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut sz) };
    (sz.cx, sz.cy)
}

fn text_run(x: i32, y: i32, s: &str, color: COLORREF) -> Run {
    Run { x, y, text: s.encode_utf16().collect(), color }
}

/// Free mode: everything on one row.
fn layout_row(dc: HDC, cells: &[Cell], dpi: u32, t: &Theme) -> Frame {
    let s = |v: i32| v * dpi as i32 / 96;
    let (px, py) = (s(10), s(5));
    let (_, line_h) = text_size(dc, "Xg");
    let mut f = Frame::default();
    let mut slots = Vec::new();
    let mut x = px;
    for (i, c) in cells.iter().enumerate() {
        let (label, price, pc, change) = cell_texts(c, t);
        // The symbol's own extent starts after the gap that separates it from the one before.
        let left = x + if i > 0 { text_size(dc, "    ").0 } else { 0 };
        let mut push = |t: String, col: COLORREF| {
            let (w, _) = text_size(dc, &t);
            f.runs.push(text_run(x, py, &t, col));
            x += w;
        };
        if i > 0 {
            push("    ".into(), t.label);
        }
        push(format!("{label} "), t.label);
        push(price, pc);
        if let Some((t, col)) = change {
            push(format!(" {t}"), col);
        }
        slots.push(keys::Slot { index: i, left, top: 0, right: x, bottom: 0 });
    }
    let dot = (line_h / 2).max(4);
    f.h = line_h + 2 * py;
    f.slots = slots.into_iter().map(|s| keys::Slot { bottom: f.h, ..s }).collect();
    f.dot = (x + px, (f.h - dot) / 2, dot);
    f.w = x + px + dot + px;
    f
}

/// Dock mode: the first `k` rows of the board as a column-major grid with `rows` lines,
/// label | right-aligned price | right-aligned change.
fn layout_grid(dc: HDC, cells: &[Cell], dpi: u32, rows: usize, show_change: bool, t: &Theme) -> Frame {
    let s = |v: i32| v * dpi as i32 / 96;
    let (px, py, gap_s, gap_l) = (s(7), s(3), s(5), s(12));
    let (_, line_h) = text_size(dc, "Xg");
    let rows = rows.clamp(1, cells.len().max(1));
    let mut f = Frame::default();
    let mut x = px;
    for (ci, col) in cells.chunks(rows).enumerate() {
        if ci > 0 {
            x += gap_l;
        }
        let texts: Vec<_> = col.iter().map(|c| cell_texts(c, t)).collect();
        let lw = texts.iter().map(|t| text_size(dc, &t.0).0).max().unwrap_or(0);
        let pw = texts.iter().map(|t| text_size(dc, &t.1).0).max().unwrap_or(0);
        let cw = if show_change {
            texts.iter().filter_map(|t| t.3.as_ref().map(|(c, _)| text_size(dc, c).0)).max().unwrap_or(0)
        } else {
            0
        };
        let col_w = lw + gap_s + pw + if cw > 0 { gap_s + cw } else { 0 };
        for (r, (label, price, pc, change)) in texts.iter().enumerate() {
            let y = py + r as i32 * line_h;
            f.slots.push(keys::Slot { index: ci * rows + r, left: x, top: y, right: x + col_w, bottom: y + line_h });
            f.runs.push(text_run(x, y, label, t.label));
            let w = text_size(dc, price).0;
            // Without the change column the price itself carries the day's direction.
            let pc = match change {
                Some((_, col)) if cw == 0 && *pc == t.price => *col,
                _ => *pc,
            };
            f.runs.push(text_run(x + lw + gap_s + pw - w, y, price, pc));
            if let (true, Some((t, col))) = (cw > 0, change) {
                let w = text_size(dc, t).0;
                f.runs.push(text_run(x + col_w - w, y, t, *col));
            }
        }
        x += col_w;
    }
    let dot = (line_h / 3).max(4);
    f.h = rows as i32 * line_h + 2 * py;
    f.dot = (x + gap_s, (f.h - dot) / 2, dot);
    f.w = x + gap_s + dot + px;
    f
}

/// Lay out the strip. Docked with `avail` px: the full grid if it fits, else every symbol without
/// the change column (prices tinted by direction), else the leading symbols only.
fn layout(u: &Ui, avail: Option<i32>) -> Frame {
    unsafe {
        let dc = GetDC(u.hwnd);
        let old = SelectObject(dc, u.font);
        // The strip shows the pinned cards only (`config::strip_rows`; every card when none is pinned, as
        // in 1.8). Pinned by label, because the board's cells and the config rows can differ for an
        // instant. Its slots are numbered by the card's own index (`keys::remap_slots`), so a click
        // finds the right card on the panel.
        let pinned: Vec<bool> = u.view.cells.iter().map(|c| u.cfg.rows.iter().find(|r| r.label == c.label).is_none_or(|r| r.strip)).collect();
        let on_strip = crate::pins::strip_indices(&pinned);
        let shown: Vec<Cell> = on_strip.iter().map(|&i| u.view.cells[i].clone()).collect();
        let cells = &shown;
        let t = u.theme;
        let mut frame = match avail {
            None => layout_row(dc, cells, u.dpi, t),
            Some(avail) => {
                let rows = u.cfg.dock_rows.max(1) as usize;
                let mut pick = None;
                'fit: for k in (1..=cells.len()).rev() {
                    for show_change in [true, false] {
                        let f = layout_grid(dc, &cells[..k], u.dpi, rows, show_change, t);
                        if f.w <= avail {
                            pick = Some(f);
                            break 'fit;
                        }
                    }
                }
                pick.unwrap_or_else(|| layout_grid(dc, &cells[..cells.len().min(1)], u.dpi, rows, false, t))
            }
        };
        SelectObject(dc, old);
        ReleaseDC(u.hwnd, dc);
        frame.slots = keys::remap_slots(std::mem::take(&mut frame.slots), &on_strip);
        frame
    }
}

/// Draw the strip frame at (0, 0).
fn draw_strip(u: &Ui, dc: HDC) {
    unsafe {
        let (w, h) = (u.frame.w.max(1), u.frame.h.max(1));
        FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, u.bg);
        let old_font = SelectObject(dc, u.font);
        SetBkMode(dc, TRANSPARENT as _);
        for r in &u.frame.runs {
            SetTextColor(dc, r.color);
            TextOutW(dc, r.x, r.y, r.text.as_ptr(), r.text.len() as i32);
        }
        let (dx, dy, d) = u.frame.dot;
        let brush = CreateSolidBrush(theme::link_color(u.theme, u.view.link));
        let old_brush = SelectObject(dc, brush);
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        Ellipse(dc, dx, dy, dx + d, dy + d);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_font);
        DeleteObject(brush);
    }
}

/// A picture of the docked strip for the stills: the board laid out as `layout` lays it out when the
/// taskbar gap is `avail` px wide and the strip has `rows` lines (the full grid if it fits, else every
/// symbol without the change column, else the leading symbols), painted as `draw_strip` paints it, in
/// the theme's background and the default strip font. Off-screen; only the `shots` feature has it.
#[cfg(feature = "shots")]
pub(crate) fn strip_still(cells: &[Cell], t: &Theme, dpi: u32, rows: usize, avail: i32) -> (u32, u32, Vec<u8>) {
    let cfg = Config::default();
    let font = make_font(&Config { placement: Some(Placement::TaskbarRight), ..cfg }, dpi);
    let frame = unsafe {
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let old = SelectObject(dc, font);
        let mut pick = None;
        'fit: for k in (1..=cells.len()).rev() {
            for show_change in [true, false] {
                let f = layout_grid(dc, &cells[..k], dpi, rows, show_change, t);
                if f.w <= avail {
                    pick = Some(f);
                    break 'fit;
                }
            }
        }
        let frame = pick.unwrap_or_else(|| layout_grid(dc, &cells[..cells.len().min(1)], dpi, rows, false, t));
        SelectObject(dc, old);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        frame
    };
    let (w, h) = (frame.w.max(1), frame.h.max(1));
    let out = crate::shot::capture_with(w, h, |dc| unsafe {
        let bg = CreateSolidBrush(t.bg);
        FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg);
        DeleteObject(bg);
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as _);
        for r in &frame.runs {
            SetTextColor(dc, r.color);
            TextOutW(dc, r.x, r.y, r.text.as_ptr(), r.text.len() as i32);
        }
        let (dx, dy, d) = frame.dot;
        let brush = CreateSolidBrush(theme::link_color(t, Link::Live));
        let old_brush = SelectObject(dc, brush);
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        Ellipse(dc, dx, dy, dx + d, dy + d);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_font);
        DeleteObject(brush);
    });
    unsafe { DeleteObject(font) };
    out
}

fn draw_panel(u: &mut Ui, dc: HDC, w: i32, h: i32) {
    let Some(f) = &u.fonts else { return };
    // The catalogue is only drawn while the picker is open; building it is a clone of every row, so
    // it is not built for the frames (every hover repaint) that do not show it.
    let open = picker_open(u);
    let list = if open { config::menu_list(&u.cfg) } else { Vec::new() };
    let shown: Vec<bool> = list.iter().map(|r| config::is_shown(&u.cfg, &r.symbol)).collect();
    // The background's loop position. Each palette carries its own cycle length, and the clock only
    // runs in Ultra: the classic build keeps the one look it shipped with, so it holds phase 0. It is
    // also read from the panel being open, so a collapsed strip never carries a moving timer.
    let phase = if clock_runs(u.profile.ultra, u.motion, u.theme) {
        let start = *u.anim.get_or_insert_with(Instant::now);
        (start.elapsed().as_secs_f64() / u.theme.cycle.unwrap_or(ANIM_CYCLE_S)) % 1.0
    } else {
        0.0
    };
    let now = crate::source::now_s();
    // The locked symbol's alert levels, for the pane (Ultra: the classic build has no alerts menu).
    let levels: Vec<f64> = if u.profile.ultra {
        selected_row(u)
            .and_then(|row| u.view.cells.get(row))
            .and_then(|c| u.cfg.alerts.iter().find(|a| a.label == c.label))
            .map(|a| [a.above, a.below].into_iter().flatten().collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // The note on a long range: past its time to live, or dates that are off (`Series::note`).
    let note: String = if u.chart && u.range != Range::D1 {
        selected_row(u)
            .and_then(|row| u.view.cells.get(row))
            .and_then(|c| u.cfg.rows.iter().find(|r| r.label == c.label))
            .and_then(|r| u.range.key(&r.symbol))
            .and_then(|k| u.book.get(&k).map(|sr| sr.note(now, ranges::ttl_of(k.file), if u.cfg.chart_utc { 0 } else { local_offset_s() })))
            .unwrap_or_default()
    } else {
        String::new()
    };
    // A card with a level set carries a dot (Ultra only; with no alerts nothing is drawn).
    let dots: Vec<bool> = if u.profile.ultra && !u.cfg.alerts.is_empty() {
        u.view.cells.iter().map(|c| u.cfg.alerts.iter().any(|a| a.label == c.label && alerts::has_level(a))).collect()
    } else {
        Vec::new()
    };
    let scene = panel::Scene {
        title: u.profile.title(),
        board: &u.view,
        hist: &u.hist,
        fonts: f,
        theme: u.theme,
        grad: &u.grad,
        dpi: u.dpi,
        now,
        btn_hover: u.hover,
        pick_hover: u.pick_hover,
        chart_hover: u.chart_hover,
        gear_hover: u.gear_hover,
        select: u.profile.ultra,
        keys_hint: u.profile.ultra,
        selected: selected_row(u),
        open_hover: u.open_hover,
        hover: u.hover_at,
        chart_at: u.chart_at,
        layout: &u.chart_layout,
        picker: open.then(|| panel::Picker { list: &list, shown: &shown }),
        picker_scroll: u.picker_scroll,
        picker_btn: u.profile.ultra,
        chart_btn: u.profile.ultra,
        settings_btn: u.profile.ultra,
        alert_dots: &dots,
        // The chart shows the locked card and nothing else. It used to follow the pointer, which made
        // the pane change under the cursor on the way to it; a click is the one thing that moves it now.
        // With nothing locked the pane says so instead of guessing which card was meant.
        chart: u.chart.then(|| match selected_row(u) {
            Some(row) => {
                let fallback = u.view.cells.get(row).and_then(|c| c.quote.as_ref()).is_some_and(|q| !q.source.is_empty());
                let quote = u.view.cells.get(row).and_then(|c| c.quote.as_ref());
                let range = u.range;
                // The long ranges come from the cache: the series trimmed to the range's span, or the
                // reason there is none yet. 1D is the board's own day, as it always was.
                let long: Option<(&[crate::model::Bar], f64, f64, ranges::Status)> = u
                    .view
                    .cells
                    .get(row)
                    .and_then(|c| u.cfg.rows.iter().find(|r| r.label == c.label))
                    .and_then(|r| range.key(&r.symbol))
                    .map(|k| match u.book.get(&k) {
                        Some(sr) => {
                            let w = sr.window(now, range.span_s());
                            (w, sr.t0, sr.bar_s, if w.is_empty() { ranges::Status::Empty } else { ranges::Status::Ready })
                        }
                        None => (&[][..], 0.0, 0.0, u.book.status(&k)),
                    });
                let day = range == Range::D1;
                let (bars, t0, bar_s, status) = match long {
                    Some(l) if !day => l,
                    // A long range for a card with no configured row has nothing to show yet.
                    _ if !day => (&[][..], 0.0, 0.0, ranges::Status::Loading),
                    _ => (u.view.bars.get(row).map(|b| &b[..]).unwrap_or(&[]), u.view.bars_t0.get(row).copied().unwrap_or(0.0), 60.0, ranges::Status::Ready),
                };
                panel::Chart {
                    zone: if u.cfg.chart_utc || bars.is_empty() {
                        chartview::Zone::fixed(if u.cfg.chart_utc { 0 } else { local_offset_s() })
                    } else {
                        let end = |b: &crate::model::Bar| t0 + b.dt as f64;
                        chartview::Zone::spanning(end(&bars[0]), end(&bars[bars.len() - 1]), &local_offset_at)
                    },
                    tz_label: chartview::zone_label(u.cfg.chart_utc),
                    note: &note,
                    // Today's references and today's change belong to 1D; a long range is coloured by
                    // its own first open and last close.
                    prev: chartview::positive(quote.and_then(|q| q.prev_close)).filter(|_| day),
                    day_hi: chartview::positive(quote.and_then(|q| q.high)).filter(|_| day),
                    day_lo: chartview::positive(quote.and_then(|q| q.low)).filter(|_| day),
                    change_pct: quote.and_then(|q| q.change_pct).filter(|_| day),
                    levels: &levels,
                    unavailable: fallback,
                    range,
                    status,
                    bar_s,
                    tabs: true,
                    label: u.view.cells.get(row).map(|c| c.label.as_str()).unwrap_or(""),
                    digits: u.view.cells.get(row).map(|c| c.digits).unwrap_or(2),
                    t0,
                    bars: if fallback { &[] } else { bars },
                    last: u.view.cells.get(row).and_then(|c| c.quote.as_ref()).map(|q| q.price),
                }
            }
            None => panel::Chart {
                label: "",
                digits: 2,
                t0: 0.0,
                bars: &[],
                last: None,
                unavailable: false,
                range: Range::D1,
                status: ranges::Status::Ready,
                bar_s: 60.0,
                tabs: false,
                zone: chartview::Zone::fixed(0),
                tz_label: "UTC",
                note: "",
                prev: None,
                day_hi: None,
                day_lo: None,
                change_pct: None,
                levels: &[],
            },
        }),
        alert: u.alert_note.as_deref(),
        phase,
    };
    panel::draw(dc, w, h, &scene);
}

/// Seconds east of UTC the machine's clock runs on right now (daylight time included): the pane's
/// "local". 0 if Windows cannot say.
fn local_offset_s() -> i32 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    offset_from_bias(id, tz.Bias, tz.StandardBias, tz.DaylightBias)
}

/// Seconds east of UTC the machine's clock ran on at the UTC instant `t` (UTC seconds): the offset
/// in force then, daylight time included, not today's. Windows converts the instant with the zone's
/// own rules (`SystemTimeToTzSpecificLocalTime`); the difference of the two readings is the offset.
/// Falls back to [`local_offset_s`] when Windows cannot convert it.
fn local_offset_at(t: f64) -> i32 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    if !t.is_finite() || !(0.0..=253_402_300_799.0).contains(&t) {
        return local_offset_s();
    }
    let ticks = ((t + 11_644_473_600.0) * 10_000_000.0) as u64;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let (mut utc, mut local): (SYSTEMTIME, SYSTEMTIME) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    let ok = unsafe { FileTimeToSystemTime(&ft, &mut utc) != 0 && SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) != 0 };
    if !ok {
        return local_offset_s();
    }
    let secs = |s: &SYSTEMTIME| chartview::civil_secs(s.wYear as i64, s.wMonth as u32, s.wDay as u32, s.wHour as u32, s.wMinute as u32, s.wSecond as u32);
    (secs(&local) - secs(&utc)) as i32
}

/// The offset east of UTC from `GetTimeZoneInformation`'s answer. `id` 2 (`TIME_ZONE_ID_DAYLIGHT`)
/// means daylight time is in force; 0 and 1 are standard time (unknown / standard); anything else
/// (`TIME_ZONE_ID_INVALID`) is "cannot say" and gives 0. Biases are minutes WEST of UTC.
fn offset_from_bias(id: u32, bias: i32, standard_bias: i32, daylight_bias: i32) -> i32 {
    match id {
        0 | 1 => -(bias + standard_bias) * 60,
        2 => -(bias + daylight_bias) * 60,
        _ => 0,
    }
}

#[cfg(test)]
mod offset_tests {
    use super::{local_offset_at, local_offset_s, offset_from_bias};

    #[test]
    fn the_offset_at_an_instant_agrees_with_the_offset_now() {
        // Not a daylight-saving test (the machine's zone is not under the test's control): the
        // conversion call must give what `GetTimeZoneInformation` gives for the present instant,
        // and must not panic for instants far out or not numbers.
        let now = crate::source::now_s();
        assert_eq!(local_offset_at(now), local_offset_s());
        for t in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e12] {
            let _ = local_offset_at(t);
        }
    }

    #[test]
    fn the_bias_becomes_seconds_east_with_daylight_time_applied() {
        // New York: 300 minutes west, daylight bias -60.
        assert_eq!(offset_from_bias(1, 300, 0, -60), -5 * 3_600);
        assert_eq!(offset_from_bias(2, 300, 0, -60), -4 * 3_600);
        // India has no daylight time: +05:30.
        assert_eq!(offset_from_bias(0, -330, 0, 0), 19_800);
        // Newfoundland in summer: -02:30.
        assert_eq!(offset_from_bias(2, 210, 0, -60), -9_000);
        // TIME_ZONE_ID_INVALID is 0xFFFFFFFF: say nothing rather than guess.
        assert_eq!(offset_from_bias(u32::MAX, 300, 0, -60), 0);
    }
}

/// Render into a fresh w x h bitmap compatible with the window.
fn snapshot(hwnd: HWND, w: i32, h: i32, draw: impl FnOnce(HDC)) -> HBITMAP {
    unsafe {
        let screen = GetDC(hwnd);
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w.max(1), h.max(1));
        let old = SelectObject(mem, bmp);
        draw(mem);
        SelectObject(mem, old);
        DeleteDC(mem);
        ReleaseDC(hwnd, screen);
        bmp
    }
}

/// One morph frame: the old state stretched to the current size, the new state stretched on top
/// with a crossfade that runs over the middle of the move.
fn draw_morph(m: &Morph, dc: HDC, w: i32, h: i32) {
    unsafe {
        let src = CreateCompatibleDC(dc);
        SetStretchBltMode(dc, HALFTONE as _);
        SetBrushOrgEx(dc, 0, 0, null_mut());
        let old = SelectObject(src, m.from);
        StretchBlt(dc, 0, 0, w, h, src, 0, 0, m.from_sz.0, m.from_sz.1, SRCCOPY);
        let alpha = ((m.t - 0.1) / 0.7).clamp(0.0, 1.0);
        SelectObject(src, m.to);
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: (alpha * 255.0) as u8, AlphaFormat: 0 };
        AlphaBlend(dc, 0, 0, w, h, src, 0, 0, m.to_sz.0, m.to_sz.1, blend);
        SelectObject(src, old);
        DeleteDC(src);
    }
}

/// The hover state a mouse move can change, as the painter reads it.
fn hot_of(u: &Ui) -> panel::Hot {
    panel::Hot { btn: u.hover, pick: u.pick_hover, chart_btn: u.chart_hover, gear: u.gear_hover, open: u.open_hover, aim: u.chart_at, card: u.hover_at }
}

/// The rectangle a hover change from `was` to the state now in `u` has to repaint, or `None` for the
/// whole window. A moving background is drawn from the clock at paint time, so two partial paints of
/// it would not match along their edge: it always repaints whole (it repaints every 100 ms anyway).
fn hover_dirty(u: &Ui, g: &panel::Geo, was: &panel::Hot) -> Option<RECT> {
    if clock_runs(u.profile.ultra, u.motion, u.theme) {
        return None;
    }
    let pane = u.chart_layout.get().map(|l| RECT { left: l.pane.left, top: l.pane.top, right: l.pane.right, bottom: l.pane.bottom });
    panel::hot_dirty(g, u.dpi, selected_row(u), pane, was, &hot_of(u))
}

fn paint(u: &mut Ui) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(u.hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(u.hwnd, &mut rc);
        let (w, h) = (rc.right.max(1), rc.bottom.max(1));
        let fresh = !matches!(&u.back, Some(b) if b.size == (w, h));
        if fresh {
            u.back = Some(Back::new(w, h));
        }
        // Only the invalid part is drawn and copied: a hover invalidates the rectangle it changed
        // (`panel::hot_dirty`), and Windows hands a partly uncovered window the uncovered part. A new
        // buffer holds nothing yet, so it is drawn whole; so is an empty or odd rectangle.
        let r = ps.rcPaint;
        let part = if fresh || r.right <= r.left || r.bottom <= r.top { RECT { left: 0, top: 0, right: w, bottom: h } } else { r };
        // Take the buffer out of `u` so the painter can borrow `u` immutably while drawing into it.
        if let Some(back) = u.back.take() {
            IntersectClipRect(back.dc, part.left, part.top, part.right, part.bottom);
            match (u.mode, &u.morph) {
                (Mode::Morph, Some(m)) => draw_morph(m, back.dc, w, h),
                (Mode::Panel, _) => draw_panel(u, back.dc, w, h),
                _ => draw_strip(u, back.dc),
            }
            BitBlt(hdc, part.left, part.top, part.right - part.left, part.bottom - part.top, back.dc, part.left, part.top, SRCCOPY);
            SelectClipRgn(back.dc, null_mut());
            u.back = Some(back);
        }
        EndPaint(u.hwnd, &ps);
    }
}

/// The tray tooltip. A pending alert leads: `Alert: XAU above 4,100.00`.
fn tooltip(view: &Board, pending: Option<&str>) -> String {
    tooltip_with(view, pending, None)
}

/// [`tooltip`] with one more line while alerts cannot be checked, just under the title so the tray's
/// length limit cuts prices rather than the warning.
fn tooltip_with(view: &Board, pending: Option<&str>, blind: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(title) = pending {
        s.push_str(&format!("Alert: {title}\n"));
    }
    s.push_str(&format!("{} - {}", config::profile().title(), theme::link_name(view.link)));
    if let Some(line) = blind {
        s.push('\n');
        s.push_str(line);
    }
    for c in &view.cells {
        if let Some(q) = &c.quote {
            s.push_str(&format!("\n{} {}", c.label, fmt_price(q.price, c.digits)));
            if !q.source.is_empty() { s.push_str(&format!(" ({}; age unknown)", q.source)); }
            else if c.stale { s.push_str(" (stale)"); }
        }
    }
    s
}

#[cfg(test)]
mod tooltip_tests {
    use super::*;

    #[test]
    fn a_pending_alert_leads_the_tooltip_and_nothing_else_changes() {
        let b = Board::new(&[]);
        let plain = tooltip(&b, None);
        assert!(!plain.contains("Alert"));
        let lit = tooltip(&b, Some("XAU above 4,100.00"));
        assert_eq!(lit.lines().next(), Some("Alert: XAU above 4,100.00"));
        assert_eq!(lit.strip_prefix("Alert: XAU above 4,100.00\n"), Some(plain.as_str()));
    }
}

/// The tray tooltip as the window shows it now: a pending alert, and the gap line while one lasts.
fn tooltip_for(u: &Ui) -> String {
    let blind = u
        .watch
        .blind_since()
        .map(|since| watch::blind_tip(&alerts::local_hhmm(&local_offset_at, crate::source::now_s(), since)));
    tooltip_with(&u.view, u.alert_pending.as_deref(), blind.as_deref())
}

/// Rebuild the tray icon when the link colour or the alert marker differs from what it was drawn
/// for. True when it was rebuilt (the caller then pushes it to the tray).
fn sync_icon(u: &mut Ui) -> bool {
    let key = (u.view.link, u.alert_pending.is_some());
    if u.icon_link == Some(key) {
        return false;
    }
    let fresh = make_icon(theme::link_color(u.theme, key.0), key.1);
    if !u.icon.is_null() {
        unsafe { DestroyIcon(u.icon) };
    }
    u.icon = fresh;
    u.icon_link = Some(key);
    true
}

/// Push the icon and tooltip as they now stand to the tray.
fn refresh_tray(hwnd: HWND) {
    let Some((icon, tip)) = with_ui(|u| {
        sync_icon(u);
        u.tip_at = Some(Instant::now());
        (u.icon, tooltip_for(u))
    }) else {
        return;
    };
    tray(hwnd, NIM_MODIFY, icon, &tip);
}

/// The user is looking: drop the alert marker and the tooltip line. Called when the panel expands
/// and when any menu opens. Must not be called from inside `with_ui`.
fn clear_pending(hwnd: HWND) {
    if with_ui(|u| u.alert_pending.take().is_some()).unwrap_or(false) {
        refresh_tray(hwnd);
    }
}

/// Minutes since local midnight, for quiet hours and clock labels in the menu.
fn local_minute() -> u16 {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    (st.wHour % 24) * 60 + st.wMinute % 60
}

fn tray(hwnd: HWND, msg: u32, icon: HICON, tip: &str) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
            *dst = src;
        }
        Shell_NotifyIconW(msg, &nid);
    }
}

fn tray_delete(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ID;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Default free-strip spot: top-right of the primary work area. A saved spot that is no longer
/// on any monitor (display unplugged) falls back to the default.
fn start_pos(cfg: &Config, w: i32) -> (i32, i32) {
    unsafe {
        if let (Some(x), Some(y)) = (cfg.x, cfg.y) {
            if !MonitorFromPoint(POINT { x: x + 8, y: y + 8 }, MONITOR_DEFAULTTONULL).is_null() {
                return (x, y);
            }
        }
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0);
        (work.right - w - 16, work.top + 8)
    }
}

/// The primary display's work area (the screen minus the taskbar).
fn primary_work() -> RECT {
    unsafe {
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0);
        work
    }
}

/// `f` in corner `p` of `work`, `m` px in from both edges.
fn corner(work: &RECT, p: Placement, f: &Frame, m: i32) -> RECT {
    let left = matches!(p, Placement::TopLeft | Placement::BottomLeft);
    let top = matches!(p, Placement::TopRight | Placement::TopLeft);
    let x = if left { work.left + m } else { work.right - m - f.w };
    let y = if top { work.top + m } else { work.bottom - m - f.h };
    RECT { left: x, top: y, right: x + f.w, bottom: y + f.h }
}

/// Recompute the strip frame and where it goes (`Placement`):
/// - taskbar placements: a compact grid in a taskbar gap (`dock.rs`). With no usable gap (an
///   auto-hidden or vertical taskbar, or gaps too narrow) the grid sits in the bottom-right
///   corner above the taskbar instead;
/// - corners: one row in that corner of the work area;
/// - floating: one row at its saved spot (right edge kept when it sits on the right half).
fn strip_place(u: &mut Ui) -> RECT {
    let p = u.cfg.placement();
    let s = |v: i32| v * u.dpi as i32 / 96;
    let margin = s(8);
    let mon = dock::monitor(u.cfg.monitor.as_deref());
    let tb = mon.as_ref().map_or(null_mut(), dock::taskbar_on);
    dock::set_target(tb);
    let work = mon.as_ref().map(|m| m.work).unwrap_or_else(primary_work);
    if p.on_taskbar() {
        let bar = dock::rect(tb).filter(|r| r.right - r.left > r.bottom - r.top && !dock::autohide());
        if let Some(r) = bar {
            let (left, right) = dock::spans(tb, &r);
            let gap = match p {
                Placement::TaskbarLeft => left,
                Placement::TaskbarRight => right,
                _ => match dock::choose(left, right, s(MIN_GAP)) {
                    Some(dock::Side::Left) => left,
                    Some(dock::Side::Right) => right,
                    None => None,
                },
            }
            .filter(|(a, b)| b - a >= s(MIN_FORCED_GAP));
            if let Some((a, b)) = gap {
                u.frame = layout(u, Some(b - a - 2 * margin));
                let (x, y) = (a + margin, r.top + (r.bottom - r.top - u.frame.h) / 2);
                return RECT { left: x, top: y, right: x + u.frame.w, bottom: y + u.frame.h };
            }
        }
        u.frame = layout(u, Some((work.right - work.left) / 2));
        return corner(&work, Placement::BottomRight, &u.frame, margin);
    }
    u.frame = layout(u, None);
    if p != Placement::Floating {
        return corner(&work, p, &u.frame, margin);
    }
    let w = u.frame.w;
    let h = u.frame.h;
    let (x, y) = if !u.placed {
        u.placed = true;
        start_pos(&u.cfg, w)
    } else {
        let r = u.strip_rect;
        let right_half = (r.left + r.right) / 2 > (work.left + work.right) / 2;
        (if right_half && r.right > r.left { r.right - w } else { r.left }, r.top)
    };
    RECT { left: x, top: y, right: x + w, bottom: y + h }
}

/// Re-fit and move the strip (no-op while the panel is open or morphing).
fn relayout_and_place(hwnd: HWND) {
    let Some(r) = with_ui(|u| {
        (u.mode == Mode::Strip).then(|| {
            u.strip_rect = strip_place(u);
            u.strip_rect
        })
    })
    .flatten() else {
        return;
    };
    unsafe {
        SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hwnd, null(), 0);
    }
}

/// Start the webhook POST for log entry `seq`. The result comes back as `WM_ALERT` with `wparam`
/// 1 = failed and `lparam` = `seq`, so it updates the right log line. If the thread cannot even
/// start, the failure is posted the same way.
fn post_alert(hwnd: HWND, url: String, title: String, body: String, seq: u64, test: bool) {
    let target = hwnd as isize;
    let started = alerts::spawn_post(url, title, body, Box::new(move |err, retried| unsafe {
        PostMessageW(target as HWND, WM_ALERT, alert_wparam_retry(err.is_some(), test, retried), seq as LPARAM);
    }));
    if !started {
        unsafe { PostMessageW(hwnd, WM_ALERT, alert_wparam(true, test), seq as LPARAM) };
    }
}

/// POST the gap notice (no log line; `WM_ALERT` bit 3 says so, and the note names it).
fn post_notice(hwnd: HWND, url: String, text: String) {
    let target = hwnd as isize;
    let started = alerts::spawn_post(url, "MH Ticker".into(), text, Box::new(move |err, retried| unsafe {
        PostMessageW(target as HWND, WM_ALERT, alert_wparam_retry(err.is_some(), false, retried) | 8, 0);
    }));
    if !started {
        unsafe { PostMessageW(hwnd, WM_ALERT, alert_wparam(true, false) | 8, 0) };
    }
}

/// `WM_ALERT`'s `wparam`: bit 0 = the POST failed, bit 1 = it was the test alert. The test bit travels
/// with the result because the log line it belongs to may be gone by the time it arrives (the list was
/// cleared, or ten newer alerts pushed it out), and the note must still say which it was.
fn alert_wparam(failed: bool, test: bool) -> usize {
    alert_wparam_retry(failed, test, false)
}

/// [`alert_wparam`] with bit 2 = the second try was needed (`sent on retry`, or a failure after it).
fn alert_wparam_retry(failed: bool, test: bool, retried: bool) -> usize {
    failed as usize | (test as usize) << 1 | (retried as usize) << 2
}

/// The footer note for a webhook result decoded from `WM_ALERT`'s `wparam`.
fn webhook_note(wp: usize) -> String {
    let (failed, test, retried) = (wp & 1 != 0, wp & 2 != 0, wp & 4 != 0);
    if wp & 8 != 0 {
        return if failed { "gap notice: webhook failed" } else { "gap notice: webhook sent" }.to_string();
    }
    let what = if test { "test alert" } else { "alert" };
    match (failed, test) {
        (true, true) => "test alert: webhook failed".to_string(),
        (true, false) => "alert kept here: webhook failed".to_string(),
        (false, _) if retried => format!("{what}: webhook sent on retry"),
        (false, _) => format!("{what}: webhook sent"),
    }
}

/// Ask the `ranges` worker for the file the pane needs, if one is due. Call it wherever the pane's
/// answer could change: the range or the lock moved, the chart opened, the panel expanded, the rows
/// changed, and on every feed tick (that is the retry and the refresh: `Book::want` is a no-op unless a
/// fetch is due, so it is cheap). No window calls, no I/O: only a brief lock on the slot.
///
/// Nothing is asked for: off Ultra, with the pane closed or the panel collapsed, on 1D (the board's
/// own day), with no card locked, or while the locked card is on a fallback source (it has no bars).
/// A request this one replaces in the slot (the worker had not taken it) is cancelled in the book, so
/// its key can be asked for again.
fn want_range(u: &mut Ui) {
    if !u.profile.ultra || !u.chart || u.mode != Mode::Panel || u.range == Range::D1 {
        return;
    }
    let Some(cell) = selected_row(u).and_then(|i| u.view.cells.get(i)) else { return };
    if cell.quote.as_ref().is_some_and(|q| !q.source.is_empty()) {
        return;
    }
    let Some(key) = u.cfg.rows.iter().find(|r| r.label == cell.label).and_then(|r| u.range.key(&r.symbol)) else { return };
    if let Some(req) = u.book.want(&key, crate::source::now_s()) {
        let replaced = u.range_slots.lock().ok().and_then(|mut s| s.submit(req));
        if let Some(old) = replaced {
            u.book.cancel(old.generation);
        }
    }
}

/// Hand the poll governor what the UI owns: its two ticks and whether any alert is armed. Called from
/// `save_cfg` (so every settings or alert change gets here) and from `on_feed`. A change that
/// could shorten a governed wait ends it now, through the feed's `kick`. State only: no window call,
/// no I/O.
fn sync_gov(u: &mut Ui) {
    let set = u.cfg.governor(u.profile.ultra);
    let armed = u.profile.ultra && alerts::armed(&u.cfg.alerts);
    if let Ok(mut b) = u.board.lock() {
        let changed = b.gov != set || b.armed != armed;
        b.gov = set;
        b.armed = armed;
        if changed && b.slowed {
            b.kick = b.kick.wrapping_add(1);
        }
    }
}

/// The user is here (expand, the hotkey, an unlock): end a governed wait and put the quiet clock back
/// to the base poll at once, by the same `kick` as *Refresh now*. Nothing happens while both governor
/// ticks are off, so Classic's polling is exactly what it was. Takes the board out of `with_ui` first
/// and bumps it outside.
fn wake_feed() {
    let Some(board) = with_ui(|u| u.board.clone()) else { return };
    if let Ok(mut b) = board.lock() {
        if b.gov.any() {
            b.kick = b.kick.wrapping_add(1);
        }
    };
}

/// `WM_WTSSESSION_CHANGE`: record the lock in the board (state only), then wake the feed on an unlock.
fn session_changed(locked: bool) {
    let Some(board) = with_ui(|u| u.board.clone()) else { return };
    if let Ok(mut b) = board.lock() {
        b.locked = locked;
    }
    if !locked {
        wake_feed();
    }
}

/// Ask Windows to send `WM_WTSSESSION_CHANGE` for this window (this session only).
fn register_session_notice(hwnd: HWND) {
    unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) };
}

/// What one feed tick has to say beyond the alerts that fired.
#[derive(Default)]
struct FeedNotices {
    /// The balloon for the end of a mute or quiet hours.
    digest: Option<String>,
    /// The webhook line for a gap (only when the user asked for it).
    gap: Option<String>,
}

/// Pull the latest board, refresh the tray icon colour and tooltip, redraw. The tray tooltip (an
/// Explorer round trip) refreshes at most every 5 s unless the link colour changed, and a hidden
/// strip is not re-measured.
fn on_feed(hwnd: HWND) {
    let Some((tray_update, fired, hook, save, notices)) = with_ui(|u| {
        if let Ok(b) = u.board.lock() {
            let sources = |board: &Board| board.cells.iter()
                .map(|c| c.quote.as_ref().map(|q| q.source).unwrap_or(""))
                .collect::<Vec<_>>();
            if sources(&u.view) != sources(&b) { u.tip_at = None; }
            u.view = b.clone();
        }
        let now = crate::source::now_s();
        // The stale limit follows the governed interval (`governor::stale_after`), so a quote one
        // slow poll old is not stale; with the governor off it is the configured limit as before.
        let stale_after = crate::governor::stale_after(u.cfg.stale_after_s, u.view.governed_s);
        for cell in &mut u.view.cells {
            if let Some(q) = &cell.quote {
                cell.stale |= crate::model::is_stale(q, now, stale_after, u.cfg.delayed_grace_s);
            }
        }
        sync_gov(u);
        u.hist.record(&u.view, now);
        want_range(u);
        // Alerts run after the view is current, so a balloon names the price the strip is showing.
        // The checker drops stale quotes itself: a closed market frozen at a level must not
        // retrigger on every cooldown.
        // Each fired alert is classified here, in one place: logged, and either silenced (no
        // balloon, no POST) or handed back with its log sequence number for delivery. A silenced
        // alert has already burnt its cooldown inside `check`.
        let mut deliver: Vec<(alerts::Fired, u64)> = Vec::new();
        let mut save = false;
        if u.profile.ultra && !u.cfg.alerts.is_empty() {
            let fired = alerts::check_with(&mut u.cfg, &u.view, now, &|label| u.hist.series_of(label));
            if !fired.is_empty() {
                // Written after the closure: `with_ui` does no I/O.
                save = true;
                let minute = local_minute();
                for f in fired {
                    let silence = alerts::silence(&u.cfg, now, minute);
                    let outcome = match silence {
                        None => alerts::Outcome::Shown,
                        Some(alerts::Silence::Quiet) => alerts::Outcome::Quiet,
                        Some(_) => alerts::Outcome::Muted,
                    };
                    let seq = u.alert_log.push(f.at, f.title.clone(), outcome);
                    if marks_tray(u.mode, unsafe { GetForegroundWindow() } == u.hwnd) {
                        u.alert_pending = Some(f.title.clone());
                    }
                    if silence.is_none() {
                        u.alert_note = Some(format!("alert: {}", f.title));
                        deliver.push((f, seq));
                    } else {
                        u.alert_note = Some(format!("alert silenced: {}", f.title));
                    }
                }
            }
        }
        // What the feed tick can tell the user about alerts, without a timer: a span in which they
        // could not be checked (`watch`), and what a mute or quiet hours swallowed (`hush`).
        let mut notices = FeedNotices::default();
        if u.profile.ultra {
            let (armed, fresh) = alerts::checkable(&u.cfg, &u.view);
            let interval = if u.view.governed_s > 0.0 { u.view.governed_s } else { u.cfg.poll_s };
            match u.watch.step(now, fresh, armed, watch::threshold(interval)) {
                Some(watch::Event::Back { from, to, jump }) => {
                    let (f, t) = (alerts::local_hhmm(&local_offset_at, now, from), alerts::local_hhmm(&local_offset_at, now, to));
                    let line = watch::gap_line(&f, &t, jump);
                    u.alert_log.push(to, line.clone(), alerts::Outcome::Gap);
                    u.alert_note = Some(line);
                    if u.cfg.gap_notice && alerts::silence(&u.cfg, now, local_minute()).is_none() {
                        notices.gap = Some(watch::gap_notice(&f, &t));
                    }
                    u.tip_at = None;
                }
                Some(watch::Event::Blind { .. }) => u.tip_at = None,
                None => {}
            }
            let silenced = alerts::silence(&u.cfg, now, local_minute()).is_some();
            if let Some(since) = u.hush.step(now, silenced) {
                if let Some(text) = alerts::digest(&u.alert_log.newest_first(), since) {
                    u.alert_note = Some(text.clone());
                    notices.digest = Some(text);
                }
            }
        }
        let relinked = sync_icon(u);
        let due = relinked || u.tip_at.is_none_or(|t| t.elapsed() >= TIP_EVERY);
        let tray_update = due.then(|| {
            u.tip_at = Some(Instant::now());
            (u.icon, tooltip_for(u))
        });
        let fired = deliver;
        (tray_update, fired, u.cfg.webhook.clone(), save, notices)
    }) else {
        return;
    };
    if save {
        save_cfg();
    }
    if let Some((icon, tip)) = tray_update {
        tray(hwnd, NIM_MODIFY, icon, &tip);
    }
    for (f, seq) in fired {
        alerts::balloon(hwnd, TRAY_ID, &f.title, &f.body);
        if let Some(url) = hook.clone() {
            post_alert(hwnd, url, f.title, f.body, seq, false);
        }
    }
    // One balloon when a mute or quiet hours ended, one POST per gap and only if asked for. Neither
    // carries a price.
    if let Some(text) = notices.digest {
        alerts::balloon(hwnd, TRAY_ID, config::profile().title(), &text);
    }
    if let (Some(text), Some(url)) = (notices.gap, hook) {
        post_notice(hwnd, url, text);
    }
    match with_ui(|u| u.mode) {
        Some(Mode::Strip) if unsafe { IsWindowVisible(hwnd) } != 0 => relayout_and_place(hwnd),
        Some(Mode::Panel) => unsafe {
            InvalidateRect(hwnd, null(), 0);
        },
        _ => {}
    }
}

fn apply_visibility(hwnd: HWND) {
    let Some(show) = with_ui(|u| u.mode != Mode::Strip || (u.cfg.strip_visible && !u.suppressed)) else { return };
    dock::set_paused(!show);
    unsafe {
        let visible = IsWindowVisible(hwnd) != 0;
        if show && !visible {
            // a hidden strip skips re-layout, so catch up before it reappears
            relayout_and_place(hwnd);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        } else if !show && visible {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn set_topmost(hwnd: HWND, on: bool) {
    unsafe {
        SetWindowPos(hwnd, if on { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Docked housekeeping, on every foreground change and every 500 ms: hide while a fullscreen
/// app is in front, climb back above the taskbar after a taskbar click, and undo anything else
/// that moved or hid the strip behind its back (a screenshot or automation tool, a shell restart).
fn dock_check(hwnd: HWND) {
    let Some((strip, dock, was)) =
        with_ui(|u| (u.mode == Mode::Strip, u.mode == Mode::Strip && u.cfg.placement().on_taskbar(), u.suppressed))
    else {
        return;
    };
    // Every placement steps aside for a fullscreen app; only a taskbar one climbs back over the taskbar.
    let fullscreen = strip && dock::fullscreen_foreground(hwnd);
    if fullscreen != was {
        with_ui(|u| u.suppressed = fullscreen);
        apply_visibility(hwnd);
    }
    own_by_taskbar(hwnd, dock);
    heal(hwnd);
    if dock && !fullscreen && dock::taskbar_above(hwnd) {
        set_topmost(hwnd, true);
        // Seen 2026-10-15: the taskbar stayed above a topmost strip that re-asserted HWND_TOPMOST
        // every 500 ms. Dropping and re-taking topmost forces a fresh place in the topmost band.
        if dock::taskbar_above(hwnd) {
            set_topmost(hwnd, false);
            set_topmost(hwnd, true);
        }
    }
}

/// A docked strip is an owned window of its taskbar. Windows keeps an owned window above its owner,
/// and moves it with the owner into a higher z-band: while Start or Search is open the taskbar
/// leaves the normal band (1) for band 6, where a plain topmost window cannot follow, so the strip
/// sat under the taskbar (seen 2026-10-15 by GetWindowBand logging). Ownership also stops a taskbar
/// click from covering the strip until the next re-raise. Every other placement (and the panel) is
/// unowned. An Explorer restart gives a new taskbar; a dead owner in another process only clears the
/// owner field (tested 2026-10-15 with two processes), so the next check re-points it.
fn own_by_taskbar(hwnd: HWND, dock: bool) {
    let want = if dock { dock::target() } else { null_mut() };
    unsafe {
        if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as HWND != want {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, want as isize);
            let top = with_ui(|u| u.cfg.topmost || u.cfg.placement().on_taskbar()).unwrap_or(true);
            set_topmost(hwnd, top);
        }
    }
}

/// Put the strip back where (and as) the app placed it if something outside changed it. Floating
/// is left alone: the user drags it, and a drag saves on `WM_EXITSIZEMOVE`.
fn heal(hwnd: HWND) {
    let Some((show, fixed, want)) = with_ui(|u| {
        (u.mode == Mode::Strip).then(|| (u.cfg.strip_visible && !u.suppressed, u.cfg.placement() != Placement::Floating, u.strip_rect))
    })
    .flatten() else {
        return;
    };
    unsafe {
        if show && IsWindowVisible(hwnd) == 0 {
            relayout_and_place(hwnd);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            return;
        }
        if !show || !fixed || want.right <= want.left {
            return;
        }
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        if (r.left, r.top, r.right, r.bottom) != (want.left, want.top, want.right, want.bottom) {
            relayout_and_place(hwnd);
        }
    }
}

unsafe extern "system" fn on_win_event(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    let h = STRIP.load(Ordering::Relaxed);
    if h != 0 {
        PostMessageW(h as HWND, WM_FGCHANGE, 0, 0);
    }
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Glide the window from `a` to `b` over `ms`, morphing `from` into `to` and fading the window
/// opacity between `alpha` (the strip is see-through, the panel opaque so charts don't bleed in). Each frame is drawn
/// synchronously and paced by `DwmFlush` (one frame per vsync). Blocks the UI thread for `ms`;
/// posted feed updates wait in the queue meanwhile.
#[allow(clippy::too_many_arguments)]
fn run_morph(hwnd: HWND, a: RECT, b: RECT, from: (HBITMAP, (i32, i32)), to: (HBITMAP, (i32, i32)), alpha: (u8, u8), ms: f64, ease: fn(f64) -> f64) {
    with_ui(|u| {
        u.mode = Mode::Morph;
        u.morph = Some(Morph { from: from.0, from_sz: from.1, to: to.0, to_sz: to.1, t: 0.0 });
    });
    let start = Instant::now();
    loop {
        let t = (start.elapsed().as_secs_f64() * 1000.0 / ms).min(1.0);
        let e = ease(t);
        with_ui(|u| {
            if let Some(m) = u.morph.as_mut() {
                m.t = e;
            }
        });
        let lerp = |p: i32, q: i32| p + ((q - p) as f64 * e).round() as i32;
        let (l, tp, r, bt) = (lerp(a.left, b.left), lerp(a.top, b.top), lerp(a.right, b.right), lerp(a.bottom, b.bottom));
        let op = (alpha.0 as f64 + (alpha.1 as f64 - alpha.0 as f64) * e).round() as u8;
        unsafe {
            SetLayeredWindowAttributes(hwnd, 0, op, LWA_ALPHA);
            SetWindowPos(hwnd, null_mut(), l, tp, r - l, bt - tp, SWP_NOZORDER | SWP_NOACTIVATE);
            RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_UPDATENOW);
            DwmFlush();
        }
        if t >= 1.0 {
            break;
        }
    }
    with_ui(|u| {
        if let Some(m) = u.morph.take() {
            unsafe {
                DeleteObject(m.from);
                DeleteObject(m.to);
            }
        }
    });
}

/// The panel's rect: centred in the work area of the monitor the strip is on.
fn panel_rect(near: &RECT, w: i32, h: i32) -> RECT {
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromRect(near, MONITOR_DEFAULTTONEAREST), &mut mi);
        let wa = mi.rcWork;
        let w = w.min(wa.right - wa.left).max(1);
        let h = h.min(wa.bottom - wa.top).max(1);
        let x = wa.left + (wa.right - wa.left - w) / 2;
        let y = wa.top + (wa.bottom - wa.top - h) / 2;
        RECT { left: x, top: y, right: x + w, bottom: y + h }
    }
}

/// Where the window goes when the chart opens, if it has to widen at all. Both panes need room: a frame
/// that cannot hold them is widened to the two-pane size rather than squeezing the cards into one
/// column. Records the move so closing the chart can undo exactly it.
fn grow_target(hwnd: HWND, cur: RECT) -> Option<RECT> {
    let (natural, chart_max, gap) = with_ui(|u| {
        let s = |v: i32| v * u.dpi as i32 / 96;
        (panel::geometry(u.view.cells.len(), catalog_len(u), u.dpi, false), s(720), s(10))
    })?;
    // Wide enough for the cards' natural grid plus a full chart pane; the pane takes what is left
    // over, so this is the one width where both are whole.
    let want = natural.w + chart_max + gap;
    if cur.right - cur.left >= want {
        with_ui(|u| u.chart_grew = None);
        return None;
    }
    let work = unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return None;
        }
        mi.rcWork
    };
    let grown = grown_rect(cur, want, work);
    // Remember what the chart cost, so closing it can pay it back.
    with_ui(|u| u.chart_grew = Some((cur, grown)));
    Some(grown)
}

/// Where the window goes when the chart closes: back to the frame it grew from (see
/// `rect_after_chart`), fitted to the monitor it is on. `None` when the chart never widened it, or
/// the user has resized it since.
fn shrink_target(cur: RECT) -> Option<RECT> {
    let (before, grown) = with_ui(|u| u.chart_grew.take()).flatten()?;
    rect_after_chart(cur, grown, before).map(fit_panel)
}

/// Open or close the candle pane, with the window gliding to match: opening widens it (centred on where
/// it was), closing brings it back to where it came from. Without the give-back the pane could be
/// closed but the window stayed wide, with the cards stretched across the gap. The picker and the chart
/// share the space under the cards, so opening one closes the other (`apply_picker` calls this with
/// `false`).
///
/// The glide is the strip <-> panel morph: a picture of the old state and one of the new, stretched
/// between the two frames. The panel is laid out from its width, so animating the live window would
/// reflow the card grid on every frame.
fn set_chart(hwnd: HWND, on: bool) {
    let Some((was, open)) = with_ui(|u| (u.chart, u.mode == Mode::Panel)) else {
        return;
    };
    if was == on {
        return;
    }
    let mut cur: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(hwnd, &mut cur) };
    // Only an open panel has a frame to move; a chart toggled from the tray menu while the strip is up
    // just changes state.
    let target = if !open {
        // No window to move, but the frame the panel will reopen at may be the chart's wide one: closing
        // the chart gives that back too, or the next expand is wide with nothing in the gap.
        with_ui(|u| {
            let grew = u.chart_grew.take();
            if !on {
                u.last_panel = u.last_panel.map(|f| compact_frame(f, grew));
            }
        });
        None
    } else if on {
        grow_target(hwnd, cur)
    } else {
        shrink_target(cur)
    };
    let size = |r: RECT| ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
    // The old picture is taken before the state changes, the new one after.
    let from = target.and_then(|_| {
        let (w, h) = size(cur);
        with_ui(|u| (snapshot(hwnd, w, h, |dc| draw_panel(u, dc, w, h)), (w, h)))
    });
    with_ui(|u| {
        u.chart = on;
        u.open_hover = false;
        u.chart_hover = false;
        u.chart_at = None;
        u.chart_layout.set(None);
        // The pane reopens on the range last used (`chart_range`), so the range is not reset here; the
        // cache keeps what was fetched.
        if on {
            u.pick_open = false;
            // Nothing locked yet: lock the first card, so the pane opens on a chart instead of an
            // instruction. A second click on the locked card unlocks it again.
            if u.profile.ultra && selected_row(u).is_none() {
                u.selected = u.view.cells.first().map(|c| c.label.clone());
            }
            want_range(u);
        }
    });
    if let Some(to_rect) = target {
        let (w, h) = size(to_rect);
        let to = with_ui(|u| (snapshot(hwnd, w, h, |dc| draw_panel(u, dc, w, h)), (w, h)));
        match (from, to) {
            (Some(from), Some(to)) => {
                run_morph(hwnd, cur, to_rect, from, to, (255, 255), CHART_MS, ease_in_out);
                with_ui(|u| u.mode = Mode::Panel);
            }
            // The glide is a nicety; the frame is not. If a picture could not be taken, the window
            // still goes where the state says, and nothing is leaked.
            (from, to) => unsafe {
                for pic in [from, to].into_iter().flatten() {
                    DeleteObject(pic.0);
                }
            },
        }
        unsafe {
            SetWindowPos(hwnd, null_mut(), to_rect.left, to_rect.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

/// A saved panel rect, kept inside the work area of the monitor it lands on. A rect whose header
/// would sit off the desktop — the monitor was unplugged, or the resolution dropped — is pulled back
/// in rather than left unreachable, and a frame larger than the screen is trimmed to it.
fn fit_panel(r: RECT) -> RECT {
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return r;
        }
        let wa = mi.rcWork;
        let w = (r.right - r.left).min(wa.right - wa.left);
        let h = (r.bottom - r.top).min(wa.bottom - wa.top);
        let x = r.left.clamp(wa.left, (wa.right - w).max(wa.left));
        let y = r.top.clamp(wa.top, (wa.bottom - h).max(wa.top));
        RECT { left: x, top: y, right: x + w, bottom: y + h }
    }
}

/// Strip -> panel.
fn expand(hwnd: HWND) {
    clear_pending(hwnd);
    wake_feed();
    let Some((a, b, from, to, op, dpi0)) = with_ui(|u| {
        if u.mode != Mode::Strip {
            return None;
        }
        if u.fonts.is_none() {
            u.fonts = Some(panel::Fonts::new(u.face, u.dpi, u.theme.is_pixel()));
        }
        // Every expand starts the drift from the top of its loop.
        u.anim = None;
        u.hover = false;
        u.pick_hover = false;
        u.chart_hover = false;
        u.gear_hover = false;
        u.open_hover = false;
        u.hover_at = None;
        u.chart_at = None;
        u.chart_layout.set(None);
        u.prev_fg = unsafe { GetForegroundWindow() };
        let a = u.strip_rect;
        let natural = panel_geo(u);
        // A frame the user dragged last time comes back the way they left it; anything that no longer
        // fits the monitor (unplugged, resolution changed) is fitted again rather than lost.
        let b = if let Some(last) = u.last_panel {
            fit_panel(last)
        } else { match u.cfg.panel {
            Some(p) => fit_panel(RECT { left: p[0], top: p[1], right: p[0] + p[2], bottom: p[1] + p[3] }),
            None => panel_rect(&a, natural.w, natural.h),
        }};
        // The content is laid out for the frame it is about to be drawn in, not for the one it would
        // have had if the window were still content-sized.
        let (pw, ph) = (b.right - b.left, b.bottom - b.top);
        let fsz = (u.frame.w.max(1), u.frame.h.max(1));
        let from = snapshot(hwnd, fsz.0, fsz.1, |dc| draw_strip(u, dc));
        let to = snapshot(hwnd, pw, ph, |dc| draw_panel(u, dc, pw, ph));
        Some((a, b, (from, fsz), (to, (pw, ph)), u.cfg.opacity, u.dpi))
    })
    .flatten() else {
        return;
    };
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        SetForegroundWindow(hwnd);
    }
    run_morph(hwnd, a, b, from, to, (op, 255), EXPAND_MS, ease_out);
    // A saved frame can sit on a monitor of another scale: `WM_DPICHANGED` arrived mid-morph and could
    // not lay anything out, so the panel is fitted again at the new scale (audit 1.8, finding 9).
    let b = with_ui(|u| {
        u.mode = Mode::Panel;
        // The cache may have expired while the panel was collapsed, and the pane is about to show.
        want_range(u);
        settled_frame(dpi0, u.dpi, b, || {
            let g = panel_geo(u);
            let mut r: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetWindowRect(hwnd, &mut r) };
            panel_rect(&r, g.w, g.h)
        })
    })
    .unwrap_or(b);
    sync_anim(hwnd);
    unsafe {
        SetWindowPos(hwnd, null_mut(), b.left, b.top, b.right - b.left, b.bottom - b.top, SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hwnd, null(), 0);
        SetFocus(hwnd);
    }
}

/// Panel -> strip (back into the taskbar gap when docked, re-measured in case Start moved).
fn collapse(hwnd: HWND) {
    let Some((a, b, from, to, prev, op, dpi0)) = with_ui(|u| {
        if u.mode != Mode::Panel {
            return None;
        }
        let mut a: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(hwnd, &mut a) };
        u.last_panel = Some(a);
        let psz = ((a.right - a.left).max(1), (a.bottom - a.top).max(1));
        u.hover = false;
        u.chart_at = None;
        let from = snapshot(hwnd, psz.0, psz.1, |dc| draw_panel(u, dc, psz.0, psz.1));
        u.strip_rect = strip_place(u);
        let fsz = (u.frame.w.max(1), u.frame.h.max(1));
        let to = snapshot(hwnd, fsz.0, fsz.1, |dc| draw_strip(u, dc));
        Some((a, u.strip_rect, (from, psz), (to, fsz), std::mem::replace(&mut u.prev_fg, null_mut()), u.cfg.opacity, u.dpi))
    })
    .flatten() else {
        return;
    };
    run_morph(hwnd, a, b, from, to, (255, op), COLLAPSE_MS, ease_in_out);
    // The strip is the window's mode again, so it can be laid out: if the morph crossed to a monitor
    // of another scale, the frame planned above is for the old one (audit 1.8, finding 9).
    let b = with_ui(|u| {
        u.mode = Mode::Strip;
        // The snapshots above painted the pane at the morph's sizes; none of them is the frame the
        // pointer would be answered from.
        u.chart_layout.set(None);
        settled_frame(dpi0, u.dpi, b, || {
            u.strip_rect = strip_place(u);
            u.strip_rect
        })
    })
    .unwrap_or(b);
    sync_anim(hwnd);
    unsafe {
        SetWindowPos(hwnd, null_mut(), b.left, b.top, b.right - b.left, b.bottom - b.top, SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hwnd, null(), 0);
        if !prev.is_null() && IsWindow(prev) != 0 && prev != hwnd {
            SetForegroundWindow(prev);
        }
    }
    apply_visibility(hwnd);
    dock_check(hwnd);
}

/// The context menu. `at` is the client-space click that opened it, when there was one: a
/// right-click on a card acts on that card, everywhere else it is the window's own menu.
fn menu(hwnd: HWND, at: Option<(i32, i32)>) {
    menu_at(hwnd, at, None)
}

/// The panel's settings button: the same menu the tray icon opens, dropped from under the button
/// (left edges aligned; Windows slides it back on screen if it would not fit) instead of wherever
/// the cursor happened to be.
fn settings_menu(hwnd: HWND) {
    let Some((x, y)) = with_ui(|u| {
        let g = panel_geo(u);
        (g.gear_btn.left, g.gear_btn.bottom + 4 * u.dpi as i32 / 96)
    }) else {
        return;
    };
    let mut pt = POINT { x, y };
    unsafe { ClientToScreen(hwnd, &mut pt) };
    menu_at(hwnd, None, Some((pt.x, pt.y)));
    // The menu ran its own message loop, so the pointer's departure from the button was never seen.
    with_ui(|u| u.gear_hover = false);
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

/// `anchor` is a screen point to drop the menu from; `None` drops it at the cursor.
fn menu_at(hwnd: HWND, at: Option<(i32, i32)>, anchor: Option<(i32, i32)>) {
    clear_pending(hwnd);
    #[allow(clippy::type_complexity)]
    let Some((visible, top, docked, status, open, list, shown, theme_name, theme_font, poll_s, ultra, picked, alerted)) = with_ui(|u| {
        let list = config::menu_list(&u.cfg);
        let shown: Vec<bool> = list.iter().map(|r| config::is_shown(&u.cfg, &r.symbol)).collect();
        let note = if u.view.note.is_empty() { theme::link_name(u.view.link).to_string() } else { u.view.note.clone() };
        let shown_labels: Vec<&str> = list.iter().zip(&shown).filter(|(_, on)| **on).map(|(r, _)| r.label.as_str()).collect();
        let alerted = armed_labels(&u.cfg.alerts, &shown_labels);
        (
            u.cfg.strip_visible,
            u.cfg.topmost,
            u.cfg.placement().on_taskbar(),
            note,
            u.mode != Mode::Strip,
            list,
            shown,
            u.cfg.theme.clone(),
            u.cfg.theme_font.clone(),
            u.cfg.poll_s,
            u.profile.ultra,
            u.pick_open,
            alerted,
        )
    }) else {
        return;
    };
    let (placement, cur_mon) = with_ui(|u| (u.cfg.placement(), u.cfg.monitor.clone())).unwrap_or((Placement::Auto, None));
    let mons = dock::monitors();
    let mut commands = crate::menu_commands::MenuCommands::default();
    let cmd = unsafe {
        let m = CreatePopupMenu();
        let add = |menu: HMENU, flags: u32, id: usize, text: &str| {
            let t = wide(text);
            AppendMenuW(menu, flags, id, t.as_ptr());
        };
        let item = |flags: u32, id: usize, text: &str| add(m, flags, id, text);
        let dynamic = |menu, flags, commands: &mut crate::menu_commands::MenuCommands, action, text: &str| {
            let id = commands.add(action);
            add(menu, flags | if id.is_some() { 0 } else { MF_GRAYED }, id.unwrap_or(0), text);
            id
        };
        // A card under the pointer: the two things a card is for, first in the menu.
        let card = at.and_then(|(x, y)| {
            with_ui(|u| {
                if u.mode != Mode::Panel {
                    return None;
                }
                let i = panel::card_at(&panel_geo(u), u.view.cells.len(), x, y)?;
                let c = u.view.cells.get(i)?;
                let price = fmt_price(c.quote.as_ref()?.price, c.digits);
                Some((i, c.label.clone(), price))
            })
            .flatten()
        });
        if let Some((_, label, price)) = &card {
            use crate::menu_commands::Action;
            // Both entries carry the label (Copy) and the symbol (Open), like the order entries below:
            // the rows can change while the menu is open, and a position would then name another card.
            dynamic(m, MF_STRING, &mut commands, Action::Copy(label.clone()), &format!("Copy {label} {price}"));
            let symbol = with_ui(|u| u.cfg.rows.iter().find(|r| &r.label == label).map(|r| r.symbol.clone())).flatten();
            dynamic(m, MF_STRING | if symbol.is_some() { 0 } else { MF_GRAYED }, &mut commands, Action::Open(symbol.unwrap_or_default()), &format!("Open {label} on CNBC"));
            // Order and removal (Ultra). The actions carry the symbol, so they act on this card even
            // if the rows change while the menu is open. Read in a call of its own (the big tuple
            // above is not extended).
            let place = if ultra {
                with_ui(|u| {
                    let pos = u.cfg.rows.iter().position(|r| &r.label == label)?;
                    Some((u.cfg.rows[pos].symbol.clone(), pos, u.cfg.rows.len()))
                })
                .flatten()
            } else {
                None
            };
            // Name and decimals (both builds). The actions carry the symbol, like the order entries.
            let look = with_ui(|u| {
                let r = u.cfg.rows.iter().find(|r| &r.label == label)?;
                let price = u.view.cells.iter().find(|c| &c.label == label).and_then(|c| c.quote.as_ref()).map(|q| q.price);
                Some((r.symbol.clone(), r.digits, config::auto_digits(&r.symbol, price), config::derived_label(&u.cfg, &r.symbol) != r.label))
            })
            .flatten();
            let pin = with_ui(|u| {
                let i = u.cfg.rows.iter().position(|r| &r.label == label)?;
                let pinned: Vec<bool> = u.cfg.rows.iter().map(|r| r.strip).collect();
                Some((u.cfg.rows[i].symbol.clone(), pinned[i], crate::pins::can_unpin(&pinned, i), pinned.len() > 1))
            })
            .flatten();
            if let Some((symbol, pinned, can_unpin, several)) = pin {
                AppendMenuW(m, MF_SEPARATOR, 0, null());
                if pinned {
                    dynamic(m, MF_STRING | if can_unpin { 0 } else { MF_GRAYED }, &mut commands, Action::Pin(symbol, false), "Unpin from strip");
                } else {
                    dynamic(m, MF_STRING | if several { 0 } else { MF_GRAYED }, &mut commands, Action::Pin(symbol, true), "Pin to strip");
                }
            }
            if let Some((symbol, digits, auto, renamed)) = look {
                AppendMenuW(m, MF_SEPARATOR, 0, null());
                dynamic(m, MF_STRING, &mut commands, Action::Rename(symbol.clone()), "Rename...");
                dynamic(m, MF_STRING | if renamed { 0 } else { MF_GRAYED }, &mut commands, Action::ResetName(symbol.clone()), "Reset name");
                let dec = CreatePopupMenu();
                dynamic(dec, MF_STRING | if digits == auto { MF_CHECKED } else { 0 }, &mut commands, Action::Digits(symbol.clone(), None), &format!("Auto ({auto})"));
                for d in 0..=6u32 {
                    dynamic(dec, MF_STRING | if digits == d && digits != auto { MF_CHECKED } else { 0 }, &mut commands, Action::Digits(symbol.clone(), Some(d)), &d.to_string());
                }
                add(m, MF_POPUP, dec as usize, &format!("Decimals: {digits}"));
            }
            if let Some((symbol, pos, count)) = place {
                AppendMenuW(m, MF_SEPARATOR, 0, null());
                let grey = |off: bool| MF_STRING | if off { MF_GRAYED } else { 0 };
                dynamic(m, grey(pos == 0), &mut commands, Action::Move(symbol.clone(), -1), "Move left");
                dynamic(m, grey(pos + 1 >= count), &mut commands, Action::Move(symbol.clone(), 1), "Move right");
                dynamic(m, grey(pos == 0), &mut commands, Action::MoveFirst(symbol.clone()), "Move first");
                dynamic(m, grey(count <= 1), &mut commands, Action::Remove(symbol), "Remove from strip");
            }
            AppendMenuW(m, MF_SEPARATOR, 0, null());
        }
        // Symbols: tick to show, untick to hide. The last one cannot go; at most MAX_ROWS show.
        let n_shown = shown.iter().filter(|s| **s).count();
        let full = n_shown >= config::MAX_ROWS;
        let syms = CreatePopupMenu();
        for (i, r) in list.iter().enumerate() {
            let on = shown[i];
            let locked = if on { n_shown <= 1 } else { full };
            let flags = MF_STRING | if on { MF_CHECKED } else { 0 } | if locked { MF_GRAYED } else { 0 };
            let text = if r.name.is_empty() { r.label.clone() } else { format!("{}\t{}", r.label, r.name) };
            dynamic(syms, flags, &mut commands, crate::menu_commands::Action::Symbol(r.symbol.clone()), &text);
        }
        AppendMenuW(syms, MF_SEPARATOR, 0, null());
        // Add and remove by hand (both builds). Read in a call of its own: the big tuple is not extended.
        let (adding, custom) = with_ui(|u| (u.adding.clone(), config::custom_entries(&u.cfg))).unwrap_or((None, Vec::new()));
        match &adding {
            Some(symbol) => add(syms, MF_STRING | MF_GRAYED, CMD_ADD_SYMBOL, &format!("Adding {symbol}...")),
            None => add(syms, MF_STRING, CMD_ADD_SYMBOL, "Add symbol..."),
        }
        let forget = CreatePopupMenu();
        for r in &custom {
            // The only symbol shown cannot go, here as everywhere.
            let last = n_shown <= 1 && list.iter().zip(&shown).any(|(l, on)| *on && l.symbol == r.symbol);
            let text = format!("{}\t{}", r.label, r.symbol);
            dynamic(forget, MF_STRING | if last { MF_GRAYED } else { 0 }, &mut commands, crate::menu_commands::Action::Forget(r.symbol.clone()), &text);
        }
        add(syms, MF_POPUP | if custom.is_empty() { MF_GRAYED } else { 0 }, forget as usize, "Remove a symbol I added");
        let order = CreatePopupMenu();
        add(order, MF_STRING, CMD_ORDER_MOVE, "By day move, biggest first");
        add(order, MF_STRING, CMD_ORDER_NAME, "By name, A to Z");
        add(order, MF_STRING, CMD_ORDER_DEFAULT, "Default order");
        add(syms, MF_POPUP | if n_shown <= 1 { MF_GRAYED } else { 0 }, order as usize, "Order the cards (once)");
        let unpinned = with_ui(|u| u.cfg.rows.iter().any(|r| !r.strip)).unwrap_or(false);
        add(syms, MF_STRING | if unpinned { 0 } else { MF_GRAYED }, CMD_PIN_ALL, "Show all on strip");
        add(syms, MF_STRING, CMD_RESET, "Reset to default");
        item(MF_STRING, CMD_PANEL, if open { "Back to taskbar" } else { "Expand view" });
        SetMenuDefaultItem(m, CMD_PANEL as u32, 0);
        add(m, MF_POPUP, syms as usize, &format!("Symbols ({n_shown})"));
        // Undo (W3 J2): one snapshot, so one line; it says what it would undo.
        match with_ui(|u| u.undo.kind()).flatten() {
            Some(kind) => item(MF_STRING, CMD_UNDO, &format!("Undo the last {}{}", kind.label(), if ultra { "\tCtrl+Z" } else { "" })),
            None => item(MF_STRING | MF_GRAYED, CMD_UNDO, "Undo"),
        }
        // Poll interval: `poll_s` has always been in the config; nothing exposed it until now.
        let poll = CreatePopupMenu();
        for (i, secs) in POLL_CHOICES.iter().enumerate() {
            add(poll, MF_STRING, CMD_POLL + i, &format!("{secs} s"));
        }
        let cur_poll = POLL_CHOICES.iter().position(|secs| (secs - poll_s).abs() < 0.01).unwrap_or(2);
        CheckMenuRadioItem(
            poll,
            CMD_POLL as u32,
            (CMD_POLL + POLL_CHOICES.len() - 1) as u32,
            (CMD_POLL + cur_poll) as u32,
            MF_BYCOMMAND,
        );
        // The governor (W2): the choice above is the pace while prices move; these two only ever slow it,
        // and never below one poll in 15 s while an alert is armed.
        let gov = with_ui(|u| u.cfg.governor(u.profile.ultra)).unwrap_or_default();
        AppendMenuW(poll, MF_SEPARATOR, 0, null());
        add(poll, MF_STRING | if gov.slow_when_quiet { MF_CHECKED } else { 0 }, CMD_SLOW_QUIET, "Slow down when nothing moves");
        add(poll, MF_STRING | if gov.rest_when_locked { MF_CHECKED } else { 0 }, CMD_REST_LOCKED, "Rest while the PC is locked");
        add(m, MF_POPUP, poll as usize, &format!("Poll every {poll_s} s"));
        let sources = CreatePopupMenu();
        let fallback = with_ui(|u| u.cfg.fallback_enabled).unwrap_or(false);
        add(sources, MF_STRING | if fallback { MF_CHECKED } else { 0 }, CMD_FALLBACK, "Allow fallback sources");
        add(m, MF_POPUP, sources as usize, "Data sources");
        item(MF_STRING, CMD_REFRESH, "Refresh now");
        item(MF_STRING, CMD_COPY_ALL, "Copy all prices");
        if ultra {
            // Theme: four tier submenus, because 25 entries in one popup is a wall. The tier is a
            // rendering rule (`theme::Kind`), so the menu cannot disagree with the painter.
            let th = CreatePopupMenu();
            let cur = theme::index_of(&theme_name);
            for kind in theme::Kind::ALL {
                let sub = CreatePopupMenu();
                // The tier explains itself, so "Animated" and "Retro" do not need a README.
                add(sub, MF_STRING | MF_GRAYED, 0, kind.note());
                AppendMenuW(sub, MF_SEPARATOR, 0, null());
                for i in theme::tier(kind) {
                    let t = &theme::ALL[i];
                    let flags = MF_STRING | if i == cur { MF_CHECKED } else { 0 };
                    add(sub, flags, CMD_THEME + i, &format!("{}\t{}", t.name, t.note));
                }
                add(th, MF_POPUP, sub as usize, kind.label());
            }
            add(m, MF_POPUP, th as usize, &format!("Theme \u{b7} {}", theme::ALL[cur].name));
            // Font: the same idea one axis over — grouped by family, not one list of 13 names.
            let face = theme::font_of(&theme::ALL[cur], theme_font.as_deref());
            let ft = CreatePopupMenu();
            for group in theme::FACE_GROUPS {
                let sub = CreatePopupMenu();
                for (i, f) in theme::FACES.iter().enumerate().filter(|(_, f)| f.group == group) {
                    // A face this machine cannot draw is greyed rather than hidden: the menu is a
                    // catalogue of what the app can do, and "installed on other machines" is
                    // information, not noise.
                    let missing = !theme::face_available(f.name);
                    let flags = MF_STRING
                        | if f.name == face { MF_CHECKED } else { 0 }
                        | if missing { MF_GRAYED } else { 0 };
                    add(sub, flags, CMD_FONT + i, &format!("{}\t{}", f.name, f.note));
                }
                add(ft, MF_POPUP, sub as usize, group);
            }
            add(m, MF_POPUP, ft as usize, &format!("Font \u{b7} {face}"));
            item(MF_STRING | if picked { MF_CHECKED } else { 0 }, CMD_PICK, "Symbol picker");
            // Alerts: per symbol the presets, what is set and Clear; then the settings that apply
            // to every alert. The state is read in one separate `with_ui` call (`alerts_view`).
            let av = alerts_view(&list, &shown).unwrap_or_default();
            let al = CreatePopupMenu();
            for row in &av.rows {
                let sub = CreatePopupMenu();
                let mut group = None;
                for (k, p) in alerts::PRESETS.iter().enumerate() {
                    if group.is_some_and(|g| g != p.group) {
                        AppendMenuW(sub, MF_SEPARATOR, 0, null());
                    }
                    group = Some(p.group);
                    let on = row.enabled.get(k).copied().unwrap_or(false);
                    dynamic(sub, MF_STRING | if on { 0 } else { MF_GRAYED }, &mut commands, crate::menu_commands::Action::Alert(row.label.clone(), k), p.label);
                }
                // The fast move: from the hour of samples held, so it needs no fresh price to be set.
                AppendMenuW(sub, MF_SEPARATOR, 0, null());
                let cur_fast = row.alert.as_ref().map_or(0, |a| a.fast as usize);
                for k in 0..alerts::FAST_MOVES.len() {
                    let text = format!("fast move {}", alerts::fast_label(k));
                    dynamic(sub, MF_STRING | if cur_fast == k + 1 { MF_CHECKED } else { 0 }, &mut commands, crate::menu_commands::Action::Fast(row.label.clone(), k), &text);
                }
                // A level typed by hand: greyed without a fresh price, like the presets.
                AppendMenuW(sub, MF_SEPARATOR, 0, null());
                dynamic(sub, MF_STRING | if row.fresh { 0 } else { MF_GRAYED }, &mut commands, crate::menu_commands::Action::AlertAt(row.label.clone()), "at a price...");
                AppendMenuW(sub, MF_SEPARATOR, 0, null());
                let line = row.alert.as_ref().map(|a| alerts::describe(a, row.digits)).filter(|d| !d.is_empty());
                add(sub, MF_STRING | MF_GRAYED, 0, &line.unwrap_or_else(|| "nothing set".into()));
                if alerted.contains(&row.label) {
                    dynamic(sub, MF_STRING, &mut commands, crate::menu_commands::Action::ClearAlert(row.label.clone()), "Clear this symbol's alerts");
                }
                add(al, MF_POPUP, sub as usize, &row.label);
            }
            if av.rows.is_empty() {
                add(al, MF_GRAYED, 0, "(no symbols shown)");
            }
            AppendMenuW(al, MF_SEPARATOR, 0, null());
            // At a glance: everything that is set, how far each level is, and Clear all.
            let glance = CreatePopupMenu();
            if av.overview.is_empty() {
                add(glance, MF_STRING | MF_GRAYED, 0, "(nothing set)");
            }
            for line in &av.overview {
                add(glance, MF_STRING | MF_GRAYED, 0, &line.text.replace('&', "&&"));
            }
            AppendMenuW(glance, MF_SEPARATOR, 0, null());
            add(glance, MF_STRING | if av.overview.is_empty() { MF_GRAYED } else { 0 }, CMD_ALERTS_CLEAR_ALL, "Clear all alerts...");
            let cannot_fire = av.overview.iter().filter(|l| l.warn).count();
            let glance_title = if cannot_fire > 0 {
                format!("At a glance ({}, {cannot_fire} cannot fire)", av.overview.len())
            } else {
                format!("At a glance ({})", av.overview.len())
            };
            add(al, MF_POPUP, glance as usize, &glance_title);
            // Repeat: how an alert comes back after it has fired. Every cooldown is 1.8's rule.
            let repeat = CreatePopupMenu();
            add(repeat, MF_STRING, CMD_REPEAT_COOLDOWN, "Every cooldown");
            add(repeat, MF_STRING, CMD_ALERT_ONCE, "Fire once, then clear");
            add(repeat, MF_STRING, CMD_REPEAT_CROSSING, "Each crossing");
            let (cur_repeat, repeat_word) = if av.once {
                (CMD_ALERT_ONCE, "once")
            } else if av.crossing {
                (CMD_REPEAT_CROSSING, "each crossing")
            } else {
                (CMD_REPEAT_COOLDOWN, "every cooldown")
            };
            CheckMenuRadioItem(repeat, CMD_REPEAT_COOLDOWN as u32, CMD_REPEAT_CROSSING as u32, cur_repeat as u32, MF_BYCOMMAND);
            add(al, MF_POPUP, repeat as usize, &format!("Repeat \u{b7} {repeat_word}"));
            // Mute. A timed mute ends by the clock; the label says when, in this PC's local time.
            let mute = CreatePopupMenu();
            add(mute, MF_STRING, CMD_MUTE_1H, "For 1 hour");
            add(mute, MF_STRING, CMD_MUTE_8H, "For 8 hours");
            add(mute, MF_STRING, CMD_MUTE_INDEF, "Until I turn it back on");
            AppendMenuW(mute, MF_SEPARATOR, 0, null());
            let (mute_title, is_muted) = match av.silence {
                Some(alerts::Silence::Muted) => ("Muted".to_string(), true),
                Some(alerts::Silence::MutedUntil(t)) => (format!("Muted until {}", alerts::local_hhmm(&local_offset_at, av.now, t)), true),
                _ => ("Mute".to_string(), false),
            };
            add(mute, MF_STRING | if is_muted { 0 } else { MF_GRAYED }, CMD_UNMUTE, "Unmute");
            add(al, MF_POPUP, mute as usize, &mute_title);
            // Quiet hours (local time).
            let quiet = CreatePopupMenu();
            add(quiet, MF_STRING, CMD_QUIET_OFF, "Off");
            for (i, [from, to]) in alerts::QUIET_CHOICES.iter().enumerate() {
                add(quiet, MF_STRING, CMD_QUIET + i, &format!("{} to {}", alerts::hhmm(*from), alerts::hhmm(*to)));
            }
            let cur_quiet = match av.quiet {
                None => Some(CMD_QUIET_OFF),
                Some(w) => alerts::QUIET_CHOICES.iter().position(|c| *c == w).map(|i| CMD_QUIET + i),
            };
            if let Some(c) = cur_quiet {
                CheckMenuRadioItem(quiet, CMD_QUIET_OFF as u32, (CMD_QUIET + alerts::QUIET_CHOICES.len() - 1) as u32, c as u32, MF_BYCOMMAND);
            }
            let quiet_title = match av.quiet {
                Some([f, t]) if f != t => format!("Quiet hours \u{b7} {} to {}", alerts::hhmm(f), alerts::hhmm(t)),
                _ => "Quiet hours".to_string(),
            };
            add(al, MF_POPUP, quiet as usize, &quiet_title);
            // Webhook: only the host is ever shown.
            let hook = CreatePopupMenu();
            let hook_text = av.host.as_ref().map_or("(no webhook set)".to_string(), |h| format!("Sends to {h}"));
            add(hook, MF_STRING | MF_GRAYED, CMD_HOOK_HOST, &hook_text);
            AppendMenuW(hook, MF_SEPARATOR, 0, null());
            add(hook, MF_STRING, CMD_HOOK_PASTE, "Paste URL from clipboard");
            let has_hook = av.host.is_some();
            add(hook, MF_STRING | if has_hook { 0 } else { MF_GRAYED }, CMD_HOOK_TEST, "Send a test alert");
            add(hook, MF_STRING | if has_hook { 0 } else { MF_GRAYED }, CMD_HOOK_REMOVE, "Remove");
            AppendMenuW(hook, MF_SEPARATOR, 0, null());
            add(
                hook,
                MF_STRING | if has_hook { 0 } else { MF_GRAYED } | if av.gap_notice { MF_CHECKED } else { 0 },
                CMD_GAP_NOTICE,
                "Say when alerts could not be checked",
            );
            add(al, MF_POPUP, hook as usize, "Webhook");
            // Recent alerts, newest first.
            let recent = CreatePopupMenu();
            if av.log.is_empty() {
                add(recent, MF_STRING | MF_GRAYED, 0, "(nothing yet)");
            }
            for e in &av.log {
                add(recent, MF_STRING | MF_GRAYED, 0, &alerts::entry_line(e, &local_offset_at, av.now).replace('&', "&&"));
            }
            AppendMenuW(recent, MF_SEPARATOR, 0, null());
            add(recent, MF_STRING | if av.log.is_empty() { MF_GRAYED } else { 0 }, CMD_LOG_COPY, "Copy list");
            add(recent, MF_STRING | if av.log.is_empty() { MF_GRAYED } else { 0 }, CMD_LOG_CLEAR, "Clear list");
            add(al, MF_POPUP, recent as usize, &format!("Recent ({})", av.log.len()));
            add(m, MF_POPUP, al as usize, &format!("Alerts ({})", alerted.len()));
        }
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        item(MF_STRING | if visible { MF_CHECKED } else { 0 }, CMD_STRIP, "Show strip");
        // Position: where the strip sits (and on which display, when there are several).
        let pos = CreatePopupMenu();
        for (i, p) in Placement::ALL.iter().enumerate() {
            add(pos, MF_STRING, CMD_POS + i, p.label());
        }
        let cur = Placement::ALL.iter().position(|p| *p == placement).unwrap_or(0);
        let last = CMD_POS + Placement::ALL.len() - 1;
        CheckMenuRadioItem(pos, CMD_POS as u32, last as u32, (CMD_POS + cur) as u32, MF_BYCOMMAND);
        if mons.len() > 1 {
            AppendMenuW(pos, MF_SEPARATOR, 0, null());
            let primary = mons.iter().position(|d| d.primary).unwrap_or(0);
            let active = mons.iter().position(|d| Some(&d.device) == cur_mon.as_ref()).unwrap_or(primary);
            let mut monitor_ids = Vec::with_capacity(mons.len());
            for (i, d) in mons.iter().enumerate() {
                let (w, h) = (d.bounds.right - d.bounds.left, d.bounds.bottom - d.bounds.top);
                let main = if d.primary { ", main" } else { "" };
                let id = dynamic(pos, MF_STRING, &mut commands,
                    crate::menu_commands::Action::Monitor((!d.primary).then(|| d.device.clone())),
                    &format!("Display {}{main}\t{w} x {h}", i + 1));
                monitor_ids.push(id);
            }
            if let (Some(first), Some(last), Some(selected)) = (
                monitor_ids.iter().flatten().next(), monitor_ids.iter().flatten().next_back(), monitor_ids[active]
            ) {
                CheckMenuRadioItem(pos, *first as u32, *last as u32, selected as u32, MF_BYCOMMAND);
            }
        }
        add(m, MF_POPUP | if open { MF_GRAYED } else { 0 }, pos as usize, "Position");
        item(MF_STRING | if top || docked { MF_CHECKED } else { 0 } | if docked { MF_GRAYED } else { 0 }, CMD_TOP, "Always on top");
        // Strip: rows, text size and opacity, applied live. Radio marks show the nearest step.
        {
            let (rows, dock_pt, pt, opacity) =
                with_ui(|u| (u.cfg.dock_rows, u.cfg.dock_font_pt, u.cfg.font_pt, u.cfg.opacity)).unwrap_or((2, 9, 10, 235));
            let st = CreatePopupMenu();
            let sub_rows = CreatePopupMenu();
            for (i, n) in STRIP_ROWS.iter().enumerate() {
                add(sub_rows, MF_STRING, CMD_STRIP_ROWS + i, &format!("{n} row{}", if *n == 1 { "" } else { "s" }));
            }
            CheckMenuRadioItem(sub_rows, CMD_STRIP_ROWS as u32, (CMD_STRIP_ROWS + STRIP_ROWS.len() - 1) as u32,
                (CMD_STRIP_ROWS + nearest_rows(rows)) as u32, MF_BYCOMMAND);
            add(st, MF_POPUP | if docked { 0 } else { MF_GRAYED }, sub_rows as usize, "Rows in the taskbar");
            let sub_text = CreatePopupMenu();
            for (i, (_, _, label)) in TEXT_STEPS.iter().enumerate() {
                add(sub_text, MF_STRING, CMD_STRIP_TEXT + i, label);
            }
            CheckMenuRadioItem(sub_text, CMD_STRIP_TEXT as u32, (CMD_STRIP_TEXT + TEXT_STEPS.len() - 1) as u32,
                (CMD_STRIP_TEXT + nearest_text(dock_pt, pt)) as u32, MF_BYCOMMAND);
            add(st, MF_POPUP, sub_text as usize, "Text size");
            let sub_op = CreatePopupMenu();
            for (i, (_, label)) in OPACITY_STEPS.iter().enumerate() {
                add(sub_op, MF_STRING, CMD_STRIP_OPACITY + i, label);
            }
            CheckMenuRadioItem(sub_op, CMD_STRIP_OPACITY as u32, (CMD_STRIP_OPACITY + OPACITY_STEPS.len() - 1) as u32,
                (CMD_STRIP_OPACITY + nearest_opacity(opacity)) as u32, MF_BYCOMMAND);
            add(st, MF_POPUP, sub_op as usize, "Opacity");
            add(m, MF_POPUP, st as usize, "Strip");
        }
        // Start with Windows: the registry is the source of truth, read each time the menu opens.
        item(MF_STRING | if autostart::enabled(config::profile().id) { MF_CHECKED } else { 0 }, CMD_AUTOSTART, "Start with Windows");
        // Hotkey: Off, the presets, and the configured combo when it is none of them.
        {
            let (cfg_key, failed) = with_ui(|u| (u.cfg.hotkey.clone(), u.hotkey_failed)).unwrap_or((None, false));
            let hk = CreatePopupMenu();
            let parsed = cfg_key.as_deref().and_then(hotkey::parse);
            let preset = cfg_key.as_deref().and_then(hotkey::preset_index);
            let custom = cfg_key.as_ref().filter(|_| preset.is_none());
            let note = if !failed { "" } else if parsed.is_some() { " (in use by another app)" } else { " (not a valid hotkey)" };
            add(hk, MF_STRING, CMD_HOTKEY, "Off");
            for (i, p) in hotkey::PRESETS.iter().enumerate() {
                let tail = if preset == Some(i) { note } else { "" };
                add(hk, MF_STRING, CMD_HOTKEY + 1 + i, &format!("{p}{tail}"));
            }
            if let Some(text) = custom {
                let shown = parsed.map(|c| hotkey::label(&c)).unwrap_or_else(|| text.chars().take(24).collect());
                add(hk, MF_STRING, CMD_HOTKEY_CUSTOM, &format!("{shown}{note}"));
            }
            let selected = match (&cfg_key, preset) {
                (None, _) => CMD_HOTKEY,
                (Some(_), Some(i)) => CMD_HOTKEY + 1 + i,
                (Some(_), None) => CMD_HOTKEY_CUSTOM,
            };
            CheckMenuRadioItem(hk, CMD_HOTKEY as u32, CMD_HOTKEY_CUSTOM as u32, selected as u32, MF_BYCOMMAND);
            let head = match &cfg_key {
                None => "Hotkey \u{b7} off".to_string(),
                Some(text) => format!("Hotkey \u{b7} {}{note}", parsed.map(|c| hotkey::label(&c)).unwrap_or_else(|| text.chars().take(24).collect())),
            };
            add(m, MF_POPUP, hk as usize, &head);
        }
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        let status: String = status.chars().take(90).collect();
        item(MF_STRING | MF_GRAYED, CMD_STATUS, &status);
        if config::save_failed() {
            item(MF_STRING | MF_GRAYED, CMD_SAVE_STATUS, "Settings could not be saved (changes last until you quit)");
        }
        if let Some(name) = config::kept_bad_config() {
            item(MF_STRING | MF_GRAYED, CMD_KEPT_STATUS, &format!("Settings were unreadable; kept as {name}, defaults loaded"));
        }
        item(MF_STRING | MF_GRAYED, CMD_METER, &crate::source::meter_line());
        item(MF_STRING | MF_GRAYED, CMD_VERSION, &format!("{} {}", config::profile().title(), env!("CARGO_PKG_VERSION")));
        if with_ui(|u| u.update_running).unwrap_or(false) {
            item(MF_STRING | MF_GRAYED, CMD_UPDATE, "Checking...");
        } else {
            item(MF_STRING, CMD_UPDATE, "Check for updates");
        }
        if with_ui(|u| u.report_running).unwrap_or(false) {
            item(MF_STRING | MF_GRAYED, CMD_REPORT, "Diagnostics report: checking...");
        } else {
            item(MF_STRING, CMD_REPORT, "Copy diagnostics report");
        }
        item(MF_STRING, CMD_FOLDER, "Open config folder");
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        item(MF_STRING, CMD_EXIT, "Quit");
        let mut pt = POINT { x: 0, y: 0 };
        match anchor {
            Some((x, y)) => pt = POINT { x, y },
            None => {
                GetCursorPos(&mut pt);
            }
        }
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(m);
        cmd as usize
    };
    // Dynamic actions are assigned unique IDs when this particular menu is built. They retain
    // their row identity even if an event changes the catalogue during the modal menu loop.
    if let Some(action) = commands.get(cmd) {
        use crate::menu_commands::Action;
        match action {
            Action::Symbol(symbol) => {
                change_cards(hwnd, |cfg| toggle_kind(cfg, symbol));
            }
            Action::Rename(symbol) => rename_card(hwnd, symbol),
            Action::ResetName(symbol) => relabel_card(hwnd, symbol, None),
            Action::Digits(symbol, d) => {
                let changed = with_ui(|u| {
                    let price = u.view.cells.iter().zip(&u.cfg.rows).find(|(_, r)| &r.symbol == symbol).and_then(|(c, _)| c.quote.as_ref()).map(|q| q.price);
                    config::set_digits(&mut u.cfg, symbol, *d, price)
                })
                .unwrap_or(false);
                if changed {
                    apply_rows(hwnd);
                }
            }
            Action::Pin(symbol, on) => pin_card(hwnd, symbol, *on),
            Action::Remove(symbol) => remove_card(hwnd, symbol),
            Action::Move(symbol, delta) => reorder(hwnd, symbol, Some(*delta)),
            Action::MoveFirst(symbol) => reorder(hwnd, symbol, None),
            Action::Alert(label, k) => {
                let changed = with_ui(|u| {
                    let def = alerts::PRESETS.get(*k)?;
                    let cell = u.view.cells.iter().find(|c| &c.label == label)?;
                    let reference = alerts::reference_price(cell, &u.cfg, u.view.governed_s, crate::source::now_s())?;
                    let level = alerts::preset_level(def.preset, cell, reference)?;
                    if !u.cfg.alerts.iter().any(|a| &a.label == label) {
                        u.cfg.alerts.push(config::Alert { label: label.clone(), once: u.cfg.alert_once, crossing: u.cfg.alert_crossing, ..Default::default() });
                    }
                    let a = u.cfg.alerts.iter_mut().find(|a| &a.label == label)?;
                    alerts::apply_level(a, level);
                    Some(())
                }).flatten().is_some();
                if changed {
                    save_cfg();
                }
            }
            Action::Fast(label, k) => {
                let changed = with_ui(|u| {
                    if *k >= alerts::FAST_MOVES.len() || !u.cfg.rows.iter().any(|r| &r.label == label) {
                        return false;
                    }
                    if !u.cfg.alerts.iter().any(|a| &a.label == label) {
                        u.cfg.alerts.push(config::Alert {
                            label: label.clone(),
                            once: u.cfg.alert_once,
                            crossing: u.cfg.alert_crossing,
                            ..Default::default()
                        });
                    }
                    let Some(a) = u.cfg.alerts.iter_mut().find(|a| &a.label == label) else { return false };
                    // The current choice again turns it off. Either way the alert starts armed.
                    a.fast = if a.fast as usize == k + 1 { 0 } else { (k + 1) as u8 };
                    a.last = 0.0;
                    a.latched = 0;
                    true
                })
                .unwrap_or(false);
                if changed {
                    save_cfg();
                }
            }
            Action::ClearAlert(label) => {
                with_ui(|u| {
                    u.undo.put(undo::Kind::AlertClear, u.cfg.snapshot_alerts());
                    u.cfg.alerts.retain(|a| &a.label != label)
                });
                save_cfg();
            }
            Action::AlertAt(label) => alert_at_price(hwnd, label),
            Action::Copy(label) => {
                if let Some((text, _)) = with_ui(|u| card_target(&u.view.cells, &u.cfg.rows, label)).flatten() {
                    copy_text(&text);
                }
            }
            Action::Open(symbol) => open_cnbc(hwnd, symbol),
            Action::Forget(symbol) => {
                if with_ui(|u| config::forget_custom(&mut u.cfg, symbol)).unwrap_or(false) {
                    apply_rows(hwnd);
                }
            }
            Action::Monitor(device) => {
                // The modal menu pumps messages: use the current mode, not its opening snapshot.
                if with_ui(|u| u.mode == Mode::Strip).unwrap_or(false) {
                    with_ui(|u| u.cfg.monitor = device.clone());
                    apply_placement(hwnd);
                }
            }
        }
        return;
    }
    // The menu's own message loop ran the hotkey and the tray: what is chosen acts on the mode now,
    // not the one the menu was opened in (audit 1.8, finding 8).
    let now = with_ui(|u| u.mode).unwrap_or(if open { Mode::Panel } else { Mode::Strip });
    match cmd {
        CMD_PANEL => match panel_step(open, now) {
            Some(PanelStep::Collapse) => collapse(hwnd),
            Some(PanelStep::Expand) => expand(hwnd),
            None => {}
        },
        CMD_FALLBACK => {
            let Some(enabling) = with_ui(|u| !u.cfg.fallback_enabled) else { return; };
            if enabling {
                let message = wide("If CNBC is unavailable or a quote is stale, MH Ticker may request prices from Gold API or Kraken. Prices and update times can differ; Bitcoin may come from a different exchange. Some symbols have no fallback.\n\nYour device connects directly to these providers. Check the source and freshness shown before relying on a quote. Disabling stops new requests from being scheduled and clears fallback prices. Already-authorized requests may still connect or finish; their results are discarded. Kraken requires permission for non-personal commercial use of its data.\n\nAllow fallback sources?");
                let title = wide("Allow fallback sources?");
                let yes = unsafe { MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING) } == IDYES;
                if !yes { return; }
            }
            // The settings to write are read now, after the confirmation box: its modal loop runs the
            // feed (a fired alert updates its cooldown and may spend a once level) and the tray menu,
            // and a copy taken before it would write all of that back out of the file (audit 1.8,
            // finding 2).
            let Some(next) = with_ui(|u| fallback_settings(&u.cfg, enabling)) else { return; };
            // Refuse enable on persistence failure. Disable in memory first even when saving fails.
            if !enabling {
                with_ui(|u| {
                    u.cfg.fallback_enabled = false;
                    u.tip_at = None; // Force an immediate Explorer tooltip refresh.
                    if let Ok(mut b) = u.board.lock() { b.set_fallback(false); }
                });
                on_feed(hwnd);
            }
            match config::save_checked(&next) {
                Ok(()) => {
                    if enabling {
                        with_ui(|u| {
                            u.cfg.fallback_enabled = true;
                            if let Ok(mut b) = u.board.lock() { b.set_fallback(true); }
                        });
                        on_feed(hwnd);
                    }
                }
                Err(_) => {
                    let msg = if enabling { "Fallback was not enabled because settings could not be saved." }
                        else { "Fallback is off for this session, but settings could not be saved. It may be enabled again after restart." };
                    let text = wide(msg);
                    let title = wide("Could not save settings");
                    unsafe { MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR); }
                }
            }
        }
        CMD_STRIP => {
            with_ui(|u| u.cfg.strip_visible = !visible);
            save_cfg();
            apply_visibility(hwnd);
        }
        c if position_applies(open, now) && (CMD_POS..CMD_POS + Placement::ALL.len()).contains(&c) => {
            with_ui(|u| u.cfg.set_placement(Placement::ALL[c - CMD_POS]));
            apply_placement(hwnd);
        }
        CMD_TOP => {
            with_ui(|u| u.cfg.topmost = !top);
            save_cfg();
            set_topmost(hwnd, !top);
        }
        CMD_FOLDER => {
            if let Some(dir) = config::path().parent() {
                let d = wide(&dir.to_string_lossy());
                let op = wide("open");
                unsafe { ShellExecuteW(hwnd, op.as_ptr(), d.as_ptr(), null(), null(), SW_SHOWNORMAL) };
            }
        }
        CMD_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        CMD_RESET => {
            with_ui(|u| u.cfg.rows = config::default_rows());
            apply_rows(hwnd);
        }
        CMD_ADD_SYMBOL => add_symbol(hwnd),
        CMD_UNDO => undo_last(hwnd),
        CMD_PIN_ALL => {
            change_cards(hwnd, |cfg| (config::pin_all(cfg), Some(undo::Kind::Pin)));
        }
        CMD_ORDER_MOVE => order_cards(hwnd, config::OrderKey::DayMove),
        CMD_ORDER_NAME => order_cards(hwnd, config::OrderKey::Name),
        CMD_ORDER_DEFAULT => order_cards(hwnd, config::OrderKey::Default),
        c if (CMD_POLL..CMD_POLL + POLL_CHOICES.len()).contains(&c) => {
            let secs = POLL_CHOICES[c - CMD_POLL];
            with_ui(|u| {
                u.cfg.poll_s = secs;
                // The feed reads the cadence from the board, so this applies on the next tick.
                if let Ok(mut b) = u.board.lock() {
                    b.poll_s = secs;
                }
            });
            save_cfg();
        }
        c if ultra && (CMD_THEME..CMD_THEME + theme::ALL.len()).contains(&c) => {
            with_ui(|u| u.cfg.theme = theme::ALL[c - CMD_THEME].name.to_string());
            apply_theme(hwnd);
        }
        c if ultra && (CMD_FONT..CMD_FONT + theme::FACES.len()).contains(&c) => {
            with_ui(|u| u.cfg.theme_font = Some(theme::FACES[c - CMD_FONT].name.to_string()));
            apply_theme(hwnd);
        }
        CMD_REFRESH => {
            // Wakes the quote loop and makes chart rows with no bars due now. The backoff counter
            // is untouched: a still-failing source backs off again after this one attempt.
            // The range book is a separate machine: a long range that just failed would otherwise keep
            // saying so until its own 30 s back-off ends (audit 1.8, finding 1).
            with_ui(|u| {
                if let Ok(mut b) = u.board.lock() {
                    b.kick = b.kick.wrapping_add(1);
                }
                u.book.retry_now();
                want_range(u);
            });
        }
        CMD_COPY_ALL => {
            let text = with_ui(|u| crate::model::board_text(&u.view.cells));
            if let Some(text) = text.filter(|t| !t.is_empty()) {
                copy_text(&text);
            }
        }
        CMD_AUTOSTART => {
            let id = config::profile().id;
            if autostart::set(id, !autostart::enabled(id)).is_err() {
                let text = wide("Could not change the Start with Windows setting.");
                let title = wide("Start with Windows");
                unsafe { MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING) };
            }
        }
        CMD_UPDATE => start_update_check(hwnd),
        CMD_REPORT => start_report(hwnd),
        CMD_HOTKEY => set_hotkey(hwnd, None),
        c if (CMD_HOTKEY + 1..CMD_HOTKEY + 1 + hotkey::PRESETS.len()).contains(&c) => {
            set_hotkey(hwnd, Some(hotkey::PRESETS[c - CMD_HOTKEY - 1].to_string()));
        }
        // Choosing the configured combo again retries its registration.
        CMD_HOTKEY_CUSTOM => {
            let keep = with_ui(|u| u.cfg.hotkey.clone()).flatten();
            set_hotkey(hwnd, keep);
        }
        CMD_SLOW_QUIET | CMD_REST_LOCKED => {
            with_ui(|u| {
                let now = u.cfg.governor(u.profile.ultra);
                if cmd == CMD_SLOW_QUIET { u.cfg.slow_when_quiet = Some(!now.slow_when_quiet) } else { u.cfg.rest_when_locked = Some(!now.rest_when_locked) }
            });
            save_cfg();
        }
        c if (CMD_STRIP_ROWS..CMD_STRIP_ROWS + STRIP_ROWS.len()).contains(&c) => {
            with_ui(|u| u.cfg.dock_rows = STRIP_ROWS[c - CMD_STRIP_ROWS]);
            apply_strip(hwnd);
        }
        c if (CMD_STRIP_TEXT..CMD_STRIP_TEXT + TEXT_STEPS.len()).contains(&c) => {
            let (dock_pt, pt, _) = TEXT_STEPS[c - CMD_STRIP_TEXT];
            with_ui(|u| {
                u.cfg.dock_font_pt = dock_pt;
                u.cfg.font_pt = pt;
            });
            apply_strip(hwnd);
        }
        c if (CMD_STRIP_OPACITY..CMD_STRIP_OPACITY + OPACITY_STEPS.len()).contains(&c) => {
            with_ui(|u| u.cfg.opacity = OPACITY_STEPS[c - CMD_STRIP_OPACITY].0);
            apply_strip(hwnd);
        }
        CMD_PICK if ultra => apply_picker(hwnd, !picked),
        c if ultra && (CMD_ALERT_ONCE..=CMD_ALERT_LAST).contains(&c) => alert_command(hwnd, c),
        _ => {}
    }
}

/// The labels with an alert set, among the symbols the Alerts submenu lists (the shown ones). The
/// submenu's title counts these: an alert on a hidden symbol is kept (it fires again once the symbol
/// is shown) but is not listed, so counting it promised an entry the menu did not have.
fn armed_labels(alerts: &[config::Alert], shown_labels: &[&str]) -> Vec<String> {
    alerts
        .iter()
        .filter(|a| alerts::has_level(a))
        .filter(|a| shown_labels.contains(&a.label.as_str()))
        .map(|a| a.label.clone())
        .collect()
}

/// Where a morph's last frame goes. The morph runs on this thread and a monitor of another scale sends
/// `WM_DPICHANGED` in the middle of it, while the mode is `Morph` and the handler can lay nothing out
/// (audit 1.8, finding 9). `planned` was worked out before the morph at the old scale; if the scale
/// changed, `replan` works it out again at the new one.
fn settled_frame(dpi_before: u32, dpi_now: u32, planned: RECT, replan: impl FnOnce() -> RECT) -> RECT {
    if dpi_now == dpi_before {
        planned
    } else {
        replan()
    }
}

/// What the menu's *Expand view* / *Back to taskbar* entry does once chosen. `listed_open` is the mode
/// the entry was labelled for when the menu opened; `now` is the mode when it was chosen. The menu's
/// modal loop runs the hotkey and the tray, so the two can differ (audit 1.8, finding 8).
#[derive(Debug, PartialEq)]
enum PanelStep {
    Expand,
    Collapse,
}

fn panel_step(listed_open: bool, now: Mode) -> Option<PanelStep> {
    match (listed_open, now) {
        (false, Mode::Strip) => Some(PanelStep::Expand),
        (true, Mode::Panel) => Some(PanelStep::Collapse),
        _ => None,
    }
}

/// Whether a Position entry (listed only for a strip) may still be applied: the strip must still be the
/// window's mode, or the placement would be applied to an open panel.
fn position_applies(listed_open: bool, now: Mode) -> bool {
    !listed_open && now == Mode::Strip
}

/// Whether a fired alert lights the tray marker. The marker is the fallback for a balloon that Windows
/// held back (Focus Assist), so it exists for a user who is not looking. With the panel open and in
/// front the user is looking at the footer note that names the alert, and no later "expand" would
/// ever clear a marker lit now (audit 1.8, finding 6). A panel that is open behind another window, or
/// a strip, still lights it.
fn marks_tray(mode: Mode, panel_in_front: bool) -> bool {
    !(mode == Mode::Panel && panel_in_front)
}

/// What a card's *Copy* entry puts on the clipboard, and the symbol its *Open* entry opens, for the
/// card that was right-clicked, found by its label as the menu's other card entries find it by symbol:
/// the rows can change while the menu is open, so a position taken then would name another card.
fn card_target(cells: &[crate::model::Cell], rows: &[config::Row], label: &str) -> Option<(String, String)> {
    let cell = cells.iter().find(|c| c.label == label)?;
    let text = format!("{} {}", cell.label, fmt_price(cell.quote.as_ref()?.price, cell.digits));
    let symbol = rows.iter().find(|r| r.label == cell.label)?.symbol.clone();
    Some((text, symbol))
}

/// The settings *Allow fallback sources* writes: the configuration as it stands when the choice is
/// made (not as it stood when the menu opened), with only the fallback policy changed.
fn fallback_settings(current: &Config, enabling: bool) -> Config {
    let mut next = current.clone();
    next.fallback_enabled = enabling;
    next
}

/// What the Alerts submenu needs, read in one `with_ui` call so the menu build never nests.
#[derive(Default)]
struct AlertsView {
    /// UTC seconds now.
    now: f64,
    once: bool,
    crossing: bool,
    quiet: Option<[u16; 2]>,
    /// Whether alerts are silenced right now, and why.
    silence: Option<alerts::Silence>,
    /// The webhook's host (never the URL), when one is set.
    host: Option<String>,
    /// Newest first.
    log: Vec<alerts::Entry>,
    /// Everything that is set, with distances (`alerts::overview`).
    overview: Vec<alerts::OverviewLine>,
    gap_notice: bool,
    /// One per shown symbol.
    rows: Vec<AlertsRow>,
}

struct AlertsRow {
    label: String,
    digits: u32,
    /// Per `alerts::PRESETS` entry: can it be set now?
    enabled: Vec<bool>,
    /// There is a fresh price (`alerts::reference_price`): *at a price...* can be offered.
    fresh: bool,
    alert: Option<config::Alert>,
}

fn alerts_view(list: &[config::Row], shown: &[bool]) -> Option<AlertsView> {
    with_ui(|u| {
        let now = crate::source::now_s();
        let rows = list
            .iter()
            .zip(shown)
            .filter(|(_, on)| **on)
            .map(|(r, _)| {
                let cell = u.view.cells.iter().find(|c| c.label == r.label);
                let reference = cell.and_then(|c| alerts::reference_price(c, &u.cfg, u.view.governed_s, now));
                let enabled = alerts::PRESETS
                    .iter()
                    .map(|p| match (cell, reference) {
                        (Some(c), Some(r)) => alerts::preset_level(p.preset, c, r).is_some(),
                        _ => false,
                    })
                    .collect();
                AlertsRow {
                    label: r.label.clone(),
                    digits: r.digits,
                    enabled,
                    fresh: reference.is_some(),
                    alert: u.cfg.alerts.iter().find(|a| a.label == r.label).cloned(),
                }
            })
            .collect();
        AlertsView {
            now,
            once: u.cfg.alert_once,
            crossing: u.cfg.alert_crossing,
            quiet: u.cfg.quiet_hours,
            silence: alerts::mute_state(&u.cfg, now),
            host: u.cfg.webhook.as_deref().map(crate::webhook_host::redact),
            log: u.alert_log.newest_first(),
            overview: alerts::overview(&u.cfg, &u.view, now),
            gap_notice: u.cfg.gap_notice,
            rows,
        }
    })
}

/// Write the config as it now stands. Kept out of `with_ui` closures: no I/O inside them.
fn save_cfg() {
    if let Some(cfg) = with_ui(|u| {
        sync_gov(u);
        u.cfg.clone()
    }) {
        config::save(&cfg);
    }
}

fn alert_box(hwnd: HWND, title: &str, text: &str, flags: u32) -> i32 {
    let (t, x) = (wide(title), wide(text));
    unsafe { MessageBoxW(hwnd, x.as_ptr(), t.as_ptr(), flags) }
}

/// The fixed entries of the Alerts submenu (`CMD_ALERT_ONCE..=CMD_LOG_CLEAR`). No text here may
/// contain the webhook URL: messages name the host (`redact`) or nothing.
fn alert_command(hwnd: HWND, cmd: usize) {
    let now = crate::source::now_s();
    match cmd {
        CMD_ALERT_ONCE | CMD_REPEAT_COOLDOWN | CMD_REPEAT_CROSSING => {
            // One radio: the choice is both the default for new alerts and the mode of every alert set.
            with_ui(|u| {
                let (once, crossing) = (cmd == CMD_ALERT_ONCE, cmd == CMD_REPEAT_CROSSING);
                u.cfg.alert_once = once;
                u.cfg.alert_crossing = crossing;
                for a in &mut u.cfg.alerts {
                    a.once = once;
                    a.crossing = crossing;
                    a.latched = 0;
                }
            });
            save_cfg();
        }
        CMD_MUTE_1H | CMD_MUTE_8H | CMD_MUTE_INDEF | CMD_UNMUTE => {
            with_ui(|u| {
                let (muted, until) = match cmd {
                    CMD_MUTE_1H => (false, now + 3600.0),
                    CMD_MUTE_8H => (false, now + 8.0 * 3600.0),
                    CMD_MUTE_INDEF => (true, 0.0),
                    _ => (false, 0.0),
                };
                u.cfg.alerts_muted = muted;
                u.cfg.alerts_mute_until = until;
            });
            save_cfg();
        }
        CMD_QUIET_OFF => {
            with_ui(|u| u.cfg.quiet_hours = None);
            save_cfg();
        }
        c if (CMD_QUIET..CMD_QUIET + alerts::QUIET_CHOICES.len()).contains(&c) => {
            with_ui(|u| u.cfg.quiet_hours = Some(alerts::QUIET_CHOICES[c - CMD_QUIET]));
            save_cfg();
        }
        CMD_HOOK_PASTE => {
            // Read on this click only, validated, confirmed by host, then saved.
            let Some(text) = clipboard_text() else {
                alert_box(hwnd, "Webhook", "The clipboard holds no text.", MB_OK | MB_ICONINFORMATION);
                return;
            };
            match alerts::accept_webhook(&text) {
                Ok(url) => {
                    let host = crate::webhook_host::redact(&url);
                    let ask = format!("Send alerts to {host}?\n\nOnly the host is shown here; the rest of the address stays private.");
                    if alert_box(hwnd, "Use this webhook?", &ask, MB_YESNO | MB_ICONQUESTION) == IDYES {
                        with_ui(|u| u.cfg.webhook = Some(url));
                        save_cfg();
                    }
                }
                Err(why) => {
                    alert_box(hwnd, "Webhook", why, MB_OK | MB_ICONINFORMATION);
                }
            }
        }
        CMD_HOOK_TEST => {
            // An explicit click, so it works while muted. One in flight: a second click inside
            // `TEST_ALERT_GAP_S` is refused.
            let Some(url) = with_ui(|u| u.cfg.webhook.clone()).flatten() else { return };
            let host = crate::webhook_host::redact(&url);
            let seq = with_ui(|u| {
                if u.test_alert_at.is_some_and(|t| t.elapsed().as_secs() < TEST_ALERT_GAP_S) {
                    u.alert_note = Some("test alert: the last one is still being sent".into());
                    return None;
                }
                u.test_alert_at = Some(Instant::now());
                u.alert_note = Some(format!("test alert: sending to {host}"));
                Some(u.alert_log.push(now, alerts::TEST_TITLE.into(), alerts::Outcome::Shown))
            })
            .flatten();
            if let Some(seq) = seq {
                post_alert(hwnd, url, alerts::TEST_TITLE.into(), "If you can read this, the webhook works.".into(), seq, true);
            }
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
        CMD_HOOK_REMOVE => {
            with_ui(|u| u.cfg.webhook = None);
            save_cfg();
        }
        CMD_LOG_CLEAR => {
            with_ui(|u| u.alert_log.clear());
        }
        CMD_LOG_COPY => {
            // Clipboard I/O after the borrow is released.
            let text = with_ui(|u| alerts::log_text(&u.alert_log.newest_first(), &local_offset_at, now));
            if let Some(text) = text.filter(|t| !t.is_empty()) {
                copy_text(&text);
            }
        }
        CMD_GAP_NOTICE => {
            with_ui(|u| u.cfg.gap_notice = !u.cfg.gap_notice);
            save_cfg();
        }
        CMD_ALERTS_CLEAR_ALL => {
            let n = with_ui(|u| u.cfg.alerts.iter().filter(|a| alerts::has_level(a)).count()).unwrap_or(0);
            if n == 0 {
                return;
            }
            let ask = format!(
                "Clear all {n} alert{}?\n\nEvery level, day move and fast move you have set is removed. The Recent list stays.",
                if n == 1 { "" } else { "s" }
            );
            if alert_box(hwnd, "Clear all alerts", &ask, MB_YESNO | MB_ICONQUESTION) == IDYES {
                // The box ran the message loop: act on what is set now, not on the count above.
                with_ui(|u| {
                    u.undo.put(undo::Kind::AlertClear, u.cfg.snapshot_alerts());
                    u.cfg.alerts.clear()
                });
                save_cfg();
            }
        }
        _ => {}
    }
}

/// (Re)register the configured global hotkey. Always unregisters first, so it is safe to call at
/// any time and when nothing was registered. Records the outcome in `Ui::hotkey_failed`: true when
/// `cfg.hotkey` is set and either does not parse or `RegisterHotKey` refused it (the combo belongs
/// to another app); false when it is unset or registered. Uses `MOD_NOREPEAT`.
fn register_hotkey(hwnd: HWND) {
    let want = with_ui(|u| u.cfg.hotkey.clone()).flatten();
    unsafe { UnregisterHotKey(hwnd, HOTKEY_ID) };
    let failed = match want {
        None => false,
        Some(text) => match hotkey::parse(&text) {
            Some(c) => (unsafe { RegisterHotKey(hwnd, HOTKEY_ID, c.mods | hotkey::MOD_NOREPEAT, c.vk) }) == 0,
            None => true,
        },
    };
    with_ui(|u| u.hotkey_failed = failed);
}

/// The Hotkey submenu changed the combo: store it, save, register it.
fn set_hotkey(hwnd: HWND, combo: Option<String>) {
    let cfg = with_ui(|u| {
        u.cfg.hotkey = combo;
        u.cfg.clone()
    });
    if let Some(cfg) = cfg {
        config::save(&cfg);
    }
    register_hotkey(hwnd);
}

/// The Strip submenu changed `dock_rows`, the text size or the opacity (already in `cfg`): save,
/// rebuild the strip font and re-lay-out like `apply_placement` does.
///
/// Opacity: `expand` fades the window from `cfg.opacity` up to 255, and `collapse` reads
/// `cfg.opacity` again when it fades back down, so while the panel is open the window must stay
/// at 255 and the new value is simply picked up on collapse. The layered alpha is therefore set
/// here only in `Mode::Strip`; setting it in `Mode::Panel` would make the panel see-through, and
/// `Mode::Morph` cannot be observed (morphs block this thread).
fn apply_strip(hwnd: HWND) {
    let Some((cfg, strip)) = with_ui(|u| {
        unsafe { DeleteObject(u.font) };
        u.font = make_font(&u.cfg, u.dpi);
        (u.cfg.clone(), u.mode == Mode::Strip)
    }) else {
        return;
    };
    config::save(&cfg);
    if strip {
        unsafe { SetLayeredWindowAttributes(hwnd, 0, cfg.opacity, LWA_ALPHA) };
        relayout_and_place(hwnd);
    }
}

/// *Check for updates*: one GET on a worker thread, refused while another is running. The worker
/// leaves an `update::Outcome` in `Ui::update_result` and posts `WM_UPDATE`. A failed spawn takes
/// the same road (a `Failed` outcome posted from here), so the flag always gets cleared.
fn start_update_check(hwnd: HWND) {
    let Some(slot) = with_ui(|u| {
        if u.update_running {
            None
        } else {
            u.update_running = true;
            Some(u.update_result.clone())
        }
    })
    .flatten() else {
        return;
    };
    let target = hwnd as isize;
    let worker_slot = slot.clone();
    let spawned = std::thread::Builder::new().name("update".into()).spawn(move || {
        let outcome = match crate::source::get_release(update::API_URL) {
            Ok(body) => update::decide(env!("CARGO_PKG_VERSION"), &body),
            Err(e) => update::Outcome::Failed(e),
        };
        if let Ok(mut g) = worker_slot.lock() {
            *g = Some(outcome);
        }
        unsafe { PostMessageW(target as HWND, WM_UPDATE, 0, 0) };
    });
    if spawned.is_err() {
        if let Ok(mut g) = slot.lock() {
            *g = Some(update::Outcome::Failed("could not start".into()));
        }
        unsafe { PostMessageW(hwnd, WM_UPDATE, 0, 0) };
    }
}

/// `WM_UPDATE`: show the outcome in a message box. "Open the download page" opens the constant
/// `update::RELEASES_URL` and nothing from the response. `update_running` is cleared last, after
/// the box closes, so a second check cannot start underneath it.
fn on_update(hwnd: HWND) {
    let outcome = with_ui(|u| u.update_result.lock().ok().and_then(|mut g| g.take())).flatten()
        .unwrap_or_else(|| update::Outcome::Failed("no answer".into()));
    let title = wide("Check for updates");
    let current = env!("CARGO_PKG_VERSION");
    unsafe {
        match outcome {
            update::Outcome::UpToDate => {
                let text = wide(&format!("MH Ticker {current} is the latest version."));
                MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
            }
            update::Outcome::Newer(tag) => {
                let tag: String = tag.chars().filter(|c| !c.is_control()).take(40).collect();
                let text = wide(&format!("Version {tag} is available. You have {current}.\n\nOpen the download page?"));
                if MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONINFORMATION) == IDYES {
                    let url = wide(update::RELEASES_URL);
                    let op = wide("open");
                    ShellExecuteW(hwnd, op.as_ptr(), url.as_ptr(), null(), null(), SW_SHOWNORMAL);
                }
            }
            update::Outcome::Failed(why) => {
                let why: String = why.chars().filter(|c| !c.is_control()).take(40).collect();
                let text = wide(&format!("Could not check for updates ({why}). Try again later."));
                MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING);
            }
        }
    }
    with_ui(|u| u.update_running = false);
}

/// *Copy diagnostics report*: the probe's checks on a worker thread, then the report as text on the
/// clipboard. One at a time (a second click while it runs does nothing). The facts are read here, in
/// one borrow and with no I/O; the worker fetches (the quotes of every shown symbol, one unknown
/// symbol, the first row's 1D and range files, the release answer: the same hosts and the same
/// requests as `--probe`, about 50 to 250 KB), judges, builds the text and posts `WM_REPORT`. A failed
/// spawn posts it too, so the flag always gets cleared.
fn start_report(hwnd: HWND) {
    // The registry read is I/O: done before the borrow.
    let autostart_on = autostart::enabled(config::profile().id);
    let Some((slot, rows, mut facts)) = with_ui(|u| {
        if u.report_running {
            return None;
        }
        u.report_running = true;
        let facts = report::Facts {
            version: env!("CARGO_PKG_VERSION").to_string(),
            build: if u.profile.ultra { "Ultra" } else { "Classic" }.to_string(),
            dpi: u.dpi,
            placement: u.cfg.placement().label().to_string(),
            mode: if u.mode == Mode::Panel { "panel" } else { "strip" }.to_string(),
            symbols: u.cfg.rows.len(),
            alerts: u.cfg.alerts.len(),
            hotkey: u.cfg.hotkey.clone(),
            hotkey_failed: u.hotkey_failed,
            autostart: autostart_on,
            settings_path: config::path().display().to_string(),
            user: std::env::var("USERNAME").ok(),
            save_failed: config::save_failed(),
            kept_config: config::kept_bad_config(),
            webhook: u.cfg.webhook.clone(),
            fallback: u.cfg.fallback_enabled,
            poll_s: u.cfg.poll_s,
            stale_after_s: u.cfg.stale_after_s,
            meter: String::new(),
            findings: Vec::new(),
        };
        Some((u.report_result.clone(), u.cfg.rows.clone(), facts))
    })
    .flatten() else {
        return;
    };
    let target = hwnd as isize;
    let worker_slot = slot.clone();
    let spawned = std::thread::Builder::new().name("report".into()).spawn(move || {
        let bodies = crate::source::collect_bodies(&rows, 1, env!("CARGO_PKG_VERSION"));
        facts.findings = probe::judge(&bodies);
        // Read last, so the meter includes what these checks just fetched.
        facts.meter = crate::source::meter_line();
        let warns = facts.findings.iter().filter(|f| f.verdict == probe::Verdict::Warn).count();
        if let Ok(mut g) = worker_slot.lock() {
            *g = Some((report::text(&facts), warns));
        }
        unsafe { PostMessageW(target as HWND, WM_REPORT, 0, 0) };
    });
    if spawned.is_err() {
        unsafe { PostMessageW(hwnd, WM_REPORT, 0, 0) };
    }
}

/// `WM_REPORT`: put the report on the clipboard and say so. `report_running` is cleared last, after
/// the box closes, so a second report cannot start underneath it.
fn on_report(hwnd: HWND) {
    let result = with_ui(|u| u.report_result.lock().ok().and_then(|mut g| g.take())).flatten();
    match result {
        Some((text, warns)) => {
            copy_text(&text);
            let msg = if warns == 0 {
                "The diagnostics report is on the clipboard. Every check passed or was a note. Paste it into a message."
                    .to_string()
            } else {
                format!("The diagnostics report is on the clipboard. {warns} check{} warned. Paste it into a message.", if warns == 1 { "" } else { "s" })
            };
            alert_box(hwnd, "Diagnostics report", &msg, MB_OK | MB_ICONINFORMATION);
        }
        None => {
            alert_box(hwnd, "Diagnostics report", "The report could not be made. Try again later.", MB_OK | MB_ICONWARNING);
        }
    }
    with_ui(|u| u.report_running = false);
}

/// What *Add symbol...* does with a cleaned symbol, decided in one `with_ui` call.
enum AddStep {
    /// On the strip already.
    Shown,
    /// In the Symbols menu and now ticked.
    Ticked,
    /// In the Symbols menu, but the strip is full.
    Full,
    /// A lookup is in flight already.
    Busy,
    /// Look it up; the slot the worker writes.
    Look(Arc<Mutex<Option<(String, crate::source::Lookup)>>>),
}

/// Text from the network or the keyboard, made safe for a message box: no control characters, at most
/// `n` characters.
fn shown_text(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// Symbols > *Add symbol...* (both builds). Asks for a symbol (outside `with_ui`), cleans it, ticks it
/// when it is already listed, and otherwise looks it up on a worker thread: `adding` is set first and
/// `WM_LOOKUP` comes back in every outcome, a failed thread start included, so it is always cleared.
fn add_symbol(hwnd: HWND) {
    if with_ui(|u| u.adding.is_some()).unwrap_or(true) {
        return;
    }
    let title = "Add symbol";
    let Some(text) = prompt::ask(hwnd, title, "CNBC symbol to add (for example AAPL, @GC.1 or EUR=):", "", SYMBOL_PROMPT_LEN) else {
        return;
    };
    let Some(symbol) = config::clean_symbol(&text) else {
        let typed = shown_text(text.trim(), 40);
        let msg = format!("\"{typed}\" is not a CNBC symbol. Use 1 to {} letters, digits or . @ = - ^ / : _", config::MAX_SYMBOL);
        alert_box(hwnd, title, &msg, MB_OK | MB_ICONWARNING);
        return;
    };
    let step = with_ui(|u| {
        if config::is_shown(&u.cfg, &symbol) {
            AddStep::Shown
        } else if config::menu_list(&u.cfg).iter().any(|r| r.symbol == symbol) {
            if config::toggle(&mut u.cfg, &symbol) { AddStep::Ticked } else { AddStep::Full }
        } else if u.adding.is_some() {
            AddStep::Busy
        } else {
            u.adding = Some(symbol.clone());
            AddStep::Look(u.lookup_result.clone())
        }
    });
    match step {
        Some(AddStep::Ticked) => apply_rows(hwnd),
        Some(AddStep::Shown) => {
            alert_box(hwnd, title, &format!("{symbol} is already on the strip."), MB_OK | MB_ICONINFORMATION);
        }
        Some(AddStep::Full) => {
            let msg = format!("{symbol} is in the Symbols menu, but the strip already shows {} symbols. Untick one first.", config::MAX_ROWS);
            alert_box(hwnd, title, &msg, MB_OK | MB_ICONINFORMATION);
        }
        Some(AddStep::Look(slot)) => start_lookup(hwnd, symbol, slot),
        Some(AddStep::Busy) => {
            // A second prompt (opened from the tray while the first was up) can start its lookup first;
            // the symbol typed here is then not dropped without a word (audit 1.8, finding 7).
            let msg = format!("{symbol} was not added: another symbol is being looked up. Try again in a few seconds.");
            alert_box(hwnd, title, &msg, MB_OK | MB_ICONINFORMATION);
        }
        None => {}
    }
}

/// One `source::lookup` on its own thread; the outcome goes into `slot` and `WM_LOOKUP` is posted. A
/// failed spawn takes the same road from here, so `on_lookup` always runs and clears `Ui::adding`.
fn start_lookup(hwnd: HWND, symbol: String, slot: Arc<Mutex<Option<(String, crate::source::Lookup)>>>) {
    let target = hwnd as isize;
    let (worker_slot, worker_symbol) = (slot.clone(), symbol.clone());
    let spawned = std::thread::Builder::new().name("lookup".into()).spawn(move || {
        let outcome = crate::source::lookup(&worker_symbol);
        if let Ok(mut g) = worker_slot.lock() {
            *g = Some((worker_symbol, outcome));
        }
        unsafe { PostMessageW(target as HWND, WM_LOOKUP, 0, 0) };
    });
    if spawned.is_err() {
        if let Ok(mut g) = slot.lock() {
            *g = Some((symbol, crate::source::Lookup::Failed("could not start".into())));
        }
        unsafe { PostMessageW(hwnd, WM_LOOKUP, 0, 0) };
    }
}

/// `WM_LOOKUP`: clear `adding` first (so the menu is usable again even while a box is up), then act on
/// the outcome: found adds it to the catalogue and ticks it when there is room; not found and failed
/// say which in a message box. A message with no outcome in the slot only clears `adding`.
fn on_lookup(hwnd: HWND) {
    let taken = with_ui(|u| {
        u.adding = None;
        u.lookup_result.lock().ok().and_then(|mut g| g.take())
    })
    .flatten();
    let Some((symbol, outcome)) = taken else { return };
    let title = "Add symbol";
    match outcome {
        crate::source::Lookup::Found { name, price } => match with_ui(|u| config::add_custom(&mut u.cfg, &symbol, &name, price)) {
            Some(config::Added::Shown) => apply_rows(hwnd),
            Some(config::Added::Listed) => {
                apply_rows(hwnd);
                let msg = format!("{symbol} is in the Symbols menu now, but the strip already shows {} symbols. Untick one to show it.", config::MAX_ROWS);
                alert_box(hwnd, title, &msg, MB_OK | MB_ICONINFORMATION);
            }
            Some(config::Added::AlreadyShown) | None => {}
        },
        crate::source::Lookup::NotFound => {
            let msg = format!("CNBC has no price for {symbol}. Check the symbol on cnbc.com; nothing was added.");
            alert_box(hwnd, title, &msg, MB_OK | MB_ICONWARNING);
        }
        crate::source::Lookup::Failed(why) => {
            let msg = format!("Could not reach CNBC ({}). {symbol} was not added; try again later.", shown_text(&why, 40));
            alert_box(hwnd, title, &msg, MB_OK | MB_ICONWARNING);
        }
    }
}

/// The fresh price of the card labelled `label` and its digits, or `None` when there is no fresh
/// CNBC price (`alerts::reference_price`).
fn fresh_price(label: &str) -> Option<(f64, u32)> {
    with_ui(|u| {
        let cell = u.view.cells.iter().find(|c| c.label == label)?;
        Some((alerts::reference_price(cell, &u.cfg, u.view.governed_s, crate::source::now_s())?, cell.digits))
    })
    .flatten()
}

/// Set `level` on `label`'s alert, creating the alert (with the *Fire once* setting) if it has none.
fn put_level(u: &mut Ui, label: &str, level: alerts::Level) {
    if !u.cfg.alerts.iter().any(|a| a.label == label) {
        u.cfg.alerts.push(config::Alert { label: label.to_string(), once: u.cfg.alert_once, crossing: u.cfg.alert_crossing, ..Default::default() });
    }
    if let Some(a) = u.cfg.alerts.iter_mut().find(|a| a.label == label) {
        alerts::apply_level(a, level);
    }
}

/// Apply the side rule to `level` against the fresh price as it is NOW (not when a menu or prompt
/// opened), then set it and save; a refusal says why in a message box.
fn set_price_level(hwnd: HWND, label: &str, level: f64) {
    let title = format!("Alert for {label}");
    let Some((reference, digits)) = fresh_price(label) else {
        alert_box(hwnd, &title, &format!("No alert set: there is no fresh price for {label} right now."), MB_OK | MB_ICONWARNING);
        return;
    };
    match alerts::side_of(level, reference, digits) {
        Ok(l) => {
            with_ui(|u| put_level(u, label, l));
            save_cfg();
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        Err(why) => {
            alert_box(hwnd, &title, &format!("No alert set: {why}."), MB_OK | MB_ICONWARNING);
        }
    }
}

/// Alerts > symbol > *at a price...* (Ultra): the prompt opens on the current price; the typed text
/// must parse (`alerts::parse_level`), then the side rule decides above or below.
fn alert_at_price(hwnd: HWND, label: &str) {
    let Some((reference, digits)) = fresh_price(label) else { return };
    let now = fmt_price(reference, digits);
    let title = format!("Alert for {label}");
    let text = format!("Alert when {label} reaches this price (now {now}):");
    let Some(typed) = prompt::ask(hwnd, &title, &text, &now, LEVEL_PROMPT_LEN) else { return };
    match alerts::parse_level_why(&typed) {
        Ok(level) => {
            // A level far from the price is read back in the symbol's digits and asked about first.
            if let Some(q) = alerts::far_level_question(label, level, reference, digits) {
                if alert_box(hwnd, &title, &q, MB_YESNO | MB_ICONQUESTION) != IDYES {
                    return;
                }
            }
            set_price_level(hwnd, label, level)
        }
        Err(why) => {
            let msg = format!("\"{}\": {why}. Type a number such as 4200 or 4,200.50.", shown_text(typed.trim(), 32));
            alert_box(hwnd, &title, &msg, MB_OK | MB_ICONWARNING);
        }
    }
}

/// The candle pane's right-click menu (Ultra, panel open, pane drawing candles, `at` inside the price
/// plot of the stored layout): *Alert at <price under the pointer>*, rounded to the symbol's digits and
/// greyed when there is no fresh price or it is the current price; *Clear <label> alerts* when it has
/// any; and *More...*, the main menu. Returns false, having shown nothing, anywhere else, and the caller
/// opens the main menu. Separate from `menu_at` on purpose: it is a menu about one point of one chart.
fn pane_menu(hwnd: HWND, at: (i32, i32)) -> bool {
    let (x, y) = at;
    let Some((label, digits, price, alerted)) = with_ui(|u| {
        if !u.profile.ultra || !u.chart || u.mode != Mode::Panel {
            return None;
        }
        let lay = u.chart_layout.get()?;
        if lay.bars == 0 || !lay.plot.contains(x, y) {
            return None;
        }
        let price = lay.price_at(y)?;
        let cell = u.view.cells.get(selected_row(u)?)?;
        let alerted = u.cfg.alerts.iter().any(|a| a.label == cell.label && (a.above.is_some() || a.below.is_some() || a.day_move.is_some()));
        Some((cell.label.clone(), cell.digits, price, alerted))
    })
    .flatten() else {
        return false;
    };
    clear_pending(hwnd);
    let level = alerts::round_level(price, digits);
    let settable = fresh_price(&label).is_some_and(|(reference, d)| alerts::side_of(level, reference, d).is_ok());
    let cmd = unsafe {
        let m = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            let t = wide(text);
            AppendMenuW(m, flags, id, t.as_ptr());
        };
        add(MF_STRING | if settable { 0 } else { MF_GRAYED }, CMD_PANE_ALERT, &format!("Alert at {}", fmt_price(level, digits)));
        if alerted {
            add(MF_STRING, CMD_PANE_CLEAR, &format!("Clear {label} alerts"));
        }
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        add(MF_STRING, CMD_PANE_MENU, "More...");
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(m);
        cmd as usize
    };
    match cmd {
        CMD_PANE_ALERT => set_price_level(hwnd, &label, level),
        CMD_PANE_CLEAR => {
            with_ui(|u| {
                u.undo.put(undo::Kind::AlertClear, u.cfg.snapshot_alerts());
                u.cfg.alerts.retain(|a| a.label != label)
            });
            save_cfg();
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        CMD_PANE_MENU => menu(hwnd, Some(at)),
        _ => {}
    }
    true
}

/// The Position menu moved the strip: new font (the taskbar grid uses `dock_font_pt`), topmost
/// state (forced on the taskbar), place, show.
fn apply_placement(hwnd: HWND) {
    let top = with_ui(|u| {
        unsafe { DeleteObject(u.font) };
        u.font = make_font(&u.cfg, u.dpi);
        u.placed = false;
        u.suppressed = false;
        u.cfg.topmost || u.cfg.placement().on_taskbar()
    })
    .unwrap_or(true);
    save_cfg();
    set_topmost(hwnd, top);
    relayout_and_place(hwnd);
    apply_visibility(hwnd);
    dock_check(hwnd);
}

/// The Symbols menu changed `cfg.rows`: save it, hand the list to the feed through the board, and
/// re-fit the strip, or resize the open panel in place (it stays centred).
fn apply_rows(hwnd: HWND) {
    save_cfg();
    let Some(panel) = with_ui(|u| {
        // The cards changed. An undoable action puts its snapshot back after this call; any other change
        // makes a held snapshot of the cards stale.
        u.undo.drop_rows();
        if let Ok(mut b) = u.board.lock() {
            b.set_rows(&u.cfg.rows);
            u.view = b.clone();
        }
        u.hist.record(&u.view, crate::source::now_s());
        u.chart_at = None;
        u.chart_layout.set(None);
        u.open_hover = false;
        if u.profile.ultra {
            let labels: Vec<&str> = u.view.cells.iter().map(|c| c.label.as_str()).collect();
            u.selected = settle_lock(u.selected.as_deref(), &labels, u.chart);
            // Series of symbols that left the strip go, in-flight fetches for them are dropped when
            // they arrive, and a request still waiting for the worker is taken back.
            let shown: Vec<&str> = u.cfg.rows.iter().map(|r| r.symbol.as_str()).collect();
            u.book.forget(&shown);
            // The request taken back is cancelled in the book too: if its symbol is still shown, the
            // key would otherwise count as in flight (and the pane say "loading") until the book gave
            // it up for lost. `want_range` below asks again if it is still what the pane needs.
            let waiting = u.range_slots.lock().ok().and_then(|mut s| s.clear_request());
            if let Some(old) = waiting {
                u.book.cancel(old.generation);
            }
            want_range(u);
        }
        u.mode == Mode::Panel
    }) else {
        return;
    };
    if panel { unsafe { InvalidateRect(hwnd, null(), 0); } }
    else { relayout_and_place(hwnd); }
    unsafe { PostMessageW(hwnd, WM_FEED, 0, 0) };
}

/// *Rename...*: ask for a label, then `relabel_card`. The prompt runs outside `with_ui`.
fn rename_card(hwnd: HWND, symbol: &str) {
    let Some(current) = with_ui(|u| u.cfg.rows.iter().find(|r| r.symbol == symbol).map(|r| r.label.clone())).flatten() else {
        return;
    };
    let ask = format!("Name for {symbol} (up to {} characters, a taken name gets a number):", config::MAX_LABEL);
    let Some(text) = prompt::ask(hwnd, "Rename card", &ask, &current, config::MAX_LABEL) else {
        return;
    };
    relabel_card(hwnd, symbol, Some(text));
}

/// Give a card a new label (`Some(typed)`) or its derived one (`None`). The alerts and the lock follow
/// in the same step (`config::rename` moves the alerts; the lock is moved here), so nothing is keyed by
/// a label that no longer exists.
fn relabel_card(hwnd: HWND, symbol: &str, typed: Option<String>) {
    let changed = with_ui(|u| {
        let old = u.cfg.rows.iter().find(|r| r.symbol == symbol).map(|r| r.label.clone())?;
        let new = match &typed {
            Some(t) => config::rename(&mut u.cfg, symbol, t),
            None => config::reset_name(&mut u.cfg, symbol),
        }?;
        if u.selected.as_deref() == Some(old.as_str()) {
            u.selected = Some(new);
        }
        Some(())
    })
    .flatten()
    .is_some();
    if changed {
        apply_rows(hwnd);
    }
}

/// *Pin to strip* / *Unpin from strip* for one card. The config refuses to unpin the last pinned card.
fn pin_card(hwnd: HWND, symbol: &str, on: bool) {
    change_cards(hwnd, |cfg| (config::set_pin(cfg, symbol, on), Some(undo::Kind::Pin)));
}

/// Rewrite the order of the shown cards once (*Order the cards*). The day moves are read from the board
/// as it is now, fresh quotes only; a card with none keeps its relative place at the end. The lock is by
/// label, so it follows its card, and the panel scrolls to keep it in view.
fn order_cards(hwnd: HWND, key: config::OrderKey) {
    let moves: std::collections::HashMap<String, f64> = with_ui(|u| {
        u.view
            .cells
            .iter()
            .zip(&u.cfg.rows)
            .filter(|(c, _)| !c.stale)
            .filter_map(|(c, r)| Some((r.symbol.clone(), c.quote.as_ref()?.change_pct?)))
            .collect()
    })
    .unwrap_or_default();
    if change_cards(hwnd, |cfg| (config::order_by(cfg, &moves, key), Some(undo::Kind::Sort))) {
        reveal_lock(hwnd);
    }
}

/// Hide `symbol` (the card menu's Remove and the Delete key). The lock passes to the card that takes the
/// removed one's place when the locked card is the one going. A symbol that is no longer shown, or the
/// last one, changes nothing.
fn remove_card(hwnd: HWND, symbol: &str) {
    let removed = with_ui(|u| {
        let label = u.cfg.rows.iter().find(|r| r.symbol == symbol).map(|r| r.label.clone());
        let next = label.as_deref().and_then(|l| neighbour_of(&u.view.cells, l));
        let snap = u.cfg.snapshot_rows();
        if !config::remove(&mut u.cfg, symbol) {
            return None;
        }
        if label.is_some() && u.selected == label {
            lock_card(u, next);
        }
        Some(snap)
    })
    .flatten();
    if let Some(snap) = removed {
        apply_rows(hwnd);
        keep_undo(undo::Kind::Remove, snap);
        reveal_lock(hwnd);
    }
}

/// Move `symbol` along the strip: `Some(delta)` places, or `None` for the front. The saved order is
/// `config`'s business; this applies it, then keeps the locked card in view.
fn reorder(hwnd: HWND, symbol: &str, delta: Option<i32>) {
    let changed = change_cards(hwnd, |cfg| {
        let did = match delta {
            Some(d) => config::move_row(cfg, symbol, d),
            None => config::move_first(cfg, symbol),
        };
        (did, Some(undo::Kind::Move))
    });
    if changed {
        reveal_lock(hwnd);
    }
}

/// `config::toggle` as a `change_cards` step: hiding a symbol can be undone, showing one cannot (it is
/// not a removal, and undo would put the old list back over it).
fn toggle_kind(cfg: &mut Config, symbol: &str) -> (bool, Option<undo::Kind>) {
    let was_shown = config::is_shown(cfg, symbol);
    (config::toggle(cfg, symbol), was_shown.then_some(undo::Kind::Remove))
}

/// Hold `snap` as the one undo snapshot. Called after `apply_rows`, which drops a stale one.
fn keep_undo(kind: undo::Kind, snap: config::Snapshot) {
    with_ui(|u| u.undo.put(kind, snap));
}

/// Change the cards and apply them. `f` makes the change and answers whether it changed anything and
/// which kind of undo it is (`None`: a change undo does not cover, which also makes a held snapshot of
/// the cards stale). The snapshot is taken before `f` and kept after the rows are applied.
fn change_cards(hwnd: HWND, f: impl FnOnce(&mut Config) -> (bool, Option<undo::Kind>)) -> bool {
    let done = with_ui(|u| {
        let snap = u.cfg.snapshot_rows();
        let (changed, kind) = f(&mut u.cfg);
        changed.then_some((snap, kind))
    })
    .flatten();
    let Some((snap, kind)) = done else { return false };
    apply_rows(hwnd);
    if let Some(kind) = kind {
        keep_undo(kind, snap);
    }
    true
}

/// *Undo* (the menu line and Ctrl+Z): put the held snapshot back, once. Cards go through `apply_rows`;
/// alerts are merged back and saved. The webhook is not in a snapshot, so it is never touched.
fn undo_last(hwnd: HWND) {
    let Some((kind, snap)) = with_ui(|u| u.undo.take()).flatten() else { return };
    with_ui(|u| u.cfg.restore(snap));
    if kind.is_rows() {
        apply_rows(hwnd);
        reveal_lock(hwnd);
    } else {
        save_cfg();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

/// Home and End on Ultra: lock the first or last card and show it.
fn lock_edge(hwnd: HWND, last: bool) {
    with_ui(|u| {
        let label = if last { u.view.cells.last() } else { u.view.cells.first() }.map(|c| c.label.clone());
        lock_card(u, label);
        u.hover_at = None;
    });
    reveal_lock(hwnd);
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

/// `/`: ask for a few letters, then lock the first card they match (`find::find`) and scroll to it.
/// Cancel, empty text and a text nothing matches change nothing. The prompt runs outside `with_ui`.
/// What the system does with the keyboard focus once the prompt closes is not verified.
fn find_card(hwnd: HWND) {
    let Some(text) = prompt::ask(hwnd, "Find a card", "Find a card by label or name:", "", config::MAX_NAME) else {
        return;
    };
    let found = with_ui(|u| {
        let cards: Vec<(&str, &str)> = u
            .view
            .cells
            .iter()
            .map(|c| (c.label.as_str(), u.cfg.rows.iter().find(|r| r.label == c.label).map_or("", |r| r.name.as_str())))
            .collect();
        let i = find::find(&cards, &text)?;
        let label = u.view.cells.get(i)?.label.clone();
        lock_card(u, Some(label));
        u.hover_at = None;
        Some(())
    })
    .flatten();
    if found.is_some() {
        reveal_lock(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

/// Scroll the body just far enough that the locked card is on screen, and repaint.
fn reveal_lock(hwnd: HWND) {
    let moved = with_ui(|u| {
        if u.mode != Mode::Panel {
            return false;
        }
        let Some(i) = selected_row(u) else { return false };
        let want = panel::reveal(&panel_geo(u), i);
        let moved = want != u.picker_scroll;
        u.picker_scroll = want;
        if moved {
            u.hover_at = None;
        }
        moved
    })
    .unwrap_or(false);
    if moved {
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

/// A key press in the open panel (`keys::is_bound` keys only). Returns whether it did something, so
/// the caller can hand an unmapped key back to the system. State is decided inside one `with_ui`
/// call; the clipboard, the browser, the config save and the chart/picker changes (which re-enter
/// `with_ui`) all happen after it. Keys act only in `Mode::Panel`: a strip has no keyboard, and a
/// morph in flight ignores them just as it ignores a click.
fn on_key(hwnd: HWND, vk: u32) -> bool {
    enum Do {
        Redraw,
        Open(String),
        Chart(bool),
        Picker(bool),
        Copy(String),
        Remove(String),
        Shift(String, i32),
        Undo,
    }
    let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
    let todo = with_ui(|u| {
        if u.mode != Mode::Panel {
            return None;
        }
        let row = selected_row(u);
        let state = keys::State { ultra: u.profile.ultra, has_lock: row.is_some(), picker_open: picker_open(u), chart_open: u.chart };
        let action = keys::map(vk, ctrl, state)?;
        let n = u.view.cells.len();
        let symbol_of = |u: &Ui, i: usize| u.view.cells.get(i).and_then(|c| u.cfg.rows.iter().find(|r| r.label == c.label)).map(|r| r.symbol.clone());
        use keys::Action as K;
        Some(match action {
            K::LockFirst | K::Move(_) => {
                let to = match (action, row) {
                    (K::Move(dir), Some(i)) => keys::step(i, n, panel_geo(u).cols, dir),
                    _ => 0,
                };
                let label = u.view.cells.get(to)?.label.clone();
                lock_card(u, Some(label));
                u.picker_scroll = panel::reveal(&panel_geo(u), to);
                u.hover_at = None;
                Do::Redraw
            }
            K::OpenLocked => Do::Open(symbol_of(u, row?)?),
            K::SetChart(on) => Do::Chart(on),
            K::SetPicker(on) => Do::Picker(on),
            K::CopyLocked => {
                let c = u.view.cells.get(row?)?;
                Do::Copy(format!("{} {}", c.label, fmt_price(c.quote.as_ref()?.price, c.digits)))
            }
            K::CopyAll => Do::Copy(crate::model::board_text(&u.view.cells)),
            K::Remove => Do::Remove(symbol_of(u, row?)?),
            K::Shift(d) => Do::Shift(symbol_of(u, row?)?, d),
            K::Undo => Do::Undo,
        })
    })
    .flatten();
    match todo {
        None => return false,
        Some(Do::Redraw) => unsafe { InvalidateRect(hwnd, null(), 0); },
        Some(Do::Open(sym)) => open_cnbc(hwnd, &sym),
        Some(Do::Chart(on)) => set_chart(hwnd, on),
        Some(Do::Picker(on)) => apply_picker(hwnd, on),
        Some(Do::Copy(text)) => {
            if !text.is_empty() {
                copy_text(&text);
            }
        }
        Some(Do::Remove(sym)) => remove_card(hwnd, &sym),
        Some(Do::Shift(sym, d)) => reorder(hwnd, &sym, Some(d)),
        Some(Do::Undo) => undo_last(hwnd),
    }
    true
}

/// Open or close the symbol picker: the panel grows or shrinks in place, staying centred.
fn apply_picker(hwnd: HWND, open: bool) {
    // The chart goes first so its width is handed back before the chips need the room.
    if open { set_chart(hwnd, false); }
    with_ui(|u| {
        u.pick_open = open;
        u.picker_scroll = 0;
        u.wheel_remainder = 0;
        u.hover_at = None;
    });
    unsafe { InvalidateRect(hwnd, null(), 0); }
}

fn wheel_movement(remainder: &mut i32, delta: i32, dpi: u32) -> i32 {
    if delta != 0 && *remainder != 0 && delta.signum() != remainder.signum() { *remainder = 0; }
    let total = remainder.saturating_add(delta);
    *remainder = total % 120;
    -(total / 120) * (dpi as i32).max(1)
}

fn scroll_picker(hwnd: HWND, delta: i32) {
    let changed = with_ui(|u| {
        if u.mode != Mode::Panel { return false; }
        let g = panel_geo(u);
        let old = u.picker_scroll.clamp(0, g.scroll_max);
        u.picker_scroll = old.saturating_add(delta).clamp(0, g.scroll_max);
        let changed = old != u.picker_scroll;
        if changed { u.hover_at = None; }
        changed
    }).unwrap_or(false);
    if changed { unsafe { InvalidateRect(hwnd, null(), 0); } }
}

/// Switch palette or face. Every GDI object derived from either is rebuilt here, in one place, so
/// no repaint can be left using the previous palette's brush or font.
fn apply_theme(hwnd: HWND) {
    let Some((icon, tip)) = with_ui(|u| {
        let (t, face) = selected_look(&u.cfg, u.profile.ultra);
        if std::ptr::eq(u.theme, t) && face == u.face {
            return None;
        }
        unsafe {
            DeleteObject(u.bg);
            DeleteObject(u.font);
        }
        u.theme = t;
        u.face = face;
        // A theme change restarts an animated background at the top of its loop.
        u.anim = None;
        u.bg = unsafe { CreateSolidBrush(t.bg) };
        u.font = make_font(&u.cfg, u.dpi);
        u.grad = Gradient::new(t);
        u.fonts = u.fonts.take().map(|_| panel::Fonts::new(face, u.dpi, t.is_pixel()));
        let fresh = make_icon(theme::link_color(t, u.view.link), u.alert_pending.is_some());
        if !u.icon.is_null() {
            unsafe { DestroyIcon(u.icon) };
        }
        u.icon = fresh;
        u.icon_link = Some((u.view.link, u.alert_pending.is_some()));
        Some((u.icon, tooltip_for(u)))
    })
    .flatten()
    else {
        return;
    };
    save_cfg();
    tray(hwnd, NIM_MODIFY, icon, &tip);
    sync_anim(hwnd);
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

/// Whether the background clock runs at all. Three things can stop it, and each one is a promise:
/// the classic build keeps the one look it shipped with, Windows' "Show animations" can be off, and
/// a palette with `cycle: None` is still. Kept as one function so the painter and the timer cannot
/// disagree about whether the panel is moving.
fn clock_runs(ultra: bool, motion: bool, t: &Theme) -> bool {
    ultra && motion && t.animated()
}

/// Whether Windows allows animation. "Show animations in Windows" is a preference a person sets
/// once; a panel that ignores it is the kind of thing that gets an app switched off rather than
/// configured, so an explicit off holds every cycle at phase 0.
fn allow_motion() -> bool {
    /// Not exported by windows-sys 0.59's WindowsAndMessaging.
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    let mut on: BOOL = 1;
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut _ as _, 0)
    };
    // A query that fails must not freeze the background; only a real "off" does.
    ok == 0 || on != 0
}

/// Start or stop the animation timer to match what is on screen: only an open panel running an
/// animated theme needs one, so the strip and every still theme pay nothing for it.
fn sync_anim(hwnd: HWND) {
    let on = with_ui(|u| u.mode == Mode::Panel && clock_runs(u.profile.ultra, u.motion, u.theme))
        .unwrap_or(false);
    unsafe {
        if on {
            SetTimer(hwnd, TIMER_ANIM, ANIM_MS, None);
        } else {
            KillTimer(hwnd, TIMER_ANIM);
        }
    }
}

/// The text on the clipboard, as UTF-16. Fails silently: a clipboard held open by another app is a
/// lost convenience, not an error worth interrupting anyone over.
fn copy_text(s: &str) {
    /// CF_UNICODETEXT. The windows-sys constant lives behind a feature nothing else here needs.
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let w: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
            }
            // The clipboard takes ownership of the handle on success. A failed set would leak the
            // block; `GlobalFree` is not exported by the memory module in this windows-sys
            // version, and a clipboard failure is a one-handle leak in a process that lives for
            // hours, so the leak is the cheaper of the two options.
            SetClipboardData(CF_UNICODETEXT, h);
        }
        CloseClipboard();
    }
}

/// The clipboard's text (CF_UNICODETEXT), at most 4,096 UTF-16 units, or `None` when there is
/// none or the clipboard is busy. Never reads past the block's size, and always closes the
/// clipboard. Only called on an explicit click.
fn clipboard_text() -> Option<String> {
    const CF_UNICODETEXT: u32 = 13;
    const MAX_UNITS: usize = 4096;
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return None;
        }
        let mut text = None;
        let h = GetClipboardData(CF_UNICODETEXT);
        if !h.is_null() {
            let bytes = GlobalSize(h);
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let cap = (bytes / 2).min(MAX_UNITS);
                let units = std::slice::from_raw_parts(p, cap);
                let end = units.iter().position(|u| *u == 0).unwrap_or(cap);
                text = Some(String::from_utf16_lossy(&units[..end]));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        text
    }
}

/// Open a symbol's page on CNBC. The canonical symbol (`XAU=`, `@DJ.1`) is what the quote URL wants.
fn open_cnbc(hwnd: HWND, symbol: &str) {
    let url = wide(&format!("https://www.cnbc.com/quotes/{symbol}"));
    let op = wide("open");
    unsafe { ShellExecuteW(hwnd, op.as_ptr(), url.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_FEED => {
            on_feed(hwnd);
            0
        }
        WM_DOCK | WM_DISPLAYCHANGE => {
            relayout_and_place(hwnd);
            dock_check(hwnd);
            0
        }
        WM_SETTINGCHANGE => {
            let motion = allow_motion();
            let changed = with_ui(|u| {
                let changed = u.motion != motion;
                u.motion = motion;
                changed
            }).unwrap_or(false);
            if changed {
                sync_anim(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            PostMessageW(hwnd, WM_DOCK, 0, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_FGCHANGE => {
            dock_check(hwnd);
            SetTimer(hwnd, TIMER_RECHECK, 150, None);
            0
        }
        WM_ALERT => {
            let failed = wp & 1 != 0;
            let seq = lp as u64;
            with_ui(|u| {
                // A gap notice has no log line of its own; seq 0 is never a live entry.
                let outcome = match (failed, wp & 4 != 0) {
                    (true, _) => alerts::Outcome::SendFailed,
                    (false, true) => alerts::Outcome::SentOnRetry,
                    (false, false) => alerts::Outcome::Sent,
                };
                u.alert_log.set_outcome(seq, outcome);
                u.alert_note = Some(webhook_note(wp));
            });
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_RANGE => {
            // Results are drained and filed under one borrow: no I/O, no window calls. A result the
            // book does not recognise (rows changed, request lost) is dropped by `arrived`.
            with_ui(|u| {
                let done = u.range_slots.lock().map(|mut s| s.take_done()).unwrap_or_default();
                let now = crate::source::now_s();
                for d in done {
                    u.book.arrived(d.generation, &d.key, d.result, now);
                }
            });
            if with_ui(|u| u.mode == Mode::Panel && u.chart).unwrap_or(false) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_TIMER if wp == TIMER_ANIM => {
            // The clock keeps running while the strip is collapsed; only the panel draws it.
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_TIMER if wp == TIMER_RECHECK => {
            KillTimer(hwnd, TIMER_RECHECK);
            dock_check(hwnd);
            0
        }
        WM_TIMER if wp == TIMER_DOCK => {
            // A morph that never finished (it cannot be running here: morphs block this thread).
            with_ui(|u| {
                if u.mode == Mode::Morph && u.morph.is_none() {
                    u.mode = Mode::Strip;
                }
            });
            dock_check(hwnd);
            // keep the panel's ages ticking
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        // The global hotkey: expand when the strip is showing, collapse when the panel is, nothing
        // during a morph (which blocks this thread, so only a stale mode could say Morph here).
        m if m == hotkey::WM_HOTKEY => {
            if wp as i32 == HOTKEY_ID {
                wake_feed();
                match with_ui(|u| u.mode) {
                    Some(Mode::Strip) => expand(hwnd),
                    Some(Mode::Panel) => collapse(hwnd),
                    _ => {}
                }
            }
            0
        }
        WM_WTSSESSION_CHANGE => {
            match wp {
                WTS_SESSION_LOCK => session_changed(true),
                WTS_SESSION_UNLOCK => session_changed(false),
                _ => {}
            }
            0
        }
        WM_LOOKUP => {
            on_lookup(hwnd);
            0
        }
        WM_UPDATE => {
            on_update(hwnd);
            0
        }
        WM_REPORT => {
            on_report(hwnd);
            0
        }
        // The client area is the whole window: no caption, no border, no offset for the hit tests.
        WM_NCCALCSIZE => 0,
        // Only the panel is resizable, so only the panel advertises a size range: the strip is a bar,
        // and clamping it to the panel's minimum would stop it being a bar.
        WM_GETMINMAXINFO => {
            // The floor is both panes when the chart is open: a narrower frame would have to fold
            // the card grid, and a pane opened by hand must not reflow the thing it sits beside.
            let Some((mode, min)) = with_ui(|u| {
                let (w, h) = panel::minimum(u.view.cells.len(), catalog_len(u), u.dpi, false, u.chart);
                (u.mode, (w, h))
            }) else {
                return 0;
            };
            if mode == Mode::Panel {
                unsafe {
                    let info = lp as *mut MINMAXINFO;
                    // Smallest: the content size, so the cards never fold below their columns.
                    (*info).ptMinTrackSize = POINT { x: min.0, y: min.1 };
                    // Largest: the monitor's work area, so maximise cannot hide under the taskbar.
                    let mut mi: MONITORINFO = std::mem::zeroed();
                    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    if GetMonitorInfoW(mon, &mut mi) != 0 {
                        (*info).ptMaxTrackSize = POINT { x: mi.rcWork.right - mi.rcWork.left, y: mi.rcWork.bottom - mi.rcWork.top };
                        (*info).ptMinTrackSize.x = (*info).ptMinTrackSize.x.min((*info).ptMaxTrackSize.x);
                        (*info).ptMinTrackSize.y = (*info).ptMinTrackSize.y.min((*info).ptMaxTrackSize.y);
                    }
                }
            }
            0
        }
        // A dragged edge changes the width; the panel reflows its columns from it.
        WM_SIZE => {
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_PAINT => {
            if with_ui(paint).is_none() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            0
        }
        WM_ERASEBKGND => 1,
        // Floating strip: the whole strip is a drag handle. Any other placement: fixed, a click opens the panel.
        // Panel: the header (minus the button) drags it.
        WM_NCHITTEST => {
            // The panel's edges resize it. The strip stays a drag handle with nothing to grab: it is
            // a bar, and a bar that can be stretched into a rectangle is a bug with a cursor.
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                let mut pt = POINT { x: (lp & 0xffff) as i16 as i32, y: ((lp >> 16) & 0xffff) as i16 as i32 };
                ScreenToClient(hwnd, &mut pt);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                let b = 6 * with_ui(|u| u.dpi as i32).unwrap_or(96) / 96;
                let (l, r, t, bo) = (pt.x < b, pt.x >= rc.right - b, pt.y < b, pt.y >= rc.bottom - b);
                let edge = match (l, r, t, bo) {
                    (true, _, true, _) => Some(HTTOPLEFT),
                    (_, true, true, _) => Some(HTTOPRIGHT),
                    (true, _, _, true) => Some(HTBOTTOMLEFT),
                    (_, true, _, true) => Some(HTBOTTOMRIGHT),
                    (true, _, _, _) => Some(HTLEFT),
                    (_, true, _, _) => Some(HTRIGHT),
                    (_, _, true, _) => Some(HTTOP),
                    (_, _, _, true) => Some(HTBOTTOM),
                    _ => None,
                };
                if let Some(h) = edge {
                    return h as LRESULT;
                }
            }
            let Some((mode, dock, header, btn, pick_btn, chart_btn, gear_btn)) = with_ui(|u| {
                let g = panel_geo(u);
                (u.mode, u.cfg.placement() != Placement::Floating, g.header, g.btn, g.pick_btn, g.chart_btn, g.gear_btn)
            }) else {
                return HTCLIENT as LRESULT;
            };
            match mode {
                Mode::Strip if !dock => HTCAPTION as LRESULT,
                Mode::Panel => {
                    let mut pt = POINT { x: (lp & 0xffff) as i16 as i32, y: ((lp >> 16) & 0xffff) as i16 as i32 };
                    ScreenToClient(hwnd, &mut pt);
                    let on_button = panel::in_rect(&btn, pt.x, pt.y)
                        || panel::in_rect(&pick_btn, pt.x, pt.y)
                        || panel::in_rect(&chart_btn, pt.x, pt.y)
                        || panel::in_rect(&gear_btn, pt.x, pt.y);
                    if pt.y < header && !on_button {
                        HTCAPTION as LRESULT
                    } else {
                        HTCLIENT as LRESULT
                    }
                }
                _ => HTCLIENT as LRESULT,
            }
        }
        // The strip never takes focus from the app being typed in; the open panel does (for Esc).
        WM_MOUSEACTIVATE => {
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
                MA_ACTIVATE as LRESULT
            } else {
                MA_NOACTIVATE as LRESULT
            }
        }
        WM_SETCURSOR => {
            let fixed = |u: &Ui| u.cfg.placement() != Placement::Floating;
            // The whole open panel is clickable (a card locks or opens, the chips toggle symbols),
            // so the hand cursor covers it. The header stays a drag handle via WM_NCHITTEST, which
            // reports HTCAPTION there and never reaches this branch.
            let hand = with_ui(|u| (u.mode == Mode::Strip && fixed(u)) || u.mode == Mode::Panel).unwrap_or(false);
            if hand && (lp & 0xffff) as u32 == HTCLIENT {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            // One click can mean several things; decide which from the same `Geo` the painter used.
            enum Act {
                Expand,
                /// A click on one symbol of the docked or cornered strip: open with that card locked.
                ExpandLock(String),
                Collapse,
                TogglePicker,
                ToggleChart,
                /// The zone name in the chart pane's header: local time <-> UTC.
                Zone,
                /// A range tab in the chart pane's header.
                Range(Range),
                /// Open the settings menu from under its button.
                Settings,
                Symbol(String),
                /// Lock (or unlock) this card by label. Ultra's answer to a click on a card.
                Select(String),
                /// Open this symbol's CNBC page: the classic build's click, and Ultra's Open button.
                Open(String),
                Nothing,
            }
            let what = with_ui(|u| {
                let g = panel_geo(u);
                // A card's symbol, for the places that open its page.
                let symbol_of = |u: &Ui, i: usize| {
                    u.view.cells.get(i).and_then(|c| u.cfg.rows.iter().find(|r| r.label == c.label)).map(|r| r.symbol.clone())
                };
                match u.mode {
                    Mode::Strip if u.cfg.placement() != Placement::Floating => {
                        let hit = if u.profile.ultra { keys::slot_at(&u.frame.slots, x, y) } else { None };
                        match hit.and_then(|i| u.view.cells.get(i)) {
                            Some(c) => Act::ExpandLock(c.label.clone()),
                            None => Act::Expand,
                        }
                    }
                    Mode::Panel if panel::in_rect(&g.btn, x, y) => Act::Collapse,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.gear_btn, x, y) => Act::Settings,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.pick_btn, x, y) => Act::TogglePicker,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.chart_btn, x, y) => Act::ToggleChart,
                    // The zone name is inside the pane, which is beside the cards, never over them; it
                    // is still answered before the card hit test so no later arm can claim the click.
                    // The button's rectangle is empty unless the pane drew it, so a stale button cannot be hit.
                    Mode::Panel if u.profile.ultra && u.chart && u.chart_layout.get().is_some_and(|l| l.zone_btn.contains(x, y)) => Act::Zone,
                    // The range tabs are on the pane's second header line, answered from the stored layout
                    // like the zone button: the layout exists only while the pane drew a card.
                    Mode::Panel if u.profile.ultra && u.chart && u.chart_layout.get().and_then(|l| l.tab_at(x, y)).is_some() => {
                        match u.chart_layout.get().and_then(|l| l.tab_at(x, y)).and_then(|i| Range::ALL.get(i)) {
                            Some(rg) => Act::Range(*rg),
                            None => Act::Nothing,
                        }
                    }
                    // A chip, when the picker is open and the pointer is on one.
                    Mode::Panel if picker_open(u) && panel::pick_hit(&g, x, y).is_some() => {
                        let list = config::menu_list(&u.cfg);
                        match panel::pick_hit(&g, x, y).and_then(|i| list.get(i)) {
                            Some(r) => Act::Symbol(r.symbol.clone()),
                            None => Act::Nothing,
                        }
                    }
                    // The Open button on the locked card is the one way Ultra reaches CNBC from a card,
                    // and it is checked before the card under it: the button is *on* the card.
                    Mode::Panel if panel::open_btn_at(&g, selected_row(u), u.dpi, x, y) => {
                        match selected_row(u).and_then(|i| symbol_of(u, i)) {
                            Some(sym) => Act::Open(sym),
                            None => Act::Nothing,
                        }
                    }
                    // Otherwise the card under the pointer is the target. Ultra locks it (the chart
                    // follows, and the page is a button away); the classic build has no chart to lock,
                    // so its click still opens CNBC at once.
                    Mode::Panel => match panel::card_at(&g, u.view.cells.len(), x, y) {
                        Some(i) if u.profile.ultra => match u.view.cells.get(i) {
                            Some(c) => Act::Select(c.label.clone()),
                            None => Act::Nothing,
                        },
                        Some(i) => match symbol_of(u, i) {
                            Some(sym) => Act::Open(sym),
                            None => Act::Nothing,
                        },
                        None => Act::Nothing,
                    },
                    _ => Act::Nothing,
                }
            });
            match what {
                Some(Act::Expand) => expand(hwnd),
                Some(Act::ExpandLock(label)) => {
                    with_ui(|u| lock_card(u, Some(label)));
                    expand(hwnd);
                }
                Some(Act::Collapse) => collapse(hwnd),
                Some(Act::Settings) => settings_menu(hwnd),
                // `apply_picker` closes the chart when the picker opens, and hands its width back.
                Some(Act::TogglePicker) => {
                    let open = !with_ui(|u| u.pick_open).unwrap_or(false);
                    apply_picker(hwnd, open);
                }
                Some(Act::ToggleChart) => {
                    let on = !with_ui(|u| u.chart).unwrap_or(false);
                    set_chart(hwnd, on);
                }
                Some(Act::Zone) => {
                    // Saved outside the closure: `with_ui` does no I/O.
                    let cfg = with_ui(|u| {
                        u.cfg.chart_utc = !u.cfg.chart_utc;
                        // Candles refold on the new clock, so the pointer's bar index no longer means
                        // the same candle.
                        u.chart_at = None;
                        u.cfg.clone()
                    });
                    if let Some(cfg) = cfg {
                        config::save(&cfg);
                    }
                    InvalidateRect(hwnd, null(), 0);
                }
                Some(Act::Range(rg)) => {
                    let changed = with_ui(|u| {
                        if u.range != rg {
                            u.range = rg;
                            // Remembered for the next time the pane opens (saved below, outside).
                            u.cfg.chart_range = rg.to_saved();
                            // The candles are refolded and may be another series: the pointer's bar
                            // index means nothing now, and the old layout is not what is on screen.
                            u.chart_at = None;
                            u.chart_layout.set(None);
                            want_range(u);
                            return true;
                        }
                        false
                    })
                    .unwrap_or(false);
                    // Saved outside the closure: `with_ui` does no I/O.
                    if changed {
                        save_cfg();
                    }
                    InvalidateRect(hwnd, null(), 0);
                }
                Some(Act::Select(label)) => {
                    with_ui(|u| {
                        // The same card again unlocks it: a lock that cannot be undone is a trap.
                        let next = toggle_lock(u.selected.as_deref(), label);
                        lock_card(u, next);
                    });
                    InvalidateRect(hwnd, null(), 0);
                }
                Some(Act::Symbol(sym)) => {
                    change_cards(hwnd, |cfg| toggle_kind(cfg, &sym));
                }
                Some(Act::Open(sym)) => open_cnbc(hwnd, &sym),
                _ => {}
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            let now = crate::source::now_s();
            // `Hover { row: usize::MAX }` stands in for "off every chart" so one comparison covers
            // all three hover targets and a repaint only happens when something visual moved.
            let away = panel::Hover { row: usize::MAX, t: 0.0 };
            let state = with_ui(|u| {
                if u.mode != Mode::Panel {
                    return (false, false, None);
                }
                let g = panel_geo(u);
                let at = panel::hit(&g, u.view.cells.len(), u.dpi, x, y, now).map(snap).unwrap_or(away);
                let btn = panel::in_rect(&g.btn, x, y);
                let pick = u.profile.ultra && panel::in_rect(&g.pick_btn, x, y);
                let chart_btn = u.profile.ultra && panel::in_rect(&g.chart_btn, x, y);
                let gear = u.profile.ultra && panel::in_rect(&g.gear_btn, x, y);
                let open = panel::open_btn_at(&g, selected_row(u), u.dpi, x, y);
                // Answered from the layout the last paint stored, so it cannot disagree with the pixels.
                let band = (CHART_Y_BAND * u.dpi as i32 / 96).max(1);
                let aim = if u.profile.ultra && u.chart { u.chart_layout.get().and_then(|l| l.pointer_at(x, y, band)) } else { None };
                let track = !u.tracking;
                u.tracking = true;
                let was = hot_of(u);
                // Hover moves the crosshair and lights buttons, and nothing else. In particular it no
                // longer re-targets the chart: that is the lock's job.
                let changed = btn != u.hover
                    || pick != u.pick_hover
                    || chart_btn != u.chart_hover
                    || gear != u.gear_hover
                    || open != u.open_hover
                    || aim != u.chart_at
                    || at != u.hover_at.unwrap_or(away);
                u.hover = btn;
                u.pick_hover = pick;
                u.chart_hover = chart_btn;
                u.gear_hover = gear;
                u.open_hover = open;
                u.chart_at = aim;
                u.hover_at = (at.row != usize::MAX).then_some(at);
                (changed, track, changed.then(|| hover_dirty(u, &g, &was)).flatten())
            });
            if let Some((changed, track, dirty)) = state {
                if track {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    TrackMouseEvent(&mut tme);
                }
                if changed {
                    // Only what the move changed, when that can be named; everything otherwise.
                    InvalidateRect(hwnd, dirty.as_ref().map_or(null(), |r| r as *const RECT), 0);
                }
            }
            0
        }
        WM_MOUSELEAVE => {
            let was = with_ui(|u| {
                u.tracking = false;
                let (before, g) = (hot_of(u), panel_geo(u));
                let was = u.hover || u.pick_hover || u.chart_hover || u.gear_hover || u.open_hover || u.hover_at.is_some() || u.chart_at.is_some();
                u.hover = false;
                u.pick_hover = false;
                u.chart_hover = false;
                u.gear_hover = false;
                u.open_hover = false;
                u.hover_at = None;
                u.chart_at = None;
                (was, was.then(|| hover_dirty(u, &g, &before)).flatten())
            });
            if let Some((true, dirty)) = was {
                InvalidateRect(hwnd, dirty.as_ref().map_or(null(), |r| r as *const RECT), 0);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) as u16 as i16) as i32;
            let movement = with_ui(|u| wheel_movement(&mut u.wheel_remainder, delta, u.dpi)).unwrap_or(0);
            if movement != 0 { scroll_picker(hwnd, movement); }
            0
        }
        WM_KEYDOWN if [VK_PRIOR as usize, VK_NEXT as usize, VK_HOME as usize, VK_END as usize].contains(&wp) => {
            // W3 J6: on Ultra with cards, Home and End lock the first and last card (and scroll to it);
            // everywhere else, and for PgUp and PgDn, they page the body as in 1.8.
            let edge = with_ui(|u| if u.mode == Mode::Panel { keys::edge(wp as u32, u.profile.ultra, u.view.cells.len()) } else { None }).flatten();
            if let Some(last) = edge {
                lock_edge(hwnd, last);
                return 0;
            }
            let step = with_ui(|u| (240 * u.dpi as i32 / 96).max(1)).unwrap_or(240);
            let delta = if wp == VK_PRIOR as usize { -step } else if wp == VK_NEXT as usize { step } else if wp == VK_HOME as usize { -i32::MAX } else { i32::MAX };
            scroll_picker(hwnd, delta);
            0
        }
        WM_CHAR if with_ui(|u| u.mode == Mode::Panel && keys::opens_find(wp as u32, u.profile.ultra, GetKeyState(VK_CONTROL as i32) < 0)).unwrap_or(false) => {
            find_card(hwnd);
            0
        }
        // P5: the card keys. `on_key` answers false for a key that means nothing right now (Classic's
        // Ultra-only keys, a strip, a morph), and that key goes back to the system untouched.
        WM_KEYDOWN if keys::is_bound(wp as u32) => {
            if on_key(hwnd, wp as u32) { 0 } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_KEYDOWN if wp == VK_ESCAPE as usize => {
            collapse(hwnd);
            0
        }
        WM_NCLBUTTONDBLCLK => {
            expand(hwnd);
            0
        }
        WM_NCRBUTTONUP | WM_CONTEXTMENU => {
            // lParam is screen coordinates (client ones for WM_CONTEXTMENU from a menu key are
            // not sent), and -1 means the keyboard opened it, with no card to act on.
            let at = if lp == -1 {
                None
            } else {
                let mut pt = POINT { x: (lp & 0xffff) as i16 as i32, y: ((lp >> 16) & 0xffff) as i16 as i32 };
                ScreenToClient(hwnd, &mut pt);
                Some((pt.x, pt.y))
            };
            // Inside the candle plot the pane's own menu comes first (Ultra); anywhere else the main one.
            if !at.is_some_and(|p| pane_menu(hwnd, p)) {
                menu(hwnd, at);
            }
            0
        }
        WM_EXITSIZEMOVE => {
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            let moved = with_ui(|u| match u.mode {
                Mode::Panel => {
                    // A dragged edge makes the width the user's, so closing the chart must not undo it.
                    if u.chart_grew.is_some_and(|(_, grown)| grown.right - grown.left != r.right - r.left) {
                        u.chart_grew = None;
                    }
                    // The frame is the user's now: remember it, and only write when it changed. While
                    // the chart is what made it wide, the compact frame is what is worth remembering:
                    // the chart is not saved, so the wide one would reopen empty.
                    let c = compact_frame(r, u.chart_grew);
                    let frame = [c.left, c.top, c.right - c.left, c.bottom - c.top];
                    if u.cfg.panel != Some(frame) {
                        u.cfg.panel = Some(frame);
                        return true;
                    }
                    false
                }
                Mode::Strip if u.cfg.placement() == Placement::Floating => {
                    u.strip_rect = r;
                    u.cfg.x = Some(r.left);
                    u.cfg.y = Some(r.top);
                    true
                }
                _ => false,
            });
            if moved == Some(true) {
                save_cfg();
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = (wp >> 16) as u32 & 0xffff;
            let open = with_ui(|u| {
                unsafe { DeleteObject(u.font) };
                u.dpi = dpi;
                // The recorded frames are in the old scale's pixels and could never match again.
                u.chart_grew = None;
                u.font = make_font(&u.cfg, dpi);
                u.fonts = u.fonts.take().map(|_| panel::Fonts::new(u.face, dpi, u.theme.is_pixel()));
                (u.mode == Mode::Panel).then(|| panel_geo(u))
            })
            .flatten();
            match open {
                Some(g) => {
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    let b = panel_rect(&r, g.w, g.h);
                    SetWindowPos(hwnd, null_mut(), b.left, b.top, b.right - b.left, b.bottom - b.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                None => relayout_and_place(hwnd),
            }
            0
        }
        WM_TRAY => {
            match (lp as u32) & 0xffff {
                WM_LBUTTONUP => {
                    let mode = with_ui(|u| {
                        let recent = u.tray_click_at.is_some_and(|t| t.elapsed() < TRAY_DEBOUNCE);
                        u.tray_click_at = Some(Instant::now());
                        (!recent).then_some(u.mode)
                    })
                    .flatten();
                    match mode {
                        Some(Mode::Panel) => collapse(hwnd),
                        Some(Mode::Strip) => expand(hwnd),
                        _ => {}
                    }
                }
                // The tray has no cards, so there is nothing for a contextual entry to point at.
                WM_RBUTTONUP => menu(hwnd, None),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            STRIP.store(0, Ordering::Relaxed);
            UnregisterHotKey(hwnd, HOTKEY_ID);
            WTSUnRegisterSessionNotification(hwnd);
            tray_delete(hwnd);
            save_cfg();
            // A webhook POST still in flight gets up to two seconds (its retry goes at once); nothing
            // in flight costs nothing. After the icon and the save, so the quit already looks done.
            alerts::wait_for_posts();
            PostQuitMessage(0);
            0
        }
        _ => {
            // Explorer restarted: the tray icon is gone and must be re-added, and the taskbar moved.
            let recreated = with_ui(|u| (u.taskbar_created, u.icon, tooltip_for(u)));
            if let Some((tc, icon, tip)) = recreated {
                if tc != 0 && msg == tc {
                    tray(hwnd, NIM_ADD, icon, &tip);
                    PostMessageW(hwnd, WM_DOCK, 0, 0);
                    return 0;
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

pub fn run(cfg: Config, board: Arc<Mutex<Board>>) {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let inst = GetModuleHandleW(null());
        let profile = config::profile();
        let class = wide(&format!("{}_Strip", profile.id));
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            // icon 1 = assets/icon.ico, embedded by build.rs (null when built without windres)
            hIcon: LoadIconW(inst, 1 as _),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let title = wide(profile.title());
        let ex = WS_EX_TOOLWINDOW | WS_EX_LAYERED | if cfg.topmost || cfg.placement().on_taskbar() { WS_EX_TOPMOST } else { 0 };
        // WS_THICKFRAME is what makes the panel resizable (and what lets Windows run its own sizing
        // loop on wm_nchittest's HTLEFT/HTRIGHT/... edges). WM_NCCALCSIZE then returns 0, so the
        // frame costs no visible border and no client-area offset: the look is still ours.
        let style = WS_POPUP | WS_THICKFRAME | WS_MAXIMIZEBOX;
        let hwnd = CreateWindowExW(ex, class.as_ptr(), title.as_ptr(), style, 0, 0, 10, 10, null_mut(), null_mut(), inst, null());
        if hwnd.is_null() {
            return;
        }
        STRIP.store(hwnd as isize, Ordering::Relaxed);
        SetLayeredWindowAttributes(hwnd, 0, cfg.opacity, LWA_ALPHA);
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as _, &corner as *const _ as _, std::mem::size_of_val(&corner) as u32);

        let dpi = GetDpiForWindow(hwnd).max(96);
        let tc_name = wide("TaskbarCreated");
        // The UI owns the poll cadence: the feed reads it from the board each pass.
        if let Ok(mut b) = board.lock() {
            b.poll_s = cfg.poll_s;
            b.gov = cfg.governor(profile.ultra);
            b.armed = profile.ultra && alerts::armed(&cfg.alerts);
            b.set_fallback(cfg.fallback_enabled);
        }
        let view = board.lock().map(|b| b.clone()).unwrap_or_else(|_| Board::new(&cfg.rows));
        let theme = theme::pick(profile.ultra, &cfg.theme);
        let ui = Ui {
            hwnd,
            profile,
            font: make_font(&cfg, dpi),
            bg: CreateSolidBrush(theme.bg),
            theme,
            face: face_of(theme, &cfg, profile.ultra),
            grad: Gradient::new(theme),
            icon: make_icon(theme::link_color(theme, Link::Down), false),
            icon_link: Some((Link::Down, false)),
            dpi,
            cfg: cfg.clone(),
            board: board.clone(),
            view,
            frame: Frame::default(),
            placed: false,
            suppressed: false,
            taskbar_created: RegisterWindowMessageW(tc_name.as_ptr()),
            mode: Mode::Strip,
            morph: None,
            strip_rect: RECT { left: 0, top: 0, right: 0, bottom: 0 },
            last_panel: None,
            fonts: None,
            hist: panel::History::new(),
            hover: false,
            pick_hover: false,
            chart_hover: false,
            gear_hover: false,
            open_hover: false,
            chart_at: None,
            chart_layout: std::cell::Cell::new(None),
            range: if profile.ultra { Range::from_saved(cfg.chart_range.as_deref()) } else { Range::D1 },
            book: ranges::Book::new(),
            undo: undo::Undo::default(),
            range_slots: Arc::new(Mutex::new(ranges::Slots::default())),
            pick_open: false,
            picker_scroll: 0,
            wheel_remainder: 0,
            chart: false,
            selected: None,
            chart_grew: None,
            hover_at: None,
            alert_note: None,
            alert_pending: None,
            alert_log: alerts::Log::default(),
            watch: watch::Watch::default(),
            hush: watch::Hush::default(),
            test_alert_at: None,
            tracking: false,
            prev_fg: null_mut(),
            tip_at: None,
            tray_click_at: None,
            hotkey_failed: false,
            update_running: false,
            update_result: Arc::new(Mutex::new(None)),
            report_running: false,
            report_result: Arc::new(Mutex::new(None)),
            adding: None,
            lookup_result: Arc::new(Mutex::new(None)),
            back: None,
            anim: None,
            motion: allow_motion(),
        };
        let (icon, tip) = (ui.icon, tooltip(&ui.view, None));
        let range_slots = ui.range_slots.clone();
        UI.with(|c| *c.borrow_mut() = Some(ui));

        relayout_and_place(hwnd);
        tray(hwnd, NIM_ADD, icon, &tip);
        apply_visibility(hwnd);
        register_hotkey(hwnd);
        register_session_notice(hwnd);

        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, null_mut(), Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        SetTimer(hwnd, TIMER_DOCK, 500, None);

        let target = hwnd as isize;
        dock::spawn_probe(Box::new(move || {
            PostMessageW(target as HWND, WM_DOCK, 0, 0);
        }));
        crate::source::spawn(cfg, board, Box::new(move || {
            PostMessageW(target as HWND, WM_FEED, 0, 0);
        }));
        // Its own worker and message: `spawn`'s signature is shared with other packages, and a range
        // result is not a feed update. Only Ultra has a chart to ask for a range, so Classic does not
        // carry the thread: nothing would ever be put in its slot.
        if profile.ultra {
            crate::source::spawn_ranges(range_slots, Box::new(move || {
                PostMessageW(target as HWND, WM_RANGE, 0, 0);
            }));
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_switch_takes_the_new_themes_face() {
        let mut cfg = Config::default();
        for name in ["midnight", "aurora", "pc98", "midnight"] {
            cfg.theme = name.to_string();
            let (theme, face) = selected_look(&cfg, true);
            assert_eq!(theme.name, name);
            assert_eq!(face, theme.font, "{name} must not keep the previous theme's face");
        }
        cfg.theme_font = Some("Consolas".to_string());
        assert_eq!(selected_look(&cfg, true).1, "Consolas", "an explicit face still wins");
    }

    #[test]
    fn a_click_locks_a_card_and_the_same_click_again_unlocks_it() {
        assert_eq!(toggle_lock(None, "XAU".into()).as_deref(), Some("XAU"));
        // Another card takes the lock; it does not stack.
        assert_eq!(toggle_lock(Some("XAU"), "BTC".into()).as_deref(), Some("BTC"));
        assert_eq!(toggle_lock(Some("BTC"), "BTC".into()), None);
    }

    fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT { left: l, top: t, right: r, bottom: b }
    }

    #[test]
    fn the_chart_opens_the_window_out_from_its_centre() {
        let work = rect(0, 0, 1_920, 1_040);
        // The panel as it opens: 760 wide, centred. The old rule kept its left edge and slid it to 430
        // to fit, so the wide panel hugged the right and closing left the compact one stuck left.
        let before = rect(580, 300, 1_340, 694);
        let grown = grown_rect(before, 1_490, work);
        assert_eq!((grown.left, grown.right), (215, 1_705), "centred, 1,490 wide");
        assert_eq!(((grown.left + grown.right) / 2), (before.left + before.right) / 2, "same centre");
        assert_eq!((grown.top, grown.bottom), (before.top, before.bottom), "height untouched");
        // Near an edge it slides back inside rather than hanging off the screen...
        let near = grown_rect(rect(1_200, 300, 1_960, 694), 1_490, work);
        assert_eq!((near.left, near.right), (430, 1_920));
        // ...and a monitor narrower than the want caps the width.
        assert_eq!(grown_rect(before, 2_500, work).right - grown_rect(before, 2_500, work).left, 1_920);
    }

    #[test]
    fn closing_the_chart_glides_back_to_where_the_window_came_from() {
        let before = rect(580, 300, 1_340, 694);
        let grown = rect(215, 300, 1_705, 694);
        // Untouched since it opened: exactly the frame it grew from, so it is centred again.
        assert_eq!(rect_after_chart(grown, grown, before).map(|r| (r.left, r.right)), Some((580, 1_340)));
        // Dragged 100 px right and 40 down in between: it shrinks where it now is, not back where it was.
        let moved = rect(315, 340, 1_805, 734);
        let back = rect_after_chart(moved, grown, before).unwrap();
        assert_eq!((back.left, back.right, back.top, back.bottom), (680, 1_440, 340, 734));
        // Resized by hand: the size is the user's, so nothing is undone.
        assert!(rect_after_chart(rect(215, 300, 1_600, 694), grown, before).is_none());
    }

    #[test]
    fn a_frame_that_outlives_the_chart_is_the_compact_one() {
        let before = rect(580, 300, 1_340, 694);
        let grown = rect(215, 300, 1_705, 694);
        // Saved or reopened while the chart is what made it wide: the compact frame, in the same place.
        let c = compact_frame(grown, Some((before, grown)));
        assert_eq!((c.left, c.right), (580, 1_340));
        // Dragged to the right first: compact, and carried by the same distance.
        let c = compact_frame(rect(415, 300, 1_905, 694), Some((before, grown)));
        assert_eq!((c.left, c.right), (780, 1_540));
        // No chart growth on record, or a width the chart did not make: left exactly as it is.
        let hand = rect(100, 100, 1_000, 600);
        assert_eq!(compact_frame(hand, None).right, 1_000);
        assert_eq!(compact_frame(hand, Some((before, grown))).right, 1_000);
    }

    #[test]
    fn a_lock_on_a_vanished_symbol_does_not_come_back_with_it() {
        let rows = ["XAU", "BTC", "S&P"];
        assert_eq!(settle_lock(Some("BTC"), &rows, false).as_deref(), Some("BTC"), "still there");
        // Removed, chart closed: no lock at all, so re-adding the symbol later does not re-light it.
        assert_eq!(settle_lock(Some("NAS"), &rows, false), None);
        // Removed with the chart open: the first card takes over, so the pane is not left asking.
        assert_eq!(settle_lock(Some("NAS"), &rows, true).as_deref(), Some("XAU"));
        assert_eq!(settle_lock(None, &rows, false), None, "no lock stays no lock");
        assert_eq!(settle_lock(Some("XAU"), &[], true), None, "nothing to lock");
    }

    #[test]
    fn high_resolution_wheel_accumulates_partial_events() {
        let mut remainder = 0;
        let mut total = 0;
        for _ in 0..12 { total += wheel_movement(&mut remainder, -10, 144); }
        assert_eq!(total, 144);
        assert_eq!(remainder, 0);
        assert_eq!(wheel_movement(&mut remainder, 120, 144), -144);
        assert_eq!(wheel_movement(&mut remainder, -60, 144), 0);
        assert_eq!(wheel_movement(&mut remainder, 120, 144), -144, "reversing direction must not consume a full detent");
    }

    #[test]
    fn the_classic_build_never_runs_a_clock() {
        let t = theme::classic();
        assert!(!clock_runs(false, true, t), "the classic build keeps its one still look");
        assert!(clock_runs(true, true, t), "Ultra lets midnight breathe");
    }

    #[test]
    fn windows_and_a_still_palette_can_both_stop_the_clock() {
        let t = theme::resolve("drift");
        assert!(!clock_runs(true, false, t), "Show animations is off");
        let still = Theme { cycle: None, ..*t };
        assert!(!clock_runs(true, true, &still), "a still palette holds phase 0");
    }

    /// Audit 1.8, finding 2: the fallback toggle writes the settings as they are when the user
    /// answers the confirmation box, not a copy from before it. Here a once alert fires while the box
    /// is up (its modal loop runs the feed) and is spent; the file must not get it back.
    #[test]
    fn the_fallback_toggle_writes_what_happened_during_its_confirmation() {
        let mut cfg = Config { fallback_enabled: false, ..Config::default() };
        cfg.alerts.push(config::Alert { label: "XAU".into(), above: Some(4100.0), once: true, ..Default::default() });
        // The menu opened: the old code took its copy here.
        let before_box = cfg.clone();
        // While the box is up the alert fires and, being a once alert, is spent.
        cfg.alerts.clear();
        cfg.poll_s = 10.0;
        let next = fallback_settings(&cfg, true);
        assert!(next.fallback_enabled);
        assert!(next.alerts.is_empty(), "a spent once alert must not be written back");
        assert_eq!(next.poll_s, 10.0, "a setting changed from the tray during the box is kept");
        assert_ne!(next.alerts.len(), before_box.alerts.len());
        assert!(!fallback_settings(&cfg, false).fallback_enabled);
    }

    /// Audit 1.8, finding 3: a webhook result names the test alert even when its log line is gone
    /// (the list was cleared, or newer alerts pushed it out), and never calls a test a kept alert.
    #[test]
    fn a_webhook_result_knows_a_test_without_its_log_line() {
        assert_eq!(webhook_note(alert_wparam(true, true)), "test alert: webhook failed");
        assert_eq!(webhook_note(alert_wparam(false, true)), "test alert: webhook sent");
        assert_eq!(webhook_note(alert_wparam(true, false)), "alert kept here: webhook failed");
        assert_eq!(webhook_note(alert_wparam(false, false)), "alert: webhook sent");
        assert_eq!(alert_wparam(true, false), 1, "bit 0 is still the failure, as before");
        assert_eq!(webhook_note(alert_wparam_retry(false, false, true)), "alert: webhook sent on retry");
        assert_eq!(webhook_note(alert_wparam_retry(false, true, true)), "test alert: webhook sent on retry");
        assert_eq!(webhook_note(alert_wparam_retry(true, false, true)), "alert kept here: webhook failed", "a failure after the retry is a failure");
        let mut log = alerts::Log::default();
        let seq = log.push(0.0, alerts::TEST_TITLE.into(), alerts::Outcome::Shown);
        log.clear();
        assert!(!log.set_outcome(seq, alerts::Outcome::Sent), "the line is gone");
    }

    /// Audit 1.8, finding 4: the Alerts submenu's count is of the symbols it lists. An alert on a hidden
    /// symbol is kept in the settings but not counted.
    #[test]
    fn the_alerts_count_is_of_listed_symbols_only() {
        let set = |label: &str| config::Alert { label: label.into(), above: Some(1.0), ..Default::default() };
        let empty = config::Alert { label: "EUR".into(), ..Default::default() };
        let alerts = vec![set("XAU"), set("BTC"), empty];
        assert_eq!(armed_labels(&alerts, &["XAU", "EUR"]), vec!["XAU".to_string()]);
        assert_eq!(armed_labels(&alerts, &["XAU", "BTC"]).len(), 2);
        assert!(armed_labels(&alerts, &[]).is_empty());
    }

    /// Audit 1.8, finding 5: the card menu's Copy and Open act on the card that was right-clicked even
    /// when a row lands before it while the menu is open (a finished lookup adds a hand-added row).
    #[test]
    fn the_card_menu_copy_and_open_follow_the_card_not_its_old_index() {
        let cell = |label: &str, price: f64| crate::model::Cell {
            label: label.into(),
            digits: 2,
            quote: Some(crate::model::Quote { price, ..Default::default() }),
            stale: false,
        };
        let row = |label: &str| config::Row { label: label.into(), symbol: format!("{label}="), digits: 2, name: String::new(), strip: true };
        // B was right-clicked at index 1; then a new row N lands in front of it.
        let cells = [cell("N", 9.0), cell("A", 1.0), cell("B", 2.0)];
        let rows = [row("N"), row("A"), row("B")];
        let (text, symbol) = card_target(&cells, &rows, "B").unwrap();
        assert_eq!(text, "B 2.00");
        assert_eq!(symbol, "B=");
        // And the card is gone: nothing to copy or open, not the neighbour that took its place.
        assert!(card_target(&cells[..2], &rows[..2], "B").is_none());
        assert!(card_target(&cells, &rows[..1], "Z").is_none());
    }

    /// Audit 1.8, finding 6: an alert that fires while the panel is open and in front does not light the
    /// tray marker (nothing would clear it); every other situation still does.
    #[test]
    fn an_alert_does_not_light_the_marker_while_the_panel_is_in_front() {
        assert!(!marks_tray(Mode::Panel, true));
        assert!(marks_tray(Mode::Panel, false), "an open panel behind another window is not being looked at");
        assert!(marks_tray(Mode::Strip, false));
        assert!(marks_tray(Mode::Strip, true), "only the panel is a place to read the footer note");
        assert!(marks_tray(Mode::Morph, true));
    }

    /// Audit 1.8, finding 8: the menu's entries act on the mode when they are chosen, not the mode the
    /// menu was opened in, because the hotkey and the tray run during the menu's own message loop.
    #[test]
    fn the_menu_acts_on_the_mode_it_is_chosen_in_not_the_one_it_opened_in() {
        // Listed "Expand view" (strip), then the hotkey opened the panel: choosing it does nothing more.
        assert_eq!(panel_step(false, Mode::Panel), None);
        assert_eq!(panel_step(false, Mode::Strip), Some(PanelStep::Expand));
        // Listed "Back to taskbar" (panel), then the hotkey collapsed it: nothing more to collapse.
        assert_eq!(panel_step(true, Mode::Strip), None);
        assert_eq!(panel_step(true, Mode::Panel), Some(PanelStep::Collapse));
        // A Position entry listed for the strip is dropped if the panel is open when it is chosen.
        assert!(!position_applies(false, Mode::Panel));
        assert!(position_applies(false, Mode::Strip));
        assert!(!position_applies(true, Mode::Strip));
    }

    /// Audit 1.8, finding 9: a morph that crosses to a monitor of another scale lays its end frame out
    /// again at the new scale instead of landing at the old one's size.
    #[test]
    fn a_morph_across_a_scale_change_replans_its_end_frame() {
        let rect = |w: i32| RECT { left: 0, top: 0, right: w, bottom: 40 };
        let planned = rect(400);
        // Same scale: the plan stands and the replan is never asked for.
        let kept = settled_frame(96, 96, planned, || panic!("no replan without a scale change"));
        assert_eq!(kept.right, 400);
        // 96 -> 144 dpi in the middle of the morph: the end frame is the one worked out at 144.
        let moved = settled_frame(96, 144, planned, || rect(600));
        assert_eq!(moved.right, 600);
    }
}
