//! Contrast arithmetic and the system colour table, `std` only.
//!
//! Colours here are Win32 `COLORREF` values (`0x00BBGGRR`) held as plain `u32`, so this module has
//! no `windows_sys` in it and the Linux harness builds it. `ratio` is the WCAG relative-luminance
//! ratio, `(L1 + 0.05) / (L2 + 0.05)` with `L1` the lighter. The thresholds the 1.9 contrast test
//! holds every Ultra theme to are `TEXT_MIN` (4.5 to 1) and `DIRECTION_MIN` (3 to 1).

/// Text against the background it is drawn on.
#[cfg(test)]
pub const TEXT_MIN: f64 = 4.5;
/// A rise or fall colour against the background it is drawn on.
#[cfg(test)]
pub const DIRECTION_MIN: f64 = 3.0;

fn channel(v: u32) -> f64 {
    let c = (v & 0xff) as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of a `COLORREF`, 0 (black) to 1 (white).
pub fn luminance(c: u32) -> f64 {
    0.2126 * channel(c) + 0.7152 * channel(c >> 8) + 0.0722 * channel(c >> 16)
}

#[cfg(test)]
/// The contrast ratio of two colours, 1 (equal) to 21 (black on white). Order does not matter.
pub fn ratio(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The system colours the contrast theme is built from, as read from `GetSysColor`. The window
/// text and window pair is the page, the button pair is the raised parts, the highlight pair is the
/// selection, `gray_text` is the disabled text and `hotlight` the link colour. A Windows contrast
/// theme guarantees that each text colour reads on the background it is paired with here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemColours {
    pub window: u32,
    pub window_text: u32,
    pub button_face: u32,
    pub button_text: u32,
    pub highlight: u32,
    pub highlight_text: u32,
    pub gray_text: u32,
    pub hotlight: u32,
}

const fn bgr(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// The "High Contrast Black" palette, as Windows 10 and 11 ship it. The still renderer and the
/// tests use this and `HC_WHITE`, so the contrast theme is judged against two fixed palettes and
/// never against whatever the machine running the test happens to have set.
pub const HC_BLACK: SystemColours = SystemColours {
    window: bgr(0, 0, 0),
    window_text: bgr(255, 255, 255),
    button_face: bgr(0, 0, 0),
    button_text: bgr(255, 255, 255),
    highlight: bgr(26, 235, 255),
    highlight_text: bgr(0, 0, 0),
    gray_text: bgr(63, 242, 63),
    hotlight: bgr(255, 255, 0),
};

/// The "High Contrast White" palette, as Windows 10 and 11 ship it.
pub const HC_WHITE: SystemColours = SystemColours {
    window: bgr(255, 255, 255),
    window_text: bgr(0, 0, 0),
    button_face: bgr(255, 255, 255),
    button_text: bgr(0, 0, 0),
    highlight: bgr(55, 0, 110),
    highlight_text: bgr(255, 255, 255),
    gray_text: bgr(96, 0, 0),
    hotlight: bgr(0, 0, 255),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_twenty_one_and_equal_is_one() {
        assert!((ratio(0, 0xFFFFFF) - 21.0).abs() < 1e-9);
        assert!((ratio(0xFFFFFF, 0) - 21.0).abs() < 1e-9);
        assert!((ratio(0x123456, 0x123456) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_known_grey_pair_matches_the_published_figure() {
        // #777 on white is the textbook 4.48 to 1 case.
        let g = bgr(0x77, 0x77, 0x77);
        assert!((ratio(g, 0xFFFFFF) - 4.48).abs() < 0.01);
    }

    #[test]
    fn the_two_system_palettes_pair_every_text_colour_with_a_background_it_reads_on() {
        for p in [HC_BLACK, HC_WHITE] {
            assert!(ratio(p.window_text, p.window) >= 7.0);
            assert!(ratio(p.button_text, p.button_face) >= 7.0);
            assert!(ratio(p.highlight_text, p.highlight) >= 7.0);
            assert!(ratio(p.hotlight, p.window) >= 7.0);
            assert!(ratio(p.gray_text, p.window) >= 4.5);
        }
    }
}
