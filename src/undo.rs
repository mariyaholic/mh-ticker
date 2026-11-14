//! W3 J2: one undo snapshot, held in memory. Pure and portable: `std` only.
//!
//! The last removal, move, sort, pin change or alert clear leaves one snapshot here; the next such
//! action replaces it, `take` empties it (a second undo finds nothing), and it is never written to
//! disk, so a quit forgets it. What the snapshot holds is the caller's `T` (`config::Snapshot`); this
//! module only keeps it and says what it was for.

/// What the held snapshot undoes. Names the menu line (`Undo the last removal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Remove,
    Move,
    Sort,
    Pin,
    AlertClear,
}

impl Kind {
    /// The words of the menu line: `Undo the last <label>`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Remove => "removal",
            Kind::Move => "move",
            Kind::Sort => "sort",
            Kind::Pin => "pin change",
            Kind::AlertClear => "alert clear",
        }
    }

    /// Does this snapshot hold the list of cards (and so go stale when the list changes some other way)?
    /// An alert clear holds alerts only and is merged back, so it never does.
    pub fn is_rows(self) -> bool {
        !matches!(self, Kind::AlertClear)
    }
}

/// The one snapshot.
#[derive(Debug)]
pub struct Undo<T> {
    held: Option<(Kind, T)>,
}

impl<T> Default for Undo<T> {
    fn default() -> Self {
        Undo { held: None }
    }
}

impl<T> Undo<T> {
    /// Hold `snapshot`, dropping whatever was held.
    pub fn put(&mut self, kind: Kind, snapshot: T) {
        self.held = Some((kind, snapshot));
    }

    /// Hand the snapshot over and hold nothing: the second call finds nothing.
    pub fn take(&mut self) -> Option<(Kind, T)> {
        self.held.take()
    }

    /// What is held, for the menu line.
    pub fn kind(&self) -> Option<Kind> {
        self.held.as_ref().map(|(k, _)| *k)
    }

    /// Forget a snapshot of a list of cards: the list was changed in a way undo does not cover (a
    /// symbol added, shown, renamed, forgotten or reset), and putting the old list back would undo
    /// that too. An alert snapshot stays.
    pub fn drop_rows(&mut self) {
        if self.kind().is_some_and(Kind::is_rows) {
            self.held = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_taken_once_and_the_second_undo_finds_nothing() {
        let mut u: Undo<Vec<u8>> = Undo::default();
        assert!(u.take().is_none(), "nothing held at the start");
        u.put(Kind::Remove, vec![1, 2, 3]);
        assert_eq!(u.kind(), Some(Kind::Remove));
        assert_eq!(u.take(), Some((Kind::Remove, vec![1, 2, 3])));
        assert!(u.take().is_none(), "a second undo does nothing");
        assert_eq!(u.kind(), None);
    }

    #[test]
    fn the_next_action_replaces_the_snapshot() {
        let mut u = Undo::default();
        u.put(Kind::Move, "a");
        u.put(Kind::Sort, "b");
        assert_eq!(u.take(), Some((Kind::Sort, "b")));
        assert!(u.take().is_none(), "only one is ever held");
    }

    #[test]
    fn a_change_the_undo_does_not_cover_drops_a_list_snapshot_but_not_an_alert_one() {
        let mut u = Undo::default();
        for kind in [Kind::Remove, Kind::Move, Kind::Sort, Kind::Pin] {
            u.put(kind, 1);
            u.drop_rows();
            assert!(u.take().is_none(), "{kind:?} is stale once the list changed another way");
        }
        u.put(Kind::AlertClear, 2);
        u.drop_rows();
        assert_eq!(u.take(), Some((Kind::AlertClear, 2)));
    }

    #[test]
    fn every_kind_has_words_for_the_menu() {
        for kind in [Kind::Remove, Kind::Move, Kind::Sort, Kind::Pin, Kind::AlertClear] {
            assert!(!kind.label().is_empty());
        }
    }
}
