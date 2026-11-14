//! Floods that cost time rather than bytes: the screen keeps the terminal within a slice's time,
//! a hidden pane's events stay few, a sliver of a size leaves the history alone, one cell can't
//! gather endless marks, and copying a word out of a huge line is quick.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sshee_term::engine::{AlacrittyEngine, Engine, EngineConfig, GridPoint, Size, TermEvent, MAX_MARKS, MAX_REPLIES};
use sshee_term::model::Model;
use sshee_term::select::{AsIs, SelectionKind, Side};

/// How long the UI thread may wait for the terminal. A slice is about a millisecond in a release
/// build (some ten in a debug one); the rest is room for a busy machine that doesn't run the
/// waiting thread at once. Before slicing, one read held it for seconds.
fn longest_wait() -> Duration {
    Duration::from_millis(if cfg!(debug_assertions) { 500 } else { 150 })
}

/// `x` and REP 65,535 over and over: 8 bytes that wrote 65,535 cells each, so one 16 KiB read
/// held the terminal for seconds. The screen now gets it within a slice.
#[test]
fn a_rep_flood_never_holds_the_screen_up() {
    let m = Arc::new(Model::new(Size::new(80, 24), &EngineConfig::default()));
    let chunk: Vec<u8> = b"x\x1b[65535b".iter().copied().cycle().take(16 * 1024).collect();
    let reads = if cfg!(debug_assertions) { 2 } else { 10 };
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (m, done) = (m.clone(), done.clone());
        std::thread::spawn(move || {
            for _ in 0..reads {
                m.feed(&chunk);
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    let mut longest = Duration::ZERO;
    let mut asked = 0;
    while !done.load(Ordering::SeqCst) {
        let t = Instant::now();
        m.due(Instant::now());
        longest = longest.max(t.elapsed());
        let t = Instant::now();
        m.frame(false);
        longest = longest.max(t.elapsed());
        asked += 1;
        std::thread::sleep(Duration::from_millis(2));
    }
    reader.join().unwrap();
    eprintln!("{asked} frames while flooded; the longest wait {longest:?}");
    assert!(asked > 1);
    assert!(longest < longest_wait(), "the screen waited {longest:?}");
}

/// The cut REP leaves the screen and the cursor exactly as the full one would.
#[test]
fn a_cut_rep_shows_the_same() {
    for (cols, rows, out) in [(20, 5, &b"ab\x1b[65535b!"[..]), (21, 4, b"\x1b[3;7H\xe2\x94\x80\x1b[40000b|"), (30, 3, b"q\x1b[89b.")] {
        let m = Model::new(Size::new(cols, rows), &EngineConfig::default());
        m.feed(out);
        let frame = m.frame(true);
        let mut e = AlacrittyEngine::new(Size::new(cols, rows), &EngineConfig::default());
        e.feed(out);
        let want: Vec<String> = (0..rows as usize).map(|l| e.row(l).text()).collect();
        assert_eq!(frame.rows.iter().map(|r| r.row.text()).collect::<Vec<_>>(), want, "{out:?}");
        assert_eq!((frame.cursor.line, frame.cursor.col), (e.cursor().line, e.cursor().col));
    }
}

/// A hidden or minimized pane's 0×0, or a split dragged to the edge: the history stays whole
/// and nothing reflows.
#[test]
fn a_sliver_of_a_size_keeps_the_history() {
    let m = Model::new(Size::new(200, 50), &EngineConfig::default());
    let line = "w".repeat(186);
    let mut out = Vec::new();
    for i in 0..10_100 {
        out.extend_from_slice(format!("{i:05}{line}\r\n").as_bytes());
    }
    for chunk in out.chunks(16 * 1024) {
        m.feed(chunk);
    }
    let history = m.frame(true).history;
    assert_eq!(history, 10_000);
    for size in [Size::new(0, 0), Size::new(2, 50), Size::new(200, 1)] {
        let t = Instant::now();
        assert_eq!(m.resize(size), Size::new(200, 50));
        assert!(t.elapsed() < Duration::from_millis(50), "{size:?} took {:?}", t.elapsed());
    }
    assert_eq!(m.resize(Size::new(200, 50)), Size::new(200, 50));
    assert_eq!(m.frame(true).history, history);
}

/// Combining marks on one character, millions of them: the cell keeps a few, and drawing it
/// stays quick.
#[test]
fn one_cell_keeps_a_few_marks() {
    let m = Model::new(Size::new(40, 4), &EngineConfig::default());
    m.feed(b"a");
    let marks = "\u{301}".repeat(if cfg!(debug_assertions) { 100_000 } else { 1_000_000 });
    for chunk in marks.as_bytes().chunks(16 * 1024) {
        m.feed(chunk);
    }
    m.feed("b\u{308}".as_bytes());
    let frame = m.frame(true);
    let cells = &frame.rows[0].row.cells;
    assert_eq!(cells[0].ch, 'a');
    assert_eq!(cells[0].zerowidth.as_deref().map(|z| z.chars().count()), Some(MAX_MARKS));
    assert_eq!((cells[1].ch, cells[1].zerowidth.as_deref()), ('b', Some("\u{308}")));
    let t = Instant::now();
    m.feed(b"c");
    m.frame(false);
    assert!(t.elapsed() < Duration::from_millis(20), "an echo took {:?}", t.elapsed());
}

/// Bells and questions from a pane nobody takes events from (hidden, minimized).
#[test]
fn a_hidden_panes_events_stay_few() {
    let m = Model::new(Size::new(80, 24), &EngineConfig::default());
    let total = if cfg!(debug_assertions) { 4 << 20 } else { 32 << 20 };
    let bells = vec![0x07u8; 16 * 1024];
    for _ in 0..total / bells.len() {
        m.feed(&bells);
    }
    let questions: Vec<u8> = b"\x1b[6n".iter().copied().cycle().take(16 * 1024).collect();
    for _ in 0..total / 4 / questions.len() {
        m.feed(&questions);
    }
    let events = m.take_events();
    assert_eq!(events.len(), 2, "{} events", events.len());
    assert_eq!(events[0], TermEvent::Bell);
    assert!(matches!(&events[1], TermEvent::Reply(r) if r.len() <= MAX_REPLIES && r.starts_with(b"\x1b[1;1R")));
}

/// A double-click on a word in the middle of a 2 MB line (minified JSON, a base64 blob) and the
/// copy: a few rows around it are read, not ten thousand.
#[test]
fn copying_a_word_out_of_a_huge_line_is_quick() {
    let m = Model::new(Size::new(200, 50), &EngineConfig::default());
    let size = if cfg!(debug_assertions) { 512 * 1024 } else { 2 << 20 };
    let mut line = String::with_capacity(size + 64);
    for i in 0.. {
        if line.len() >= size {
            break;
        }
        line.push_str(&format!("\"key{i}\":\"value{i}\","));
    }
    for chunk in line.as_bytes().chunks(16 * 1024) {
        m.feed(chunk);
    }
    let history = m.frame(true).history as i32;
    let middle = GridPoint::new(-history / 2, 100);
    m.with_engine(|e| e.start_selection(SelectionKind::Word, middle, Side::Left));
    let t = Instant::now();
    let word = m.selected_text(&AsIs).unwrap();
    let took = t.elapsed();
    assert!(!word.is_empty() && word.len() < 40, "{word:?}");
    assert!(took < longest_wait(), "the copy took {took:?}");
    // Select all still copies everything there is (the rows the history keeps).
    m.with_engine(|e| e.select_all());
    let all = m.selected_text(&AsIs).unwrap();
    assert!(all.len() > (history as usize) * 199, "{} bytes", all.len());
}
