//! A flood through the whole engine half, headless: tens of megabytes of log-like output fed in
//! the 16 KiB reads sshee-core makes, on a reader thread, while another thread takes frames as
//! a screen would. It has to finish, keep every line, and end on the right screen.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sshee_term::engine::{EngineConfig, Size, TermEvent};
use sshee_term::model::{Due, Model};
use sshee_term::osc::OscEvent;

/// One line of a made-up log: timestamps, coloured levels, Cyrillic, CJK, emoji, box drawing,
/// now and then a line long enough to wrap, CRLF endings.
fn log_line(i: usize, out: &mut Vec<u8>) {
    let level = ["\x1b[32mINFO\x1b[0m", "\x1b[33mWARN\x1b[0m", "\x1b[1;31mERROR\x1b[0m", "\x1b[2mDEBUG\x1b[0m"][i % 4];
    let extra = match i % 7 {
        0 => "запрос обработан",
        1 => "処理が完了しました",
        2 => "deploy ok ✅",
        3 => "──────────────",
        _ => "",
    };
    let line = format!("2026-11-13T12:{:02}:{:02}.{:03}Z {level} web-7731 req={i} {extra}", (i / 60) % 60, i % 60, i % 1000);
    out.extend_from_slice(line.as_bytes());
    if i.is_multiple_of(11) {
        out.extend(std::iter::repeat_n(b'x', 300));
    }
    out.extend_from_slice(b"\r\n");
}

#[test]
fn a_flood_of_tens_of_megabytes_finishes() {
    // Less in a debug build, where alacritty runs some 10 times slower; still tens of MB.
    let target = if cfg!(debug_assertions) { 24 << 20 } else { 64 << 20 };
    let mut stream = Vec::with_capacity(target + 4096);
    let mut lines = 0;
    while stream.len() < target {
        log_line(lines, &mut stream);
        lines += 1;
    }
    // A notification and a bell at the very end, to see the side pass kept up.
    stream.extend_from_slice(b"\x1b]9;flood done\x07\x07end of flood");

    let model = Arc::new(Model::new(Size::new(120, 40), &EngineConfig::default()));
    let started = Instant::now();
    let reader = {
        let model = model.clone();
        let stream = Arc::new(stream);
        let s = stream.clone();
        std::thread::spawn(move || {
            for chunk in s.chunks(16 * 1024) {
                model.feed(chunk);
            }
            stream.len()
        })
    };

    // The UI thread: a frame whenever the model says so, at most 60 a second.
    let mut frames = 0usize;
    let mut rows_built = 0usize;
    while !reader.is_finished() {
        if model.due(Instant::now()) == Due::Now {
            let frame = model.frame(false);
            rows_built += frame.rows.len();
            frames += 1;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let bytes = reader.join().expect("reader thread");
    let took = started.elapsed();
    let frame = model.frame(true);
    let mib = bytes as f64 / (1 << 20) as f64;
    eprintln!(
        "fed {mib:.1} MiB ({lines} lines) in {:.2} s = {:.1} MiB/s; {frames} frames, {rows_built} rows built",
        took.as_secs_f64(),
        mib / took.as_secs_f64()
    );

    assert!(took < Duration::from_secs(if cfg!(debug_assertions) { 240 } else { 30 }), "took {took:?}");
    assert_eq!(frame.rows.len(), 40);
    let last = frame.rows.iter().rev().find(|r| !r.row.trimmed_text().is_empty()).expect("text on screen");
    assert_eq!(last.row.trimmed_text(), "end of flood");
    // The line before it is the last log line, whole (it may have wrapped onto several rows).
    let screen: String = frame.rows.iter().map(|r| if r.row.wraps { r.row.text() } else { r.row.trimmed_text() + "\n" }).collect();
    assert!(screen.contains(&format!("req={} ", lines - 1)), "last log line missing:\n{screen}");
    // History is full: 10,000 lines kept, the rest dropped.
    assert_eq!(frame.history, 10_000);

    let events = model.take_events();
    assert!(events.contains(&TermEvent::Osc(OscEvent::Notify(Some("flood done".into())))));
    assert!(events.contains(&TermEvent::Bell));
}
