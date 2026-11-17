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
//! Files took Ctrl+Shift+E off a Mac in the first 1.0 builds; 0.10's Split down has it back,
//! and Files is Ctrl+Shift+O there (⇧⌘E on a Mac, where Split down is ⇧⌘D).
//!
//! New in 1.0, not in 0.10: jumping to a tab by its number (⌘1 to ⌘9 on a Mac, Ctrl+Shift+1 to
//! 9 elsewhere; 9 is the last tab, as in a browser), the list of agents, and Files. Files took
//! Ctrl+Shift+E off a Mac in the first 1.0 builds; 0.10's Split down has it back, and Files is
//! Ctrl+Shift+O there (⇧⌘E on a Mac, where Split down is ⇧⌘D).
//!
//! **Keys you gave a shortcut win over a shortcut's own.** A shortcut 1.0 added comes with keys
//! a shortcut changed in 0.10 may already have (your own Ctrl+Shift+A, before the agents had
//! it): those stay yours, and the new one has no keys until you give it some.

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
    /// Files on the machine on screen (new in 1.0: 0.10 opened Files from menus only).
    OpenFiles,
    /// Opens Settings, or goes back to the tab that was on screen.
    Settings,
    /// Shows or hides the machine's vital signs in the status bar.
    ToggleVitals,
    /// Ctrl+Tab and Ctrl+Shift+Tab, on a Mac too: +1 or -1.
    StepTab(i32),
    /// The tab at this place in the row, 1 to 9.
    JumpToTab(usize),
    /// Find in the terminal. `always`: pressed in a way no program in a terminal reads (⌘F,
    /// Ctrl+Shift+F), so the find bar opens whatever runs there. Ctrl+F alone is also vim's
    /// and less's "page down": the terminal decides who gets it (`sshee_term::find::takes_find_key`).
    Find { always: bool },
    /// Streamer mode: names, addresses and secrets hidden everywhere, or shown again.
    Streamer,
    /// Two tabs side by side, or one at a time.
    SideBySide,
    /// A new terminal beside the tab on screen, to its right, or under it.
    SplitRight,
    SplitDown,
    /// One pane of a split fills the window, or all of them show again.
    Zoom,
    /// Compact mode: the bars slide in at the window's edge, or stay.
    Compact,
    /// The command palette; `PaletteAlt` is its second shortcut off a Mac (0.10's `paletteAlt`).
    Palette,
    PaletteAlt,
    /// "Add a server".
    AddServer,
    /// The list of agents on every server, the waiting ones first (new in 1.0).
    Agents,
}

/// The shortcuts that can be changed in Settings, with 0.10's ids.
const NAMED: [(Shortcut, &str); 18] = [
    (Shortcut::Agents, "agents"),
    (Shortcut::CloseTab, "closePane"),
    (Shortcut::NewTerminal, "newTerminal"),
    (Shortcut::ReopenTab, "reopenTab"),
    (Shortcut::Settings, "settings"),
    (Shortcut::ToggleSidebar, "sidebar"),
    (Shortcut::OpenFiles, "files"),
    (Shortcut::ToggleVitals, "vitals"),
    (Shortcut::Find { always: false }, "find"),
    (Shortcut::Streamer, "streamer"),
    (Shortcut::SideBySide, "toggleSplit"),
    (Shortcut::SplitRight, "splitRight"),
    (Shortcut::SplitDown, "splitDown"),
    (Shortcut::Zoom, "zoomPane"),
    (Shortcut::Compact, "compact"),
    (Shortcut::Palette, "palette"),
    (Shortcut::PaletteAlt, "paletteAlt"),
    (Shortcut::AddServer, "addHost"),
];

/// The id of "a tab by its number": one shortcut in Settings, whose keys are those held with
/// a digit (stored with the digit 1).
pub const JUMP_TO_TAB: &str = "jumpToTab";

impl Shortcut {
    /// Its id in 0.10's settings (`shortcuts`), for the ones Settings can change.
    pub fn id(self) -> Option<&'static str> {
        // However find was pressed, it's one shortcut in Settings; so are the nine tabs.
        let this = match self {
            Shortcut::Find { .. } => Shortcut::Find { always: false },
            Shortcut::JumpToTab(_) => return Some(JUMP_TO_TAB),
            other => other,
        };
        NAMED.iter().find(|(s, _)| *s == this).map(|(_, id)| *id)
    }
}

/// A digit key that picks a tab: "1" to "9".
pub fn tab_digit(key: &str) -> Option<usize> {
    key.parse::<usize>().ok().filter(|n| (1..=9).contains(n) && key.len() == 1)
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
        // No terminal program reads Ctrl+comma, so elsewhere it needs no Shift.
        Shortcut::Settings => return Some(if mac { keys(",", false, true, false) } else { keys(",", true, false, false) }),
        // As in a browser: ⌘F, and Ctrl+F without Shift elsewhere.
        Shortcut::Find { .. } => return Some(if mac { keys("f", false, true, false) } else { keys("f", true, false, false) }),
        Shortcut::Streamer => ("h", true),
        Shortcut::SideBySide => ("s", true),
        // 0.10's: ⌘D and ⇧⌘D on a Mac, Ctrl+Shift+D and Ctrl+Shift+E elsewhere.
        Shortcut::SplitRight => ("d", false),
        Shortcut::SplitDown => (if mac { "d" } else { "e" }, true),
        Shortcut::Zoom => ("enter", true),
        Shortcut::Compact => ("m", true),
        // ⌘K clears the terminal on a Mac, so the palette has one shortcut there.
        Shortcut::Palette => ("l", false),
        Shortcut::PaletteAlt if mac => return None,
        Shortcut::PaletteAlt => ("k", false),
        Shortcut::AddServer => ("n", true),
        // ⌘A selects all on a Mac, so the agents take ⌘⇧A there.
        Shortcut::Agents => ("a", true),
        // Like a file explorer: ⇧⌘E on a Mac (F is Find); elsewhere E is 0.10's Split down.
        Shortcut::OpenFiles => (if mac { "e" } else { "o" }, true),
        // ⌘1 on a Mac, Ctrl+Shift+1 elsewhere: the digit stands for all nine.
        Shortcut::JumpToTab(_) => ("1", false),
        Shortcut::StepTab(_) => return None,
    };
    Some(if mac { keys(key, false, true, mac_shift) } else { keys(key, true, false, true) })
}

/// Whether two chords are the same keys. With `digits`, a digit stands for all nine.
fn clash(a: &Keys, b: &Keys, digits: bool) -> bool {
    let on = |b: Option<bool>| b.unwrap_or(false);
    let mods = |k: &Keys| (on(k.ctrl), on(k.meta), on(k.alt), on(k.shift));
    let keys = if digits { tab_digit(&a.key).is_some() && tab_digit(&b.key).is_some() } else { a.key == b.key };
    keys && mods(a) == mods(b)
}

/// Whether `keys`, a shortcut's own, are already yours for another one: a shortcut changed in
/// Settings to these keys, or to one of the digits when `keys` are "a tab by its number".
/// `id` is the shortcut's own id (its own entry doesn't count).
pub fn taken_by_yours(id: &str, keys: &Keys, changed: &IndexMap<String, Option<Keys>>) -> bool {
    changed.iter().any(|(other, mine)| {
        other != id && mine.as_ref().is_some_and(|mine| !mine.key.is_empty() && clash(keys, mine, id == JUMP_TO_TAB || other == JUMP_TO_TAB))
    })
}

/// The keys a shortcut has now: yours from Settings, else the ones it comes with unless you
/// gave those to another shortcut. `None` for one turned off, or with no keys.
pub fn keys_now(shortcut: Shortcut, mac: bool, changed: &IndexMap<String, Option<Keys>>) -> Option<Keys> {
    let id = shortcut.id();
    match id.and_then(|id| changed.get(id)) {
        Some(Some(k)) if !k.key.is_empty() => Some(k.clone()),
        Some(_) => None,
        None => default_keys(shortcut, mac).filter(|own| !id.is_some_and(|id| taken_by_yours(id, own, changed))),
    }
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
    let pressed = Keys {
        key: p.key.to_owned(),
        ctrl: p.ctrl.then_some(true),
        meta: p.meta.then_some(true),
        alt: p.alt.then_some(true),
        shift: p.shift.then_some(true),
        ..Keys::default()
    };
    let tab = tab_digit(p.key).map(Shortcut::JumpToTab);
    let mut candidates = NAMED.iter().map(|(shortcut, _)| *shortcut).chain(tab);
    let found = candidates.find(|s| keys_now(*s, mac, changed).is_some_and(|k| clash(&k, &pressed, matches!(s, Shortcut::JumpToTab(_)))));
    if let Some(shortcut) = found {
        return Some(match shortcut {
            // ⌘F is nobody else's; a bare Ctrl+letter is also a program's key.
            Shortcut::Find { .. } => Shortcut::Find { always: p.meta || p.shift || p.alt },
            other => other,
        });
    }
    // Find with Shift, off a Mac: the chord no program reads, so it always opens the bar.
    if !mac && p.key == "f" && p.ctrl && p.shift && !p.alt && !p.meta && !matches!(changed.get("find"), Some(None)) {
        return Some(Shortcut::Find { always: true });
    }
    None
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
        assert_eq!(hit("h", "ms", true), Some(Shortcut::Streamer));
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
        assert_eq!(hit("a", "cs", false), Some(Shortcut::Agents));
        assert_eq!(hit("a", "ms", true), Some(Shortcut::Agents));
        // ⌘A stays "select all" on a Mac, and Ctrl+A the start of the line everywhere.
        assert_eq!((hit("a", "m", true), hit("a", "c", false), hit("a", "c", true)), (None, None, None));
        assert_eq!(hit(",", "c", false), Some(Shortcut::Settings));
        assert_eq!(hit(",", "cs", false), None);
        assert_eq!(hit("j", "cs", false), Some(Shortcut::ToggleVitals));
        assert_eq!(hit("h", "cs", false), Some(Shortcut::Streamer));
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
        assert_eq!(Shortcut::Streamer.id(), Some("streamer"));
        assert_eq!(Shortcut::StepTab(1).id(), None);
        assert_eq!(default_keys(Shortcut::ReopenTab, true), Some(keys("t", false, true, true)));
        assert_eq!(default_keys(Shortcut::ReopenTab, false), Some(keys("z", true, false, true)));
        assert_eq!(default_keys(Shortcut::JumpToTab(1), true), Some(keys("1", false, true, false)));
        assert_eq!(default_keys(Shortcut::JumpToTab(7), false), Some(keys("1", true, false, true)));
        assert_eq!((Shortcut::JumpToTab(3).id(), Shortcut::Agents.id()), (Some(JUMP_TO_TAB), Some("agents")));
        assert_eq!(Shortcut::Find { always: true }.id(), Some("find"));
        assert_eq!(Shortcut::Streamer.id(), Some("streamer"));
        assert_eq!(Shortcut::SideBySide.id(), Some("toggleSplit"));
        assert_eq!((Shortcut::SplitRight.id(), Shortcut::SplitDown.id(), Shortcut::Zoom.id()), (Some("splitRight"), Some("splitDown"), Some("zoomPane")));
    }

    #[test]
    fn splits_and_filling_the_window_keep_0_10s_keys() {
        assert_eq!(hit("d", "m", true), Some(Shortcut::SplitRight));
        assert_eq!(hit("d", "ms", true), Some(Shortcut::SplitDown));
        assert_eq!(hit("enter", "ms", true), Some(Shortcut::Zoom));
        assert_eq!(hit("d", "cs", false), Some(Shortcut::SplitRight));
        assert_eq!(hit("e", "cs", false), Some(Shortcut::SplitDown));
        assert_eq!(hit("enter", "cs", false), Some(Shortcut::Zoom));
        // Ctrl+D ends a shell, Ctrl+E goes to the line's end, Enter is Enter: the terminal's.
        for key in ["d", "e", "enter"] {
            assert_eq!(hit(key, "c", false), None, "Ctrl+{key}");
            assert_eq!(hit(key, "c", true), None, "Ctrl+{key} on a Mac");
        }
        assert_eq!(hit("enter", "m", true), None, "⌘↩ alone");
        assert_eq!((hit("m", "ms", true), hit("m", "cs", false), hit("m", "c", false)), (Some(Shortcut::Compact), Some(Shortcut::Compact), None));
    }

    #[test]
    fn find_streamer_mode_and_side_by_side_keep_0_10s_keys() {
        // A Mac: ⌘F, ⌘⇧H, ⌘⇧S.
        assert_eq!(hit("f", "m", true), Some(Shortcut::Find { always: true }));
        assert_eq!(hit("h", "ms", true), Some(Shortcut::Streamer));
        assert_eq!(hit("s", "ms", true), Some(Shortcut::SideBySide));
        // ⌘H hides the app and ⌘S is nothing here; Ctrl+F is the shell's on a Mac.
        assert_eq!(hit("h", "m", true), None);
        assert_eq!(hit("s", "m", true), None);
        assert_eq!(hit("f", "c", true), None);
        // Elsewhere: Ctrl+F as in a browser (the terminal may leave it to vim or less), and
        // Ctrl+Shift+F, which no program reads.
        assert_eq!(hit("f", "c", false), Some(Shortcut::Find { always: false }));
        assert_eq!(hit("f", "cs", false), Some(Shortcut::Find { always: true }));
        assert_eq!(hit("h", "cs", false), Some(Shortcut::Streamer));
        assert_eq!(hit("s", "cs", false), Some(Shortcut::SideBySide));
        // Ctrl+H is Backspace and Ctrl+S stops output: the terminal's.
        assert_eq!(hit("h", "c", false), None);
        assert_eq!(hit("s", "c", false), None);
        // Find turned off in Settings is off with Shift too.
        let mut changed = IndexMap::new();
        changed.insert("find".to_owned(), None);
        assert_eq!(match_shortcut(&press("f", "c"), false, &changed), None);
        assert_eq!(match_shortcut(&press("f", "cs"), false, &changed), None);
    }

    #[test]
    fn the_palette_and_add_a_server_keep_0_10s_keys() {
        assert_eq!(hit("l", "cs", false), Some(Shortcut::Palette));
        assert_eq!(hit("k", "cs", false), Some(Shortcut::PaletteAlt));
        assert_eq!(hit("n", "cs", false), Some(Shortcut::AddServer));
        // Files, as a file explorer: ⇧⌘E; F stays Find's, and elsewhere E is Split down's.
        assert_eq!(hit("o", "cs", false), Some(Shortcut::OpenFiles));
        assert_eq!(hit("e", "ms", true), Some(Shortcut::OpenFiles));
        assert_eq!(Shortcut::OpenFiles.id(), Some("files"));
        assert_eq!(hit("l", "m", true), Some(Shortcut::Palette));
        assert_eq!(hit("n", "ms", true), Some(Shortcut::AddServer));
        // ⌘K clears the terminal on a Mac; Ctrl+L and Ctrl+K are the shell's everywhere.
        assert_eq!(hit("k", "m", true), None);
        assert_eq!(hit("l", "c", false), None);
        assert_eq!(hit("k", "c", true), None);
        assert_eq!((Shortcut::Palette.id(), Shortcut::PaletteAlt.id(), Shortcut::AddServer.id()), (Some("palette"), Some("paletteAlt"), Some("addHost")));
    }

    #[test]
    fn keys_you_gave_a_shortcut_win_over_a_new_shortcuts_own() {
        // Your own Ctrl+Shift+A, from before the agents had it: it stays yours.
        let mut changed = IndexMap::new();
        changed.insert("sidebar".to_owned(), Some(keys("a", true, false, true)));
        assert_eq!(match_shortcut(&press("a", "cs"), false, &changed), Some(Shortcut::ToggleSidebar));
        assert_eq!(keys_now(Shortcut::Agents, false, &changed), None);
        // Its own keys for the sidebar are free, and nothing else changes.
        assert_eq!(match_shortcut(&press("b", "cs"), false, &changed), None);
        assert_eq!(match_shortcut(&press("t", "cs"), false, &changed), Some(Shortcut::NewTerminal));
        // Give the agents keys of their own and they have them.
        changed.insert("agents".to_owned(), Some(keys("g", true, false, true)));
        assert_eq!(match_shortcut(&press("g", "cs"), false, &changed), Some(Shortcut::Agents));
        // A digit you took is yours; then the tabs by number have no keys until you give them some.
        let mut changed = IndexMap::new();
        changed.insert("vitals".to_owned(), Some(keys("3", true, false, true)));
        assert_eq!(match_shortcut(&press("3", "cs"), false, &changed), Some(Shortcut::ToggleVitals));
        assert_eq!(match_shortcut(&press("4", "cs"), false, &changed), None);
        changed.insert(JUMP_TO_TAB.to_owned(), Some(keys("1", false, true, false)));
        assert_eq!(match_shortcut(&press("4", "m"), false, &changed), Some(Shortcut::JumpToTab(4)));
    }

    #[test]
    fn tabs_by_number_take_the_keys_held_with_any_digit() {
        let mut changed = IndexMap::new();
        // Settings keeps them with the digit the keys were recorded with.
        changed.insert(JUMP_TO_TAB.to_owned(), Some(keys("5", false, true, true)));
        let m = |key: &str, held: &str, changed: &IndexMap<String, Option<Keys>>| match_shortcut(&press(key, held), true, changed);
        assert_eq!(m("2", "ms", &changed), Some(Shortcut::JumpToTab(2)));
        assert_eq!(m("9", "ms", &changed), Some(Shortcut::JumpToTab(9)));
        assert_eq!(m("2", "m", &changed), None);
        // Not 0, and not a letter.
        assert_eq!(m("0", "ms", &changed), None);
        assert_eq!(m("q", "ms", &changed), None);
        // Turned off.
        changed.insert(JUMP_TO_TAB.to_owned(), None);
        assert_eq!(m("2", "m", &changed), None);
    }
}
