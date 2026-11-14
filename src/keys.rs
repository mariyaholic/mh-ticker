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
// W4 M1: the chart and menu keys.
pub const VK_A: u32 = 0x41;
pub const VK_R: u32 = 0x52;
pub const VK_APPS: u32 = 0x5D;
pub const VK_F1: u32 = 0x70;
pub const VK_F10: u32 = 0x79;

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
    /// Move the chart's crosshair one candle: -1 left (older), +1 right (newer). Chart open and a lock.
    Crosshair(i32),
    /// The next range tab, wrapping from 5Y to 1D. Chart open.
    NextRange,
    /// Flip the chart's clock between local and UTC. Chart open.
    FlipZone,
    /// Ask for an alert at the crosshair's price (through the price prompt). Chart open and a lock.
    AlertAtCrosshair,
    /// The card menu for the locked card (the menu key with a lock).
    CardMenu,
    /// The main menu (F10, and the menu key with no lock).
    MainMenu,
    /// List the keys.
    Help,
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
        // W4 M1. The chart keys answer only while the pane is open (and, for the two that act on a
        // candle's price, only with a card locked), so with the chart closed they are `None` and the
        // key goes back to the system as it always did.
        VK_R if state.chart_open => Some(Action::NextRange),
        VK_Z if state.chart_open => Some(Action::FlipZone),
        VK_A if state.chart_open && state.has_lock => Some(Action::AlertAtCrosshair),
        VK_APPS => Some(if state.has_lock { Action::CardMenu } else { Action::MainMenu }),
        VK_F10 => Some(Action::MainMenu),
        VK_F1 => Some(Action::Help),
        _ => None,
    }
}

/// `map` with the Shift key known: Shift+Left and Shift+Right move the chart's crosshair one candle
/// (`Crosshair(-1)`, `Crosshair(1)`) while the pane is open and a card is locked. Everything else,
/// including a plain arrow, is `map` unchanged, so the arrows keep their 1.8 meaning. Shift with any
/// other key is that key's own answer.
pub fn map_shifted(vk: u32, ctrl: bool, shift: bool, state: State) -> Option<Action> {
    if shift && !ctrl && state.ultra && state.chart_open && state.has_lock {
        match vk {
            VK_LEFT => return Some(Action::Crosshair(-1)),
            VK_RIGHT => return Some(Action::Crosshair(1)),
            _ => {}
        }
    }
    map(vk, ctrl, state)
}

/// The keys W4 added, which `is_bound` (P5's list) does not name and `ui.rs` routes to `map` too. The menu
/// key is not one: Windows turns its key-up into the context-menu message, and `ui.rs` answers `map` there.
pub fn is_reach_key(vk: u32) -> bool {
    matches!(vk, VK_A | VK_R | VK_F1 | VK_F10)
}

/// Where the crosshair goes when a key moves it: one candle from `current` among `bars` drawn, never
/// past either end. With no crosshair yet it starts from the newest candle (the right edge), so the
/// first Shift+Left lands one candle in from it and the first Shift+Right on it. `None` with no
/// candles. The result is a layout index, the same number a pointer over that candle gives
/// (`Layout::index_at(Layout::x_of(i)) == i`).
pub fn step_candle(current: Option<usize>, bars: usize, delta: i32) -> Option<usize> {
    if bars == 0 {
        return None;
    }
    let last = bars - 1;
    let from = current.map_or(last, |c| c.min(last));
    Some(if delta < 0 { from.saturating_sub(delta.unsigned_abs() as usize) } else { (from + delta as usize).min(last) })
}

/// One line of the key list: the keys as they are typed, what they do, and the action they map to
/// (`None` for the keys `ui.rs` owns outside `map`: Esc, paging, `/`).
pub struct Help {
    pub keys: &'static str,
    pub text: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    pub action: Option<Action>,
}

/// The key list, in reading order. `help_for` and F1's text are both read from it, so a key that is
/// not here has no line, and a test fails for any action `map` can answer that has none.
pub const HELP: [Help; 22] = [
    Help { keys: "Arrows, nothing locked", text: "lock the first card", action: Some(Action::LockFirst) },
    Help { keys: "Arrows", text: "move the lock", action: Some(Action::Move(Dir::Right)) },
    Help { keys: "Home, End", text: "lock the first or the last card", action: None },
    Help { keys: "Enter", text: "open the locked card on CNBC", action: Some(Action::OpenLocked) },
    Help { keys: "C", text: "open or close the chart", action: Some(Action::SetChart(true)) },
    Help { keys: "S", text: "open or close the symbol picker", action: Some(Action::SetPicker(true)) },
    Help { keys: "Ctrl+C", text: "copy the locked card", action: Some(Action::CopyLocked) },
    Help { keys: "Ctrl+C, nothing locked", text: "copy all prices", action: Some(Action::CopyAll) },
    Help { keys: "Delete", text: "take the locked card off the strip", action: Some(Action::Remove) },
    Help { keys: "Ctrl+Left, Ctrl+Right", text: "move the locked card in the order", action: Some(Action::Shift(1)) },
    Help { keys: "Ctrl+Z", text: "undo the last change", action: Some(Action::Undo) },
    Help { keys: "/", text: "find a symbol", action: None },
    Help { keys: "Shift+Left, Shift+Right", text: "move the chart crosshair one candle", action: Some(Action::Crosshair(1)) },
    Help { keys: "R", text: "next range on the chart", action: Some(Action::NextRange) },
    Help { keys: "Z", text: "chart clock, local or UTC", action: Some(Action::FlipZone) },
    Help { keys: "A", text: "alert at the crosshair price", action: Some(Action::AlertAtCrosshair) },
    Help { keys: "Menu key", text: "menu for the locked card", action: Some(Action::CardMenu) },
    Help { keys: "F10", text: "the main menu", action: Some(Action::MainMenu) },
    Help { keys: "F1", text: "this list", action: Some(Action::Help) },
    Help { keys: "Esc", text: "back to the taskbar", action: None },
    Help { keys: "Page Up, Page Down", text: "scroll the panel", action: None },
    Help { keys: "Mouse move", text: "takes the crosshair back", action: None },
];

/// Whether `action` has a line in the key list (the same variant; payloads do not matter).
#[cfg_attr(not(test), allow(dead_code))]
pub fn help_for(action: Action) -> Option<&'static Help> {
    HELP.iter().find(|h| h.action.is_some_and(|a| std::mem::discriminant(&a) == std::mem::discriminant(&action)))
}

/// F1's text: one `keys: what` line per entry (Ultra only: Classic honours Ctrl+C and nothing else).
pub fn help_text() -> String {
    HELP.iter()
        .map(|h| format!("{}: {}", h.keys, h.text))
        .collect::<Vec<_>>()
        .join("\n")
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

    fn chart(has_lock: bool, chart_open: bool) -> State {
        State { ultra: true, has_lock, picker_open: false, chart_open }
    }

    #[test]
    fn the_chart_keys_answer_only_with_the_chart_open() {
        for has_lock in [false, true] {
            assert_eq!(map(VK_R, false, chart(has_lock, true)), Some(Action::NextRange));
            assert_eq!(map(VK_Z, false, chart(has_lock, true)), Some(Action::FlipZone));
            assert_eq!(map(VK_R, false, chart(has_lock, false)), None, "R with the chart closed");
            assert_eq!(map(VK_Z, false, chart(has_lock, false)), None, "plain Z with the chart closed");
            // Ctrl+Z stays undo, open or closed.
            assert_eq!(map(VK_Z, true, chart(has_lock, true)), Some(Action::Undo));
            assert_eq!(map(VK_Z, true, chart(has_lock, false)), Some(Action::Undo));
            assert_eq!(map(VK_R, true, chart(has_lock, true)), None, "Ctrl+R");
        }
    }

    #[test]
    fn a_acts_on_the_crosshair_so_it_needs_the_chart_and_a_lock() {
        assert_eq!(map(VK_A, false, chart(true, true)), Some(Action::AlertAtCrosshair));
        assert_eq!(map(VK_A, false, chart(false, true)), None, "nothing locked");
        assert_eq!(map(VK_A, false, chart(true, false)), None, "chart closed");
        assert_eq!(map(VK_A, false, chart(false, false)), None);
        assert_eq!(map(VK_A, true, chart(true, true)), None, "Ctrl+A");
    }

    #[test]
    fn the_menu_key_opens_the_card_menu_with_a_lock_and_the_main_menu_without_one() {
        for chart_open in [false, true] {
            assert_eq!(map(VK_APPS, false, chart(true, chart_open)), Some(Action::CardMenu));
            assert_eq!(map(VK_APPS, false, chart(false, chart_open)), Some(Action::MainMenu));
            assert_eq!(map(VK_F10, false, chart(true, chart_open)), Some(Action::MainMenu));
            assert_eq!(map(VK_F10, false, chart(false, chart_open)), Some(Action::MainMenu));
            assert_eq!(map(VK_F1, false, chart(true, chart_open)), Some(Action::Help));
            assert_eq!(map(VK_F1, false, chart(false, chart_open)), Some(Action::Help));
        }
    }

    #[test]
    fn classic_keeps_1_8s_keys_whatever_the_chart_state() {
        for has_lock in [false, true] {
            for chart_open in [false, true] {
                let s = State { ultra: false, has_lock, picker_open: false, chart_open };
                for vk in [VK_R, VK_Z, VK_A, VK_APPS, VK_F1, VK_F10, VK_LEFT, VK_RIGHT] {
                    for shift in [false, true] {
                        assert_eq!(map_shifted(vk, false, shift, s), None, "vk {vk:#x} shift {shift}");
                    }
                }
            }
        }
    }

    #[test]
    fn shift_arrows_move_the_crosshair_and_plain_arrows_keep_their_1_8_meaning() {
        assert_eq!(map_shifted(VK_LEFT, false, true, chart(true, true)), Some(Action::Crosshair(-1)));
        assert_eq!(map_shifted(VK_RIGHT, false, true, chart(true, true)), Some(Action::Crosshair(1)));
        // Without the chart or without a lock, Shift+arrow is the plain arrow's answer.
        for (has_lock, chart_open) in [(true, false), (false, true), (false, false)] {
            let s = chart(has_lock, chart_open);
            assert_eq!(map_shifted(VK_LEFT, false, true, s), map(VK_LEFT, false, s));
        }
        for dir_key in [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN] {
            for s in [chart(true, true), chart(true, false), chart(false, true), chart(false, false)] {
                assert_eq!(map_shifted(dir_key, false, false, s), map(dir_key, false, s), "plain {dir_key:#x}");
            }
        }
        assert_eq!(map_shifted(VK_UP, false, true, chart(true, true)), map(VK_UP, false, chart(true, true)));
        // Ctrl+Shift+arrow is the Ctrl arrow.
        assert_eq!(map_shifted(VK_LEFT, true, true, chart(true, true)), Some(Action::Shift(-1)));
        // Shift with another key changes nothing.
        assert_eq!(map_shifted(VK_R, false, true, chart(true, true)), Some(Action::NextRange));
    }

    #[test]
    fn the_new_keys_are_named_by_is_reach_key_and_p5s_list_is_untouched() {
        for vk in [VK_A, VK_R, VK_F1, VK_F10] {
            assert!(is_reach_key(vk) && !is_bound(vk), "{vk:#x}");
        }
        assert!(!is_reach_key(VK_APPS), "the menu key is answered on the context-menu message");
        assert!(!is_reach_key(VK_Z) && !is_reach_key(VK_LEFT) && !is_reach_key(VK_ESCAPE));
    }

    #[test]
    fn step_candle_starts_at_the_newest_and_stops_at_both_ends() {
        assert_eq!(step_candle(None, 0, 1), None);
        assert_eq!(step_candle(Some(3), 0, -1), None);
        assert_eq!(step_candle(None, 10, 1), Some(9), "first Right: the newest");
        assert_eq!(step_candle(None, 10, -1), Some(8), "first Left: one in from the newest");
        assert_eq!(step_candle(Some(0), 10, -1), Some(0));
        assert_eq!(step_candle(Some(9), 10, 1), Some(9));
        assert_eq!(step_candle(Some(4), 10, -1), Some(3));
        assert_eq!(step_candle(Some(4), 10, 1), Some(5));
        assert_eq!(step_candle(Some(40), 10, -1), Some(8), "a stale index is brought back first");
        assert_eq!(step_candle(None, 1, -1), Some(0));
        for bars in 1..40usize {
            for from in 0..bars + 3 {
                for d in [-1, 1] {
                    assert!(step_candle(Some(from), bars, d).unwrap() < bars);
                }
            }
        }
    }

    /// Every action `map` or `map_shifted` can answer, for any key, modifier and state, has a line in
    /// the key list, and every line names keys that really give its action.
    #[test]
    fn every_action_has_a_help_line_and_every_line_is_true() {
        let mut seen = Vec::new();
        for vk in 0..=255u32 {
            for ctrl in [false, true] {
                for shift in [false, true] {
                    for ultra in [false, true] {
                        for (has_lock, picker_open, chart_open) in [(false, false, false), (true, false, false), (false, false, true), (true, true, true)] {
                            let s = State { ultra, has_lock, picker_open, chart_open };
                            if let Some(a) = map_shifted(vk, ctrl, shift, s) {
                                assert!(help_for(a).is_some(), "{a:?} (vk {vk:#x}) has no help line");
                                seen.push(std::mem::discriminant(&a));
                            }
                        }
                    }
                }
            }
        }
        seen.dedup();
        assert!(seen.len() >= 15, "the sweep reached {} kinds of action", seen.len());
        for h in HELP.iter().filter_map(|h| h.action.map(|a| (h, a))) {
            assert_eq!(help_for(h.1).map(|x| x.keys), Some(h.0.keys), "{:?}", h.1);
        }
        let text = help_text();
        assert_eq!(text.lines().count(), HELP.len());
        for h in &HELP {
            assert!(text.contains(h.keys) && text.contains(h.text));
        }
        assert!(!text.contains('\u{2014}'));
    }
}
