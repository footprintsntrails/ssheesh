//! A character cut between two reads shows exactly as it does uncut.
//!
//! vte 0.15 drops the byte after a 2-byte character that the next read completes ("приве\xD1" then
//! "\x82 мир" showed "приветмир"), so the engine never lets it see a read that ends inside a
//! character. Here every sample is cut at every place, in two and in three, and a byte at a
//! time, through the engine and through the model, and compared with the uncut screen.

use sshee_term::engine::{AlacrittyEngine, Engine, EngineConfig, Size};
use sshee_term::model::Model;

/// Cyrillic, Greek and accented letters (2 bytes), combining marks, CJK (3), emoji (4), and the
/// pattern that lost a byte: a 2-byte character, one ASCII byte, another multi-byte character.
const SAMPLES: &[&str] = &[
    "привет мир",
    "дат\nмир",
    "e\u{301} 👍",
    "αβγ δ é ü ñ",
    "да\u{301}л ✅ 漢字 ok",
    "\u{1F44D}\u{1F3FD} x я",
    "щ\r\nж\x1b[31mк\x1b[0m з",
];

/// Broken UTF-8 shows the same replacement characters cut or uncut.
const BROKEN: &[&[u8]] = &[b"a\xD1x b", b"\xF0\x9F x\xD0", b"\xE2\x82\x1b[31mred", b"\xC0\xAF z", b"\xED\xA0\x80 q", b"\xD1\xD1\x82"];

type Screen = (Vec<String>, (usize, u16));

fn engine_screen(chunks: &[&[u8]]) -> Screen {
    let mut e = AlacrittyEngine::new(Size::new(24, 4), &EngineConfig::default());
    for chunk in chunks {
        e.feed(chunk);
    }
    let rows = (0..4).map(|line| e.row(line).text()).collect();
    let cursor = e.cursor();
    (rows, (cursor.line, cursor.col))
}

fn model_screen(chunks: &[&[u8]]) -> Screen {
    let m = Model::new(Size::new(24, 4), &EngineConfig::default());
    for chunk in chunks {
        m.feed(chunk);
    }
    let frame = m.frame(true);
    (frame.rows.iter().map(|r| r.row.text()).collect(), (frame.cursor.line, frame.cursor.col))
}

fn every_cut(bytes: &[u8], screen: fn(&[&[u8]]) -> Screen) {
    let whole = screen(&[bytes]);
    for a in 0..=bytes.len() {
        assert_eq!(screen(&[&bytes[..a], &bytes[a..]]), whole, "{bytes:?} cut at {a}");
        for b in a..=bytes.len() {
            assert_eq!(screen(&[&bytes[..a], &bytes[a..b], &bytes[b..]]), whole, "{bytes:?} cut at {a} and {b}");
        }
    }
    let one_by_one: Vec<&[u8]> = bytes.iter().map(std::slice::from_ref).collect();
    assert_eq!(screen(&one_by_one), whole, "{bytes:?} a byte at a time");
}

#[test]
fn characters_cut_between_reads_show_as_uncut() {
    assert_eq!(engine_screen(&["приве\u{442} мир".as_bytes()]).0[0].trim_end(), "привет мир");
    for sample in SAMPLES {
        every_cut(sample.as_bytes(), engine_screen);
    }
}

/// The two reads from the report: "приве\xD1" + "\x82 мир" and "да\xD1" + "\x82\nмир".
#[test]
fn the_reported_cuts() {
    let mut first = "приве".as_bytes().to_vec();
    first.push(0xD1);
    let (rows, _) = engine_screen(&[&first, b"\x82 \xD0\xBC\xD0\xB8\xD1\x80"]);
    assert_eq!(rows[0].trim_end(), "привет мир");
    let mut first = "да".as_bytes().to_vec();
    first.push(0xD1);
    let (rows, _) = engine_screen(&[&first, b"\x82\n\xD0\xBC\xD0\xB8\xD1\x80"]);
    assert_eq!(rows[0].trim_end(), "дат");
    assert_eq!(rows[1].trim_end(), "   мир");
}

#[test]
fn through_the_model_too() {
    for sample in SAMPLES {
        every_cut(sample.as_bytes(), model_screen);
    }
}

#[test]
fn broken_characters_show_as_uncut() {
    for bytes in BROKEN {
        every_cut(bytes, engine_screen);
    }
}

/// Russian log output in the 16 KiB reads sshee-core makes: every line in the history comes out
/// whole, not only the last ones on screen.
#[test]
fn every_line_of_a_long_russian_log_survives_16_kib_reads() {
    let mut stream = Vec::new();
    let mut want = Vec::new();
    for i in 0..3000 {
        let line = format!("{i:05} запрос обработан: ответ «готово», время {} мс — ok ✅", i % 97);
        stream.extend_from_slice(line.as_bytes());
        stream.extend_from_slice(b"\r\n");
        want.push(line);
    }
    let m = Model::new(Size::new(80, 24), &EngineConfig::default());
    for chunk in stream.chunks(16 * 1024) {
        m.feed(chunk);
    }
    let got: Vec<String> = m.with_engine(|e| {
        let mut lines = Vec::new();
        let mut line = String::new();
        let top = -(e.history_size() as i32);
        for l in top..e.size().rows as i32 {
            let row = e.grid_row(l).unwrap();
            if row.wraps {
                line.push_str(&row.text());
            } else {
                line.push_str(&row.trimmed_text());
                lines.push(std::mem::take(&mut line));
            }
        }
        lines
    });
    let got: Vec<&String> = got.iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(got.len(), want.len());
    for (got, want) in got.iter().zip(&want) {
        assert_eq!(*got, want);
    }
}
