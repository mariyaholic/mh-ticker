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
use crate::config::{self, Config, Placement, Profile};
use crate::dock;
use crate::panel;
use crate::theme::{self, Gradient, Theme};
use crate::model::{fmt_change, fmt_price, Board, Cell, Link};
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_PRIOR, VK_NEXT, VK_HOME, VK_END};
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
/// Symbols submenu: CMD_SYM + index into `config::menu_list`.
const CMD_SYM: usize = 2000;
/// Position submenu: CMD_POS + index into `Placement::ALL`, CMD_MON + index into `dock::monitors()`.
const CMD_POS: usize = 4000;
const CMD_MON: usize = 4100;
/// Poll interval: CMD_POLL + index into `POLL_CHOICES`.
const CMD_POLL: usize = 5000;
/// Theme: CMD_THEME + index into `theme::ALL` (Ultra only).
const CMD_THEME: usize = 5100;
/// Font: CMD_FONT + index into `theme::FACES` (Ultra only).
const CMD_FONT: usize = 5150;
/// The card under the right-click: copy its price, or open it on CNBC. The index is the card's, so
/// the two ranges must not overlap (`MAX_ROWS` apart).
const CMD_COPY: usize = 5600;
const CMD_OPEN: usize = 5700;
/// The Ultra symbol picker open/closed toggle.
const CMD_PICK: usize = 5800;
/// Alerts: CMD_ALERT + row*4 + step, CMD_ALERT_CLEAR + row (Ultra only).
const CMD_ALERT: usize = 5200;
const CMD_ALERT_CLEAR: usize = 5400;
/// Offsets offered by the Alerts submenu, in percent from the current price. Arbitrary levels are
/// a config edit for now; these are the ones a glance-at-the-strip user actually reaches for.
const ALERT_STEPS: [(f64, bool); 4] = [(1.0, true), (2.0, true), (1.0, false), (2.0, false)];

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
/// The fallback cycle length, and how often a moving background is advanced. Each palette names its
/// own cycle in `theme::ALL`; this only covers a theme that somehow has none.
const ANIM_CYCLE_S: f64 = 9.0;
const ANIM_MS: u32 = 100;
const TIP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// A second tray click inside this window (a double-click) is ignored.
const TRAY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

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
    icon_link: Option<Link>,
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
    tracking: bool,
    /// Foreground window before the panel opened; it gets focus back on collapse.
    prev_fg: HWND,
    /// Last tray tooltip refresh.
    tip_at: Option<Instant>,
    tray_click_at: Option<Instant>,
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
/// red offline) on a dark rounded tile, so it is easy to spot among the hidden icons.
fn make_icon(color: COLORREF) -> HICON {
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
    let mut x = px;
    for (i, c) in cells.iter().enumerate() {
        let (label, price, pc, change) = cell_texts(c, t);
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
    }
    let dot = (line_h / 2).max(4);
    f.h = line_h + 2 * py;
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
        let cells = &u.view.cells;
        let t = u.theme;
        let frame = match avail {
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

fn draw_panel(u: &mut Ui, dc: HDC, w: i32, h: i32) {
    let Some(f) = &u.fonts else { return };
    let list = config::menu_list(&u.cfg);
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
    let scene = panel::Scene {
        title: u.profile.title(),
        board: &u.view,
        hist: &u.hist,
        fonts: f,
        theme: u.theme,
        grad: &u.grad,
        dpi: u.dpi,
        now: crate::source::now_s(),
        btn_hover: u.hover,
        pick_hover: u.pick_hover,
        chart_hover: u.chart_hover,
        gear_hover: u.gear_hover,
        select: u.profile.ultra,
        selected: selected_row(u),
        open_hover: u.open_hover,
        hover: u.hover_at,
        picker: picker_open(u).then(|| panel::Picker { list: &list, shown: &shown }),
        picker_scroll: u.picker_scroll,
        picker_btn: u.profile.ultra,
        chart_btn: u.profile.ultra,
        settings_btn: u.profile.ultra,
        // The chart shows the locked card and nothing else. It used to follow the pointer, which made
        // the pane change under the cursor on the way to it; a click is the one thing that moves it now.
        // With nothing locked the pane says so instead of guessing which card was meant.
        chart: u.chart.then(|| match selected_row(u) {
            Some(row) => {
                let fallback = u.view.cells.get(row).and_then(|c| c.quote.as_ref()).is_some_and(|q| !q.source.is_empty());
                panel::Chart {
                    unavailable: fallback,
                    label: u.view.cells.get(row).map(|c| c.label.as_str()).unwrap_or(""),
                    digits: u.view.cells.get(row).map(|c| c.digits).unwrap_or(2),
                    t0: u.view.bars_t0.get(row).copied().unwrap_or(0.0),
                    bars: if fallback { &[] } else { u.view.bars.get(row).map(|b| &b[..]).unwrap_or(&[]) },
                    last: u.view.cells.get(row).and_then(|c| c.quote.as_ref()).map(|q| q.price),
                }
            }
            None => panel::Chart { label: "", digits: 2, t0: 0.0, bars: &[], last: None, unavailable: false },
        }),
        alert: u.alert_note.as_deref(),
        phase,
    };
    panel::draw(dc, w, h, &scene);
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

fn paint(u: &mut Ui) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(u.hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(u.hwnd, &mut rc);
        let (w, h) = (rc.right.max(1), rc.bottom.max(1));
        if !matches!(&u.back, Some(b) if b.size == (w, h)) {
            u.back = Some(Back::new(w, h));
        }
        // Take the buffer out of `u` so the painter can borrow `u` immutably while drawing into it.
        if let Some(back) = u.back.take() {
            match (u.mode, &u.morph) {
                (Mode::Morph, Some(m)) => draw_morph(m, back.dc, w, h),
                (Mode::Panel, _) => draw_panel(u, back.dc, w, h),
                _ => draw_strip(u, back.dc),
            }
            BitBlt(hdc, 0, 0, w, h, back.dc, 0, 0, SRCCOPY);
            u.back = Some(back);
        }
        EndPaint(u.hwnd, &ps);
    }
}

fn tooltip(view: &Board) -> String {
    let mut s = format!("{} - {}", config::profile().title(), theme::link_name(view.link));
    for c in &view.cells {
        if let Some(q) = &c.quote {
            s.push_str(&format!("\n{} {}", c.label, fmt_price(q.price, c.digits)));
            if !q.source.is_empty() { s.push_str(&format!(" ({}; age unknown)", q.source)); }
            else if c.stale { s.push_str(" (stale)"); }
        }
    }
    s
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

/// Pull the latest board, refresh the tray icon colour and tooltip, redraw. The tray tooltip (an
/// Explorer round trip) refreshes at most every 5 s unless the link colour changed, and a hidden
/// strip is not re-measured.
fn on_feed(hwnd: HWND) {
    let Some((tray_update, fired, hook)) = with_ui(|u| {
        if let Ok(b) = u.board.lock() {
            let sources = |board: &Board| board.cells.iter()
                .map(|c| c.quote.as_ref().map(|q| q.source).unwrap_or(""))
                .collect::<Vec<_>>();
            if sources(&u.view) != sources(&b) { u.tip_at = None; }
            u.view = b.clone();
        }
        let now = crate::source::now_s();
        for cell in &mut u.view.cells {
            if let Some(q) = &cell.quote {
                cell.stale |= crate::model::is_stale(q, now, u.cfg.stale_after_s, u.cfg.delayed_grace_s);
            }
        }
        u.hist.record(&u.view, now);
        // Alerts run after the view is current, so a balloon names the price the strip is showing.
        // The checker drops stale quotes itself: a closed market frozen at a level must not
        // retrigger on every cooldown.
        let fired = if u.profile.ultra && !u.cfg.alerts.is_empty() {
            let f = alerts::check(&mut u.cfg, &u.view, now);
            if !f.is_empty() {
                config::save(&u.cfg);
            }
            f
        } else {
            Vec::new()
        };
        let relinked = u.icon_link != Some(u.view.link);
        if relinked {
            let fresh = make_icon(theme::link_color(u.theme, u.view.link));
            if !u.icon.is_null() {
                unsafe { DestroyIcon(u.icon) };
            }
            u.icon = fresh;
            u.icon_link = Some(u.view.link);
        }
        let due = relinked || u.tip_at.is_none_or(|t| t.elapsed() >= TIP_EVERY);
        let tray_update = due.then(|| {
            u.tip_at = Some(Instant::now());
            (u.icon, tooltip(&u.view))
        });
        (tray_update, fired, u.cfg.webhook.clone())
    }) else {
        return;
    };
    if let Some((icon, tip)) = tray_update {
        tray(hwnd, NIM_MODIFY, icon, &tip);
    }
    for f in fired {
        alerts::balloon(hwnd, TRAY_ID, &f.title, &f.body);
        with_ui(|u| u.alert_note = Some(format!("alert: {}", f.title)));
        if let Some(url) = hook.clone() {
            let target = hwnd as isize;
            // The posting thread reports back by message: `Ui` is thread-local, so it cannot
            // touch it directly.
            alerts::spawn_post(url, f.title.clone(), f.body.clone(), Box::new(move |err| unsafe {
                PostMessageW(target as HWND, WM_ALERT, err.is_some() as usize, 0);
            }));
        }
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
        with_ui(|u| u.chart_grew = None);
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
        if on {
            u.pick_open = false;
            // Nothing locked yet: lock the first card, so the pane opens on a chart instead of an
            // instruction. A second click on the locked card unlocks it again.
            if u.profile.ultra && selected_row(u).is_none() {
                u.selected = u.view.cells.first().map(|c| c.label.clone());
            }
        }
    });
    if let (Some(to_rect), Some(from)) = (target, from) {
        let (w, h) = size(to_rect);
        if let Some(to) = with_ui(|u| (snapshot(hwnd, w, h, |dc| draw_panel(u, dc, w, h)), (w, h))) {
            run_morph(hwnd, cur, to_rect, from, to, (255, 255), CHART_MS, ease_in_out);
            with_ui(|u| u.mode = Mode::Panel);
            unsafe {
                SetWindowPos(hwnd, null_mut(), to_rect.left, to_rect.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            }
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
    let Some((a, b, from, to, op)) = with_ui(|u| {
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
        Some((a, b, (from, fsz), (to, (pw, ph)), u.cfg.opacity))
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
    with_ui(|u| u.mode = Mode::Panel);
    sync_anim(hwnd);
    unsafe {
        SetWindowPos(hwnd, null_mut(), b.left, b.top, b.right - b.left, b.bottom - b.top, SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hwnd, null(), 0);
        SetFocus(hwnd);
    }
}

/// Panel -> strip (back into the taskbar gap when docked, re-measured in case Start moved).
fn collapse(hwnd: HWND) {
    let Some((a, b, from, to, prev, op)) = with_ui(|u| {
        if u.mode != Mode::Panel {
            return None;
        }
        let mut a: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(hwnd, &mut a) };
        u.last_panel = Some(a);
        let psz = ((a.right - a.left).max(1), (a.bottom - a.top).max(1));
        u.hover = false;
        let from = snapshot(hwnd, psz.0, psz.1, |dc| draw_panel(u, dc, psz.0, psz.1));
        u.strip_rect = strip_place(u);
        let fsz = (u.frame.w.max(1), u.frame.h.max(1));
        let to = snapshot(hwnd, fsz.0, fsz.1, |dc| draw_strip(u, dc));
        Some((a, u.strip_rect, (from, psz), (to, fsz), std::mem::replace(&mut u.prev_fg, null_mut()), u.cfg.opacity))
    })
    .flatten() else {
        return;
    };
    run_morph(hwnd, a, b, from, to, (255, op), COLLAPSE_MS, ease_in_out);
    with_ui(|u| u.mode = Mode::Strip);
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
}

/// `anchor` is a screen point to drop the menu from; `None` drops it at the cursor.
fn menu_at(hwnd: HWND, at: Option<(i32, i32)>, anchor: Option<(i32, i32)>) {
    #[allow(clippy::type_complexity)]
    let Some((visible, top, docked, status, open, list, shown, theme_name, theme_font, poll_s, ultra, picked, alerted)) = with_ui(|u| {
        let list = config::menu_list(&u.cfg);
        let shown: Vec<bool> = list.iter().map(|r| config::is_shown(&u.cfg, &r.symbol)).collect();
        let note = if u.view.note.is_empty() { theme::link_name(u.view.link).to_string() } else { u.view.note.clone() };
        let alerted: Vec<String> = u
            .cfg
            .alerts
            .iter()
            .filter(|a| a.above.is_some() || a.below.is_some())
            .map(|a| a.label.clone())
            .collect();
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
    let cmd = unsafe {
        let m = CreatePopupMenu();
        let add = |menu: HMENU, flags: u32, id: usize, text: &str| {
            let t = wide(text);
            AppendMenuW(menu, flags, id, t.as_ptr());
        };
        let item = |flags: u32, id: usize, text: &str| add(m, flags, id, text);
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
        if let Some((i, label, price)) = &card {
            add(m, MF_STRING, CMD_COPY + i, &format!("Copy {label} {price}"));
            add(m, MF_STRING, CMD_OPEN + i, &format!("Open {label} on CNBC"));
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
            add(syms, flags, CMD_SYM + i, &text);
        }
        AppendMenuW(syms, MF_SEPARATOR, 0, null());
        add(syms, MF_STRING, CMD_RESET, "Reset to default");
        item(MF_STRING, CMD_PANEL, if open { "Back to taskbar" } else { "Expand view" });
        SetMenuDefaultItem(m, CMD_PANEL as u32, 0);
        add(m, MF_POPUP, syms as usize, &format!("Symbols ({n_shown})"));
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
        add(m, MF_POPUP, poll as usize, &format!("Poll every {poll_s} s"));
        let sources = CreatePopupMenu();
        let fallback = with_ui(|u| u.cfg.fallback_enabled).unwrap_or(false);
        add(sources, MF_STRING | if fallback { MF_CHECKED } else { 0 }, CMD_FALLBACK, "Allow fallback sources");
        add(m, MF_POPUP, sources as usize, "Data sources");
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
            // Alerts at a move from the current price. Arbitrary levels are a config edit for now.
            let al = CreatePopupMenu();
            let mut any = false;
            for (i, r) in list.iter().enumerate() {
                if !shown[i] {
                    continue;
                }
                any = true;
                let sub = CreatePopupMenu();
                let can_base_alert = with_ui(|u| u.view.cells.iter().find(|c| c.label == r.label)
                    .and_then(|c| alerts::reference_price(c, &u.cfg, crate::source::now_s())).is_some()).unwrap_or(false);
                for (k, (pct, up)) in ALERT_STEPS.iter().enumerate() {
                    let text = format!("{} {pct:.0} %", if *up { "above" } else { "below" });
                    add(sub, MF_STRING | if can_base_alert { 0 } else { MF_GRAYED }, CMD_ALERT + i * 4 + k, &text);
                }
                if alerted.contains(&r.label) {
                    AppendMenuW(sub, MF_SEPARATOR, 0, null());
                    add(sub, MF_STRING, CMD_ALERT_CLEAR + i, "Clear this symbol's alerts");
                }
                add(al, MF_POPUP, sub as usize, &r.label);
            }
            if !any {
                add(al, MF_GRAYED, 0, "(no symbols shown)");
            }
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
            for (i, d) in mons.iter().enumerate() {
                let (w, h) = (d.bounds.right - d.bounds.left, d.bounds.bottom - d.bounds.top);
                let main = if d.primary { ", main" } else { "" };
                add(pos, MF_STRING, CMD_MON + i, &format!("Display {}{main}\t{w} x {h}", i + 1));
            }
            let lastm = CMD_MON + mons.len() - 1;
            CheckMenuRadioItem(pos, CMD_MON as u32, lastm as u32, (CMD_MON + active) as u32, MF_BYCOMMAND);
        }
        add(m, MF_POPUP | if open { MF_GRAYED } else { 0 }, pos as usize, "Position");
        item(MF_STRING | if top || docked { MF_CHECKED } else { 0 } | if docked { MF_GRAYED } else { 0 }, CMD_TOP, "Always on top");
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        let status: String = status.chars().take(90).collect();
        item(MF_STRING | MF_GRAYED, CMD_STATUS, &status);
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
    match cmd {
        CMD_PANEL => {
            if open {
                collapse(hwnd)
            } else {
                expand(hwnd)
            }
        }
        CMD_FALLBACK => {
            let Some(mut next) = with_ui(|u| u.cfg.clone()) else { return; };
            let enabling = !next.fallback_enabled;
            if enabling {
                let message = wide("If CNBC is unavailable or a quote is stale, MH Ticker may request prices from Gold API or Kraken. Prices and update times can differ; Bitcoin may come from a different exchange. Some symbols have no fallback.\n\nYour device connects directly to these providers. Check the source and freshness shown before relying on a quote. Disabling stops new requests from being scheduled and clears fallback prices. Already-authorized requests may still connect or finish; their results are discarded. Kraken requires permission for non-personal commercial use of its data.\n\nAllow fallback sources?");
                let title = wide("Allow fallback sources?");
                let yes = unsafe { MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING) } == IDYES;
                if !yes { return; }
            }
            next.fallback_enabled = enabling;
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
            with_ui(|u| {
                u.cfg.strip_visible = !visible;
                config::save(&u.cfg);
            });
            apply_visibility(hwnd);
        }
        c if !open && (CMD_POS..CMD_POS + Placement::ALL.len()).contains(&c) => {
            with_ui(|u| u.cfg.set_placement(Placement::ALL[c - CMD_POS]));
            apply_placement(hwnd);
        }
        c if !open && (CMD_MON..CMD_MON + mons.len()).contains(&c) => {
            let d = &mons[c - CMD_MON];
            let pick = (!d.primary).then(|| d.device.clone());
            with_ui(|u| u.cfg.monitor = pick);
            apply_placement(hwnd);
        }
        CMD_TOP => {
            with_ui(|u| {
                u.cfg.topmost = !top;
                config::save(&u.cfg);
            });
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
        c if (CMD_SYM..CMD_SYM + list.len()).contains(&c) => {
            let k = list[c - CMD_SYM].symbol.clone();
            if with_ui(|u| config::toggle(&mut u.cfg, &k)).unwrap_or(false) {
                apply_rows(hwnd);
            }
        }
        CMD_RESET => {
            with_ui(|u| u.cfg.rows = config::default_rows());
            apply_rows(hwnd);
        }
        c if (CMD_POLL..CMD_POLL + POLL_CHOICES.len()).contains(&c) => {
            let secs = POLL_CHOICES[c - CMD_POLL];
            with_ui(|u| {
                u.cfg.poll_s = secs;
                config::save(&u.cfg);
                // The feed reads the cadence from the board, so this applies on the next tick.
                if let Ok(mut b) = u.board.lock() {
                    b.poll_s = secs;
                }
            });
        }
        c if ultra && (CMD_THEME..CMD_THEME + theme::ALL.len()).contains(&c) => {
            with_ui(|u| u.cfg.theme = theme::ALL[c - CMD_THEME].name.to_string());
            apply_theme(hwnd);
        }
        c if ultra && (CMD_FONT..CMD_FONT + theme::FACES.len()).contains(&c) => {
            with_ui(|u| u.cfg.theme_font = Some(theme::FACES[c - CMD_FONT].name.to_string()));
            apply_theme(hwnd);
        }
        c if (CMD_COPY..CMD_COPY + config::MAX_ROWS).contains(&c) => {
            let text = with_ui(|u| {
                let c = u.view.cells.get(c - CMD_COPY)?;
                Some(format!("{} {}", c.label, fmt_price(c.quote.as_ref()?.price, c.digits)))
            })
            .flatten();
            if let Some(text) = text {
                copy_text(&text);
            }
        }
        c if (CMD_OPEN..CMD_OPEN + config::MAX_ROWS).contains(&c) => {
            let sym = with_ui(|u| {
                let cell = u.view.cells.get(c - CMD_OPEN)?;
                u.cfg.rows.iter().find(|r| r.label == cell.label).map(|r| r.symbol.clone())
            })
            .flatten();
            if let Some(sym) = sym {
                open_cnbc(hwnd, &sym);
            }
        }
        CMD_PICK if ultra => apply_picker(hwnd, !picked),
        c if ultra && (CMD_ALERT..CMD_ALERT + list.len() * 4).contains(&c) => {
            let (i, step) = ((c - CMD_ALERT) / 4, (c - CMD_ALERT) % 4);
            let label = list.get(i).map(|r| r.label.clone());
            let price = with_ui(|u| {
                u.view.cells.iter().find(|c| Some(&c.label) == label.as_ref()).and_then(|c| alerts::reference_price(c, &u.cfg, crate::source::now_s()))
            })
            .flatten();
            if let (Some(label), Some(price)) = (label, price) {
                let (pct, up) = ALERT_STEPS[step];
                let level = price * (1.0 + if up { pct } else { -pct } / 100.0);
                with_ui(|u| {
                    if !u.cfg.alerts.iter().any(|a| a.label == label) {
                        u.cfg.alerts.push(config::Alert { label: label.clone(), ..Default::default() });
                    }
                    if let Some(a) = u.cfg.alerts.iter_mut().find(|a| a.label == label) {
                        if up {
                            a.above = Some(level);
                        } else {
                            a.below = Some(level);
                        }
                        // A fresh level may already be met: let it fire on the next poll.
                        a.last = 0.0;
                    }
                    config::save(&u.cfg);
                });
            }
        }
        c if ultra && (CMD_ALERT_CLEAR..CMD_ALERT_CLEAR + list.len()).contains(&c) => {
            let label = list[c - CMD_ALERT_CLEAR].label.clone();
            with_ui(|u| {
                u.cfg.alerts.retain(|a| a.label != label);
                config::save(&u.cfg);
            });
        }
        _ => {}
    }
}

/// The Position menu moved the strip: new font (the taskbar grid uses `dock_font_pt`), topmost
/// state (forced on the taskbar), place, show.
fn apply_placement(hwnd: HWND) {
    let top = with_ui(|u| {
        unsafe { DeleteObject(u.font) };
        u.font = make_font(&u.cfg, u.dpi);
        u.placed = false;
        u.suppressed = false;
        config::save(&u.cfg);
        u.cfg.topmost || u.cfg.placement().on_taskbar()
    })
    .unwrap_or(true);
    set_topmost(hwnd, top);
    relayout_and_place(hwnd);
    apply_visibility(hwnd);
    dock_check(hwnd);
}

/// The Symbols menu changed `cfg.rows`: save it, hand the list to the feed through the board, and
/// re-fit the strip, or resize the open panel in place (it stays centred).
fn apply_rows(hwnd: HWND) {
    let Some(panel) = with_ui(|u| {
        config::save(&u.cfg);
        if let Ok(mut b) = u.board.lock() {
            b.set_rows(&u.cfg.rows);
            u.view = b.clone();
        }
        u.hist.record(&u.view, crate::source::now_s());
        u.mode == Mode::Panel
    }) else {
        return;
    };
    if panel { unsafe { InvalidateRect(hwnd, null(), 0); } }
    else { relayout_and_place(hwnd); }
    unsafe { PostMessageW(hwnd, WM_FEED, 0, 0) };
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
        let fresh = make_icon(theme::link_color(t, u.view.link));
        if !u.icon.is_null() {
            unsafe { DestroyIcon(u.icon) };
        }
        u.icon = fresh;
        u.icon_link = Some(u.view.link);
        config::save(&u.cfg);
        Some((u.icon, tooltip(&u.view)))
    })
    .flatten()
    else {
        return;
    };
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
            PostMessageW(hwnd, WM_DOCK, 0, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_FGCHANGE => {
            dock_check(hwnd);
            SetTimer(hwnd, TIMER_RECHECK, 150, None);
            0
        }
        WM_ALERT => {
            let failed = wp != 0;
            with_ui(|u| {
                u.alert_note = Some(if failed { "alert kept here: webhook failed" } else { "alert: webhook sent" }.into())
            });
            if with_ui(|u| u.mode == Mode::Panel).unwrap_or(false) {
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
                Collapse,
                TogglePicker,
                ToggleChart,
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
                    Mode::Strip if u.cfg.placement() != Placement::Floating => Act::Expand,
                    Mode::Panel if panel::in_rect(&g.btn, x, y) => Act::Collapse,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.gear_btn, x, y) => Act::Settings,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.pick_btn, x, y) => Act::TogglePicker,
                    Mode::Panel if u.profile.ultra && panel::in_rect(&g.chart_btn, x, y) => Act::ToggleChart,
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
                Some(Act::Select(label)) => {
                    with_ui(|u| {
                        // The same card again unlocks it: a lock that cannot be undone is a trap.
                        u.selected = toggle_lock(u.selected.as_deref(), label);
                        u.open_hover = false;
                    });
                    InvalidateRect(hwnd, null(), 0);
                }
                Some(Act::Symbol(sym)) if with_ui(|u| config::toggle(&mut u.cfg, &sym)).unwrap_or(false) => {
                    apply_rows(hwnd)
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
                    return (false, false);
                }
                let g = panel_geo(u);
                let at = panel::hit(&g, u.view.cells.len(), u.dpi, x, y, now).map(snap).unwrap_or(away);
                let btn = panel::in_rect(&g.btn, x, y);
                let pick = u.profile.ultra && panel::in_rect(&g.pick_btn, x, y);
                let chart_btn = u.profile.ultra && panel::in_rect(&g.chart_btn, x, y);
                let gear = u.profile.ultra && panel::in_rect(&g.gear_btn, x, y);
                let open = panel::open_btn_at(&g, selected_row(u), u.dpi, x, y);
                let track = !u.tracking;
                u.tracking = true;
                // Hover moves the crosshair and lights buttons, and nothing else. In particular it no
                // longer re-targets the chart: that is the lock's job.
                let changed = btn != u.hover
                    || pick != u.pick_hover
                    || chart_btn != u.chart_hover
                    || gear != u.gear_hover
                    || open != u.open_hover
                    || at != u.hover_at.unwrap_or(away);
                u.hover = btn;
                u.pick_hover = pick;
                u.chart_hover = chart_btn;
                u.gear_hover = gear;
                u.open_hover = open;
                u.hover_at = (at.row != usize::MAX).then_some(at);
                (changed, track)
            });
            if let Some((changed, track)) = state {
                if track {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    TrackMouseEvent(&mut tme);
                }
                if changed {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_MOUSELEAVE => {
            let was = with_ui(|u| {
                u.tracking = false;
                let was = u.hover || u.pick_hover || u.chart_hover || u.gear_hover || u.open_hover || u.hover_at.is_some();
                u.hover = false;
                u.pick_hover = false;
                u.chart_hover = false;
                u.gear_hover = false;
                u.open_hover = false;
                u.hover_at = None;
                was
            });
            if was == Some(true) {
                InvalidateRect(hwnd, null(), 0);
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
            let step = with_ui(|u| (240 * u.dpi as i32 / 96).max(1)).unwrap_or(240);
            let delta = if wp == VK_PRIOR as usize { -step } else if wp == VK_NEXT as usize { step } else if wp == VK_HOME as usize { -i32::MAX } else { i32::MAX };
            scroll_picker(hwnd, delta);
            0
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
            menu(hwnd, at);
            0
        }
        WM_EXITSIZEMOVE => {
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            with_ui(|u| match u.mode {
                Mode::Panel => {
                    // The frame is the user's now: remember it, and only write when it changed.
                    let frame = [r.left, r.top, r.right - r.left, r.bottom - r.top];
                    // A dragged edge makes the width the user's, so closing the chart must not undo it.
                    if u.chart_grew.is_some_and(|(_, grown)| grown.right - grown.left != frame[2]) {
                        u.chart_grew = None;
                    }
                    if u.cfg.panel != Some(frame) {
                        u.cfg.panel = Some(frame);
                        config::save(&u.cfg);
                    }
                }
                Mode::Strip if u.cfg.placement() == Placement::Floating => {
                    u.strip_rect = r;
                    u.cfg.x = Some(r.left);
                    u.cfg.y = Some(r.top);
                    config::save(&u.cfg);
                }
                _ => {}
            });
            0
        }
        WM_DPICHANGED => {
            let dpi = (wp >> 16) as u32 & 0xffff;
            let open = with_ui(|u| {
                unsafe { DeleteObject(u.font) };
                u.dpi = dpi;
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
            tray_delete(hwnd);
            with_ui(|u| config::save(&u.cfg));
            PostQuitMessage(0);
            0
        }
        _ => {
            // Explorer restarted: the tray icon is gone and must be re-added, and the taskbar moved.
            let recreated = with_ui(|u| (u.taskbar_created, u.icon, tooltip(&u.view)));
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
            icon: make_icon(theme::link_color(theme, Link::Down)),
            icon_link: Some(Link::Down),
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
            pick_open: false,
            picker_scroll: 0,
            wheel_remainder: 0,
            chart: false,
            selected: None,
            chart_grew: None,
            hover_at: None,
            alert_note: None,
            tracking: false,
            prev_fg: null_mut(),
            tip_at: None,
            tray_click_at: None,
            back: None,
            anim: None,
            motion: allow_motion(),
        };
        let (icon, tip) = (ui.icon, tooltip(&ui.view));
        UI.with(|c| *c.borrow_mut() = Some(ui));

        relayout_and_place(hwnd);
        tray(hwnd, NIM_ADD, icon, &tip);
        apply_visibility(hwnd);

        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, null_mut(), Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        SetTimer(hwnd, TIMER_DOCK, 500, None);

        let target = hwnd as isize;
        dock::spawn_probe(Box::new(move || {
            PostMessageW(target as HWND, WM_DOCK, 0, 0);
        }));
        crate::source::spawn(cfg, board, Box::new(move || {
            PostMessageW(target as HWND, WM_FEED, 0, 0);
        }));

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
}
