//! Taskbar dock: where the free space on the taskbar is, and whether the strip should show.
//!
//! A horizontal taskbar has two gaps the strip can use:
//! - **left**: from the screen edge (or the Widgets button) to the Start button. Roomy when the
//!   taskbar icons are centred (the Windows 11 default), nothing when they are left-aligned.
//! - **right**: from the end of the app icons to the tray (the notification area and clock).
//!   Usable in either alignment; it shrinks as apps open.
//!
//! Windows 11 draws the taskbar buttons in XAML with no child HWNDs, so their positions come from
//! UI Automation: the Start and Widgets buttons by AutomationId, and the end of the icons as the
//! right edge of the last child of `TaskbarFrame` (Start, Search, Task View and every app button
//! are its children, in order). A probe thread reads them every 2 s, because the icons move as
//! apps open and close, and posts the UI thread when anything moved. UIA runs off the UI thread so
//! its cross-process COM calls cannot re-enter the window procedure. Found elements are cached, so
//! a normal probe is a few cheap reads; the tree is searched again only when a read fails (Explorer
//! restarted) or, for an absent button, every 4-30 s. The probe sleeps while the strip is hidden
//! (`set_paused`), e.g. under a fullscreen game.
//!
//! The tray's left edge is the `TrayNotifyWnd` child window on the main taskbar (no UIA needed).
//! A secondary monitor's taskbar has none; there the probe reads the first tray button instead.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, GW_HWNDPREV,
};

const UNKNOWN: i32 = i32::MIN;
/// Left edge of the Start button, screen px.
static START_LEFT: AtomicI32 = AtomicI32::new(UNKNOWN);
/// Edges of the Widgets button when it is on (far left when centred, by the tray when left-aligned).
static WIDGETS_LEFT: AtomicI32 = AtomicI32::new(UNKNOWN);
static WIDGETS_RIGHT: AtomicI32 = AtomicI32::new(UNKNOWN);
/// Right edge of the last taskbar icon (the end of Start/Search/Task View/apps).
static ICONS_RIGHT: AtomicI32 = AtomicI32::new(UNKNOWN);
/// Left edge of the first tray button; read by the probe only for a taskbar without `TrayNotifyWnd`.
static TRAY_LEFT: AtomicI32 = AtomicI32::new(UNKNOWN);
/// The taskbar the probe reads (0 = the main one); set by the UI from the chosen monitor.
static TARGET: AtomicIsize = AtomicIsize::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Stop probing while nothing is shown (strip hidden, fullscreen app in front).
pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The main taskbar (on the primary monitor).
pub fn taskbar() -> HWND {
    let class = wide("Shell_TrayWnd");
    unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
}

pub fn rect(h: HWND) -> Option<RECT> {
    if h.is_null() {
        return None;
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    (unsafe { GetWindowRect(h, &mut r) } != 0).then_some(r)
}

/// One display, as the Position menu lists it.
#[derive(Clone)]
pub struct Monitor {
    pub handle: HMONITOR,
    /// `\\.\DISPLAY2`: stable enough to save in the config.
    pub device: String,
    pub bounds: RECT,
    pub work: RECT,
    pub primary: bool,
}

fn monitor_info(h: HMONITOR) -> Option<Monitor> {
    unsafe {
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) == 0 {
            return None;
        }
        let n = mi.szDevice.iter().position(|c| *c == 0).unwrap_or(mi.szDevice.len());
        Some(Monitor {
            handle: h,
            device: String::from_utf16_lossy(&mi.szDevice[..n]),
            bounds: mi.monitorInfo.rcMonitor,
            work: mi.monitorInfo.rcWork,
            primary: mi.monitorInfo.dwFlags & 1 != 0,
        })
    }
}

unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data as *mut Vec<Monitor>);
    if let Some(m) = monitor_info(h) {
        list.push(m);
    }
    1
}

/// Every display, primary first.
pub fn monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect), &mut list as *mut Vec<Monitor> as LPARAM) };
    list.sort_by_key(|m| (!m.primary, m.bounds.left, m.bounds.top));
    list
}

/// The chosen display (by device name), or the primary one when unset or unplugged.
pub fn monitor(device: Option<&str>) -> Option<Monitor> {
    let all = monitors();
    device
        .and_then(|d| all.iter().find(|m| m.device == d).cloned())
        .or_else(|| all.iter().find(|m| m.primary).cloned())
        .or_else(|| unsafe { monitor_info(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)) })
}

/// The taskbar on `mon`: the main one, or the secondary taskbar Windows puts on that display
/// (null when it has none, e.g. "show taskbar on all displays" is off).
pub fn taskbar_on(mon: &Monitor) -> HWND {
    unsafe {
        let main = taskbar();
        if !main.is_null() && MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST) == mon.handle {
            return main;
        }
        let class = wide("Shell_SecondaryTrayWnd");
        let mut h = FindWindowExW(std::ptr::null_mut(), std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
        while !h.is_null() {
            if MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) == mon.handle {
                return h;
            }
            h = FindWindowExW(std::ptr::null_mut(), h, class.as_ptr(), std::ptr::null());
        }
        std::ptr::null_mut()
    }
}

/// Point the probe at another taskbar; its cached elements are dropped on the next beat.
pub fn set_target(tb: HWND) {
    TARGET.store(tb as isize, Ordering::Relaxed);
}

/// The taskbar the strip docks to (the main one unless a display was picked).
pub fn target() -> HWND {
    match TARGET.load(Ordering::Relaxed) {
        0 => taskbar(),
        h => h as HWND,
    }
}

/// The taskbar hides itself until the pointer reaches the screen edge: no gap to sit in.
pub fn autohide() -> bool {
    unsafe {
        let mut abd: APPBARDATA = std::mem::zeroed();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32 & ABS_AUTOHIDE != 0
    }
}

/// Left edge of the tray on taskbar `tb`.
fn tray_left(tb: HWND, r: &RECT) -> i32 {
    let class = wide("TrayNotifyWnd");
    let tray = unsafe { FindWindowExW(tb, std::ptr::null_mut(), class.as_ptr(), std::ptr::null()) };
    if let Some(t) = rect(tray).filter(|t| t.right > t.left) {
        return t.left;
    }
    match TRAY_LEFT.load(Ordering::Relaxed) {
        x if x != UNKNOWN && x > r.left => x,
        // no tray found: leave room for a clock
        _ => r.right - 3 * (r.bottom - r.top),
    }
}

/// A free span of the taskbar as (left, right) screen px; None when unknown or empty.
pub type Gap = Option<(i32, i32)>;

/// What the probe saw, in screen px (UNKNOWN where it saw nothing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub start_left: i32,
    pub widgets: (i32, i32),
    pub icons_right: i32,
    pub tray_left: i32,
}

/// The two gaps on taskbar `tb` as (left, right) px, or None when unknown or empty.
/// Without a probe result yet, the left gap falls back to 24% of the taskbar width (left of where
/// centred icons start on a 1080p screen with a few apps open).
pub fn gaps(tb: &RECT, s: &Seen) -> (Gap, Gap) {
    let known = |x: i32| x != UNKNOWN;
    let (wl, wr) = s.widgets;
    let left = if !known(s.start_left) {
        Some((tb.left, tb.left + (tb.right - tb.left) * 24 / 100))
    } else {
        // Widgets sits left of Start only when the icons are centred
        let from = if known(wr) && wr <= s.start_left && wr > tb.left { wr } else { tb.left };
        (s.start_left > from).then_some((from, s.start_left))
    };
    let right = known(s.icons_right).then(|| {
        // Widgets moves next to the tray when the icons are left-aligned
        let to = if known(wl) && wl >= s.icons_right && wl < s.tray_left { wl } else { s.tray_left };
        (s.icons_right, to)
    });
    (left, right.filter(|(a, b)| b > a))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Auto placement: the left gap while it is roomy (it stays put as apps open), the gap by the tray
/// when that is clearly wider (left-aligned icons, or a crowded centre), else none.
pub fn choose(left: Gap, right: Gap, min_w: i32) -> Option<Side> {
    let w = |g: Gap| g.map_or(0, |(a, b)| b - a);
    let (l, r) = (w(left), w(right));
    if l >= min_w && l * 13 / 10 >= r {
        Some(Side::Left)
    } else if r >= min_w {
        Some(Side::Right)
    } else if l >= min_w {
        Some(Side::Left)
    } else {
        None
    }
}

/// The gaps on taskbar `tb` right now, from the probe's last readings.
pub fn spans(tb: HWND, r: &RECT) -> (Gap, Gap) {
    let seen = Seen {
        start_left: START_LEFT.load(Ordering::Relaxed),
        widgets: (WIDGETS_LEFT.load(Ordering::Relaxed), WIDGETS_RIGHT.load(Ordering::Relaxed)),
        icons_right: ICONS_RIGHT.load(Ordering::Relaxed),
        tray_left: tray_left(tb, r),
    };
    gaps(r, &seen)
}

/// Windows' own shell surfaces: the desktop, the taskbar, and the hosts of Start, Search, the
/// thumbnails and flyouts, the touch keyboard. Several of them are monitor-sized windows (Search's
/// and the touch keyboard's are 0,0-1920,1080 on a 1080p screen), so taking the foreground made
/// the strip read them as a fullscreen app and hide: seen 2026-10-15 on clicking the taskbar
/// search box and app icons.
pub fn shell_window(class: &str, exe: &str) -> bool {
    const CLASSES: [&str; 11] = [
        "Progman",
        "WorkerW",
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "Windows.UI.Core.CoreWindow",
        "XamlExplorerHostIslandWindow",
        "XamlExplorerHostIslandWindow_WASDK",
        "TopLevelWindowForOverflowXamlIsland",
        "ForegroundStaging",
        "MultitaskingViewFrame",
        "NotifyIconOverflowWindow",
    ];
    const HOSTS: [&str; 7] = [
        "explorer.exe",
        "searchhost.exe",
        "searchapp.exe",
        "startmenuexperiencehost.exe",
        "shellexperiencehost.exe",
        "shellhost.exe",
        "textinputhost.exe",
    ];
    let exe = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_ascii_lowercase();
    CLASSES.contains(&class) || HOSTS.contains(&exe.as_str())
}

/// File name of the process that owns `h` ("" when it cannot be read).
fn exe_of(h: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 260];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n) != 0;
        CloseHandle(p);
        if ok { String::from_utf16_lossy(&buf[..n as usize]) } else { String::new() }
    }
}

/// True when the foreground window covers its whole monitor (a game, a fullscreen video).
/// Windows' own shell surfaces (`shell_window`) and hidden or cloaked windows never count.
pub fn fullscreen_foreground(own: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() || fg == own || IsWindowVisible(fg) == 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let got = DwmGetWindowAttribute(fg, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as _, 4);
        if got == 0 && cloaked != 0 {
            return false;
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(fg, buf.as_mut_ptr(), buf.len() as i32);
        let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(fg, &mut r);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        // the process lookup only runs for a monitor-sized window
        covers && !shell_window(&class, &exe_of(fg))
    }
}

/// True when a taskbar sits above `own` in the z-order (a taskbar click raises it).
pub fn taskbar_above(own: HWND) -> bool {
    let tb = target();
    if tb.is_null() {
        return false;
    }
    unsafe {
        let mut h = GetWindow(own, GW_HWNDPREV);
        for _ in 0..512 {
            if h.is_null() {
                return false;
            }
            if h == tb {
                return true;
            }
            h = GetWindow(h, GW_HWNDPREV);
        }
    }
    false
}

#[cfg(windows)]
use windows::Win32::UI::Accessibility::{IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker};

/// A taskbar element found once by AutomationId and then read by its cached element.
#[cfg(windows)]
struct Button {
    id: &'static str,
    el: Option<IUIAutomationElement>,
    /// Earliest next tree search while the element is absent (None = search now).
    retry_at: Option<Instant>,
    absent_retry: Duration,
}

#[cfg(windows)]
impl Button {
    fn new(id: &'static str, absent_retry: Duration) -> Self {
        Self { id, el: None, retry_at: None, absent_retry }
    }

    fn reset(&mut self) {
        self.el = None;
        self.retry_at = None;
    }

    fn find(&self, uia: &IUIAutomation, tb: HWND) -> Option<IUIAutomationElement> {
        use windows::core::{BSTR, VARIANT};
        use windows::Win32::UI::Accessibility::{TreeScope_Descendants, UIA_AutomationIdPropertyId};
        if tb.is_null() {
            return None;
        }
        unsafe {
            let root = uia.ElementFromHandle(windows::Win32::Foundation::HWND(tb)).ok()?;
            let cond = uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(BSTR::from(self.id))).ok()?;
            root.FindFirst(TreeScope_Descendants, &cond).ok()
        }
    }

    fn read(&self) -> Option<(i32, i32)> {
        let r = unsafe { self.el.as_ref()?.CurrentBoundingRectangle().ok()? };
        (r.right > r.left).then_some((r.left, r.right))
    }

    /// (left, right) in screen px. Re-searches only after a failed read.
    fn edges(&mut self, uia: &IUIAutomation, tb: HWND) -> Option<(i32, i32)> {
        if let Some(e) = self.read() {
            return Some(e);
        }
        self.el = None;
        if self.retry_at.is_some_and(|t| Instant::now() < t) {
            return None;
        }
        self.el = self.find(uia, tb);
        match self.read() {
            Some(e) => {
                self.retry_at = None;
                Some(e)
            }
            None => {
                self.el = None;
                self.retry_at = Some(Instant::now() + self.absent_retry);
                None
            }
        }
    }
}

/// Right edge of the last child of `TaskbarFrame`: where the taskbar icons end.
#[cfg(windows)]
fn icons_right(walker: &IUIAutomationTreeWalker, frame: &IUIAutomationElement) -> Option<i32> {
    unsafe {
        let last = walker.GetLastChildElement(frame).ok()?;
        let r = last.CurrentBoundingRectangle().ok()?;
        (r.right > r.left).then_some(r.right)
    }
}

/// Poll the taskbar every 2 s; call `changed` when anything the gaps depend on moved.
#[cfg(windows)]
pub fn spawn_probe(changed: Box<dyn Fn() + Send>) {
    let _ = thread::Builder::new().name("dock-probe".into()).spawn(move || {
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
        use windows::Win32::UI::Accessibility::CUIAutomation;
        let uia: Option<IUIAutomation> = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        };
        let Some(uia) = uia else { return };
        let Ok(walker) = (unsafe { uia.ControlViewWalker() }) else { return };
        let mut start = Button::new("StartButton", Duration::from_secs(4));
        let mut widgets = Button::new("WidgetsButton", Duration::from_secs(30));
        let mut frame = Button::new("TaskbarFrame", Duration::from_secs(4));
        let mut tray = Button::new("SystemTrayIcon", Duration::from_secs(30));
        let mut last_tb: HWND = std::ptr::null_mut();
        let tray_class = wide("TrayNotifyWnd");
        loop {
            if !PAUSED.load(Ordering::Relaxed) {
                let tb = target();
                if tb != last_tb {
                    last_tb = tb;
                    for b in [&mut start, &mut widgets, &mut frame, &mut tray] {
                        b.reset();
                    }
                }
                let s = start.edges(&uia, tb).map_or(UNKNOWN, |e| e.0);
                let (wl, wr) = widgets.edges(&uia, tb).unwrap_or((UNKNOWN, UNKNOWN));
                frame.edges(&uia, tb);
                let ir = frame.el.as_ref().and_then(|f| icons_right(&walker, f)).unwrap_or(UNKNOWN);
                let has_tray_wnd = unsafe { !FindWindowExW(tb, std::ptr::null_mut(), tray_class.as_ptr(), std::ptr::null()).is_null() };
                let tl = if has_tray_wnd { UNKNOWN } else { tray.edges(&uia, tb).map_or(UNKNOWN, |e| e.0) };
                let mut moved = false;
                for (a, v) in [(&START_LEFT, s), (&WIDGETS_LEFT, wl), (&WIDGETS_RIGHT, wr), (&ICONS_RIGHT, ir), (&TRAY_LEFT, tl)] {
                    moved |= a.swap(v, Ordering::Relaxed) != v;
                }
                if moved {
                    changed();
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

/// The taskbar probe needs Windows UI Automation; there is no portable equivalent.
#[cfg(not(windows))]
pub fn spawn_probe(_changed: Box<dyn Fn() + Send>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_windows_are_not_fullscreen_apps() {
        assert!(shell_window("Windows.UI.Core.CoreWindow", r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe"));
        assert!(shell_window("Shell_TrayWnd", ""));
        assert!(shell_window("SomethingNew", r"C:\Windows\SystemApps\X\StartMenuExperienceHost.exe"));
        assert!(shell_window("XamlExplorerHostIslandWindow", r"C:\Windows\explorer.exe"));
        assert!(!shell_window("RiotWindowClass", r"C:\Riot Games\League of Legends\Game\League of Legends.exe"));
        assert!(!shell_window("UnityWndClass", ""));
        assert!(!shell_window("Chrome_WidgetWin_1", r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
    }

    const TB: RECT = RECT { left: 0, top: 1015, right: 1920, bottom: 1080 };

    fn seen(start_left: i32, widgets: (i32, i32), icons_right: i32, tray_left: i32) -> Seen {
        Seen { start_left, widgets, icons_right, tray_left }
    }

    #[test]
    fn centred_icons_leave_both_gaps() {
        // measured 2026-10-15: Start at 504, last app ends at 1416, tray from 1599
        let (l, r) = gaps(&TB, &seen(504, (UNKNOWN, UNKNOWN), 1416, 1599));
        assert_eq!((l, r), (Some((0, 504)), Some((1416, 1599))));
        assert_eq!(choose(l, r, 140), Some(Side::Left));
        // Widgets on, at the far left
        assert_eq!(gaps(&TB, &seen(504, (0, 170), 1416, 1599)).0, Some((170, 504)));
    }

    #[test]
    fn left_aligned_icons_use_the_tray_side() {
        // Start at the far left, Widgets next to the tray
        let (l, r) = gaps(&TB, &seen(0, (1500, 1560), 700, 1599));
        assert_eq!(l, None);
        assert_eq!(r, Some((700, 1500)));
        assert_eq!(choose(l, r, 140), Some(Side::Right));
    }

    #[test]
    fn choose_prefers_left_until_right_is_clearly_wider() {
        assert_eq!(choose(Some((0, 300)), Some((0, 380)), 140), Some(Side::Left));
        assert_eq!(choose(Some((0, 300)), Some((0, 400)), 140), Some(Side::Right));
        assert_eq!(choose(Some((0, 100)), Some((0, 90)), 140), None);
        assert_eq!(choose(None, Some((0, 150)), 140), Some(Side::Right));
    }

    #[test]
    fn no_probe_yet_falls_back_on_the_left() {
        let (l, r) = gaps(&TB, &seen(UNKNOWN, (UNKNOWN, UNKNOWN), UNKNOWN, 1599));
        assert_eq!(l, Some((0, 460)));
        assert_eq!(r, None);
    }
}
