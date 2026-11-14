//! A pane's "session" played from a fixture on the pane's clock (Rune's virtual clock in a
//! headless run), so CI sees the same snapshots on every machine; and a record of what the
//! terminal wrote, resized and copied, for tests to compare.
//!
//! ```json
//! { "cols": 120, "rows": 40,
//!   "events": [
//!     { "t": 0.00, "out": "\u001b[?2004h" },
//!     { "t": 0.10, "out": "web-7731:~$ " },
//!     { "t": 0.20, "echo": true },
//!     { "t": 0.50, "out_file": "stream-1m.bin" },
//!     { "t": 0.80, "out": "TOKEN={{SECRET_A}}\r\n" } ] }
//! ```
//!
//! - `out` is fed as it is, `out_file` (beside the fixture) as its bytes; `{{SECRET_A}}` and
//!   `{{SECRET_B}}` in `out` are made-up secrets of GitHub's token shape, made at run time from
//!   `--secret-seed`, so none ever sits in a fixture. `{{SECRET_A:4:20}}` is characters 4 to 20
//!   of it, for a secret that comes in pieces.
//! - `echo: true` turns on a tiny line discipline: printable input is echoed, Backspace as
//!   `\b \b`, Return (and Ctrl+C, as `^C`) as a new line and the last prompt (the last `out`'s
//!   text after its last line break). Escape sequences (arrows, reports, paste brackets) aren't.
//! - `cols`/`rows`: the size the fixture was made for; the pane opens at the size its window
//!   holds (`--size`), and a difference is said on stderr.
//! - Anything else in an event (`mark`, notes) is ignored: marks belong in Rune's `--script`.
//!
//! `--replay-out` records JSON lines with the pane's clock: `{"t":…,"write":"…"}` (the bytes as
//! text, invalid UTF-8 as U+FFFD with `"hex"` beside it), `{"t":…,"resize":[cols,rows]}`,
//! `{"t":…,"copy":"…"}`.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use sshee_term::engine::Size;
use sshee_term::model::Model;
use sshee_ui::terminal::{LinkStatus, StatusSink, TermLink};

/// History a replayed terminal keeps (as a session without tmux).
const HISTORY: usize = 10_000;

/// One thing a fixture does at its time.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Out(Vec<u8>),
    Echo(bool),
}

/// A fixture, read and filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixture {
    pub size: Option<Size>,
    pub steps: Vec<(f64, Step)>,
}

/// A made-up secret of GitHub's token shape (`ghp_` and 36 letters and digits), different for
/// each `seed` and `which`, built from pieces so no token-shaped string is in the source.
pub fn made_up_secret(seed: u64, which: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    // splitmix64: plenty for made-up characters.
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ which.wrapping_add(0x1234_5678);
    let body: String = (0..36)
        .map(|_| {
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            ALPHABET[(z % ALPHABET.len() as u64) as usize] as char
        })
        .collect();
    format!("{}{}{}", "gh", "p_", body)
}

impl Fixture {
    /// Reads `path`; `out_file`s are found beside it.
    pub fn load(path: &Path, seed: u64) -> Result<Fixture, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Fixture::from_json(&value, path.parent().unwrap_or(Path::new(".")), seed).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(value: &Value, dir: &Path, seed: u64) -> Result<Fixture, String> {
        let size = match (value["cols"].as_u64(), value["rows"].as_u64()) {
            (Some(c), Some(r)) => Some(Size::new(c.min(u64::from(u16::MAX)) as u16, r.min(u64::from(u16::MAX)) as u16)),
            _ => None,
        };
        let events = value["events"].as_array().ok_or("a fixture needs \"events\": [...]")?;
        let (a, b) = (made_up_secret(seed, 0), made_up_secret(seed, 1));
        let mut steps = Vec::new();
        for (i, e) in events.iter().enumerate() {
            let t = e["t"].as_f64().ok_or_else(|| format!("event {i} needs a time \"t\""))?;
            if let Some(out) = e["out"].as_str() {
                let out = fill_secrets(out, &a, &b).map_err(|e| format!("event {i}: {e}"))?;
                steps.push((t, Step::Out(out.into_bytes())));
            } else if let Some(file) = e["out_file"].as_str() {
                if Path::new(file).is_absolute() || file.contains("..") {
                    return Err(format!("event {i}: out_file is a name beside the fixture"));
                }
                let bytes = std::fs::read(dir.join(file)).map_err(|e| format!("event {i}: {file}: {e}"))?;
                steps.push((t, Step::Out(bytes)));
            } else if let Some(on) = e["echo"].as_bool() {
                steps.push((t, Step::Echo(on)));
            }
        }
        steps.sort_by(|x, y| x.0.total_cmp(&y.0));
        Ok(Fixture { size, steps })
    }
}

/// `out` with `{{SECRET_A}}`, `{{SECRET_B}}` and their pieces (`{{SECRET_A:FROM:TO}}`, in
/// characters) filled in.
fn fill_secrets(out: &str, a: &str, b: &str) -> Result<String, String> {
    let mut filled = String::with_capacity(out.len());
    let mut rest = out;
    while let Some(at) = rest.find("{{SECRET_") {
        filled.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let end = tail.find("}}").ok_or("a {{SECRET_...}} isn't closed")?;
        let mut parts = tail[..end].split(':');
        let secret = match parts.next() {
            Some("SECRET_A") => a,
            Some("SECRET_B") => b,
            other => return Err(format!("unknown {other:?}")),
        };
        let range: Vec<usize> = parts.map(|p| p.parse::<usize>().map_err(|_| format!("{p:?} isn't a number"))).collect::<Result<_, _>>()?;
        match range[..] {
            [] => filled.push_str(secret),
            [from, to] if from <= to && to <= secret.chars().count() => filled.extend(secret.chars().skip(from).take(to - from)),
            _ => return Err(format!("{{{{{}}}}} isn't a piece of a secret", &tail[..end])),
        }
        rest = &tail[end + 2..];
    }
    filled.push_str(rest);
    Ok(filled)
}

/// The line discipline of `echo: true`: what a shell at a prompt would echo for input.
#[derive(Debug, Default)]
pub struct Echo {
    pub on: bool,
    /// The prompt drawn after a new line.
    pub prompt: Vec<u8>,
    /// Inside an escape sequence: 1 after ESC, 2 inside CSI, 3 after ESC O.
    esc: u8,
}

impl Echo {
    /// What the "shell" echoes for `input`.
    pub fn echo(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.on {
            return out;
        }
        for &b in input {
            match (self.esc, b) {
                (1, b'[') => self.esc = 2,
                (1, b'O') => self.esc = 3,
                (1, _) | (3, _) => self.esc = 0,
                (2, 0x40..=0x7e) => self.esc = 0,
                (2, _) => {}
                (_, 0x1b) => self.esc = 1,
                (_, b'\r' | b'\n') => {
                    out.extend_from_slice(b"\r\n");
                    out.extend_from_slice(&self.prompt);
                }
                (_, 0x03) => {
                    out.extend_from_slice(b"^C\r\n");
                    out.extend_from_slice(&self.prompt);
                }
                (_, 0x7f | 0x08) => out.extend_from_slice(b"\x08 \x08"),
                (_, 0x20..=0x7e | 0x80..=0xff) => out.push(b),
                _ => {}
            }
        }
        out
    }
}

/// The link: plays the fixture into the pane's model as the pane's clock passes each step.
pub struct ReplayLink {
    fixture: Fixture,
    next: usize,
    model: Option<Arc<Model>>,
    echo: RefCell<Echo>,
    out: Option<RefCell<File>>,
    clock: Cell<f64>,
    now: Cell<Option<Instant>>,
    bytes: Cell<u64>,
}

impl ReplayLink {
    pub fn new(fixture: Fixture, out: Option<File>) -> Self {
        ReplayLink {
            fixture,
            next: 0,
            model: None,
            echo: RefCell::new(Echo::default()),
            out: out.map(RefCell::new),
            clock: Cell::new(0.0),
            now: Cell::new(None),
            bytes: Cell::new(0),
        }
    }

    fn record(&self, mut v: Value) {
        let Some(out) = &self.out else { return };
        v["t"] = json!((self.clock.get() * 1e4).round() / 1e4);
        let mut line = v.to_string();
        line.push('\n');
        // A record that can't be written is lost; the replay goes on.
        let _ = out.borrow_mut().write_all(line.as_bytes());
    }

    fn feed(&self, bytes: &[u8]) {
        let Some(model) = &self.model else { return };
        self.bytes.set(self.bytes.get() + bytes.len() as u64);
        match self.now.get() {
            Some(now) => model.feed_at(bytes, now),
            None => model.feed(bytes),
        }
    }
}

impl TermLink for ReplayLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        if let Some(want) = self.fixture.size.filter(|s| *s != size) {
            eprintln!("ssheesh-proto: the fixture was made for {}×{}; the window holds {}×{}", want.cols, want.rows, size.cols, size.rows);
        }
        model.connected(false, HISTORY);
        self.model = Some(model);
        status(LinkStatus::Connected { durable: false });
        self.record(json!({ "open": [size.cols, size.rows] }));
    }

    fn write(&self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut rec = json!({ "write": text });
        if std::str::from_utf8(bytes).is_err() {
            rec["hex"] = json!(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
        }
        self.record(rec);
        let echo = self.echo.borrow_mut().echo(bytes);
        if !echo.is_empty() {
            self.feed(&echo);
        }
    }

    fn resize(&self, size: Size) {
        self.record(json!({ "resize": [size.cols, size.rows] }));
    }

    fn reconnect(&self) {}

    fn close(&self) {
        if let Some(out) = &self.out {
            let _ = out.borrow_mut().flush();
        }
    }

    fn tick(&mut self, clock: f64, now: Instant) -> Option<f64> {
        self.clock.set(clock);
        self.now.set(Some(now));
        while let Some((t, step)) = self.fixture.steps.get(self.next) {
            if *t > clock + 1e-9 {
                return Some(*t);
            }
            let step = step.clone();
            self.next += 1;
            match step {
                Step::Out(bytes) => {
                    // The prompt: what the last output left on its last line.
                    let tail = bytes.iter().rposition(|&b| b == b'\n').map_or(&bytes[..], |i| &bytes[i + 1..]);
                    if !tail.is_empty() {
                        self.echo.borrow_mut().prompt = tail.to_vec();
                    }
                    self.feed(&bytes);
                }
                Step::Echo(on) => self.echo.borrow_mut().on = on,
            }
        }
        None
    }

    fn bytes_received(&self) -> u64 {
        self.bytes.get()
    }

    fn note_copy(&self, text: &str) {
        self.record(json!({ "copy": text }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_with_secrets_filled_at_run_time() {
        let v = json!({ "cols": 80, "rows": 24, "events": [
            { "t": 0.5, "out": "TOKEN={{SECRET_A}} {{SECRET_B}}\r\n" },
            { "t": 0.1, "echo": true },
            { "t": 0.2, "mark": "here" } ] });
        let f = Fixture::from_json(&v, Path::new("."), 7).unwrap();
        assert_eq!(f.size, Some(Size::new(80, 24)));
        assert_eq!(f.steps[0], (0.1, Step::Echo(true)));
        let Step::Out(out) = &f.steps[1].1 else { panic!() };
        let out = String::from_utf8(out.clone()).unwrap();
        let (a, b) = (made_up_secret(7, 0), made_up_secret(7, 1));
        assert_eq!(out, format!("TOKEN={a} {b}\r\n"));
        assert_eq!(a.len(), 40);
        assert!(a.starts_with("ghp_") && a[4..].chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
        assert_ne!(made_up_secret(8, 0), a, "another seed, another secret");
        assert_eq!(made_up_secret(7, 0), a, "the same seed, the same secret");
        assert_eq!(f.steps.len(), 2, "marks are ignored");
    }

    #[test]
    fn a_secret_in_pieces() {
        let v = json!({ "events": [ { "t": 0, "out": "x{{SECRET_A:0:6}}|{{SECRET_A:6:40}}y" }, { "t": 1, "out": "{{SECRET_B:38:40}}" } ] });
        let f = Fixture::from_json(&v, Path::new("."), 3).unwrap();
        let (a, b) = (made_up_secret(3, 0), made_up_secret(3, 1));
        assert_eq!(f.steps[0].1, Step::Out(format!("x{}|{}y", &a[..6], &a[6..]).into_bytes()));
        assert_eq!(f.steps[1].1, Step::Out(b.as_bytes()[38..].to_vec()));
        for bad in ["{{SECRET_A:3}}", "{{SECRET_A:5:2}}", "{{SECRET_A:0:41}}", "{{SECRET_C}}", "{{SECRET_A:x:2}}", "{{SECRET_A"] {
            let v = json!({ "events": [ { "t": 0, "out": bad } ] });
            assert!(Fixture::from_json(&v, Path::new("."), 0).is_err(), "{bad}");
        }
    }

    #[test]
    fn out_files_stay_beside_the_fixture() {
        for file in ["/etc/passwd", "../x.bin", "a/../../b"] {
            let v = json!({ "events": [ { "t": 0, "out_file": file } ] });
            assert!(Fixture::from_json(&v, Path::new("."), 0).is_err(), "{file}");
        }
    }

    #[test]
    fn the_line_discipline_echoes_like_a_prompt() {
        let mut e = Echo { on: true, prompt: b"web-7731:~$ ".to_vec(), ..Echo::default() };
        assert_eq!(e.echo(b"ls"), b"ls");
        assert_eq!(e.echo("ж".as_bytes()), "ж".as_bytes());
        assert_eq!(e.echo(b"\x7f"), b"\x08 \x08");
        assert_eq!(e.echo(b"\r"), b"\r\nweb-7731:~$ ");
        assert_eq!(e.echo(b"\x03"), b"^C\r\nweb-7731:~$ ");
        // Arrows, mouse reports and paste brackets aren't echoed; what's pasted is.
        assert_eq!(e.echo(b"\x1b[A\x1bOB\x1b[<0;3;4M"), b"");
        assert_eq!(e.echo(b"\x1b[200~hi\x1b[201~"), b"hi");
        // A sequence split across two writes.
        assert_eq!(e.echo(b"\x1b["), b"");
        assert_eq!(e.echo(b"1;5Cx"), b"x");
        e.on = false;
        assert_eq!(e.echo(b"ls"), b"");
    }
}
