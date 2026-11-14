//! The settings window: its class, its state and its messages.
//!
//! One instance. It is a normal top-level window (caption, resizable) whose client area is painted by
//! `settings::draw` from the theme the strip uses. A click or a key runs the same `run_command` or
//! `run_action` the menu ran, then the page is rebuilt from the config, so what is drawn is what is
//! saved.
//!
//! Re-entrancy: `SW` is never borrowed across a call into `run_command`, `run_action`, `with_ui` or any
//! Win32 call that sends messages (`SetWindowPos`, `ShowWindow`, `DestroyWindow`, `MessageBoxW`), because
//! those re-enter `settings_proc`. The window is disabled while a command runs, so a prompt or a
//! confirmation box that command opens is modal over it.

use super::settings_view::{self, Env, MonitorFact};
use super::*;
use crate::settings::{self, Act, BarHit, BarState, Control, Focus, Hit, Key, PageKey, Paint, Section};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWLP_HWNDPARENT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, EnableWindow, SetFocus, TME_LEAVE, TRACKMOUSEEVENT};
use std::cell::Cell;

/// The refresh timer: picks up changes made through the menu, the tray, the hotkey or the feed.
const TIMER_REFRESH: usize = 77;
const REFRESH_MS: u32 = 1000;

struct Sw {
    hwnd: HWND,
    ultra: bool,
    sections: Vec<Section>,
    cur: usize,
    controls: Vec<Control>,
    focus: Focus,
    hover: Option<Hit>,
    scroll: i32,
    theme: &'static Theme,
    face: &'static str,
    zoom: u32,
    fonts: Option<(&'static str, u32, bool, panel::Fonts)>,
    tracking: bool,
    /// The symbol the Symbols and Alerts pages are about (`Env::card`).
    card: Option<String>,
    /// Wheel movement not yet shown as whole pixels (`settings::wheel_step`).
    wheel_rem: i64,
    /// While the scrollbar thumb is dragged: how far below the thumb's top the pointer took hold.
    drag: Option<i32>,
    /// The pointer is over the scrollbar.
    bar_hot: bool,
}

thread_local! {
    static SW: RefCell<Option<Sw>> = const { RefCell::new(None) };
    /// The open window, kept outside `SW` so that finding it never depends on a borrow of the state.
    static OPEN: Cell<isize> = const { Cell::new(0) };
    /// How many times `slow_facts` read the registry and the display list for a fresh refresh.
    static FRESH_READS: Cell<u32> = const { Cell::new(0) };
    /// The autostart flag and the monitor list. Reading them costs a registry call and a display
    /// enumeration, so the 1 s refresh reuses them; they are read again when the window opens, is
    /// activated, a command ran or the displays changed.
    static SLOW: RefCell<Option<(bool, Vec<MonitorFact>)>> = const { RefCell::new(None) };
}

fn slow_facts(fresh: bool) -> (bool, Vec<MonitorFact>) {
    if !fresh {
        if let Some(c) = SLOW.with(|c| c.try_borrow().ok().and_then(|g| g.clone())) {
            return c;
        }
    }
    if fresh {
        FRESH_READS.with(|c| c.set(c.get().wrapping_add(1)));
    }
    let monitors: Vec<MonitorFact> = dock::monitors()
        .into_iter()
        .map(|d| MonitorFact { w: d.bounds.right - d.bounds.left, h: d.bounds.bottom - d.bounds.top, device: d.device, primary: d.primary })
        .collect();
    let auto = autostart::enabled(config::profile().id);
    SLOW.with(|c| {
        if let Ok(mut g) = c.try_borrow_mut() {
            *g = Some((auto, monitors.clone()));
        }
    });
    (auto, monitors)
}

fn with_sw<R>(f: impl FnOnce(&mut Sw) -> R) -> Option<R> {
    SW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Everything a page is built from, read from the main window's state in separate `with_ui` calls
/// (none nests, and none is held across I/O).
struct Snap {
    cfg: Config,
    env: Env,
    theme: &'static Theme,
    face: &'static str,
}

fn snapshot(fresh: bool) -> Option<Snap> {
    let card = with_sw(|s| s.card.clone()).flatten();
    let (cfg, ultra, open, picked, undo, status, adding, hotkey_failed, update_running, report_running, theme, face) = with_ui(|u| {
        let note = if u.view.note.is_empty() { theme::link_name(u.view.link).to_string() } else { u.view.note.clone() };
        (
            u.cfg.clone(),
            u.profile.ultra,
            u.mode != Mode::Strip,
            u.pick_open,
            u.undo.kind().map(|k| k.label()),
            note,
            u.adding.clone(),
            u.hotkey_failed,
            u.update_running,
            u.report_running,
            u.theme,
            u.face,
        )
    })?;
    let list = config::menu_list(&cfg);
    let shown: Vec<bool> = list.iter().map(|r| config::is_shown(&cfg, &r.symbol)).collect();
    let alerts = if ultra { alerts_view(&list, &shown).unwrap_or_default() } else { AlertsView::default() };
    let (autostart, monitors) = slow_facts(fresh);
    let autos = with_ui(|u| {
        u.cfg
            .rows
            .iter()
            .map(|r| {
                let price = u.view.cells.iter().zip(&u.cfg.rows).find(|(_, x)| x.symbol == r.symbol).and_then(|(c, _)| c.quote.as_ref()).map(|q| q.price);
                (r.symbol.clone(), config::auto_digits(&r.symbol, price))
            })
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let env = Env {
        ultra,
        open,
        picked,
        undo,
        status: status.chars().take(90).collect(),
        meter: crate::source::meter_line(),
        autostart,
        hotkey_failed,
        adding,
        monitors,
        alerts,
        reach: reach_view(),
        save_failed: config::save_failed(),
        kept_bad: config::kept_bad_config(),
        update_running,
        report_running,
        version: format!("{} {}", config::profile().title(), env!("CARGO_PKG_VERSION")),
        card,
        autos,
    };
    Some(Snap { cfg, env, theme, face })
}

/// Put a fresh snapshot into the window state. Returns whether anything drawn changed.
fn apply(s: &mut Sw, snap: &Snap) -> bool {
    s.ultra = snap.env.ultra;
    s.sections = Section::for_build(snap.env.ultra);
    s.cur = s.cur.min(s.sections.len() - 1);
    let controls = settings_view::build(&snap.cfg, &snap.env, s.sections[s.cur]);
    let zoom = snap.cfg.zoom(snap.env.ultra);
    let changed = controls != s.controls || !std::ptr::eq(s.theme, snap.theme) || s.face != snap.face || s.zoom != zoom;
    // The focus follows its option by control label and command, because a rebuild can insert or
    // remove controls above it and an index would then point at a different control.
    let old = std::mem::replace(&mut s.controls, controls);
    s.theme = snap.theme;
    s.face = snap.face;
    s.zoom = zoom;
    s.focus = settings::settle(s.focus, &old, s.cur, s.sections.len(), &s.controls);
    changed
}

/// A dark caption for a dark theme, so the title bar does not stay light over dark content.
fn set_dark_caption(hwnd: HWND, t: &Theme) {
    let dark: i32 = settings::is_dark(t) as i32;
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const i32 as _, 4) };
}

fn invalidate(hwnd: HWND) {
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

/// Who owns the settings window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    /// The strip window, which is the panel while it is open.
    Panel,
    Nobody,
}

/// What to do to the window's place in the z-order once the owner is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Restack {
    Keep,
    /// Put it on top of its new owner.
    Raise,
    /// Take it out of the topmost band it shared with its owner.
    NotTopmost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnerPlan {
    owner: Owner,
    restack: Restack,
}

/// The owner the window should have and whether to restack it. While the panel is open the strip owns
/// the window, so it stays above the panel. Otherwise nobody owns it: an owned window follows its
/// owner's topmost state, and a docked strip is topmost, which would keep the window above every other
/// app. Changing the owner does not move the window in the z-order, so a window that has just lost its
/// owner is taken out of the topmost band explicitly (a floating strip may be topmost too), and one
/// that has just gained the panel as its owner is raised above it. `owned_now` is whether it has an
/// owner at the moment.
fn owner_plan(panel_open: bool, owned_now: bool) -> OwnerPlan {
    match (panel_open, owned_now) {
        (true, false) => OwnerPlan { owner: Owner::Panel, restack: Restack::Raise },
        (true, true) => OwnerPlan { owner: Owner::Panel, restack: Restack::Keep },
        (false, true) => OwnerPlan { owner: Owner::Nobody, restack: Restack::NotTopmost },
        (false, false) => OwnerPlan { owner: Owner::Nobody, restack: Restack::Keep },
    }
}

fn sync_owner(hwnd: HWND, open: bool) {
    unsafe {
        let now = GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT);
        let plan = owner_plan(open, now != 0);
        let want = match plan.owner {
            Owner::Panel => main_hwnd() as isize,
            Owner::Nobody => 0,
        };
        if now != want {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, want);
        }
        let place = |after: HWND| SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        match plan.restack {
            Restack::Keep => {}
            Restack::Raise => {
                place(HWND_TOP);
            }
            Restack::NotTopmost => {
                place(HWND_NOTOPMOST);
            }
        }
    }
}

/// Re-read the config and repaint when something differs.
fn refresh(hwnd: HWND) {
    refresh_with(hwnd, false);
}

/// `refresh` that also re-reads the autostart flag and the monitor list.
fn refresh_fresh(hwnd: HWND) {
    refresh_with(hwnd, true);
}

fn refresh_with(hwnd: HWND, fresh: bool) {
    let Some(snap) = snapshot(fresh) else { return };
    sync_owner(hwnd, snap.env.open);
    if with_sw(|s| apply(s, &snap)).unwrap_or(false) {
        if let Some(t) = with_sw(|s| s.theme) {
            set_dark_caption(hwnd, t);
        }
        invalidate(hwnd);
    }
}

fn main_hwnd() -> HWND {
    STRIP.load(Ordering::Relaxed) as HWND
}

fn window_dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}

/// What opening the window does when it may already be open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reopen {
    Create,
    /// Bring the open window to the front, on the section it shows.
    Front,
    /// The same, after restoring it from the taskbar.
    RestoreAndFront,
}

fn reopen(alive: bool, minimized: bool) -> Reopen {
    match (alive, minimized) {
        (false, _) => Reopen::Create,
        (true, false) => Reopen::Front,
        (true, true) => Reopen::RestoreAndFront,
    }
}

/// Open the window, or bring the open one to the front. There is one window: a second request shows
/// the first, on the section it is on and with its scroll position, and never opens another.
pub(super) fn open_settings(main: HWND) {
    let open = OPEN.with(|c| c.get()) as HWND;
    let alive = !open.is_null() && unsafe { IsWindow(open) } != 0;
    let minimized = alive && unsafe { IsIconic(open) } != 0;
    match reopen(alive, minimized) {
        Reopen::Create => {}
        plan => unsafe {
            if plan == Reopen::RestoreAndFront {
                ShowWindow(open, SW_RESTORE);
            }
            // The panel may have opened or closed since the window last refreshed: settle its owner
            // from the cached facts before raising it, so it lands above the panel. Raising it
            // activates it, and `WM_ACTIVATE` does the one fresh read.
            refresh(open);
            read_fresh_after(|| {
                BringWindowToTop(open);
                SetForegroundWindow(open);
            }, || refresh_fresh(open));
            return;
        },
    }
    let Some(snap) = snapshot(true) else { return };
    unsafe {
        let inst = GetModuleHandleW(null());
        let class = wide(&format!("{}_Settings", config::profile().id));
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(settings_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: LoadIconW(inst, 1 as _),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
        };
        // A second registration fails harmlessly: the class from the first open is still there.
        RegisterClassW(&wc);
        let title = wide(&format!("{} settings", config::profile().title()));
        let style = WS_OVERLAPPEDWINDOW;
        // The window opens on the monitor the strip is on, so its size uses that monitor's DPI.
        let mon = if main.is_null() { GetDpiForSystem().max(96) } else { window_dpi(main) };
        let eff = zoom::effective_dpi(mon, snap.cfg.zoom(snap.env.ultra));
        let (cw, ch) = settings::window_size(eff);
        let mut rc = RECT { left: 0, top: 0, right: cw, bottom: ch };
        AdjustWindowRectExForDpi(&mut rc, style, 0, 0, mon);
        let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
        let (x, y) = if main.is_null() {
            (CW_USEDEFAULT, CW_USEDEFAULT)
        } else {
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST), &mut mi) != 0 {
                let w = mi.rcWork;
                settings::place(settings::Rc::new(w.left, w.top, w.right, w.bottom), ww, wh)
            } else {
                (CW_USEDEFAULT, CW_USEDEFAULT)
            }
        };
        // Owned by the strip while the panel is open (so it stays above the panel), unowned otherwise;
        // `sync_owner` keeps that true on every refresh. WS_EX_APPWINDOW keeps a taskbar button and an
        // Alt+Tab entry for an owned window.
        let owner = if snap.env.open { main } else { null_mut() };
        let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class.as_ptr(), title.as_ptr(), style, x, y, ww, wh, owner, null_mut(), inst, null());
        if hwnd.is_null() {
            return;
        }
        let sections = Section::for_build(snap.env.ultra);
        let mut sw = Sw {
            hwnd,
            ultra: snap.env.ultra,
            sections,
            cur: 0,
            controls: Vec::new(),
            focus: Hit::Nav(0),
            hover: None,
            scroll: 0,
            theme: snap.theme,
            face: snap.face,
            zoom: 100,
            fonts: None,
            tracking: false,
            card: None,
            wheel_rem: 0,
            drag: None,
            bar_hot: false,
        };
        apply(&mut sw, &snap);
        set_dark_caption(hwnd, sw.theme);
        SW.with(|c| *c.borrow_mut() = Some(sw));
        OPEN.with(|c| c.set(hwnd as isize));
        SetTimer(hwnd, TIMER_REFRESH, REFRESH_MS, None);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        // The snapshot above was fresh; activation reads again only when nothing else did.
        read_fresh_after(|| { SetForegroundWindow(hwnd); }, || refresh_fresh(hwnd));
    }
}

/// Close the window when the app quits.
pub(super) fn close() {
    if let Some(h) = with_sw(|s| s.hwnd) {
        unsafe {
            if IsWindow(h) != 0 {
                DestroyWindow(h);
            }
        }
    }
}

/// Run what an option stands for, exactly as its menu entry did.
fn activate(hwnd: HWND, act: Act) {
    let main = main_hwnd();
    let listed = with_ui(|u| Listed { open: u.mode != Mode::Strip, visible: u.cfg.strip_visible, top: u.cfg.topmost, picked: u.pick_open, ultra: u.profile.ultra });
    unsafe { EnableWindow(hwnd, 0) };
    if let Some(listed) = listed {
        match &act {
            Act::Cmd(id) => run_command(main, *id, listed),
            Act::Do(action) => run_action(main, action),
            // Handled by the window before a command is chosen (`choose`); nothing to run.
            Act::Card(_) => {}
        }
    }
    unsafe {
        if IsWindow(hwnd) == 0 {
            return;
        }
        EnableWindow(hwnd, 1);
    }
    // SetFocus activates the window when it was not active, and `WM_ACTIVATE` then reads the registry
    // and the display list. Reading them again here would be a second read for the same command.
    read_fresh_after(|| unsafe { SetFocus(hwnd); }, || refresh_fresh(hwnd));
}

/// Run `raise` (a call that may activate the window), then `read` only when nothing read the registry
/// and the display list during `raise`. Opening, reopening and finishing a command all use this, so each
/// does one fresh read.
fn read_fresh_after(raise: impl FnOnce(), read: impl FnOnce()) {
    read_fresh_after_counted(|| FRESH_READS.with(|c| c.get()), raise, read);
}

fn read_fresh_after_counted(count: impl Fn() -> u32, raise: impl FnOnce(), read: impl FnOnce()) {
    let before = count();
    raise();
    if needs_fresh_read(before, count()) {
        read();
    }
}

/// Whether the fresh refresh a command needs still has to run: it does when nothing read the registry
/// and the display list since the count was `before`.
fn needs_fresh_read(before: u32, now: u32) -> bool {
    before == now
}

/// What activating the focused option or a clicked one means: a section change for the sidebar, a
/// command for an option.
enum Do {
    Section(usize),
    Run(Act),
    /// Pick the symbol the pages are about (the window's own state, not a command).
    Card(String),
}

fn choose(s: &mut Sw, at: Hit) -> Option<Do> {
    s.focus = at;
    match at {
        Hit::Nav(i) => (i != s.cur).then_some(Do::Section(i)),
        Hit::Opt(ci, oi) => settings_view::opt_of(&s.controls, ci, oi).filter(|o| o.enabled).map(|o| match &o.act {
            Act::Card(sym) => Do::Card(sym.clone()),
            act => Do::Run(act.clone()),
        }),
    }
}

/// A new section starts at the top with nothing hovered, no thumb held and no wheel remainder: a
/// fraction of a pixel carried over from the other page would move this one.
fn reset_view(s: &mut Sw) {
    s.scroll = 0;
    s.controls = Vec::new();
    s.hover = None;
    s.wheel_rem = 0;
    s.drag = None;
    s.bar_hot = false;
}

fn go_section(hwnd: HWND, i: usize) {
    let Some(snap) = snapshot(true) else { return };
    with_sw(|s| {
        s.cur = i.min(s.sections.len() - 1);
        reset_view(s);
        apply(s, &snap);
        s.focus = Hit::Nav(s.cur);
    });
    invalidate(hwnd);
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    (rc.right, rc.bottom)
}

fn eff_dpi(s: &Sw, hwnd: HWND) -> u32 {
    zoom::effective_dpi(window_dpi(hwnd), s.zoom)
}

fn current_layout(s: &Sw, hwnd: HWND) -> settings::Layout {
    let (w, h) = client_size(hwnd);
    settings::layout(eff_dpi(s, hwnd), w, h, s.sections.len(), &s.controls, s.scroll)
}

fn bar_state(s: &Sw) -> BarState {
    if s.drag.is_some() {
        BarState::Dragging
    } else if s.bar_hot {
        BarState::Hot
    } else {
        BarState::Idle
    }
}

/// A press at (`x`, `y`) on the scrollbar. `None` when the point is not on it; `Some(true)` when the
/// thumb was taken (the window captures the mouse), `Some(false)` when a click in the track paged.
fn press_bar(s: &mut Sw, l: &settings::Layout, x: i32, y: i32) -> Option<bool> {
    let bar = settings::scrollbar(l)?;
    let hit = settings::bar_hit(&bar, x, y)?;
    match hit {
        BarHit::Thumb => {
            s.drag = Some(y - bar.thumb.top);
            Some(true)
        }
        BarHit::PageUp | BarHit::PageDown => {
            s.scroll = settings::page_click(l, hit);
            Some(false)
        }
    }
}

/// The pointer moved to (`x`, `y`). Follows the thumb while it is held, else tracks whether the pointer
/// is over the bar. Returns whether something drawn changed.
fn move_over_bar(s: &mut Sw, l: &settings::Layout, x: i32, y: i32) -> bool {
    let bar = settings::scrollbar(l);
    if let (Some(grab), Some(b)) = (s.drag, bar) {
        let to = settings::drag_scroll(&b, grab, y);
        return std::mem::replace(&mut s.scroll, to) != to;
    }
    let hot = bar.and_then(|b| settings::bar_hit(&b, x, y)).is_some();
    std::mem::replace(&mut s.bar_hot, hot) != hot
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let (w, h) = client_size(hwnd);
        if w > 0 && h > 0 {
            let mem = CreateCompatibleDC(dc);
            let bmp = CreateCompatibleBitmap(dc, w, h);
            let old = SelectObject(mem, bmp);
            with_sw(|s| {
                let eff = eff_dpi(s, hwnd);
                let wanted = (s.face, eff, s.theme.is_pixel());
                if s.fonts.as_ref().map(|(f, d, px, _)| (*f, *d, *px)) != Some(wanted) {
                    s.fonts = Some((s.face, eff, s.theme.is_pixel(), panel::Fonts::new(s.face, eff, s.theme.is_pixel())));
                }
                let l = settings::layout(eff, w, h, s.sections.len(), &s.controls, s.scroll);
                s.scroll = l.scroll;
                if let Some((_, _, _, fonts)) = &s.fonts {
                    let p = Paint {
                        theme: s.theme,
                        fonts,
                        title: "Settings",
                        sections: &s.sections,
                        cur: s.cur,
                        controls: &s.controls,
                        layout: &l,
                        focus: Some(s.focus),
                        hover: s.hover,
                        bar: bar_state(s),
                    };
                    settings::draw(mem, w, h, &p);
                }
            });
            BitBlt(dc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
        EndPaint(hwnd, &ps);
    }
}

fn key_of(vk: u32, shift: bool) -> Option<Key> {
    Some(match vk {
        0x09 if shift => Key::BackTab,
        0x09 => Key::Tab,
        0x25 => Key::Left,
        0x27 => Key::Right,
        0x26 => Key::Up,
        0x28 => Key::Down,
        0x24 => Key::Home,
        0x23 => Key::End,
        _ => return None,
    })
}

/// The scroll offset a key sets, when it scrolls the page: Page Up, Page Down, Home and End. The caller
/// only asks for a focus on a control; Home and End in the sidebar pick the first or last section first.
fn key_scroll(vk: u32, l: &settings::Layout) -> Option<i32> {
    let key = match vk {
        0x21 => PageKey::PageUp,
        0x22 => PageKey::PageDown,
        0x24 => PageKey::Home,
        0x23 => PageKey::End,
        _ => return None,
    };
    Some(settings::scroll_for(l, key))
}

/// A key press. Returns the command to run (outside the borrow), or a section to go to.
fn on_key(hwnd: HWND, vk: u32) -> Option<Do> {
    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
    if vk == 0x1B {
        unsafe { DestroyWindow(hwnd) };
        return None;
    }
    with_sw(|s| {
        if vk == 0x0D || vk == 0x20 {
            let at = s.focus;
            return choose(s, at);
        }
        if vk == 0x21 || vk == 0x22 {
            let l = current_layout(s, hwnd);
            s.scroll = key_scroll(vk, &l).unwrap_or(l.scroll);
            invalidate(hwnd);
            return None;
        }
        let key = key_of(vk, shift)?;
        let was = s.focus;
        s.focus = settings::step(s.focus, key, s.cur, s.sections.len(), &s.controls);
        if let Hit::Opt(ci, _) = s.focus {
            let l = current_layout(s, hwnd);
            s.scroll = key_scroll(vk, &l).unwrap_or_else(|| settings::reveal(&l, ci));
        }
        invalidate(hwnd);
        // The sidebar entry under the focus is the section shown.
        match s.focus {
            Hit::Nav(i) if i != s.cur && was != s.focus => Some(Do::Section(i)),
            _ => None,
        }
    })
    .flatten()
}

fn dispatch(hwnd: HWND, what: Option<Do>) {
    match what {
        Some(Do::Section(i)) => go_section(hwnd, i),
        Some(Do::Run(act)) => activate(hwnd, act),
        Some(Do::Card(sym)) => {
            with_sw(|s| s.card = Some(sym));
            refresh(hwnd);
        }
        None => {}
    }
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_SIZE => {
            invalidate(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            let info = lp as *mut MINMAXINFO;
            if !info.is_null() {
                let dpi = window_dpi(hwnd);
                // The layout runs at the effective DPI, so the smallest size does too.
                let eff = with_sw(|s| eff_dpi(s, hwnd)).unwrap_or(dpi);
                let (cw, ch) = settings::min_size(eff);
                let mut rc = RECT { left: 0, top: 0, right: cw, bottom: ch };
                AdjustWindowRectExForDpi(&mut rc, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
                (*info).ptMinTrackSize = POINT { x: rc.right - rc.left, y: rc.bottom - rc.top };
            }
            0
        }
        WM_DPICHANGED => {
            let r = lp as *const RECT;
            if !r.is_null() {
                let r = *r;
                SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            invalidate(hwnd);
            0
        }
        WM_TIMER if wp == TIMER_REFRESH => {
            refresh(hwnd);
            0
        }
        WM_ACTIVATE => {
            // Coming back to the window picks up what changed meanwhile, the autostart flag and the
            // displays included.
            if (wp & 0xffff) != 0 {
                refresh_fresh(hwnd);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DISPLAYCHANGE => {
            refresh_fresh(hwnd);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            refresh(hwnd);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            let track = with_sw(|s| {
                let l = current_layout(s, hwnd);
                if move_over_bar(s, &l, x, y) {
                    invalidate(hwnd);
                }
                let now = if s.drag.is_some() { None } else { settings::hit(&l, &s.controls, x, y) };
                if now != s.hover {
                    s.hover = now;
                    invalidate(hwnd);
                }
                !std::mem::replace(&mut s.tracking, true)
            })
            .unwrap_or(false);
            if track {
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut t);
            }
            0
        }
        WM_MOUSELEAVE => {
            with_sw(|s| {
                s.tracking = false;
                s.hover = None;
                s.bar_hot = false;
            });
            invalidate(hwnd);
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            let held = with_sw(|s| s.drag.take().is_some()).unwrap_or(false);
            if held {
                if msg == WM_LBUTTONUP {
                    ReleaseCapture();
                }
                invalidate(hwnd);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
            let bar = with_sw(|s| {
                let l = current_layout(s, hwnd);
                press_bar(s, &l, x, y)
            })
            .flatten();
            if let Some(grabbed) = bar {
                if grabbed {
                    SetCapture(hwnd);
                }
                invalidate(hwnd);
                return 0;
            }
            let what = with_sw(|s| {
                let l = current_layout(s, hwnd);
                let at = settings::hit(&l, &s.controls, x, y)?;
                invalidate(hwnd);
                choose(s, at)
            })
            .flatten();
            dispatch(hwnd, what);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xffff) as i16 as i32;
            with_sw(|s| {
                let l = current_layout(s, hwnd);
                let dpi = eff_dpi(s, hwnd);
                s.scroll = l.scroll - settings::wheel_step(delta, dpi, &mut s.wheel_rem);
            });
            invalidate(hwnd);
            0
        }
        WM_KEYDOWN => {
            let what = on_key(hwnd, wp as u32);
            dispatch(hwnd, what);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_REFRESH);
            OPEN.with(|c| c.set(0));
            SLOW.with(|c| {
                if let Ok(mut g) = c.try_borrow_mut() {
                    *g = None;
                }
            });
            SW.with(|c| {
                if let Ok(mut g) = c.try_borrow_mut() {
                    *g = None;
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_commands::Action;

    #[test]
    fn keys_map_to_focus_moves() {
        assert_eq!(key_of(0x09, false), Some(Key::Tab));
        assert_eq!(key_of(0x09, true), Some(Key::BackTab));
        assert_eq!(key_of(0x25, false), Some(Key::Left));
        assert_eq!(key_of(0x28, false), Some(Key::Down));
        assert_eq!(key_of(0x41, false), None);
        assert_eq!(key_of(0x1B, false), None, "Esc closes the window and is not a focus move");
    }

    fn state(controls: Vec<Control>) -> Sw {
        Sw {
            hwnd: null_mut(),
            ultra: true,
            sections: Section::for_build(true),
            cur: 1,
            controls,
            focus: Hit::Nav(1),
            hover: None,
            scroll: 0,
            theme: theme::classic(),
            face: "Segoe UI",
            zoom: 100,
            fonts: None,
            tracking: false,
            card: None,
            wheel_rem: 0,
            drag: None,
            bar_hot: false,
        }
    }

    #[test]
    fn choosing_the_current_section_does_nothing_and_another_changes_it() {
        let mut s = state(Vec::new());
        assert!(choose(&mut s, Hit::Nav(1)).is_none());
        assert!(matches!(choose(&mut s, Hit::Nav(3)), Some(Do::Section(3))));
        assert_eq!(s.focus, Hit::Nav(3), "a click also moves the focus");
    }

    #[test]
    fn choosing_an_option_runs_its_act_and_a_disabled_one_runs_nothing() {
        let mut s = state(vec![
            Control::toggle("Show strip", "", true, Act::Cmd(CMD_STRIP), true),
            Control::button("Update", "", "Check", Act::Cmd(CMD_UPDATE), false),
            Control::choice("Symbols", "", vec![settings::opt("BTC", Act::Do(Action::Symbol("BTC=".into())))], None),
        ]);
        assert!(matches!(choose(&mut s, Hit::Opt(0, 0)), Some(Do::Run(Act::Cmd(CMD_STRIP)))));
        assert!(choose(&mut s, Hit::Opt(1, 0)).is_none());
        assert!(matches!(choose(&mut s, Hit::Opt(2, 0)), Some(Do::Run(Act::Do(Action::Symbol(ref sym)))) if sym == "BTC="));
        assert!(choose(&mut s, Hit::Opt(9, 0)).is_none(), "a control that is gone runs nothing");
    }

    #[test]
    fn choosing_a_card_chip_picks_the_symbol_and_runs_no_command() {
        let mut s = state(vec![Control::choice("Symbol", "", vec![settings::opt("Gold", Act::Card("XAU=".into()))], Some(0))]);
        assert!(matches!(choose(&mut s, Hit::Opt(0, 0)), Some(Do::Card(ref sym)) if sym == "XAU="));
    }

    fn tall_layout(scroll: i32) -> settings::Layout {
        let page: Vec<Control> = (0..40).map(|i| Control::button(&format!("Row {i}"), "", "Go", Act::Cmd(i), true)).collect();
        settings::layout(96, 780, 400, 6, &page, scroll)
    }

    #[test]
    fn the_owner_is_the_panel_while_it_is_open_and_the_window_is_raised_when_it_gains_it() {
        assert_eq!(owner_plan(true, false), OwnerPlan { owner: Owner::Panel, restack: Restack::Raise });
        assert_eq!(owner_plan(true, true), OwnerPlan { owner: Owner::Panel, restack: Restack::Keep }, "already owned: left where it is");
    }

    #[test]
    fn a_docked_strip_with_the_panel_closed_owns_nothing_and_leaves_the_topmost_band() {
        // A docked strip is topmost, and an owned window follows its owner there.
        let plan = owner_plan(false, true);
        assert_eq!(plan.owner, Owner::Nobody);
        assert_eq!(plan.restack, Restack::NotTopmost, "it must not stay above other apps");
    }

    #[test]
    fn a_floating_strip_with_the_panel_closed_owns_nothing_either() {
        assert_eq!(owner_plan(false, true), OwnerPlan { owner: Owner::Nobody, restack: Restack::NotTopmost });
        // Already unowned: nothing to undo, and no restack on every refresh.
        assert_eq!(owner_plan(false, false), OwnerPlan { owner: Owner::Nobody, restack: Restack::Keep });
    }

    #[test]
    fn opening_again_shows_the_open_window_and_restores_it_when_minimised() {
        assert_eq!(reopen(false, false), Reopen::Create);
        assert_eq!(reopen(false, true), Reopen::Create, "a destroyed window is not shown");
        assert_eq!(reopen(true, false), Reopen::Front);
        assert_eq!(reopen(true, true), Reopen::RestoreAndFront);
    }

    #[test]
    fn a_command_does_one_fresh_read_not_two() {
        // Nothing read since the count was taken: the command reads once itself.
        assert!(needs_fresh_read(7, 7));
        // SetFocus activated the window and `WM_ACTIVATE` read: no second read.
        assert!(!needs_fresh_read(7, 8));
        assert!(!needs_fresh_read(u32::MAX, 0), "the count wraps");
    }

    #[test]
    fn opening_reopening_and_commands_read_once_around_the_raise() {
        use std::cell::Cell;
        let count = Cell::new(10u32);
        let reads = Cell::new(0);
        // The raise activates the window and the activation reads: no second read.
        read_fresh_after_counted(|| count.get(), || count.set(count.get() + 1), || reads.set(reads.get() + 1));
        assert_eq!(reads.get(), 0);
        // The window was already active, nothing read: one read.
        read_fresh_after_counted(|| count.get(), || {}, || reads.set(reads.get() + 1));
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_new_section_clears_the_wheel_remainder_the_thumb_and_the_scroll() {
        let mut s = state(Vec::new());
        s.scroll = 300;
        s.wheel_rem = 4_608;
        s.drag = Some(5);
        s.bar_hot = true;
        reset_view(&mut s);
        assert_eq!((s.scroll, s.wheel_rem, s.drag, s.bar_hot), (0, 0, None, false));
    }

    #[test]
    fn a_press_on_the_thumb_takes_it_and_a_press_in_the_track_pages() {
        let mut s = state(Vec::new());
        let l = tall_layout(0);
        let b = settings::scrollbar(&l).unwrap();
        let x = b.track.left + 3;
        assert_eq!(press_bar(&mut s, &l, x, b.thumb.top + 6), Some(true));
        assert_eq!(s.drag, Some(6));
        s.drag = None;
        let y = b.thumb.bottom + 10;
        assert_eq!(press_bar(&mut s, &l, x, y), Some(false));
        assert_eq!(s.scroll, settings::page_px(&l), "one page down");
        assert_eq!(press_bar(&mut s, &l, b.track.left - 40, 100), None, "off the bar");
    }

    #[test]
    fn dragging_the_thumb_scrolls_to_the_end_and_back() {
        let mut s = state(Vec::new());
        let l = tall_layout(0);
        let b = settings::scrollbar(&l).unwrap();
        let x = b.track.left + 3;
        press_bar(&mut s, &l, x, b.thumb.top + 4);
        assert!(move_over_bar(&mut s, &l, x, 100_000));
        assert_eq!(s.scroll, l.max_scroll());
        assert!(move_over_bar(&mut s, &l, x, -100_000));
        assert_eq!(s.scroll, 0);
        assert!(!move_over_bar(&mut s, &l, x, -100_000), "no change, nothing to repaint");
    }

    #[test]
    fn the_bar_lights_when_the_pointer_is_over_it_and_not_while_it_is_away() {
        let mut s = state(Vec::new());
        let l = tall_layout(0);
        let b = settings::scrollbar(&l).unwrap();
        assert!(move_over_bar(&mut s, &l, b.track.left + 2, 50));
        assert_eq!(bar_state(&s), BarState::Hot);
        assert!(move_over_bar(&mut s, &l, 300, 50));
        assert_eq!(bar_state(&s), BarState::Idle);
        s.drag = Some(0);
        assert_eq!(bar_state(&s), BarState::Dragging);
    }

    #[test]
    fn page_and_end_keys_scroll_and_home_and_end_in_the_sidebar_do_not() {
        let l = tall_layout(100);
        assert_eq!(key_scroll(0x22, &l), Some(settings::scroll_for(&l, PageKey::PageDown)));
        assert_eq!(key_scroll(0x21, &l), Some(settings::scroll_for(&l, PageKey::PageUp)));
        assert_eq!(key_scroll(0x24, &l), Some(0));
        assert_eq!(key_scroll(0x23, &l), Some(l.max_scroll()));
        assert_eq!(key_scroll(0x41, &l), None);
    }

    #[test]
    fn the_class_name_is_per_build_so_classic_and_ultra_can_both_open_it() {
        // The two builds run side by side; a class name shared between them would still work because
        // the classes are per process, but the names follow the strip's own scheme.
        let classic = format!("{}_Settings", Profile::classic().id);
        let ultra = format!("{}_Settings", Profile::ultra().id);
        assert_ne!(classic, ultra);
    }
}
