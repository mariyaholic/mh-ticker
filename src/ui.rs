//! The strip (borderless, layered, always-on-top popup drawn with GDI) and the tray icon.
//! Plain Win32: no GUI toolkit, no GPU context, which is what keeps the process to a few MB.
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

use crate::config::{self, Config, Placement};
use crate::dock;
use crate::panel;
use crate::model::{fmt_change, fmt_price, Board, Cell, Link};
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_FEED: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
/// The free taskbar span moved (probe thread), or the display/taskbar changed.
const WM_DOCK: u32 = WM_APP + 3;
/// The foreground window changed (WinEvent hook).
const WM_FGCHANGE: u32 = WM_APP + 4;
const TRAY_ID: u32 = 1;
const TIMER_DOCK: usize = 1;
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
/// Symbols submenu: CMD_SYM + index into `config::menu_list`.
const CMD_SYM: usize = 2000;
/// Position submenu: CMD_POS + index into `Placement::ALL`, CMD_MON + index into `dock::monitors()`.
const CMD_POS: usize = 4000;
const CMD_MON: usize = 4100;
/// Auto takes a taskbar gap at least this wide (96-dpi px); a forced side needs `MIN_FORCED_GAP`.
const MIN_GAP: i32 = 140;
const MIN_FORCED_GAP: i32 = 60;
/// Not exported by windows-sys 0.59's WindowsAndMessaging.
const WM_MOUSELEAVE: u32 = 0x02A3;
const EXPAND_MS: f64 = 230.0;
const COLLAPSE_MS: f64 = 190.0;
const TIP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// A second tray click inside this window (a double-click) is ignored.
const TRAY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
pub(crate) const BG: COLORREF = rgb(18, 20, 24);
pub(crate) const LABEL: COLORREF = rgb(140, 148, 160);
pub(crate) const PRICE: COLORREF = rgb(235, 238, 242);
pub(crate) const DIM: COLORREF = rgb(105, 110, 118);
pub(crate) const UP: COLORREF = rgb(80, 200, 120);
pub(crate) const DOWN: COLORREF = rgb(235, 90, 90);
pub(crate) const AMBER: COLORREF = rgb(240, 180, 60);

static STRIP: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn link_color(link: Link) -> COLORREF {
    match link {
        Link::Live => UP,
        Link::Partial => AMBER,
        Link::Down => DOWN,
    }
}

pub(crate) fn link_name(link: Link) -> &'static str {
    match link {
        Link::Live => "CNBC live",
        Link::Partial => "CNBC, some prices not fresh",
        Link::Down => "offline",
    }
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
    font: HFONT,
    bg: HBRUSH,
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
    /// Panel fonts, made on first expand.
    fonts: Option<panel::Fonts>,
    hist: panel::History,
    /// The minimise button is hovered.
    hover: bool,
    tracking: bool,
    /// Foreground window before the panel opened; it gets focus back on collapse.
    prev_fg: HWND,
    /// Last tray tooltip refresh.
    tip_at: Option<Instant>,
    tray_click_at: Option<Instant>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// False when another MH Ticker already runs (one strip, one tray icon).
pub fn single_instance() -> bool {
    let name = wide("Local\\MH_Ticker_single_instance");
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
fn cell_texts(c: &Cell) -> (String, String, COLORREF, Option<(String, COLORREF)>) {
    let label = c.label.clone();
    match &c.quote {
        Some(q) => {
            let change = q.change_pct.map(|p| {
                let col = if c.stale { DIM } else if p >= 0.0 { UP } else { DOWN };
                (fmt_change(p), col)
            });
            (label, fmt_price(q.price, c.digits), if c.stale { DIM } else { PRICE }, change)
        }
        None => (label, "--".into(), DIM, None),
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
fn layout_row(dc: HDC, cells: &[Cell], dpi: u32) -> Frame {
    let s = |v: i32| v * dpi as i32 / 96;
    let (px, py) = (s(10), s(5));
    let (_, line_h) = text_size(dc, "Xg");
    let mut f = Frame::default();
    let mut x = px;
    for (i, c) in cells.iter().enumerate() {
        let (label, price, pc, change) = cell_texts(c);
        let mut push = |t: String, col: COLORREF| {
            let (w, _) = text_size(dc, &t);
            f.runs.push(text_run(x, py, &t, col));
            x += w;
        };
        if i > 0 {
            push("    ".into(), LABEL);
        }
        push(format!("{label} "), LABEL);
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
fn layout_grid(dc: HDC, cells: &[Cell], dpi: u32, rows: usize, show_change: bool) -> Frame {
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
        let texts: Vec<_> = col.iter().map(cell_texts).collect();
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
            f.runs.push(text_run(x, y, label, LABEL));
            let w = text_size(dc, price).0;
            // Without the change column the price itself carries the day's direction.
            let pc = match change {
                Some((_, col)) if cw == 0 && *pc == PRICE => *col,
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
        let frame = match avail {
            None => layout_row(dc, cells, u.dpi),
            Some(avail) => {
                let rows = u.cfg.dock_rows.max(1) as usize;
                let mut pick = None;
                'fit: for k in (1..=cells.len()).rev() {
                    for show_change in [true, false] {
                        let f = layout_grid(dc, &cells[..k], u.dpi, rows, show_change);
                        if f.w <= avail {
                            pick = Some(f);
                            break 'fit;
                        }
                    }
                }
                pick.unwrap_or_else(|| layout_grid(dc, &cells[..cells.len().min(1)], u.dpi, rows, false))
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
        let brush = CreateSolidBrush(link_color(u.view.link));
        let old_brush = SelectObject(dc, brush);
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        Ellipse(dc, dx, dy, dx + d, dy + d);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_font);
        DeleteObject(brush);
    }
}

fn draw_panel(u: &Ui, dc: HDC, w: i32, h: i32) {
    if let Some(f) = &u.fonts {
        panel::draw(dc, w, h, &u.view, &u.hist, f, u.dpi, u.hover, crate::source::now_s());
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

fn paint(u: &Ui) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(u.hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(u.hwnd, &mut rc);
        let (w, h) = (rc.right.max(1), rc.bottom.max(1));
        let bmp = snapshot(u.hwnd, w, h, |dc| match (u.mode, &u.morph) {
            (Mode::Morph, Some(m)) => draw_morph(m, dc, w, h),
            (Mode::Panel, _) => draw_panel(u, dc, w, h),
            _ => draw_strip(u, dc),
        });
        let mem = CreateCompatibleDC(hdc);
        let old = SelectObject(mem, bmp);
        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        EndPaint(u.hwnd, &ps);
    }
}

fn tooltip(view: &Board) -> String {
    let mut s = format!("MH Ticker - {}", link_name(view.link));
    for c in &view.cells {
        if let Some(q) = &c.quote {
            s.push_str(&format!("\n{} {}", c.label, fmt_price(q.price, c.digits)));
            if c.stale {
                s.push_str(" (stale)");
            }
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
    let Some(tray_update) = with_ui(|u| {
        if let Ok(b) = u.board.lock() {
            u.view = b.clone();
        }
        u.hist.record(&u.view, crate::source::now_s());
        let relinked = u.icon_link != Some(u.view.link);
        if relinked {
            let fresh = make_icon(link_color(u.view.link));
            if !u.icon.is_null() {
                unsafe { DestroyIcon(u.icon) };
            }
            u.icon = fresh;
            u.icon_link = Some(u.view.link);
        }
        let due = relinked || u.tip_at.is_none_or(|t| t.elapsed() >= TIP_EVERY);
        due.then(|| {
            u.tip_at = Some(Instant::now());
            (u.icon, tooltip(&u.view))
        })
    }) else {
        return;
    };
    if let Some((icon, tip)) = tray_update {
        tray(hwnd, NIM_MODIFY, icon, &tip);
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
        let x = wa.left + (wa.right - wa.left - w) / 2;
        let y = wa.top + (wa.bottom - wa.top - h) / 2;
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
            u.fonts = Some(panel::Fonts::new(&u.cfg.font, u.dpi));
        }
        u.hover = false;
        u.prev_fg = unsafe { GetForegroundWindow() };
        let a = u.strip_rect;
        let g = panel::geometry(u.view.cells.len(), u.dpi);
        let b = panel_rect(&a, g.w, g.h);
        let fsz = (u.frame.w.max(1), u.frame.h.max(1));
        let from = snapshot(hwnd, fsz.0, fsz.1, |dc| draw_strip(u, dc));
        let to = snapshot(hwnd, g.w, g.h, |dc| draw_panel(u, dc, g.w, g.h));
        Some((a, b, (from, fsz), (to, (g.w, g.h)), u.cfg.opacity))
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

fn menu(hwnd: HWND) {
    let Some((visible, top, docked, status, open, list, shown)) = with_ui(|u| {
        let list = config::menu_list(&u.cfg);
        let shown: Vec<bool> = list.iter().map(|r| config::is_shown(&u.cfg, &r.symbol)).collect();
        let note = if u.view.note.is_empty() { link_name(u.view.link).to_string() } else { u.view.note.clone() };
        (u.cfg.strip_visible, u.cfg.topmost, u.cfg.placement().on_taskbar(), note, u.mode != Mode::Strip, list, shown)
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
        GetCursorPos(&mut pt);
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
        (u.mode == Mode::Panel).then(|| {
            let g = panel::geometry(u.view.cells.len(), u.dpi);
            let mut cur: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetWindowRect(hwnd, &mut cur) };
            panel_rect(&cur, g.w, g.h)
        })
    }) else {
        return;
    };
    match panel {
        Some(r) => unsafe {
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            InvalidateRect(hwnd, null(), 0);
        },
        None => relayout_and_place(hwnd),
    }
    unsafe { PostMessageW(hwnd, WM_FEED, 0, 0) };
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
        WM_PAINT => {
            if with_ui(|u| paint(u)).is_none() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            0
        }
        WM_ERASEBKGND => 1,
        // Floating strip: the whole strip is a drag handle. Any other placement: fixed, a click opens the panel.
        // Panel: the header (minus the button) drags it.
        WM_NCHITTEST => {
            let Some((mode, dock, header, btn)) = with_ui(|u| {
                let g = panel::geometry(u.view.cells.len(), u.dpi);
                (u.mode, u.cfg.placement() != Placement::Floating, g.header, g.btn)
            }) else {
                return HTCLIENT as LRESULT;
            };
            match mode {
                Mode::Strip if !dock => HTCAPTION as LRESULT,
                Mode::Panel => {
                    let mut pt = POINT { x: (lp & 0xffff) as i16 as i32, y: ((lp >> 16) & 0xffff) as i16 as i32 };
                    ScreenToClient(hwnd, &mut pt);
                    if pt.y < header && !panel::in_rect(&btn, pt.x, pt.y) {
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
            let hand = with_ui(|u| (u.mode == Mode::Strip && fixed(u)) || (u.mode == Mode::Panel && u.hover)).unwrap_or(false);
            if hand && (lp & 0xffff) as u32 == HTCLIENT {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            let what = with_ui(|u| {
                let g = panel::geometry(u.view.cells.len(), u.dpi);
                (u.mode, u.cfg.placement() != Placement::Floating, panel::in_rect(&g.btn, x, y))
            });
            match what {
                Some((Mode::Strip, true, _)) => expand(hwnd),
                Some((Mode::Panel, _, true)) => collapse(hwnd),
                _ => {}
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            let state = with_ui(|u| {
                if u.mode != Mode::Panel {
                    return (false, false);
                }
                let g = panel::geometry(u.view.cells.len(), u.dpi);
                let over = panel::in_rect(&g.btn, x, y);
                let track = !u.tracking;
                u.tracking = true;
                let changed = over != u.hover;
                u.hover = over;
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
                std::mem::replace(&mut u.hover, false)
            });
            if was == Some(true) {
                InvalidateRect(hwnd, null(), 0);
            }
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
            menu(hwnd);
            0
        }
        WM_EXITSIZEMOVE => {
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            with_ui(|u| {
                if u.cfg.placement() == Placement::Floating && u.mode == Mode::Strip {
                    u.strip_rect = r;
                    u.cfg.x = Some(r.left);
                    u.cfg.y = Some(r.top);
                    config::save(&u.cfg);
                }
            });
            0
        }
        WM_DPICHANGED => {
            let dpi = (wp >> 16) as u32 & 0xffff;
            let open = with_ui(|u| {
                unsafe { DeleteObject(u.font) };
                u.dpi = dpi;
                u.font = make_font(&u.cfg, dpi);
                u.fonts = u.fonts.take().map(|_| panel::Fonts::new(&u.cfg.font, dpi));
                (u.mode == Mode::Panel).then(|| panel::geometry(u.view.cells.len(), dpi))
            })
            .flatten();
            match open {
                Some(g) => {
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    let b = panel_rect(&r, g.w, g.h);
                    SetWindowPos(hwnd, null_mut(), b.left, b.top, g.w, g.h, SWP_NOZORDER | SWP_NOACTIVATE);
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
                WM_RBUTTONUP => menu(hwnd),
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
        let class = wide("MH_Ticker_Strip");
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
        let title = wide("MH Ticker");
        let ex = WS_EX_TOOLWINDOW | WS_EX_LAYERED | if cfg.topmost || cfg.placement().on_taskbar() { WS_EX_TOPMOST } else { 0 };
        let hwnd = CreateWindowExW(ex, class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 10, 10, null_mut(), null_mut(), inst, null());
        if hwnd.is_null() {
            return;
        }
        STRIP.store(hwnd as isize, Ordering::Relaxed);
        SetLayeredWindowAttributes(hwnd, 0, cfg.opacity, LWA_ALPHA);
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as _, &corner as *const _ as _, std::mem::size_of_val(&corner) as u32);

        let dpi = GetDpiForWindow(hwnd).max(96);
        let tc_name = wide("TaskbarCreated");
        let view = board.lock().map(|b| b.clone()).unwrap_or_else(|_| Board::new(&cfg.rows));
        let ui = Ui {
            hwnd,
            font: make_font(&cfg, dpi),
            bg: CreateSolidBrush(BG),
            icon: make_icon(link_color(Link::Down)),
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
            fonts: None,
            hist: panel::History::new(),
            hover: false,
            tracking: false,
            prev_fg: null_mut(),
            tip_at: None,
            tray_click_at: None,
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
