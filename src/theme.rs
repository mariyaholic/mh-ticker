//! Palettes, gradients and fonts, in one place, so no module has to know a colour literal.
//!
//! The catalogue is four tiers, and the tier is a *rendering* rule, not a label:
//!
//! | Tier | `Kind` | What it draws |
//! |---|---|---|
//! | Basic | `Flat` | one solid fill |
//! | Gradient | `Gradient` | 64 vertical bands between `bg` and `bg_to` |
//! | Animated | `Animated` | a 96-band ring cycling `bg → bg_to → bg_mid → bg`, slid by a phase |
//! | Retro | `Pixel` | 16 chunky bands, scanlines, square corners, a bitmap face |
//!
//! Motion is separate from the tier: moving palettes name a `cycle`, and the same surface moves in
//! the way that surface can. Imported Yozora palettes are still. A moving flat theme breathes
//! through one colour, lifted and settled, still flat.
//! A ramp becomes a ring. Retro keeps its sixteen bands and rolls its scanlines a cell per cycle.
//!
//! The classic build has exactly one theme (`MIDNIGHT`, today's look) and no menu. The Ultra build
//! lists the whole catalogue in a grouped Theme menu. `resolve` is the only lookup either of them
//! uses, so a theme name in a hand-edited config that no longer exists falls back to the classic
//! look instead of panicking.
//!
//! Two rules hold for every theme:
//! - `up` and `down` stay semantic (green rises, red falls, or their light-theme equivalents) and
//!   are set separately from `accent`, so no palette can make rising and falling hard to tell apart.
//! - Nothing here is cached on disk or in a bitmap. `Gradient` owns GDI brushes and is rebuilt when
//!   the theme changes, never per frame, so a gradient costs no allocation while the panel is open —
//!   including an animated one, which slides over pre-built brushes instead of rebuilding them.

use crate::model::Link;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{COLORREF, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, EnumFontFamiliesExW, FillRect, GetDC, ReleaseDC, LOGFONTW,
    TEXTMETRICW, DEFAULT_CHARSET, HBRUSH, HDC,
};

/// The dot beside the title and the tray icon: the same three states, in the theme's colours.
pub fn link_color(t: &Theme, link: Link) -> COLORREF {
    match link {
        Link::Live => t.up,
        Link::Partial => t.amber,
        Link::Down => t.down,
    }
}

pub fn link_name(link: Link) -> &'static str {
    match link {
        Link::Live => "CNBC live",
        Link::Partial => "CNBC, some prices not fresh",
        Link::Down => "offline",
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// How a theme is painted. The menu groups by this, and `panel` reads it for corners, band count
/// and font quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flat,
    Gradient,
    Animated,
    Pixel,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Flat, Kind::Gradient, Kind::Animated, Kind::Pixel];

    /// The submenu title.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Flat => "Basic",
            Kind::Gradient => "Gradients",
            Kind::Animated => "Animated",
            Kind::Pixel => "Retro",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Kind::Flat => "solid colour",
            Kind::Gradient => "two-colour ramp",
            Kind::Animated => "the ramp drifts",
            Kind::Pixel => "chunky, dithered, bitmap font",
        }
    }
}

/// One palette. `bg`, `bg_to` and `bg_mid` are the three stops of the background's cycle; a still
/// theme would set `cycle: None` and get `bg` alone. How those stops are *laid out* is the kind's
/// job: a flat fill uses `bg` at phase 0 and breathes through the rest, a ramp runs `bg` to `bg_to`
/// top to bottom, and a moving ramp wraps all three into a ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    /// One line for the Theme menu, so the picker explains itself.
    pub note: &'static str,
    pub kind: Kind,
    /// One full motion cycle, in seconds. `None` is still. Motion is a field rather than a tier,
    /// so a palette can keep its surface (a flat fill, a two-colour ramp) and still move; `Kind`
    /// stays the rendering rule and the Theme menu still groups by it.
    pub cycle: Option<f64>,
    pub bg: COLORREF,
    pub bg_to: COLORREF,
    pub bg_mid: COLORREF,
    pub card: COLORREF,
    pub label: COLORREF,
    pub price: COLORREF,
    pub dim: COLORREF,
    pub up: COLORREF,
    pub down: COLORREF,
    pub amber: COLORREF,
    pub guide: COLORREF,
    /// Hovered button and picker chip background.
    pub btn: COLORREF,
    /// Selection, crosshair and the busy accents (never a replacement for up/down).
    pub accent: COLORREF,
    /// The panel face. The strip keeps `cfg.font` (its size comes from `dock_font_pt`/`font_pt`).
    pub font: &'static str,
}

/// A compact literal for the catalogue below: `t(name, note, kind, [bg, bg_to, bg_mid],
/// [card, label, price, dim, up, down, amber, guide, btn, accent], font)` would be unreadable at 25
/// entries, so each theme is spelled out with named fields instead and grouped by comment.
macro_rules! theme {
    ($name:literal, $note:literal, $kind:expr, $cycle:expr, $font:literal, {
        bg: $bg:expr, bg_to: $bg_to:expr, bg_mid: $bg_mid:expr, card: $card:expr,
        label: $label:expr, price: $price:expr, dim: $dim:expr,
        up: $up:expr, down: $down:expr, amber: $amber:expr,
        guide: $guide:expr, btn: $btn:expr, accent: $accent:expr,
    }) => {
        Theme {
            name: $name,
            note: $note,
            kind: $kind,
            cycle: $cycle,
            bg: $bg,
            bg_to: $bg_to,
            bg_mid: $bg_mid,
            card: $card,
            label: $label,
            price: $price,
            dim: $dim,
            up: $up,
            down: $down,
            amber: $amber,
            guide: $guide,
            btn: $btn,
            accent: $accent,
            font: $font,
        }
    };
}

// Palette values and stable IDs are generated from the shared estate catalog.
include!("theme_catalog.rs");

/// The catalogue entry a config names, or the classic look if the name is unknown.
pub fn resolve(name: &str) -> &'static Theme {
    ALL.iter().find(|t| t.name == name).unwrap_or(&MIDNIGHT)
}

pub fn index_of(name: &str) -> usize {
    ALL.iter().position(|t| t.name == name).unwrap_or(0)
}

/// The catalogue indices of one tier, in `ALL` order. The Theme menu is built from this, so adding
/// a palette to `ALL` is all it takes for it to appear in the right submenu.
pub fn tier(kind: Kind) -> impl Iterator<Item = usize> {
    ALL.iter().enumerate().filter(move |(_, t)| t.kind == kind).map(|(i, _)| i)
}

/// The classic build's look: one palette, no menu.
pub fn classic() -> &'static Theme {
    &MIDNIGHT
}

/// The theme in force. The classic build always gets `MIDNIGHT` whatever the config says, so a
/// hand-edited `theme` cannot restyle it — the one-theme promise is enforced here, not by convention.
pub fn pick(ultra: bool, name: &str) -> &'static Theme {
    if ultra {
        resolve(name)
    } else {
        classic()
    }
}

/// One selectable face. `group` is the submenu it lands in, so the Font menu reads as a catalogue
/// rather than one long list of names.
pub struct Face {
    pub name: &'static str,
    pub group: &'static str,
    pub note: &'static str,
}

pub const FACES: [Face; 22] = [
    Face { name: "Segoe UI", group: "System", note: "Windows' default" },
    Face { name: "Segoe UI Variable", group: "System", note: "Win11's default, airy" },
    Face { name: "Bahnschrift", group: "System", note: "DIN, instrument panel" },
    Face { name: "Tahoma", group: "System", note: "narrow, taskbar-ish" },
    Face { name: "Verdana", group: "System", note: "wide, very legible" },
    Face { name: "Calibri", group: "System", note: "soft, rounded" },
    Face { name: "Candara", group: "System", note: "humanist, organic" },
    Face { name: "Corbel", group: "System", note: "humanist, cool" },
    Face { name: "Franklin Gothic Medium", group: "System", note: "architectural, compact" },
    Face { name: "Consolas", group: "Mono", note: "tabular figures" },
    Face { name: "Cascadia Mono", group: "Mono", note: "modern terminal" },
    Face { name: "Cascadia Code", group: "Mono", note: "programmer's terminal" },
    Face { name: "Courier New", group: "Mono", note: "typewriter" },
    Face { name: "Lucida Console", group: "Mono", note: "soft terminal" },
    Face { name: "Constantia", group: "Serif", note: "editorial" },
    Face { name: "Georgia", group: "Serif", note: "warm, screen-safe" },
    Face { name: "Palatino Linotype", group: "Serif", note: "bookish" },
    Face { name: "Cambria", group: "Serif", note: "sturdy screen serif" },
    Face { name: "Sitka Text", group: "Serif", note: "reading serif" },
    Face { name: "Terminal", group: "Bitmap", note: "raster, never antialiased" },
    Face { name: "Fixedsys", group: "Bitmap", note: "the classic console face" },
    Face { name: "MS Gothic", group: "Bitmap", note: "crisp at small sizes" },
];

/// The groups the Font menu nests under, in order.
pub const FACE_GROUPS: [&str; 4] = ["System", "Mono", "Serif", "Bitmap"];

/// The panel face in force: the config's override when it names a face that exists, else the
/// theme's own. An override that names a font this machine lacks would silently fall back to a
/// system font at paint time, which is harder to notice than a refused config value.
pub fn font_of(t: &Theme, over: Option<&str>) -> &'static str {
    over.and_then(|n| FACES.iter().find(|f| f.name.eq_ignore_ascii_case(n)))
        .map(|f| f.name)
        .unwrap_or(t.font)
}

/// Every family this machine has, enumerated once. Empty means the query itself failed, and
/// callers read that as "do not hide anything": a Font menu emptied by one failed API call is worse
/// than a menu offering a face the system will substitute.
fn installed() -> &'static [String] {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(|| unsafe {
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() {
            return Vec::new();
        }
        let mut lf: LOGFONTW = std::mem::zeroed();
        lf.lfCharSet = DEFAULT_CHARSET;
        let mut out: Vec<String> = Vec::new();
        EnumFontFamiliesExW(dc, &lf, Some(collect_face), &mut out as *mut _ as LPARAM, 0);
        ReleaseDC(std::ptr::null_mut(), dc);
        out.sort_unstable();
        out.dedup();
        out
    })
}

unsafe extern "system" fn collect_face(
    lf: *const LOGFONTW,
    _tm: *const TEXTMETRICW,
    _kind: u32,
    p: LPARAM,
) -> i32 {
    let out = &mut *(p as *mut Vec<String>);
    let name = (*lf).lfFaceName;
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    if end > 0 {
        out.push(String::from_utf16_lossy(&name[..end]));
    }
    1 // keep enumerating
}

/// Whether this machine can actually draw `name`. The catalogue ships faces that not every Windows
/// has — Bahnschrift is 1709+, Segoe UI Variable is 11-only, Cascadia arrives with Windows Terminal
/// — and GDI substitutes a missing family silently, which is the one failure the Font menu cannot
/// show. Asking is the only way to grey it out instead.
pub fn face_available(name: &str) -> bool {
    let list = installed();
    list.is_empty() || list.iter().any(|f| f.eq_ignore_ascii_case(name))
}

/// Flat themes paint one fill; pixel themes snap their ramp and draw scanlines.
impl Theme {
    pub fn is_pixel(&self) -> bool {
        self.kind == Kind::Pixel
    }

    /// Whether the background moves at all. A still palette answers 0 for any phase.
    pub fn animated(&self) -> bool {
        self.cycle.is_some()
    }
}

/// How many bands a gradient is drawn in. Enough that a dark-to-dark ramp shows no steps at 100 %
/// scaling, few enough that a repaint stays a handful of GDI calls.
const BANDS: usize = 64;
/// Pixel themes want visible steps, not a smooth ramp: sixteen is a 1980s dither, not a gradient.
const PIXEL_BANDS: usize = 16;
/// A moving ramp is three segments of this many bands. The last colour equals the first, so the
/// ring wraps without a seam at the top or bottom edge.
const RING_SEGMENT: usize = 32;
/// A breathing flat theme is one colour chosen from a ring this long: fine enough that a two-second
/// step at 10 fps cannot be seen as a jump.
const BREATH_BANDS: usize = 48;

fn mix(a: COLORREF, b: COLORREF, f: f64) -> COLORREF {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xff) as f64;
        let y = ((b >> shift) & 0xff) as f64;
        ((x + (y - x) * f).round() as u32) & 0xff
    };
    ch(0) | ch(8) << 8 | ch(16) << 16
}

/// A colour pushed a quarter of the way towards black, for the CRT scanline.
fn darker(c: COLORREF) -> COLORREF {
    mix(c, 0, 0.28)
}

/// Vertical gradient brushes for one theme, built once when the theme is chosen.
///
/// The brushes are created up front rather than per paint: a panel repaint happens every poll
/// (3 s), on every hover move, and ten times a second while a moving background is open — and 64
/// `CreateSolidBrush`/`DeleteObject` pairs per frame is a lot of GDI churn for something that can
/// never change while the theme is fixed.
///
/// Motion is a field, not a tier, so one type covers all four surfaces: `slide` says whether the
/// phase picks a different brush, `bands` says how many rows the rect is cut into. A breathing
/// flat theme is a 48-brush ring shown as one band; a retro ring is the same idea squeezed into
/// sixteen chunky bands.
pub struct Gradient {
    brushes: Vec<HBRUSH>,
    /// The phase slides the index instead of drawing the brushes from the top.
    slide: bool,
    /// Bands across the rect. One band means the whole rect takes a single brush.
    bands: usize,
    /// Scanline colour for pixel themes, `None` for everything else.
    scan: Option<COLORREF>,
}

/// Sample the three-stop loop `bg → bg_to → bg_mid → bg` into `n` brushes. The ends meet, so the
/// wrap is invisible; the caller indexes them modulo `n`.
fn loop_ring(t: &Theme, seg: usize) -> Vec<HBRUSH> {
    let mut brushes = Vec::with_capacity(seg * 3 + 1);
    for (a, b) in [(t.bg, t.bg_to), (t.bg_to, t.bg_mid), (t.bg_mid, t.bg)] {
        for i in 0..seg {
            let f = i as f64 / seg as f64;
            brushes.push(unsafe { CreateSolidBrush(mix(a, b, f)) });
        }
    }
    // The ring closes on the colour it opened with, so the phase is continuous.
    brushes.push(unsafe { CreateSolidBrush(t.bg) });
    brushes
}

impl Gradient {
    pub fn new(t: &Theme) -> Self {
        let scan = if t.is_pixel() { Some(darker(t.bg)) } else { None };
        let moving = t.animated();
        let (brushes, bands) = match t.kind {
            // A flat theme keeps its flat fill; breath only lifts the single colour it uses.
            Kind::Flat if moving => (loop_ring(t, BREATH_BANDS), 1),
            Kind::Flat => (vec![unsafe { CreateSolidBrush(t.bg) }], 1),
            // Retro keeps its sixteen bands and its dither; the ring moves through them.
            Kind::Pixel if moving => (loop_ring(t, PIXEL_BANDS), PIXEL_BANDS),
            Kind::Pixel => {
                let n = PIXEL_BANDS;
                let b = (0..n)
                    .map(|i| {
                        let f = i as f64 / (n - 1) as f64;
                        unsafe { CreateSolidBrush(mix(t.bg, t.bg_to, f)) }
                    })
                    .collect();
                (b, n)
            }
            // A ramp that moves is a ring: three segments, slid by the phase.
            Kind::Gradient | Kind::Animated if moving => {
                (loop_ring(t, RING_SEGMENT), RING_SEGMENT * 3)
            }
            Kind::Gradient | Kind::Animated => {
                let n = BANDS;
                let b = (0..n)
                    .map(|i| {
                        let f = i as f64 / (n - 1) as f64;
                        unsafe { CreateSolidBrush(mix(t.bg, t.bg_to, f)) }
                    })
                    .collect();
                (b, n)
            }
        };
        Self { brushes, slide: moving, bands, scan }
    }

    /// Paint the background into `r`. `phase` (0..1) only slides a moving background; a still theme
    /// ignores it, so the caller can pass a clock without checking the theme first.
    pub fn paint(&self, dc: HDC, r: RECT, phase: f64) {
        let len = self.brushes.len();
        let offset = if self.slide {
            ((phase.rem_euclid(1.0) * len as f64) as usize) % len
        } else {
            0
        };
        if self.bands == 1 {
            unsafe { FillRect(dc, &r, self.brushes[offset]) };
            self.scanlines(dc, r, phase);
            return;
        }
        unsafe {
            let h = (r.bottom - r.top).max(1);
            let n = self.bands as i32;
            for i in 0..n {
                let top = r.top + h * i / n;
                let bottom = r.top + h * (i + 1) / n;
                let band = RECT { left: r.left, top, right: r.right, bottom: bottom.max(top + 1) };
                FillRect(dc, &band, self.brushes[(i as usize + offset) % len]);
            }
        }
        self.scanlines(dc, r, phase);
    }

    /// A CRT's dark line every third row, rolled one cell per cycle so the field crawls instead of
    /// sitting still. Skipped for every non-pixel theme, and it runs before the cards so it never
    /// crosses text.
    fn scanlines(&self, dc: HDC, r: RECT, phase: f64) {
        let Some(scan) = self.scan else { return };
        // Three pixels is the period of the pattern, so rolling by up to 3 wraps invisibly.
        let roll = if self.slide { (phase.rem_euclid(1.0) * 3.0) as i32 } else { 0 };
        let brush = unsafe { CreateSolidBrush(scan) };
        unsafe {
            let mut y = r.top + 2 + roll;
            while y < r.bottom {
                FillRect(dc, &RECT { left: r.left, top: y, right: r.right, bottom: y + 1 }, brush);
                y += 3;
            }
            DeleteObject(brush);
        }
    }
}

impl Drop for Gradient {
    fn drop(&mut self) {
        unsafe {
            for &b in &self.brushes {
                DeleteObject(b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_theme_names_fall_back_to_the_classic_look() {
        assert_eq!(resolve("midnight").name, "midnight");
        assert_eq!(resolve("aurora").name, "aurora");
        assert_eq!(resolve("pc98").name, "pc98");
        assert_eq!(resolve("no-such-theme").name, MIDNIGHT.name);
        assert_eq!(index_of("nonsense"), 0);
    }

    #[test]
    fn classic_is_todays_palette() {
        let t = classic();
        assert_eq!(t.bg, rgb(18, 20, 24));
        assert_eq!(t.card, rgb(28, 31, 37));
        assert_eq!(t.up, rgb(80, 200, 120));
        // Ultra lets midnight breathe, but the classic build never runs a clock: it holds the first
        // brush, which is exactly the colour it shipped with, drawn as one flat fill.
        assert_eq!(Gradient::new(t).bands, 1, "the classic build draws a flat fill");
        assert_eq!(t.kind, Kind::Flat);
    }

    #[test]
    fn every_theme_keeps_rises_and_falls_apart() {
        for t in ALL {
            assert_ne!(t.up, t.down, "{}: rise and fall must differ", t.name);
            assert_ne!(t.price, t.bg, "{}: price must not vanish into the background", t.name);
            assert_ne!(t.card, t.bg, "{}: cards must stand off the background", t.name);
            assert_ne!(t.accent, t.bg, "{}: the accent must be visible", t.name);
            assert!(!t.font.is_empty(), "{}: names no font", t.name);
        }
    }

    #[test]
    fn names_are_unique_and_every_tier_is_populated() {
        for (i, t) in ALL.iter().enumerate() {
            assert!(!ALL[..i].iter().any(|o| o.name == t.name), "duplicate theme name {}", t.name);
        }
        for kind in Kind::ALL {
            let n = tier(kind).count();
            assert!(n >= 5, "{:?} has only {n} entries", kind);
        }
        // Menu order is tier order: the Theme menu walks ALL, and the tiers must be contiguous.
        let mut seen: Vec<Kind> = ALL.iter().map(|t| t.kind).collect();
        seen.dedup();
        assert_eq!(seen, Kind::ALL, "the catalogue is grouped by tier");
    }

    #[test]
    fn moving_themes_keep_three_distinct_stops() {
        for t in ALL.iter().filter(|t| t.cycle.is_some()) {
            assert!(t.animated(), "{}: every palette in this catalogue moves", t.name);
            assert_ne!(t.bg, t.bg_to, "{}: a cycle with one stop is still", t.name);
            assert_ne!(t.bg_mid, t.bg_to, "{}: two stops, so nothing to cycle through", t.name);
            assert_ne!(t.bg_mid, t.bg, "{}: two stops, so nothing to cycle through", t.name);
            // The surface may lift, but it must never lift into the cards that sit on it.
            for stop in [t.bg, t.bg_to, t.bg_mid] {
                assert_ne!(stop, t.card, "{}: the background would swallow a card", t.name);
                assert_ne!(stop, t.price, "{}: the background would swallow a price", t.name);
            }
            let c = t.cycle.unwrap();
            assert!((5.0..=24.0).contains(&c), "{}: a {c} s cycle is a flicker or a standstill", t.name);
        }
    }

    #[test]
    fn pixel_themes_ask_for_a_fixed_pitch_face() {
        // A retro theme is drawn with no antialiasing and a fixed pitch; a proportional face would
        // break the grid it is imitating. That leaves the bitmap faces and the monospaced ones —
        // c64 uses Consolas on purpose, since it is the closest thing to a PETSCII cell.
        for t in ALL.iter().filter(|t| t.is_pixel()) {
            let group = FACES.iter().find(|f| f.name == t.font).map(|f| f.group);
            assert!(
                matches!(group, Some("Bitmap") | Some("Mono")),
                "{}: {} is not a fixed-pitch face",
                t.name,
                t.font
            );
        }
    }

    #[test]
    fn a_font_override_only_wins_when_the_face_exists() {
        assert_eq!(font_of(&MIDNIGHT, None), "Bahnschrift");
        assert_eq!(font_of(&MIDNIGHT, Some("Georgia")), "Georgia");
        assert_eq!(font_of(&MIDNIGHT, Some("georgia")), "Georgia", "case does not matter");
        assert_eq!(font_of(&MIDNIGHT, Some("Comic Sans")), "Bahnschrift", "unknown faces are refused");
        assert_eq!(font_of(&EGA, Some("Consolas")), "Consolas");
    }

    #[test]
    fn motion_is_a_ring_and_stillness_is_one_brush() {
        // Imported web palettes are deliberately still; this code path must keep working.
        let still = Theme { cycle: None, ..MIDNIGHT };
        let s = Gradient::new(&still);
        assert_eq!(s.brushes.len(), 1);
        assert_eq!(s.bands, 1);
        assert!(!s.slide);

        // A breathing flat theme is a ring worn as one band: one visible colour, many steps.
        let breath = Gradient::new(&MIDNIGHT);
        assert_eq!(breath.bands, 1, "breath keeps the flat fill");
        assert_eq!(breath.brushes.len(), BREATH_BANDS * 3 + 1);
        assert!(breath.slide);

        // A moving ramp is the same ring, spread across 96 bands.
        let ring = Gradient::new(&DRIFT);
        assert_eq!(ring.bands, RING_SEGMENT * 3);
        assert_eq!(ring.brushes.len(), RING_SEGMENT * 3 + 1);
        assert!(ring.slide);

        // Retro keeps sixteen chunky bands and slides the ring through them.
        let pixel = Gradient::new(&EGA);
        assert_eq!(pixel.bands, PIXEL_BANDS);
        assert_eq!(pixel.brushes.len(), PIXEL_BANDS * 3 + 1);
        assert!(pixel.slide);
    }

    #[test]
    fn mixing_is_linear_in_each_channel() {
        assert_eq!(mix(rgb(0, 0, 0), rgb(255, 255, 255), 0.5), rgb(128, 128, 128));
        assert_eq!(mix(rgb(10, 20, 30), rgb(10, 20, 30), 0.7), rgb(10, 20, 30));
    }
}
