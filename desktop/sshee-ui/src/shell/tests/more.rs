//! What the terminal and the row of tabs gained after the first window: find, links, the
//! scroll bar, "While you were away", the pane's menu, streamer mode, tabs named after folders,
//! the tab menu, dragging tabs, the list of tabs, and two tabs side by side. Same harness as
//! the tests above: fake sessions, real frames, clicks by element key.

use rune::PhysicalInput;
use sshee_app::workbench::TabId;
use sshee_term::scroll::TmuxScroll;

use super::*;
use crate::terminal::overlays::{Answer, Ask, Reply};
use crate::terminal::replay::made_up_secret;

/// A terminal with a prompt and `lines` numbered lines above it, some of them saying "needle".
fn with_history(w: &mut Window, session: usize, lines: usize) {
    let mut out = String::new();
    for n in 1..=lines {
        out.push_str(&match n % 40 {
            7 => format!("line {n} has a needle in it\r\n"),
            _ => format!("line {n}\r\n"),
        });
    }
    out.push_str("web-7731:~$ ");
    w.feed(session, out.as_bytes());
}

impl Window {
    fn pane(&mut self) -> &mut crate::terminal::TermPane {
        self.rt.state.active_pane().expect("a terminal is on screen")
    }

    fn secondary(&mut self, x: f32, y: f32) {
        self.rt.secondary_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.secondary_up(x, y);
        self.settle();
    }

    fn hold(&mut self, mods: Mods) {
        self.rt.physical_input(PhysicalInput::Modifiers(mods));
        self.rt.frame(1.0 / 60.0);
    }

    fn wait(&mut self, seconds: f64) {
        for _ in 0..(seconds * 60.0).ceil() as usize {
            self.rt.frame(1.0 / 60.0);
        }
        self.settle();
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Text(c.to_string()), "");
        }
    }

    /// The top-left of a cell of the terminal on screen, in window points.
    fn cell(&mut self, line: usize, col: u16) -> (f32, f32) {
        let [x, y, ..] = self.rect("term").expect("a terminal is on screen");
        let m = self.pane().metrics.get().expect("the terminal was drawn");
        (x + 12.0 + f32::from(col) * m.cell_w + m.cell_w / 2.0, y + 12.0 + line as f32 * m.cell_h + m.cell_h / 2.0)
    }

    /// The view line whose text starts with `start`, on the terminal on screen.
    fn line_of(&mut self, start: &str) -> usize {
        let rows = self.pane().size().rows as usize;
        let mut view = self.pane().view.borrow_mut();
        (0..rows).find(|l| view.shown(*l).is_some_and(|r| r.text.starts_with(start))).unwrap_or_else(|| panic!("no line starts with {start:?}"))
    }
}

const CTRL: Mods = Mods { ctrl: true, cmd: !cfg!(target_os = "macos"), shift: false, alt: false, logo: false };

#[test]
fn find_marks_counts_steps_scrolls_and_closes() {
    let mut w = linux();
    w.click("server-1");
    with_history(&mut w, 0, 200);
    assert!(w.rt.find("find").is_none());
    w.key(Key::Command('f'), "cs");
    assert!(w.rt.find("find").is_some() && w.pane().find_is_open());
    w.typed("needle");
    // Five lines say it (7, 47, 87, 127, 167): the bar starts on the newest.
    assert!(w.tree().contains("5 of 5"), "{}", w.tree());
    // Nothing typed into the bar reached the shell.
    assert_eq!(w.written(0), "");
    // Return: the older one, which is above the screen, so the view goes there.
    let before = w.pane().scroll_view();
    w.key(Key::Enter, "");
    assert!(w.tree().contains("4 of 5"));
    let after = w.pane().scroll_view();
    assert!(after.top < before.top, "the view didn't go to the older match: {before:?} {after:?}");
    let line = w.line_of("line 127 has a needle");
    // It's marked as the one the bar is on, and the others on screen as matches.
    let shown = w.pane().view.borrow_mut().shown(line).unwrap();
    assert!(shown.spans.iter().any(|s| s.role == sshee_term::present::Role::Found { current: true }), "{:?}", shown.spans);
    // Shift+Return: back to the newer; around the end it wraps.
    w.key(Key::Enter, "s");
    assert!(w.tree().contains("5 of 5"));
    w.key(Key::Enter, "s");
    assert!(w.tree().contains("1 of 5"));
    // Nothing to find says so.
    w.typed("zz");
    assert!(w.tree().contains("No results"), "{}", w.tree());
    // Escape closes it, forgets what was typed, and the keyboard is the shell's again.
    w.key(Key::Escape, "");
    assert!(w.rt.find("find").is_none());
    w.typed("ls");
    assert_eq!(w.written(0), "ls");
    w.key(Key::Command('f'), "cs");
    assert!(!w.tree().contains("needle"), "the old query came back");
}

#[test]
fn a_hidden_secret_is_never_found() {
    let mut w = linux();
    w.click("server-1");
    let secret = made_up_secret(7, 0);
    w.feed(0, format!("export GH_TOKEN={secret}\r\nweb-7731:~$ ").as_bytes());
    w.key(Key::Command('f'), "cs");
    w.typed("GH_TOKEN");
    assert!(w.tree().contains("1 of 1"), "{}", w.tree());
    // A piece of the secret itself: not there to find, and nothing of it in the tree.
    for _ in 0.."GH_TOKEN".len() {
        w.key(Key::Backspace, "");
    }
    w.typed(&secret[6..14]);
    assert!(w.tree().contains("No results"), "{}", w.tree());
    w.key(Key::Escape, "");
    assert!(!w.tree().contains(&secret[6..14]));
}

#[test]
fn ctrl_f_is_the_find_bar_at_a_prompt_and_the_programs_on_its_own_screen() {
    let mut w = linux();
    w.click("server-1");
    w.feed(0, b"web-7731:~$ ");
    w.key(Key::Command('f'), "c");
    assert!(w.rt.find("find").is_some());
    w.key(Key::Escape, "");
    // vim, less: the second screen. Ctrl+F is theirs (page down); Ctrl+Shift+F still finds.
    w.feed(0, b"\x1b[?1049h");
    w.key(Key::Command('f'), "c");
    assert!(w.rt.find("find").is_none());
    assert_eq!(w.written(0), "\u{6}");
    w.key(Key::Command('f'), "cs");
    assert!(w.rt.find("find").is_some());
}

#[test]
fn a_link_is_underlined_under_the_pointer_and_opens_with_ctrl_click() {
    let mut w = linux();
    w.click("server-1");
    w.feed(0, b"see https://example.com/docs for more\r\nedit /etc/hosts or src/main.rs:12\r\nweb-7731:~$ ");
    // Over the web link: it's the one hovered, underlined with a line.
    let (x, y) = w.cell(0, 8);
    w.rt.pointer_move(x, y);
    w.settle();
    let hover = w.pane().over.links.hover.clone().expect("the link under the pointer");
    assert_eq!(hover.link.target, "https://example.com/docs");
    assert_eq!(hover.parts, [(0, 4..28)]);
    assert!(!hover.dotted());
    // The hint waits for the pointer to rest.
    assert!(w.rt.find("link-hint").is_none());
    w.wait(0.6);
    assert!(w.rt.find("link-hint").is_some());
    assert!(w.tree().contains("Ctrl+click to open"), "{}", w.tree());
    // A plain click selects as ever and opens nothing; off the link nothing is underlined.
    let (x, y) = w.cell(0, 1);
    w.rt.pointer_move(x, y);
    w.settle();
    assert!(w.pane().over.links.hover.is_none());
    // A path: dotted, and Ctrl+click asks the window for it. Files isn't there yet, so the
    // path is copied and the terminal says so. Nothing reaches the shell, nothing is selected.
    let (x, y) = w.cell(1, 7);
    w.rt.pointer_move(x, y);
    w.settle();
    assert!(w.pane().over.links.hover.as_ref().is_some_and(|h| h.dotted() && h.link.target == "/etc/hosts"));
    w.hold(CTRL);
    w.rt.pointer_down(x, y);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(x, y);
    w.settle();
    assert!(w.tree().contains("Copied the path"), "{}", w.tree());
    assert_eq!(w.written(0), "");
    assert!(w.pane().view.borrow().selection.is_none());
    w.hold(Mods::default());
    // The note goes by itself.
    w.wait(3.0);
    assert!(!w.tree().contains("Copied the path"));
}

#[test]
fn the_scroll_bar_follows_the_history_and_new_output_offers_the_bottom() {
    let mut w = linux();
    w.click("server-1");
    w.feed(0, b"web-7731:~$ ");
    assert!(w.rt.find("scroll").is_none(), "a scroll bar with nothing to scroll");
    with_history(&mut w, 0, 200);
    assert!(w.rt.find("scroll").is_some());
    let bottom = w.pane().scroll_view();
    assert_eq!(bottom.top + bottom.rows, bottom.total);
    // Shift+Page Up: a screen up.
    w.key(Key::PageUp, "s");
    let up = w.pane().scroll_view();
    assert_eq!(up.top + up.rows, bottom.top);
    assert!(w.rt.find("to-bottom").is_none(), "nothing new was printed yet");
    // The thumb moved up with it.
    let [_, track_y, _, _] = w.rect("scroll").unwrap();
    let [_, thumb_y, _, thumb_h] = w.rect("thumb").unwrap();
    assert!(thumb_y > track_y && thumb_y + thumb_h < track_y + 716.0 - 1.0, "{thumb_y} {thumb_h}");
    // New output while scrolled up: the view stays, and the way back down is offered.
    w.feed(0, b"\r\nnews\r\nweb-7731:~$ ");
    assert!(w.rt.find("to-bottom").is_some());
    assert!(w.tree().contains("Jump to the bottom"));
    w.click("to-bottom");
    let v = w.pane().scroll_view();
    assert_eq!(v.top + v.rows, v.total);
    assert!(w.rt.find("to-bottom").is_none());
    // A press on the track above the thumb goes a screen up.
    let [x, y, tw, _] = w.rect("scroll").unwrap();
    w.rt.pointer_down(x + tw / 2.0, y + 4.0);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(x + tw / 2.0, y + 4.0);
    w.settle();
    let v2 = w.pane().scroll_view();
    assert_eq!(v2.top + v2.rows - 1, v.top);
    // Typing goes to the prompt at the bottom.
    w.typed("x");
    let v3 = w.pane().scroll_view();
    assert_eq!(v3.top + v3.rows, v3.total);
}

/// What a fake tmux session was asked, and what it answers with.
#[derive(Default)]
struct Tmux {
    asks: Vec<Ask>,
    rows: String,
    history: Option<String>,
    /// What the pane copied to the clipboard, in order.
    copies: Vec<String>,
}

/// A session in tmux on a server: it answers the pane's asks at once.
struct TmuxLink {
    seen: Arc<Mutex<Seen>>,
    tmux: Arc<Mutex<Tmux>>,
}

impl TermLink for TmuxLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        let mut seen = self.seen.lock().unwrap();
        model.connected(true, 1_000);
        seen.model = Some(model);
        seen.size = Some(size);
        status(LinkStatus::Connected { durable: true });
        seen.status = Some(status);
    }
    fn write(&self, bytes: &[u8]) {
        self.seen.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
    fn note_copy(&self, text: &str) {
        self.tmux.lock().unwrap().copies.push(text.to_owned());
    }
    fn ask(&self, ask: Ask, done: Answer) -> bool {
        let mut tmux = self.tmux.lock().unwrap();
        tmux.asks.push(ask);
        done(match ask {
            Ask::Rows => Some(Reply::Rows(tmux.rows.clone())),
            Ask::History { .. } => tmux.history.clone().map(|text| Reply::History { text, away_ms: Some(41 * 60_000) }),
            Ask::Scroll { position } => Some(Reply::Scroll(TmuxScroll { in_mode: position.is_some_and(|p| p > 0), position: position.unwrap_or(0), history: 480, height: 33, limit: 2000 })),
        });
        true
    }
}

struct TmuxBackend(Arc<Mutex<World>>, Arc<Mutex<Tmux>>);

impl Backend for TmuxBackend {
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.0.lock().unwrap().sessions.push((tab.server.clone(), seen.clone()));
        Box::new(TmuxLink { seen, tmux: self.1.clone() })
    }
    fn replays(&self) -> bool {
        true
    }
}

/// A window whose servers have tmux.
fn with_tmux() -> (Window, Arc<Mutex<Tmux>>) {
    crate::fonts::declare();
    let world = Arc::new(Mutex::new(World::default()));
    let tmux = Arc::new(Mutex::new(Tmux::default()));
    let posted: Queue = Arc::new(Mutex::new(Vec::new()));
    let queue = posted.clone();
    let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
    let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
    let shell = Shell::new(Workbench::new(servers()), Box::new(TmuxBackend(world.clone(), tmux.clone())), options, Some(Remote::new(post)));
    let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
    let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
    let mut w = Window { rt, world, posted };
    w.settle();
    (w, tmux)
}

#[test]
fn in_tmux_the_history_found_and_scrolled_is_tmuxs() {
    let (mut w, tmux) = with_tmux();
    w.click("server-1");
    // tmux reports its history in the title: 480 lines above a 33-row screen.
    w.feed(0, b"\x1b]2;sshee-scroll 0 480 33 2000\x07web-7731:~$ ");
    let v = w.pane().scroll_view();
    assert_eq!((v.total, v.top, v.rows), (513, 480, 33));
    assert!(w.rt.find("scroll").is_some());
    // A find reads tmux's rows from the server, once, and counts what is far above the screen.
    let mut rows = String::new();
    for n in 0..513 {
        rows.push_str(if n == 10 || n == 500 { "the needle\n" } else { "hay\n" });
    }
    tmux.lock().unwrap().rows = rows;
    w.key(Key::Command('f'), "cs");
    w.typed("needle");
    assert!(w.tree().contains("2 of 2"), "{}", w.tree());
    assert_eq!(tmux.lock().unwrap().asks, [Ask::Rows]);
    // The older match is 502 rows up from the bottom: tmux is asked to scroll there.
    w.key(Key::Enter, "");
    assert!(w.tree().contains("1 of 2"));
    let asks = tmux.lock().unwrap().asks.clone();
    assert!(matches!(asks.last(), Some(Ask::Scroll { position: Some(p) }) if (470..=502).contains(p)), "{asks:?}");
    let v = w.pane().scroll_view();
    assert!(v.top < 40, "{v:?}");
    // Ctrl+F alone stays the program's until something says which program is in front.
    w.key(Key::Escape, "");
    w.key(Key::Command('f'), "c");
    assert!(w.rt.find("find").is_none());
    w.pane().set_foreground(Some("bash".into()));
    w.key(Key::Command('f'), "c");
    assert!(w.rt.find("find").is_some());
    w.key(Key::Escape, "");
    w.pane().set_foreground(Some("vim".into()));
    w.key(Key::Command('f'), "c");
    assert!(w.rt.find("find").is_none());
}

#[test]
fn back_from_a_dropped_connection_a_line_says_what_was_printed_meanwhile() {
    let (mut w, tmux) = with_tmux();
    w.click("server-1");
    w.feed(0, b"web-7731:~$ make deploy\r\nbuilding\r\npushing\r\n");
    let status = w.session(0).lock().unwrap().status.clone().unwrap();
    status(LinkStatus::Reconnecting { note: "reconnecting (try 1)".into() });
    w.settle();
    assert!(w.rt.find("away").is_none());
    // Back: tmux redraws, and its history has two more lines than the screen had.
    tmux.lock().unwrap().history = Some("web-7731:~$ make deploy\nbuilding\npushing\nERROR: the registry refused the image\ndone in 41 min\n".into());
    status(LinkStatus::Connected { durable: true });
    w.settle();
    w.feed(0, b"ERROR: the registry refused the image\r\ndone in 41 min\r\nweb-7731:~$ ");
    assert!(w.rt.find("away").is_none(), "the history is read once tmux has had a moment to redraw");
    w.wait(0.8);
    assert!(w.tree().contains("While you were away (41 min): 2 new lines, 1 error"), "{}", w.tree());
    // The line sits above the first new line, and the rows under it moved down to make room.
    let line = w.line_of("ERROR: the registry");
    assert_eq!(w.pane().over.away.shown.as_ref().and_then(|s| s.above), Some(line));
    let [_, away_y, _, away_h] = w.rect("away").unwrap();
    let [_, below_y, ..] = w.rect("grid-below").unwrap();
    let [_, above_y, _, above_h] = w.rect("grid-above").unwrap();
    assert_eq!(away_y + away_h, below_y);
    assert_eq!(above_y + above_h, away_y);
    // A click on a cell under the line still lands on that cell.
    let (x, y) = w.cell(line, 0);
    let m = w.pane().metrics.get().unwrap();
    let inset = w.pane().inset(&m).unwrap();
    let drawn = y + inset.down;
    let [tx, ty, ..] = w.rect("term").unwrap();
    assert_eq!(w.pane().cell_at(x - tx, drawn - ty).map(|c| c.0), Some(line));
    // Its × takes it away, and the rows go back.
    w.click("away-close");
    assert!(w.rt.find("away").is_none() && w.rt.find("grid-below").is_none());
}

#[test]
fn the_panes_menu_copies_finds_clears_and_opens_a_terminal_here() {
    let mut w = linux();
    w.click("server-1");
    with_history(&mut w, 0, 60);
    let (x, y) = w.cell(5, 20);
    w.secondary(x, y);
    assert!(w.rt.find("menu").is_some());
    let tree = w.tree();
    for row in ["Copy", "Paste", "Select all", "Find", "Clear", "New terminal here"] {
        assert!(tree.contains(row), "{row} isn't in the menu: {tree}");
    }
    // Find.
    w.click("menu-find");
    assert!(w.rt.find("menu").is_none() && w.rt.find("find").is_some());
    w.key(Key::Escape, "");
    // Clear: the history goes.
    assert!(w.rt.find("scroll").is_some());
    w.secondary(x, y);
    w.click("menu-clear");
    assert!(w.rt.find("scroll").is_none(), "the history is still there");
    // New terminal here: on this machine, next to this tab.
    w.secondary(x, y);
    w.click("menu-new");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    // Escape closes the menu without doing anything.
    let (x, y) = w.cell(2, 2);
    w.secondary(x, y);
    assert!(w.rt.find("menu").is_some());
    w.key(Key::Escape, "");
    assert!(w.rt.find("menu").is_none());
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    // A program that asked for the mouse gets the secondary button; Shift opens the menu.
    w.feed(1, b"\x1b[?1000h\x1b[?1006h");
    w.secondary(x, y);
    assert!(w.rt.find("menu").is_none());
    assert!(w.written(1).contains("\u{1b}[<2;"), "{:?}", w.written(1));
}

#[test]
fn streamer_mode_hides_names_everywhere_at_once_and_shows_them_again() {
    let mut w = linux();
    w.click("server-1");
    let secret = made_up_secret(3, 0);
    w.feed(0, format!("deploy@web-7731:~$ ssh postgres@db-main\r\nTOKEN={secret}\r\n").as_bytes());
    w.click("server-2");
    w.click("server-1");
    let before = w.tree();
    assert!(before.contains("web-7731") && before.contains("db-main") && before.contains("deploy@web-7731:~$ ssh postgres@db-main"));
    // The eye's shortcut.
    w.key(Key::Command('h'), "cs");
    assert!(w.rt.state.streaming());
    let tree = w.tree();
    for name in ["web-7731", "db-main", "postgres", "deploy", "203.0.113"] {
        assert!(!tree.contains(name), "{name} still shows: {tree}");
    }
    // Servers are numbered instead, in the title, the tabs and the sidebar's tooltips.
    assert!(tree.contains("Server 1") && tree.contains("Server 2") && tree.contains("Terminal, Server 2"), "{tree}");
    assert!(tree.contains("This computer"));
    // The terminal's text is still there, with the names dotted, and nothing was cleared.
    assert!(tree.contains("••••••@••••••••:~$ ssh ••••••••@•••••••"), "{tree}");
    // A secret can't be clicked open while streaming.
    let line = w.line_of("TOKEN=");
    let (x, y) = w.cell(line, 10);
    w.rt.pointer_down(x, y);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(x, y);
    w.settle();
    assert!(!w.tree().contains(&secret[4..12]));
    // A find doesn't find a hidden name.
    w.key(Key::Command('f'), "cs");
    w.typed("db-main");
    assert!(w.tree().contains("No results"), "{}", w.tree());
    w.key(Key::Escape, "");
    // A terminal opened while streaming is hidden from its first frame.
    w.click("new-tab");
    w.feed(2, b"deploy@web-7731:~$ ");
    assert!(!w.tree().contains("deploy@web-7731"));
    // The eye again: everything is back.
    w.click("streamer");
    assert!(!w.rt.state.streaming());
    let tree = w.tree();
    assert!(tree.contains("web-7731") && tree.contains("deploy@web-7731:~$"), "{tree}");
}

#[test]
fn a_terminal_tab_is_named_after_the_shells_folder() {
    let mut w = linux();
    w.click("server-1");
    assert_eq!(w.row(), "*Terminal@web-7731");
    // OSC 7: the folder. The server's user is deploy, so its home is a tilde.
    w.feed(0, b"\x1b]7;file://web-7731/home/deploy/shop\x07deploy@web-7731:~/shop$ ");
    assert_eq!(w.row(), "*~/shop@web-7731");
    // The title a shell sets says it too.
    w.feed(0, b"\x1b]0;deploy@web-7731: /etc/nginx\x07");
    assert_eq!(w.row(), "*/etc/nginx@web-7731");
    // Another program's title names nothing.
    w.feed(0, b"\x1b]0;vim notes.md\x07");
    assert_eq!(w.row(), "*/etc/nginx@web-7731");
    // A tab something else named keeps its name.
    let id = w.rt.state.bench.active_id().unwrap();
    w.rt.update(|s, _| s.bench.set_title(id, Some("Migration 0042".into())));
    w.feed(0, b"\x1b]7;file://web-7731/tmp\x07");
    assert_eq!(w.row(), "*Migration 0042@web-7731");
}

#[test]
fn a_tabs_menu_closes_duplicates_and_shows_side_by_side() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    w.click("new-tab");
    w.click("server-2");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 Terminal 3@web-7731 *Terminal@db-main");
    let ids: Vec<u64> = w.rt.state.bench.tabs().iter().map(|t| t.id.0).collect();
    let menu = |w: &mut Window, n: usize| {
        let (x, y) = w.rt.find(&format!("tab-{}", ids[n])).unwrap();
        w.secondary(x, y);
        assert!(w.rt.find("tab-menu").is_some(), "no menu for tab {n}");
    };
    menu(&mut w, 1);
    let tree = w.tree();
    for row in ["Show side by side", "Duplicate tab", "Close tab", "Close other tabs", "Close tabs to the right"] {
        assert!(tree.contains(row), "{row}: {tree}");
    }
    // Duplicate: another terminal of that machine, right after it.
    w.click("tab-duplicate");
    assert!(w.rt.find("tab-menu").is_none());
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal 4@web-7731 Terminal 3@web-7731 Terminal@db-main");
    // Close tabs to the right of the second.
    menu(&mut w, 1);
    w.click("tab-close-right");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    // Close the others.
    menu(&mut w, 1);
    w.click("tab-close-others");
    assert_eq!(w.row(), "*Terminal 2@web-7731");
    // Every closed tab's session ended.
    let ended = (0..w.sessions()).filter(|n| w.session(*n).lock().unwrap().ended).count();
    assert_eq!(ended, w.sessions() - 1);
    // Escape closes the menu.
    menu(&mut w, 1);
    w.key(Key::Escape, "");
    assert!(w.rt.find("tab-menu").is_none());
    assert_eq!(w.row(), "*Terminal 2@web-7731");
}

#[test]
fn a_tab_is_dragged_to_another_place_in_the_row() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    w.click("server-2");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal@db-main");
    let ids: Vec<u64> = w.rt.state.bench.tabs().iter().map(|t| t.id.0).collect();
    let centre = |w: &Window, n: usize| {
        let [x, y, tw, th] = w.rect(&format!("tab-{}", ids[n])).unwrap();
        (x + tw / 2.0, y + th / 2.0)
    };
    // The first tab, dragged past the middle of the second: they swap.
    let (x0, y0) = centre(&w, 0);
    let (x1, _) = centre(&w, 1);
    w.rt.pointer_down(x0, y0);
    w.rt.frame(1.0 / 60.0);
    for step in 1..=8 {
        w.rt.pointer_move(x0 + (x1 + 12.0 - x0) * step as f32 / 8.0, y0);
        w.rt.frame(1.0 / 60.0);
    }
    w.rt.pointer_up(x1 + 12.0, y0);
    w.settle();
    assert_eq!(w.row(), "Terminal 2@web-7731 *Terminal@web-7731 Terminal@db-main");
    // Across machines: db-main's tab dragged to the front of the row.
    let (x2, y2) = centre(&w, 2);
    w.rt.pointer_down(x2, y2);
    w.rt.frame(1.0 / 60.0);
    let first = w.rect(&format!("tab-{}", ids[1])).unwrap()[0];
    for step in 1..=8 {
        w.rt.pointer_move(x2 + (first + 2.0 - x2) * step as f32 / 8.0, y2);
        w.rt.frame(1.0 / 60.0);
    }
    w.rt.pointer_up(first + 2.0, y2);
    w.settle();
    assert_eq!(w.row(), "*Terminal@db-main Terminal 2@web-7731 Terminal@web-7731");
    // A click without a drag only goes to the tab.
    w.click(&format!("tab-{}", ids[1]));
    assert_eq!(w.row(), "Terminal@db-main *Terminal 2@web-7731 Terminal@web-7731");
}

#[test]
fn the_row_keeps_still_under_the_pointer() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    w.click("new-tab");
    let ids: Vec<TabId> = w.rt.state.bench.tabs().iter().map(|t| t.id).collect();
    let x_of = |w: &Window, n: usize| w.rect(&format!("tab-{}", ids[n].0)).unwrap();
    let before = (x_of(&w, 1), x_of(&w, 2));
    // The pointer comes over the row; then the first tab's shell renames it to something long.
    let (x, y) = w.rt.find(&format!("tab-{}", ids[1].0)).unwrap();
    w.rt.pointer_move(x, y);
    w.settle();
    let first = ids[0];
    w.rt.update(|s, _| s.bench.set_title(first, Some("~/a/much/longer/folder/name".into())));
    w.settle();
    assert_eq!((x_of(&w, 1), x_of(&w, 2)), before, "the row moved under the pointer");
    // The pointer leaves: now the row makes room for the name.
    w.rt.pointer_move(700.0, 400.0);
    w.settle();
    assert!(x_of(&w, 1)[0] > before.0[0] + 20.0);
}

#[test]
fn tabs_that_do_not_fit_are_in_the_list_and_one_that_waits_shows_there() {
    let mut w = linux();
    w.click("server-1");
    assert!(w.rt.find("tab-list").is_none());
    for _ in 0..24 {
        w.rt.update(|s, tx| s.new_terminal(tx));
    }
    w.settle();
    // The row can't hold 25 tabs: a run of them with the tab on screen in it, and the list.
    assert!(w.rt.find("tab-list").is_some());
    let tabs = w.rt.state.bench.tabs().to_vec();
    let drawn: Vec<bool> = tabs.iter().map(|t| w.rt.find(&format!("tab-{}", t.id.0)).is_some()).collect();
    assert!(drawn[24] && !drawn[0], "{drawn:?}");
    for t in tabs.iter().filter(|t| w.rt.find(&format!("tab-{}", t.id.0)).is_some()) {
        let [x, _, tw, _] = w.rect(&format!("tab-{}", t.id.0)).unwrap();
        assert!(x + tw <= 1280.0 && tw >= 60.0, "tab {} at {x} is {tw} wide", t.id.0);
    }
    assert!(w.rt.find("new-tab").is_some_and(|(x, _)| x < 1280.0));
    // The first tab's program rings: it's out of sight, so the list's button says so.
    assert!(w.rt.find("tab-list-waits").is_none());
    w.feed(0, b"\x07");
    assert!(w.rt.find("tab-list-waits").is_some());
    // The list names every tab; a pick goes to it, and the row moves to show it.
    w.click("tab-list");
    assert!(w.rt.find("tab-list-menu").is_some());
    assert!(w.tree().contains("Terminal, web-7731, Needs you"), "{}", w.tree());
    w.click(&format!("list-{}", tabs[0].id.0));
    assert!(w.rt.find("tab-list-menu").is_none());
    assert_eq!(w.rt.state.bench.active_id(), Some(tabs[0].id));
    assert!(w.rt.find(&format!("tab-{}", tabs[0].id.0)).is_some());
    assert!(w.rt.find("tab-list-waits").is_none());
}

#[test]
fn two_tabs_side_by_side_share_the_page_and_come_apart_without_closing() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    let ids: Vec<TabId> = w.rt.state.bench.tabs().iter().map(|t| t.id).collect();
    let pane = |n: usize| format!("pane-{}", ids[n].0);
    assert!(w.rt.find("side").is_none());
    // The shortcut: the tab on screen (the second) beside its neighbour.
    w.key(Key::Command('s'), "cs");
    assert!(w.rt.find("side").is_some());
    let (left, right) = (w.rect(&pane(1)).unwrap(), w.rect(&pane(0)).unwrap());
    assert!(left[0] < right[0] && (left[2] - right[2]).abs() < 12.0, "{left:?} {right:?}");
    assert_eq!(w.sessions(), 2);
    // Both are drawn; keys go to the tab you're on.
    w.typed("a");
    assert_eq!((w.written(0), w.written(1)), (String::new(), "a".into()));
    // A press on the other side makes it the tab you're on.
    let (x, y) = (right[0] + right[2] / 2.0, right[1] + right[3] / 2.0);
    w.rt.pointer_down(x, y);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(x, y);
    w.settle();
    assert_eq!(w.rt.state.bench.active_id(), Some(ids[0]));
    assert!(w.rt.find("side").is_some(), "the pair came apart on a click");
    w.typed("b");
    assert_eq!((w.written(0), w.written(1)), ("b".into(), "a".into()));
    // The divider, dragged left: the left side narrows, as far as it may.
    let [dx, dy, dw, dh] = w.rect("side-divider").unwrap();
    w.rt.pointer_down(dx + dw / 2.0, dy + dh / 2.0);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_move(dx - 200.0, dy + dh / 2.0);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(dx - 200.0, dy + dh / 2.0);
    w.settle();
    let narrowed = w.rect(&pane(1)).unwrap();
    assert!((narrowed[2] - (left[2] - 200.0)).abs() < 8.0, "{narrowed:?} from {left:?}");
    w.rt.pointer_down(dx - 200.0, dy + dh / 2.0);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_move(60.0, dy + dh / 2.0);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(60.0, dy + dh / 2.0);
    w.settle();
    assert!(w.rect(&pane(1)).unwrap()[2] > 200.0, "a side was squeezed away");
    // A side's ×: one at a time again, and both tabs are still open and running.
    w.click(&format!("side-close-{}", ids[1].0));
    assert!(w.rt.find("side").is_none());
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal 2@web-7731");
    assert!((0..2).all(|n| !w.session(n).lock().unwrap().ended));
    // With one tab on the machine the shortcut opens a terminal beside it; closing a side's
    // tab ends the pair.
    w.click("server-2");
    w.key(Key::Command('s'), "cs");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal@db-main Terminal 2@db-main");
    assert!(w.rt.find("side").is_some());
    w.key(Key::Command('w'), "cs");
    assert!(w.rt.find("side").is_none());
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal 2@db-main");
}

const SHIFT: Mods = Mods { shift: true, ctrl: false, cmd: false, alt: false, logo: false };

impl Window {
    /// A press at `from`, a drag to `to` and a release, with whatever modifiers are held.
    fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
        self.rt.pointer_down(from.0, from.1);
        self.rt.frame(1.0 / 60.0);
        for step in 1..=4 {
            let k = step as f32 / 4.0;
            self.rt.pointer_move(from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            self.rt.frame(1.0 / 60.0);
        }
        self.rt.pointer_up(to.0, to.1);
        self.settle();
    }

    /// Drags over the first `chars` characters of view line `line`: from the left half of the
    /// first cell to the right half of the last.
    fn select(&mut self, line: usize, chars: u16) {
        let (x0, y) = self.cell(line, 0);
        let (x1, _) = self.cell(line, chars - 1);
        self.drag((x0 - 3.0, y), (x1 + 3.0, y));
    }
}

/// What reached the clipboard from the first terminal of a [`with_tmux`] window.
fn copies(tmux: &Arc<Mutex<Tmux>>) -> Vec<String> {
    tmux.lock().unwrap().copies.clone()
}

#[test]
fn with_copy_on_select_off_only_the_copy_key_and_the_menu_copy() {
    let (mut w, tmux) = with_tmux();
    w.click("server-1");
    w.feed(0, b"hello world\r\nweb-7731:~$ ");
    // On, as it comes: a selection that rests is copied, once.
    w.select(0, 5);
    assert!(copies(&tmux).is_empty(), "copied before the selection rested");
    w.wait(0.4);
    assert_eq!(copies(&tmux), ["hello"]);
    // Off: selecting never touches the clipboard.
    w.rt.update(|s, _| s.set_copy_modes(false, true));
    w.select(0, 11);
    w.wait(0.6);
    assert_eq!(copies(&tmux), ["hello"]);
    assert!(w.pane().view.borrow().selection.is_some());
    // Ctrl+Shift+C copies what is selected, and so does the menu.
    w.key(Key::Command('c'), "cs");
    assert_eq!(copies(&tmux), ["hello", "hello world"]);
    let (x, y) = w.cell(1, 30);
    w.secondary(x, y);
    w.click("menu-copy");
    assert_eq!(copies(&tmux), ["hello", "hello world", "hello world"]);
    // Nothing of this was typed into the shell.
    assert_eq!(w.written(0), "");
    // A terminal opened later has the setting too.
    w.click("new-tab");
    assert!(!w.pane().options.copy_on_select && w.pane().options.program_clipboard);
}

#[test]
fn a_program_with_the_mouse_gets_the_drag_and_shift_selects() {
    let (mut w, tmux) = with_tmux();
    w.click("server-1");
    w.feed(0, b"hello world\r\n\x1b[?1002h\x1b[?1006h");
    // The program asked for the mouse: a drag is its own (press, moves, release), and
    // nothing is selected here, so nothing is copied here.
    w.select(0, 5);
    w.wait(0.4);
    let reports = w.written(0);
    assert!(reports.starts_with("\u{1b}[<0;1;1M") && reports.ends_with("\u{1b}[<0;5;1m"), "{reports:?}");
    assert!(w.pane().view.borrow().selection.is_none());
    assert!(copies(&tmux).is_empty());
    // Shift makes the drag a selection of the terminal's own: nothing is reported, and (copy
    // on select being on) it is copied once.
    w.hold(SHIFT);
    w.select(0, 5);
    w.hold(Mods::default());
    w.wait(0.4);
    assert_eq!(w.written(0), reports, "a Shift+drag reached the program");
    assert!(w.pane().view.borrow().selection.is_some());
    assert_eq!(copies(&tmux), ["hello"]);
}

#[test]
fn a_program_that_copies_its_own_selection_writes_the_clipboard_once_or_not_at_all() {
    // As OpenCode does: it takes the mouse, selects in its own way, and sends what was
    // selected to the clipboard itself (OSC 52) when the button comes up.
    let osc52 = b"\x1b]52;c;cGlja2Vk\x07";
    let (mut w, tmux) = with_tmux();
    w.click("server-1");
    w.feed(0, b"hello world\r\n\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    w.select(0, 5);
    w.feed(0, osc52);
    w.wait(0.6);
    // One write: the program's. The terminal's own copy on select never ran.
    assert_eq!(copies(&tmux), ["picked"]);
    // Programs may not write the clipboard: the same again writes nothing at all.
    w.rt.update(|s, _| s.set_copy_modes(true, false));
    w.select(0, 5);
    w.feed(0, osc52);
    w.wait(0.6);
    assert_eq!(copies(&tmux), ["picked"]);
    // And with both off, still nothing, while the copy key works on a Shift selection.
    w.rt.update(|s, _| s.set_copy_modes(false, false));
    w.hold(SHIFT);
    w.select(0, 5);
    w.hold(Mods::default());
    w.feed(0, osc52);
    w.wait(0.6);
    assert_eq!(copies(&tmux), ["picked"]);
    w.key(Key::Command('c'), "cs");
    assert_eq!(copies(&tmux), ["picked", "hello"]);
}
