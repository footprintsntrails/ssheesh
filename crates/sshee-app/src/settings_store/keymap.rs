//! Every keyboard shortcut Settings lists, and the rules for changing one: a port of the rest
//! of 0.10's `src/lib/keys.ts` (the part `workbench::shortcuts` doesn't need to match a key
//! press). The same ids, names, groups and defaults, so shortcuts changed in 0.10 carry over,
//! and the same refusals in the same words.
//!
//! The rule that shapes all of it: shortcuts use ⌘ on a Mac and Ctrl+Shift elsewhere, so a
//! shell's Ctrl+letter keys, Alt and AltGr stay the terminal's. [`reserved_reason`] says so
//! to whoever tries to take one.

use crate::model::Keys;
use crate::workbench::shortcuts::{self as rules, Shortcut, JUMP_TO_TAB};
use indexmap::IndexMap;

/// Shortcuts changed in Settings: by id, the keys given, or `None` for one turned off.
pub type Changed = IndexMap<String, Option<Keys>>;

/// A shortcut's own keys on one system: the key, and how Shift comes into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Combo {
    key: &'static str,
    /// On a Mac: Shift is held too. Elsewhere Shift is the rule and `Some(false)` drops it.
    shift: Option<bool>,
}

const fn k(key: &'static str) -> Option<Combo> {
    Some(Combo { key, shift: None })
}

const fn ks(key: &'static str, shift: bool) -> Option<Combo> {
    Some(Combo { key, shift: Some(shift) })
}

/// One shortcut as Settings lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Its id in 0.10's settings (`shortcuts`).
    pub id: &'static str,
    /// Its name, in plain words.
    pub name: &'static str,
    mac: Option<Combo>,
    other: Option<Combo>,
    /// What the 1.0 window does on it, once it does anything.
    pub does: Option<Shortcut>,
}

/// Every shortcut that can be changed: 0.10's, in its order, then the three 1.0 added.
pub const SHORTCUTS: [Entry; 19] = [
    // The command bar sits on L, like a browser's address bar. On a Mac ⌘K clears the
    // terminal, so the bar has only ⌘L; elsewhere Ctrl+Shift+K opens it too, as it always did.
    Entry { id: "palette", name: "Command bar", mac: k("l"), other: k("l"), does: Some(Shortcut::Palette) },
    Entry { id: "paletteAlt", name: "Command bar", mac: None, other: k("k"), does: Some(Shortcut::PaletteAlt) },
    Entry { id: "splitRight", name: "Split right", mac: k("d"), other: k("d"), does: Some(Shortcut::SplitRight) },
    Entry { id: "splitDown", name: "Split down", mac: ks("d", true), other: k("e"), does: Some(Shortcut::SplitDown) },
    Entry { id: "closePane", name: "Close tab", mac: k("w"), other: k("w"), does: Some(Shortcut::CloseTab) },
    Entry { id: "newTerminal", name: "New terminal", mac: k("t"), other: k("t"), does: Some(Shortcut::NewTerminal) },
    // Elsewhere Ctrl+Shift+T is a new tab, as in Windows Terminal; Z undoes a close.
    Entry { id: "reopenTab", name: "Reopen closed tab", mac: ks("t", true), other: k("z"), does: Some(Shortcut::ReopenTab) },
    Entry { id: "toggleSplit", name: "Tabs side by side, or one at a time", mac: ks("s", true), other: k("s"), does: Some(Shortcut::SideBySide) },
    Entry { id: "addHost", name: "Add a server", mac: ks("n", true), other: k("n"), does: Some(Shortcut::AddServer) },
    Entry { id: "settings", name: "Settings", mac: k(","), other: ks(",", false), does: Some(Shortcut::Settings) },
    Entry { id: "sidebar", name: "Show or hide the sidebar", mac: k("b"), other: k("b"), does: Some(Shortcut::ToggleSidebar) },
    Entry { id: "vitals", name: "Show or hide server vitals", mac: k("j"), other: k("j"), does: Some(Shortcut::ToggleVitals) },
    Entry { id: "zoomPane", name: "Fill the window with a pane", mac: ks("enter", true), other: k("enter"), does: Some(Shortcut::Zoom) },
    Entry { id: "streamer", name: "Streamer mode", mac: ks("h", true), other: k("h"), does: Some(Shortcut::Streamer) },
    Entry { id: "compact", name: "Compact mode", mac: ks("m", true), other: k("m"), does: Some(Shortcut::Compact) },
    // As in a browser. In 0.10, vim and less keep it.
    Entry { id: "find", name: "Find in the terminal", mac: k("f"), other: ks("f", false), does: Some(Shortcut::Find { always: false }) },
    // New in 1.0. ⌘A selects all on a Mac, so the agents take ⇧⌘A there.
    Entry { id: "agents", name: "Agents at work", mac: ks("a", true), other: k("a"), does: Some(Shortcut::Agents) },
    // Like a file explorer: ⇧⌘E on a Mac; elsewhere Ctrl+Shift+E is 0.10's Split down, so O.
    Entry { id: "files", name: "Files on this server", mac: ks("e", true), other: k("o"), does: Some(Shortcut::OpenFiles) },
    // The digit stands for all nine: ⌘1 to ⌘9, Ctrl+Shift+1 to 9; 9 is the last tab.
    Entry { id: JUMP_TO_TAB, name: "Go to a tab by its number", mac: k("1"), other: k("1"), does: Some(Shortcut::JumpToTab(1)) },
];

/// How Settings groups them. The command bar's second key shows on its row.
pub const GROUPS: [(&str, &[&str]); 3] = [
    ("Find and open", &["palette", "find", "addHost", "settings", "files", "agents"]),
    ("Tabs and panes", &["newTerminal", "closePane", "reopenTab", JUMP_TO_TAB, "splitRight", "splitDown", "zoomPane", "toggleSplit"]),
    ("Show and hide", &["sidebar", "vitals", "compact", "streamer"]),
];

/// The shortcut with this id.
pub fn entry(id: &str) -> Option<&'static Entry> {
    SHORTCUTS.iter().find(|e| e.id == id)
}

/// Keys with only the modifiers that are held, so equal chords are equal values.
pub fn keys(key: &str, ctrl: bool, meta: bool, alt: bool, shift: bool) -> Keys {
    let on = |b: bool| b.then_some(true);
    Keys { key: key.to_owned(), ctrl: on(ctrl), meta: on(meta), alt: on(alt), shift: on(shift), ..Keys::default() }
}

fn held(k: &Keys) -> (bool, bool, bool, bool) {
    let on = |b: Option<bool>| b.unwrap_or(false);
    (on(k.ctrl), on(k.meta), on(k.alt), on(k.shift))
}

/// Whether two chords are the same keys.
pub fn same_keys(a: &Keys, b: &Keys) -> bool {
    a.key == b.key && held(a) == held(b)
}

/// The keys a shortcut comes with: ⌘ (and ⇧) on a Mac, Ctrl+Shift (or plain Ctrl) elsewhere.
pub fn default_keys(id: &str, mac: bool) -> Option<Keys> {
    let e = entry(id)?;
    let c = if mac { e.mac } else { e.other }?;
    Some(if mac { keys(c.key, false, true, false, c.shift.unwrap_or(false)) } else { keys(c.key, true, false, false, c.shift.unwrap_or(true)) })
}

/// The keys a shortcut has now: yours if you changed them, else its own unless you gave those
/// to another shortcut (one 1.0 added, whose keys you had taken before it came); `None` when
/// it has none (turned off, or saved without a key).
pub fn current_keys(id: &str, mac: bool, changed: &Changed) -> Option<Keys> {
    match changed.get(id) {
        Some(Some(mine)) if !mine.key.is_empty() => {
            let (ctrl, meta, alt, shift) = held(mine);
            Some(keys(&mine.key, ctrl, meta, alt, shift))
        }
        Some(_) => None,
        None => default_keys(id, mac).filter(|own| !rules::taken_by_yours(id, own, changed)),
    }
}

/// A key's name as it is shown, for the keys that aren't a single character: on a Mac, and
/// elsewhere.
const KEY_NAMES: [(&str, &str, &str); 15] = [
    ("enter", "↩", "Enter"),
    ("tab", "⇥", "Tab"),
    ("space", "Space", "Space"),
    ("backspace", "⌫", "Backspace"),
    ("delete", "⌦", "Delete"),
    ("escape", "Esc", "Esc"),
    ("insert", "Insert", "Insert"),
    ("home", "Home", "Home"),
    ("end", "End", "End"),
    ("pageup", "Page Up", "PgUp"),
    ("pagedown", "Page Down", "PgDn"),
    ("up", "↑", "Up"),
    ("down", "↓", "Down"),
    ("left", "←", "Left"),
    ("right", "→", "Right"),
];

/// The system the keys are shown for: it names the modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    Mac,
    Windows,
    Other,
}

impl Keyboard {
    /// A Mac's keys, or this computer's when they aren't (a window can be told to use a Mac's
    /// keys elsewhere, as tests and demos do).
    pub fn of(mac: bool) -> Keyboard {
        match Keyboard::current() {
            _ if mac => Keyboard::Mac,
            Keyboard::Mac => Keyboard::Other,
            other => other,
        }
    }

    /// This computer's.
    pub fn current() -> Keyboard {
        if cfg!(target_os = "macos") {
            Keyboard::Mac
        } else if cfg!(windows) {
            Keyboard::Windows
        } else {
            Keyboard::Other
        }
    }

    pub fn mac(self) -> bool {
        self == Keyboard::Mac
    }
}

/// A chord as the app shows it: "⌘K", "⇧⌘D" on a Mac; "Ctrl+Shift+K" elsewhere. With an empty
/// key it is the modifiers held so far, while a shortcut is being recorded: "⇧⌘", "Ctrl+Shift+".
pub fn keys_label(k: &Keys, on: Keyboard) -> String {
    let (ctrl, meta, alt, shift) = held(k);
    let name = KEY_NAMES
        .iter()
        .find(|(key, _, _)| *key == k.key)
        .map_or_else(|| k.key.to_uppercase(), |(_, mac, other)| (if on.mac() { *mac } else { *other }).to_owned());
    if on.mac() {
        return format!("{}{}{}{}{name}", if ctrl { "⌃" } else { "" }, if alt { "⌥" } else { "" }, if shift { "⇧" } else { "" }, if meta { "⌘" } else { "" });
    }
    let mut parts = vec![];
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if meta {
        parts.push(if on == Keyboard::Windows { "Win" } else { "Super" });
    }
    parts.push(&name);
    parts.join("+")
}

/// A shortcut's keys as the app shows them; empty when it has none. A tab by its number
/// reads "⌘1…9".
pub fn shortcut_label(id: &str, on: Keyboard, changed: &Changed) -> String {
    current_keys(id, on.mac(), changed).map(|k| label_of(id, &k, on)).unwrap_or_default()
}

/// `keys` as `id`'s: "Ctrl+Shift+1…9" for a tab by its number, else as [`keys_label`] says.
pub fn label_of(id: &str, k: &Keys, on: Keyboard) -> String {
    if id == JUMP_TO_TAB && rules::tab_digit(&k.key).is_some() {
        let (ctrl, meta, alt, shift) = held(k);
        return format!("{}…9", keys_label(&keys("1", ctrl, meta, alt, shift), on));
    }
    keys_label(k, on)
}

/// Whether `a` (keys for `a_id`) and `b` (for `b_id`) are the same keys: a tab by its number
/// has all nine digits.
fn clash(a_id: &str, a: &Keys, b_id: &str, b: &Keys) -> bool {
    if a_id == JUMP_TO_TAB || b_id == JUMP_TO_TAB {
        return held(a) == held(b) && rules::tab_digit(&a.key).is_some() && rules::tab_digit(&b.key).is_some();
    }
    same_keys(a, b)
}

/// Plain Ctrl without Shift is safe with these: terminal programs don't read them.
const CTRL_FREE: [&str; 5] = [",", ".", ";", "'", "="];

const MAC_TAKEN: [(&str, &str); 10] = [
    ("⌘K", "⌘K clears the terminal, as in other Mac terminals."),
    ("⌘C", "⌘C copies."),
    ("⌘V", "⌘V pastes."),
    ("⌘A", "⌘A selects all the text."),
    ("⌘X", "⌘X cuts text."),
    ("⌘Z", "⌘Z undoes typing."),
    ("⇧⌘Z", "⇧⌘Z redoes typing."),
    ("⌘Q", "⌘Q quits SSHeesh."),
    ("⌘H", "⌘H hides SSHeesh, as in every Mac app."),
    ("⌘M", "⌘M minimizes the window, as in every Mac app."),
];
const MAC_SYSTEM: [&str; 12] = ["⌥⌘H", "⌘Space", "⌃⌘Space", "⌥⌘Space", "⌘`", "⇧⌘`", "⌃⌘F", "⌃⌘Q", "⇧⌘3", "⇧⌘4", "⇧⌘5", "⌥⌘D"];

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('f').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Why these keys can't be a shortcut, in one plain sentence, or `None` if they can. The
/// terminal keeps what programs in it read; the system keeps its own.
pub fn reserved_reason(k: &Keys, on: Keyboard) -> Option<String> {
    let name = keys_label(k, on);
    let (ctrl, meta, alt, shift) = held(k);
    if k.key == "escape" {
        return Some("Esc is kept for closing things.".into());
    }
    if on.mac() {
        if !meta {
            let why = if ctrl {
                "Terminal programs use ⌃ with a key themselves, so hold ⌘ instead."
            } else if alt {
                "⌥ types accented letters and symbols, so hold ⌘ instead."
            } else {
                "Hold ⌘ with a key, so plain typing still reaches the terminal."
            };
            return Some(why.into());
        }
        if k.key == "tab" || MAC_SYSTEM.contains(&name.as_str()) {
            return Some(format!("macOS uses {name} itself."));
        }
        return MAC_TAKEN.iter().find(|(keys, _)| *keys == name).map(|(_, why)| (*why).to_owned());
    }
    if meta {
        return Some(if on == Keyboard::Windows { "The Windows key belongs to the system." } else { "The Super key belongs to the system." }.into());
    }
    if alt {
        return Some("Alt and AltGr stay with the terminal and your keyboard layout, so use Ctrl+Shift instead.".into());
    }
    if !ctrl {
        return Some("Hold Ctrl and Shift with a key, so plain typing still reaches the terminal.".into());
    }
    if k.key == "tab" {
        return Some("Ctrl+Tab and Ctrl+Shift+Tab switch tabs.".into());
    }
    if shift && (k.key == "c" || k.key == "v") {
        return Some("Ctrl+Shift+C and Ctrl+Shift+V copy and paste in terminals.".into());
    }
    if !shift && !CTRL_FREE.contains(&k.key.as_str()) && !is_function_key(&k.key) {
        return Some(format!("Terminal programs use {name} themselves, so add Shift."));
    }
    None
}

/// Why `id` can't have these keys, or `None` if it can: the terminal or the system keeps
/// them, or another shortcut already has them.
pub fn problem(id: &str, k: &Keys, on: Keyboard, changed: &Changed) -> Option<String> {
    // A tab by its number is keys held with a digit, and every digit must be free to take.
    if id == JUMP_TO_TAB {
        if rules::tab_digit(&k.key).is_none() {
            return Some("Hold the keys with a digit: 1 goes to the first tab, 9 to the last.".into());
        }
        let (ctrl, meta, alt, shift) = held(k);
        if let Some(reserved) = (1..=9).find_map(|d| reserved_reason(&keys(&d.to_string(), ctrl, meta, alt, shift), on)) {
            return Some(reserved);
        }
    }
    // Ctrl+F is a terminal program's key, but finding hands it back to the programs that read
    // it, which no other shortcut can do.
    let hands_back = id == "find" && same_keys(k, &keys("f", true, false, false, false));
    if !hands_back && id != JUMP_TO_TAB {
        if let Some(reserved) = reserved_reason(k, on) {
            return Some(reserved);
        }
    }
    let name = entry(id).map(|e| e.name);
    for other in &SHORTCUTS {
        let Some(has) = current_keys(other.id, on.mac(), changed).filter(|_| other.id != id) else { continue };
        if !clash(id, k, other.id, &has) {
            continue;
        }
        // For the nine tabs, the one key of theirs another shortcut has.
        let label = keys_label(if id == JUMP_TO_TAB { &has } else { k }, on);
        return Some(if Some(other.name) == name {
            format!("{label} already opens the command bar.")
        } else {
            format!("{label} already does “{}”, so change that one first.", other.name)
        });
    }
    None
}

/// Gives `id` new keys in `changed`: `Some(Some(keys))` those, `Some(None)` none at all,
/// `None` its own again. Its own keys need no entry, so Reset shows as done.
pub fn set_keys(changed: &mut Changed, id: &str, to: Option<Option<Keys>>, mac: bool) {
    // A tab by its number keeps the keys held, with the digit 1 for all nine.
    let to = match to {
        Some(Some(k)) if id == JUMP_TO_TAB => {
            let (ctrl, meta, alt, shift) = held(&k);
            Some(Some(keys("1", ctrl, meta, alt, shift)))
        }
        other => other,
    };
    let own = default_keys(id, mac);
    let is_own = match &to {
        None => true,
        Some(None) => own.is_none(),
        Some(Some(k)) => own.as_ref().is_some_and(|own| same_keys(own, k)),
    };
    match to {
        Some(keys) if !is_own => {
            changed.insert(id.to_owned(), keys);
        }
        _ => {
            changed.shift_remove(id);
        }
    }
}

/// Gives `ids` their own keys back. `Err`: one of them can't have its own keys, because
/// another shortcut was given them; nothing is changed, and the line says which.
pub fn reset(changed: &mut Changed, ids: &[&str], on: Keyboard) -> Result<(), String> {
    for id in ids {
        if let Some(why) = default_keys(id, on.mac()).and_then(|own| problem(id, &own, on, changed)) {
            return Err(why);
        }
    }
    for id in ids {
        changed.shift_remove(*id);
    }
    Ok(())
}

/// Whether the 1.0 window acts on this shortcut yet.
pub fn works(id: &str) -> bool {
    entry(id).is_some_and(|e| e.does.is_some())
}

/// A key that works the same everywhere and can't be changed, listed so the page has them all.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixed {
    pub name: &'static str,
    pub note: Option<&'static str>,
    pub keys: Vec<Keys>,
}

/// The keys that can't be changed, as 0.10 lists them under "Always the same".
pub fn fixed(on: Keyboard) -> Vec<Fixed> {
    let mut list = vec![
        Fixed { name: "Next tab", note: None, keys: vec![keys("tab", true, false, false, false)] },
        Fixed { name: "Previous tab", note: None, keys: vec![keys("tab", true, false, false, true)] },
    ];
    if on.mac() {
        list.push(Fixed { name: "Clear the terminal", note: None, keys: vec![keys("k", false, true, false, false)] });
        list.push(Fixed { name: "Copy", note: None, keys: vec![keys("c", false, true, false, false)] });
        list.push(Fixed { name: "Paste", note: None, keys: vec![keys("v", false, true, false, false)] });
    } else {
        list.push(Fixed { name: "Copy in a terminal", note: Some("Ctrl+C copies too while text is selected."), keys: vec![keys("c", true, false, false, true)] });
        let mut paste = vec![keys("v", true, false, false, true)];
        if on == Keyboard::Windows {
            paste.push(keys("v", true, false, false, false));
        }
        list.push(Fixed { name: "Paste in a terminal", note: None, keys: paste });
    }
    list.push(Fixed {
        name: "New line in AI coding agents",
        note: Some("Claude Code, OpenCode and Codex. A shell takes it as Enter."),
        keys: vec![keys("enter", false, false, false, true)],
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::shortcuts as rules;

    const MAC: Keyboard = Keyboard::Mac;
    const WIN: Keyboard = Keyboard::Windows;
    const LINUX: Keyboard = Keyboard::Other;

    fn none() -> Changed {
        Changed::new()
    }

    #[test]
    fn every_0_10_shortcut_is_listed_once_with_its_keys() {
        let ids: Vec<&str> = SHORTCUTS.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 19);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 19, "an id is listed twice");
        // The table of the inventory (section 6), Mac then elsewhere.
        for (id, mac, other) in [
            ("palette", "⌘L", "Ctrl+Shift+L"),
            ("paletteAlt", "", "Ctrl+Shift+K"),
            ("find", "⌘F", "Ctrl+F"),
            ("addHost", "⇧⌘N", "Ctrl+Shift+N"),
            ("settings", "⌘,", "Ctrl+,"),
            ("newTerminal", "⌘T", "Ctrl+Shift+T"),
            ("closePane", "⌘W", "Ctrl+Shift+W"),
            ("reopenTab", "⇧⌘T", "Ctrl+Shift+Z"),
            ("splitRight", "⌘D", "Ctrl+Shift+D"),
            ("splitDown", "⇧⌘D", "Ctrl+Shift+E"),
            ("zoomPane", "⇧⌘↩", "Ctrl+Shift+Enter"),
            ("toggleSplit", "⇧⌘S", "Ctrl+Shift+S"),
            ("sidebar", "⌘B", "Ctrl+Shift+B"),
            ("vitals", "⌘J", "Ctrl+Shift+J"),
            ("compact", "⇧⌘M", "Ctrl+Shift+M"),
            ("streamer", "⇧⌘H", "Ctrl+Shift+H"),
            // New in 1.0.
            ("agents", "⇧⌘A", "Ctrl+Shift+A"),
            ("files", "⇧⌘E", "Ctrl+Shift+O"),
            ("jumpToTab", "⌘1…9", "Ctrl+Shift+1…9"),
        ] {
            assert_eq!(shortcut_label(id, MAC, &none()), mac, "{id} on a Mac");
            assert_eq!(shortcut_label(id, LINUX, &none()), other, "{id} elsewhere");
        }
        // Every one is in a group, but the command bar's second key, which shows on its row.
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
        assert_eq!(grouped.len(), 18);
        for id in ids {
            assert!(grouped.contains(&id) || id == "paletteAlt", "{id} is in no group");
        }
    }

    #[test]
    fn the_list_and_the_windows_rules_agree() {
        // What the window acts on has, here, the keys it matches there.
        for e in SHORTCUTS.iter().filter(|e| e.does.is_some()) {
            let shortcut = e.does.unwrap();
            assert_eq!(shortcut.id(), Some(e.id));
            for mac in [true, false] {
                assert_eq!(default_keys(e.id, mac), rules::default_keys(shortcut, mac), "{} (mac: {mac})", e.id);
                // The command bar has no second key on a Mac.
                let Some(k) = default_keys(e.id, mac) else { continue };
                let (ctrl, meta, alt, shift) = held(&k);
                let pressed = rules::Pressed { key: &k.key, ctrl, meta, alt, shift };
                // Find comes back saying whether a program may keep it; it's the same shortcut.
                assert_eq!(rules::match_shortcut(&pressed, mac, &none()).and_then(Shortcut::id), Some(e.id), "{} (mac: {mac})", e.id);
            }
        }
        assert!(works("newTerminal") && works("settings") && works("streamer") && works("palette") && works("find"));
        assert!(works("splitRight") && works("splitDown") && works("zoomPane"));
        assert!(works("compact") && !works("nobody"));
    }

    #[test]
    fn no_two_shortcuts_come_with_the_same_keys_and_none_is_refused() {
        for on in [MAC, WIN, LINUX] {
            for e in &SHORTCUTS {
                let Some(own) = default_keys(e.id, on.mac()) else { continue };
                assert_eq!(problem(e.id, &own, on, &none()), None, "{} on {on:?}", e.id);
            }
        }
    }

    #[test]
    fn keys_the_terminal_or_the_system_needs_are_refused_with_the_reason() {
        let why = |key: &str, mods: &str, on: Keyboard| {
            reserved_reason(&keys(key, mods.contains('c'), mods.contains('m'), mods.contains('a'), mods.contains('s')), on)
        };
        // Elsewhere: a shell's Ctrl+letter keys can't be taken from the terminal.
        for key in ["w", "c", "d", "z", "r", "1", "enter"] {
            let said = why(key, "c", LINUX).unwrap_or_else(|| panic!("Ctrl+{key} was allowed"));
            assert!(said.ends_with("themselves, so add Shift."), "{said}");
        }
        assert_eq!(why("w", "c", LINUX).as_deref(), Some("Terminal programs use Ctrl+W themselves, so add Shift."));
        // Plain Ctrl with punctuation no program reads, and function keys, are fine.
        for key in [",", ".", ";", "'", "=", "f5", "f12"] {
            assert_eq!(why(key, "c", LINUX), None, "Ctrl+{key}");
        }
        assert_eq!(why("k", "cs", LINUX), None);
        assert_eq!(why("k", "", LINUX).as_deref(), Some("Hold Ctrl and Shift with a key, so plain typing still reaches the terminal."));
        assert_eq!(why("k", "s", LINUX).as_deref(), Some("Hold Ctrl and Shift with a key, so plain typing still reaches the terminal."));
        assert_eq!(why("k", "ca", LINUX).as_deref(), Some("Alt and AltGr stay with the terminal and your keyboard layout, so use Ctrl+Shift instead."));
        assert_eq!(why("k", "cm", LINUX).as_deref(), Some("The Super key belongs to the system."));
        assert_eq!(why("k", "cm", WIN).as_deref(), Some("The Windows key belongs to the system."));
        assert_eq!(why("tab", "c", LINUX).as_deref(), Some("Ctrl+Tab and Ctrl+Shift+Tab switch tabs."));
        assert_eq!(why("c", "cs", LINUX).as_deref(), Some("Ctrl+Shift+C and Ctrl+Shift+V copy and paste in terminals."));
        assert_eq!(why("v", "cs", WIN).as_deref(), Some("Ctrl+Shift+C and Ctrl+Shift+V copy and paste in terminals."));
        assert_eq!(why("escape", "cs", LINUX).as_deref(), Some("Esc is kept for closing things."));
        // A Mac: ⌘ it must be; ⌃ and ⌥ stay the terminal's and typing's.
        assert_eq!(why("k", "c", MAC).as_deref(), Some("Terminal programs use ⌃ with a key themselves, so hold ⌘ instead."));
        assert_eq!(why("k", "a", MAC).as_deref(), Some("⌥ types accented letters and symbols, so hold ⌘ instead."));
        assert_eq!(why("k", "", MAC).as_deref(), Some("Hold ⌘ with a key, so plain typing still reaches the terminal."));
        assert_eq!(why("k", "m", MAC).as_deref(), Some("⌘K clears the terminal, as in other Mac terminals."));
        assert_eq!(why("q", "m", MAC).as_deref(), Some("⌘Q quits SSHeesh."));
        assert_eq!(why("z", "ms", MAC).as_deref(), Some("⇧⌘Z redoes typing."));
        assert_eq!(why("space", "m", MAC).as_deref(), Some("macOS uses ⌘Space itself."));
        assert_eq!(why("tab", "m", MAC).as_deref(), Some("macOS uses ⌘⇥ itself."));
        assert_eq!(why("4", "ms", MAC).as_deref(), Some("macOS uses ⇧⌘4 itself."));
        assert_eq!(why("e", "m", MAC), None);
        assert_eq!(why("e", "mca", MAC), None);
    }

    #[test]
    fn keys_another_shortcut_has_are_refused_until_that_one_changes() {
        let mut changed = none();
        let taken = keys("t", true, false, false, true);
        assert_eq!(problem("closePane", &taken, LINUX, &changed).as_deref(), Some("Ctrl+Shift+T already does “New terminal”, so change that one first."));
        // Its own keys are no conflict with itself.
        assert_eq!(problem("newTerminal", &taken, LINUX, &changed), None);
        // The command bar's two keys are one thing.
        assert_eq!(problem("paletteAlt", &keys("l", true, false, false, true), LINUX, &changed).as_deref(), Some("Ctrl+Shift+L already opens the command bar."));
        // Move New terminal away and its old keys are free.
        set_keys(&mut changed, "newTerminal", Some(Some(keys("y", true, false, false, true))), false);
        assert_eq!(problem("closePane", &taken, LINUX, &changed), None);
        assert_eq!(
            problem("closePane", &keys("y", true, false, false, true), LINUX, &changed).as_deref(),
            Some("Ctrl+Shift+Y already does “New terminal”, so change that one first.")
        );
        // A shortcut that was turned off holds no keys.
        set_keys(&mut changed, "paletteAlt", Some(None), false);
        assert_eq!(problem("closePane", &keys("k", true, false, false, true), LINUX, &changed), None);
        // Finding may have plain Ctrl+F; nothing else may.
        assert_eq!(problem("find", &keys("f", true, false, false, false), LINUX, &changed), None);
        assert_eq!(problem("sidebar", &keys("f", true, false, false, false), LINUX, &changed).as_deref(), Some("Terminal programs use Ctrl+F themselves, so add Shift."));
    }

    #[test]
    fn changing_and_resetting_keeps_only_what_differs() {
        let mut changed = none();
        let own = default_keys("sidebar", false).unwrap();
        set_keys(&mut changed, "sidebar", Some(Some(own.clone())), false);
        assert!(changed.is_empty(), "its own keys need no entry");
        set_keys(&mut changed, "sidebar", Some(Some(keys("u", true, false, false, true))), false);
        assert_eq!(shortcut_label("sidebar", LINUX, &changed), "Ctrl+Shift+U");
        // The Mac has no second key for the command bar: "none" is its own there.
        set_keys(&mut changed, "paletteAlt", Some(None), true);
        assert_eq!(changed.len(), 1);
        set_keys(&mut changed, "paletteAlt", Some(None), false);
        assert_eq!(changed.get("paletteAlt"), Some(&None));
        assert_eq!(shortcut_label("paletteAlt", LINUX, &changed), "");
        // Reset one, then the other.
        assert_eq!(reset(&mut changed, &["sidebar"], LINUX), Ok(()));
        assert_eq!(shortcut_label("sidebar", LINUX, &changed), "Ctrl+Shift+B");
        assert_eq!(reset(&mut changed, &["palette", "paletteAlt"], LINUX), Ok(()));
        assert!(changed.is_empty());
        // A reset that would land on keys given to another shortcut is refused, and says why.
        set_keys(&mut changed, "sidebar", Some(Some(keys("u", true, false, false, true))), false);
        set_keys(&mut changed, "vitals", Some(Some(keys("b", true, false, false, true))), false);
        let before = changed.clone();
        assert_eq!(reset(&mut changed, &["sidebar"], LINUX), Err("Ctrl+Shift+B already does “Show or hide server vitals”, so change that one first.".into()));
        assert_eq!(changed, before);
        // Keys saved without a key name hold nothing.
        changed.insert("compact".into(), Some(Keys::default()));
        assert_eq!(current_keys("compact", false, &changed), None);
    }

    #[test]
    fn keys_read_as_each_system_writes_them() {
        assert_eq!(keys_label(&keys("d", false, true, false, true), MAC), "⇧⌘D");
        assert_eq!(keys_label(&keys("up", true, true, true, true), MAC), "⌃⌥⇧⌘↑");
        assert_eq!(keys_label(&keys("pageup", true, false, false, false), LINUX), "Ctrl+PgUp");
        assert_eq!(keys_label(&keys("k", true, true, true, true), WIN), "Ctrl+Alt+Shift+Win+K");
        assert_eq!(keys_label(&keys("k", true, true, false, false), LINUX), "Ctrl+Super+K");
        assert_eq!(keys_label(&keys("f5", true, false, false, false), LINUX), "Ctrl+F5");
        // Held so far, while recording.
        assert_eq!(keys_label(&keys("", true, false, false, true), LINUX), "Ctrl+Shift+");
        assert_eq!(keys_label(&keys("", false, true, false, true), MAC), "⇧⌘");
    }

    #[test]
    fn the_keys_that_never_change_are_listed_for_each_system() {
        let names = |on: Keyboard| fixed(on).iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(MAC), ["Next tab", "Previous tab", "Clear the terminal", "Copy", "Paste", "New line in AI coding agents"]);
        assert_eq!(names(LINUX), ["Next tab", "Previous tab", "Copy in a terminal", "Paste in a terminal", "New line in AI coding agents"]);
        let paste = |on: Keyboard| fixed(on).into_iter().find(|f| f.name == "Paste in a terminal").unwrap().keys.len();
        assert_eq!((paste(WIN), paste(LINUX)), (2, 1));
    }

    #[test]
    fn the_shortcuts_1_0_added_are_listed_changed_and_checked_like_the_others() {
        // Each is in Settings, and does what the window does on it.
        for id in ["agents", "files", JUMP_TO_TAB] {
            assert!(works(id), "{id}");
            assert!(GROUPS.iter().any(|(_, ids)| ids.contains(&id)), "{id} is in no group");
        }
        // Files no longer has 0.10's Split down keys, and nothing shares keys with another.
        assert_eq!(problem("splitDown", &keys("e", true, false, false, true), LINUX, &none()), None);
        assert_eq!(
            problem("files", &keys("e", true, false, false, true), LINUX, &none()).as_deref(),
            Some("Ctrl+Shift+E already does “Split down”, so change that one first.")
        );
        // The agents' keys are taken like any shortcut's.
        assert_eq!(
            problem("sidebar", &keys("a", true, false, false, true), LINUX, &none()).as_deref(),
            Some("Ctrl+Shift+A already does “Agents at work”, so change that one first.")
        );
        let mut changed = none();
        set_keys(&mut changed, "agents", Some(Some(keys("g", true, false, false, true))), false);
        assert_eq!(shortcut_label("agents", LINUX, &changed), "Ctrl+Shift+G");
        assert_eq!(problem("sidebar", &keys("a", true, false, false, true), LINUX, &changed), None);
    }

    #[test]
    fn keys_you_had_before_a_new_shortcut_came_stay_yours() {
        // 0.10 let you put the sidebar on Ctrl+Shift+A; the agents came with it later.
        let mut changed = none();
        changed.insert("sidebar".into(), Some(keys("a", true, false, false, true)));
        assert_eq!(shortcut_label("sidebar", LINUX, &changed), "Ctrl+Shift+A");
        assert_eq!(shortcut_label("agents", LINUX, &changed), "");
        // The agents can take other keys; their own come back only once the sidebar lets go.
        assert_eq!(problem("agents", &keys("g", true, false, false, true), LINUX, &changed), None);
        assert_eq!(
            reset(&mut changed, &["agents"], LINUX),
            Err("Ctrl+Shift+A already does “Show or hide the sidebar”, so change that one first.".into())
        );
        assert_eq!(reset(&mut changed, &["sidebar"], LINUX), Ok(()));
        assert_eq!(shortcut_label("agents", LINUX, &changed), "Ctrl+Shift+A");
    }

    #[test]
    fn a_tab_by_its_number_is_the_keys_held_with_any_digit() {
        let mut changed = none();
        // Recorded with any digit, kept with 1.
        assert_eq!(problem(JUMP_TO_TAB, &keys("4", false, true, false, false), MAC, &changed), None);
        set_keys(&mut changed, JUMP_TO_TAB, Some(Some(keys("4", false, true, true, false))), true);
        assert_eq!(changed.get(JUMP_TO_TAB), Some(&Some(keys("1", false, true, true, false))));
        assert_eq!(shortcut_label(JUMP_TO_TAB, MAC, &changed), "⌥⌘1…9");
        // A letter isn't a number, and a digit the system keeps refuses all nine.
        assert_eq!(
            problem(JUMP_TO_TAB, &keys("t", false, true, false, false), MAC, &none()).as_deref(),
            Some("Hold the keys with a digit: 1 goes to the first tab, 9 to the last.")
        );
        assert_eq!(problem(JUMP_TO_TAB, &keys("2", false, true, false, true), MAC, &none()).as_deref(), Some("macOS uses ⇧⌘3 itself."));
        assert_eq!(
            problem(JUMP_TO_TAB, &keys("2", true, false, false, false), LINUX, &none()).as_deref(),
            Some("Terminal programs use Ctrl+1 themselves, so add Shift.")
        );
        // Another shortcut can't take one of the nine, nor the nine one another has.
        assert_eq!(
            problem("vitals", &keys("7", true, false, false, true), LINUX, &none()).as_deref(),
            Some("Ctrl+Shift+7 already does “Go to a tab by its number”, so change that one first.")
        );
        let mut changed = none();
        set_keys(&mut changed, "vitals", Some(Some(keys("7", false, true, false, false))), true);
        assert_eq!(
            problem(JUMP_TO_TAB, &keys("1", false, true, false, false), MAC, &changed).as_deref(),
            Some("⌘7 already does “Show or hide server vitals”, so change that one first.")
        );
    }
}
