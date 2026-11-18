//! What 0.10 had around a terminal and 1.0 gained from it: "Browse files in this folder",
//! "Not kept", a program's progress, a sleeping tab's moon, a terminal opened from Files beside
//! it, paths opened at their lines, and files dropped on a server's terminal asking where they
//! go. Same harness as the tests above.

use std::path::PathBuf;

use sshee_app::files::{Answer, Clock, Entry, EntryKind, Listing, Reply, Stamp, TextFile, Want};
use sshee_app::workbench::TabId;

use super::*;
use crate::terminal::overlays::{Answer as LinkAnswer, Ask, Reply as LinkReply};

/// A session in tmux on a server whose shell is in `cwd`: tmux answers where it is.
struct TmuxIn {
    seen: Arc<Mutex<Seen>>,
    cwd: String,
}

impl TermLink for TmuxIn {
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
    fn ask(&self, ask: Ask, done: LinkAnswer) -> bool {
        done(match ask {
            Ask::Cwd => Some(LinkReply::Cwd(self.cwd.clone())),
            _ => None,
        });
        true
    }
}

struct TmuxBackend(Arc<Mutex<World>>);

impl Backend for TmuxBackend {
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.0.lock().unwrap().sessions.push((tab.server.clone(), seen.clone()));
        Box::new(TmuxIn { seen, cwd: "/srv/www".into() })
    }
    fn files(&mut self, ask: sshee_app::files::Ask) {
        self.0.lock().unwrap().files.push(ask);
    }
    fn replays(&self) -> bool {
        true
    }
}

/// A window whose servers have tmux, every shell in /srv/www.
fn in_tmux() -> Window {
    crate::fonts::declare();
    let world = Arc::new(Mutex::new(World::default()));
    let posted: Queue = Arc::new(Mutex::new(Vec::new()));
    let queue = posted.clone();
    let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
    let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
    let shell = Shell::new(Workbench::new(servers()), Box::new(TmuxBackend(world.clone())), options, Some(Remote::new(post)));
    let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
    let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
    let mut w = Window { rt, world, posted };
    w.settle();
    w
}

impl Window {
    fn secondary_at(&mut self, key: &str) {
        let [x, y, w, h] = self.rect(key).unwrap_or_else(|| panic!("no element {key}"));
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        self.rt.secondary_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.secondary_up(x, y);
        self.settle();
    }

    /// The tab on screen.
    fn shown_tab(&self) -> &Tab {
        self.rt.state.bench.active().expect("a tab on screen")
    }

    /// The folder the Files tab `tab` shows, or is on its way to.
    fn files_at(&self, tab: TabId) -> String {
        self.rt.state.screens.files.get(&tab).map(|st| st.browser.asked().to_owned()).expect("Files was made")
    }

    /// What Files tabs asked the backend for since the last look.
    fn wants(&self) -> Vec<Want> {
        std::mem::take(&mut self.world.lock().unwrap().files).into_iter().map(|a| a.want).collect()
    }

    /// Files tab `tab` listed `path` (the last folder it asked for) with these files in it.
    fn list(&mut self, tab: TabId, path: &str, names: &[&str]) {
        let Some((ticket, asked)) = self.wants().into_iter().rev().find_map(|w| match w {
            Want::List { ticket, path } => Some((ticket, path)),
            _ => None,
        }) else {
            panic!("Files asked for no folder")
        };
        let entries = names.iter().map(|n| Entry { name: (*n).into(), kind: EntryKind::File, is_link: false, size: 100, modified: 1_700_000_000, mode: "-rw-r--r--".into() }).collect();
        let listing = Listing { path: path.into(), entries };
        self.rt.update(|s, _| crate::screens::files::deliver(s, Reply { tab, server: WEB.into(), answer: Answer::Listed { ticket, asked, result: Ok((listing, Clock::default())) } }));
        self.settle();
    }

    /// Files tab `tab` read the file it asked for: `text`.
    fn read(&mut self, tab: TabId, text: &str) {
        let Some((ticket, path)) = self.wants().into_iter().find_map(|w| match w {
            Want::Read { ticket, path } => Some((ticket, path)),
            _ => None,
        }) else {
            panic!("Files read no file")
        };
        let file = TextFile { path: path.clone(), content: Some(text.into()), binary: false, size: text.len() as u64, lossy: false, crlf: false };
        let stamp = Stamp { size: text.len() as u64, modified: 1_700_000_000, sha256: None };
        self.rt.update(|s, _| crate::screens::files::deliver(s, Reply { tab, server: WEB.into(), answer: Answer::Read { ticket, path, result: Ok((file, stamp)) } }));
        self.settle();
    }
}

#[test]
fn browse_files_in_this_folder_opens_files_where_the_shell_is() {
    let mut w = in_tmux();
    w.click("server-1");
    let term = w.shown_tab().id;
    // From the terminal's own menu: tmux says the shell is in /srv/www.
    w.secondary_at("term");
    assert!(w.tree().contains("Browse files in this folder"), "{}", w.tree());
    w.click("menu-browse");
    let files = w.shown_tab().id;
    assert_eq!(w.shown_tab().kind, TabKind::Files);
    assert_eq!(w.files_at(files), "/srv/www");
    // From the tab's menu too, and only a terminal's menu has it.
    w.rt.update(|s, tx| s.open_tab_menu(files, [300.0, 30.0], tx));
    w.settle();
    assert!(w.rt.find("tab-browse").is_none());
    w.key(Key::Escape, "");
    w.rt.update(|s, tx| s.open_tab_menu(term, [200.0, 30.0], tx));
    w.settle();
    w.click("tab-browse");
    assert_eq!(w.shown_tab().id, files);
    assert_eq!(w.files_at(files), "/srv/www");
}

#[test]
fn without_tmux_browse_files_follows_the_shell_or_says_why_it_cant() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    // No tmux and the shell said nothing: home, and why.
    w.rt.update(|s, tx| s.active_pane().unwrap().browse_files_here(tx));
    w.settle();
    let files = w.shown_tab().id;
    assert_eq!(w.files_at(files), "~");
    let tree = w.tree();
    assert!(tree.contains("Opened your home folder") && tree.contains("Following the terminal's folder needs tmux on the server."), "{tree}");
    // The shell said where it is (OSC 7): there.
    w.rt.update(|s, tx| s.focus_tab(term, tx));
    w.settle();
    w.feed(0, b"\x1b]7;file://web-7731/var/log/nginx\x07");
    w.rt.update(|s, tx| s.active_pane().unwrap().browse_files_here(tx));
    w.settle();
    assert_eq!(w.shown_tab().id, files);
    assert_eq!(w.files_at(files), "/var/log/nginx");
}

#[test]
fn a_server_shell_outside_tmux_says_it_is_not_kept() {
    let mut w = linux();
    w.click("server-1");
    let id = w.shown_tab().id;
    let key = format!("not-kept-{}", id.0);
    assert!(w.rt.find(&key).is_some(), "no Not kept");
    assert!(w.tree().contains("Not kept. Install tmux on the server to keep sessions alive through disconnects"), "{}", w.tree());
    // Beside another tab it's in the pane's own header.
    w.click("server-2");
    let db = w.shown_tab().id;
    w.rt.update(|s, tx| s.show_beside(id, sshee_app::workbench::layout::Dir::Row, tx));
    w.settle();
    let [_, head_y, _, head_h] = w.rect(&format!("side-head-{}", id.0)).expect("its header");
    let [_, y, ..] = w.rect(&key).expect("Not kept in its header");
    assert!(y >= head_y && y < head_y + head_h, "Not kept isn't in the header");
    assert!(w.rt.find(&format!("not-kept-{}", db.0)).is_some());
    // In tmux it's kept: nothing said.
    let mut w = in_tmux();
    w.click("server-1");
    assert!(!w.tree().contains("Not kept"), "{}", w.tree());
}

#[test]
fn a_programs_progress_shows_on_its_tab_and_along_the_top_of_its_terminal() {
    let mut w = linux();
    w.click("server-1");
    let id = w.shown_tab().id;
    let line = format!("progress-{}", id.0);
    assert!(w.rt.find(&line).is_none());
    w.feed(0, b"\x1b]9;4;1;42\x07");
    w.settle();
    let tree = w.tree();
    assert!(tree.contains("Terminal, web-7731, 42% done"), "the tab doesn't say it: {tree}");
    // The line runs along the top of the terminal, under the top row, 42% of the way.
    let [x, y, width, h] = w.rect(&line).expect("the progress line");
    let [tx, ty, tw, _] = w.rect(&format!("pane-{}", id.0)).expect("the pane");
    assert_eq!((x, y, width, h), (tx, ty, tw, 2.0));
    // It can't tell how far: "Working"; it stops: gone from both.
    w.feed(0, b"\x1b]9;4;3;0\x07");
    w.settle();
    assert!(w.tree().contains("Terminal, web-7731, Working"), "{}", w.tree());
    w.feed(0, b"\x1b]9;4;0;0\x07");
    w.settle();
    assert!(w.rt.find(&line).is_none() && !w.tree().contains("Working"));
}

#[test]
fn a_sleeping_tab_is_a_quiet_moon_and_a_click_wakes_it() {
    let mut w = linux();
    w.click("server-1");
    let web = w.shown_tab().id;
    w.click("server-2");
    let sink = w.session(0).lock().unwrap().status.clone().expect("the session opened");
    sink(LinkStatus::Asleep);
    w.settle();
    let tab = w.rt.state.bench.tab(web).unwrap();
    assert_eq!((tab.link, tab.badge()), (Link::Asleep, Some(Badge::Asleep)));
    let tree = w.tree();
    assert!(tree.contains("Terminal, web-7731, Asleep") && !tree.contains("Disconnected"), "{tree}");
    // The moon alone: no words in the row.
    let [_, _, width, _] = w.rect("badge").expect("its mark");
    assert!(width < 16.0, "the mark has words: {width}");
    w.click(&format!("tab-{}", web.0));
    assert!(w.written(0).ends_with("<reconnect>"), "{}", w.written(0));
}

#[test]
fn open_new_terminal_here_from_files_opens_it_beside() {
    let mut w = linux();
    w.click("server-1");
    let mut files = TabId(0);
    w.rt.update(|s, tx| files = s.open_files_on(WEB, Some("/home/deploy"), tx));
    w.settle();
    w.list(files, "/home/deploy", &["notes.md"]);
    w.rt.update(|s, tx| crate::screens::files::terminal_here(s, files, "/home/deploy/shop", tx));
    w.settle();
    let term = w.shown_tab().id;
    assert_eq!(w.shown_tab().kind, TabKind::Terminal);
    assert!(w.rt.state.bench.together(files, term), "not beside Files");
    assert!(w.rect(&format!("side-head-{}", files.0)).is_some() && w.rect(&format!("side-head-{}", term.0)).is_some());
}

/// Twelve numbered lines.
const TWELVE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";

#[test]
fn a_path_with_lines_opens_the_file_scrolled_to_them_and_marked() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    // "orders.rs:4-6" clicked in the terminal, found on the server.
    w.rt.update(|s, tx| crate::screens::prompts::show_path(s, term, "/home/deploy", Some("orders.rs"), Some((4, 6)), tx));
    w.settle();
    let files = w.rt.state.bench.tabs().iter().find(|t| t.kind == TabKind::Files).unwrap().id;
    w.list(files, "/home/deploy", &["orders.rs", "notes.md"]);
    w.read(files, TWELVE);
    let tree = w.tree();
    assert!(tree.contains("lines 4–6"), "the title doesn't say which: {tree}");
    let [_, band_y, _, band_h] = w.rect(&format!("lines-band-{}", files.0)).expect("the band");
    assert_eq!(band_h, 3.0 * 18.0);
    // Scrolled so three lines come before them (clamped: the file is short, the band stays
    // where the text puts it).
    let [_, top, ..] = w.rect("preview-scroll").expect("the text");
    assert!(band_y >= top, "the band is above the text");
    // Edited, they may not be those lines anymore: the band and the note go.
    w.click("preview-text");
    w.key(Key::Text("x".into()), "");
    assert!(w.rect(&format!("lines-band-{}", files.0)).is_none() && !w.tree().contains("lines 4–6"));
    // Another file opened by hand isn't marked.
    w.rt.update(|s, _| crate::screens::files::open(s, files, "notes.md"));
    w.settle();
    assert!(!w.tree().contains("lines 4–6"));
}

#[test]
fn a_line_in_a_terminal_link_marks_one_line() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    w.rt.update(|s, tx| s.open_path(term, "/home/deploy/orders.rs", Some(9), tx));
    w.settle();
    let files = w.shown_tab().id;
    w.list(files, "/home/deploy", &["orders.rs"]);
    w.read(files, TWELVE);
    assert!(w.tree().contains("line 9"), "{}", w.tree());
    let [_, _, _, h] = w.rect(&format!("lines-band-{}", files.0)).expect("the band");
    assert_eq!(h, 18.0);
}

#[test]
fn markdown_opened_at_lines_marks_the_blocks_they_make_up() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    w.rt.update(|s, tx| crate::screens::prompts::show_path(s, term, "/home/deploy", Some("notes.md"), Some((3, 4)), tx));
    w.settle();
    let files = w.rt.state.bench.tabs().iter().find(|t| t.kind == TabKind::Files).unwrap().id;
    w.list(files, "/home/deploy", &["notes.md"]);
    w.read(files, "# Notes\n\nFirst paragraph,\non two lines.\n\nSecond one.\n");
    assert!(w.tree().contains("lines 3–4"));
    let [_, y, _, h] = w.rect(&format!("md-band-{}", files.0)).expect("the band");
    // It holds the first paragraph, not the heading nor the second paragraph.
    let [_, py, _, ph] = w.rect("md-1").expect("the first paragraph");
    assert!(y <= py && y + h >= py + ph, "the band misses the paragraph");
    let [_, sy, ..] = w.rect("md-2").expect("the second paragraph");
    assert!(y + h <= sy, "the band reaches the second paragraph");
}

#[test]
fn files_dropped_on_a_servers_terminal_ask_where_to_go_starting_at_its_folder() {
    let mut w = in_tmux();
    w.click("server-1");
    let [x, y, ..] = w.rect("term").unwrap();
    w.rt.drop_files(vec![PathBuf::from("/tmp/made-up/report.pdf"), PathBuf::from("/tmp/made-up/notes.md")], Some((x + 100.0, y + 100.0)));
    w.settle();
    let tree = w.tree();
    assert!(tree.contains("Upload 2 items to web-7731?") && tree.contains("report.pdf, notes.md"), "{tree}");
    assert_eq!(w.rt.state.upload.as_ref().map(|a| a.folder.text.clone()).as_deref(), Some("/srv/www"));
    // Nothing goes before the yes.
    assert!(w.wants().iter().all(|w| !matches!(w, Want::Upload { .. })));
    // Cancel: nothing at all.
    w.click("upload-cancel");
    assert!(w.rt.find("upload").is_none() && w.rt.state.upload.is_none());
    assert_eq!(w.shown_tab().kind, TabKind::Terminal);
    // Again, to another folder: Files opens there and the files go.
    w.rt.drop_files(vec![PathBuf::from("/tmp/made-up/report.pdf")], Some((x + 100.0, y + 100.0)));
    w.settle();
    assert!(w.tree().contains("Upload “report.pdf” to web-7731?"));
    w.rt.update(|s, _| {
        if let Some(ask) = s.upload.as_mut() {
            ask.folder = rune::TextEdit::new("/srv/uploads");
        }
    });
    w.settle();
    w.click("upload-go");
    assert!(w.rt.state.upload.is_none());
    let files = w.shown_tab().id;
    assert_eq!(w.shown_tab().kind, TabKind::Files);
    assert_eq!(w.files_at(files), "/srv/uploads");
    let sent: Vec<(Vec<PathBuf>, String)> = w
        .wants()
        .into_iter()
        .filter_map(|w| match w {
            Want::Upload { paths, dir, .. } => Some((paths, dir)),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [(vec![PathBuf::from("/tmp/made-up/report.pdf")], "/srv/uploads".to_owned())]);
}

#[test]
fn without_tmux_the_upload_question_starts_at_home() {
    let mut w = linux();
    w.click("server-1");
    let [x, y, ..] = w.rect("term").unwrap();
    w.rt.drop_files(vec![PathBuf::from("/tmp/made-up/report.pdf")], Some((x + 100.0, y + 100.0)));
    w.settle();
    assert_eq!(w.rt.state.upload.as_ref().map(|a| a.folder.text.clone()).as_deref(), Some("~"));
    // Return uploads, as the default button.
    w.key(Key::Enter, "");
    assert!(w.rt.state.upload.is_none());
    assert_eq!(w.shown_tab().kind, TabKind::Files);
}

#[test]
fn a_long_file_scrolls_so_the_marked_lines_sit_near_the_top() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    w.rt.update(|s, tx| s.open_path(term, "/home/deploy/orders.rs", Some(150), tx));
    w.settle();
    let files = w.shown_tab().id;
    w.list(files, "/home/deploy", &["orders.rs"]);
    let long: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    w.read(files, &long);
    let [_, band_y, ..] = w.rect(&format!("lines-band-{}", files.0)).expect("the band");
    let [_, top, _, h] = w.rect("preview-scroll").expect("the text");
    // Three lines of what comes before stay in sight above it.
    assert!((band_y - (top + 3.0 * 18.0)).abs() < 1.0, "the band is at {band_y}, the text starts at {top}");
    assert!(band_y + 18.0 <= top + h);
}

#[test]
fn a_long_line_runs_on_to_the_right_and_scrolls_there() {
    let mut w = linux();
    w.click("server-1");
    let term = w.shown_tab().id;
    w.rt.update(|s, tx| s.open_path(term, "/home/deploy/wide.csv", None, tx));
    w.settle();
    let files = w.shown_tab().id;
    w.list(files, "/home/deploy", &["wide.csv"]);
    let wide: String = (0..40).map(|n| format!("{n:04},a1b2c3d4-e5f6-7a8b-9c0d-ef1234567890,")).collect();
    w.read(files, &format!("short line\n{wide}\nlast\n"));
    let [_, _, view, _] = w.rect("preview-scroll").expect("the text");
    let text = w.rect("preview-text").or_else(|| w.rect("preview-lines")).expect("the lines");
    // The whole line is laid out, wider than the pane, so it scrolls sideways instead of
    // being cut at the pane's edge.
    assert!(text[2] > view * 2.0, "the text is {} wide in a pane {view} wide", text[2]);
}
