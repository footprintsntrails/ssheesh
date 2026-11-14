//! An agent's question in SSHeesh Preview's window, headless: `--demo` has Claude Code waiting
//! on db-main (Production) with the Migration 0042 question, as the 1.0 design draws it. Rune's
//! `--script` is the pointer and the keyboard; what the window holds afterwards is read from
//! Rune's element tree (`--tree`) and from the picture (`--snap`), in both appearances.
//!
//! Needs a Vulkan driver, as `window.rs` does; with none the tests say so and pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const LVP: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";

fn vulkan() -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("VK_ICD_FILENAMES").is_some() {
        return true;
    }
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().any(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some()))
}

/// What one run left.
struct Run {
    tree: Value,
    tree_text: String,
    png: PathBuf,
}

impl Run {
    fn elements(&self) -> &[Value] {
        self.tree["elements"].as_array().map(Vec::as_slice).unwrap_or_default()
    }

    /// The element whose path ends in `path` ("card-prod/ask/answers/answer-1", or a key).
    fn element(&self, path: &str) -> Option<&Value> {
        self.elements().iter().find(|e| e["path"].as_str().is_some_and(|p| p == path || p.ends_with(&format!("/{path}"))))
    }

    fn fill(&self, path: &str) -> String {
        self.element(path).unwrap_or_else(|| panic!("no element {path}"))["fill"].as_str().unwrap_or_default().to_owned()
    }

    fn rect(&self, path: &str) -> [f64; 4] {
        let r = &self.element(path).unwrap_or_else(|| panic!("no element {path}"))["rect"];
        [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap())
    }

    fn says(&self, text: &str) -> bool {
        self.tree_text.contains(text)
    }

    fn pixels(&self) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&self.png).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }
}

/// How many pixels of the rectangle `[x0, y0, x1, y1]` are within a small step of `rgb`.
fn near(pixels: &(u32, u32, Vec<u8>), [x0, y0, x1, y1]: [u32; 4], rgb: [u8; 3]) -> usize {
    let mut n = 0;
    for y in y0..y1.min(pixels.1) {
        for x in x0..x1.min(pixels.0) {
            let i = ((y * pixels.0 + x) * 4) as usize;
            if (0..3).all(|c| pixels.2[i + c].abs_diff(rgb[c]) < 24) {
                n += 1;
            }
        }
    }
    n
}

/// Runs the window with `args` at `size`, and `script` when given (ending in a mark named
/// "end"). `None`, after saying why, when there is nothing to draw with.
fn run(name: &str, args: &[&str], size: &str, script: Option<Value>) -> Option<Run> {
    if !vulkan() {
        eprintln!("{name}: skipped, no Vulkan driver here");
        return None;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("agent").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let (png, tree) = (dir.join("end.png"), dir.join("tree.json"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    cmd.env("HOME", &home).env("XDG_DATA_HOME", home.join("data")).env("XDG_CONFIG_HOME", home.join("config"));
    cmd.args(args).arg("--snap").arg(&png).arg("--tree").arg(&tree).args(["--scale", "1", "--size", size]);
    match script {
        Some(script) => {
            let path = dir.join("script.json");
            std::fs::write(&path, serde_json::to_string_pretty(&script).unwrap()).unwrap();
            cmd.arg("--script").arg(&path).args(["--at", "mark:end"]);
        }
        None => {
            cmd.args(["--frames", "20"]);
        }
    }
    if cfg!(target_os = "linux") && std::env::var_os("VK_ICD_FILENAMES").is_none() && Path::new(LVP).exists() {
        cmd.env("VK_ICD_FILENAMES", LVP);
    }
    let result = cmd.output().expect("the preview runs");
    assert!(result.status.success(), "{name}: {}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
    let tree_text = std::fs::read_to_string(&tree).unwrap();
    Some(Run { tree: serde_json::from_str(&tree_text).unwrap(), tree_text, png })
}

fn key(t: f64, name: &str, held: &str) -> Value {
    let mut e = json!({ "t": t, "type": "key", "name": name });
    if held.contains('c') {
        e["ctrl"] = json!(true);
        e["cmd"] = json!(true);
    }
    if held.contains('s') {
        e["shift"] = json!(true);
    }
    if name.len() == 1 {
        e["code"] = json!(name);
    }
    e
}

/// The pointer comes to rest on the agent's tab (the third in the demo's row).
fn rest_on_the_agents_tab(t: f64) -> Value {
    json!({ "t": t, "type": "move", "key": "tab-3" })
}

/// A row that isn't filled.
const NONE: &str = "#00000000";
const BLUE: &str = "#0071e3ff";
const BLUE_DOWN: &str = "#005bbfff";

#[test]
fn the_card_opens_under_the_waiting_agents_tab_in_both_appearances() {
    // (theme, the popover, the code box)
    for (theme, pop, code) in [("dark", "#2c2c2eff", "#1f1f21ff"), ("light", "#f6f6f8ff", "#ececeeff")] {
        let script = json!([rest_on_the_agents_tab(0.3), { "t": 1.6, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("card-{theme}"), &["--demo", "--theme", theme], "1280x800", Some(script)) else { return };
        assert_eq!(run.fill("agent-card"), pop, "{theme}");
        assert_eq!(run.fill("agent-card/ask/context"), code, "{theme}");
        // Under its tab, as wide as the design's.
        let (card, tab) = (run.rect("agent-card"), run.rect("tab-3"));
        assert_eq!(card[2], 480.0, "{theme}");
        assert!((card[0] - tab[0]).abs() < 1.0 && card[1] > tab[1] + tab[3], "{theme}: {card:?} isn't under {tab:?}");
        // Who asks and where, the command, the question, the answers, in the design's words.
        for said in [
            "Claude Code",
            "on db-main, in /srv/shop-db",
            "Prod",
            "waiting 2 min",
            "psql -h db-main -d shop -f migrations/0042_orders_index.sql",
            "Do you want to proceed?",
            "Yes, and don't ask again for psql in /srv/shop-db",
            "No, and tell Claude what to do differently…",
            "Click an answer. Keys still go to the terminal.",
        ] {
            assert!(run.says(said), "{theme}: {said} isn't on the card");
        }
        // Production, and the card hasn't got the keyboard: no answer is chosen, nothing is blue.
        for n in 1..=3 {
            assert_eq!(run.fill(&format!("agent-card/ask/answers/answer-{n}")), NONE, "{theme}: answer {n} is filled");
        }
        let px = run.pixels();
        let [x, y, w, h] = card.map(|v| v as u32);
        assert_eq!(near(&px, [x, y, x + w, y + h], [0x00, 0x71, 0xe3]), 0, "{theme}: something on the card is blue");
        // You're still on the shop's tab.
        assert!(run.element("pane-1").is_some(), "{theme}");
    }
}

#[test]
fn a_click_gives_the_card_the_keyboard_and_a_digit_answers() {
    for theme in ["dark", "light"] {
        let script = json!([
            rest_on_the_agents_tab(0.3),
            // On the card's heading: it takes the keyboard, and says what the keys do.
            { "t": 1.4, "type": "tap", "key": "who" },
            { "t": 2.0, "type": "mark", "name": "keys" },
            key(2.2, "1", ""),
            { "t": 2.6, "type": "mark", "name": "end" }
        ]);
        let Some(run) = run(&format!("answered-{theme}"), &["--demo", "--theme", theme], "1280x800", Some(script)) else { return };
        assert!(run.says("Sent 1 to Claude Code"), "{theme}: {}", run.tree_text);
        assert_eq!(run.fill("agent-card/ask/answers/answer-1"), BLUE_DOWN, "{theme}");
        assert_eq!(run.fill("agent-card/ask/answers/answer-2"), NONE, "{theme}");
    }
}

#[test]
fn the_agents_list_has_the_waiting_agent_chosen() {
    for theme in ["dark", "light"] {
        let script = json!([key(0.4, "a", "cs"), { "t": 1.0, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("list-{theme}"), &["--demo", "--theme", theme], "1280x800", Some(script)) else { return };
        assert!(run.element("agents").is_some(), "{theme}: the list didn't open");
        assert_eq!(run.fill("agents/agents-rows/agent-3"), BLUE, "{theme}");
        for said in ["Agents", "Migration 0042", "db-main", "Needs you, 2 min"] {
            assert!(run.says(said), "{theme}: {said}");
        }
        // In the middle of the window, under the bar.
        let r = run.rect("agents");
        assert_eq!((r[0], r[1], r[2]), (400.0, 72.0, 480.0), "{theme}");
    }
}

#[test]
fn return_in_the_list_opens_the_card_with_the_keyboard() {
    let script = json!([key(0.4, "a", "cs"), key(0.9, "Enter", ""), { "t": 1.6, "type": "mark", "name": "end" }]);
    let Some(run) = run("list-card", &["--demo"], "1280x800", Some(script)) else { return };
    assert!(run.element("agents").is_none());
    assert!(run.element("agent-card").is_some());
    assert!(run.says("Press 1, 2 or 3, or click an answer") && run.says("Esc to decide later"), "{}", run.tree_text);
    // Production: still nothing chosen, with the keyboard too.
    for n in 1..=3 {
        assert_eq!(run.fill(&format!("agent-card/ask/answers/answer-{n}")), NONE);
    }
}

#[test]
fn the_sheet_draws_the_card_in_its_states_in_both_appearances() {
    for theme in ["dark", "light"] {
        let Some(run) = run(&format!("sheet-{theme}"), &["--demo", "--sheet", "--theme", theme], "1280x1500", None) else { return };
        // On Production nothing is chosen; elsewhere the first answer is, and Return sends it.
        for n in 1..=3 {
            assert_eq!(run.fill(&format!("card-prod/ask/answers/answer-{n}")), NONE, "{theme}: Production, answer {n}");
        }
        assert_eq!(run.fill("card-other/ask/answers/answer-1"), BLUE, "{theme}");
        assert_eq!(run.fill("card-other/ask/answers/answer-2"), NONE, "{theme}");
        // The third answer as a field; an answer sent; one kept; a standing permission once.
        assert!(run.element("card-words/ask/answers/words-box").is_some(), "{theme}");
        assert!(run.element("card-words/ask/answers/answer-3").is_none(), "{theme}");
        assert_eq!(run.fill("card-sent/ask/answers/answer-1"), BLUE_DOWN, "{theme}");
        assert_eq!(run.fill("card-kept/ask/answers/answer-1"), BLUE, "{theme}");
        assert_eq!(run.fill("card-armed/ask/answers/answer-2"), BLUE, "{theme}");
        assert!(run.element("card-unread/ask/answers").is_none(), "{theme}");
        for said in [
            "Use a transaction and CONCURRENTLY",
            "Return sends your words",
            "Sent 1 to Claude Code",
            "Not sent: the tab is reconnecting. Your answer is kept.",
            "2 again sends it. Claude Code won't ask next time.",
            "Codex is waiting for you.",
            "Open the tab to answer",
        ] {
            assert!(run.says(said), "{theme}: {said}");
        }
    }
}
