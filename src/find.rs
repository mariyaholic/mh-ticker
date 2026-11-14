//! W3 J6: finding a card, and scrolling the body to show it. Pure and portable: `std` only.

/// The first card matching `text`, by index into `cards` (`(label, name)` pairs, in card order).
/// Case is ignored (ASCII and Unicode lower-casing). Three tiers, best first, and within a tier the
/// first card in order wins: a label that starts with the text, a label that contains it, a name that
/// contains it. Blank text (or only spaces) matches nothing, as does text no card has; the caller then
/// leaves the lock where it is.
pub fn find(cards: &[(&str, &str)], text: &str) -> Option<usize> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let lower: Vec<(String, String)> = cards.iter().map(|(l, n)| (l.to_lowercase(), n.to_lowercase())).collect();
    lower
        .iter()
        .position(|(l, _)| l.starts_with(&needle))
        .or_else(|| lower.iter().position(|(l, _)| l.contains(&needle)))
        .or_else(|| lower.iter().position(|(_, n)| n.contains(&needle)))
}

/// The scroll offset that shows the item spanning `top..bottom` (content coordinates, so they do not
/// move with the scroll) in a body `view` tall, from the current offset `scroll`, within `0..=max`.
/// An item already inside leaves the offset alone; one above the view is aligned to its top; one below
/// is aligned to its bottom; one taller than the view is aligned to its top, which is the part that
/// names it. The answer is always within `0..=max`.
pub fn scroll_to_show(scroll: i32, view: i32, top: i32, bottom: i32, max: i32) -> i32 {
    let height = bottom - top;
    let want = if height >= view || top < scroll {
        top
    } else if bottom > scroll + view {
        bottom - view
    } else {
        scroll
    };
    want.clamp(0, max.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARDS: [(&str, &str); 5] = [
        ("XAU", "Gold spot"),
        ("XAG", "Silver spot"),
        ("GOLDX", "Gold miners"),
        ("BTC", "Bitcoin (Coinbase)"),
        ("DOW", "Dow futures, delayed"),
    ];

    #[test]
    fn a_prefix_beats_a_substring_wherever_it_sits() {
        assert_eq!(find(&CARDS, "x"), Some(0), "XAU starts with x; GOLDX only contains it");
        assert_eq!(find(&CARDS, "gold"), Some(2), "a label prefix beats the earlier card whose name has gold");
        assert_eq!(find(&CARDS, "OL"), Some(2), "a label substring: GOLDX before DOW");
    }

    #[test]
    fn case_is_ignored_and_the_name_is_the_last_resort() {
        assert_eq!(find(&CARDS, "dow"), Some(4));
        assert_eq!(find(&CARDS, "DoW"), Some(4));
        assert_eq!(find(&CARDS, "coinbase"), Some(3));
        assert_eq!(find(&CARDS, "  btc "), Some(3), "spaces around are not part of it");
    }

    #[test]
    fn no_match_and_blank_text_answer_none_so_the_lock_stays() {
        assert_eq!(find(&CARDS, "zzz"), None);
        assert_eq!(find(&CARDS, ""), None);
        assert_eq!(find(&CARDS, "   "), None);
        assert_eq!(find(&[], "x"), None);
    }

    #[test]
    fn the_first_card_in_order_wins_within_a_tier() {
        let cards = [("AB", ""), ("AC", ""), ("A", "")];
        assert_eq!(find(&cards, "a"), Some(0));
    }

    #[test]
    fn scroll_to_show_at_the_top_leaves_a_visible_card_and_pulls_up_one_above() {
        // A view 300 tall, scrolled to 0: an item at 20..120 is inside.
        assert_eq!(scroll_to_show(0, 300, 20, 120, 900), 0);
        // Scrolled to 400, an item at 100..200 is above: its top goes to the top.
        assert_eq!(scroll_to_show(400, 300, 100, 200, 900), 100);
        // The first card never scrolls past 0.
        assert_eq!(scroll_to_show(50, 300, 0, 100, 900), 0);
    }

    #[test]
    fn scroll_to_show_at_the_bottom_brings_the_card_up_to_the_bottom_edge_and_stops_at_the_end() {
        // Item 500..600 in a 300-tall view at 0: the bottom edge meets 600.
        assert_eq!(scroll_to_show(0, 300, 500, 600, 900), 300);
        // The last card: 800..900 with max 600 gives 600 (the end), never beyond.
        assert_eq!(scroll_to_show(0, 300, 800, 900, 600), 600);
        // Already inside at the bottom edge: unchanged.
        assert_eq!(scroll_to_show(300, 300, 500, 600, 900), 300);
    }

    #[test]
    fn a_card_taller_than_the_view_is_aligned_to_its_top() {
        assert_eq!(scroll_to_show(0, 200, 300, 700, 900), 300);
        assert_eq!(scroll_to_show(800, 200, 300, 700, 900), 300);
        assert_eq!(scroll_to_show(300, 200, 300, 700, 900), 300, "and stays there");
    }

    #[test]
    fn scroll_to_show_never_leaves_the_range() {
        for scroll in [-5, 0, 50, 1000] {
            for (top, bottom) in [(0, 10), (100, 400), (880, 990), (-20, 5)] {
                for max in [0, 300, 900] {
                    let got = scroll_to_show(scroll, 250, top, bottom, max);
                    assert!((0..=max).contains(&got), "{scroll} {top} {bottom} {max}: {got}");
                }
            }
        }
        assert_eq!(scroll_to_show(10, 100, 0, 50, -5), 0, "a negative maximum reads as zero");
    }
}
