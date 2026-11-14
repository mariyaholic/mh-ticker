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
use crate::settings::{self, Act, Control, Focus, Hit, Key, Paint, Section};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWLP_HWNDPARENT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, EnableWindow, SetFocus, TME_LEAVE, TRACKMOUSEEVENT};

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
}

thread_local! {
    static SW: RefCell<Option<Sw>> = const { RefCell::new(None) };
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

/// Own the window by the strip while the panel is open, so it stays above the panel, and leave it
/// unowned otherwise: an owned window follows its owner's topmost state, and a docked strip is
/// topmost, which would keep the window above every other app.
fn sync_owner(hwnd: HWND, open: bool) {
    let want = if open { main_hwnd() as isize } else { 0 };
    unsafe {
        if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) != want {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, want);
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

/// Open the window, or bring the open one to the front.
pub(super) fn open_settings(main: HWND) {
    if let Some(h) = with_sw(|s| s.hwnd) {
        unsafe {
            if IsWindow(h) != 0 {
                ShowWindow(h, SW_RESTORE);
                SetForegroundWindow(h);
                return;
            }
        }
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
        };
        apply(&mut sw, &snap);
        set_dark_caption(hwnd, sw.theme);
        SW.with(|c| *c.borrow_mut() = Some(sw));
        SetTimer(hwnd, TIMER_REFRESH, REFRESH_MS, None);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
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
        SetFocus(hwnd);
    }
    refresh_fresh(hwnd);
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

fn go_section(hwnd: HWND, i: usize) {
    let Some(snap) = snapshot(true) else { return };
    with_sw(|s| {
        s.cur = i.min(s.sections.len() - 1);
        s.scroll = 0;
        s.controls = Vec::new();
        s.hover = None;
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
            let page = l.viewport.height() * 9 / 10;
            s.scroll = l.scroll + if vk == 0x22 { page } else { -page };
            invalidate(hwnd);
            return None;
        }
        let key = key_of(vk, shift)?;
        let was = s.focus;
        s.focus = settings::step(s.focus, key, s.cur, s.sections.len(), &s.controls);
        if let Hit::Opt(ci, _) = s.focus {
            let l = current_layout(s, hwnd);
            s.scroll = settings::reveal(&l, ci);
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
                let now = settings::hit(&l, &s.controls, x, y);
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
            });
            invalidate(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
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

    #[test]
    fn the_class_name_is_per_build_so_classic_and_ultra_can_both_open_it() {
        // The two builds run side by side; a class name shared between them would still work because
        // the classes are per process, but the names follow the strip's own scheme.
        let classic = format!("{}_Settings", Profile::classic().id);
        let ultra = format!("{}_Settings", Profile::ultra().id);
        assert_ne!(classic, ultra);
    }
}
