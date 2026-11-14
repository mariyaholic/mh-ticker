//! W3 J1: which cards are on the taskbar strip. Pure and portable: `std` only.
//!
//! The panel always shows every card; the strip shows the pinned ones. A card is pinned unless its row
//! says `strip: false`, so a config without the field puts every card on the strip, as 1.8 did. One
//! card is always on the strip: when none is pinned (a hand-edited file) every card is.

/// The cards on the strip, as indexes into the list `pinned` describes (`pinned[i]` is whether card
/// `i` is pinned), in card order. Every card when none is pinned, so the strip is never empty unless
/// there are no cards at all.
pub fn strip_indices(pinned: &[bool]) -> Vec<usize> {
    let shown: Vec<usize> = pinned.iter().enumerate().filter(|(_, p)| **p).map(|(i, _)| i).collect();
    if shown.is_empty() {
        (0..pinned.len()).collect()
    } else {
        shown
    }
}

/// May card `i` be unpinned? Not when it is the only pinned one (or not pinned at all, or absent).
pub fn can_unpin(pinned: &[bool], i: usize) -> bool {
    pinned.get(i) == Some(&true) && pinned.iter().filter(|p| **p).count() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_is_on_the_strip_when_all_are_pinned_or_none_is() {
        assert_eq!(strip_indices(&[true, true, true]), [0, 1, 2]);
        assert_eq!(strip_indices(&[false, false, false]), [0, 1, 2], "one row is always shown");
        assert!(strip_indices(&[]).is_empty());
    }

    #[test]
    fn only_pinned_cards_are_on_the_strip_in_card_order() {
        assert_eq!(strip_indices(&[false, true, false, true]), [1, 3]);
    }

    #[test]
    fn the_last_pinned_card_cannot_be_unpinned() {
        assert!(can_unpin(&[true, true], 0));
        assert!(!can_unpin(&[false, true], 1));
        assert!(!can_unpin(&[true], 0));
        assert!(!can_unpin(&[true, false], 1), "not pinned: nothing to unpin");
        assert!(!can_unpin(&[true, true], 5), "absent");
    }
}
