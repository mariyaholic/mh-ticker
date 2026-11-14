//! How the panel opens and closes, `std` only.
//!
//! The panel glides between the strip and itself, and the chart pane glides the window wider. Windows
//! has a "Show animations" switch, and a person who turned it off should not be shown a glide. Ultra
//! follows that switch by default. Classic does not, because its default is the 1.8 behaviour: it
//! always glides until the user ticks "Follow the Windows animation setting".

/// How a transition is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frames {
    /// The whole glide, paced by the display.
    Glide,
    /// The end state in one step: one resize, one repaint, no in-between frames.
    One,
}

/// `follow` is the setting (follow Windows), `system` is whether Windows has animations on.
pub fn frames(follow: bool, system: bool) -> Frames {
    if follow && !system {
        Frames::One
    } else {
        Frames::Glide
    }
}

impl Frames {
    /// The duration to give the morph loop: the full time for a glide, nothing for one step (the loop
    /// then runs its single iteration at the end state and stops).
    pub fn ms(self, full: f64) -> f64 {
        match self {
            Frames::Glide => full,
            Frames::One => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_one_step_and_on_is_the_glide() {
        assert_eq!(frames(true, false), Frames::One);
        assert_eq!(frames(true, true), Frames::Glide);
    }

    #[test]
    fn a_build_that_does_not_follow_windows_always_glides() {
        // Classic's default: the setting is not followed, so the Windows switch changes nothing.
        assert_eq!(frames(false, false), Frames::Glide);
        assert_eq!(frames(false, true), Frames::Glide);
    }

    #[test]
    fn a_single_step_has_no_duration() {
        assert_eq!(Frames::One.ms(230.0), 0.0);
        assert_eq!(Frames::Glide.ms(230.0), 230.0);
    }
}
