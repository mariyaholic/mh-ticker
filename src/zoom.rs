//! Panel zoom, `std` only. The panel is laid out and drawn for an effective DPI, which is the
//! window's DPI times the zoom. Zoom 150 percent at 96 DPI therefore draws exactly what 100 percent
//! draws at 144 DPI, and the layout, the hit tests and the stills need no second code path.

/// The steps the menu offers, in percent.
pub const STEPS: [u32; 5] = [75, 90, 100, 125, 150];

/// The step a saved value names; anything that is not a step is 100.
pub fn from_saved(saved: Option<u32>) -> u32 {
    match saved {
        Some(z) if STEPS.contains(&z) => z,
        _ => 100,
    }
}

/// What the settings file holds: nothing for 100.
pub fn to_saved(zoom: u32) -> Option<u32> {
    (zoom != 100 && STEPS.contains(&zoom)).then_some(zoom)
}

/// The DPI the panel is drawn at. Never below 48, so a pixel is never lost to rounding.
pub fn effective_dpi(dpi: u32, zoom: u32) -> u32 {
    ((dpi as u64 * zoom as u64 + 50) / 100).max(48) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_is_a_dpi_multiplier_and_a_hundred_is_the_identity() {
        for dpi in [96, 120, 144, 168, 192, 288] {
            assert_eq!(effective_dpi(dpi, 100), dpi);
        }
        assert_eq!(effective_dpi(96, 150), 144);
        assert_eq!(effective_dpi(96, 125), 120);
        assert_eq!(effective_dpi(96, 75), 72);
        assert_eq!(effective_dpi(96, 90), 86);
    }

    #[test]
    fn saved_values_round_trip_and_unknown_numbers_read_as_a_hundred() {
        for z in STEPS {
            assert_eq!(from_saved(to_saved(z)), z);
        }
        assert_eq!(to_saved(100), None);
        assert_eq!(from_saved(Some(110)), 100);
        assert_eq!(from_saved(None), 100);
    }
}
