//! What a program's output can make one terminal hold stays small: an OSC that never ends, a
//! huge title saved over and over, a copy too big to take. Any server, or a `cat` of a hostile
//! log, could otherwise grow the whole app's memory without end (both parsers keep a whole OSC
//! until it ends; alacritty kept 4,096 copies of a saved title).
//!
//! One test alone in its binary, so the process's memory is this test's.

use std::time::{Duration, Instant};

use sshee_term::engine::{EngineConfig, Size, TermEvent, MAX_TITLE};
use sshee_term::model::Model;

/// The process's resident memory in KiB, where the system says (Linux).
fn resident_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn feed_repeated(m: &Model, head: &[u8], body: &[u8], total: usize) {
    m.feed(head);
    let chunk: Vec<u8> = body.iter().copied().cycle().take(16 * 1024).collect();
    for _ in 0..total / chunk.len() {
        m.feed(&chunk);
    }
}

#[test]
fn unending_oscs_and_saved_titles_stay_small() {
    // Less in a debug build, where everything runs some ten times slower.
    let total = if cfg!(debug_assertions) { 16 << 20 } else { 64 << 20 };
    let m = Model::new(Size::new(200, 50), &EngineConfig::default());
    m.feed(b"warm up\r\n");
    let before = resident_kib();
    let started = Instant::now();

    // A title that never ends, then a copy (OSC 52) that never ends.
    feed_repeated(&m, b"\x1b]2;", b"x", total);
    feed_repeated(&m, b"\x07\x1b]52;c;", b"QUFB", total);
    m.feed(b"\x07");
    // A 1 MiB title, saved 1,000 times, then saved and restored 1,000 times.
    let mut title = b"\x1b]2;".to_vec();
    title.extend(std::iter::repeat_n(b't', 1 << 20));
    title.push(0x07);
    m.feed(&title);
    m.feed(&b"\x1b[22t".repeat(1000));
    m.feed(&b"\x1b[22t\x1b[23t".repeat(1000));

    let took = started.elapsed();
    let events = m.take_events();
    let after = resident_kib();

    // Nothing was copied, and the titles came as one event, cut.
    assert!(!events.iter().any(|e| matches!(e, TermEvent::Copy(_))), "{events:?}");
    let titles: Vec<&TermEvent> = events.iter().filter(|e| matches!(e, TermEvent::Title(_))).collect();
    assert_eq!(titles.len(), 1);
    assert!(matches!(titles[0], TermEvent::Title(Some(t)) if t.len() <= MAX_TITLE && t.bytes().all(|b| b == b't')));
    assert!(m.title().unwrap().len() <= MAX_TITLE);
    if let (Some(before), Some(after)) = (before, after) {
        let grown = after.saturating_sub(before);
        eprintln!("resident memory grew by {grown} KiB in {took:?}");
        assert!(grown < 48 * 1024, "resident memory grew by {grown} KiB");
    }
    assert!(took < Duration::from_secs(if cfg!(debug_assertions) { 60 } else { 10 }), "took {took:?}");

    // Still a working terminal: the next title and text show.
    m.feed(b"\x1b]2;web-7731\x07\r\nstill here");
    assert_eq!(m.title().as_deref(), Some("web-7731"));
    let frame = m.frame(true);
    assert!(frame.rows.iter().any(|r| r.row.text().contains("still here")));
}
