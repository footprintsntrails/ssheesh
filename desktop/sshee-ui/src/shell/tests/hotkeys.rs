//! Every shortcut in Settings, pressed with its own keys (Ctrl+Shift off a Mac, ⌘ on one) with
//! a terminal focused, with Files on screen and on the main menu, and what it did. Then each
//! one moved to other keys: the new keys work and the old ones don't.

use sshee_app::model::{CompactMode, Keys};
use sshee_app::settings_store::keymap::{self, SHORTCUTS};
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::TabId;

use super::*;

/// Where the keyboard is when the shortcut is pressed.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Terminal,
    Files,
    Menu,
}

const PLACES: [Place; 3] = [Place::Terminal, Place::Files, Place::Menu];

/// What a shortcut can change, read before and after it.
#[derive(Debug, Clone, PartialEq)]
struct Probe {
    tabs: Vec<(TabKind, String)>,
    closed: usize,
    active: Option<TabId>,
    active_kind: Option<TabKind>,
    first: Option<TabId>,
    on_screen: usize,
    home: bool,
    sidebar: bool,
    vitals: bool,
    compact: CompactMode,
    settings: bool,
    palette: bool,
    add_server: bool,
    agents: bool,
    streamer: bool,
    find: bool,
}

impl Window {
    fn probe(&mut self) -> Probe {
        let find = self.rt.state.active_pane().is_some_and(|p| p.find_is_open());
        let s = &self.rt.state;
        let b = &s.bench;
        Probe {
            tabs: b.tabs().iter().map(|t| (t.kind, t.server.clone())).collect(),
            closed: b.closed().len(),
            active: b.active_id(),
            active_kind: b.active().map(|t| t.kind),
            first: b.tabs().first().map(|t| t.id),
            on_screen: b.on_screen().len(),
            home: b.showing_home(),
            sidebar: b.sidebar_shown(),
            vitals: s.settings.get().show_vitals,
            compact: s.settings.get().compact_mode.clone(),
            settings: s.screens.settings.open,
            palette: s.screens.palette.open,
            add_server: s.screens.add_server.open,
            agents: s.screens.agent.list_open(),
            streamer: s.streaming(),
            find,
        }
    }

    /// Presses `k` as a keyboard sends it: ⌘ is the command key on a Mac, Ctrl elsewhere.
    fn chord(&mut self, k: &Keys, mac: bool) {
        let on = |b: Option<bool>| b.unwrap_or(false);
        let key = match k.key.as_str() {
            "enter" => Key::Enter,
            "tab" => Key::Tab,
            other => Key::Command(other.chars().next().expect("a key")),
        };
        let (ctrl, logo) = (on(k.ctrl), on(k.meta));
        let mods = Mods { shift: on(k.shift), alt: on(k.alt), ctrl, logo, cmd: if mac { logo } else { ctrl } };
        self.rt.key(KeyEvent { key, mods });
        self.settle();
    }

    /// Two terminals (db's on screen), a tab closed before them, and the keyboard at `place`;
    /// with `split`, the tab on screen shares the page with a terminal beside it.
    fn ready(mac: bool, place: Place, split: bool) -> Window {
        // Off a Mac, as on Windows, where the owner found Ctrl+Shift+Z doing nothing.
        let mut w = Window::open(if mac { Frame::Mac } else { Frame::Windows }, mac);
        w.rt.update(|s, tx| {
            let gone = s.bench.open(TabKind::Terminal, DB);
            s.settle(tx);
            s.bench.close(gone);
            s.bench.open(TabKind::Terminal, WEB);
            s.bench.open(TabKind::Terminal, DB);
            // The last one, so "the first tab" is somewhere else.
            let last = s.bench.tabs()[1].id;
            s.bench.focus(last);
            s.settle(tx);
        });
        w.settle();
        match place {
            Place::Terminal => w.click("term"),
            Place::Files => {
                w.rt.update(|s, tx| {
                    s.open_files_on(WEB, None, tx);
                });
                w.settle();
            }
            Place::Menu => {}
        }
        if split {
            w.rt.update(|s, tx| s.split_pane(sshee_app::workbench::layout::Dir::Row, tx));
            w.settle();
            if place == Place::Terminal {
                w.click("term");
            }
        }
        if place == Place::Menu {
            w.rt.update(|s, tx| s.show_home(tx));
            w.settle();
        }
        w
    }
}

/// Whether `shortcut` did its work between `a` and `b` at `place`; `None` where it has nothing
/// to do (Find with no terminal on screen, a split on the main menu).
fn did(shortcut: Shortcut, place: Place, a: &Probe, b: &Probe) -> Option<bool> {
    let menu = place == Place::Menu;
    let kinds = |p: &Probe, k: TabKind| p.tabs.iter().filter(|t| t.0 == k).count();
    Some(match shortcut {
        Shortcut::Palette | Shortcut::PaletteAlt => !a.palette && b.palette,
        Shortcut::SplitRight | Shortcut::SplitDown if menu => return None,
        Shortcut::SplitRight | Shortcut::SplitDown => b.on_screen == a.on_screen + 1 && b.tabs.len() == a.tabs.len() + 1,
        Shortcut::CloseTab if menu => return None,
        Shortcut::CloseTab => b.tabs.len() + 1 == a.tabs.len() && b.closed == a.closed + 1,
        Shortcut::NewTerminal => kinds(b, TabKind::Terminal) == kinds(a, TabKind::Terminal) + 1 && !b.home,
        Shortcut::ReopenTab => b.tabs.len() == a.tabs.len() + 1 && b.closed + 1 == a.closed && !b.home,
        Shortcut::SideBySide if menu => return None,
        Shortcut::SideBySide => b.on_screen != a.on_screen,
        Shortcut::AddServer => !a.add_server && b.add_server,
        Shortcut::Settings => !a.settings && b.settings,
        Shortcut::ToggleSidebar => a.sidebar != b.sidebar,
        Shortcut::ToggleVitals => a.vitals != b.vitals,
        Shortcut::Zoom if menu => return None,
        Shortcut::Zoom => b.on_screen == 1 && a.on_screen == 2,
        Shortcut::Streamer => a.streamer != b.streamer,
        Shortcut::Compact => a.compact != b.compact,
        // Find is the terminal's; Files has its own filter, the menu its own search.
        Shortcut::Find { .. } if place != Place::Terminal => return None,
        Shortcut::Find { .. } => !a.find && b.find,
        Shortcut::Agents => !a.agents && b.agents,
        // Already on this machine's Files: it stays there.
        Shortcut::OpenFiles if place == Place::Files => return None,
        Shortcut::OpenFiles => b.active_kind == Some(TabKind::Files) && !b.home,
        Shortcut::JumpToTab(_) => b.active == b.first && !b.home,
        Shortcut::StepTab(_) => return None,
    })
}

/// Whether a split must be on the page first.
fn needs_split(shortcut: Shortcut) -> bool {
    matches!(shortcut, Shortcut::Zoom)
}

/// `shortcut`, pressed with `keys` at `place`, after it was moved to other keys when `moved`
/// says so: whether it did its work (`None`: nothing to do there), and what changed.
fn press(mac: bool, shortcut: Shortcut, place: Place, moved: Option<(&str, &Keys)>, keys: &Keys) -> (Option<bool>, String) {
    let mut w = Window::ready(mac, place, needs_split(shortcut));
    if let Some((id, k)) = moved {
        let (id, k) = (id.to_owned(), k.clone());
        w.rt.update(|s, tx| {
            s.change_settings(tx, |st| {
                st.set(|x| {
                    x.shortcuts.insert(id.clone(), Some(k.clone()));
                })
            })
        });
        w.settle();
    }
    let before = w.probe();
    w.chord(keys, mac);
    let after = w.probe();
    (did(shortcut, place, &before, &after), format!("{before:?} -> {after:?}"))
}

/// The owner's way on Windows: Ctrl and Shift go down first (as the keyboard sends them), the
/// last tab closes with W, and Z on the main menu brings it back, as its "Recently closed" says.
#[test]
fn on_windows_ctrl_shift_z_brings_back_the_tab_closed_last() {
    use rune::{PhysicalInput, PhysicalKeyCode};
    let mut w = Window::open(Frame::Windows, false);
    w.click("server-1");
    w.click("term");
    let held = Mods { ctrl: true, shift: true, cmd: true, ..Mods::default() };
    w.rt.physical_input(PhysicalInput::Key { key: PhysicalKeyCode::ControlLeft, mods: Mods { ctrl: true, cmd: true, ..Mods::default() }, pressed: true, repeat: false });
    w.rt.physical_input(PhysicalInput::Key { key: PhysicalKeyCode::ShiftLeft, mods: held, pressed: true, repeat: false });
    for (code, c) in [(PhysicalKeyCode::KeyW, 'w'), (PhysicalKeyCode::KeyZ, 'z')] {
        w.rt.physical_input(PhysicalInput::Key { key: code, mods: held, pressed: true, repeat: false });
        w.rt.key(KeyEvent { key: Key::Command(c), mods: held });
        w.rt.physical_input(PhysicalInput::Key { key: code, mods: held, pressed: false, repeat: false });
        w.settle();
        if c == 'w' {
            assert!(w.rt.state.bench.showing_home() && w.rt.state.bench.tabs().is_empty());
            assert!(w.tree().contains("Ctrl+Shift+Z brings back the last one"), "{}", w.tree());
        }
    }
    assert_eq!(w.row(), "*Terminal@web-7731");
}

/// Every shortcut Settings lists, at every place, on both keyboards.
#[test]
fn every_shortcut_works_with_its_own_keys_everywhere() {
    let mut failed = Vec::new();
    for mac in [false, true] {
        for e in SHORTCUTS.iter() {
            let (Some(shortcut), Some(keys)) = (e.does, keymap::default_keys(e.id, mac)) else { continue };
            for place in PLACES {
                if let (Some(false), how) = press(mac, shortcut, place, None, &keys) {
                    failed.push(format!("{} ({}) at {place:?}: {how}", e.id, keymap::keys_label(&keys, keymap::Keyboard::of(mac))));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
}

/// Each shortcut moved to Ctrl+Shift+Y (⇧⌘Y on a Mac):
/// the new keys do its work, and its own keys no longer do.
#[test]
fn a_moved_shortcut_works_on_its_new_keys_and_not_its_old_ones() {
    let mut failed = Vec::new();
    for mac in [false, true] {
        for e in SHORTCUTS.iter() {
            let (Some(shortcut), Some(own)) = (e.does, keymap::default_keys(e.id, mac)) else { continue };
            // "A tab by its number" stays on the digits: 1 stands for all nine.
            // "A tab by its number" stays on the digits, and Settings allows them with nothing
            // else held (Ctrl+1 is the terminal's, ⇧⌘3 the Mac's screenshot): nowhere to move.
            if e.id == sshee_app::workbench::shortcuts::JUMP_TO_TAB {
                continue;
            }
            let moved = if mac { keymap::keys("y", false, true, false, true) } else { keymap::keys("y", true, false, false, true) };
            assert_eq!(keymap::problem(e.id, &moved, keymap::Keyboard::of(mac), &keymap::Changed::new()), None, "Settings would refuse {}", e.id);
            for place in PLACES {
                if let (Some(true), how) = press(mac, shortcut, place, Some((e.id, &moved)), &own) {
                    failed.push(format!("{} still works on its old keys at {place:?} (mac: {mac}): {how}", e.id));
                }
                if let (Some(false), how) = press(mac, shortcut, place, Some((e.id, &moved)), &moved) {
                    failed.push(format!("{} doesn't work on its new keys at {place:?} (mac: {mac}): {how}", e.id));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
}

