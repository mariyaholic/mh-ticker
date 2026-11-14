//! P5: the keyboard map for the open panel, card-grid stepping, and the strip's click rectangles.
//!
//! Pure and portable: `std` only, no `windows_sys`, no clock. Virtual-key codes are plain `u32`
//! constants defined here, so the module runs natively under `scripts/check-state-linux.sh`.
//!
//! `ui.rs` owns every side effect. It asks `map` what a key means, applies the answer inside one
//! `with_ui` closure where that only changes state, and does the clipboard, the browser, the config
//! save and the menus after the closure has returned (`with_ui` does not nest).

pub const VK_RETURN: u32 = 0x0D;
// Esc and the paging keys are owned by `ui.rs` (it reads the `windows_sys` constants), so these five
// are used only by the tests, which check that `map` leaves them alone.
#[cfg_attr(not(test), allow(dead_code))]
pub const VK_ESCAPE: u32 = 0x1B;
#[cfg_attr(not(test), allow(dead_code))]
pub const VK_PRIOR: u32 = 0x21;
#[cfg_attr(not(test), allow(dead_code))]
pub const VK_NEXT: u32 = 0x22;
#[cfg_attr(not(test), allow(dead_code))]
pub const VK_END: u32 = 0x23;
#[cfg_attr(not(test), allow(dead_code))]
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_C: u32 = 0x43;
pub const VK_S: u32 = 0x53;
pub const VK_Z: u32 = 0x5A;

/// A direction on the card grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// What a key means. `ui.rs` turns each into its effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing is locked: lock the first card.
    LockFirst,
    /// Move the lock one step on the grid (see `step`).
    Move(Dir),
    /// Open the locked card on CNBC.
    OpenLocked,
    /// Open (`true`) or close (`false`) the chart: always the opposite of `State::chart_open`.
    SetChart(bool),
    /// Open (`true`) or close (`false`) the symbol picker: the opposite of `State::picker_open`.
    SetPicker(bool),
    /// Copy the locked card's line (`LABEL price`).
    CopyLocked,
    /// Copy every price (what the menu's "Copy all prices" does).
    CopyAll,
    /// Remove the locked card from the strip (the config refuses the last one).
    Remove,
    /// Move the locked card one place in the order: -1 left, +1 right.
    Shift(i32),
    /// Put the held undo snapshot back (Ultra; Classic's undo is the menu line).
    Undo,
}

/// What `map` needs to know. Plain data, so the table is testable without a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The Ultra build. Classic honours only Ctrl+C.
    pub ultra: bool,
    /// A card is locked and still on the strip.
    pub has_lock: bool,
    pub picker_open: bool,
    pub chart_open: bool,
}

/// Every key `map` can answer for. `ui.rs` routes exactly these to `map` and leaves the rest
/// (Esc, PgUp, PgDn, Home, End, anything else) to the arms that already own them.
pub fn is_bound(vk: u32) -> bool {
    matches!(vk, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_RETURN | VK_DELETE | VK_C | VK_S | VK_Z)
}

/// The meaning of a key press in the open panel, or `None` when the key does nothing here.
///
/// | key | meaning | build |
/// |---|---|---|
/// | arrows | `LockFirst` with no lock, else `Move(dir)` | Ultra |
/// | Enter | `OpenLocked` (only with a lock) | Ultra |
/// | C | `SetChart(!chart_open)` | Ultra |
/// | S | `SetPicker(!picker_open)` | Ultra |
/// | Ctrl+C | `CopyLocked` with a lock on Ultra, else `CopyAll` | both |
/// | Delete | `Remove` (only with a lock) | Ultra |
/// | Ctrl+Left, Ctrl+Right | `Shift(-1)`, `Shift(1)` (only with a lock) | Ultra |
/// | Ctrl+Z | `Undo` (with or without a lock) | Ultra |
///
/// A plain arrow with Ctrl held and a vertical arrow with Ctrl are `None` (Ctrl+Up and Ctrl+Down
/// mean nothing). Plain C, S, Enter and Delete with Ctrl held are `None`, except Ctrl+C. Every
/// other key is `None`, including Esc and the paging keys.
pub fn map(vk: u32, ctrl: bool, state: State) -> Option<Action> {
    if vk == VK_C && ctrl {
        // Classic has no card lock, so its Ctrl+C always copies everything.
        return Some(if state.ultra && state.has_lock { Action::CopyLocked } else { Action::CopyAll });
    }
    if !state.ultra {
        return None;
    }
    if vk == VK_Z && ctrl {
        return Some(Action::Undo);
    }
    let dir = match vk {
        VK_LEFT => Some(Dir::Left),
        VK_RIGHT => Some(Dir::Right),
        VK_UP => Some(Dir::Up),
        VK_DOWN => Some(Dir::Down),
        _ => None,
    };
    if let Some(dir) = dir {
        if ctrl {
            // Ctrl+Left and Ctrl+Right move the locked card; Ctrl+Up and Ctrl+Down do nothing.
            return match (dir, state.has_lock) {
                (Dir::Left, true) => Some(Action::Shift(-1)),
                (Dir::Right, true) => Some(Action::Shift(1)),
                _ => None,
            };
        }
        return Some(if state.has_lock { Action::Move(dir) } else { Action::LockFirst });
    }
    if ctrl {
        return None;
    }
    match vk {
        VK_RETURN if state.has_lock => Some(Action::OpenLocked),
        VK_DELETE if state.has_lock => Some(Action::Remove),
        VK_C => Some(Action::SetChart(!state.chart_open)),
        VK_S => Some(Action::SetPicker(!state.picker_open)),
        _ => None,
    }
}

/// What Home and End mean (W3 J6). On Ultra with cards on the panel they lock the first (`false`) or the
/// last (`true`) card, and the body scrolls to show it; on Classic, or with no cards, they are `None`
/// and keep paging the body as in 1.8.
pub fn edge(vk: u32, ultra: bool, cards: usize) -> Option<bool> {
    if !ultra || cards == 0 {
        return None;
    }
    match vk {
        VK_HOME => Some(false),
        VK_END => Some(true),
        _ => None,
    }
}

/// Does the typed character open the find prompt? `/` on Ultra, with or without a card locked (the
/// character, not a key code, so a layout that types `/` on another key still works). Classic: no.
pub fn opens_find(ch: u32, ultra: bool, ctrl: bool) -> bool {
    ultra && !ctrl && ch == u32::from(b'/')
}

/// Where the lock goes from `index` on a grid of `n` cards laid out `cols` per row, row by row.
///
/// Left and right walk the cards in reading order and stop at the ends (index 0 stays 0, the last
/// card stays last); they cross from the end of one row to the start of the next. Up and down move
/// by `cols` and stay put when there is no card there (a ragged last row has no card below the
/// columns it does not reach). `n == 0` answers 0, `cols == 0` is treated as 1, and an `index`
/// past the end is first brought back to the last card. The result is always below `n` (or 0).
pub fn step(index: usize, n: usize, cols: usize, dir: Dir) -> usize {
    if n == 0 {
        return 0;
    }
    let cols = cols.max(1);
    let last = n - 1;
    let index = index.min(last);
    match dir {
        Dir::Left => index.saturating_sub(1),
        Dir::Right => (index + 1).min(last),
        Dir::Up => {
            if index >= cols {
                index - cols
            } else {
                index
            }
        }
        Dir::Down => {
            let below = index + cols;
            if below <= last {
                below
            } else {
                index
            }
        }
    }
}

/// One symbol's clickable rectangle on the strip, in strip pixels (half open: `left <= x < right`).
/// `index` is the position in `Board::cells`, which is also the position in the frame's cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub index: usize,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The slot under (x, y), or `None` between symbols. The strip painter and this test read the same
/// `Slot`s, which the layout functions record while they place the text.
pub fn slot_at(slots: &[Slot], x: i32, y: i32) -> Option<usize> {
    slots.iter().find(|s| x >= s.left && x < s.right && y >= s.top && y < s.bottom).map(|s| s.index)
}

/// The strip shows only the pinned cards, so the layout numbers its slots by position on the strip.
/// Turn each slot's `index` into the card's own index (`cards[slot.index]`, as `pins::strip_indices`
/// gave them), so a click answers with the card on the panel. A slot with no card (cannot happen with
/// the layout's own list) is dropped.
pub fn remap_slots(slots: Vec<Slot>, cards: &[usize]) -> Vec<Slot> {
    slots.into_iter().filter_map(|s| cards.get(s.index).map(|&index| Slot { index, ..s })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_slots_points_each_rectangle_at_its_own_card() {
        let slots = vec![
            Slot { index: 0, left: 0, top: 0, right: 10, bottom: 10 },
            Slot { index: 1, left: 12, top: 0, right: 22, bottom: 10 },
            Slot { index: 2, left: 24, top: 0, right: 34, bottom: 10 },
        ];
        let cards = [2, 5, 9];
        let mapped = remap_slots(slots.clone(), &cards);
        assert_eq!(slot_at(&mapped, 13, 5), Some(5));
        assert_eq!(slot_at(&mapped, 30, 5), Some(9));
        assert_eq!(slot_at(&mapped, 1, 1), Some(2));
        assert_eq!(mapped[1].left, 12, "the rectangle itself is untouched");
        // Nothing unpinned: the identity.
        assert_eq!(remap_slots(slots.clone(), &[0, 1, 2]), slots);
        // A slot past the list is dropped, not mapped to a wrong card.
        assert_eq!(remap_slots(slots, &[4]).len(), 1);
    }

    fn ultra(has_lock: bool) -> State {
        State { ultra: true, has_lock, picker_open: false, chart_open: false }
    }

    #[test]
    fn is_bound_names_exactly_the_keys_map_answers() {
        for vk in [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_RETURN, VK_DELETE, VK_C, VK_S, VK_Z] {
            assert!(is_bound(vk));
        }
        for vk in [VK_ESCAPE, VK_PRIOR, VK_NEXT, VK_HOME, VK_END, 0x41, 0] {
            assert!(!is_bound(vk));
        }
    }

    #[test]
    fn slot_at_is_half_open_and_none_between_symbols() {
        let slots = [Slot { index: 0, left: 10, top: 0, right: 50, bottom: 20 }, Slot { index: 1, left: 60, top: 0, right: 90, bottom: 20 }];
        assert_eq!(slot_at(&slots, 10, 0), Some(0));
        assert_eq!(slot_at(&slots, 49, 19), Some(0));
        assert_eq!(slot_at(&slots, 50, 5), None, "right edge is outside");
        assert_eq!(slot_at(&slots, 55, 5), None, "the gap");
        assert_eq!(slot_at(&slots, 60, 5), Some(1));
        assert_eq!(slot_at(&slots, 70, 20), None, "bottom edge is outside");
        assert_eq!(slot_at(&slots, 5, 5), None);
        assert_eq!(slot_at(&[], 0, 0), None);
    }

    #[test]
    fn the_test_helper_builds_what_the_finisher_expects() {
        assert!(ultra(true).has_lock && !ultra(false).has_lock);
    }

    fn classic(has_lock: bool) -> State {
        State { ultra: false, has_lock, picker_open: false, chart_open: false }
    }

    const ARROWS: [(u32, Dir); 4] = [(VK_LEFT, Dir::Left), (VK_RIGHT, Dir::Right), (VK_UP, Dir::Up), (VK_DOWN, Dir::Down)];

    #[test]
    fn map_arrows_lock_the_first_card_when_nothing_is_locked_and_move_the_lock_otherwise() {
        for (vk, dir) in ARROWS {
            assert_eq!(map(vk, false, ultra(false)), Some(Action::LockFirst));
            assert_eq!(map(vk, false, ultra(true)), Some(Action::Move(dir)));
            assert_eq!(map(vk, false, classic(false)), None);
            assert_eq!(map(vk, false, classic(true)), None);
        }
    }

    #[test]
    fn map_enter_and_delete_need_a_lock_on_ultra_and_do_nothing_on_classic() {
        assert_eq!(map(VK_RETURN, false, ultra(true)), Some(Action::OpenLocked));
        assert_eq!(map(VK_DELETE, false, ultra(true)), Some(Action::Remove));
        assert_eq!(map(VK_RETURN, false, ultra(false)), None);
        assert_eq!(map(VK_DELETE, false, ultra(false)), None);
        assert_eq!(map(VK_RETURN, false, classic(true)), None);
        assert_eq!(map(VK_DELETE, false, classic(true)), None);
        assert_eq!(map(VK_RETURN, true, ultra(true)), None, "Ctrl+Enter");
        assert_eq!(map(VK_DELETE, true, ultra(true)), None, "Ctrl+Delete");
    }

    #[test]
    fn map_c_and_s_toggle_to_the_opposite_state() {
        for has_lock in [false, true] {
            let closed = State { ultra: true, has_lock, picker_open: false, chart_open: false };
            let open = State { chart_open: true, picker_open: true, ..closed };
            assert_eq!(map(VK_C, false, closed), Some(Action::SetChart(true)));
            assert_eq!(map(VK_C, false, open), Some(Action::SetChart(false)));
            assert_eq!(map(VK_S, false, closed), Some(Action::SetPicker(true)));
            assert_eq!(map(VK_S, false, open), Some(Action::SetPicker(false)));
        }
        assert_eq!(map(VK_C, false, classic(true)), None);
        assert_eq!(map(VK_S, false, classic(true)), None);
        assert_eq!(map(VK_S, true, ultra(true)), None, "Ctrl+S");
    }

    #[test]
    fn map_ctrl_c_copies_the_lock_or_everything_on_both_builds() {
        assert_eq!(map(VK_C, true, ultra(true)), Some(Action::CopyLocked));
        assert_eq!(map(VK_C, true, ultra(false)), Some(Action::CopyAll));
        assert_eq!(map(VK_C, true, classic(true)), Some(Action::CopyAll), "Classic has no lock");
        assert_eq!(map(VK_C, true, classic(false)), Some(Action::CopyAll));
        assert_eq!(map(VK_C, false, classic(true)), None, "plain C on Classic");
        assert_eq!(map(VK_C, false, classic(false)), None);
    }

    #[test]
    fn map_ctrl_left_and_right_shift_the_locked_card() {
        assert_eq!(map(VK_LEFT, true, ultra(true)), Some(Action::Shift(-1)));
        assert_eq!(map(VK_RIGHT, true, ultra(true)), Some(Action::Shift(1)));
        assert_eq!(map(VK_LEFT, true, ultra(false)), None, "nothing locked");
        assert_eq!(map(VK_RIGHT, true, ultra(false)), None, "nothing locked");
        assert_eq!(map(VK_LEFT, true, classic(true)), None);
        assert_eq!(map(VK_RIGHT, true, classic(true)), None);
        for has_lock in [false, true] {
            assert_eq!(map(VK_UP, true, ultra(has_lock)), None, "Ctrl+Up");
            assert_eq!(map(VK_DOWN, true, ultra(has_lock)), None, "Ctrl+Down");
        }
    }

    #[test]
    fn home_and_end_lock_the_first_and_last_card_on_ultra_and_keep_paging_on_classic() {
        assert_eq!(edge(VK_HOME, true, 6), Some(false));
        assert_eq!(edge(VK_END, true, 6), Some(true));
        assert_eq!(edge(VK_HOME, false, 6), None, "Classic pages");
        assert_eq!(edge(VK_END, false, 6), None);
        assert_eq!(edge(VK_HOME, true, 0), None, "no cards: page");
        assert_eq!(edge(VK_PRIOR, true, 6), None);
        assert_eq!(edge(VK_NEXT, true, 6), None);
    }

    #[test]
    fn the_slash_character_opens_find_on_ultra_only() {
        assert!(opens_find(u32::from(b'/'), true, false));
        assert!(!opens_find(u32::from(b'/'), false, false));
        assert!(!opens_find(u32::from(b'/'), true, true), "Ctrl+/ is not it");
        assert!(!opens_find(u32::from(b'?'), true, false));
    }

    #[test]
    fn map_ctrl_z_undoes_on_ultra_only_and_plain_z_is_nothing() {
        for has_lock in [false, true] {
            assert_eq!(map(VK_Z, true, ultra(has_lock)), Some(Action::Undo));
            assert_eq!(map(VK_Z, false, ultra(has_lock)), None, "plain Z");
            assert_eq!(map(VK_Z, true, classic(has_lock)), None, "Classic keeps 1.8's keys");
            assert_eq!(map(VK_Z, false, classic(has_lock)), None);
        }
    }

    #[test]
    fn map_leaves_esc_paging_and_unknown_keys_alone() {
        let states = [ultra(false), ultra(true), classic(false), classic(true)];
        for vk in [VK_ESCAPE, VK_PRIOR, VK_NEXT, VK_HOME, VK_END, 0x41, 0] {
            for state in states {
                assert_eq!(map(vk, false, state), None, "vk {vk:#x}");
                assert_eq!(map(vk, true, state), None, "Ctrl+vk {vk:#x}");
            }
        }
    }

    #[test]
    fn step_left_and_right_stop_at_the_ends_on_grids_of_1_2_3_and_6_columns() {
        for n in [1usize, 2, 3, 6, 7] {
            for cols in [1usize, 2, 3, 6] {
                assert_eq!(step(0, n, cols, Dir::Left), 0, "n {n} cols {cols}");
                assert_eq!(step(n - 1, n, cols, Dir::Right), n - 1, "n {n} cols {cols}");
                for i in 0..n - 1 {
                    assert_eq!(step(i, n, cols, Dir::Right), i + 1, "n {n} cols {cols} from {i}");
                    assert_eq!(step(i + 1, n, cols, Dir::Left), i, "n {n} cols {cols} from {}", i + 1);
                }
            }
        }
        // Right from the end of a row lands on the start of the next row.
        assert_eq!(step(2, 7, 3, Dir::Right), 3);
        assert_eq!(step(5, 7, 3, Dir::Right), 6);
        assert_eq!(step(3, 7, 3, Dir::Left), 2);
    }

    #[test]
    fn step_up_and_down_stay_put_where_there_is_no_card() {
        assert_eq!(step(2, 5, 3, Dir::Down), 2, "no card at 5");
        assert_eq!(step(1, 5, 3, Dir::Down), 4);
        assert_eq!(step(0, 5, 3, Dir::Up), 0);
        assert_eq!(step(4, 5, 3, Dir::Up), 1);
        assert_eq!(step(0, 5, 1, Dir::Up), 0, "cols 1: Up stops at the top");
        assert_eq!(step(0, 5, 1, Dir::Down), 1);
        assert_eq!(step(4, 5, 1, Dir::Down), 4);
        assert_eq!(step(4, 5, 1, Dir::Up), 3);
        assert_eq!(step(0, 1, 3, Dir::Down), 0, "one card");
    }

    #[test]
    fn step_survives_degenerate_input() {
        let dirs = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
        for dir in dirs {
            for index in 0..4 {
                assert_eq!(step(index, 0, 3, dir), 0, "n 0");
            }
        }
        for dir in dirs {
            for index in 0..5 {
                assert_eq!(step(index, 5, 0, dir), step(index, 5, 1, dir), "cols 0 is 1");
            }
        }
        assert_eq!(step(9, 4, 2, Dir::Right), 3, "past the end clamps to the last card");
        assert_eq!(step(9, 4, 2, Dir::Left), 2, "past the end clamps then steps left");
        assert_eq!(step(9, 4, 2, Dir::Up), 1);
        for n in 1..=8usize {
            for cols in 0..=4usize {
                for index in 0..=10usize {
                    for dir in dirs {
                        assert!(step(index, n, cols, dir) < n, "n {n} cols {cols} index {index} {dir:?}");
                    }
                }
            }
        }
    }
}
