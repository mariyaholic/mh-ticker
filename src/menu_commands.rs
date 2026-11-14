//! Per-popup command registry. IDs must not depend on an unbounded catalogue index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    Symbol(String),
    /// Card menu: copy this card's label and price (found by label when chosen).
    Copy(String),
    /// Card menu: open this symbol on CNBC.
    Open(String),
    /// Card menu: hide this symbol (the config refuses the last one).
    Remove(String),
    /// Card menu: move this symbol by this many places (-1 left, 1 right).
    Move(String, i32),
    /// Card menu: move this symbol to the front.
    MoveFirst(String),
    /// Card menu: pin the symbol to the taskbar strip (`true`) or take it off (`false`).
    Pin(String, bool),
    /// Card menu: ask for a new label for this symbol.
    Rename(String),
    /// Card menu: the derived label back on this symbol.
    ResetName(String),
    /// Card menu, Decimals: this many places, or `None` for auto.
    Digits(String, Option<u32>),
    Alert(String, usize),
    /// Alerts submenu, per symbol (Ultra): the fast-move choice `alerts::FAST_MOVES[k]`; the current one again turns it off.
    Fast(String, usize),
    ClearAlert(String),
    /// Alerts submenu, per symbol (Ultra): ask for how long this symbol's alert waits before it can fire again.
    Cooldown(String),
    /// Alerts submenu, per symbol (Ultra): ask for a price level for this label.
    AlertAt(String),
    /// Symbols > Remove a symbol I added (both builds): drop this catalogue entry, by symbol.
    Forget(String),
    Monitor(Option<String>),
}

// The other menu commands occupy IDs below 6000. TrackPopupMenu uses TPM_RETURNCMD,
// which returns the selected command directly rather than routing a WM_COMMAND.
const FIRST: usize = 10_000;
// Stay within Win32's 16-bit command-ID range even for hostile-size catalogs.
// Exhaustion disables further dynamic entries; it never aliases a prior action.
const LAST: usize = u16::MAX as usize;

#[derive(Default)]
pub(crate) struct MenuCommands(Vec<Action>);

impl MenuCommands {
    pub(crate) fn add(&mut self, action: Action) -> Option<usize> {
        let id = FIRST.checked_add(self.0.len()).filter(|id| *id <= LAST)?;
        self.0.push(action);
        Some(id)
    }

    pub(crate) fn get(&self, id: usize) -> Option<&Action> {
        id.checked_sub(FIRST).and_then(|i| self.0.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_never_wraps_or_reuses_a_command() {
        let mut commands = MenuCommands::default();
        for id in FIRST..=LAST {
            assert_eq!(commands.add(Action::ClearAlert(id.to_string())), Some(id));
        }
        for _ in 0..100 {
            assert_eq!(commands.add(Action::Symbol("overflow".into())), None);
        }
        assert_eq!(commands.get(LAST), Some(&Action::ClearAlert(LAST.to_string())));
        assert_eq!(commands.get(LAST + 1), None);
        assert_eq!(commands.get(0), None);
        assert_eq!(commands.get(5400), None);
    }

    #[test]
    fn large_catalogue_and_alerts_keep_unique_identities() {
        let mut commands = MenuCommands::default();
        let mut expected = Vec::new();
        for i in 0..10_000 {
            let symbol = format!("symbol-{i}");
            let actions = [Action::Symbol(symbol.clone()), Action::Alert(symbol.clone(), i % 4), Action::ClearAlert(symbol)];
            for action in actions {
                let id = commands.add(action.clone()).unwrap();
                expected.push((id, action));
            }
        }
        for (id, action) in expected {
            assert_eq!(commands.get(id), Some(&action));
        }
        let monitor = commands.add(Action::Monitor(Some("display-1".into()))).unwrap();
        assert_eq!(commands.get(monitor), Some(&Action::Monitor(Some("display-1".into()))));
        assert_eq!(commands.get(FIRST - 1), None);
        assert_eq!(commands.get(FIRST + 30_001), None);
    }
}
