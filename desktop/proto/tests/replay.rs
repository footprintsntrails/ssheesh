//! The prototype headless: a fixture played as the session (`--replay`), Rune's `--script` as
//! the person at the keyboard and mouse, and what the terminal wrote, resized and copied
//! (`--replay-out`) compared with what sshee-term's encoders say it must.
//!
//! Needs a Vulkan driver (Rune draws headless frames on the GPU); with none (no ICD files) the
//! tests say so and pass. Without a GPU, Mesa's software driver (lavapipe) does.
//!
//! Geometry at `--scale 1 --size 960x700`: the title row is 28 pt, the padding 12, a cell 7 × 21,
//! so the terminal is 133 × 30 and cell (line, col) is centred at
//! (12 + 7·col + 3.5, 40 + 21·line + 10.5).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const LVP: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";
/// One row of the test window in points (see `cell`).
#[allow(dead_code)]
const NOTCH_ROW: f64 = 21.0;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

/// Whether a Vulkan driver is there to draw with (Linux; other systems have their own GPU API).
fn vulkan() -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("VK_ICD_FILENAMES").is_some() {
        return true;
    }
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().any(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some()))
}

/// What one run wrote: `--replay-out`'s records.
struct Run {
    records: Vec<Value>,
}

impl Run {
    fn writes(&self) -> Vec<String> {
        self.records.iter().filter_map(|r| r["write"].as_str().map(str::to_owned)).collect()
    }

    fn with(&self, key: &str) -> Vec<&Value> {
        self.records.iter().filter(|r| !r[key].is_null()).collect()
    }
}

/// Runs the prototype on `fixture` with `script` (a JSON array of Rune script events) until the
/// script's `end` mark; `None` (after saying why) when there is nothing to draw with.
fn run(name: &str, fixture_name: &str, script: Value) -> Option<Run> {
    if !vulkan() {
        eprintln!("{name}: skipped, no Vulkan driver here");
        return None;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("replay").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = dir.join("script.json");
    std::fs::write(&script_path, serde_json::to_string_pretty(&script).unwrap()).unwrap();
    let out = dir.join("writes.jsonl");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-proto"));
    cmd.arg("--replay")
        .arg(fixture(fixture_name))
        .arg("--replay-out")
        .arg(&out)
        .arg("--script")
        .arg(&script_path)
        .args(["--snap"])
        .arg(dir.join("end.png"))
        .args(["--at", "mark:end", "--scale", "1", "--size", "960x700"]);
    if cfg!(target_os = "linux") && std::env::var_os("VK_ICD_FILENAMES").is_none() && Path::new(LVP).exists() {
        cmd.env("VK_ICD_FILENAMES", LVP);
    }
    let result = cmd.output().expect("the prototype runs");
    assert!(result.status.success(), "{name}: {}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    let records = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    Some(Run { records })
}

/// The window point at the middle of cell (`line`, `col`).
fn cell(line: u32, col: u32) -> (f64, f64) {
    (12.0 + 7.0 * f64::from(col) + 3.5, 40.0 + 21.0 * f64::from(line) + 10.5)
}

fn key(t: f64, name: &str, extra: Value) -> Value {
    let mut e = serde_json::json!({ "t": t, "type": "key", "name": name });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            e[k] = v;
        }
    }
    e
}

/// Keys on every layout: through Rune's key events and sshee-term's port of xterm.js's
/// encoder, as the bytes a shell gets. Linux's chords (Ctrl+V is the terminal's there).
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "Linux's key chords")]
fn keys_to_bytes() {
    use serde_json::json;
    let script = json!([
        { "t": 0.2, "type": "type", "text": "ls" },
        { "t": 0.3, "type": "type", "text": "привет", "every": 0.02 },
        // Ctrl+С on a Russian layout: the window says Ctrl and C's place, the layout says "с".
        key(0.6, "c", json!({ "ctrl": true, "cmd": true, "code": "c", "base": "с" })),
        key(0.65, "d", json!({ "ctrl": true, "cmd": true, "code": "d" })),
        key(0.7, "b", json!({ "alt": true, "code": "b" })),
        key(0.75, "Up", json!({})),
        key(0.8, "F1", json!({ "ctrl": true })),
        key(0.85, "F12", json!({ "shift": true })),
        key(0.9, "Enter", json!({ "shift": true })),
        key(0.95, "Tab", json!({})),
        // The fixture turns application cursor keys on at 1.0.
        key(1.2, "Up", json!({})),
        // Linux: Ctrl+V is the terminal's (^V), not a paste.
        key(1.25, "v", json!({ "ctrl": true, "cmd": true, "code": "v" })),
        // By the layout's letter: Dvorak types "k" in V's place (^K), "v" in the period's (^V).
        key(1.27, "k", json!({ "ctrl": true, "cmd": true, "code": "v", "base": "k" })),
        key(1.28, "v", json!({ "ctrl": true, "cmd": true, "code": ".", "base": "v" })),
        key(1.3, "Backspace", json!({})),
        key(1.35, "Escape", json!({})),
        { "t": 1.4, "type": "type", "text": "Ж" },
        // Ctrl+Shift+C with nothing selected copies nothing and sends nothing.
        key(1.45, "c", json!({ "ctrl": true, "cmd": true, "shift": true, "code": "c" })),
        key(1.5, "Enter", json!({})),
        { "t": 1.6, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("keys_to_bytes", "modes.json", script) else { return };
    let want: Vec<&str> = vec![
        "l", "s", "п", "р", "и", "в", "е", "т", "\x03", "\x04", "\x1bb", "\x1b[A", "\x1b[1;5P", "\x1b[24;2~", "\n", "\t", "\x1bOA", "\x16",
        "\x0b", "\x16", "\x7f", "\x1b", "Ж", "\r",
    ];
    assert_eq!(run.writes(), want);
    assert!(run.with("copy").is_empty());
}

/// A paste: one write, line breaks as Enter, wrapped when the program asked for bracketed
/// paste, and control characters gone.
#[test]
fn paste_bracketed() {
    use serde_json::json;
    let script = json!([
        { "t": 0.5, "type": "paste", "text": "one\ntwo" },
        // The fixture turns bracketed paste on at 1.0.
        { "t": 1.2, "type": "paste", "text": "echo a\r\necho b\u{1b}[201~\u{3}" },
        { "t": 1.4, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("paste_bracketed", "modes.json", script) else { return };
    assert_eq!(run.writes(), ["one\rtwo", "\x1b[200~echo a\recho b[201~\x1b[201~"]);
}

/// The input method: a composition sends nothing (Return and Escape inside it are the input
/// method's), its commit is typed once.
#[test]
fn ime_commit() {
    use serde_json::json;
    let script = json!([
        { "t": 0.2, "type": "type", "text": "echo " },
        { "t": 0.5, "type": "ime", "preedit": "にほんご", "cursor": [0, 12], "clauses": [[0, 9, true], [9, 12]] },
        key(0.6, "Escape", json!({})),
        { "t": 0.7, "type": "ime", "preedit": "にほんご", "cursor": [12, 12] },
        { "t": 0.8, "type": "ime", "commit": "日本語" },
        key(0.9, "Enter", json!({})),
        // A dead key's commit with no composition before it.
        { "t": 1.0, "type": "ime", "commit": "é" },
        // A commit with a line break goes as a paste.
        { "t": 1.1, "type": "ime", "commit": "a\nb" },
        { "t": 1.3, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("ime_commit", "prompt.json", script) else { return };
    assert_eq!(run.writes(), ["e", "c", "h", "o", " ", "日本語", "\r", "é", "a\rb"]);
}

/// Mouse reports once the program asks (`?1000h ?1006h`, SGR): the primary, middle and
/// secondary buttons, a drag (not asked for: no moves), and the wheel.
#[test]
fn mouse_reports() {
    use serde_json::json;
    let (x, y) = cell(2, 4);
    let (x2, y2) = cell(5, 10);
    let script = json!([
        // Before the program asks: a click selects nothing and reports nothing.
        { "t": 0.5, "type": "tap", "x": x, "y": y },
        // The fixture turns reports on at 2.0.
        { "t": 2.2, "type": "tap", "x": x, "y": y },
        { "t": 2.4, "type": "tap", "x": x2, "y": y2, "button": "middle" },
        { "t": 2.6, "type": "secondary-click", "x": x2, "y": y2 },
        { "t": 2.8, "type": "wheel", "x": x, "y": y, "dy": 63.0 },
        // A mouse wheel's notch: Rune gives it in points, 120 a line on Linux and 40 on macOS.
        // On Windows a notch comes as pixels (100 at the system's three lines), which the
        // pane's own tests cover; here it is given the points of one row, for the same report.
        { "t": 2.9, "type": "wheel", "x": x, "y": y, "dy": if cfg!(target_os = "linux") { -120.0 } else if cfg!(windows) { -NOTCH_ROW } else { -40.0 }, "precise": false },
        { "t": 3.0, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("mouse_reports", "modes.json", script) else { return };
    assert_eq!(
        run.writes(),
        [
            "\x1b[<0;5;3M",
            "\x1b[<0;5;3m",
            "\x1b[<1;11;6M",
            "\x1b[<1;11;6m",
            "\x1b[<2;11;6M",
            "\x1b[<2;11;6m",
            // 63 points up is three rows of 21.
            "\x1b[<64;5;3M",
            "\x1b[<64;5;3M",
            "\x1b[<64;5;3M",
            // One notch of a mouse wheel down.
            "\x1b[<65;5;3M",
        ]
    );
}

/// A drag selects; copy-on-select copies what is shown once the selection has rested 250 ms.
#[test]
fn selection_copy() {
    use serde_json::json;
    // From the left edge of the first cell to the left edge of the eleventh.
    let (x, y) = (13.0, cell(0, 0).1);
    let script = json!([
        { "t": 0.5, "type": "drag", "x": x, "y": y, "dx": 7.0 * 10.0, "dy": 0.0, "dur": 0.2 },
        // A click elsewhere clears it; nothing more is copied.
        { "t": 1.5, "type": "tap", "x": x, "y": cell(1, 0).1 },
        // A double click selects a word.
        { "t": 2.0, "type": "double-click", "x": cell(0, 7).0, "y": y },
        { "t": 2.8, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("selection_copy", "select.json", script) else { return };
    let copies = run.with("copy");
    assert_eq!(copies.len(), 2, "{copies:?}");
    assert_eq!(copies[0]["copy"], "hello worl");
    // Released at 0.72 (the drag's end), copied once it rested 250 ms.
    let t = copies[0]["t"].as_f64().unwrap();
    assert!((0.97..1.05).contains(&t), "copied at {t}");
    assert_eq!(copies[1]["copy"], "world");
    assert!(run.writes().is_empty(), "nothing goes to the session: {:?}", run.writes());
}

/// A window dragged smaller resizes the terminal once, 90 ms after it stops.
#[test]
fn resize_debounce() {
    use serde_json::json;
    let script = json!([
        { "t": 0.5, "type": "resize", "dw": -140.0, "dh": -105.0, "dur": 0.3 },
        { "t": 1.4, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("resize_debounce", "prompt.json", script) else { return };
    let resizes = run.with("resize");
    assert_eq!(resizes.len(), 1, "{resizes:?}");
    // 820 × 595: (820 − 24) / 7 = 113 columns, (595 − 28 − 24) / 21 = 25 rows.
    assert_eq!(resizes[0]["resize"], json!([113, 25]));
    // The drag's smoothstep reaches 113 columns and 25 rows at about 0.78 s (its last steps
    // change no cell count); the terminal takes them 90 ms later.
    let t = resizes[0]["t"].as_f64().unwrap();
    assert!((0.86..0.95).contains(&t), "resized at {t}");
    assert_eq!(run.with("open")[0]["open"], json!([133, 30]));
}
