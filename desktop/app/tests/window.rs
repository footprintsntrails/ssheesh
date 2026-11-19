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
    run_in(name, args, script, |_| {})
}

/// [`run`], with `prepare` putting files into the run's home folder before the window starts.
fn run_in(name: &str, args: &[&str], script: Option<Value>, prepare: impl FnOnce(&Path)) -> Option<Run> {
    if !vulkan() {
        eprintln!("{name}: skipped, no Vulkan driver here");
        return None;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("window").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    prepare(&home);
    let (png, tree) = (dir.join("end.png"), dir.join("tree.json"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    // A home of its own: whatever the window writes or reads, it isn't this machine's.
    cmd.env("HOME", &home).env("XDG_DATA_HOME", home.join("data")).env("XDG_CONFIG_HOME", home.join("config"));
    // Windows finds the home folder without HOME: the copy's own folder is named outright.
    cmd.env("SSHEE_PREVIEW_DIR", home.join(".sshee-preview"));
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
        for name in ["Main menu", "Search", "Add a server", "Security check", "Tor", "Settings", "New tab", "Hide sidebar"] {
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

/// Where Settings' list of sections starts under its search field, a row's height, and the
/// space more between its bunches (Layout to Keyboard, Privacy to Tor, Updates and About).
const FIRST_SECTION: f64 = 106.0;
const SECTION_ROW: f64 = 30.0;
const SECTION_BUNCH: f64 = 10.0;

/// The centre of the element keyed `key` in `run`.
fn centre(run: &Run, key: &str) -> (f64, f64) {
    let [x, y, w, h] = run.rect(key);
    (x + w / 2.0, y + h / 2.0)
}

#[test]
fn settings_is_a_page_in_the_window_in_both_appearances() {
    // (theme, the frame, the page, a box of rows, the section on screen)
    for (theme, frame, page, group) in [("dark", "#28282a", "#1c1c1e", "#242426"), ("light", "#ececee", "#ffffff", "#f5f5f7")] {
        let script = json!([tap(0.3, "settings-gear"), end(1.0)]);
        let Some(run) = run(&format!("settings-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        // The page takes the terminal's place; the bar, the sidebar and the status bar stay.
        assert!(run.element("term").is_none(), "{theme}: the terminal is still drawn");
        assert_eq!(run.rect("settings"), [52.0, 44.0, 1228.0, 732.0], "{theme}");
        assert_eq!(run.rect("status"), [52.0, 776.0, 1228.0, 24.0], "{theme}");
        assert_eq!(run.rect("settings-nav")[2], 232.0, "{theme}");
        assert_eq!(run.fill("window"), format!("{frame}ff"), "{theme}");
        // The list: ten sections, Layout first and picked in the theme's quiet fill.
        for (n, (id, name)) in [
            ("layout", "Layout"),
            ("appearance", "Appearance"),
            ("terminal", "Terminal"),
            ("keyboard", "Keyboard"),
            ("privacy", "Privacy"),
            ("files", "Files"),
            ("agents", "Agents and notifications"),
            ("tor", "Tor"),
            ("updates", "Updates"),
            ("about", "About"),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(run.label(&format!("nav-{id}")), *name, "{theme}");
            let (_, y) = centre(&run, &format!("nav-{id}"));
            let bunch = match n {
                0..=3 => 0.0,
                4..=7 => 1.0,
                _ => 2.0,
            };
            assert_eq!(y, FIRST_SECTION + SECTION_ROW * n as f64 + SECTION_BUNCH * bunch, "{theme}: {name}");
        }
        // The picked section is a quiet fill, not the accent blue, and it differs from the rest.
        assert_ne!(run.fill("nav-layout"), "#0071e3ff", "{theme}");
        assert_ne!(run.fill("nav-layout"), run.fill("nav-terminal"), "{theme}");
        // Rows with their words and controls.
        for said in ["Name the server on every tab", "Sidebar", "Show sidebar", "Server vitals", "Compact mode", "Where tabs go", "Under each server", "Search"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // The picture: the page's colour, a box of rows on it, the picked section's quiet fill, and
        // a switch that is on in the system's green.
        let px = run.pixels();
        assert_eq!(at(&px, 1250, 400), page, "{theme}: the page");
        assert_eq!(at(&px, 26, 500), frame, "{theme}: the sidebar");
        let [x, y, w, _] = run.rect("layout-tab-server");
        assert_eq!(at(&px, (x + w / 2.0) as u32, (y + 4.0) as u32), group, "{theme}: a row's box");
        assert_ne!(at(&px, 262, 106), "#0071e3", "{theme}: the picked section");
        assert_ne!(at(&px, 262, 106), frame, "{theme}: the picked section");
        let [sx, sy, sw, sh] = run.rect("layout-tab-server-control");
        let green = if theme == "dark" { [0x30, 0xd1, 0x58] } else { [0x34, 0xc7, 0x59] };
        assert!(near(&px, [sx as u32, sy as u32, (sx + sw) as u32, (sy + sh) as u32], green) > 100, "{theme}: the switch isn't green");
    }
}

#[test]
fn a_setting_changed_in_settings_takes_effect_at_once() {
    // The 0.10 shortcut opens it; Appearance; "Light" in a dark system.
    let first = json!([chord(0.3, Shortcut::Settings), tap(0.8, "nav-appearance"), end(1.4)]);
    let Some(look) = run("settings-look", &["--demo", "--theme", "dark"], Some(first)) else { return };
    assert_eq!(look.fill("window"), "#28282aff");
    assert!(look.tree_text.contains("Match my system"));
    let script = json!([
        chord(0.3, Shortcut::Settings),
        tap(0.8, "nav-appearance"),
        tap(1.4, "look-appearance-control/seg-1"),
        // Then the terminal section, and back to the tab with Esc.
        tap(2.0, "nav-terminal"),
        { "t": 2.6, "type": "key", "name": "Escape" },
        end(3.2)
    ]);
    let Some(run) = run("settings-light", &["--demo", "--theme", "dark"], Some(script)) else { return };
    // The whole window is the light one now, the terminal with it, on a dark system.
    assert_eq!(run.fill("window"), "#ececeeff");
    assert_eq!(run.fill("term"), "#ffffffff");
    assert!(run.element("settings").is_none(), "Esc didn't go back to the tab");
    let px = run.pixels();
    assert_eq!(at(&px, 1200, 22), "#ececee");
    assert_eq!(at(&px, 900, 600), "#ffffff");
}

#[test]
fn a_settings_file_that_does_not_read_starts_private_and_says_so() {
    // The real window (no --demo), in a home of its own with a damaged settings file.
    let damaged = r#"{"format":1,"settings":{"state":{"streamerMode":fal"#;
    let prepare = |home: &Path| {
        let dir = home.join(".sshee-preview");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), damaged).unwrap();
    };
    let Some(run) = run_in("settings-damaged", &["--theme", "dark"], None, prepare) else { return };
    // Across the top of the page: what was turned on and why.
    assert!(run.element("privacy-notice").is_some(), "nothing says the settings couldn't be read");
    assert!(run.tree_text.contains("Streamer mode is on, because your settings from before couldn't be read with certainty."));
    assert!(run.tree_text.contains("Review") && run.tree_text.contains("OK"));
    // What was there is kept aside; the new file reads, with streamer mode on in it.
    let dir = run.home.join(".sshee-preview");
    assert_eq!(std::fs::read_to_string(dir.join("settings.damaged.json")).unwrap(), damaged);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["settings"]["state"]["streamerMode"], json!(true));
    assert_eq!(saved["settings"]["state"]["hideSecrets"], json!(true));
    assert_eq!(saved["notice"]["turnedOn"], json!(["streamerMode"]));
}

#[test]
fn a_first_start_with_no_0_10_keeps_a_new_installs_settings() {
    // No settings file, and 0.10 never ran in this home: defaults, saved, nothing to say.
    let script = json!([{ "t": 1.5, "type": "mark", "name": "end" }]);
    let Some(run) = run("settings-fresh", &["--theme", "dark"], Some(script)) else { return };
    assert!(run.element("privacy-notice").is_none());
    let file = run.home.join(".sshee-preview").join("settings.json");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("the first start saved its settings")).unwrap();
    assert_eq!(saved["from"], json!("fresh"));
    assert_eq!(saved["settings"]["version"], json!(4));
    assert_eq!(saved["settings"]["state"]["streamerMode"], json!(false));
    assert_eq!(saved["settings"]["state"]["terminalFontSize"], json!(13));
    assert_eq!(saved["more"]["tabsNameServer"], json!(true));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
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
        // The shell said which folder it is in: the tab is named after it. (Its server follows
        // in the name unless the row is crowded, as it is on Windows with its window buttons,
        // where the server is named once before its tabs.)
        let name = run.label("tab-4");
        assert!(name == "~/shop" || name == "~/shop, web-staging", "{name}");
    });
}

#[test]
fn the_link_under_the_pointer_says_where_it_goes() {
    // The log's "payments runbook" is a link a program marked (OSC 8) over other words.
    let script = || json!([tap(0.3, STAGING), { "t": 0.9, "type": "move", "x": 260.0, "y": 507.0 }, end(1.9)]);
    in_both("link", script, |run, _| {
        // A plain click opens a link (Ctrl+click or ⌘-click at once).
        let hint = "Goes to https://docs.shop.example/runbooks/payments. Click to open";
        assert!(run.tree_text.contains(hint), "no hint says {hint:?}");
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
        for (key, label) in [("menu-copy", "Copy"), ("menu-paste", "Paste"), ("menu-select-all", "Select all"), ("menu-find", "Find"), ("menu-clear", "Clear"), ("menu-new", "New terminal here"), ("menu-browse", "Browse files in this folder")] {
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
        // The status bar says it's on.
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
        for label in ["Show beside this one", "Show below this one", "Duplicate tab", "Close tab", "Close other tabs", "Close tabs to the right"] {
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
        for key in ["tab-26", "tab-list", "new-tab"] {
            let r = run.rect(key);
            assert!(r[0] >= 0.0 && r[0] + r[2] <= window[2], "{key} is off the window: {r:?}");
        }
        assert!(run.rect("tab-26")[2] >= 60.0, "the tab on screen was squeezed");
    });
}

/// "+" and its menu: a terminal or Files here, then the other machines; one of them opened.
#[test]
fn plus_opens_its_menu_under_it_and_a_machines_page() {
    let first = || json!([tap(0.3, STAGING), tap(0.8, "new-tab"), end(1.4)]);
    in_both("plus", first, |run, _| {
        let (menu, plus, window) = (run.rect("new-tab-menu"), run.rect("new-tab"), run.rect("window"));
        assert!(menu[1] >= plus[1] + plus[3] && menu[0] + menu[2] <= window[2], "under +, inside the window: {menu:?} {plus:?}");
        for key in ["nt-terminal", "nt-files", "nt-server-0"] {
            assert!(run.element(key).is_some(), "{key}");
        }
        assert!(run.tree_text.contains("New terminal") && run.tree_text.contains("This computer"));
    });
    let inside = || json!([tap(0.3, STAGING), tap(0.8, "new-tab"), tap(1.3, "nt-server-0"), end(1.8)]);
    in_both("plus-machine", inside, |run, _| {
        assert!(run.element("nt-back").is_some());
        assert!(run.tree_text.contains("Files on This computer"));
    });
}

#[test]
fn two_tabs_side_by_side_share_the_page() {
    // web-staging has one tab: the shortcut opens a terminal beside it.
    let script = || json!([tap(0.3, STAGING), chord(0.9, Shortcut::SideBySide), end(1.6)]);
    in_both("side", script, |run, dark| {
        let (left, right, divider) = (run.rect("side-4"), run.rect("side-5"), run.rect("divider-0"));
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
fn split_right_and_down_with_the_pointer_and_the_keys_in_both_appearances() {
    // web-staging (one tab, 4): Split right (5), then Split down from it (6); the keyboard in
    // each in turn, by a click, and typing there. The demo's new terminals echo what's typed.
    // At 1280 x 800 the page is right of the 52-point sidebar and under the 44-point bar: the
    // left pane's middle is near (370, 420), the right column's lower pane near (990, 600).
    let script = || {
        json!([
            tap(0.3, STAGING),
            chord(0.8, Shortcut::SplitRight),
            chord(1.3, Shortcut::SplitDown),
            { "t": 1.8, "type": "type", "text": "lower" },
            { "t": 2.4, "type": "tap", "x": 990.0, "y": 300.0 },
            { "t": 2.7, "type": "type", "text": "upper" },
            end(3.4)
        ])
    };
    in_both("split", script, |run, dark| {
        let (a, b, c) = (run.rect("side-4"), run.rect("side-5"), run.rect("side-6"));
        // a at the left, half the page; b over c at the right.
        assert!(a[0] < b[0] && (b[0] - c[0]).abs() < 1.0 && b[1] < c[1], "{a:?} {b:?} {c:?}");
        assert!((a[2] - b[2]).abs() < 10.0 && (b[3] - c[3]).abs() < 10.0, "{a:?} {b:?} {c:?}");
        for key in ["pane-4", "pane-5", "pane-6", "divider-0", "divider-1-0", "side-head-5"] {
            assert!(run.element(key).is_some(), "{key}");
        }
        // The pane you're in (b, clicked last) is the tab lit in the row.
        assert_eq!(run.fill("tab-5"), if dark { "#3a3a3cff" } else { "#ffffffff" });
        assert!(run.tree_text.contains("Pane you're in"));
        // Its header's buttons are out; the others' wait for the pointer.
        for key in ["side-right-5", "side-down-5", "side-zoom-5", "side-close-5"] {
            assert!(run.element(key).is_some(), "{key}");
        }
        // Each pane drew its own typing, after its prompt ("deploy@web-staging:~$ ", 22 cells
        // of about 7.8 points from the pane's padding): something other than the background.
        let px = run.pixels();
        let typed = |r: [f64; 4]| {
            let bg = at(&px, (r[0] + r[2]) as u32 - 20, (r[1] + r[3]) as u32 - 20);
            let bg = [1, 3, 5].map(|i| u8::from_str_radix(&bg[i..i + 2], 16).unwrap());
            let (x, y) = (r[0] as u32 + 12 + 172, r[1] as u32 + 28 + 12);
            let area = [x, y, x + 40, y + 18];
            40 * 18 - near(&px, area, bg)
        };
        assert!(typed(b) > 40, "nothing typed in the upper pane");
        assert!(typed(c) > 40, "nothing typed in the lower pane");
    });
}

#[test]
fn a_pane_fills_the_window_and_shows_all_again() {
    // Rune's scripts name the key "Enter"; the chord is ⇧⌘↩ on a Mac, Ctrl+Shift+Enter elsewhere.
    let script = || json!([tap(0.3, STAGING), chord(0.8, Shortcut::SplitRight), press(1.3, "Enter", !MAC, MAC, true), end(2.0)]);
    in_both("zoom", script, |run, _| {
        let alone = run.rect("side-5");
        assert!(alone[2] > 1150.0, "{alone:?}");
        assert!(run.element("side-4").is_none() && run.element("divider-0").is_none());
        assert!(run.tree_text.contains("Show all 2 panes"));
    });
}

#[test]
fn the_sidebar_drags_wide_with_names_in_both_appearances() {
    // The edge of the sidebar, dragged 200 points toward the page.
    let script = || {
        json!([
            tap(0.3, STAGING),
            { "t": 0.8, "type": "drag", "key": "rail-edge", "dx": 200.0, "dy": 0.0, "dur": 0.4 },
            end(1.8)
        ])
    };
    in_both("rail-wide", script, |run, _| {
        let rail = run.rect("rail");
        assert!((rail[2] - 252.0).abs() < 6.0, "{rail:?}");
        assert!(run.tree_text.contains("Servers") && run.tree_text.contains("web-staging"));
        assert!(run.rect("page")[0] >= rail[0] + rail[2] - 1.0);
    });
}

#[test]
fn tabs_go_under_each_server_in_both_appearances() {
    // Settings → Layout → Where tabs go: "Under each server"; then the sidebar dragged wide.
    let script = || {
        json!([
            chord(0.3, Shortcut::Settings),
            tap(0.8, "nav-layout"),
            tap(1.3, "layout-tabs-control/seg-0"),
            { "t": 1.8, "type": "key", "name": "Escape" },
            { "t": 2.3, "type": "drag", "key": "rail-edge", "dx": 200.0, "dy": 0.0, "dur": 0.4 },
            end(3.4)
        ])
    };
    in_both("tabs-under-servers", script, |run, dark| {
        let rail = run.rect("rail");
        let lists: Vec<&Value> = run.tree["elements"].as_array().unwrap().iter().filter(|e| e["key"].as_str().is_some_and(|k| k.starts_with("tabs-"))).collect();
        // The demo's tabs are on two servers: a list under each.
        assert_eq!(lists.len(), 2, "{}", run.tree_text);
        // Every tab is in the sidebar, none along the top; "+" stays there.
        for id in 1..=3 {
            let tab = run.rect(&format!("tab-{id}"));
            assert!(tab[0] >= rail[0] && tab[0] + tab[2] <= rail[0] + rail[2] + 1.0 && tab[1] > 44.0, "tab-{id}: {tab:?} {rail:?}");
        }
        assert!(run.element("new-tab").is_some());
        // The tab on screen (~/shop) is lit quietly; the one whose agent waits says so.
        assert_eq!(run.fill("tab-1"), if dark { "#ffffff24" } else { "#0000001f" });
        assert!(run.tree_text.contains("Needs you"), "{}", run.tree_text);
        // Its line in the accent, beside it, in the picture.
        let [x, y, _, h] = run.rect("tab-1");
        let px = run.pixels();
        let blue = [0x00, 0x71, 0xe3];
        assert!(near(&px, [(x - 9.0) as u32, y as u32, x as u32, (y + h) as u32], blue) > 6, "no accent line beside the tab on screen");
    });
}

#[test]
fn compact_mode_hides_the_bars_until_the_pointer_comes() {
    let script = || {
        json!([
            tap(0.3, STAGING),
            chord(0.8, Shortcut::Compact),
            { "t": 1.3, "type": "move", "x": 2.0, "y": 400.0 },
            end(2.0)
        ])
    };
    in_both("compact", script, |run, _| {
        assert!(run.element("bar").is_none() && run.element("status").is_none());
        assert!(run.element("out-side").is_some(), "the sidebar didn't come at the edge");
        assert!(run.rect("page")[2] > 1250.0);
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
            // The key file's field shows once "Another key file" is chosen (after the typing:
            // choosing it moves the keyboard to that field).
            { "t": 2.2, "type": "tap", "key": "key-other" },
            { "t": 2.8, "type": "mark", "name": "end" }
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
        for said in ["New terminal", "Files", "Rename…", "Icon and color…", "Production", "Staging", "Development", "Remove server…"] {
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
        // "Name" stands over the names: a row's padding, its icon (18 pt) and the gap after it.
        assert_eq!(run.rect("sort-name")[0], run.rect("row-0")[0] + 10.0 + 18.0 + 8.0, "{theme}");
        for said in ["Folder path", "backups", "logs", "shop", "deploy.sh", "nginx.conf", "README.md", "8 items, 4 hidden", "1 selected, 256 B", "Today 11:02", "Oct 10", "Bookmarks: go to a place", "Upload files"] {
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
        // Markdown opens formatted: Source, to edit it.
        tap(1.9, "seg-1"),
        // On the text (the preview's header is a list header's height: its lines start higher).
        { "t": 2.2, "type": "tap", "x": 1000.0, "y": 250.0 },
        { "t": 2.5, "type": "type", "text": "x" },
        press(2.8, "s", !MAC, MAC, false),
        { "t": 3.4, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-conflict", &["--demo"], Some(script)) else { return };
    assert_eq!(run.rect("files-preview")[2], f64::from(sshee_app::files::layout::PREVIEW_DEFAULT));
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
fn return_on_the_delete_question_deletes_nothing() {
    // Delete asks; Return then means Cancel: the question goes, and the file stays.
    let script = json!([
        { "t": 0.3, "type": "tap", "key": "tab-2" },
        { "t": 0.8, "type": "tap", "x": 300.0, "y": 231.0 },
        press(1.0, "Delete", false, false, false),
        press(1.5, "Enter", false, false, false),
        { "t": 2.2, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-delete-return", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-question").is_none(), "the question is still open");
    assert!(run.tree_text.contains("deploy.sh"), "deploy.sh was deleted");
    assert!(!run.tree_text.contains("Deleting"), "a delete started");
}

#[test]
fn a_download_shows_its_card_until_it_is_stopped() {
    // Into backups, the menu on its one file, Download: the demo's endless download.
    let events = |stop: bool| {
        let mut e = vec![
            tap(0.3, "tab-2"),
            json!({ "t": 0.8, "type": "double-click", "x": 300.0, "y": 153.0 }),
            json!({ "t": 1.4, "type": "secondary-click", "x": 300.0, "y": 153.0 }),
            tap(1.8, "menu-4"),
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
    assert!(run.tree_text.contains("Move to logs"), "the folder under the pointer doesn't take the file");
    assert!(run.element("files-drag-card").is_some(), "no card flies with the pointer");
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
        tap(0.8, "files-upload"),
        { "t": 1.2, "type": "file-dialog", "path": "/home/dev/notes.txt" },
        { "t": 1.8, "type": "mark", "name": "end" }
    ]);
    let Some(run) = self::run("files-upload", &["--demo"], Some(script)) else { return };
    for said in ["Uploaded notes.txt", "notes.txt", "9 items, 4 hidden"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
}

#[test]
fn the_disk_in_the_path_goes_to_the_top_and_the_path_stays() {
    // The disk at the start of the path, twice (the second time it's where you are), then
    // into "home": the path still shows as folders, never a field holding "/".
    let script = json!([
        tap(0.3, "tab-2"),
        tap(0.8, "crumb-0"),
        tap(1.3, "crumb-0"),
        // At the top there's no "..": "home" is the first row.
        { "t": 1.8, "type": "double-click", "x": 300.0, "y": 127.0 },
        end(2.4)
    ]);
    let Some(run) = run("files-disk", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-path-field").is_none(), "the disk turned the path into a field");
    assert!(run.element("crumb-1").is_some(), "the path isn't drawn as folders");
    assert_eq!(run.label("crumb-0"), "Top of the disk");
    assert!(run.tree_text.contains("deploy"), "not in /home");
}

#[test]
fn the_path_field_gives_way_to_a_click_elsewhere() {
    // A click into the path types there; a click on a row ends that and selects the row.
    let script = json!([tap(0.3, "tab-2"), tap(0.8, "files-path-space"), { "t": 1.3, "type": "tap", "x": 300.0, "y": 231.0 }, end(1.9)]);
    let Some(run) = run("files-path-away", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-path-field").is_none(), "still typing in the path");
    assert!(run.element("crumb-1").is_some(), "the path isn't drawn as folders again");
    assert!(run.tree_text.contains("1 selected, 256 B"), "the click didn't select the row");
    // While typing, a double click on a folder still opens it, and the path follows.
    let script = json!([tap(0.3, "tab-2"), tap(0.8, "files-path-space"), { "t": 1.3, "type": "double-click", "x": 300.0, "y": 153.0 }, end(2.0)]);
    let Some(run) = self::run("files-path-open", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-path-field").is_none(), "still typing in the path");
    assert!(run.tree_text.contains("shop-db.sql.gz"), "the folder didn't open");
    assert!(run.tree_text.contains("backups"), "the path doesn't show the folder");
}

#[test]
fn rows_dragged_onto_the_path_out_of_the_window_or_dropped_from_outside() {
    let files = tap(0.3, "tab-2");
    // deploy.sh dragged onto "home" in the path: a card flies with it saying where it goes,
    // and letting go moves it there.
    let onto_path = |up: bool| {
        let mut e = vec![
            files.clone(),
            json!({ "t": 0.8, "type": "down", "x": 300.0, "y": 231.0 }),
            json!({ "t": 0.9, "type": "move", "x": 310.0, "y": 200.0 }),
            json!({ "t": 1.0, "type": "move", "key": "crumb-1" }),
        ];
        if up {
            e.push(json!({ "t": 1.2, "type": "up", "key": "crumb-1" }));
        }
        e.push(end(1.8));
        Value::Array(e)
    };
    let Some(run) = run("files-drag-path", &["--demo"], Some(onto_path(false))) else { return };
    assert!(run.element("files-drag-card").is_some(), "no card flies with the pointer");
    assert!(run.tree_text.contains("Move to home"), "the card doesn't say where it goes");
    let Some(run) = self::run("files-drag-path-dropped", &["--demo"], Some(onto_path(true))) else { return };
    for said in ["Moved deploy.sh to home", "7 items, 4 hidden"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    // Out past the window's edge: the window says letting go downloads, and letting go asks
    // first, naming where it lands.
    let out = |up: bool| {
        let mut e = vec![
            files.clone(),
            json!({ "t": 0.8, "type": "down", "x": 300.0, "y": 231.0 }),
            json!({ "t": 0.9, "type": "move", "x": 700.0, "y": 300.0 }),
            json!({ "t": 1.0, "type": "move", "x": 1400.0, "y": 300.0 }),
        ];
        if up {
            e.push(json!({ "t": 1.2, "type": "up", "x": 1400.0, "y": 300.0 }));
        }
        e.push(end(1.8));
        Value::Array(e)
    };
    let Some(run) = self::run("files-drag-out", &["--demo"], Some(out(false))) else { return };
    assert!(run.element("files-drag-out").is_some(), "nothing says letting go downloads");
    assert!(run.tree_text.contains("Let go to download deploy.sh"), "nothing says letting go downloads");
    let Some(run) = self::run("files-drag-out-asks", &["--demo"], Some(out(true))) else { return };
    for said in ["Download “deploy.sh” to Downloads?", "From web-7731, into your Downloads folder."] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    assert!(run.element("transfer-1").is_none(), "it downloaded before asking");
    // A file from another app dropped on the logs folder goes into logs, not the folder on
    // screen.
    let script = json!([files, { "t": 0.8, "drop_files": { "paths": ["/home/dev/notes.txt"], "at": [300.0, 179.0], "hover": 0.3 } }, end(1.8)]);
    let Some(run) = self::run("files-drop-on-folder", &["--demo"], Some(script)) else { return };
    assert!(run.tree_text.contains("Uploaded notes.txt"), "nothing was uploaded");
    assert!(run.tree_text.contains("8 items, 4 hidden"), "it landed in the folder on screen");
}

#[test]
fn files_peek_a_file_places_ticks_picture_sizes_and_the_copy_dialog_as_in_0_10() {
    let files = tap(0.3, "tab-2");
    // → on shop looks inside, beside its »; ↑ goes to its last row, a file, and Return opens it
    // beside the list while the tab stays in deploy.
    let script = json!([
        files.clone(),
        { "t": 0.8, "type": "tap", "x": 300.0, "y": 205.0 },
        press(1.0, "Right", false, false, false),
        press(1.3, "Up", false, false, false),
        press(1.6, "Enter", false, false, false),
        end(2.4)
    ]);
    let Some(run) = run("files-peek-file", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-preview").is_some(), "the file didn't open");
    assert!(run.tree_text.contains("package.json"), "not the file picked");
    assert!(run.element("crumb-3").is_none(), "the tab went into shop");
    // The list of places, right-clicked: a tick for each place and for recent folders.
    let script = json!([files.clone(), tap(0.8, "files-places"), { "t": 1.2, "type": "tap", "key": "place-1", "button": "secondary" }, end(1.8)]);
    let Some(run) = self::run("files-places-menu", &["--demo"], Some(script)) else { return };
    assert!(run.element("files-place-menu").is_some(), "no menu");
    for said in ["Hide Disk", "Recent folders", "Logs", "Config"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    // A picture: its size in pixels under it, its bytes once (in the header), and drawn no
    // bigger than it is. The header is as tall as the list's beside it: one hairline.
    let script = json!([files.clone(), { "t": 0.8, "type": "double-click", "x": 300.0, "y": 257.0 }, end(1.8)]);
    let Some(run) = self::run("files-picture", &["--demo"], Some(script)) else { return };
    assert!(run.tree_text.contains("48 × 48"), "no size under the picture");
    assert!(run.element("preview-size").is_some() && !run.tree_text.contains("48 × 48 ·"), "the bytes again under the picture");
    let (head, list) = (run.rect("preview-head"), run.rect("files-header"));
    assert_eq!(head[1] + head[3], list[1] + list[3], "the header's hairline and the list's don't line up");
    let [_, _, w, h] = run.rect("preview-picture-box");
    assert!(w <= 48.0 && h <= 48.0, "drawn bigger than it is: {w}×{h}");
    // Copy to another folder or server: from where, which server is this one, and never into
    // the folder they're in.
    let script = json!([
        files,
        { "t": 0.8, "type": "secondary-click", "x": 300.0, "y": 231.0 },
        tap(1.2, "menu-5"),
        tap(1.7, "copy-to-host-demo-web"),
        end(2.3)
    ]);
    let Some(run) = self::run("files-copy-here", &["--demo"], Some(script)) else { return };
    for said in ["From web-7731", "this one", "They're already in this folder. Pick another one.", "Folder on the server"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
}

// ---- Open in your own editor (the demo starts no editor: it plays a session's news)

/// The Files tab, then "Open in VS Code" from the menu of the row at `y`, and Return on the
/// warning the demo's Tor brings up before the first one.
fn open_in_editor(t: f64, y: f64) -> Vec<Value> {
    vec![
        json!({ "t": t, "type": "secondary-click", "x": 300.0, "y": y }),
        tap(t + 0.4, "menu-1"),
        press(t + 0.7, "Enter", false, false, false),
    ]
}

#[test]
fn a_file_opens_in_your_own_editor_and_says_how_its_saves_go() {
    for theme in ["dark", "light"] {
        // nginx.conf: the menu names the editor; the row and the line under the list say saved.
        let mut events = vec![tap(0.3, "tab-2")];
        events.extend(open_in_editor(0.8, 283.0));
        events.push(json!({ "t": 1.9, "type": "mark", "name": "end" }));
        let Some(run) = run(&format!("edit-{theme}"), &["--demo", "--theme", theme], Some(Value::Array(events))) else { return };
        for said in ["Uploaded 14:02", "Saved to the server at 14:02", "Stop editing", "nginx.conf"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        assert!(run.element("files-editing").is_some(), "{theme}: no line under the list");
        assert!(run.element("edit-warn").is_none(), "{theme}: the warning stayed");
    }
    // The menu on a file: "Open in VS Code", and "Open in…" with every editor found.
    let script = json!([tap(0.3, "tab-2"), { "t": 0.8, "type": "secondary-click", "x": 300.0, "y": 309.0 }, tap(1.2, "menu-2"), end(1.8)]);
    let Some(run) = self::run("edit-others", &["--demo"], Some(script)) else { return };
    for said in ["VS Code", "Zed", "Text Editor", "The default text editor", "Choose an app…"] {
        assert!(run.tree_text.contains(said), "{said} isn't in Open in…");
    }
    // Over 50 MB it asks first.
    let mut events = vec![tap(0.3, "tab-2"), json!({ "t": 0.8, "type": "double-click", "x": 300.0, "y": 179.0 })];
    events.extend(open_in_editor(1.4, 153.0));
    events.push(end(2.4));
    let Some(run) = self::run("edit-big", &["--demo"], Some(Value::Array(events))) else { return };
    assert!(run.tree_text.contains("This file is 230 MB. Download a copy to edit?"), "no question for a big file");
}

#[test]
fn a_conflict_offers_every_way_on_and_compares() {
    let mut events = vec![tap(0.3, "tab-2"), json!({ "t": 0.8, "type": "double-click", "x": 300.0, "y": 205.0 })];
    events.extend(open_in_editor(1.4, 179.0));
    events.push(tap(2.6, "conflict-compare"));
    events.push(end(3.2));
    let Some(run) = run("edit-conflict", &["--demo"], Some(Value::Array(events))) else { return };
    for said in ["This file changed on the server since you opened it", "Keep mine", "Take theirs", "Save mine as a copy", "Hide changes", "yours", "server", "Conflict"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    assert!(run.element("compare-lines").is_some());
    // The dialog sits over the window, the copy that keeps both is focused.
    assert_eq!(run.fill("edit-conflict"), "#2c2c2eff");
}

#[test]
fn copies_an_earlier_run_left_unsent_are_offered_once_at_start() {
    let Some(run) = run("edit-leftovers", &["--demo", "--demo-leftovers"], None) else { return };
    for said in ["2 files have changes that were never uploaded", "/home/deploy/nginx.conf on web-7731 (178 B)", "Upload", "Discard", "Show in folder"] {
        assert!(run.tree_text.contains(said), "{said} isn't in the tree");
    }
    // Without the flag the demo has none, and nothing is over the window.
    let Some(run) = self::run("edit-no-leftovers", &["--demo"], None) else { return };
    assert!(run.element("edit-leftovers").is_none());
}

// ---- Toasts and confirmations (`shell::notes`, `kit::toast`, `kit::dialog`)

#[test]
fn toasts_stack_in_the_corner_in_both_appearances() {
    // deploy.sh dragged onto logs ("Moved … Undo"), then an upload ("Uploaded notes.txt").
    let script = || {
        json!([
            tap(0.3, "tab-2"),
            { "t": 0.8, "type": "down", "x": 300.0, "y": 231.0 },
            { "t": 0.9, "type": "move", "x": 310.0, "y": 210.0 },
            { "t": 1.0, "type": "move", "x": 320.0, "y": 180.0 },
            { "t": 1.2, "type": "up", "x": 320.0, "y": 180.0 },
            tap(1.6, "files-upload"),
            { "t": 1.9, "type": "file-dialog", "path": "/home/dev/notes.txt" },
            { "t": 2.3, "type": "move", "x": 640.0, "y": 400.0 },
            end(2.6)
        ])
    };
    in_both("toasts", script, |run, dark| {
        for said in ["Moved deploy.sh to logs", "Undo", "Uploaded notes.txt", "Dismiss"] {
            assert!(run.tree_text.contains(said), "{said} isn't in the tree");
        }
        // Newest at the bottom, over the status bar, at the window's right edge.
        let (first, second) = (run.rect("toast-1"), run.rect("toast-2"));
        assert_eq!((first[2], second[2]), (300.0, 300.0));
        assert_eq!(second[0] + second[2], 1280.0 - 14.0);
        assert!(first[1] < second[1] && second[1] + second[3] <= 800.0 - 24.0 - 12.0 + 0.5, "{first:?} {second:?}");
        // The popover's surface.
        assert_eq!(run.fill("toast-2"), if dark { "#2c2c2eff" } else { "#f6f6f8ff" });
    });
}

#[test]
fn a_confirmation_dims_the_window_in_both_appearances() {
    let script = || json!([tap(0.3, "tab-2"), { "t": 0.8, "type": "tap", "x": 300.0, "y": 231.0 }, press(1.0, "Delete", false, false, false), end(1.6)]);
    in_both("dialog", script, |run, dark| {
        assert_eq!(run.fill("files-question"), if dark { "#2c2c2eff" } else { "#f6f6f8ff" });
        assert_eq!(run.fill("files-question-layer"), "#00000040");
        // The card in the middle of the window.
        let [x, y, w, h] = run.rect("files-question");
        assert_eq!(w, 400.0);
        assert!(((x + w / 2.0) - 640.0).abs() < 1.0 && ((y + h / 2.0) - 400.0).abs() < 1.0, "{x} {y} {w} {h}");
        // Delete takes something away: red words on a quiet button, never blue.
        let px = run.pixels();
        let [bx, by, bw, bh] = run.rect("question-yes");
        assert_eq!(near(&px, [bx as u32, by as u32, (bx + bw) as u32, (by + bh) as u32], [0x00, 0x71, 0xe3]), 0, "Delete is blue");
    });
}

// ---- a newer test build (`screens::canary`): one line across the top of the page.

#[test]
fn a_new_test_build_is_offered_across_the_top_in_both_appearances() {
    for (theme, group) in [("dark", "#242426"), ("light", "#f5f5f7")] {
        let Some(run) = run(&format!("canary-{theme}"), &["--demo", "--demo-update", "--theme", theme], None) else { return };
        assert!(run.tree_text.contains("A new test build is ready (1.0.0-canary.125)."), "{theme}");
        assert_eq!(run.label("canary-restart"), "Restart to update", "{theme}");
        assert_eq!(run.label("canary-later"), "Later", "{theme}");
        // The top of the page, its whole width, over the tab's terminal.
        let [page_x, page_y, page_w, _] = run.rect("page");
        let [x, y, w, h] = run.rect("canary-notice");
        assert_eq!((x, y, w), (page_x, page_y, page_w), "{theme}");
        assert!((36.0..=48.0).contains(&h), "{theme}: {h}");
        assert!(run.rect("term")[1] >= y + h, "{theme}: the terminal is under the line");
        assert_eq!(run.fill("canary-notice"), format!("{group}ff"), "{theme}");
        // The picture: the line's colour at its start, and the blue of the button that does it.
        let px = run.pixels();
        assert_eq!(at(&px, (x + 40.0) as u32, (y + 3.0) as u32), group, "{theme}");
        let [bx, by, bw, bh] = run.rect("canary-restart");
        let button = [bx as u32, by as u32, (bx + bw) as u32, (by + bh) as u32];
        assert!(near(&px, button, [0x00, 0x71, 0xe3]) + near(&px, button, [0x0a, 0x84, 0xff]) > 100, "{theme}: Restart isn't blue");
    }
    // Without the flag, nothing is said.
    let Some(run) = self::run("canary-none", &["--demo"], None) else { return };
    assert!(run.element("canary-notice").is_none());
}

#[test]
fn later_puts_the_line_away_and_a_failed_restart_says_why() {
    let later = json!([tap(0.5, "canary-later"), end(1.2)]);
    let Some(run) = run("canary-later", &["--demo", "--demo-update", "--theme", "dark"], Some(later)) else { return };
    assert!(run.element("canary-notice").is_none(), "Later didn't put it away");
    assert_eq!(run.rect("term")[1], run.rect("page")[1], "the terminal has the page again");
    // The demo installs nothing: the line says why and offers the download page instead.
    let restart = json!([tap(0.5, "canary-restart"), end(1.2)]);
    let Some(run) = self::run("canary-restart", &["--demo", "--demo-update", "--theme", "light"], Some(restart)) else { return };
    assert!(run.tree_text.contains("It didn't install: the demo doesn't install anything"));
    assert_eq!(run.label("canary-download"), "Download");
    assert!(run.element("canary-restart").is_none());
}

// ---- the security check, recipes and snippets; the welcome, the tour and "What's new"

#[test]
fn the_security_check_recipes_and_snippets_in_both_appearances() {
    for theme in ["dark", "light"] {
        // The shield on web-7731 (Production): its findings, worst first, and a fix's two questions.
        let script = json!([tap(0.3, "check"), tap(1.0, "fix-run-0"), tap(1.4, "fix-yes-0"), end(2.0)]);
        let Some(run) = run(&format!("check-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        for said in ["Security check", "2 to fix", "To fix: Password login", "web-7731 is a Production server", "Run on Production", "Open the recipe"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        assert!(run.rect("find-0")[1] < run.rect("find-2")[1]);
        // The recipe the first finding points to, its key read from this (made-up) computer.
        let script = json!([tap(0.3, "check"), tap(1.0, "recipe-0"), end(1.8)]);
        let Some(run) = self::run(&format!("recipe-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        for said in ["Log in with a key instead of a password", "Put your public key on the server", "id_ed25519", "SSHeesh found a key on this computer"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // Snippets, from the palette.
        let script = json!([chord(0.3, Shortcut::Palette), { "t": 0.6, "type": "type", "text": "snippets" }, press(0.9, "Enter", false, false, false), end(1.6)]);
        let Some(run) = self::run(&format!("snippets-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        for said in ["Snippets", "Tail the shop's log", "This server", "New snippet"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
    }
}

#[test]
fn the_welcome_the_tour_and_whats_new_in_both_appearances() {
    for theme in ["dark", "light"] {
        let Some(run) = run(&format!("welcome-{theme}"), &["--demo", "--demo-welcome", "--theme", theme], None) else { return };
        for said in ["Welcome to SSHeesh", "Your servers", "Bring over your server from SSHeesh 0.10", "Pick from 2 in ~/.ssh/config", "Skip", "Continue"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // What the demo's XPipe and PuTTY hold, offered by name.
        for said in ["Bring over 2 servers from XPipe", "Bring over 2 servers from PuTTY"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // The tour, from the welcome's last page.
        let mut events: Vec<Value> = (0..5).map(|i| tap(0.3 + 0.3 * i as f64, "welcome-next")).collect();
        events.extend([tap(1.8, "welcome-tour"), tap(2.2, "tour-next"), tap(2.5, "tour-next"), tap(2.8, "tour-next"), end(3.4)]);
        let script = Value::Array(events);
        let Some(run) = self::run(&format!("tour-{theme}"), &["--demo", "--demo-welcome", "--theme", theme], Some(script)) else { return };
        assert!(run.tree_text.contains("Check a server's safety"), "{theme}: the tour's fourth card");
        let Some(run) = self::run(&format!("whats-new-{theme}"), &["--demo", "--demo-whats-new", "--theme", theme], None) else { return };
        assert!(run.tree_text.contains("What's new in SSHeesh 1.0.0"), "{theme}");
        // Without the flags the demo is the design's scene, with nothing over it.
        let Some(run) = self::run(&format!("no-welcome-{theme}"), &["--demo", "--theme", theme], None) else { return };
        assert!(run.element("welcome").is_none() && run.element("news").is_none());
    }
}

/// Keeps a run's picture in the folder `SSHEESH_WELCOME_SHOTS` names, when it names one: to
/// look at, not to check.
fn keep_shot(run: &Run, name: &str) {
    if let Some(dir) = std::env::var_os("SSHEESH_WELCOME_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(&run.png, dir.join(format!("{name}.png"))).unwrap();
    }
}

/// `n` presses of the welcome's Continue, then `more`, then the end, late enough for the
/// pictures to be decoded and drawn.
fn welcome_script(n: usize, more: Vec<Value>) -> Value {
    let mut events: Vec<Value> = (0..n).map(|i| tap(0.3 + 0.3 * i as f64, "welcome-next")).collect();
    let last = more.last().and_then(|e| e["t"].as_f64()).unwrap_or(0.3 * n as f64);
    events.extend(more);
    events.push(end(last + 1.4));
    Value::Array(events)
}

#[test]
fn the_welcome_walks_its_pages_once_in_both_appearances() {
    // (theme, the window's frame)
    for (theme, frame) in [("dark", "#28282aff"), ("light", "#ececeeff")] {
        let pages = [
            ("servers", &["Your servers", "Bring over your server from SSHeesh 0.10", "Add another by hand", "1 of 6"][..]),
            ("layout", &["Layout", "Where tabs go", "Along the top", "Under each server", "Tabs on screen", "Side by side", "Compact mode", "Server vitals", "2 of 6"]),
            ("sidebar", &["Sidebar", "Side", "Server list", "Names", "Icons", "Server icons", "Pixels", "Server load", "Logo", "3 of 6"]),
            ("look", &["How it looks", "Match my system", "Light", "Dark", "Dark theme", "Midnight", "Accent color", "Interface size", "Animations", "4 of 6"]),
            ("terminal", &["In the terminal", "Copy from the terminal", "When I select text", "When the connection drops", "Reconnect", "Ask me", "5 of 6"]),
            ("tips", &["Three things people miss", "Find anything", "Files on the server", "Every agent, in one list", "Take the one-minute tour", "Done"]),
        ];
        // Every page as tall as the others: the buttons under it stay where the pointer is.
        let mut next_at: Option<[f64; 2]> = None;
        for (n, (page, said)) in pages.iter().enumerate() {
            let name = format!("welcome-{}-{page}-{theme}", n + 1);
            let Some(run) = run(&name, &["--demo", "--demo-welcome", "--theme", theme], Some(welcome_script(n, vec![]))) else { return };
            keep_shot(&run, &name);
            assert!(run.element(&format!("welcome-page-{page}")).is_some(), "{theme}: page {page} isn't on screen");
            for said in *said {
                assert!(run.tree_text.contains(said), "{theme}: {said} isn't on page {page}");
            }
            assert!(!run.tree_text.contains("Streamer"), "{theme}: streamer mode is no headline");
            // Its top and its right edge: Done, on the last page, is narrower.
            let [x, y, w, _] = run.rect("welcome-next");
            let next = [y, x + w];
            assert!(next_at.is_none_or(|at| (at[0] - next[0]).abs() < 0.5 && (at[1] - next[1]).abs() < 0.5), "{theme}: Continue moved on page {page}: {next:?} from {next_at:?}");
            next_at = Some(next);
            // The pictures are drawn: many colours where one is, not the page's plain one.
            let pictures: &[&str] = match *page {
                "layout" => &["welcome-layout-tabs-picture", "welcome-layout-view-picture", "welcome-layout-compact-picture"],
                "sidebar" => &["welcome-layout-sidebar-side-picture", "welcome-layout-server-list-picture", "welcome-layout-server-icons-picture"],
                "look" => &["welcome-look-dark"],
                "terminal" => &["welcome-term-reconnect-picture"],
                _ => &[],
            };
            for key in pictures {
                let px = run.pixels();
                let [x, y, w, h] = run.rect(key);
                let mut colours = std::collections::HashSet::new();
                for i in 0..20 {
                    for j in 0..10 {
                        colours.insert(at(&px, (x + w * (i as f64 + 0.5) / 20.0) as u32, (y + h * (j as f64 + 0.5) / 10.0) as u32));
                    }
                }
                assert!(colours.len() > 6, "{theme}: {key} is blank: {colours:?}");
            }
        }
        // Done on the last page: it's gone, and nothing changed.
        let Some(run) = run(&format!("welcome-done-{theme}"), &["--demo", "--demo-welcome", "--theme", theme], Some(welcome_script(6, vec![]))) else { return };
        assert!(run.element("welcome").is_none(), "{theme}: Done didn't close it");
        assert_eq!(run.fill("window"), frame, "{theme}: the appearance changed");
        // A choice applies at once, as in Settings: the other appearance, and the sidebar at the right.
        let other = if theme == "dark" { "welcome-look-light" } else { "welcome-look-dark" };
        let script = welcome_script(3, vec![tap(1.2, other), tap(1.5, "welcome-back"), tap(1.8, "welcome-layout-sidebar-side-right")]);
        let Some(run) = self::run(&format!("welcome-picked-{theme}"), &["--demo", "--demo-welcome", "--theme", theme], Some(script)) else { return };
        keep_shot(&run, &format!("welcome-3-sidebar-picked-{theme}"));
        assert_ne!(run.fill("window"), frame, "{theme}: the appearance didn't change");
        assert!(run.rect("rail")[0] > 600.0, "{theme}: the sidebar isn't at the right");
        // Esc on any page skips, and nothing changed.
        let script = welcome_script(2, vec![press(0.9, "Escape", false, false, false)]);
        let Some(run) = self::run(&format!("welcome-esc-{theme}"), &["--demo", "--demo-welcome", "--theme", theme], Some(script)) else { return };
        assert!(run.element("welcome").is_none(), "{theme}: Esc didn't skip");
        assert_eq!(run.fill("window"), frame, "{theme}");
    }
}

#[test]
fn a_new_server_asks_about_its_key_and_the_session_cards_show_in_both_appearances() {
    // backups is seen for the first time: its terminal asks about its key, and nothing else.
    for theme in ["dark", "light"] {
        let script = json!([tap(0.3, "server-3"), { "t": 1.2, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("hostkey-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        for said in ["New server. Is this really backups?", "Server fingerprint", "Trust and connect", "Cancel"] {
            assert!(run.tree_text.contains(said), "{theme}: {said}");
        }
        let card = run.rect("hostkey-card");
        let term = run.rect("term");
        // In the middle of the terminal.
        assert!((card[0] + card[2] / 2.0 - (term[0] + term[2] / 2.0)).abs() < 2.0, "{theme}: {card:?} in {term:?}");
    }
    // Trusting it signs in: the card goes.
    let script = json!([tap(0.3, "server-3"), tap(1.0, "hostkey-yes"), { "t": 1.6, "type": "mark", "name": "end" }]);
    let Some(run) = run("hostkey-trusted", &["--demo"], Some(script)) else { return };
    assert!(run.element("hostkey-card").is_none());
    // Every other card, as `--demo-card` plays it.
    for (state, said) in [
        ("hostkey-changed", "backups's key changed"),
        ("disconnected", "Always reconnect automatically"),
        ("retrying", "The server stopped responding"),
        ("asleep", "Disconnected after 20 min unused"),
        ("failed", "Couldn't connect"),
        ("ended", "Session ended"),
    ] {
        for theme in ["dark", "light"] {
            let script = json!([tap(0.3, "server-3"), { "t": 1.4, "type": "mark", "name": "end" }]);
            let Some(run) = self::run(&format!("card-{state}-{theme}"), &["--demo", "--demo-card", state, "--theme", theme], Some(script)) else { return };
            assert!(run.tree_text.contains(said), "{state} {theme}: {said}");
        }
    }
}

/// A slow sign-in (`--demo-card connecting`): the empty terminal says where it connects and
/// what it's doing, in both appearances, and goes once the shell draws.
#[test]
fn a_slow_sign_in_says_where_it_connects_until_the_shell_draws() {
    for theme in ["dark", "light"] {
        // Its key looked up (the first second), then tmux starting.
        for (at, said) in [(0.9, "Checking the server's key"), (2.3, "Starting tmux")] {
            let script = json!([tap(0.3, "server-3"), { "t": at, "type": "mark", "name": "end" }]);
            let Some(run) = run(&format!("connecting-{theme}-{at}"), &["--demo", "--demo-card", "connecting", "--theme", theme], Some(script)) else { return };
            assert!(run.tree_text.contains("Connecting to backups…") && run.tree_text.contains(said), "{theme} {at}: {said}");
            // In the middle of the terminal, under the top row.
            let [x, y, w, h] = run.rect("connect-screen-title");
            assert!(y > 300.0 && y + h < 500.0 && (x + w / 2.0 - 666.0).abs() < 40.0, "{theme}: {:?}", [x, y, w, h]);
        }
        let script = json!([tap(0.3, "server-3"), { "t": 3.4, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("connecting-{theme}-done"), &["--demo", "--demo-card", "connecting", "--theme", theme], Some(script)) else { return };
        assert!(run.element("connect-screen").is_none() && !run.tree_text.contains("Connecting to"), "{theme}: gone once the shell drew");
    }
}

// ---- Containers and Stats

/// The palette, `words` typed into it, and Return: the Containers or Stats tab of the machine
/// on screen (web-7731 in the demo).
fn from_palette(words: &str) -> Vec<Value> {
    vec![chord(0.3, Shortcut::Palette), json!({ "t": 0.6, "type": "type", "text": words }), press(1.0, "Enter", false, false, false)]
}

#[test]
fn containers_are_a_tab_grouped_by_project_in_both_appearances() {
    for (theme, _, page) in LOOKS {
        let mut events = from_palette("containers");
        // The pointer on the first row: its buttons show.
        events.push(json!({ "t": 1.5, "type": "move", "key": "ctr-0" }));
        events.push(end(2.0));
        let Some(run) = run(&format!("containers-{theme}"), &["--demo", "--theme", theme], Some(Value::Array(events))) else { return };
        assert_eq!(run.rect("containers-toolbar"), [52.0, 44.0, 1228.0, 40.0], "{theme}");
        for said in [
            "Containers",
            "7 containers, 5 running",
            "shop",
            "5 running",
            "Not in a project",
            "shop-api-1",
            "registry.example/shop/api:2.14.0",
            "healthy",
            "127.0.0.1:8080 → 8080",
            "443 → 443, 80 → 80",
            "Stopped 2 days ago, exit code 137",
            "Logs",
            "Shell",
            "Restart",
        ] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        assert!(run.tree_text.contains("Containers (web-7731)") || run.tree_text.contains("Containers"), "{theme}");
        let px = run.pixels();
        assert_eq!(at(&px, 700, 760), page, "{theme}: the page's background under the list");
        // Nothing blue but what's selected: no row is.
        assert_eq!(near(&px, [60, 90, 1270, 700], [0x00, 0x71, 0xe3]), 0, "{theme}: blue on the page");
    }
    // A machine without Docker says how to install it.
    let mut events = vec![tap(0.3, "server-3")];
    events.extend(from_palette("containers").into_iter().map(|mut e| {
        e["t"] = json!(e["t"].as_f64().unwrap() + 0.6);
        e
    }));
    events.push(end(2.4));
    let Some(run) = run("containers-none", &["--demo"], Some(Value::Array(events))) else { return };
    for said in ["No Docker here", "curl -fsSL https://get.docker.com | sudo sh", "Copy", "Type it in a terminal"] {
        assert!(run.tree_text.contains(said), "{said} isn't on the page");
    }
}

#[test]
fn logs_open_in_a_new_tab_inside_the_container() {
    let mut events = from_palette("containers");
    events.push(json!({ "t": 1.5, "type": "move", "key": "ctr-0" }));
    events.push(tap(1.8, "ctr-0-logs"));
    events.push(end(2.6));
    let Some(run) = run("containers-logs", &["--demo"], Some(Value::Array(events))) else { return };
    assert!(run.tree_text.contains("shop-api-1 logs"), "no tab for the logs");
    assert!(run.element("term").is_some(), "the logs' terminal isn't on screen");
}

/// "Free up disk space" from the sidebar's drive on web-7731 (Production): the disks, the
/// cleanups with their commands, and a cleanup that ran after both yeses saying what it freed.
#[test]
fn free_up_disk_space_in_both_appearances() {
    for theme in ["dark", "light"] {
        let script = json!([tap(0.3, "disk"), tap(1.0, "clean-run-1"), tap(1.3, "clean-yes-1"), tap(1.6, "clean-prod-1"), end(2.4)]);
        let Some(run) = run(&format!("disk-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        for said in ["Free up disk space", "/: 68 GB of 80 GB · 85%", "Unused Docker images", "docker image prune -a -f", "Docker build cache", "Done. Freed 3.7 GB.", "Biggest here"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        assert!(run.rect("disk-vol-0")[1] < run.rect("disk-cleanups")[1]);
    }
}

#[test]
fn stats_has_graphs_meters_and_programs_in_both_appearances() {
    for (theme, _, page) in LOOKS {
        let mut events = from_palette("stats");
        events.push(end(2.0));
        let Some(run) = run(&format!("stats-{theme}"), &["--demo", "--theme", theme], Some(Value::Array(events))) else { return };
        assert_eq!(run.rect("stats-toolbar")[3], 40.0, "{theme}");
        for said in [
            "Last 5 minutes",
            "CPU 38%",
            "8 cores",
            "Load 1.12  0.94  0.88",
            "Memory 60%",
            "Network",
            "November: ↓ 96 GB  ↑ 31 GB",
            "/srv/shop: 211 GB of 500 GB",
            "Free up disk space",
            "web-7731, Linux 6.8.0-45-generic, up 37 d 5 h",
            "Busiest programs",
            "node",
            "14.2%",
        ] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        let px = run.pixels();
        let graph = run.rect("stat-cpu");
        // The graph's line is drawn in the accent, so a quiet server's graph is still seen.
        assert!(near(&px, [graph[0] as u32, graph[1] as u32, (graph[0] + graph[2]) as u32, (graph[1] + graph[3]) as u32], [0x00, 0x71, 0xe3]) > 20, "{theme}: the CPU line isn't in the accent");
        assert_eq!(at(&px, 1270, 790), page, "{theme}");
    }
}

#[test]
fn a_question_mark_opens_its_explanation_under_it_in_both_appearances() {
    for (theme, pop, _) in LOOKS {
        // Settings → Tor, and the pointer resting on the "?" after "Connect through Tor".
        let script = json!([tap(0.3, "settings-gear"), tap(0.8, "nav-tor"), { "t": 1.3, "type": "move", "key": "tor-switch-about" }, end(2.4)]);
        let Some(run) = run(&format!("help-tip-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        assert_eq!(run.label("tor-switch-about"), "About Tor", "{theme}");
        assert!(run.tree_text.contains("Tor is a free network run by volunteers"), "{theme}: the card's words aren't in the tree");
        // The card hangs under its "?", in the popover's colour, inside the window.
        let ([mx, my, mw, mh], [cx, cy, cw, ch]) = (run.rect("tor-switch-about"), run.rect("help-card"));
        assert!(cy >= my + mh && cy <= my + mh + 8.0, "{theme}: the card at {cy}, the ? ends at {}", my + mh);
        assert!(cx <= mx + mw / 2.0 && cx + cw >= mx + mw / 2.0, "{theme}: the card isn't under the ?");
        assert!(cx >= 8.0 && cx + cw <= 1272.0 && cy + ch <= 792.0, "{theme}");
        assert!(cw <= 300.0, "{theme}: {cw} wide");
        let px = run.pixels();
        assert_eq!(at(&px, (cx + 6.0) as u32, (cy + ch / 2.0) as u32), pop, "{theme}: the card's fill");
    }
}

/// A real display of its own (Xvfb), driven with xdotool and seen with ImageMagick's `import`:
/// for what a headless run can't show, as Rune opens no second window there. `None` (after
/// saying why) where those aren't installed or there's nothing to draw with.
struct Display {
    xvfb: std::process::Child,
    name: String,
}

impl Display {
    fn start(test: &str) -> Option<Display> {
        let have = |tool: &str| Command::new("sh").arg("-c").arg(format!("command -v {tool}")).output().is_ok_and(|o| o.status.success());
        if !cfg!(target_os = "linux") || !vulkan() || !have("Xvfb") || !have("xdotool") || !have("import") {
            eprintln!("{test}: skipped, needs Linux with Xvfb, xdotool, ImageMagick and Vulkan");
            return None;
        }
        // Xvfb picks a free display and says which on the pipe.
        let mut xvfb = Command::new("Xvfb")
            .args(["-displayfd", "1", "-screen", "0", "1600x1000x24", "-nolisten", "tcp"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut out = xvfb.stdout.take()?;
        let (mut number, mut byte) = (String::new(), [0u8; 1]);
        while std::io::Read::read(&mut out, &mut byte).ok()? == 1 && byte[0] != b'\n' {
            number.push(byte[0] as char);
        }
        Some(Display { xvfb, name: format!(":{}", number.trim()) })
    }

    fn xdotool(&self, args: &[&str]) -> String {
        let out = Command::new("xdotool").env("DISPLAY", &self.name).args(args).output().expect("xdotool runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Waits up to `secs` for `n` of the app's windows on screen; them, oldest first.
    fn windows(&self, n: usize, secs: u64) -> Vec<String> {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let found: Vec<String> = self.xdotool(&["search", "--onlyvisible", "--name", "^SSHeesh Preview$"]).split_whitespace().map(str::to_owned).collect();
            if found.len() >= n || std::time::Instant::now() > until {
                return found;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    /// What a window shows, as RGBA bytes.
    fn picture(&self, window: &str, path: &Path) -> (u32, u32, Vec<u8>) {
        let done = Command::new("import").env("DISPLAY", &self.name).args(["-window", window]).arg(format!("png32:{}", path.display())).status().expect("import runs");
        assert!(done.success(), "no picture of {window}");
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.xvfb.kill();
        let _ = self.xvfb.wait();
    }
}

/// The app on that display, ended with its test.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn shift_click_on_a_server_opens_it_in_a_window_of_its_own() {
    // Where the sidebar's servers and the terminal are: the same window drawn headless.
    let Some(layout) = run("new-window-layout", &["--demo"], None) else { return };
    assert_eq!((layout.label("server-1"), layout.label("server-4")), ("web-7731".into(), "db-main".into()));
    let Some(display) = Display::start("shift_click_on_a_server_opens_it_in_a_window_of_its_own") else { return };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("window").join("new-window");
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    cmd.arg("--demo").env("DISPLAY", &display.name).env_remove("WAYLAND_DISPLAY");
    cmd.env("HOME", &home).env("XDG_DATA_HOME", home.join("data")).env("XDG_CONFIG_HOME", home.join("config")).env("SSHEE_PREVIEW_DIR", home.join(".sshee-preview"));
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if std::env::var_os("VK_ICD_FILENAMES").is_none() && Path::new(LVP).exists() {
        cmd.env("VK_ICD_FILENAMES", LVP);
    }
    let _app = Running(cmd.spawn().expect("the preview starts"));
    let first = display.windows(1, 90);
    assert_eq!(first.len(), 1, "the window didn't open");
    let main = first[0].clone();
    std::thread::sleep(std::time::Duration::from_secs(10));
    // A server's button, a little inside its corner (its fill, away from its icon).
    let fill = |picture: &(u32, u32, Vec<u8>), key: &str| {
        let [x, y, _, _] = layout.rect(key);
        at(picture, x as u32 + 3, y as u32 + 3)
    };
    let before = display.picture(&main, &dir.join("main-before.png"));
    assert_ne!(fill(&before, "server-1"), fill(&before, "server-4"), "web-7731 is the server on screen");

    // Shift+click on db-main, Shift held as a hand holds it: the window takes a change of the
    // keys held at its next frame, and a frame drawn in software can take most of a second.
    let [x, y, w, h] = layout.rect("server-4");
    display.xdotool(&["windowfocus", "--sync", &main]);
    display.xdotool(&["mousemove", "--window", &main, &((x + w / 2.0) as u32).to_string(), &((y + h / 2.0) as u32).to_string()]);
    let pause = || std::thread::sleep(std::time::Duration::from_millis(500));
    pause();
    display.xdotool(&["keydown", "shift"]);
    std::thread::sleep(std::time::Duration::from_secs(2));
    display.xdotool(&["click", "1"]);
    pause();
    display.xdotool(&["keyup", "shift"]);
    let both = display.windows(2, 60);
    if both.len() < 2 {
        let _ = Command::new("import").env("DISPLAY", &display.name).args(["-window", "root"]).arg(dir.join("fail.png")).status();
    }
    assert_eq!(both.len(), 2, "no second window opened");
    let second = both.iter().find(|w| **w != main).unwrap().clone();
    // Beside the first, clear of its sidebar, and the pointer away from both.
    display.xdotool(&["windowmove", &second, "320", "200"]);
    display.xdotool(&["mousemove", "1595", "5"]);
    std::thread::sleep(std::time::Duration::from_secs(6));
    let after = display.picture(&main, &dir.join("main-after.png"));
    let other = display.picture(&second, &dir.join("second.png"));
    // The first window is still on web-7731; the new one is on db-main, picked as web-7731 is
    // in the first.
    assert_eq!((fill(&after, "server-1"), fill(&after, "server-4")), (fill(&before, "server-1"), fill(&before, "server-4")), "the first window changed");
    assert_eq!(fill(&other, "server-4"), fill(&before, "server-1"), "db-main isn't the server on screen in the new window");
    assert_eq!(fill(&other, "server-1"), fill(&before, "server-4"), "web-7731 is picked in the new window");
    // Its terminal isn't the first window's: db-main's prompt, not web-7731's log.
    let [tx, ty, tw, th] = layout.rect("term");
    let rows = |p: &(u32, u32, Vec<u8>)| (ty as u32 + 8..(ty + th.min(200.0)) as u32).map(|y| (tx as u32 + 8..(tx + tw.min(600.0)) as u32).map(|x| at(p, x, y)).collect::<String>()).collect::<Vec<_>>();
    assert_ne!(rows(&other), rows(&after), "the new window shows the first window's terminal");
}
