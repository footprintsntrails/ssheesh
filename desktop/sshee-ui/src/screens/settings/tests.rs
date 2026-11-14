//! Settings without a screen (`Runtime::with_text`): real frames, clicks and keys, and what
//! the store, the window and the terminals hold afterwards.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use sshee_app::model::{EnvLabel, Settings};
use sshee_app::settings_store::catalog::{self, Section, Status};
use sshee_app::settings_store::{PrivacyNotice, Seed, SettingsStore};
use sshee_app::workbench::{Server, Tab, Tor, Workbench};
use sshee_term::engine::{CursorShape, Size};
use sshee_term::model::Model;

use super::TorCommand;
use crate::shell::view::view;
use crate::shell::{Backend, Options, Shell};
use crate::terminal::{LinkStatus, StatusSink, TermLink};
use crate::theme::{Frame, Theme};

/// What the fake backend was told.
#[derive(Default)]
struct Told {
    /// The size each session was last given, in the order they were made.
    sizes: Vec<Option<Size>>,
    written: Vec<u8>,
    /// How often the settings were handed over, and the last font size among them.
    settings: usize,
    reconnect: String,
    tor: Vec<TorCommand>,
}

struct Link(Arc<Mutex<Told>>, usize);

impl TermLink for Link {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        model.connected(false, 1_000);
        self.0.lock().unwrap().sizes[self.1] = Some(size);
        status(LinkStatus::Connected { durable: false });
    }
    fn write(&self, bytes: &[u8]) {
        self.0.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, size: Size) {
        self.0.lock().unwrap().sizes[self.1] = Some(size);
    }
    fn reconnect(&self) {}
    fn close(&self) {}
}

struct Fake {
    told: Arc<Mutex<Told>>,
    store: Option<SettingsStore>,
}

impl Backend for Fake {
    fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
        let mut told = self.told.lock().unwrap();
        told.sizes.push(None);
        Box::new(Link(self.told.clone(), told.sizes.len() - 1))
    }
    fn settings(&mut self) -> SettingsStore {
        self.store.take().expect("the window asks for its settings once")
    }
    fn settings_changed(&mut self, settings: &Settings) {
        let mut told = self.told.lock().unwrap();
        told.settings += 1;
        told.reconnect = settings.reconnect_mode.as_str().to_owned();
    }
    fn tor(&mut self, command: TorCommand) {
        self.told.lock().unwrap().tor.push(command);
    }
    fn replays(&self) -> bool {
        true
    }
}

struct Window {
    rt: Runtime<Shell>,
    told: Arc<Mutex<Told>>,
}

/// 0.10's defaults, with a cursor that keeps still.
fn quiet() -> Settings {
    Settings { cursor_blink: false, ..Settings::default() }
}

impl Window {
    fn open(store: SettingsStore, dark: bool, mac_keys: bool) -> Window {
        crate::fonts::declare();
        let told = Arc::new(Mutex::new(Told::default()));
        let servers = vec![
            Server::this_computer(),
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("db", "db-main", "postgres@203.0.113.31").icon("database"),
        ];
        let options = Options { frame: Frame::Linux, mac_keys, ..Options::default() };
        let shell = Shell::new(Workbench::new(servers), Box::new(Fake { told: told.clone(), store: Some(store) }), options, None);
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(dark, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, told };
        w.settle();
        w
    }

    fn defaults() -> Window {
        Window::open(SettingsStore::in_memory(quiet()), true, false)
    }

    /// A few frames, then long enough for what moves or fades to arrive.
    fn settle(&mut self) {
        for _ in 0..6 {
            self.rt.frame(1.0 / 60.0);
        }
        for _ in 0..8 {
            self.rt.frame(0.25);
        }
    }

    /// Scrolls the page so the element keyed `key` is in view, at once.
    fn show(&mut self, key: &str) {
        let key = key.to_owned();
        self.rt.update(move |_, tx| {
            tx.scroll_into_view(key);
            tx.instant();
        });
        self.settle();
    }

    fn click(&mut self, key: &str) {
        self.show(key);
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn key(&mut self, key: Key, held: &str) {
        let ctrl = held.contains('c');
        let logo = held.contains('m');
        let mods = Mods { shift: held.contains('s'), alt: held.contains('a'), ctrl, logo, cmd: if cfg!(target_os = "macos") { logo } else { ctrl } };
        self.rt.key(KeyEvent { key, mods });
        self.settle();
    }

    fn typed(&mut self, text: &str) {
        for ch in text.chars() {
            self.key(Key::Text(ch.to_string()), "");
        }
    }

    fn has(&self, key: &str) -> bool {
        self.rt.find(key).is_some()
    }

    fn rect(&self, key: &str) -> [f32; 4] {
        self.rt.presented(key).unwrap_or_else(|| panic!("no element {key}")).0
    }

    fn settings(&self) -> &Settings {
        self.rt.state.settings.get()
    }

    /// Opens Settings at `section` by its row in the list.
    fn go(&mut self, section: Section) {
        if !self.rt.state.screens.settings.open {
            self.click("settings-gear");
        }
        self.click(&format!("nav-{}", section.id()));
        assert_eq!(self.rt.state.screens.settings.section, section);
    }

    /// Every name and value a screen reader gets.
    fn tree(&self) -> String {
        let tree = self.rt.accessibility_tree();
        let mut out = String::new();
        for (_, node) in &tree.nodes {
            for t in [node.value(), node.label()].into_iter().flatten() {
                out.push_str(t);
                out.push('\n');
            }
        }
        out
    }

    fn fill(&self, key: &str) -> String {
        let info = self.rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap_or_else(|| panic!("no element {key}"));
        info.fill.hex()
    }

    /// Whether some text on screen says `words` (whole, or as its start).
    fn says(&self, words: &str) -> bool {
        self.rt.elements().iter().any(|e| !e.exiting && e.text.as_deref().is_some_and(|t| t.starts_with(words)))
    }
}

#[test]
fn the_gear_opens_settings_as_a_page_and_esc_goes_back_to_the_terminal() {
    let mut w = Window::defaults();
    w.click("server-1");
    assert!(w.has("term"));
    w.key(Key::Text("a".into()), "");
    w.click("settings-gear");
    // A page in the terminal's place, not a window over it: the bar, the sidebar and the
    // status bar stay.
    assert!(w.rt.state.screens.settings.open);
    assert!(w.has("settings") && !w.has("term"));
    for key in ["bar", "rail", "status", "settings-nav", "settings-head", "settings-search", "settings-close"] {
        assert!(w.has(key), "{key}");
    }
    let page = w.rect("page");
    assert_eq!(w.rect("settings"), page);
    // The list has the ten sections, in order, and starts on Layout.
    let mut last = 0.0;
    for section in Section::ALL {
        let [_, y, ..] = w.rect(&format!("nav-{}", section.id()));
        assert!(y > last, "{section:?} isn't under the one before it");
        last = y;
    }
    assert!(w.tree().contains("Layout") && w.tree().contains("Agents and notifications"));
    // No tab is the one on screen meanwhile; the gear is.
    assert_ne!(w.fill("tab-1"), Theme::of(true).sel.hex());
    assert_eq!(w.fill("settings-gear"), Theme::of(true).selrail.hex());
    // Nothing typed reaches the terminal nobody sees.
    w.key(Key::Text("x".into()), "");
    w.key(Key::Enter, "");
    assert_eq!(w.told.lock().unwrap().written, b"a");
    // Esc: back to the tab, which has the keyboard again.
    w.key(Key::Escape, "");
    assert!(!w.rt.state.screens.settings.open && w.has("term") && !w.has("settings"));
    assert_eq!(w.fill("tab-1"), Theme::of(true).sel.hex());
    w.key(Key::Text("b".into()), "");
    assert_eq!(w.told.lock().unwrap().written, b"ab");
}

#[test]
fn the_0_10_shortcut_opens_it_and_going_to_a_tab_leaves_it() {
    let mut w = Window::defaults();
    w.click("server-1");
    // Ctrl+, elsewhere; plain typing and Ctrl+Shift+, are not it.
    w.key(Key::Command(','), "cs");
    assert!(!w.rt.state.screens.settings.open);
    w.key(Key::Command(','), "c");
    assert!(w.rt.state.screens.settings.open);
    w.key(Key::Command(','), "c");
    assert!(!w.rt.state.screens.settings.open && w.has("term"));
    // From the main menu too, and a click on a tab or a server leaves it.
    w.key(Key::Command(','), "c");
    w.click("tab-1");
    assert!(!w.rt.state.screens.settings.open && w.has("term"));
    w.click("settings-gear");
    w.click("server-2");
    assert!(!w.rt.state.screens.settings.open);
    w.click("settings-gear");
    w.click("settings-close");
    assert!(!w.rt.state.screens.settings.open);
    // A Mac: ⌘,
    let mut mac = Window::open(SettingsStore::in_memory(quiet()), true, true);
    mac.key(Key::Command(','), "m");
    assert!(mac.rt.state.screens.settings.open);
    // Hiding the sidebar goes nowhere: Settings stays.
    mac.key(Key::Command('b'), "m");
    assert!(mac.rt.state.screens.settings.open && !mac.has("rail"));
    assert!(!mac.settings().sidebar_visible);
}

#[test]
fn every_section_draws_its_rows_in_both_appearances() {
    for dark in [true, false] {
        let mut w = Window::open(SettingsStore::in_memory(Settings { theme_follows_system: true, ..quiet() }), dark, false);
        let t = Theme::of(dark);
        assert_eq!(w.fill("window"), t.frame.hex());
        for section in Section::ALL {
            w.go(section);
            assert!(w.tree().contains(section.label()), "{section:?}");
            assert!(w.has(&format!("settings-{}", section.id())), "{section:?}");
            for row in catalog::section_rows(section, &w.rt.state.settings, sshee_app::model::Platform::current()) {
                assert!(w.has(row.key), "{} isn't drawn", row.key);
                // A row that comes later says so, in its one line.
                if let Status::Later(why) = row.status {
                    assert!(w.says(why), "{}: {why}", row.key);
                }
            }
        }
    }
}

#[test]
fn a_switch_and_a_segment_change_the_setting_at_once() {
    let mut w = Window::defaults();
    w.click("server-1");
    let before = w.told.lock().unwrap().settings;
    w.go(Section::Terminal);
    // Cursor: Bar, Block, Line.
    assert_eq!(w.rt.state.options.pane.cursor_shape, CursorShape::Bar);
    w.click("term-cursor-control/seg-1");
    assert_eq!(w.settings().cursor_style.as_str(), "block");
    assert_eq!(w.rt.state.options.pane.cursor_shape, CursorShape::Block);
    // The terminal that is already open has it too.
    assert_eq!(w.rt.state.panes.values().next().unwrap().options.cursor_shape, CursorShape::Block);
    // Blinking cursor: a switch.
    assert!(!w.settings().cursor_blink);
    w.click("term-blink-control");
    assert!(w.settings().cursor_blink && w.rt.state.options.pane.blink);
    w.click("term-blink-control");
    assert!(!w.settings().cursor_blink);
    // What only the program can do was handed to it each time.
    w.click("term-reconnect-control/seg-0");
    let told = w.told.lock().unwrap();
    assert_eq!(told.reconnect, "auto");
    assert_eq!(told.settings, before + 4);
}

#[test]
fn the_terminals_text_size_takes_effect_in_the_open_terminal() {
    let mut w = Window::defaults();
    w.click("server-1");
    assert_eq!(w.told.lock().unwrap().sizes[0], Some(Size::new(172, 33)));
    w.go(Section::Terminal);
    // The slider takes the arrow keys once it has the keyboard: three steps up from 13.
    w.click("term-font-size-control/track");
    let after_click = w.settings().terminal_font_size;
    w.key(Key::Right, "");
    w.key(Key::Right, "");
    assert_eq!(w.settings().terminal_font_size, after_click + 2.0);
    w.key(Key::Home, "");
    w.key(Key::Left, "");
    let size = w.settings().terminal_font_size;
    assert!((10.0..=20.0).contains(&size));
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|v| v.terminal_font_size = 16.0)));
    w.settle();
    assert_eq!(w.rt.state.options.pane.font_size, 16.0);
    // Back on its tab, the terminal holds fewer, larger cells, and its session was told.
    w.click("settings-close");
    w.settle();
    let size = w.told.lock().unwrap().sizes[0].unwrap();
    assert!(size.cols < 172 && size.rows < 33, "{size:?}");
    // Line height alone changes the rows and not the columns.
    let cols = size.cols;
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|v| v.terminal_line_height = 1.6)));
    w.settle();
    w.settle();
    let taller = w.told.lock().unwrap().sizes[0].unwrap();
    assert_eq!(taller.cols, cols);
    assert!(taller.rows < size.rows, "{taller:?} after {size:?}");
}

#[test]
fn appearance_follows_the_system_or_stays_light_or_dark() {
    // 0.10's default look is dark, whatever the system: so is 1.0's until told otherwise.
    let mut w = Window::open(SettingsStore::in_memory(quiet()), false, false);
    let (night, day) = (Theme::of(true), Theme::of(false));
    assert_eq!(w.fill("window"), night.frame.hex());
    w.go(Section::Appearance);
    w.click("look-appearance-control/seg-1");
    assert_eq!(w.fill("window"), day.frame.hex());
    assert_eq!(w.settings().theme, "daylight");
    // Match my system: the system here is light, then turns dark.
    w.click("look-appearance-control/seg-2");
    assert_eq!(w.fill("window"), night.frame.hex());
    w.click("look-appearance-control/seg-0");
    assert!(w.settings().theme_follows_system);
    assert_eq!(w.fill("window"), day.frame.hex());
    w.rt.set_system_dark(true);
    w.settle();
    assert_eq!(w.fill("window"), night.frame.hex());
    // A terminal follows: its page is the appearance's.
    w.click("look-appearance-control/seg-1");
    w.click("server-1");
    assert_eq!(w.fill("term"), day.page.hex());
}

#[test]
fn a_terminal_scheme_is_picked_from_0_10s() {
    let mut w = Window::defaults();
    w.click("server-1");
    assert_eq!(w.fill("term"), Theme::of(true).page.hex());
    w.go(Section::Terminal);
    // Dracula is the fifth card (Match app, SSHeesh Night, SSHeesh Day, Tokyo Night, …).
    assert!(w.tree().contains("Dracula") && w.tree().contains("Solarized Dark") && w.tree().contains("Match app"));
    w.click("scheme-5");
    assert_eq!(w.settings().terminal_scheme, "dracula");
    w.click("settings-close");
    assert_eq!(w.fill("term"), "#282a36ff");
    // "Your own" has no colours yet: it is there, and can't be picked.
    w.go(Section::Terminal);
    w.click("scheme-10");
    assert_eq!(w.settings().terminal_scheme, "dracula");
    // Reset puts the section back: the app's own colours again.
    w.click("settings-reset");
    assert_eq!(w.settings().terminal_scheme, "auto");
}

#[test]
fn a_pop_up_button_opens_its_menu_and_takes_the_pick() {
    let mut w = Window::defaults();
    w.go(Section::Terminal);
    assert!(!w.has("popup-menu"));
    w.click("term-welcome-control");
    assert!(w.has("popup-menu") && w.tree().contains("Nothing"));
    // The fifth: "Nothing". The rows about SSHeesh's own welcome go with it.
    assert!(w.has("term-welcome-parts"));
    w.click("popup-menu/item-4");
    assert!(!w.has("popup-menu"));
    assert_eq!(w.settings().welcome_mode.as_str(), "off");
    assert!(!w.has("term-welcome-parts") && !w.has("term-welcome-note"));
    // Esc closes an open menu and leaves Settings where it is.
    w.click("term-welcome-control");
    w.key(Key::Escape, "");
    assert!(!w.has("popup-menu") && w.rt.state.screens.settings.open);
    assert_eq!(w.settings().welcome_mode.as_str(), "off");
}

#[test]
fn a_row_that_comes_later_is_shown_and_cannot_be_changed() {
    let mut w = Window::defaults();
    w.go(Section::Layout);
    let before = w.settings().clone();
    // Tabs under each server, compact mode, the server list: there, dimmed, with why.
    for control in ["layout-tabs-control/seg-0", "layout-compact-control/seg-3", "layout-server-list-control/seg-0"] {
        w.click(control);
    }
    assert_eq!(*w.settings(), before);
    assert!(w.says("Tabs run along the top for now."));
    w.go(Section::Updates);
    w.click("updates-mode-control/seg-2");
    assert_eq!(w.settings().update_mode.as_str(), "auto");
    w.go(Section::Files);
    w.click("files-clean-control");
    assert!(w.settings().clean_uploads);
}

#[test]
fn the_sidebar_moves_to_the_right_and_tabs_can_stop_naming_their_server() {
    let mut w = Window::defaults();
    w.click("server-1");
    w.click("server-2");
    assert_eq!(w.rect("rail")[0], 0.0);
    w.go(Section::Layout);
    w.click("layout-sidebar-side-control/seg-1");
    assert_eq!(w.settings().sidebar_side.as_str(), "right");
    let rail = w.rect("rail");
    assert_eq!(rail[0] + rail[2], 1280.0);
    assert_eq!(w.rect("page")[0], 0.0);
    // The switch hides it and shows it, as the shortcut does.
    w.click("layout-sidebar-shown-control");
    assert!(!w.has("rail") && !w.settings().sidebar_visible);
    w.click("layout-sidebar-shown-control");
    assert!(w.has("rail") && w.settings().sidebar_visible);
    // Every tab names its server, by default.
    assert!(w.has("tab-1/server") && !w.has("group-1"));
    w.click("layout-tab-server-control");
    assert!(!w.rt.state.settings.more().tabs_name_server);
    assert!(!w.has("tab-1/server"), "the tab still names its server");
    assert!(w.has("group-1") && w.has("group-2"));
}

#[test]
fn streamer_mode_hides_names_in_the_window_and_in_the_terminal() {
    let mut w = Window::defaults();
    w.click("server-1");
    assert!(w.tree().contains("web-7731"));
    assert!(!w.rt.state.panes.values().next().unwrap().view.borrow().look().streaming);
    // Its shortcut works from anywhere.
    w.key(Key::Command('h'), "cs");
    assert!(w.settings().streamer_mode);
    let tree = w.tree();
    assert!(!tree.contains("web-7731") && !tree.contains("db-main"), "{tree}");
    assert!(tree.contains("Server 1") && tree.contains("This computer"));
    let pane = w.rt.state.panes.values().next().unwrap();
    assert!(pane.view.borrow().look().streaming);
    // The names the terminal dots out are the servers', their addresses' and their users'.
    let masked = pane.view.borrow().look().names.mask_text("ssh deploy@203.0.113.17 # web-7731");
    assert!(!masked.contains("deploy") && !masked.contains("203.0.113.17") && !masked.contains("web-7731"), "{masked}");
    // Privacy says what it does, and that a name can still slip through.
    w.go(Section::Privacy);
    assert!(w.says("Names are hidden as well as SSHeesh can spot them"));
    // Off again in Settings: the names are back. Hiding secrets is its own switch.
    w.click("privacy-streamer-control");
    assert!(!w.settings().streamer_mode && w.tree().contains("web-7731"));
    assert!(w.rt.state.panes.values().next().unwrap().view.borrow().look().hide_secrets);
    w.click("privacy-secrets-control");
    assert!(!w.settings().hide_secrets);
    assert!(!w.rt.state.panes.values().next().unwrap().view.borrow().look().hide_secrets);
}

#[test]
fn a_shortcut_is_recorded_by_pressing_keys_and_refused_with_the_reason() {
    let mut w = Window::defaults();
    w.click("server-1");
    w.go(Section::Keyboard);
    let tree = w.tree();
    for said in ["New terminal", "Ctrl+Shift+T", "Close tab", "Ctrl+Shift+W", "Streamer mode", "Next tab", "Ctrl+Tab", "Command bar", "Ctrl+Shift+L"] {
        assert!(tree.contains(said), "{said} isn't in: {tree}");
    }
    // Click New terminal's keys: it waits for new ones.
    w.click("keys-newTerminal");
    assert_eq!(w.rt.state.screens.settings.keys.id, Some("newTerminal"));
    assert!(w.tree().contains("Press keys…"));
    // Ctrl+W is the shell's: refused, in words, and it keeps waiting. No tab was closed.
    w.key(Key::Command('w'), "c");
    assert_eq!(w.rt.state.screens.settings.keys.problem.as_ref().map(|(_, why)| why.as_str()), Some("Terminal programs use Ctrl+W themselves, so add Shift."));
    assert!(w.has("problem-newTerminal") && w.rt.state.screens.settings.keys.id.is_some());
    assert_eq!(w.rt.state.bench.tabs().len(), 1);
    // Close tab's keys are taken: refused too, and the tab stays open.
    w.key(Key::Command('w'), "cs");
    assert_eq!(
        w.rt.state.screens.settings.keys.problem.as_ref().map(|(_, why)| why.as_str()),
        Some("Ctrl+Shift+W already does “Close tab”, so change that one first.")
    );
    assert_eq!(w.rt.state.bench.tabs().len(), 1);
    // Ctrl+Shift+Y is free: taken, saved, and recording ends.
    w.key(Key::Command('y'), "cs");
    assert_eq!(w.rt.state.screens.settings.keys, super::shortcuts::Recording::default());
    assert_eq!(w.settings().shortcuts["newTerminal"].as_ref().unwrap().key, "y");
    assert!(w.tree().contains("Ctrl+Shift+Y") && !w.has("problem-newTerminal"));
    // It works at once, and the old keys don't anymore.
    w.key(Key::Command('t'), "cs");
    assert_eq!(w.rt.state.bench.tabs().len(), 1);
    w.key(Key::Command('y'), "cs");
    assert_eq!(w.rt.state.bench.tabs().len(), 2);
    // Back in Settings: the arrow beside it gives its own keys back.
    w.go(Section::Keyboard);
    w.click("reset-newTerminal");
    assert!(w.settings().shortcuts.is_empty());
    assert!(!w.has("reset-newTerminal"));
    // Esc while recording ends the recording, not Settings.
    w.click("keys-closePane");
    w.key(Key::Escape, "");
    assert!(w.rt.state.screens.settings.keys.id.is_none() && w.rt.state.screens.settings.open);
    // A shortcut 1.0 doesn't act on yet (filling the window with a pane) can't be recorded.
    w.click("keys-zoomPane");
    assert!(w.rt.state.screens.settings.keys.id.is_none());
}

#[test]
fn search_finds_a_setting_and_goes_to_its_row() {
    let mut w = Window::defaults();
    w.click("settings-gear");
    w.click("settings-search");
    w.typed("font size");
    assert!(w.has("nav-results") && !w.has("nav-sections"));
    assert!(w.has("found-0") && !w.has("found-1"));
    w.click("found-0");
    let state = &w.rt.state.screens.settings;
    assert_eq!((state.section, state.found.as_deref()), (Section::Terminal, Some("term-font-size")));
    assert!(state.search.text.is_empty() && w.has("nav-sections") && w.has("term-font-size"));
    // Return takes the first thing found; a shortcut is found by its name.
    w.click("settings-search");
    w.typed("streamer");
    w.key(Key::Enter, "");
    assert_eq!(w.rt.state.screens.settings.section, Section::Privacy);
    w.click("settings-search");
    w.typed("reopen");
    w.click("found-0");
    assert_eq!(w.rt.state.screens.settings.section, Section::Keyboard);
    assert_eq!(w.rt.state.screens.settings.found.as_deref(), Some("shortcut-reopenTab"));
    // Nothing found says so; Esc empties the field before it leaves Settings.
    w.click("settings-search");
    w.typed("zebra");
    assert!(w.has("nav-none") && w.tree().contains("No setting matches “zebra”."));
    w.key(Key::Escape, "");
    assert!(w.rt.state.screens.settings.search.text.is_empty() && w.rt.state.screens.settings.open);
}

#[test]
fn settings_in_doubt_start_private_and_the_window_says_so() {
    // A first start that hasn't heard what 0.10 left: names hidden, nothing said yet.
    let dir = std::env::temp_dir().join(format!("sshee-ui-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut w = Window::open(SettingsStore::open(&dir, Default::default()), true, false);
    w.click("server-1");
    assert!(w.settings().streamer_mode && !w.has("privacy-notice"));
    assert!(!w.tree().contains("web-7731"));
    // The import was in doubt: the settings it could read come over, private, and the page
    // says so across its top.
    let notice = PrivacyNotice { reasons: vec!["no tabs were found".into()], turned_on: vec!["streamerMode".into()] };
    w.rt.update(|s, _| s.seed_settings(Seed::Imported { settings: Box::new(Settings { streamer_mode: true, terminal_font_size: 15.0, ..quiet() }), privacy: Some(notice) }));
    w.settle();
    assert!(w.has("privacy-notice"));
    assert!(w.tree().contains("Streamer mode is on, because your settings from before couldn't be read with certainty."));
    assert_eq!(w.rt.state.options.pane.font_size, 15.0);
    // The terminal is under the notice, not behind it.
    let (bar, term) = (w.rect("privacy-notice"), w.rect("term"));
    assert_eq!(term[1], bar[1] + bar[3]);
    // Review: Privacy, where the same is said, and streamer mode can be turned off.
    w.click("privacy-notice-review");
    assert_eq!(w.rt.state.screens.settings.section, Section::Privacy);
    assert!(w.has("privacy-why"));
    w.click("privacy-streamer-control");
    assert!(!w.settings().streamer_mode);
    // OK: it stops saying so.
    w.click("privacy-notice-ok");
    assert!(!w.has("privacy-notice") && !w.has("privacy-why") && w.rt.state.settings.notice().is_none());
    w.rt.state.settings.flush();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tors_switch_asks_the_backend_and_shows_its_state() {
    let mut w = Window::defaults();
    w.go(Section::Tor);
    assert!(!w.has("tor-state"));
    w.click("tor-switch-control");
    assert_eq!(w.told.lock().unwrap().tor, [TorCommand::On]);
    // The backend says how it went (here, by hand): starting, then on, with its button.
    w.rt.update(|s, _| s.bench.set_tor(Tor::Starting));
    w.settle();
    assert!(w.tree().contains("Tor is starting…") && !w.has("tor-action"));
    w.rt.update(|s, _| s.bench.set_tor(Tor::On { latency_ms: None }));
    w.settle();
    w.click("tor-action");
    w.click("tor-switch-control");
    assert_eq!(w.told.lock().unwrap().tor, [TorCommand::On, TorCommand::NewRoutes, TorCommand::Off]);
    // Hiding the typing rhythm is a setting of its own.
    w.click("tor-typing-control");
    assert!(w.settings().tor_hide_typing);
}

#[test]
fn a_word_to_highlight_is_added_checked_and_removed() {
    let mut w = Window::defaults();
    w.go(Section::Terminal);
    w.click("word-new");
    w.typed("nginx");
    w.click("word-color-3");
    w.click("word-add");
    let rules = &w.settings().highlight_rules;
    assert_eq!((rules.len(), rules[0].pattern.as_str(), rules[0].color.as_str()), (1, "nginx", "blue"));
    assert!(w.rt.state.screens.settings.word.text.is_empty() && w.has("word-0"));
    // A regular expression that doesn't work can't be added, and says why.
    w.click("word-new");
    w.typed("/(/");
    assert!(w.has("word-broken"));
    w.key(Key::Enter, "");
    assert_eq!(w.settings().highlight_rules.len(), 1);
    // The cross takes a word away.
    w.click("word-0/remove");
    assert!(w.settings().highlight_rules.is_empty() && !w.has("word-0"));
}

#[test]
fn copying_is_a_choice_and_a_switch_under_their_own_heading() {
    let mut w = Window::defaults();
    w.click("server-1");
    w.go(Section::Terminal);
    assert!(w.says("Copying"));
    assert!(w.tree().contains("Copy from the terminal") && w.tree().contains("Programs can copy to my clipboard"));
    assert!(w.says("Agents like OpenCode copy what you select inside them."));
    // As 0.10 came: selecting copies.
    assert!(w.rt.state.settings.copy_on_select() && w.rt.state.options.pane.copy_on_select);
    assert!(w.tree().contains("When I select text"));
    // The pop-up's second choice is 0.10's `copyOnSelect` off.
    w.click("term-copy-control");
    assert!(w.has("popup-menu") && w.tree().contains("Only with Ctrl+Shift+C"));
    w.click("popup-menu/item-1");
    assert!(!w.settings().copy_on_select && !w.rt.state.settings.copy_on_select());
    assert!(!w.rt.state.options.pane.copy_on_select);
    assert!(w.tree().contains("Only with Ctrl+Shift+C"));
    // Whether a program may copy is a switch of its own, on until turned off.
    assert!(w.rt.state.settings.program_clipboard());
    w.click("term-program-clipboard-control");
    assert!(!w.rt.state.settings.program_clipboard());
    // A Mac names its own key.
    let mut mac = Window::open(SettingsStore::in_memory(Settings { copy_on_select: false, ..quiet() }), true, true);
    mac.go(Section::Terminal);
    assert!(mac.tree().contains("Only with ⌘C") && !mac.tree().contains("Only with Ctrl+Shift+C"));
    assert!(mac.says("Agents like OpenCode copy what you select inside them. If that fights with SSHeesh's own copying, choose 'Only with ⌘C' above."));
}

#[test]
fn streamer_mode_is_one_thing_whichever_switch_turns_it() {
    let mut w = Window::defaults();
    w.click("server-1");
    let streaming = |w: &Window| (w.settings().streamer_mode, w.rt.state.streaming(), w.rt.state.panes.values().next().unwrap().view.borrow().look().streaming);
    // The eye in the top row: saved in Settings, and the terminal hides names.
    w.click("streamer");
    assert_eq!(streaming(&w), (true, true, true));
    assert!(!w.tree().contains("web-7731"));
    // Settings' switch shows it on, and turns it off for the eye too.
    w.go(Section::Privacy);
    w.click("privacy-streamer-control");
    assert_eq!(streaming(&w), (false, false, false));
    // The shortcut, with Settings open: on again, and Settings stays where it was.
    w.key(Key::Command('h'), "cs");
    assert_eq!(streaming(&w), (true, true, true));
    assert!(w.rt.state.screens.settings.open && w.rt.state.screens.settings.section == Section::Privacy);
    // The eye's own state is the setting's.
    assert_eq!(w.fill("streamer"), Theme::of(true).selrail.hex());
}

#[test]
fn copying_reaches_every_terminal_through_the_windows_copy_modes() {
    let mut w = Window::defaults();
    w.click("server-1");
    let modes = |w: &Window| {
        let pane = w.rt.state.panes.values().next().unwrap();
        (w.rt.state.options.pane.copy_on_select, w.rt.state.options.pane.program_clipboard, pane.options.copy_on_select, pane.options.program_clipboard)
    };
    assert_eq!(modes(&w), (true, true, true, true));
    w.go(Section::Terminal);
    w.click("term-copy-control");
    w.click("popup-menu/item-1");
    w.click("term-program-clipboard-control");
    assert_eq!(modes(&w), (false, false, false, false));
    // A terminal opened later starts with them too.
    w.click("settings-close");
    w.click("server-2");
    let fresh = w.rt.state.panes.values().filter(|p| !p.options.copy_on_select && !p.options.program_clipboard).count();
    assert_eq!(fresh, 2);
}
