//! The input method in a terminal: what it is composing (drawn over the cursor, never sent),
//! what it commits (typed), and the guard for Rune #602.
//!
//! #602: Rune keeps its own idea of whether the app's editor is composing, stepped only by
//! `Preedit` and `Commit` (it ignores `Disabled`), and while it thinks a composition is open it
//! eats Return and Escape before any handler sees them. An input method that turns off in the
//! middle of a composition would leave Return dead. So on `Disabled` with a preedit open we
//! finish it ourselves (typed, as Rune's own focus-loss finish commits it), and until Rune's
//! state steps back (the next `Preedit` or `Commit`) the terminal takes Return and Escape
//! itself, before Rune's key routing (`El::on_physical_input`). When Rune later finishes that
//! same composition on its own (the window lost focus), its `Commit` of the text we already
//! typed is dropped. The guard goes when a Rune bump fixes #602.

use std::ops::Range;

/// A composition in progress: drawn over the cursor's cell, not part of what is typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// The input method's caret or selection in `text`, as a byte range.
    pub cursor: Option<(usize, usize)>,
    /// Its clauses, as byte ranges of `text`, and whether each is the one being converted.
    pub clauses: Vec<(Range<usize>, bool)>,
}

impl Preedit {
    /// The clause being converted, if the input method says (else a cursor range that isn't
    /// empty, as Rune reads it).
    pub fn target(&self) -> Option<Range<usize>> {
        if let Some((r, _)) = self.clauses.iter().find(|(_, t)| *t) {
            return Some(r.clone());
        }
        if !self.clauses.is_empty() {
            return None;
        }
        self.cursor.filter(|(a, b)| a < b).map(|(a, b)| a..b)
    }
}

/// The terminal's side of the input method.
#[derive(Debug, Default)]
pub struct ImeState {
    preedit: Option<Preedit>,
    /// We finished a composition Rune still thinks is open (it turned off mid-way): the text we
    /// typed for it. Return and Escape are ours while this is set.
    stale: Option<String>,
}

impl ImeState {
    /// The composition being drawn, if any.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    pub fn composing(&self) -> bool {
        self.preedit.is_some()
    }

    /// Rune's composition state is behind ours: Return and Escape must be taken before Rune's
    /// key routing, or it eats them.
    pub fn guards_keys(&self) -> bool {
        self.stale.is_some()
    }

    /// `Ime::Preedit`: the composition so far (empty ends it). True when what is drawn changed.
    pub fn set_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>, clauses: Vec<(Range<usize>, bool)>) -> bool {
        // Rune stepped its own state with this event: it is in step with us again.
        self.stale = None;
        let next = (!text.is_empty()).then(|| Preedit { text: text.to_owned(), cursor, clauses });
        let changed = next != self.preedit;
        self.preedit = next;
        changed
    }

    /// `Ime::Commit`: the text to type, or `None` when there is nothing to type (empty, or
    /// Rune finishing a composition whose text we already typed when the input method turned
    /// off).
    pub fn commit(&mut self, text: &str) -> Option<String> {
        self.preedit = None;
        if let Some(typed) = self.stale.take() {
            if typed == text {
                return None;
            }
        }
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// `Ime::Disabled`: the input method turned off. A composition still open is finished here
    /// (its text returned, to be typed) and Rune's stale state guarded against.
    pub fn disabled(&mut self) -> Option<String> {
        let open = self.preedit.take()?;
        self.stale = Some(open.text.clone());
        Some(open.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_is_drawn_until_it_commits_and_only_the_commit_is_typed() {
        let mut ime = ImeState::default();
        assert!(ime.set_preedit("か", Some((3, 3)), vec![]));
        assert!(ime.composing());
        assert!(!ime.set_preedit("か", Some((3, 3)), vec![]), "the same preedit again changes nothing");
        assert!(ime.set_preedit("かな", Some((6, 6)), vec![]));
        assert_eq!(ime.commit("仮名"), Some("仮名".into()));
        assert!(!ime.composing());
        assert!(!ime.guards_keys());
    }

    #[test]
    fn an_empty_preedit_ends_the_composition_without_typing() {
        let mut ime = ImeState::default();
        ime.set_preedit("ㅎ", None, vec![]);
        assert!(ime.set_preedit("", None, vec![]));
        assert!(!ime.composing());
        assert_eq!(ime.commit(""), None);
    }

    #[test]
    fn a_dead_key_commit_without_preedit_is_typed() {
        let mut ime = ImeState::default();
        assert_eq!(ime.commit("é"), Some("é".into()));
    }

    /// #602: the input method turns off mid-composition.
    #[test]
    fn turning_off_mid_composition_types_the_preedit_and_guards_return() {
        let mut ime = ImeState::default();
        ime.set_preedit("にほん", None, vec![(0..9, true)]);
        assert_eq!(ime.disabled(), Some("にほん".into()));
        assert!(!ime.composing());
        // Rune still thinks it's composing: Return and Escape are ours.
        assert!(ime.guards_keys());
        // The window loses focus: Rune finishes "its" composition with the same text, which
        // was already typed.
        assert_eq!(ime.commit("にほん"), None);
        assert!(!ime.guards_keys());
    }

    #[test]
    fn the_guard_ends_when_runes_state_steps_again() {
        let mut ime = ImeState::default();
        ime.set_preedit("a", None, vec![]);
        ime.disabled();
        assert!(ime.guards_keys());
        // A new composition: Rune's state moved on with it.
        ime.set_preedit("b", None, vec![]);
        assert!(!ime.guards_keys());
        ime.disabled();
        // A commit of something else: typed, and Rune is in step again.
        assert_eq!(ime.commit("c"), Some("c".into()));
        assert!(!ime.guards_keys());
    }

    #[test]
    fn turning_off_with_nothing_open_does_nothing() {
        let mut ime = ImeState::default();
        assert_eq!(ime.disabled(), None);
        assert!(!ime.guards_keys());
    }

    #[test]
    fn the_target_clause_is_the_one_marked_else_a_cursor_range() {
        let p = Preedit { text: "にほんご".into(), cursor: Some((0, 6)), clauses: vec![(0..6, false), (6..12, true)] };
        assert_eq!(p.target(), Some(6..12));
        let p = Preedit { text: "にほんご".into(), cursor: Some((0, 6)), clauses: vec![] };
        assert_eq!(p.target(), Some(0..6));
        let p = Preedit { text: "にほんご".into(), cursor: Some((6, 6)), clauses: vec![] };
        assert_eq!(p.target(), None);
        let p = Preedit { text: "にほんご".into(), cursor: Some((0, 6)), clauses: vec![(0..12, false)] };
        assert_eq!(p.target(), None);
    }
}
