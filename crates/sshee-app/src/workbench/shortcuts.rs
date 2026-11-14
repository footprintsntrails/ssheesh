//! The window's keyboard shortcuts: which key does what on each system, and matching a key
//! press against them.
//!
//! A port of 0.10's `src/lib/keys.ts` for the shortcuts the 1.0 window has so far: the same
//! ids (so a shortcut changed in 0.10's Settings carries over), the same defaults (⌘ on a
//! Mac, Ctrl+Shift elsewhere, so a shell's Ctrl+letter keys stay the terminal's), and the same
//! rule for layouts. The key is named by the caller as 0.10's `keyOf` names it: the Latin
//! letter the key types (Z is the key marked Z on a German or French keyboard), and on a
//! layout without Latin letters, or for digits and punctuation, the key at that place on a US
//! keyboard. The screen layer turns its key events into that name.
//!
//! New in 1.0, not in 0.10: jumping to a tab by its number (⌘1 to ⌘9 on a Mac, Ctrl+Shift+1 to
//! 9 elsewhere; 9 is the last tab, as in a browser).

use crate::model::Keys;
use indexmap::IndexMap;

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shortcut {
    /// A new terminal on the machine on screen.
    NewTerminal,
    CloseTab,
    /// Brings back the tab closed last.
    ReopenTab,
    ToggleSidebar,
    /// Opens Settings, or goes back to the tab that was on screen.
    Settings,
    /// Shows or hides the machine's vital signs in the status bar.
    ToggleVitals,
    /// Hides server names, addresses and users everywhere, or shows them again.
    StreamerMode,
    /// Ctrl+Tab and Ctrl+Shift+Tab, on a Mac too: +1 or -1.
    StepTab(i32),
    /// The tab at this place in the row, 1 to 9.
    JumpToTab(usize),
}

/// The shortcuts that can be changed in Settings, with 0.10's ids.
const NAMED: [(Shortcut, &str); 7] = [
    (Shortcut::CloseTab, "closePane"),
    (Shortcut::NewTerminal, "newTerminal"),
    (Shortcut::ReopenTab, "reopenTab"),
    (Shortcut::Settings, "settings"),
    (Shortcut::ToggleSidebar, "sidebar"),
    (Shortcut::ToggleVitals, "vitals"),
    (Shortcut::StreamerMode, "streamer"),
];

impl Shortcut {
    /// Its id in 0.10's settings (`shortcuts`), for the ones Settings can change.
    pub fn id(self) -> Option<&'static str> {
        NAMED.iter().find(|(s, _)| *s == self).map(|(_, id)| *id)
    }
}

/// A key press: the key as 0.10's `keyOf` names it (see the module's docs) and what was held.
/// `meta` is ⌘ on a Mac and the Windows or Super key elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pressed<'a> {
    pub key: &'a str,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

fn keys(key: &str, ctrl: bool, meta: bool, shift: bool) -> Keys {
    let on = |b: bool| b.then_some(true);
    Keys { key: key.to_owned(), ctrl: on(ctrl), meta: on(meta), alt: None, shift: on(shift), ..Keys::default() }
}

/// The keys a shortcut comes with: ⌘ (and ⇧) on a Mac, Ctrl+Shift elsewhere.
pub fn default_keys(shortcut: Shortcut, mac: bool) -> Option<Keys> {
    let (key, mac_shift) = match shortcut {
        Shortcut::CloseTab => ("w", false),
        Shortcut::NewTerminal => ("t", false),
        // Elsewhere Ctrl+Shift+T is a new tab, as in Windows Terminal; Z undoes a close.
        Shortcut::ReopenTab => (if mac { "t" } else { "z" }, true),
        Shortcut::ToggleSidebar => ("b", false),
        Shortcut::ToggleVitals => ("j", false),
        Shortcut::StreamerMode => ("h", true),
        // No terminal program reads Ctrl+comma, so elsewhere it needs no Shift.
        Shortcut::Settings => return Some(if mac { keys(",", false, true, false) } else { keys(",", true, false, false) }),
        Shortcut::StepTab(_) | Shortcut::JumpToTab(_) => return None,
    };
    Some(if mac { keys(key, false, true, mac_shift) } else { keys(key, true, false, true) })
}

fn same(k: &Keys, p: &Pressed) -> bool {
    let on = |b: Option<bool>| b.unwrap_or(false);
    k.key == p.key && on(k.ctrl) == p.ctrl && on(k.meta) == p.meta && on(k.alt) == p.alt && on(k.shift) == p.shift
}

/// The shortcut a key press is, if any. `changed` is Settings' `shortcuts`: by id, the keys
/// you gave a shortcut, or `None` for one you turned off.
pub fn match_shortcut(p: &Pressed, mac: bool, changed: &IndexMap<String, Option<Keys>>) -> Option<Shortcut> {
    // Ctrl+Tab and Ctrl+Shift+Tab, as in a browser (on a Mac too).
    if p.key == "tab" && p.ctrl && !p.alt && !p.meta {
        return Some(Shortcut::StepTab(if p.shift { -1 } else { 1 }));
    }
    // Every other shortcut needs Ctrl or ⌘; plain typing never gets further than this.
    if !p.ctrl && !p.meta {
        return None;
    }
    for (shortcut, id) in NAMED {
        let keys = match changed.get(id) {
            Some(Some(k)) if !k.key.is_empty() => Some(k.clone()),
            Some(_) => None,
            None => default_keys(shortcut, mac),
        };
        if keys.is_some_and(|k| same(&k, p)) {
            return Some(shortcut);
        }
    }
    // A tab by its number: the digit key at its place, whatever the layout types there.
    let digit = p.key.parse::<usize>().ok().filter(|n| (1..=9).contains(n) && p.key.len() == 1)?;
    let held = if mac { p.meta && !p.ctrl && !p.shift } else { p.ctrl && p.shift && !p.meta };
    (held && !p.alt).then_some(Shortcut::JumpToTab(digit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press<'a>(key: &'a str, held: &str) -> Pressed<'a> {
        Pressed { key, ctrl: held.contains('c'), meta: held.contains('m'), alt: held.contains('a'), shift: held.contains('s') }
    }

    fn hit(key: &str, held: &str, mac: bool) -> Option<Shortcut> {
        match_shortcut(&press(key, held), mac, &IndexMap::new())
    }

    #[test]
    fn a_mac_uses_command() {
        assert_eq!(hit("t", "m", true), Some(Shortcut::NewTerminal));
        assert_eq!(hit("w", "m", true), Some(Shortcut::CloseTab));
        assert_eq!(hit("t", "ms", true), Some(Shortcut::ReopenTab));
        assert_eq!(hit("b", "m", true), Some(Shortcut::ToggleSidebar));
        assert_eq!(hit(",", "m", true), Some(Shortcut::Settings));
        assert_eq!(hit("j", "m", true), Some(Shortcut::ToggleVitals));
        assert_eq!(hit("h", "ms", true), Some(Shortcut::StreamerMode));
        // ⌘H hides the app on a Mac: streamer mode needs Shift.
        assert_eq!(hit("h", "m", true), None);
        assert_eq!(hit("3", "m", true), Some(Shortcut::JumpToTab(3)));
        assert_eq!(hit("9", "m", true), Some(Shortcut::JumpToTab(9)));
        // Ctrl+letter is the shell's on a Mac too, and ⌘⇧W isn't ⌘W.
        assert_eq!(hit("w", "c", true), None);
        assert_eq!(hit("w", "ms", true), None);
        assert_eq!(hit("t", "ma", true), None);
        assert_eq!(hit("0", "m", true), None);
    }

    #[test]
    fn elsewhere_it_is_ctrl_shift_so_the_shell_keeps_ctrl_letters() {
        assert_eq!(hit("t", "cs", false), Some(Shortcut::NewTerminal));
        assert_eq!(hit("w", "cs", false), Some(Shortcut::CloseTab));
        assert_eq!(hit("z", "cs", false), Some(Shortcut::ReopenTab));
        assert_eq!(hit("b", "cs", false), Some(Shortcut::ToggleSidebar));
        assert_eq!(hit(",", "c", false), Some(Shortcut::Settings));
        assert_eq!(hit(",", "cs", false), None);
        assert_eq!(hit("j", "cs", false), Some(Shortcut::ToggleVitals));
        assert_eq!(hit("h", "cs", false), Some(Shortcut::StreamerMode));
        assert_eq!(hit("1", "cs", false), Some(Shortcut::JumpToTab(1)));
        // Ctrl+W deletes a word, Ctrl+T swaps letters, Ctrl+Z suspends, Ctrl+B goes back:
        // all the terminal's.
        for key in ["w", "t", "z", "b", "c", "d", "j", "h", "1"] {
            assert_eq!(hit(key, "c", false), None, "Ctrl+{key}");
        }
        // The Windows key isn't Ctrl, and Alt changes the chord.
        assert_eq!(hit("t", "ms", false), None);
        assert_eq!(hit("t", "csa", false), None);
        assert_eq!(hit("1", "csa", false), None);
    }

    #[test]
    fn ctrl_tab_steps_on_every_system() {
        for mac in [true, false] {
            assert_eq!(hit("tab", "c", mac), Some(Shortcut::StepTab(1)));
            assert_eq!(hit("tab", "cs", mac), Some(Shortcut::StepTab(-1)));
            // Plain Tab completes in the shell; Alt+Tab and ⌘Tab are the system's.
            assert_eq!(hit("tab", "", mac), None);
            assert_eq!(hit("tab", "ca", mac), None);
            assert_eq!(hit("tab", "cm", mac), None);
        }
    }

    #[test]
    fn plain_typing_is_never_a_shortcut() {
        for key in ["t", "w", "1", "enter", "z"] {
            assert_eq!(hit(key, "", false), None);
            assert_eq!(hit(key, "s", true), None);
            assert_eq!(hit(key, "a", false), None);
        }
    }

    #[test]
    fn a_shortcut_changed_or_turned_off_in_settings_is_honoured() {
        let mut changed = IndexMap::new();
        changed.insert("newTerminal".to_owned(), Some(keys("n", true, false, false)));
        changed.insert("closePane".to_owned(), None);
        let m = |key: &str, held: &str| match_shortcut(&press(key, held), false, &changed);
        assert_eq!(m("n", "c"), Some(Shortcut::NewTerminal));
        assert_eq!(m("t", "cs"), None);
        assert_eq!(m("w", "cs"), None);
        // The rest keep their own keys.
        assert_eq!(m("z", "cs"), Some(Shortcut::ReopenTab));
        // Keys saved without a key name turn it off rather than match everything.
        changed.insert("sidebar".to_owned(), Some(Keys::default()));
        assert_eq!(match_shortcut(&press("b", "cs"), false, &changed), None);
    }

    #[test]
    fn ids_are_0_10s() {
        assert_eq!(Shortcut::CloseTab.id(), Some("closePane"));
        assert_eq!(Shortcut::NewTerminal.id(), Some("newTerminal"));
        assert_eq!(Shortcut::ReopenTab.id(), Some("reopenTab"));
        assert_eq!(Shortcut::ToggleSidebar.id(), Some("sidebar"));
        assert_eq!(Shortcut::Settings.id(), Some("settings"));
        assert_eq!(Shortcut::ToggleVitals.id(), Some("vitals"));
        assert_eq!(Shortcut::StreamerMode.id(), Some("streamer"));
        assert_eq!(Shortcut::StepTab(1).id(), None);
        assert_eq!(default_keys(Shortcut::ReopenTab, true), Some(keys("t", false, true, true)));
        assert_eq!(default_keys(Shortcut::ReopenTab, false), Some(keys("z", true, false, true)));
        assert_eq!(default_keys(Shortcut::JumpToTab(1), true), None);
    }
}
