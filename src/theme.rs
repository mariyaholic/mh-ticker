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
use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HBRUSH, HDC};

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

/// One palette. `bg_to` equals `bg` for a flat background; anything else is drawn as a vertical
/// gradient from `bg` (top) to `bg_to` (bottom). `bg_mid` is the third stop of an animated ring and
/// is ignored by every other kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// One line for the Theme menu, so the picker explains itself.
    pub note: &'static str,
    pub kind: Kind,
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
    ($name:literal, $note:literal, $kind:expr, $font:literal, {
        bg: $bg:expr, bg_to: $bg_to:expr, bg_mid: $bg_mid:expr, card: $card:expr,
        label: $label:expr, price: $price:expr, dim: $dim:expr,
        up: $up:expr, down: $down:expr, amber: $amber:expr,
        guide: $guide:expr, btn: $btn:expr, accent: $accent:expr,
    }) => {
        Theme {
            name: $name,
            note: $note,
            kind: $kind,
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

// ---------------------------------------------------------------- basic: one solid fill

/// Today's look, byte for byte. The classic build uses only this.
pub const MIDNIGHT: Theme = theme!("midnight", "the original dark", Kind::Flat, "Segoe UI", {
    bg: rgb(18, 20, 24), bg_to: rgb(18, 20, 24), bg_mid: rgb(18, 20, 24), card: rgb(28, 31, 37),
    label: rgb(140, 148, 160), price: rgb(235, 238, 242), dim: rgb(105, 110, 118),
    up: rgb(80, 200, 120), down: rgb(235, 90, 90), amber: rgb(240, 180, 60),
    guide: rgb(52, 56, 64), btn: rgb(52, 57, 66), accent: rgb(150, 120, 255),
});

const INK: Theme = theme!("ink", "near-black navy, cyan", Kind::Flat, "Segoe UI", {
    bg: rgb(12, 14, 22), bg_to: rgb(12, 14, 22), bg_mid: rgb(12, 14, 22), card: rgb(20, 24, 36),
    label: rgb(128, 140, 164), price: rgb(230, 238, 250), dim: rgb(96, 106, 128),
    up: rgb(72, 206, 168), down: rgb(236, 88, 104), amber: rgb(240, 184, 72),
    guide: rgb(40, 48, 68), btn: rgb(42, 50, 70), accent: rgb(88, 196, 236),
});

const SLATE: Theme = theme!("slate", "cool grey, ice blue", Kind::Flat, "Segoe UI", {
    bg: rgb(34, 37, 42), bg_to: rgb(34, 37, 42), bg_mid: rgb(34, 37, 42), card: rgb(46, 50, 57),
    label: rgb(158, 166, 178), price: rgb(242, 245, 249), dim: rgb(122, 130, 142),
    up: rgb(96, 206, 142), down: rgb(236, 108, 108), amber: rgb(236, 190, 86),
    guide: rgb(66, 72, 82), btn: rgb(62, 68, 78), accent: rgb(120, 180, 255),
});

const MOSS: Theme = theme!("moss", "deep green, soft", Kind::Flat, "Segoe UI", {
    bg: rgb(18, 26, 22), bg_to: rgb(18, 26, 22), bg_mid: rgb(18, 26, 22), card: rgb(28, 40, 33),
    label: rgb(144, 166, 150), price: rgb(232, 244, 236), dim: rgb(104, 124, 110),
    up: rgb(110, 220, 140), down: rgb(232, 96, 96), amber: rgb(238, 192, 88),
    guide: rgb(44, 60, 50), btn: rgb(46, 62, 52), accent: rgb(132, 214, 150),
});

const OXBLOOD: Theme = theme!("oxblood", "wine, warm cream", Kind::Flat, "Georgia", {
    bg: rgb(28, 14, 16), bg_to: rgb(28, 14, 16), bg_mid: rgb(28, 14, 16), card: rgb(42, 22, 25),
    label: rgb(176, 148, 148), price: rgb(246, 232, 230), dim: rgb(132, 104, 104),
    up: rgb(110, 208, 132), down: rgb(246, 112, 112), amber: rgb(240, 186, 96),
    guide: rgb(66, 36, 40), btn: rgb(68, 38, 42), accent: rgb(232, 148, 96),
});

const TAUPE: Theme = theme!("taupe", "muted stone, sage", Kind::Flat, "Tahoma", {
    bg: rgb(40, 38, 34), bg_to: rgb(40, 38, 34), bg_mid: rgb(40, 38, 34), card: rgb(54, 51, 46),
    label: rgb(172, 166, 152), price: rgb(244, 241, 233), dim: rgb(130, 124, 112),
    up: rgb(142, 200, 128), down: rgb(226, 116, 104), amber: rgb(230, 186, 100),
    guide: rgb(72, 68, 60), btn: rgb(70, 66, 58), accent: rgb(196, 176, 128),
});

/// Light. Contrast is inverted here, so `price` is dark on light and the rises/falls are deepened
/// rather than brightened to stay legible on paper.
const PAPER: Theme = theme!("paper", "light, low glare", Kind::Flat, "Segoe UI", {
    bg: rgb(247, 245, 240), bg_to: rgb(247, 245, 240), bg_mid: rgb(247, 245, 240), card: rgb(255, 255, 252),
    label: rgb(96, 96, 92), price: rgb(24, 24, 26), dim: rgb(132, 130, 124),
    up: rgb(20, 132, 76), down: rgb(186, 46, 46), amber: rgb(158, 110, 8),
    guide: rgb(210, 206, 198), btn: rgb(226, 222, 214), accent: rgb(52, 86, 196),
});

const SAND: Theme = theme!("sand", "warm paper, clay", Kind::Flat, "Verdana", {
    bg: rgb(240, 234, 222), bg_to: rgb(240, 234, 222), bg_mid: rgb(240, 234, 222), card: rgb(250, 246, 238),
    label: rgb(108, 98, 84), price: rgb(34, 30, 24), dim: rgb(146, 134, 116),
    up: rgb(22, 128, 84), down: rgb(176, 52, 44), amber: rgb(150, 104, 10),
    guide: rgb(220, 210, 192), btn: rgb(232, 222, 206), accent: rgb(170, 96, 40),
});

const MONO: Theme = theme!("mono", "black, white, one red", Kind::Flat, "Consolas", {
    bg: rgb(8, 8, 8), bg_to: rgb(8, 8, 8), bg_mid: rgb(8, 8, 8), card: rgb(20, 20, 20),
    label: rgb(150, 150, 150), price: rgb(245, 245, 245), dim: rgb(110, 110, 110),
    up: rgb(0, 210, 120), down: rgb(255, 70, 70), amber: rgb(255, 200, 0),
    guide: rgb(44, 44, 44), btn: rgb(34, 34, 34), accent: rgb(255, 255, 255),
});

// ---------------------------------------------------------------- gradients: two-stop ramp

const GRAPHITE: Theme = theme!("graphite", "flat neutral grey, blue accent", Kind::Gradient, "Segoe UI", {
    bg: rgb(28, 28, 30), bg_to: rgb(19, 19, 21), bg_mid: rgb(19, 19, 21), card: rgb(38, 38, 41),
    label: rgb(150, 152, 158), price: rgb(240, 240, 243), dim: rgb(112, 114, 120),
    up: rgb(88, 205, 130), down: rgb(238, 100, 100), amber: rgb(238, 184, 72),
    guide: rgb(58, 58, 63), btn: rgb(56, 56, 61), accent: rgb(90, 170, 255),
});

/// Violet to rose, the same ramp as the app icon's price line.
const AURORA: Theme = theme!("aurora", "violet to rose", Kind::Gradient, "Segoe UI", {
    bg: rgb(24, 18, 40), bg_to: rgb(46, 22, 48), bg_mid: rgb(46, 22, 48), card: rgb(37, 30, 56),
    label: rgb(168, 156, 196), price: rgb(246, 240, 250), dim: rgb(128, 118, 152),
    up: rgb(112, 226, 160), down: rgb(255, 112, 142), amber: rgb(246, 190, 110),
    guide: rgb(66, 52, 88), btn: rgb(62, 50, 84), accent: rgb(192, 124, 255),
});

const DUSK: Theme = theme!("dusk", "indigo to plum", Kind::Gradient, "Constantia", {
    bg: rgb(22, 18, 46), bg_to: rgb(58, 26, 58), bg_mid: rgb(58, 26, 58), card: rgb(36, 28, 62),
    label: rgb(164, 152, 196), price: rgb(244, 238, 252), dim: rgb(126, 114, 156),
    up: rgb(108, 220, 164), down: rgb(255, 116, 140), amber: rgb(246, 190, 110),
    guide: rgb(64, 50, 90), btn: rgb(60, 46, 86), accent: rgb(190, 120, 255),
});

const OCEAN: Theme = theme!("ocean", "teal to deep blue", Kind::Gradient, "Segoe UI", {
    bg: rgb(10, 34, 42), bg_to: rgb(8, 18, 40), bg_mid: rgb(8, 18, 40), card: rgb(16, 44, 56),
    label: rgb(138, 172, 184), price: rgb(232, 246, 250), dim: rgb(100, 132, 146),
    up: rgb(96, 222, 168), down: rgb(244, 104, 116), amber: rgb(240, 190, 96),
    guide: rgb(26, 58, 74), btn: rgb(24, 54, 70), accent: rgb(80, 208, 214),
});

const EMBER: Theme = theme!("ember", "charcoal to rust", Kind::Gradient, "Tahoma", {
    bg: rgb(26, 20, 18), bg_to: rgb(48, 22, 16), bg_mid: rgb(48, 22, 16), card: rgb(38, 28, 24),
    label: rgb(176, 152, 138), price: rgb(248, 238, 230), dim: rgb(134, 112, 100),
    up: rgb(128, 216, 144), down: rgb(250, 118, 92), amber: rgb(248, 180, 88),
    guide: rgb(64, 44, 36), btn: rgb(62, 42, 34), accent: rgb(255, 150, 80),
});

const GLASS: Theme = theme!("glass", "light, cool and airy", Kind::Gradient, "Segoe UI", {
    bg: rgb(236, 242, 248), bg_to: rgb(214, 226, 240), bg_mid: rgb(214, 226, 240), card: rgb(252, 254, 255),
    label: rgb(96, 110, 126), price: rgb(20, 28, 40), dim: rgb(134, 148, 164),
    up: rgb(18, 132, 96), down: rgb(190, 52, 72), amber: rgb(160, 112, 20),
    guide: rgb(200, 214, 230), btn: rgb(226, 236, 246), accent: rgb(44, 110, 220),
});

// ---------------------------------------------------------------- animated: bg -> bg_to -> bg_mid

const DRIFT: Theme = theme!("drift", "blue, indigo, violet", Kind::Animated, "Segoe UI", {
    bg: rgb(16, 20, 44), bg_to: rgb(30, 24, 62), bg_mid: rgb(44, 26, 58), card: rgb(26, 30, 58),
    label: rgb(152, 164, 200), price: rgb(236, 242, 255), dim: rgb(112, 124, 160),
    up: rgb(96, 224, 180), down: rgb(244, 110, 140), amber: rgb(244, 192, 104),
    guide: rgb(48, 54, 90), btn: rgb(44, 50, 86), accent: rgb(120, 180, 255),
});

const PULSE: Theme = theme!("pulse", "charcoal, violet breath", Kind::Animated, "Segoe UI", {
    bg: rgb(18, 18, 22), bg_to: rgb(30, 26, 44), bg_mid: rgb(22, 20, 28), card: rgb(34, 34, 42),
    label: rgb(156, 156, 170), price: rgb(242, 242, 248), dim: rgb(116, 116, 130),
    up: rgb(96, 214, 150), down: rgb(244, 104, 120), amber: rgb(240, 186, 88),
    guide: rgb(58, 56, 70), btn: rgb(56, 54, 68), accent: rgb(198, 130, 255),
});

const VAPOR: Theme = theme!("vaporwave", "magenta and cyan", Kind::Animated, "Consolas", {
    bg: rgb(30, 14, 42), bg_to: rgb(14, 38, 58), bg_mid: rgb(52, 16, 48), card: rgb(44, 22, 58),
    label: rgb(196, 160, 214), price: rgb(255, 246, 255), dim: rgb(150, 120, 172),
    up: rgb(110, 240, 214), down: rgb(255, 110, 190), amber: rgb(255, 200, 120),
    guide: rgb(66, 38, 84), btn: rgb(62, 34, 80), accent: rgb(90, 240, 255),
});

const AURORA_LIVE: Theme = theme!("aurora-live", "green and violet", Kind::Animated, "Segoe UI", {
    bg: rgb(10, 26, 26), bg_to: rgb(30, 18, 48), bg_mid: rgb(14, 40, 34), card: rgb(22, 40, 42),
    label: rgb(146, 180, 182), price: rgb(236, 250, 246), dim: rgb(104, 140, 142),
    up: rgb(116, 240, 176), down: rgb(250, 110, 132), amber: rgb(244, 200, 110),
    guide: rgb(34, 62, 64), btn: rgb(32, 58, 60), accent: rgb(150, 255, 200),
});

const SUNSET: Theme = theme!("sunset", "amber through purple", Kind::Animated, "Tahoma", {
    bg: rgb(40, 18, 14), bg_to: rgb(46, 16, 44), bg_mid: rgb(70, 34, 12), card: rgb(52, 26, 24),
    label: rgb(200, 160, 140), price: rgb(255, 240, 226), dim: rgb(158, 118, 100),
    up: rgb(130, 222, 150), down: rgb(255, 120, 100), amber: rgb(255, 196, 92),
    guide: rgb(74, 38, 32), btn: rgb(72, 36, 30), accent: rgb(255, 170, 90),
});

// ---------------------------------------------------------------- retro: chunky, bitmap faces

const EGA: Theme = theme!("ega", "EGA 16-colour, cyan", Kind::Pixel, "Terminal", {
    bg: rgb(0, 0, 42), bg_to: rgb(0, 0, 20), bg_mid: rgb(0, 0, 20), card: rgb(0, 0, 84),
    label: rgb(85, 85, 255), price: rgb(255, 255, 255), dim: rgb(85, 85, 170),
    up: rgb(85, 255, 85), down: rgb(255, 85, 85), amber: rgb(255, 255, 85),
    guide: rgb(0, 0, 120), btn: rgb(0, 0, 140), accent: rgb(85, 255, 255),
});

const AMBER: Theme = theme!("amber-crt", "amber phosphor", Kind::Pixel, "Terminal", {
    bg: rgb(18, 11, 2), bg_to: rgb(10, 6, 1), bg_mid: rgb(10, 6, 1), card: rgb(30, 18, 3),
    label: rgb(190, 120, 30), price: rgb(255, 196, 96), dim: rgb(140, 88, 20),
    up: rgb(255, 214, 120), down: rgb(214, 88, 24), amber: rgb(255, 176, 48),
    guide: rgb(46, 28, 6), btn: rgb(44, 26, 5), accent: rgb(255, 176, 48),
});

const PHOSPHOR: Theme = theme!("phosphor", "green phosphor", Kind::Pixel, "Terminal", {
    bg: rgb(3, 10, 4), bg_to: rgb(2, 6, 3), bg_mid: rgb(2, 6, 3), card: rgb(8, 20, 10),
    label: rgb(90, 160, 100), price: rgb(190, 255, 196), dim: rgb(64, 116, 72),
    up: rgb(140, 255, 150), down: rgb(255, 96, 88), amber: rgb(200, 255, 140),
    guide: rgb(20, 44, 26), btn: rgb(18, 40, 22), accent: rgb(140, 255, 150),
});

const GAMEBOY: Theme = theme!("gameboy", "four shades of green", Kind::Pixel, "Fixedsys", {
    bg: rgb(15, 56, 15), bg_to: rgb(10, 38, 10), bg_mid: rgb(10, 38, 10), card: rgb(48, 98, 48),
    label: rgb(138, 171, 15), price: rgb(214, 232, 140), dim: rgb(96, 130, 60),
    up: rgb(190, 230, 80), down: rgb(230, 110, 80), amber: rgb(220, 190, 60),
    guide: rgb(60, 110, 60), btn: rgb(56, 112, 56), accent: rgb(155, 188, 15),
});

const C64: Theme = theme!("c64", "breadbin blue", Kind::Pixel, "Consolas", {
    bg: rgb(53, 40, 121), bg_to: rgb(32, 24, 82), bg_mid: rgb(32, 24, 82), card: rgb(66, 50, 140),
    label: rgb(120, 160, 255), price: rgb(164, 190, 255), dim: rgb(96, 116, 200),
    up: rgb(140, 230, 160), down: rgb(255, 120, 120), amber: rgb(240, 220, 100),
    guide: rgb(78, 62, 160), btn: rgb(74, 58, 150), accent: rgb(255, 255, 255),
});

/// The estate palette: PC-98's 16 colours with the canon's single gold accent.
const PC98: Theme = theme!("pc98", "PC-98, gold accent", Kind::Pixel, "MS Gothic", {
    bg: rgb(6, 6, 10), bg_to: rgb(0, 0, 4), bg_mid: rgb(0, 0, 4), card: rgb(20, 20, 30),
    label: rgb(170, 170, 170), price: rgb(255, 255, 255), dim: rgb(110, 110, 120),
    up: rgb(85, 255, 85), down: rgb(255, 85, 85), amber: rgb(255, 205, 90),
    guide: rgb(60, 60, 80), btn: rgb(40, 40, 60), accent: rgb(255, 205, 90),
});

/// The whole catalogue, in menu order: basic, gradients, animated, retro.
pub const ALL: [Theme; 26] = [
    MIDNIGHT, INK, SLATE, MOSS, OXBLOOD, TAUPE, PAPER, SAND, MONO, //
    GRAPHITE, AURORA, DUSK, OCEAN, EMBER, GLASS, //
    DRIFT, PULSE, VAPOR, AURORA_LIVE, SUNSET, //
    EGA, AMBER, PHOSPHOR, GAMEBOY, C64, PC98,
];

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

pub const FACES: [Face; 13] = [
    Face { name: "Segoe UI", group: "System", note: "Windows' default" },
    Face { name: "Tahoma", group: "System", note: "narrow, taskbar-ish" },
    Face { name: "Verdana", group: "System", note: "wide, very legible" },
    Face { name: "Calibri", group: "System", note: "soft, rounded" },
    Face { name: "Consolas", group: "Mono", note: "tabular figures" },
    Face { name: "Cascadia Mono", group: "Mono", note: "modern terminal" },
    Face { name: "Courier New", group: "Mono", note: "typewriter" },
    Face { name: "Constantia", group: "Serif", note: "editorial" },
    Face { name: "Georgia", group: "Serif", note: "warm, screen-safe" },
    Face { name: "Palatino Linotype", group: "Serif", note: "bookish" },
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

/// Flat themes paint one fill; pixel themes snap their ramp and draw scanlines.
impl Theme {
    pub fn is_pixel(&self) -> bool {
        self.kind == Kind::Pixel
    }

    /// The drift of an animated background, 0..1. Other kinds always answer 0.
    pub fn animated(&self) -> bool {
        self.kind == Kind::Animated
    }
}

/// How many bands a gradient is drawn in. Enough that a dark-to-dark ramp shows no steps at 100 %
/// scaling, few enough that a repaint stays a handful of GDI calls.
const BANDS: usize = 64;
/// Pixel themes want visible steps, not a smooth ramp: sixteen is a 1980s dither, not a gradient.
const PIXEL_BANDS: usize = 16;
/// An animated ring is three ramps of this many bands. The last colour equals the first, so the
/// ring wraps without a seam at the top or bottom edge.
const RING_SEGMENT: usize = 32;

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
/// (3 s), on every hover move, and twelve times a second while an animated theme is open — and 64
/// `CreateSolidBrush`/`DeleteObject` pairs per frame is a lot of GDI churn for something that can
/// never change while the theme is fixed.
pub struct Gradient {
    brushes: Vec<HBRUSH>,
    /// An animated ring is indexed from a moving offset instead of from the top.
    ring: bool,
    /// Scanline colour for pixel themes, `None` for everything else.
    scan: Option<COLORREF>,
}

impl Gradient {
    pub fn new(t: &Theme) -> Self {
        let scan = if t.is_pixel() { Some(darker(t.bg)) } else { None };
        if t.kind == Kind::Animated {
            let mut brushes = Vec::with_capacity(RING_SEGMENT * 3 + 1);
            for (a, b) in [(t.bg, t.bg_to), (t.bg_to, t.bg_mid), (t.bg_mid, t.bg)] {
                for i in 0..RING_SEGMENT {
                    let f = i as f64 / RING_SEGMENT as f64;
                    brushes.push(unsafe { CreateSolidBrush(mix(a, b, f)) });
                }
            }
            // The ring closes on the colour it opened with, so the wrap is invisible.
            brushes.push(unsafe { CreateSolidBrush(t.bg) });
            return Self { brushes, ring: true, scan };
        }
        if t.bg == t.bg_to {
            // One brush, so the flat case and the gradient case are the same code path below.
            return Self { brushes: vec![unsafe { CreateSolidBrush(t.bg) }], ring: false, scan };
        }
        let n = if t.is_pixel() { PIXEL_BANDS } else { BANDS };
        let brushes = (0..n)
            .map(|i| {
                let f = i as f64 / (n - 1) as f64;
                unsafe { CreateSolidBrush(mix(t.bg, t.bg_to, f)) }
            })
            .collect();
        Self { brushes, ring: false, scan }
    }

    /// Paint the background into `r`. `phase` (0..1) only moves an animated ring; every other kind
    /// ignores it, so the caller can pass a clock without checking the kind.
    pub fn paint(&self, dc: HDC, r: RECT, phase: f64) {
        if let [only] = self.brushes[..] {
            unsafe { FillRect(dc, &r, only) };
            self.scanlines(dc, r);
            return;
        }
        unsafe {
            let h = (r.bottom - r.top).max(1);
            let n = self.brushes.len() as i32;
            let offset = if self.ring {
                // A whole ring back to the start, so the phase is continuous.
                ((phase.rem_euclid(1.0) * n as f64) as i32).rem_euclid(n) as usize
            } else {
                0
            };
            for i in 0..n {
                let top = r.top + h * i / n;
                let bottom = r.top + h * (i + 1) / n;
                let band = RECT { left: r.left, top, right: r.right, bottom: bottom.max(top + 1) };
                FillRect(dc, &band, self.brushes[(i as usize + offset) % self.brushes.len()]);
            }
        }
        self.scanlines(dc, r);
    }

    /// A CRT's dark line every third row. Skipped for every non-pixel theme, and it runs before the
    /// cards so it never crosses text.
    fn scanlines(&self, dc: HDC, r: RECT) {
        let Some(scan) = self.scan else { return };
        let brush = unsafe { CreateSolidBrush(scan) };
        unsafe {
            let mut y = r.top + 2;
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
        assert_eq!(t.bg, t.bg_to, "the classic build draws no gradient");
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
    fn animated_themes_have_three_distinct_stops() {
        for t in ALL.iter().filter(|t| t.animated()) {
            assert_ne!(t.bg, t.bg_to, "{}: animated but flat", t.name);
            assert_ne!(t.bg_mid, t.bg_to, "{}: two stops, so nothing to cycle through", t.name);
            assert_ne!(t.bg_mid, t.bg, "{}: two stops, so nothing to cycle through", t.name);
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
        assert_eq!(font_of(&MIDNIGHT, None), "Segoe UI");
        assert_eq!(font_of(&MIDNIGHT, Some("Georgia")), "Georgia");
        assert_eq!(font_of(&MIDNIGHT, Some("georgia")), "Georgia", "case does not matter");
        assert_eq!(font_of(&MIDNIGHT, Some("Comic Sans")), "Segoe UI", "unknown faces are refused");
        assert_eq!(font_of(&EGA, Some("Consolas")), "Consolas");
    }

    #[test]
    fn the_gradient_ring_closes_and_flat_themes_own_one_brush() {
        assert_eq!(Gradient::new(&MIDNIGHT).brushes.len(), 1);
        assert_eq!(Gradient::new(&AURORA).brushes.len(), BANDS);
        assert_eq!(Gradient::new(&EGA).brushes.len(), PIXEL_BANDS);
        let ring = Gradient::new(&DRIFT);
        assert_eq!(ring.brushes.len(), RING_SEGMENT * 3 + 1, "a closed ring plus the wrap colour");
        assert!(ring.ring, "an animated theme slides instead of drawing top-down");
    }

    #[test]
    fn mixing_is_linear_in_each_channel() {
        assert_eq!(mix(rgb(0, 0, 0), rgb(255, 255, 255), 0.5), rgb(128, 128, 128));
        assert_eq!(mix(rgb(10, 20, 30), rgb(10, 20, 30), 0.7), rgb(10, 20, 30));
    }
}
