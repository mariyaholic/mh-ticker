//! Which colours mean up and which mean down, `std` only.
//!
//! The catalogue's themes keep green for up and red for down. Markets and eyes differ: some read red
//! as up, and red against green is the pair most colour-vision differences confuse. `UpDown` is the
//! one setting that changes the two direction colours and nothing else in a theme. The default,
//! `GreenRed`, returns the theme's own pair untouched, so `{}` in a settings file means today's look.
//!
//! * `RedGreen` swaps the theme's own two colours. Each already holds 3 to 1 against that theme's
//!   backgrounds, so the swap does too.
//! * `BlueOrange` replaces both with one fixed pair per theme brightness: a lighter pair for dark
//!   themes and a darker one for light themes. The test in `theme.rs` holds every pairing to 3 to 1
//!   on every background of every theme.

/// Colours are Win32 `COLORREF` values (`0x00BBGGRR`).
const fn bgr(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UpDown {
    #[default]
    GreenRed,
    RedGreen,
    BlueOrange,
}

impl UpDown {
    pub const ALL: [UpDown; 3] = [UpDown::GreenRed, UpDown::RedGreen, UpDown::BlueOrange];

    /// The menu text.
    pub fn label(self) -> &'static str {
        match self {
            UpDown::GreenRed => "Green up, red down",
            UpDown::RedGreen => "Red up, green down",
            UpDown::BlueOrange => "Blue up, orange down",
        }
    }

    /// What the settings file holds. The default is never written (`None`).
    pub fn to_saved(self) -> Option<String> {
        match self {
            UpDown::GreenRed => None,
            UpDown::RedGreen => Some("red_green".into()),
            UpDown::BlueOrange => Some("blue_orange".into()),
        }
    }

    /// The mode a saved value names. Unset and unknown text both read as the default.
    pub fn from_saved(saved: Option<&str>) -> UpDown {
        match saved.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("red_green") => UpDown::RedGreen,
            Some("blue_orange") => UpDown::BlueOrange,
            _ => UpDown::GreenRed,
        }
    }

    /// The (up, down) pair for a theme whose own pair is `up`, `down` and whose brightness is
    /// `light` (a light theme gets the darker blue and orange).
    pub fn pair(self, up: u32, down: u32, light: bool) -> (u32, u32) {
        match self {
            UpDown::GreenRed => (up, down),
            UpDown::RedGreen => (down, up),
            UpDown::BlueOrange if light => (BLUE_ON_LIGHT, ORANGE_ON_LIGHT),
            UpDown::BlueOrange => (BLUE_ON_DARK, ORANGE_ON_DARK),
        }
    }
}

pub const BLUE_ON_DARK: u32 = bgr(130, 185, 255);
pub const ORANGE_ON_DARK: u32 = bgr(255, 160, 60);
pub const BLUE_ON_LIGHT: u32 = bgr(20, 90, 200);
pub const ORANGE_ON_LIGHT: u32 = bgr(176, 84, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_themes_own_pair_and_is_never_written() {
        assert_eq!(UpDown::default(), UpDown::GreenRed);
        assert_eq!(UpDown::GreenRed.pair(1, 2, false), (1, 2));
        assert_eq!(UpDown::GreenRed.to_saved(), None);
        assert_eq!(UpDown::from_saved(None), UpDown::GreenRed);
        assert_eq!(UpDown::from_saved(Some("sideways")), UpDown::GreenRed);
    }

    #[test]
    fn every_mode_round_trips_and_red_green_is_a_swap() {
        for m in UpDown::ALL {
            assert_eq!(UpDown::from_saved(m.to_saved().as_deref()), m);
        }
        assert_eq!(UpDown::from_saved(Some(" Red_Green ")), UpDown::RedGreen);
        assert_eq!(UpDown::RedGreen.pair(1, 2, true), (2, 1));
    }

    #[test]
    fn blue_orange_has_a_pair_for_dark_and_one_for_light() {
        assert_eq!(UpDown::BlueOrange.pair(1, 2, false), (BLUE_ON_DARK, ORANGE_ON_DARK));
        assert_eq!(UpDown::BlueOrange.pair(1, 2, true), (BLUE_ON_LIGHT, ORANGE_ON_LIGHT));
    }
}
