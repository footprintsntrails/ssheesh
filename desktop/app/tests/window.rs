//! SSHeesh Preview's window headless: `--demo` (made-up servers, tabs and terminal output),
//! Rune's `--script` as the person at the mouse and keyboard, and what the window holds
//! afterwards, read from Rune's element tree (`--tree`) and from the picture (`--snap`).
//!
//! Needs a Vulkan driver (Rune draws headless frames on the GPU); with none the tests say so
//! and pass. Without a GPU, Mesa's software driver (lavapipe) does.
//!
//! Geometry at `--scale 1 --size 1280x800`, from the design: the bar is 44 pt, the sidebar 52,
//! the status bar 24. Scripts click elements by their keys and press the chords the app's own
//! shortcut table gives for the system they run on, and pictures are sampled where the tree
//! says an element is: a Mac's top row starts after the window's buttons, its sidebar with the
//! logo, and its shortcuts are ⌘'s.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use sshee_app::workbench::shortcuts::{default_keys, Shortcut};

const LVP: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";

/// Whether a Vulkan driver is there to draw with (Linux; other systems have their own GPU API).
fn vulkan() -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("VK_ICD_FILENAMES").is_some() {
        return true;
    }
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().any(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some()))
}

/// What one run left: the element tree, the picture, and the home folder it ran in.
struct Run {
    tree: Value,
    tree_text: String,
    png: PathBuf,
    home: PathBuf,
}

impl Run {
    /// The element keyed `key` (the first, in tree order).
    fn element(&self, key: &str) -> Option<&Value> {
        self.tree["elements"].as_array()?.iter().find(|e| e["key"] == key)
    }

    fn fill(&self, key: &str) -> String {
        self.element(key).unwrap_or_else(|| panic!("no element {key}"))["fill"].as_str().unwrap_or_default().to_owned()
    }

    fn label(&self, key: &str) -> String {
        self.element(key).unwrap_or_else(|| panic!("no element {key}"))["label"].as_str().unwrap_or_default().to_owned()
    }

    fn rect(&self, key: &str) -> [f64; 4] {
        let r = &self.element(key).unwrap_or_else(|| panic!("no element {key}"))["rect"];
        [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap())
    }

    /// The picture's size and its pixels, as RGBA bytes.
    fn pixels(&self) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&self.png).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }
}

/// The colour at (`x`, `y`) as `#rrggbb`.
fn at(pixels: &(u32, u32, Vec<u8>), x: u32, y: u32) -> String {
    let i = ((y * pixels.0 + x) * 4) as usize;
    format!("#{:02x}{:02x}{:02x}", pixels.2[i], pixels.2[i + 1], pixels.2[i + 2])
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

/// Runs the window with `args` and, when given, `script` (Rune script events, ending in a mark
/// named "end"); `None` (after saying why) when there is nothing to draw with.
fn run(name: &str, args: &[&str], script: Option<Value>) -> Option<Run> {
    if !vulkan() {
        eprintln!("{name}: skipped, no Vulkan driver here");
        return None;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("window").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let (png, tree) = (dir.join("end.png"), dir.join("tree.json"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    // A home of its own: whatever the window writes or reads, it isn't this machine's.
    cmd.env("HOME", &home).env("XDG_DATA_HOME", home.join("data")).env("XDG_CONFIG_HOME", home.join("config"));
    cmd.args(args).arg("--snap").arg(&png).arg("--tree").arg(&tree).args(["--scale", "1", "--size", "1280x800"]);
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
    Some(Run { tree: serde_json::from_str(&tree_text).unwrap(), tree_text, png, home })
}

/// The window reads shortcuts as a Mac does (it decides the same way: `Options::default`).
const MAC: bool = cfg!(target_os = "macos");

/// A key press for a script, with Ctrl, ⌘ (a Mac's) and Shift as said. In a Rune script "cmd"
/// is ⌘ on a Mac and Ctrl elsewhere, and "ctrl" is the Ctrl key itself.
fn press(t: f64, name: &str, ctrl: bool, meta: bool, shift: bool) -> Value {
    let mut e = json!({ "t": t, "type": "key", "name": name });
    if ctrl {
        e["ctrl"] = json!(true);
    }
    if (MAC && meta) || (!MAC && ctrl) {
        e["cmd"] = json!(true);
    }
    if shift {
        e["shift"] = json!(true);
    }
    if name.len() == 1 {
        e["code"] = json!(name);
    }
    e
}

/// The chord the app gives `shortcut` on this system (⌘T on a Mac, Ctrl+Shift+T elsewhere).
fn chord(t: f64, shortcut: Shortcut) -> Value {
    let keys = default_keys(shortcut, MAC).expect("a shortcut with keys of its own");
    let on = |b: Option<bool>| b.unwrap_or(false);
    press(t, &keys.key, on(keys.ctrl), on(keys.meta), on(keys.shift))
}

/// Ctrl+Tab: the next tab, on every system.
fn ctrl_tab(t: f64) -> Value {
    press(t, "Tab", true, false, false)
}

/// A click on the element keyed `key`, wherever this system's window puts it.
fn tap(t: f64, key: &str) -> Value {
    json!({ "t": t, "type": "tap", "key": key })
}

/// The inside of the terminal on screen, as `[x0, y0, x1, y1]` in the picture: its text, away
/// from its edges.
fn text_area(run: &Run) -> [u32; 4] {
    let [x, y, w, h] = run.rect("term");
    [x as u32 + 12, y as u32 + 12, (x + w.min(660.0)) as u32, (y + h.min(440.0)) as u32]
}

#[test]
fn the_demo_is_the_designs_scene_in_both_appearances() {
    // (theme, the frame, the page, the tab you're on)
    for (theme, frame, page, tab) in [("dark", "#28282a", "#1c1c1e", "#3a3a3c"), ("light", "#ececee", "#ffffff", "#ffffff")] {
        let Some(run) = run(&format!("scene-{theme}"), &["--demo", "--theme", theme], None) else { return };
        // The window's parts are where the design puts them, in its colours.
        assert_eq!(run.fill("window"), format!("{frame}ff"), "{theme}");
        assert_eq!(run.fill("term"), format!("{page}ff"), "{theme}");
        assert_eq!(run.rect("term"), [52.0, 44.0, 1228.0, 732.0], "{theme}");
        assert_eq!(run.rect("status"), [52.0, 776.0, 1228.0, 24.0], "{theme}");
        assert_eq!(run.rect("rail")[2], 52.0, "{theme}");
        // One row of tabs for both servers; the one on screen is filled.
        assert_eq!(run.label("tab-1"), "~/shop, web-7731");
        assert_eq!(run.fill("tab-1"), format!("{tab}ff"), "{theme}");
        assert_eq!(run.label("tab-2"), "Files, web-7731");
        assert_eq!(run.label("tab-3"), "Migration 0042, db-main, Needs you");
        assert!(run.element("sep-3").is_some(), "no divider before db-main's tab");
        assert!(run.rect("tab-1")[0] < run.rect("tab-2")[0] && run.rect("tab-2")[0] < run.rect("tab-3")[0]);
        // The sidebar: five machines, the agent's dot on db-main, the main menu, Tor.
        for (i, name) in ["This computer", "web-7731", "web-staging", "backups", "db-main"].iter().enumerate() {
            assert_eq!(run.label(&format!("server-{i}")), *name);
        }
        // (On Windows the window sits inside the frame Rune draws, so the path has more in front.)
        let waits = run.element("waits").map(|e| e["path"].as_str().unwrap().to_owned()).unwrap_or_default();
        assert!(waits.ends_with("window/body/rail/server-4/waits"), "{theme}: the dot is at {waits:?}");
        for name in ["Main menu", "Search", "Add a server", "Security check", "Tor", "Settings", "New terminal", "Hide sidebar"] {
            assert!(run.tree_text.contains(&format!("\"{name}\"")), "{theme}: nothing is named {name}");
        }
        // The status bar's numbers and the title's label are there to read. (Rune's tree file
        // carries no terminal text; sshee-ui's own tests read that from the screen reader's
        // tree, secrets as dots included.)
        for said in ["CPU", "38%", "37 d 5 h", "1.6 MB/s", "84 ms", "Prod"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // And the picture: the frame, the page and the sidebar are drawn in those colours.
        let px = run.pixels();
        assert_eq!((px.0, px.1), (1280, 800));
        assert_eq!(at(&px, 1200, 22), frame, "{theme}: the bar");
        assert_eq!(at(&px, 26, 500), frame, "{theme}: the sidebar");
        assert_eq!(at(&px, 900, 600), page, "{theme}: the page");
        assert_eq!(at(&px, 900, 790), page, "{theme}: the status bar");
        // The terminal drew its text: the prompts' green is on the page, in the scheme's shade.
        let green = if theme == "dark" { [0x7f, 0xc9, 0x8b] } else { [0x1f, 0x7a, 0x3a] };
        assert!(near(&px, [64, 56, 400, 480], green) > 200, "{theme}: no prompt is drawn in the terminal");
        // --demo keeps nothing: no folder of its own, no tabs. (The graphics driver may leave
        // its shader cache in the home's `.cache`.)
        let left: Vec<String> = std::fs::read_dir(&run.home).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n != ".cache").collect();
        assert!(left.is_empty(), "{theme}: the demo left {left:?} in its home");
    }
}

#[test]
fn clicks_and_shortcuts_drive_the_tabs() {
    let script = json!([
        // A click on db-main's icon: its tab, the agent's.
        tap(0.3, "server-4"),
        // A new terminal there.
        chord(0.8, Shortcut::NewTerminal),
        // Around the end of the row to the first tab, and close it.
        ctrl_tab(1.2),
        chord(1.6, Shortcut::CloseTab),
        { "t": 2.2, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("drive", &["--demo", "--theme", "dark"], Some(script)) else { return };
    // ~/shop is closed; Files came up; db-main has the agent's tab, seen now, and a new one.
    assert!(run.element("tab-1").is_none(), "the closed tab is still drawn");
    assert_eq!(run.label("tab-2"), "Files, web-7731");
    assert_eq!(run.fill("tab-2"), "#3a3a3cff");
    assert_eq!(run.label("tab-3"), "Migration 0042, db-main");
    // The agent's tab is db-main's first terminal, whatever it's called: the new one is its second.
    assert_eq!(run.label("tab-4"), "Terminal 2, db-main");
    assert!(run.element("waits").is_none(), "the agent's dot stayed after its tab was looked at");
    // Files came up in the terminal's place, on its server's home folder.
    assert!(run.element("term").is_none());
    assert!(run.element("files-list").is_some() && run.tree_text.contains("README.md"));
    // The machine on screen is still web-7731, labelled Production in the title.
    assert!(run.tree_text.contains("web-7731"));
}

#[test]
fn a_tab_shows_its_own_terminal() {
    // db-main's agent tab, then a middle click on Files closes it without leaving.
    let script = json!([
        tap(0.3, "server-4"),
        { "t": 0.9, "type": "tap", "key": "tab-2", "button": "middle" },
        { "t": 1.6, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("agent", &["--demo", "--theme", "dark"], Some(script)) else { return };
    assert!(run.element("tab-2").is_none(), "Files wasn't closed by the middle click");
    assert_eq!(run.fill("tab-3"), "#3a3a3cff");
    assert!(run.element("pane-3").is_some());
    // The page shows that tab's terminal, not the shop's: the shop's listing has yellow lines
    // (commit ids, a restarting container), the agent's question has none.
    let yellow = [0xd9, 0xb5, 0x63];
    let Some(shop) = self::run("agent-shop", &["--demo", "--theme", "dark"], None) else { return };
    assert!(near(&shop.pixels(), text_area(&shop), yellow) > 200, "the shop's terminal isn't as the fixture draws it");
    assert_eq!(near(&run.pixels(), text_area(&run), yellow), 0, "the shop's terminal is still on the page");
    assert!(near(&run.pixels(), text_area(&run), [0x7f, 0xc9, 0x8b]) > 50, "the agent's terminal drew nothing");
    // db-main's numbers: its disk is past the line, so the bar says so in one bold item.
    assert!(run.tree_text.contains("Disk 92%"), "the full disk isn't marked");
}

#[test]
fn the_component_sheet_draws_every_state() {
    let Some(run) = run("sheet", &["--demo", "--sheet"], None) else { return };
    assert_eq!(run.fill("t-on"), "#3a3a3cff");
    assert_eq!(run.fill("m-2"), "#0071e3ff");
    assert_eq!(run.fill("m-3"), "#005bbfff");
    assert_eq!(run.fill("pop"), "#2c2c2eff");
    for key in ["t-rest", "t-off", "t-wait", "t-re", "t-gone", "r-sel", "r-wait", "r-tor-on", "r-tor-start", "r-tor-bad", "b-side", "s-disk"] {
        assert!(run.element(key).is_some(), "{key}");
    }
    for said in ["Reconnecting…", "Disconnected", "Needs you", "Esc to decide later", "Disk 92%"] {
        assert!(run.tree_text.contains(said), "{said}");
    }
}

// ---- what the terminal and the row of tabs gained: find, links, scrolling, "while you were
// away", the pane's menu, streamer mode, the tab menu and list, two tabs side by side. Each in
// both appearances; what the tree can't say (a mark's colour) is read from the picture.

/// web-staging's icon in the sidebar: its first terminal plays a long log (`demo.rs`).
const STAGING: &str = "server-2";

/// The script's last event: the picture and the tree are taken here.
fn end(t: f64) -> Value {
    json!({ "t": t, "type": "mark", "name": "end" })
}

/// Runs `script` over the demo in the dark and the light appearance, and hands each run to
/// `check` with whether it's the dark one.
fn in_both(name: &str, script: impl Fn() -> Value, check: impl Fn(&Run, bool)) {
    for theme in ["dark", "light"] {
        let Some(run) = run(&format!("{name}-{theme}"), &["--demo", "--theme", theme], Some(script())) else { return };
        check(&run, theme == "dark");
    }
}

/// The longest run of pixels within a small step of `rgb` along any one row of the rectangle
/// `[x0, y0, x1, y1]`: a filled block has long ones, text has none.
fn longest_run(pixels: &(u32, u32, Vec<u8>), [x0, y0, x1, y1]: [u32; 4], rgb: [u8; 3]) -> usize {
    let mut best = 0;
    for y in y0..y1.min(pixels.1) {
        let mut run = 0;
        for x in x0..x1.min(pixels.0) {
            let i = ((y * pixels.0 + x) * 4) as usize;
            run = if (0..3).all(|c| pixels.2[i + c].abs_diff(rgb[c]) < 24) { run + 1 } else { 0 };
            best = best.max(run);
        }
    }
    best
}

/// The whole terminal on screen, as a rectangle of the picture.
fn whole_terminal(run: &Run) -> [u32; 4] {
    let [x, y, w, h] = run.rect("term");
    [x as u32, y as u32, (x + w) as u32, (y + h) as u32]
}

#[test]
fn while_you_were_away_is_a_line_above_the_first_new_line() {
    // The shop's connection drops and comes back by itself (the fixture); a moment later the
    // line is there.
    in_both("away", || json!([end(1.6)]), |run, dark| {
        assert_eq!(run.label("away"), "While you were away (41 min): 2 new lines, 1 error");
        let (line, above, below, term) = (run.rect("away"), run.rect("grid-above"), run.rect("grid-below"), run.rect("term"));
        // One row of 18 points between two rows of the grid, touching both, and nothing pushed
        // off the page.
        assert_eq!(line[3], 18.0);
        assert_eq!(above[1] + above[3], line[1]);
        assert_eq!(line[1] + line[3], below[1]);
        assert!(above[1] >= term[1] + 2.0 && below[1] + below[3] <= term[1] + term[3] - 2.0, "{above:?} {below:?} in {term:?}");
        // After its words, a hairline to the row's end.
        let px = run.pixels();
        let hair = if dark { "#3a3a3c" } else { "#d2d2d7" };
        let y = line[1] as u32;
        assert!((y + 7..y + 11).any(|y| at(&px, 900, y) == hair), "no hairline after the words");
        // The tab came back from "Reconnecting…" by itself.
        assert_eq!(run.label("tab-1"), "~/shop, web-7731");
    });
}

#[test]
fn find_marks_what_it_finds_and_goes_to_it() {
    let script = || {
        json!([
            tap(0.3, STAGING),
            chord(0.9, Shortcut::Find { always: true }),
            { "t": 1.2, "type": "type", "text": "error" },
            // Return: the older of the two.
            press(1.8, "Enter", false, false, false),
            end(2.4)
        ])
    };
    in_both("find", script, |run, dark| {
        // The bar sits at the page's top right, and says which match it is on.
        let (bar, term) = (run.rect("find"), run.rect("term"));
        assert_eq!((bar[0] + bar[2], bar[1]), (term[0] + term[2] - 20.0, term[1] + 8.0));
        assert!(run.element("find-input").is_some());
        assert!(run.tree_text.contains("1 of 2"), "the bar doesn't count its matches");
        // The older match was above the screen: the view went up to it, and the scroll bar
        // with it.
        let (track, thumb) = (run.rect("scroll"), run.rect("thumb"));
        assert!(thumb[1] > track[1] && thumb[1] + thumb[3] < track[1] + track[3] - 20.0, "{thumb:?} in {track:?}");
        // The match the bar is on is a block in the text's colour (the others are quieter).
        let ink = if dark { [0xeb, 0xeb, 0xf0] } else { [0x1d, 0x1d, 0x1f] };
        assert!(longest_run(&run.pixels(), whole_terminal(run), ink) >= 30, "the current match isn't marked");
        // The shell said which folder it is in: the tab is named after it.
        assert_eq!(run.label("tab-4"), "~/shop, web-staging");
    });
}

#[test]
fn the_link_under_the_pointer_says_where_it_goes() {
    // The log's "payments runbook" is a link a program marked (OSC 8) over other words.
    let script = || json!([tap(0.3, STAGING), { "t": 0.9, "type": "move", "x": 260.0, "y": 507.0 }, end(1.9)]);
    in_both("link", script, |run, _| {
        let key = if MAC { "Cmd" } else { "Ctrl" };
        let hint = format!("Goes to https://docs.shop.example/runbooks/payments. {key}+click to open");
        assert!(run.tree_text.contains(&hint), "no hint says {hint:?}");
        // Under the link, inside the page.
        let (hint, term) = (run.rect("link-hint"), run.rect("term"));
        assert!(hint[1] > 507.0 && hint[1] < 540.0 && hint[0] + hint[2] <= term[0] + term[2], "{hint:?}");
    });
}

#[test]
fn the_terminals_menu_opens_on_the_secondary_button() {
    let script = || json!([tap(0.3, STAGING), { "t": 0.9, "type": "tap", "x": 500.0, "y": 300.0, "button": "secondary" }, end(1.5)]);
    in_both("menu", script, |run, dark| {
        let menu = run.rect("menu");
        assert_eq!((menu[0], menu[1]), (500.0, 300.0));
        assert_eq!(run.fill("menu"), if dark { "#2c2c2eff" } else { "#f6f6f8ff" });
        for (key, label) in [("menu-copy", "Copy"), ("menu-paste", "Paste"), ("menu-select-all", "Select all"), ("menu-find", "Find"), ("menu-clear", "Clear"), ("menu-new", "New terminal here")] {
            assert!(run.element(key).is_some(), "{key}");
            assert!(run.tree_text.contains(&format!("\"{label}\"")), "{label}");
        }
    });
}

#[test]
fn streamer_mode_hides_every_name_the_window_draws() {
    in_both("streamer", || json!([chord(0.3, Shortcut::Streamer), end(1.2)]), |run, _| {
        // Servers are numbered: in the title, the tabs, and the sidebar's names.
        assert_eq!(run.label("tab-1"), "~/shop, Server 1");
        assert_eq!(run.label("tab-3"), "Migration 0042, Server 4, Needs you");
        assert_eq!(run.label("server-1"), "Server 1");
        assert_eq!(run.label("server-0"), "This computer");
        for name in ["web-7731", "web-staging", "backups", "db-main", "203.0.113", "deploy"] {
            assert!(!run.tree_text.contains(name), "{name} is still in the tree");
        }
        // The switch says it's on.
        assert_eq!(run.label("streamer"), "Streamer mode");
        assert!(run.tree_text.contains("Streamer mode is on"));
    });
}

#[test]
fn a_tab_has_a_menu_and_a_crowded_row_has_a_list() {
    let script = || json!([{ "t": 0.3, "type": "tap", "key": "tab-2", "button": "secondary" }, end(0.9)]);
    in_both("tab-menu", script, |run, _| {
        let (menu, tab) = (run.rect("tab-menu"), run.rect("tab-2"));
        assert!(menu[1] >= tab[1] && menu[0] >= tab[0] && menu[0] <= tab[0] + tab[2], "{menu:?} isn't at {tab:?}");
        for label in ["Show side by side", "Duplicate tab", "Close tab", "Close other tabs", "Close tabs to the right"] {
            assert!(run.tree_text.contains(&format!("\"{label}\"")), "{label}");
        }
    });
    // Twenty-two more terminals: the row can't hold them, so a run of them is in sight and
    // the list has the rest, with a dot for the agent's tab, which waits out of sight.
    let script = || {
        let mut events = vec![tap(0.3, STAGING)];
        events.extend((0..22).map(|i| chord(0.5 + 0.08 * f64::from(i), Shortcut::NewTerminal)));
        events.extend([tap(2.6, "tab-list"), end(3.2)]);
        Value::Array(events)
    };
    in_both("tab-list", script, |run, _| {
        assert!(run.element("tab-list-menu").is_some() && run.element("tab-list-waits").is_some());
        assert!(run.element("tab-3").is_none(), "the agent's tab should be out of sight");
        assert!(run.tree_text.contains("Migration 0042, db-main, Needs you"));
        let window = run.rect("window");
        for key in ["tab-26", "tab-list", "new-tab", "streamer"] {
            let r = run.rect(key);
            assert!(r[0] >= 0.0 && r[0] + r[2] <= window[2], "{key} is off the window: {r:?}");
        }
        assert!(run.rect("tab-26")[2] >= 60.0, "the tab on screen was squeezed");
    });
}

#[test]
fn two_tabs_side_by_side_share_the_page() {
    // web-staging has one tab: the shortcut opens a terminal beside it.
    let script = || json!([tap(0.3, STAGING), chord(0.9, Shortcut::SideBySide), end(1.6)]);
    in_both("side", script, |run, dark| {
        let (left, right, divider) = (run.rect("side-4"), run.rect("side-5"), run.rect("side-divider"));
        assert!((left[2] - right[2]).abs() < 8.0, "{left:?} {right:?}");
        assert_eq!(left[0] + left[2], divider[0]);
        assert_eq!(divider[0] + divider[2], right[0]);
        assert!(run.element("pane-4").is_some() && run.element("pane-5").is_some());
        assert!(run.element("side-close-4").is_some() && run.element("side-close-5").is_some());
        // Both are tabs of their own; the one you're on is lit, the one beside it a little.
        assert_eq!(run.fill("tab-4"), if dark { "#3a3a3cff" } else { "#ffffffff" });
        // (The row is crowded with five tabs, so the machine is named once before its group.)
        assert_eq!(run.label("tab-5"), "Terminal 2");
        assert_ne!(run.fill("tab-5"), "#00000000", "the tab shown beside isn't lit");
        // The divider's line is drawn between them.
        let x = (divider[0] + divider[2] / 2.0) as u32;
        assert_eq!(at(&run.pixels(), x, 400), if dark { "#111113" } else { "#d2d2d7" });
    });
}

#[test]
fn new_output_while_scrolled_up_offers_the_bottom() {
    // Three notches up the log; three seconds in, the fixture prints two more lines.
    let script = || json!([tap(0.3, STAGING), { "t": 0.9, "type": "wheel", "x": 600.0, "y": 400.0, "dx": 0.0, "dy": 360.0, "precise": false }, end(3.5)]);
    in_both("scroll", script, |run, dark| {
        assert_eq!(run.label("to-bottom"), "Jump to the bottom");
        assert_eq!(run.fill("to-bottom"), if dark { "#2c2c2eff" } else { "#f6f6f8ff" });
        let (button, term, track, thumb) = (run.rect("to-bottom"), run.rect("term"), run.rect("scroll"), run.rect("thumb"));
        assert!(button[0] + button[2] <= term[0] + term[2] - 20.0 && button[1] + button[3] <= term[1] + term[3] - 10.0, "{button:?}");
        // The scroll bar is at the page's right edge, its thumb off the bottom.
        assert_eq!(track[0] + track[2], term[0] + term[2]);
        assert!(thumb[1] + thumb[3] < track[1] + track[3], "{thumb:?} in {track:?}");
    });
}

/// The surface popovers are drawn on, and the page, in each appearance.
const LOOKS: [(&str, &str, &str); 2] = [("dark", "#2c2c2e", "#1c1c1e"), ("light", "#f6f6f8", "#ffffff")];

#[test]
fn the_main_menu_lists_servers_agents_and_closed_tabs_in_both_appearances() {
    for (theme, pop, page) in LOOKS {
        let script = json!([
            // A middle click closes Files, then the logo: the main menu.
            { "t": 0.3, "type": "tap", "x": 440.0, "y": 22.0, "button": "middle" },
            { "t": 0.7, "type": "tap", "key": "home" },
            // An arrow shows the keyboard's highlight on the first tile.
            { "t": 1.0, "type": "key", "name": "Down" },
            { "t": 1.5, "type": "mark", "name": "end" }
        ]);
        let Some(run) = run(&format!("home-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        // The search field in the middle, as wide as the palette, on the popover's surface.
        let search = run.rect("home-search");
        assert_eq!((search[2], search[3]), (560.0, 44.0), "{theme}");
        assert!((search[0] + search[2] / 2.0 - (52.0 + 1228.0 / 2.0)).abs() < 1.0, "{theme}: the field isn't centred: {search:?}");
        assert_eq!(run.fill("home-search"), format!("{pop}ff"), "{theme}");
        // Five machines and "Add a server", in one row; the agent's dot on db-main.
        for (i, name) in ["This computer", "web-7731", "web-staging", "backups", "db-main"].iter().enumerate() {
            assert_eq!(run.label(&format!("tile-{i}")), *name);
        }
        assert_eq!(run.label("tile-add"), "Add a server");
        assert_eq!(run.rect("tile-0")[1], run.rect("tile-add")[1], "{theme}: the tiles wrapped");
        let dots: Vec<&str> = run.tree["elements"].as_array().unwrap().iter().filter(|e| e["key"] == "waits").filter_map(|e| e["path"].as_str()).collect();
        assert!(dots.iter().any(|path| path.contains("tile-4")), "{theme}: {dots:?}");
        // What the agent is doing, the tab closed a moment ago, and what waits to be brought in.
        for said in ["Running now", "Migration 0042", "on db-main", "Needs you", "Recently closed", "on web-7731", "Reopen", "2 servers in ~/.ssh/config to pick from", "1 server from SSHeesh 0.10 to bring over", "Prod", "Staging"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't on the main menu");
        }
        assert!(run.element("run-0").is_some() && run.element("closed-0").is_some() && run.element("term").is_none(), "{theme}");
        // The page under it all is the page's colour.
        assert_eq!(at(&run.pixels(), 1200, 700), page, "{theme}");
    }
}

#[test]
fn the_palette_hangs_under_the_title_with_files_first_in_both_appearances() {
    for (theme, pop, _) in LOOKS {
        let script = json!([chord(0.3, Shortcut::Palette), { "t": 0.9, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("palette-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        let (title, palette) = (run.rect("title"), run.rect("palette"));
        assert_eq!(palette[2], 560.0, "{theme}");
        assert!((palette[0] - title[0]).abs() < 1.0 && palette[1] > title[1] + title[3], "{theme}: {title:?} {palette:?}");
        assert_eq!(run.fill("palette"), format!("{pop}ff"), "{theme}");
        // One blue row, the first: Files.
        assert_eq!(run.fill("pal-0"), "#0071e3ff", "{theme}");
        assert_ne!(run.fill("pal-1"), "#0071e3ff", "{theme}");
        for said in ["Search or run on web-7731", "On web-7731", "Files", "df -h -x tmpfs -x devtmpfs", "snippet", "Go to", "Migration 0042", "Needs you", "Window", "Settings"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the palette");
        }
        // The terminal is still under it.
        assert!(run.element("term").is_some(), "{theme}");
    }
}

#[test]
fn a_typed_command_is_offered_for_the_production_server_and_not_chosen() {
    let script = json!([
        chord(0.3, Shortcut::Palette),
        { "t": 0.6, "type": "type", "text": "docker compose ps" },
        { "t": 1.6, "type": "key", "name": "Enter" },
        { "t": 2.0, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("palette-run", &["--demo"], Some(script)) else { return };
    // Return did nothing: the palette is open, its one row isn't the blue one, and it says why.
    assert!(run.element("palette").is_some());
    assert_ne!(run.fill("pal-0"), "#0071e3ff");
    assert!(run.element("pal-1").is_none());
    for said in ["docker compose ps", "run on web-7731", "Prod", "Nothing runs by itself"] {
        assert!(run.tree_text.contains(said), "{said}");
    }
}

#[test]
fn add_a_server_shows_its_form_and_its_sources_in_both_appearances() {
    for (theme, pop, _) in LOOKS {
        let script = json!([
            { "t": 0.3, "type": "tap", "key": "add-server" },
            { "t": 0.8, "type": "type", "text": "ssh root@203.0.113.77 -p 2222" },
            { "t": 2.2, "type": "mark", "name": "end" }
        ]);
        let Some(run) = run(&format!("add-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        let dialog = run.rect("add-dialog");
        assert_eq!(dialog[2], 520.0, "{theme}");
        assert!((dialog[0] + dialog[2] / 2.0 - 640.0).abs() < 1.0, "{theme}: the dialog isn't centred: {dialog:?}");
        assert_eq!(run.fill("add-dialog"), format!("{pop}ff"), "{theme}");
        for said in ["Add a server", "Bring over your servers from 0.10", "From ~/.ssh/config", "2 new", "Import from XPipe", "Reads as root@203.0.113.77:2222", "SSH key", "Password", "Cancel", "Add and connect"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the dialog");
        }
        for key in ["add-address", "add-port", "add-user", "add-key-file", "add-name", "add-save", "add-cancel"] {
            assert!(run.element(key).is_some(), "{theme}: {key}");
        }
    }
}

#[test]
fn a_servers_menu_and_card_open_beside_its_icon_in_both_appearances() {
    for (theme, pop, _) in LOOKS {
        // The card: the pointer rests on db-main's icon.
        let script = json!([{ "t": 0.3, "type": "move", "key": "server-4" }, { "t": 1.4, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("card-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        let (icon, card) = (run.rect("server-4"), run.rect("server-card"));
        assert_eq!(card[2], 262.0, "{theme}");
        assert!(card[0] >= icon[0] + icon[2] && (card[1] - icon[1]).abs() < 40.0, "{theme}: {icon:?} {card:?}");
        assert_eq!(run.fill("server-card"), format!("{pop}ff"), "{theme}");
        for said in ["db-main", "Prod", "postgres@203.0.113.31, through Tor", "1 tab open", "Claude Code needs you, 2 min"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't on the card");
        }
        // The menu: a right click there.
        let script = json!([{ "t": 0.3, "type": "secondary-click", "key": "server-4" }, { "t": 0.6, "type": "key", "name": "Down" }, { "t": 1.0, "type": "mark", "name": "end" }]);
        let Some(run) = self::run(&format!("menu-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        assert_eq!(run.fill("server-menu"), format!("{pop}ff"), "{theme}");
        assert_eq!(run.fill("sm-0"), "#0071e3ff", "{theme}: the arrow didn't highlight the first row");
        for said in ["New terminal", "Files", "Rename…", "Icon and colour…", "Production", "Staging", "Development", "Remove server…"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the menu");
        }
        assert!(run.element("server-card").is_none(), "{theme}: the card stayed under the menu");
    }
}

// ---- Files

/// Geometry of the Files page at 1280x800: its toolbar is 40 pt under the bar, the header 26,
/// then ".." and the rows, 26 pt each from y = 140; the Files tab of the demo is `tab-2`.
#[test]
fn files_lists_opens_and_asks_in_both_appearances() {
    for (theme, page, blue) in [("dark", "#1c1c1e", [0x00, 0x71, 0xe3]), ("light", "#ffffff", [0x00, 0x71, 0xe3])] {
        // The Files tab, then a click on deploy.sh: the list, one row selected.
        let script = json!([{ "t": 0.3, "type": "tap", "key": "tab-2" }, { "t": 0.8, "type": "tap", "x": 300.0, "y": 231.0 }, { "t": 1.4, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("files-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        assert_eq!(run.rect("files-toolbar"), [52.0, 44.0, 1228.0, 40.0], "{theme}");
        assert_eq!(run.rect("files-status")[3], 24.0, "{theme}");
        for said in ["Folder path", "backups", "logs", "shop", "deploy.sh", "nginx.conf", "README.md", "8 items, 4 hidden", "1 selected, 256 B", "Today 11:02", "Oct 10", "Bookmarks and places", "Upload files"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // Dot files stay hidden until asked for.
        assert!(!run.tree_text.contains(".bashrc"), "{theme}");
        let px = run.pixels();
        assert_eq!(at(&px, 700, 500), page, "{theme}: the list's background");
        assert!(near(&px, [60, 220, 1270, 242], blue) > 20_000, "{theme}: the selected row isn't blue");
        assert_eq!(near(&px, [60, 246, 1270, 268], blue), 0, "{theme}: a second row is blue");
    }
    // A double click opens a file beside the list; typing and saving the one that changed on
    // the server offers the three ways out, and Delete asks first, naming what goes.
    let script = json!([
        { "t": 0.3, "type": "tap", "key": "tab-2" },
        { "t": 0.8, "type": "double-click", "x": 300.0, "y": 205.0 },
        { "t": 1.3, "type": "type", "text": "ch" },
        press(1.6, "Enter", false, false, false),
        { "t": 2.0, "type": "tap", "x": 1000.0, "y": 300.0 },
        { "t": 2.3, "type": "type", "text": "x" },
        press(2.6, "s", !MAC, MAC, false),
        { "t": 3.2, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-conflict", &["--demo"], Some(script)) else { return };
    assert_eq!(run.rect("files-preview")[2], 480.0);
    for said in ["CHANGELOG.md", "This file changed on the server since you opened it, so it wasn't saved over.", "Save over it", "Load what's there", "Save mine as a copy", "Unsaved changes"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    let script = json!([
        { "t": 0.3, "type": "tap", "key": "tab-2" },
        { "t": 0.8, "type": "tap", "x": 300.0, "y": 231.0 },
        press(1.0, "Delete", false, false, false),
        { "t": 1.6, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-delete", &["--demo"], Some(script)) else { return };
    assert_eq!(run.fill("files-question"), "#2c2c2eff");
    for said in ["Delete “deploy.sh”?", "It's deleted on the server right away. This can't be undone.", "Cancel"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
}

#[test]
fn a_download_shows_its_card_until_it_is_stopped() {
    // Into backups, the menu on its one file, Download: the demo's endless download.
    let events = |stop: bool| {
        let mut e = vec![
            tap(0.3, "tab-2"),
            json!({ "t": 0.8, "type": "double-click", "x": 300.0, "y": 153.0 }),
            json!({ "t": 1.4, "type": "secondary-click", "x": 300.0, "y": 153.0 }),
            json!({ "t": 1.8, "type": "tap", "x": 340.0, "y": 198.0 }),
        ];
        if stop {
            // The card's cross.
            e.push(json!({ "t": 2.4, "type": "tap", "x": 1254.0, "y": 715.0 }));
        }
        e.push(json!({ "t": 3.0, "type": "mark", "name": "end" }));
        Value::Array(e)
    };
    let Some(run) = run("files-download", &["--demo"], Some(events(false))) else { return };
    assert!(run.element("transfer-1").is_some(), "no card for the download");
    for said in ["shop-db.sql.gz", "38%", "1.2 MB/s"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    let [x, y, w, h] = run.rect("transfer-stop-1");
    assert!((x + w / 2.0 - 1254.0).abs() < 10.0 && (y + h / 2.0 - 715.0).abs() < 10.0, "the stop button moved: {x} {y}");
    let Some(run) = self::run("files-download-stop", &["--demo"], Some(events(true))) else { return };
    assert!(run.element("transfer-1").is_none(), "the card stayed after Stop");
    assert!(run.tree_text.contains("Download stopped"), "nothing says the download stopped");
}

#[test]
fn files_move_by_dragging_rename_and_upload() {
    let files = tap(0.3, "tab-2");
    // deploy.sh dragged onto logs: while held the folder is ringed, let go it moves there.
    let drag = |up: bool| {
        let mut e = vec![
            files.clone(),
            json!({ "t": 0.8, "type": "down", "x": 300.0, "y": 231.0 }),
            json!({ "t": 0.9, "type": "move", "x": 310.0, "y": 210.0 }),
            json!({ "t": 1.0, "type": "move", "x": 320.0, "y": 180.0 }),
        ];
        if up {
            e.push(json!({ "t": 1.2, "type": "up", "x": 320.0, "y": 180.0 }));
        }
        e.push(json!({ "t": 1.8, "type": "mark", "name": "end" }));
        Value::Array(e)
    };
    let Some(run) = run("files-dragging", &["--demo"], Some(drag(false))) else { return };
    assert!(run.tree_text.contains("Let go to move deploy.sh here"), "the folder under the pointer doesn't take the file");
    let Some(run) = self::run("files-dropped", &["--demo"], Some(drag(true))) else { return };
    for said in ["Moved deploy.sh to logs", "Undo", "7 items, 4 hidden"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    // F2 on nginx.conf, a new name for all but its ending, Return.
    let script = json!([
        files.clone(),
        { "t": 0.8, "type": "tap", "x": 300.0, "y": 283.0 },
        press(1.0, "F2", false, false, false),
        { "t": 1.3, "type": "type", "text": "site" },
        press(1.6, "Enter", false, false, false),
        { "t": 2.2, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-rename", &["--demo"], Some(script)) else { return };
    assert!(run.tree_text.contains("site.conf") && !run.tree_text.contains("nginx.conf"), "the file wasn't renamed");
    // Upload: the system's picker, answered by the script; the file is in the list afterwards.
    let script = json!([
        files,
        { "t": 0.8, "type": "tap", "x": 1258.0, "y": 64.0 },
        { "t": 1.2, "type": "file-dialog", "path": "/home/dev/notes.txt" },
        { "t": 1.8, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-upload", &["--demo"], Some(script)) else { return };
    for said in ["Uploaded notes.txt", "notes.txt", "9 items, 4 hidden"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
}
