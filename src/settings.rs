//! The settings window: its model, layout, hit testing, keyboard focus order and painter.
//!
//! Everything except `draw` is pure data and integer arithmetic, so the layout at any DPI or panel
//! zoom, the hit tests and the focus order are unit tests that need no window. The window itself
//! (`ui/settings_win.rs`) owns the Win32 side, and `ui/settings_view.rs` turns a `Config` into the
//! controls drawn here. Each control carries the command the old menu entry ran (`Act`), so a click
//! and the menu entry it replaces reach the same code.
//!
//! The layout is in pixels at the effective DPI (`zoom::effective_dpi`): a zoom of 150 percent at 96
//! DPI is the 144 DPI picture. Text widths are estimated from character counts (`CHAR_W`), because the
//! layout must not depend on a device context; the painter clips each label to its rectangle.

use crate::menu_commands::Action;
use crate::panel::{self, Fonts};
use crate::theme::Theme;
use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::*;

/// A rectangle in client pixels. `right` and `bottom` are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rc {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rc {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rc {
        Rc { left, top, right, bottom }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    fn shifted(&self, dy: i32) -> Rc {
        Rc::new(self.left, self.top + dy, self.right, self.bottom + dy)
    }
    #[cfg(test)]
    fn overlaps(&self, o: &Rc) -> bool {
        self.left < o.right && o.left < self.right && self.top < o.bottom && o.top < self.bottom
    }
    fn to_rect(self) -> RECT {
        RECT { left: self.left, top: self.top, right: self.right, bottom: self.bottom }
    }
}

/// The sections of the sidebar, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Symbols,
    Display,
    Chart,
    Alerts,
    System,
    About,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Symbols => "Symbols",
            Section::Display => "Display",
            Section::Chart => "Chart",
            Section::Alerts => "Alerts",
            Section::System => "Startup & system",
            Section::About => "About",
        }
    }

    /// The sections a build shows. Chart and Alerts exist only in Ultra.
    pub fn for_build(ultra: bool) -> Vec<Section> {
        if ultra {
            vec![Section::Symbols, Section::Display, Section::Chart, Section::Alerts, Section::System, Section::About]
        } else {
            vec![Section::Symbols, Section::Display, Section::System, Section::About]
        }
    }
}

/// What activating an option does: a fixed menu command id, or a command that carries its own
/// identity (a symbol, a label, a display), exactly the two kinds the menu had.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Cmd(usize),
    Do(Action),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub label: String,
    pub act: Act,
    pub enabled: bool,
    /// Drawn as the chosen one. Several options of a control may be on (the symbol chips).
    pub on: bool,
}

impl Opt {
    pub fn checked(mut self, on: bool) -> Opt {
        self.on = on;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Opt {
        self.enabled = enabled;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A group title. Not focusable.
    Header,
    /// A line of dim text. Not focusable.
    Note,
    /// One switch (`opts[0]`), on or off (`on`).
    Toggle,
    /// Segmented choices. `sel` marks the current one; `None` means the options are plain actions.
    Choice,
    /// One push button (`opts[0]`).
    Button,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub kind: Kind,
    pub label: String,
    pub hint: String,
    pub opts: Vec<Opt>,
    pub sel: Option<usize>,
    pub on: bool,
}

impl Control {
    pub fn header(text: &str) -> Control {
        Control { kind: Kind::Header, label: text.into(), hint: String::new(), opts: Vec::new(), sel: None, on: false }
    }
    pub fn note(text: &str) -> Control {
        Control { kind: Kind::Note, label: text.into(), hint: String::new(), opts: Vec::new(), sel: None, on: false }
    }
    pub fn toggle(label: &str, hint: &str, on: bool, act: Act, enabled: bool) -> Control {
        Control {
            kind: Kind::Toggle,
            label: label.into(),
            hint: hint.into(),
            opts: vec![Opt { label: label.into(), act, enabled, on }],
            sel: None,
            on,
        }
    }
    pub fn choice(label: &str, hint: &str, mut opts: Vec<Opt>, sel: Option<usize>) -> Control {
        if let Some(o) = sel.and_then(|i| opts.get_mut(i)) {
            o.on = true;
        }
        Control { kind: Kind::Choice, label: label.into(), hint: hint.into(), opts, sel, on: false }
    }
    pub fn button(label: &str, hint: &str, text: &str, act: Act, enabled: bool) -> Control {
        Control {
            kind: Kind::Button,
            label: label.into(),
            hint: hint.into(),
            opts: vec![Opt { label: text.into(), act, enabled, on: false }],
            sel: None,
            on: false,
        }
    }
    /// Whether the keyboard can land on this control: it has at least one enabled option.
    pub fn focusable(&self) -> bool {
        matches!(self.kind, Kind::Toggle | Kind::Choice | Kind::Button) && self.opts.iter().any(|o| o.enabled)
    }
}

/// An option for `Control::choice`.
pub fn opt(label: impl Into<String>, act: Act) -> Opt {
    Opt { label: label.into(), act, enabled: true, on: false }
}

// ---------------------------------------------------------------------------------------------
// Layout

/// Estimated width of one character of chip text at 96 DPI.
const CHAR_W: i32 = 8;

fn px(v: i32, dpi: u32) -> i32 {
    ((v as i64 * dpi as i64 + 48) / 96) as i32
}

/// The window's client size at 96 DPI scaled to `dpi`, which is what a new window opens at.
pub fn window_size(dpi: u32) -> (i32, i32) {
    (px(780, dpi), px(560, dpi))
}

/// The smallest client size the layout is meant for.
pub fn min_size(dpi: u32) -> (i32, i32) {
    (px(560, dpi), px(360, dpi))
}

/// One laid-out control. `opts` has one rectangle per option (empty for a header or a note).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub bounds: Rc,
    pub label: Rc,
    pub opts: Vec<Rc>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub dpi: u32,
    pub side: Rc,
    pub nav: Vec<Rc>,
    pub viewport: Rc,
    pub rows: Vec<Row>,
    pub content_h: i32,
    /// The scroll offset in force: the one asked for, held inside `0..=max_scroll`.
    pub scroll: i32,
}

impl Layout {
    pub fn max_scroll(&self) -> i32 {
        (self.content_h - self.viewport.height()).max(0)
    }
}

/// Lay the window out: a sidebar of `nav_n` entries and a scrolling column of `controls`.
pub fn layout(dpi: u32, w: i32, h: i32, nav_n: usize, controls: &[Control], scroll: i32) -> Layout {
    let s = |v: i32| px(v, dpi);
    let side_w = s(176).min(w / 3).max(1);
    let side = Rc::new(0, 0, side_w, h);
    let nav_h = s(36);
    let nav = (0..nav_n)
        .map(|i| {
            let top = s(64) + i as i32 * (nav_h + s(2));
            Rc::new(s(8).min(side_w / 4), top, side_w - s(8).min(side_w / 4), top + nav_h)
        })
        .collect();
    let viewport = Rc::new(side_w, 0, w.max(side_w + 1), h.max(1));
    let pad = s(20);
    let inner_l = side_w + pad;
    let inner_r = (w - pad - s(10)).max(inner_l + s(120));
    let avail = inner_r - inner_l;
    let lw = (avail * 36 / 100).clamp(s(120), s(220)).min(avail / 2);
    let ctl_l = inner_l + lw + s(12);
    let ctl_r = inner_r;
    let ctl_w = (ctl_r - ctl_l).max(s(60));
    let (chip_h, gap) = (s(30), s(6));
    let mut y = s(12);
    let mut rows = Vec::with_capacity(controls.len());
    for c in controls {
        let row = match c.kind {
            Kind::Header => {
                let hgt = s(42);
                Row {
                    bounds: Rc::new(inner_l, y, inner_r, y + hgt),
                    label: Rc::new(inner_l, y + s(14), inner_r, y + hgt),
                    opts: Vec::new(),
                }
            }
            Kind::Note => {
                let hgt = s(26);
                Row { bounds: Rc::new(inner_l, y, inner_r, y + hgt), label: Rc::new(inner_l, y, inner_r, y + hgt), opts: Vec::new() }
            }
            Kind::Toggle => {
                let hgt = s(48);
                let top = y + (hgt - s(24)) / 2;
                Row {
                    bounds: Rc::new(inner_l, y, inner_r, y + hgt),
                    label: Rc::new(inner_l, y, inner_l + lw, y + hgt),
                    opts: vec![Rc::new(ctl_l, top, (ctl_l + s(92)).min(ctl_r.max(ctl_l + s(92))), top + s(24))],
                }
            }
            Kind::Button | Kind::Choice => {
                let mut x = ctl_l;
                let mut line = 0;
                let mut rects = Vec::with_capacity(c.opts.len());
                for o in &c.opts {
                    let want = (o.label.chars().count() as i32 * s(CHAR_W)).max(s(36)) + s(24);
                    let cw = want.max(s(60)).min(ctl_w);
                    if x > ctl_l && x + cw > ctl_r {
                        x = ctl_l;
                        line += 1;
                    }
                    let top = y + s(9) + line * (chip_h + gap);
                    rects.push(Rc::new(x, top, x + cw, top + chip_h));
                    x += cw + gap;
                }
                let lines = line + 1;
                let hgt = (lines * chip_h + (lines - 1) * gap + s(18)).max(s(48));
                Row {
                    bounds: Rc::new(inner_l, y, inner_r, y + hgt),
                    label: Rc::new(inner_l, y, inner_l + lw, y + hgt),
                    opts: rects,
                }
            }
        };
        y = row.bounds.bottom;
        rows.push(row);
    }
    let content_h = y + s(16);
    let mut l = Layout { dpi, side, nav, viewport, rows, content_h, scroll: 0 };
    let scroll = scroll.clamp(0, l.max_scroll());
    for r in &mut l.rows {
        r.bounds = r.bounds.shifted(-scroll);
        r.label = r.label.shifted(-scroll);
        for o in &mut r.opts {
            *o = o.shifted(-scroll);
        }
    }
    l.scroll = scroll;
    l
}

/// The scroll offset that brings control `ci` fully into view, given the layout it was measured in.
pub fn reveal(l: &Layout, ci: usize) -> i32 {
    let Some(r) = l.rows.get(ci) else { return l.scroll };
    let mut scroll = l.scroll;
    if r.bounds.top < l.viewport.top {
        scroll -= l.viewport.top - r.bounds.top;
    } else if r.bounds.bottom > l.viewport.bottom {
        scroll += r.bounds.bottom - l.viewport.bottom;
    }
    scroll.clamp(0, l.max_scroll())
}

// ---------------------------------------------------------------------------------------------
// Hit testing and focus

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Nav(usize),
    Opt(usize, usize),
}

/// What is under a client point: a sidebar entry, or an enabled option of a control inside the
/// viewport. A disabled option, a header and a gap answer `None`.
pub fn hit(l: &Layout, controls: &[Control], x: i32, y: i32) -> Option<Hit> {
    if let Some(i) = l.nav.iter().position(|r| r.contains(x, y)) {
        return Some(Hit::Nav(i));
    }
    if !l.viewport.contains(x, y) {
        return None;
    }
    for (ci, (row, c)) in l.rows.iter().zip(controls).enumerate() {
        for (oi, r) in row.opts.iter().enumerate() {
            if r.contains(x, y) && c.opts.get(oi).is_some_and(|o| o.enabled) {
                return Some(Hit::Opt(ci, oi));
            }
        }
    }
    None
}

pub type Focus = Hit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// The option a control takes the focus on: the current one, else the first enabled one.
pub fn entry_opt(c: &Control) -> Option<usize> {
    c.sel.filter(|i| c.opts.get(*i).is_some_and(|o| o.enabled)).or_else(|| c.opts.iter().position(|o| o.enabled))
}

fn focus_stops(controls: &[Control]) -> Vec<usize> {
    controls.iter().enumerate().filter(|(_, c)| c.focusable()).map(|(i, _)| i).collect()
}

/// Where the focus goes for `key`. `nav_cur` is the section shown (the sidebar entry the focus
/// returns to). Tab and Shift+Tab walk the sidebar entry then every control, then wrap to the sidebar
/// entry. Up and Down walk the controls, or the sidebar entries when the focus is in the sidebar
/// (which changes the section). Left and Right walk the options of a choice; Left on the first option
/// goes back to the sidebar and Right in the sidebar enters the first control.
pub fn step(f: Focus, key: Key, nav_cur: usize, nav_n: usize, controls: &[Control]) -> Focus {
    let stops = focus_stops(controls);
    let enter = |ci: usize| entry_opt(&controls[ci]).map(|oi| Hit::Opt(ci, oi));
    let first = || stops.first().and_then(|&ci| enter(ci));
    let last = || stops.last().and_then(|&ci| enter(ci));
    let side = Hit::Nav(nav_cur.min(nav_n.saturating_sub(1)));
    match f {
        Hit::Nav(i) => match key {
            Key::Tab | Key::Right => first().unwrap_or(f),
            Key::BackTab => last().unwrap_or(f),
            Key::Down => Hit::Nav((i + 1).min(nav_n.saturating_sub(1))),
            Key::Up => Hit::Nav(i.saturating_sub(1)),
            Key::Home => Hit::Nav(0),
            Key::End => Hit::Nav(nav_n.saturating_sub(1)),
            Key::Left => f,
        },
        Hit::Opt(ci, oi) => {
            let pos = stops.iter().position(|&s| s == ci);
            let next = || pos.and_then(|p| stops.get(p + 1)).and_then(|&c| enter(c));
            let prev = || pos.and_then(|p| p.checked_sub(1)).and_then(|p| stops.get(p)).and_then(|&c| enter(c));
            match key {
                Key::Tab => next().unwrap_or(side),
                Key::BackTab => prev().unwrap_or(side),
                Key::Down => next().unwrap_or(f),
                Key::Up => prev().unwrap_or(f),
                Key::Home => first().unwrap_or(f),
                Key::End => last().unwrap_or(f),
                Key::Left => {
                    let c = &controls[ci];
                    match (0..oi).rev().find(|&j| c.opts[j].enabled) {
                        Some(j) if c.kind == Kind::Choice => Hit::Opt(ci, j),
                        _ => side,
                    }
                }
                Key::Right => {
                    let c = &controls[ci];
                    match (oi + 1..c.opts.len()).find(|&j| c.opts[j].enabled) {
                        Some(j) if c.kind == Kind::Choice => Hit::Opt(ci, j),
                        _ => f,
                    }
                }
            }
        }
    }
}

/// Keep a focus valid after the controls were rebuilt (a symbol was hidden, an alert cleared).
pub fn settle(f: Focus, nav_n: usize, controls: &[Control]) -> Focus {
    match f {
        Hit::Nav(i) => Hit::Nav(i.min(nav_n.saturating_sub(1))),
        Hit::Opt(ci, oi) => match controls.get(ci) {
            Some(c) if c.opts.get(oi).is_some_and(|o| o.enabled) => f,
            Some(c) => entry_opt(c).map_or(Hit::Nav(0), |j| Hit::Opt(ci, j)),
            None => stops_last(controls).unwrap_or(Hit::Nav(0)),
        },
    }
}

fn stops_last(controls: &[Control]) -> Option<Focus> {
    focus_stops(controls).last().and_then(|&ci| entry_opt(&controls[ci]).map(|oi| Hit::Opt(ci, oi)))
}

// ---------------------------------------------------------------------------------------------
// Colours

/// The colours the window draws with, all derived from the theme in force (a Windows contrast theme
/// included, `Theme::contrast`). Every text colour is checked against the fill it is drawn on, and
/// replaced by black or white when the theme's own colour reads below 4.5 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub page: COLORREF,
    pub side: COLORREF,
    pub text: COLORREF,
    pub dim: COLORREF,
    pub chip: COLORREF,
    pub chip_text: COLORREF,
    pub hover: COLORREF,
    pub hover_text: COLORREF,
    pub on: COLORREF,
    pub on_text: COLORREF,
    pub ring: COLORREF,
    pub line: COLORREF,
}

pub fn contrast_ratio(a: COLORREF, b: COLORREF) -> f64 {
    let (la, lb) = (crate::contrast::luminance(a), crate::contrast::luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

const BLACK: COLORREF = 0;
const WHITE: COLORREF = 0x00ff_ffff;

/// Black or white, whichever reads better on `fill`.
pub fn ink_on(fill: COLORREF) -> COLORREF {
    if contrast_ratio(BLACK, fill) >= contrast_ratio(WHITE, fill) { BLACK } else { WHITE }
}

/// `want` when it reads on `fill` at `min` to 1, else black or white.
fn readable(want: COLORREF, fill: COLORREF, min: f64) -> COLORREF {
    if contrast_ratio(want, fill) >= min { want } else { ink_on(fill) }
}

pub fn palette(t: &Theme) -> Palette {
    let page = t.bg;
    let side = t.card;
    let chip = t.card;
    let on = t.accent;
    let hover = t.btn;
    Palette {
        page,
        side,
        text: readable(t.label, page, 4.5),
        dim: readable(t.dim, page, 4.5),
        chip,
        chip_text: readable(t.label, chip, 4.5),
        hover,
        hover_text: readable(t.price, hover, 4.5),
        on,
        on_text: ink_on(on),
        ring: readable(t.price, page, 3.0),
        line: readable(t.guide, page, 1.0),
    }
}

// ---------------------------------------------------------------------------------------------
// Painting

pub struct Paint<'a> {
    pub theme: &'a Theme,
    pub fonts: &'a Fonts,
    pub title: &'a str,
    pub sections: &'a [Section],
    pub cur: usize,
    pub controls: &'a [Control],
    pub layout: &'a Layout,
    pub focus: Option<Focus>,
    pub hover: Option<Hit>,
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
}

fn put(dc: HDC, font: HFONT, r: Rc, s: &str, color: COLORREF, align: Align) {
    if r.width() <= 0 || r.height() <= 0 {
        return;
    }
    let mut rc = r.to_rect();
    let t = utf16(s);
    let flags = DT_NOPREFIX | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | match align {
        Align::Left => DT_LEFT,
        Align::Center => DT_CENTER,
    };
    unsafe {
        let old = SelectObject(dc, font);
        SetTextColor(dc, color);
        DrawTextW(dc, t.as_ptr(), t.len() as i32, &mut rc, flags);
        SelectObject(dc, old);
    }
}

fn outline(dc: HDC, r: Rc, radius: i32, color: COLORREF, width: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, width.max(1), color);
        let old_p = SelectObject(dc, pen);
        let old_b = SelectObject(dc, GetStockObject(NULL_BRUSH));
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(dc, old_b);
        SelectObject(dc, old_p);
        DeleteObject(pen);
    }
}

fn plate(dc: HDC, r: Rc, radius: i32, fill: COLORREF) {
    panel::fill_round(dc, r.to_rect(), radius, fill);
}

fn inflate(r: Rc, by: i32) -> Rc {
    Rc::new(r.left - by, r.top - by, r.right + by, r.bottom + by)
}

/// Paint the whole window into `dc`, a `w` x `h` client area.
pub fn draw(dc: HDC, w: i32, h: i32, p: &Paint) {
    let s = |v: i32| px(v, p.layout.dpi);
    let pal = palette(p.theme);
    let contrast = p.theme.contrast;
    let f = p.fonts;
    unsafe { SetBkMode(dc, TRANSPARENT as _) };
    panel::fill_rect(dc, RECT { left: 0, top: 0, right: w, bottom: h }, pal.page);
    let side = p.layout.side;
    panel::fill_rect(dc, side.to_rect(), pal.side);
    if contrast {
        panel::fill_rect(dc, RECT { left: side.right - 1, top: 0, right: side.right, bottom: h }, pal.text);
    }
    put(dc, f.big, Rc::new(s(16), s(14), side.right - s(8), s(54)), p.title, readable(pal.text, pal.side, 4.5), Align::Left);
    let on_side = readable(p.theme.label, pal.side, 4.5);
    for (i, (sec, r)) in p.sections.iter().zip(&p.layout.nav).enumerate() {
        let current = i == p.cur;
        let hovered = p.hover == Some(Hit::Nav(i));
        let (fill, ink) = if current {
            (Some(pal.on), pal.on_text)
        } else if hovered {
            (Some(pal.hover), pal.hover_text)
        } else {
            (None, on_side)
        };
        if let Some(fill) = fill {
            plate(dc, *r, s(10), fill);
        }
        put(dc, f.mid, Rc::new(r.left + s(14), r.top, r.right - s(6), r.bottom), sec.label(), ink, Align::Left);
        if p.focus == Some(Hit::Nav(i)) {
            outline(dc, inflate(*r, s(1)), s(10), pal.ring, s(2));
        }
    }
    // The content, clipped to its viewport so a scrolled row never paints over the sidebar.
    let vp = p.layout.viewport;
    unsafe {
        SaveDC(dc);
        IntersectClipRect(dc, vp.left, vp.top, vp.right, vp.bottom);
    }
    for (ci, (c, row)) in p.controls.iter().zip(&p.layout.rows).enumerate() {
        if row.bounds.bottom < vp.top || row.bounds.top > vp.bottom {
            continue;
        }
        match c.kind {
            Kind::Header => {
                put(dc, f.mid, row.label, &c.label, pal.text, Align::Left);
                let y = row.bounds.bottom - s(4);
                panel::fill_rect(dc, RECT { left: row.bounds.left, top: y, right: row.bounds.right, bottom: y + 1 }, pal.line);
            }
            Kind::Note => put(dc, f.small, row.label, &c.label, pal.dim, Align::Left),
            _ => {
                let has_hint = !c.hint.is_empty();
                let lab = row.label;
                let (top, bottom) = if has_hint { (Rc::new(lab.left, lab.top + s(6), lab.right, lab.top + s(26)), Rc::new(lab.left, lab.top + s(26), lab.right, lab.top + s(44))) } else { (lab, Rc::default()) };
                put(dc, f.mid, top, &c.label, pal.text, Align::Left);
                if has_hint {
                    put(dc, f.small, bottom, &c.hint, pal.dim, Align::Left);
                }
                for (oi, (o, r)) in c.opts.iter().zip(&row.opts).enumerate() {
                    let here = Hit::Opt(ci, oi);
                    let hovered = p.hover == Some(here) && o.enabled;
                    if c.kind == Kind::Toggle {
                        draw_switch(dc, f, *r, c.on, hovered, o.enabled, &pal, contrast, s(1));
                    } else {
                        let selected = o.on;
                        let (fill, ink) = if !o.enabled {
                            (pal.chip, pal.dim)
                        } else if selected {
                            (pal.on, pal.on_text)
                        } else if hovered {
                            (pal.hover, pal.hover_text)
                        } else {
                            (pal.chip, pal.chip_text)
                        };
                        plate(dc, *r, s(8), fill);
                        if contrast || !selected {
                            outline(dc, *r, s(8), if o.enabled { pal.line } else { pal.dim }, if contrast { s(2) } else { 1 });
                        }
                        put(dc, f.small, Rc::new(r.left + s(6), r.top, r.right - s(6), r.bottom), &o.label, ink, Align::Center);
                    }
                    if p.focus == Some(here) {
                        outline(dc, inflate(*r, s(2)), s(10), pal.ring, s(2));
                    }
                }
            }
        }
    }
    unsafe { RestoreDC(dc, -1) };
    // A thin thumb when the content is taller than the window.
    let max = p.layout.max_scroll();
    if max > 0 {
        let track = vp.height();
        let thumb = (track * track / p.layout.content_h).max(s(24));
        let top = (track - thumb) * p.layout.scroll / max;
        plate(dc, Rc::new(vp.right - s(8), top + s(2), vp.right - s(3), top + thumb - s(2)), s(4), pal.dim);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_switch(dc: HDC, f: &Fonts, r: Rc, on: bool, hovered: bool, enabled: bool, pal: &Palette, contrast: bool, one: i32) {
    let track = Rc::new(r.left, r.top, r.left + r.height() * 2, r.bottom);
    let d = r.height() - 6 * one;
    let fill = if !enabled { pal.chip } else if on { pal.on } else if hovered { pal.hover } else { pal.chip };
    plate(dc, track, r.height(), fill);
    if !on || contrast {
        outline(dc, track, r.height(), if enabled { pal.text } else { pal.dim }, if contrast { 2 * one } else { 1 });
    }
    let knob_x = if on { track.right - d - 3 * one } else { track.left + 3 * one };
    let knob = if !enabled { pal.dim } else if on { pal.on_text } else { pal.text };
    panel::fill_ellipse(dc, knob_x, track.top + 3 * one, d, knob);
    let word = if on { "On" } else { "Off" };
    put(dc, f.small, Rc::new(track.right + 8 * one, r.top, r.right.max(track.right + 40 * one), r.bottom), word, if enabled { pal.text } else { pal.dim }, Align::Left);
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn sample() -> Vec<Control> {
        let many = |n: usize| (0..n).map(|i| opt(format!("choice {i}"), Act::Cmd(100 + i))).collect::<Vec<_>>();
        vec![
            Control::header("Strip"),
            Control::toggle("Show strip", "", true, Act::Cmd(1), true),
            Control::choice("Rows", "taskbar only", many(3), Some(1)),
            Control::choice("Themes", "", many(26), Some(4)),
            Control::note("Sends to example.com"),
            Control::button("Update", "", "Check for updates", Act::Cmd(2), true),
            Control::choice("Disabled", "", vec![Opt { label: "x".into(), act: Act::Cmd(3), enabled: false, on: false }], None),
            Control::choice("Half", "", vec![
                Opt { label: "a".into(), act: Act::Cmd(4), enabled: false, on: false },
                opt("b", Act::Cmd(5)),
                opt("c", Act::Cmd(6)),
            ], None),
        ]
    }

    /// Every DPI the window can see and every panel zoom: 96 to 288, 75 to 150 percent.
    fn scales() -> Vec<u32> {
        let mut v = Vec::new();
        for dpi in [96, 120, 144, 192, 288] {
            for z in crate::zoom::STEPS {
                v.push(crate::zoom::effective_dpi(dpi, z));
            }
        }
        v
    }

    #[test]
    fn layout_keeps_every_rectangle_inside_its_area_at_every_scale() {
        let controls = sample();
        for eff in scales() {
            let (w, h) = window_size(eff);
            let l = layout(eff, w, h, 6, &controls, 0);
            assert_eq!(l.rows.len(), controls.len());
            for r in &l.nav {
                assert!(r.left >= 0 && r.right <= l.side.right && r.height() > 0, "nav {r:?} at {eff}");
            }
            for (row, c) in l.rows.iter().zip(&controls) {
                assert_eq!(row.opts.len(), c.opts.len());
                assert!(row.bounds.left >= l.viewport.left && row.bounds.right <= l.viewport.right, "row {:?} at {eff}", row.bounds);
                for o in &row.opts {
                    assert!(o.left >= row.bounds.left && o.right <= row.bounds.right, "{o:?} outside {:?} at {eff}", row.bounds);
                    assert!(o.top >= row.bounds.top && o.bottom <= row.bounds.bottom, "{o:?} outside {:?} at {eff}", row.bounds);
                    assert!(o.left >= row.label.right, "options must start right of the label at {eff}");
                }
            }
        }
    }

    #[test]
    fn options_and_rows_never_overlap() {
        let controls = sample();
        for eff in scales() {
            let (w, h) = window_size(eff);
            let l = layout(eff, w, h, 6, &controls, 0);
            for pair in l.rows.windows(2) {
                assert!(pair[0].bounds.bottom <= pair[1].bounds.top, "rows overlap at {eff}");
            }
            for row in &l.rows {
                for (i, a) in row.opts.iter().enumerate() {
                    for b in &row.opts[i + 1..] {
                        assert!(!a.overlaps(b), "{a:?} and {b:?} overlap at {eff}");
                    }
                }
            }
        }
    }

    #[test]
    fn layout_scales_with_the_dpi() {
        let controls = sample();
        let at = |dpi: u32| {
            let (w, h) = window_size(dpi);
            layout(dpi, w, h, 6, &controls, 0)
        };
        let (a, b) = (at(96), at(192));
        assert_eq!(b.side.right, a.side.right * 2);
        assert_eq!(b.nav[0].height(), a.nav[0].height() * 2);
        assert_eq!(b.rows[1].opts[0].height(), a.rows[1].opts[0].height() * 2);
        // Zoom 150 percent at 96 DPI is the 144 DPI picture.
        let z = crate::zoom::effective_dpi(96, 150);
        assert_eq!(z, 144);
        assert_eq!(at(z).content_h, at(144).content_h);
    }

    #[test]
    fn many_choices_wrap_into_lines_and_a_wider_window_wraps_less() {
        let controls = sample();
        let lines = |w: i32| {
            let l = layout(96, w, 560, 6, &controls, 0);
            let mut tops: Vec<i32> = l.rows[3].opts.iter().map(|r| r.top).collect();
            tops.dedup();
            tops.len()
        };
        assert!(lines(780) > 1, "26 chips do not fit one line");
        assert!(lines(1_400) < lines(780));
        assert!(lines(560) >= lines(780));
    }

    #[test]
    fn the_window_survives_a_tiny_size() {
        let controls = sample();
        for (w, h) in [(0, 0), (1, 1), (100, 80), (300, 200)] {
            let l = layout(96, w, h, 6, &controls, 50);
            assert_eq!(l.rows.len(), controls.len());
            assert!(l.scroll >= 0 && l.scroll <= l.max_scroll());
        }
    }

    #[test]
    fn scroll_is_clamped_and_shifts_rows() {
        let controls = sample();
        let (w, h) = (780, 300);
        let top = layout(96, w, h, 6, &controls, 0);
        assert!(top.max_scroll() > 0, "the sample is taller than 300 px");
        let far = layout(96, w, h, 6, &controls, 100_000);
        assert_eq!(far.scroll, far.max_scroll());
        assert_eq!(far.rows[0].bounds.top, top.rows[0].bounds.top - far.scroll);
        assert_eq!(layout(96, w, h, 6, &controls, -40).scroll, 0);
        // The last row is reachable: its bottom is inside the viewport at the end.
        assert!(far.rows.last().unwrap().bounds.bottom <= far.viewport.bottom);
    }

    #[test]
    fn reveal_brings_a_row_into_view() {
        let controls = sample();
        let (w, h) = (780, 300);
        let l = layout(96, w, h, 6, &controls, 0);
        let want = reveal(&l, 5);
        let after = layout(96, w, h, 6, &controls, want);
        assert!(after.rows[5].bounds.top >= after.viewport.top && after.rows[5].bounds.bottom <= after.viewport.bottom);
        let back = reveal(&after, 0);
        let first = layout(96, w, h, 6, &controls, back);
        assert!(first.rows[0].bounds.top >= first.viewport.top, "the first row is in view again");
    }

    #[test]
    fn a_click_in_the_middle_of_each_option_hits_that_option() {
        let controls = sample();
        for eff in scales() {
            let (w, h) = (window_size(eff).0, 4_000);
            let l = layout(eff, w, h, 6, &controls, 0);
            for (ci, row) in l.rows.iter().enumerate() {
                for (oi, r) in row.opts.iter().enumerate() {
                    let (x, y) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                    let want = controls[ci].opts[oi].enabled.then_some(Hit::Opt(ci, oi));
                    assert_eq!(hit(&l, &controls, x, y), want, "control {ci} option {oi} at {eff}");
                }
            }
            for (i, r) in l.nav.iter().enumerate() {
                assert_eq!(hit(&l, &controls, r.left + 2, r.top + 2), Some(Hit::Nav(i)));
            }
        }
    }

    #[test]
    fn hit_testing_ignores_gaps_headers_and_rows_scrolled_out_of_view() {
        let controls = sample();
        let l = layout(96, 780, 300, 6, &controls, 0);
        assert_eq!(hit(&l, &controls, l.rows[0].bounds.left + 4, l.rows[0].bounds.top + 4), None, "header");
        assert_eq!(hit(&l, &controls, 2, 299), None, "sidebar below the entries");
        assert_eq!(hit(&l, &controls, -5, -5), None);
        let last = l.rows.len() - 1;
        let r = l.rows[last].opts[1];
        assert!(r.top >= l.viewport.bottom, "the last row starts below the window");
        assert_eq!(hit(&l, &controls, r.left + 2, r.top + 2), None, "below the viewport is not clickable");
    }

    #[test]
    fn tab_walks_the_sidebar_then_every_focusable_control_then_wraps() {
        let controls = sample();
        let mut f = Hit::Nav(2);
        let mut seen = Vec::new();
        for _ in 0..8 {
            f = step(f, Key::Tab, 2, 6, &controls);
            seen.push(f);
        }
        // Control 0 is a header, 4 a note, 6 has no enabled option: none of them is a stop.
        assert_eq!(seen[0], Hit::Opt(1, 0));
        assert_eq!(seen[1], Hit::Opt(2, 1), "a choice is entered on its current option");
        assert_eq!(seen[2], Hit::Opt(3, 4));
        assert_eq!(seen[3], Hit::Opt(5, 0));
        assert_eq!(seen[4], Hit::Opt(7, 1), "the first enabled option when none is current");
        assert_eq!(seen[5], Hit::Nav(2), "past the last control it wraps to the sidebar entry");
        assert_eq!(seen[6], Hit::Opt(1, 0));
        let back = step(Hit::Opt(1, 0), Key::BackTab, 2, 6, &controls);
        assert_eq!(back, Hit::Nav(2));
        assert_eq!(step(Hit::Nav(2), Key::BackTab, 2, 6, &controls), Hit::Opt(7, 1));
    }

    #[test]
    fn arrows_move_within_a_choice_and_between_controls() {
        let controls = sample();
        assert_eq!(step(Hit::Opt(2, 1), Key::Right, 0, 6, &controls), Hit::Opt(2, 2));
        assert_eq!(step(Hit::Opt(2, 2), Key::Right, 0, 6, &controls), Hit::Opt(2, 2), "stops at the last option");
        assert_eq!(step(Hit::Opt(2, 1), Key::Left, 0, 6, &controls), Hit::Opt(2, 0));
        assert_eq!(step(Hit::Opt(2, 0), Key::Left, 3, 6, &controls), Hit::Nav(3), "Left from the first option returns to the sidebar");
        assert_eq!(step(Hit::Opt(1, 0), Key::Left, 3, 6, &controls), Hit::Nav(3), "a switch has one option");
        assert_eq!(step(Hit::Opt(2, 1), Key::Down, 0, 6, &controls), Hit::Opt(3, 4));
        assert_eq!(step(Hit::Opt(3, 4), Key::Up, 0, 6, &controls), Hit::Opt(2, 1));
        assert_eq!(step(Hit::Opt(1, 0), Key::Up, 0, 6, &controls), Hit::Opt(1, 0), "the top control has nothing above");
        // A disabled option is skipped.
        assert_eq!(step(Hit::Opt(7, 1), Key::Left, 0, 6, &controls), Hit::Nav(0));
        assert_eq!(step(Hit::Opt(7, 1), Key::Right, 0, 6, &controls), Hit::Opt(7, 2));
        // The sidebar.
        assert_eq!(step(Hit::Nav(0), Key::Down, 0, 6, &controls), Hit::Nav(1));
        assert_eq!(step(Hit::Nav(5), Key::Down, 5, 6, &controls), Hit::Nav(5));
        assert_eq!(step(Hit::Nav(0), Key::Up, 0, 6, &controls), Hit::Nav(0));
        assert_eq!(step(Hit::Nav(1), Key::Right, 1, 6, &controls), Hit::Opt(1, 0));
        assert_eq!(step(Hit::Nav(1), Key::Home, 1, 6, &controls), Hit::Nav(0));
        assert_eq!(step(Hit::Opt(3, 4), Key::Home, 1, 6, &controls), Hit::Opt(1, 0));
        assert_eq!(step(Hit::Opt(1, 0), Key::End, 1, 6, &controls), Hit::Opt(7, 1));
    }

    #[test]
    fn focus_with_nothing_to_focus_stays_in_the_sidebar() {
        let controls = vec![Control::header("Only a header"), Control::note("and a note")];
        assert_eq!(step(Hit::Nav(1), Key::Tab, 1, 4, &controls), Hit::Nav(1));
        assert_eq!(step(Hit::Nav(1), Key::Right, 1, 4, &controls), Hit::Nav(1));
        assert_eq!(settle(Hit::Opt(0, 0), 4, &controls), Hit::Nav(0));
        assert_eq!(settle(Hit::Nav(9), 4, &controls), Hit::Nav(3));
    }

    #[test]
    fn settle_repairs_a_focus_the_rebuilt_controls_no_longer_have() {
        let controls = sample();
        assert_eq!(settle(Hit::Opt(2, 1), 6, &controls), Hit::Opt(2, 1));
        assert_eq!(settle(Hit::Opt(7, 0), 6, &controls), Hit::Opt(7, 1), "option 0 is disabled now");
        assert_eq!(settle(Hit::Opt(40, 0), 6, &controls), Hit::Opt(7, 1), "the control is gone");
    }

    #[test]
    fn the_palette_reads_on_every_theme_and_both_contrast_palettes() {
        let mut themes: Vec<Theme> = theme::ALL.to_vec();
        themes.push(theme::classic().clone());
        themes.push(*theme::system(&crate::contrast::HC_BLACK));
        themes.push(*theme::system(&crate::contrast::HC_WHITE));
        for t in &themes {
            let p = palette(t);
            for (name, fg, bg, min) in [
                ("text on page", p.text, p.page, 4.5),
                ("dim on page", p.dim, p.page, 4.5),
                ("text on side", readable(t.label, p.side, 4.5), p.side, 4.5),
                ("chip text", p.chip_text, p.chip, 4.5),
                ("hover text", p.hover_text, p.hover, 4.5),
                ("selected text", p.on_text, p.on, 4.5),
                ("focus ring on page", p.ring, p.page, 3.0),
            ] {
                let r = contrast_ratio(fg, bg);
                assert!(r >= min, "{}: {name} is {r:.2} to 1", t.name);
            }
        }
    }

    #[test]
    fn sections_follow_the_build() {
        assert_eq!(Section::for_build(false), vec![Section::Symbols, Section::Display, Section::System, Section::About]);
        let u = Section::for_build(true);
        assert_eq!(u.len(), 6);
        assert!(u.contains(&Section::Alerts) && u.contains(&Section::Chart));
    }
}
