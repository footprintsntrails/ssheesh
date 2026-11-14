//! The prototype's window: a title row over one terminal pane.

use std::sync::Arc;
use std::time::Instant;

use rune::tok::TextStyle;
use rune::{col, el, row, spacer, text, App, AppSpec, Color, Cx, El, LiveText};

use crate::meter::{Meter, MeterOptions, SharedMeter};
use crate::terminal::view::pane_view;
use crate::terminal::{NoLink, PaneOptions, StatusSink, TermLink, TermPane};

pub use crate::terminal::LinkStatus;

/// The window's title (taskbar, window list). Never a host: traces and window lists carry it.
pub const TITLE: &str = "SSHeesh prototype";

/// The title row's height in points; whole pixels at 1, 1.25, 1.5, 1.75 and 2x, so the grid
/// below starts on a pixel.
pub const TITLE_ROW_HEIGHT: f32 = 28.0;

/// What the program tells the window when it starts.
pub struct Options {
    /// The session's name in the title row ("This computer", or the ssh destination as typed).
    pub label: String,
    /// The pane's session.
    pub link: Box<dyn TermLink>,
    pub pane: PaneOptions,
    /// Measure (`--metrics`, `--hud`).
    pub meter: Option<MeterOptions>,
    /// Run the pane on Rune's clock, virtual in a headless run (a replay), so its snapshots
    /// are the same on every machine.
    pub virtual_clock: bool,
}

struct State {
    label: String,
    pane: TermPane,
    /// The terminal has been given the keyboard once.
    focused_once: bool,
    /// The meter's numbers in the title row (`--hud`).
    hud: Option<LiveText>,
}

fn pane_of(s: &mut State) -> &mut TermPane {
    &mut s.pane
}

/// The program's own arguments: what Rune didn't take (`--snap`, `--script` and the rest are
/// Rune's), in order.
pub fn extra_args() -> &'static [String] {
    rune::extra_args()
}

fn quiet() -> StatusSink {
    Arc::new(|_| {})
}

/// Opens the window and returns when it closes. Prints why and exits 1 when there's no display.
pub fn run(options: Options) {
    crate::fonts::declare();
    let spec = AppSpec::<State> {
        fonts: crate::fonts::FONTS,
        families: crate::fonts::FAMILIES,
        setup: Some(|s, waker| s.pane.set_waker(waker)),
        poll: Some(poll),
        on_key: Some(|s, ev, tx| s.pane.key(ev, tx)),
        ..AppSpec::new(
            TITLE,
            (960.0, 700.0),
            || State { label: String::new(), pane: TermPane::new(Box::new(NoLink), quiet(), PaneOptions::default()), focused_once: false, hud: None },
            view,
        )
    };
    let app = App::new(spec);
    let handle = app.handle();
    // Status comes from the session's threads; the title row reads it on the UI thread.
    let status: StatusSink = Arc::new(move |st| handle.update(move |s: &mut State, _| s.pane.status = st));
    let Options { label, link, pane, meter, virtual_clock } = options;
    let meter: Option<SharedMeter> = meter.map(Meter::shared);
    let reports = meter.clone();
    let mut app = app
        .on_focus(|s, focused, tx| s.pane.focus(focused, tx))
        .on_ime(|s, ev, tx| s.pane.ime(ev, tx))
        .on_close(|s, _| s.pane.close());
    if let Some(reports) = reports {
        app = app.on_frame_encoded(move |f| reports.borrow_mut().frame(f));
    }
    // A burst the gate holds: told when to poll from the session's thread, with no frame now.
    let hold_handle = app.handle();
    app.init(move || {
        let mut pane = TermPane::new(link, status, pane);
        pane.model.set_hold_waker(Some(Arc::new(move |at| hold_handle.update_if(move |s: &mut State, tx| {
            s.pane.wake_at(at, tx);
            false
        }))));
        if virtual_clock {
            pane.use_virtual_clock(Instant::now());
        }
        let hud = meter.as_ref().and_then(|m| m.borrow().hud().cloned());
        if let Some(meter) = meter {
            pane.set_meter(meter);
        }
        State { label, pane, focused_once: false, hud }
    })
    .run();
}

fn poll(s: &mut State, clock: f64, tx: &mut rune::Tx) -> bool {
    if !s.focused_once {
        s.focused_once = true;
        tx.focus("term", false);
    }
    s.pane.poll(clock, tx)
}

/// 0.10.14's window colours (index.css: midnight and daylight), for the parts around the
/// terminal.
struct Chrome {
    window: Color,
    fg: Color,
    fg_3: Color,
    line: Color,
    ok: Color,
    warn: Color,
    bad: Color,
}

impl Chrome {
    fn new(dark: bool) -> Self {
        let c = |hex: &str| rune::color::hex(hex).expect("a colour written here");
        if dark {
            Chrome {
                window: c("#0f1013"),
                fg: c("#eceef1"),
                fg_3: c("#8a919b"),
                line: c("#ffffff").alpha(0.075),
                ok: c("#4eba65"),
                warn: c("#f5b841"),
                bad: c("#ff6159"),
            }
        } else {
            Chrome {
                window: c("#eceef2"),
                fg: c("#14171c"),
                fg_3: c("#626973"),
                line: c("#0f172a").alpha(0.075),
                ok: c("#15803d"),
                warn: c("#9a5800"),
                bad: c("#c4352d"),
            }
        }
    }

    fn dot(&self, status: &LinkStatus) -> Color {
        match status {
            LinkStatus::Idle | LinkStatus::Connecting => self.fg_3,
            LinkStatus::Connected { .. } => self.ok,
            LinkStatus::Waiting { .. } => self.warn,
            LinkStatus::Gone { .. } => self.bad,
        }
    }
}

const NAME: TextStyle = TextStyle::new(12.0, 600.0);
const HUD_WIDTH: f32 = 230.0;
const META: TextStyle = TextStyle::new(12.0, 400.0).features("'tnum' 1");

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let chrome = Chrome::new(cx.env().system_dark);
    let title = title_row(s, &chrome, cx.titlebar_inset());
    col()
        .w_full()
        .h_full()
        .fill(chrome.window)
        .on_paste_content(|s: &mut State, content, tx| s.pane.paste(content, tx))
        .child(title)
        .child(pane_view(&s.pane, cx, pane_of))
}

/// [state dot] [label] [tmux] [note] ...... [cols×rows]; on macOS after the window buttons,
/// which draw over it (the row is the title bar there).
fn title_row(s: &State, chrome: &Chrome, inset: f32) -> El {
    let status = &s.pane.status;
    let dot = el().key("state").size(8.0, 8.0).radius(4.0).fill(chrome.dot(status));
    let mut row = row()
        .key("title")
        .w_full()
        .h(TITLE_ROW_HEIGHT)
        .items_center()
        .gap(8.0)
        .pl(12.0 + inset)
        .pr(12.0)
        .fill(chrome.window)
        .border_sides([0.0, 0.0, 0.5, 0.0], chrome.line)
        .window_drag()
        .child(dot)
        .child(text(s.label.as_str()).key("label").font(NAME).color(chrome.fg).single_line().truncate());
    if matches!(status, LinkStatus::Connected { durable: true }) {
        row = row.child(text("tmux").key("tmux").font(META).color(chrome.fg_3).single_line());
    }
    let note = status.note();
    if !note.is_empty() {
        row = row.child(text(note).key("note").font(META).color(chrome.fg_3).single_line().truncate());
    }
    row = row.child(spacer());
    if let Some(hud) = &s.hud {
        // Set once a second by the meter; the view doesn't run again for it.
        // A live text keeps the box it was laid out in: wide enough for "keys p50 123.4 ms ·
        // 120 fps · 1234 MB".
        row = row.child(text(hud.get().to_string()).key("hud").w(HUD_WIDTH).font(META).color(chrome.fg_3).single_line().live_text(hud));
    }
    let size = s.pane.size();
    row.child(text(format!("{}×{}", size.cols, size.rows)).key("size").font(META).color(chrome.fg_3).single_line())
}

#[cfg(test)]
mod tests {
    //! The window headless (`Runtime::with_text`): a fake session, frames, keys and clicks, and
    //! what the accessibility tree (the text a screen reader gets) holds.

    use super::*;
    use std::sync::Mutex;

    use rune::{Env, Key, KeyEvent, Mods, Runtime};
    use sshee_term::engine::Size;
    use sshee_term::model::Model;

    /// What the fake session saw: the model it feeds, its size, and what was written to it.
    #[derive(Default)]
    struct Seen {
        model: Option<Arc<Model>>,
        size: Option<Size>,
        written: Vec<u8>,
    }

    struct FakeLink(Arc<Mutex<Seen>>);

    impl TermLink for FakeLink {
        fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
            let mut seen = self.0.lock().unwrap();
            seen.model = Some(model);
            seen.size = Some(size);
            status(LinkStatus::Connected { durable: false });
        }
        fn write(&self, bytes: &[u8]) {
            self.0.lock().unwrap().written.extend_from_slice(bytes);
        }
        fn resize(&self, size: Size) {
            self.0.lock().unwrap().size = Some(size);
        }
        fn reconnect(&self) {}
        fn close(&self) {}
    }

    fn window() -> (Runtime<State>, Arc<Mutex<Seen>>) {
        window_with(PaneOptions { blink: false, ..PaneOptions::default() })
    }

    fn window_with(options: PaneOptions) -> (Runtime<State>, Arc<Mutex<Seen>>) {
        crate::fonts::declare();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let mut pane = TermPane::new(Box::new(FakeLink(seen.clone())), quiet(), options);
        // The model on the runtime's (virtual) clock, as a replay runs.
        pane.use_virtual_clock(std::time::Instant::now());
        let state = State { label: "web-7731".into(), pane, focused_once: false, hud: None };
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(state, view, (960.0, 700.0), Env::new(true, 1.0), text);
        rt.poll = Some(poll);
        rt.on_key = Some(|s, ev, tx| s.pane.key(ev, tx));
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        (rt, seen)
    }

    /// Output parsed now (on the pane's clock), then a few frames.
    fn feed(rt: &mut Runtime<State>, seen: &Arc<Mutex<Seen>>, bytes: &[u8]) {
        let model = seen.lock().unwrap().model.clone().expect("the session opened");
        model.feed_at(bytes, rt.state.pane.now());
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
    }

    /// Every text the accessibility tree holds (names, values, the terminal's text runs).
    fn tree_text(rt: &Runtime<State>) -> String {
        let tree = rt.accessibility_tree();
        let mut out = String::new();
        for (_, node) in &tree.nodes {
            for t in [node.value(), node.label()].into_iter().flatten() {
                out.push_str(t);
                out.push('\n');
            }
        }
        out
    }

    /// A made-up token of GitHub's shape, built at run time so none sits in the source.
    fn token() -> String {
        let body: String = (0..36).map(|i| (b'A' + (i * 11 % 26) as u8) as char).collect();
        format!("{}{}{}", "gh", "p_", body)
    }

    #[test]
    fn the_session_opens_at_the_size_the_window_holds() {
        let (rt, seen) = window();
        // 960 × 700 at 1x: 936 × 648 points of cells, 7 × 21 pixels each.
        assert_eq!(seen.lock().unwrap().size, Some(Size::new(133, 30)));
        assert_eq!(rt.state.pane.size(), Size::new(133, 30));
    }

    #[test]
    fn output_reaches_the_screen_reader_and_keys_reach_the_session() {
        let (mut rt, seen) = window();
        feed(&mut rt, &seen, b"web-7731:~$ echo hello\r\nhello\r\nweb-7731:~$ ");
        let tree = tree_text(&rt);
        assert!(tree.contains("echo hello"), "{tree}");
        rt.key(KeyEvent { key: Key::Text("l".into()), mods: Mods::default() });
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(seen.lock().unwrap().written, b"l\r");
    }

    /// The input method's anchor in window points: the cursor's cell.
    fn anchor_at(line: f32, col: f32) -> Option<[f32; 4]> {
        Some([12.0 + col * 7.0, 28.0 + 12.0 + line * 21.0, 7.0, 21.0])
    }

    #[test]
    fn the_input_method_is_anchored_on_the_cursor_while_the_window_has_the_keyboard() {
        let (mut rt, seen) = window();
        feed(&mut rt, &seen, b"web-7731:~$ ");
        // Output has been quiet: the anchor follows the cursor.
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(0.0, 12.0));
        // Also while the program hides its cursor (vim, Claude Code).
        feed(&mut rt, &seen, b"\x1b[?25l");
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(0.0, 12.0));
        // Not while the window doesn't have the keyboard (`App::on_focus`, which a test
        // runtime has no hook for).
        rt.update(|s, tx| s.pane.focus(false, tx));
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, None);
        rt.update(|s, tx| s.pane.focus(true, tx));
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(0.0, 12.0));
    }

    /// Rune #225: while output streams, the anchor stays put (a new cursor rectangle every
    /// frame loops Wayland input methods); it catches up once output is quiet for 100 ms.
    #[test]
    fn the_anchor_waits_for_quiet_output() {
        let (mut rt, seen) = window();
        let model = seen.lock().unwrap().model.clone().unwrap();
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(0.0, 0.0));
        // A line a frame for a third of a second.
        for i in 0..20 {
            model.feed_at(format!("line {i}\r\n").as_bytes(), rt.state.pane.now());
            rt.frame(1.0 / 60.0);
            assert_eq!(rt.ime_area, anchor_at(0.0, 0.0), "moved during output, frame {i}");
        }
        // Quiet: it stays until the output is 100 ms old (the last line came a frame before the
        // loop's last frame), then moves to the cursor below the lines.
        for _ in 0..4 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(0.0, 0.0), "moved before output was quiet for 100 ms");
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.ime_area, anchor_at(20.0, 0.0));
    }

    /// Rune #602: the input method turns off in the middle of a composition. The composition is
    /// typed, and Return (which Rune would eat, thinking a composition is still open) is taken
    /// before Rune's key routing.
    #[test]
    fn an_input_method_turning_off_mid_composition_leaves_return_working() {
        use rune::{Ime, PhysicalInput, PhysicalKeyCode};
        let (mut rt, seen) = window();
        feed(&mut rt, &seen, b"web-7731:~$ ");
        rt.update(|s, tx| {
            s.pane.ime(&Ime::Preedit("にほん".into(), Some((9, 9)), vec![]), tx);
        });
        rt.frame(1.0 / 60.0);
        assert!(seen.lock().unwrap().written.is_empty(), "a composition sends nothing");
        assert!(tree_text(&rt).contains("web-7731:~$"));
        rt.update(|s, tx| {
            s.pane.ime(&Ime::Disabled, tx);
        });
        rt.frame(1.0 / 60.0);
        assert_eq!(String::from_utf8(seen.lock().unwrap().written.clone()).unwrap(), "にほん");
        let enter = |pressed| PhysicalInput::Key { key: PhysicalKeyCode::Enter, mods: Mods::default(), pressed, repeat: false };
        rt.physical_input(enter(true));
        rt.physical_input(enter(false));
        rt.frame(1.0 / 60.0);
        assert_eq!(String::from_utf8(seen.lock().unwrap().written.clone()).unwrap(), "にほん\r");
        // Rune finishing "its" composition later (the window lost focus) types nothing twice.
        rt.update(|s, tx| {
            s.pane.ime(&Ime::Commit("にほん".into()), tx);
        });
        rt.frame(1.0 / 60.0);
        assert_eq!(String::from_utf8(seen.lock().unwrap().written.clone()).unwrap(), "にほん\r");
        // And the guard is gone: Return goes Rune's way again (the key handler sends it).
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(String::from_utf8(seen.lock().unwrap().written.clone()).unwrap(), "にほん\r\r");
    }

    /// Windows' console (ConPTY) answers "which terminal are you?" itself: a second answer
    /// from the pane would land in the shell as typed text.
    #[test]
    fn da1_is_answered_except_behind_windows_console() {
        use sshee_term::input::Platform;
        for (platform, answer) in [(Platform::Windows, ""), (Platform::Other, "\x1b[?6c"), (Platform::Mac, "\x1b[?6c")] {
            let (mut rt, seen) = window_with(PaneOptions { blink: false, ..PaneOptions::for_platform(platform) });
            feed(&mut rt, &seen, b"\x1b[c");
            assert_eq!(String::from_utf8(seen.lock().unwrap().written.clone()).unwrap(), answer, "{platform:?}");
        }
    }

    /// A pane squeezed below the smallest size the terminal takes (a tiling window manager
    /// ignoring the window's minimum) keeps its size and waits for nothing: no resize every
    /// 90 ms, no full frame each time.
    #[test]
    fn a_pane_too_small_for_the_terminal_rests() {
        let (mut rt, seen) = window();
        let size = rt.state.pane.size();
        // 100 × 60 points of terminal area: 10 columns, 1 row.
        rt.update(|s, tx| s.pane.laid_out(100.0, 60.0, tx));
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
            assert!(!rt.state.pane.waiting_to_resize(), "a resize was scheduled");
        }
        assert_eq!(rt.state.pane.size(), size);
        assert_eq!(seen.lock().unwrap().size, Some(size));
        // Big enough again: resized once.
        rt.update(|s, tx| s.pane.laid_out(500.0, 300.0, tx));
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(seen.lock().unwrap().size, Some(sshee_term::engine::Size::new(68, 13)));
    }

    /// The window moved to a display of another scale: its size in points is the same, so no
    /// layout comes; the view wakes the next poll, which fits the terminal to the new cells.
    #[test]
    fn a_new_scale_refits_the_columns() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (mut rt, seen) = window();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        rt.state.pane.set_waker(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        rt.set_viewport((960.0, 700.0), 2.0);
        rt.frame(1.0 / 60.0);
        assert!(woken.load(Ordering::SeqCst) >= 1, "the view didn't ask for a poll");
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        // At 2x a cell is 15 × 42 pixels: 7.5 × 21 points, so 124 columns fit in 936 points.
        assert_eq!(seen.lock().unwrap().size, Some(sshee_term::engine::Size::new(124, 30)));
    }

    /// A block cursor over a character the program concealed (SGR 8, as a PIN prompt echoes)
    /// draws a space there, as the row does, never the character.
    #[test]
    fn a_block_cursor_never_shows_a_concealed_character() {
        let (mut rt, seen) = window_with(PaneOptions { blink: false, cursor_shape: sshee_term::engine::CursorShape::Block, ..PaneOptions::default() });
        // "PIN: " then 4 concealed digits, and the cursor back over the first.
        feed(&mut rt, &seen, b"PIN: \x1b[8m2468\x1b[0m\x1b[4D");
        let shown = rt.state.pane.view.borrow_mut().shown_char(0, 5);
        assert_eq!(shown.0, " ");
        let tree = tree_text(&rt);
        assert!(['2', '4', '6', '8'].iter().all(|d| !tree.contains(*d)), "{tree}");
    }

    #[test]
    fn a_hidden_secret_never_reaches_the_tree_and_a_click_shows_it() {
        let (mut rt, seen) = window();
        let secret = token();
        feed(&mut rt, &seen, format!("export GH_TOKEN={secret}\r\nweb-7731:~$ ").as_bytes());
        let tree = tree_text(&rt);
        let body = &secret[4..];
        for k in 0..body.len() - 3 {
            assert!(!tree.contains(&body[k..k + 4]), "the tree holds {:?}", &body[k..k + 4]);
        }
        assert!(tree.contains("GH_TOKEN=••••"), "{tree}");
        // A click on the dots: the title row is 28 pt, the padding 12, a cell 7 × 21.
        let (x, y) = (12.0 + 25.0 * 7.0 + 3.0, 28.0 + 12.0 + 10.0);
        rt.pointer_down(x, y);
        rt.frame(1.0 / 60.0);
        rt.pointer_up(x, y);
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let tree = tree_text(&rt);
        assert!(tree.contains(&secret), "shown after the click: {tree}");
        // Nothing went to the session for that click.
        assert!(seen.lock().unwrap().written.is_empty());
    }
}
