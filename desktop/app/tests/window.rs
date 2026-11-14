//! SSHeesh Preview's window headless: `--demo` (made-up servers, tabs and terminal output),
//! Rune's `--script` as the person at the mouse and keyboard, and what the window holds
//! afterwards, read from Rune's element tree (`--tree`) and from the picture (`--snap`).
//!
//! Needs a Vulkan driver (Rune draws headless frames on the GPU); with none the tests say so
//! and pass. Without a GPU, Mesa's software driver (lavapipe) does.
//!
//! Geometry at `--scale 1 --size 1280x800`, from the design: the bar is 44 pt, the sidebar 52,
//! the status bar 24; a sidebar button is 32 pt, the fifth server's (db-main) centred at
//! (26, 251).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

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
        assert_eq!(run.element("waits").map(|e| e["path"].as_str().unwrap().to_owned()).as_deref(), Some("window/body/rail/server-4/waits"));
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
        { "t": 0.3, "type": "tap", "x": 26.0, "y": 251.0 },
        // A new terminal there.
        key(0.8, "t", "cs"),
        // Around the end of the row to the first tab, and close it.
        key(1.2, "Tab", "c"),
        key(1.6, "w", "cs"),
        { "t": 2.2, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("drive", &["--demo"], Some(script)) else { return };
    // ~/shop is closed; Files came up; db-main has the agent's tab, seen now, and a new one.
    assert!(run.element("tab-1").is_none(), "the closed tab is still drawn");
    assert_eq!(run.label("tab-2"), "Files, web-7731");
    assert_eq!(run.fill("tab-2"), "#3a3a3cff");
    assert_eq!(run.label("tab-3"), "Migration 0042, db-main");
    // The agent's tab is db-main's first terminal, whatever it's called: the new one is its second.
    assert_eq!(run.label("tab-4"), "Terminal 2, db-main");
    assert!(run.element("waits").is_none(), "the agent's dot stayed after its tab was looked at");
    // Files isn't built yet: its place says so, and no terminal is drawn.
    assert!(run.element("term").is_none());
    assert!(run.tree_text.contains("This page isn't in the preview yet."));
    // The machine on screen is still web-7731, labelled Production in the title.
    assert!(run.tree_text.contains("web-7731"));
}

#[test]
fn a_tab_shows_its_own_terminal() {
    // db-main's agent tab, then a middle click on Files closes it without leaving.
    let script = json!([
        { "t": 0.3, "type": "tap", "x": 26.0, "y": 251.0 },
        { "t": 0.9, "type": "tap", "x": 440.0, "y": 22.0, "button": "middle" },
        { "t": 1.6, "type": "mark", "name": "end" }
    ]);
    let Some(run) = run("agent", &["--demo"], Some(script)) else { return };
    assert!(run.element("tab-2").is_none(), "Files wasn't closed by the middle click");
    assert_eq!(run.fill("tab-3"), "#3a3a3cff");
    assert!(run.element("pane-3").is_some());
    // The page shows that tab's terminal, not the shop's: the shop's listing has yellow lines
    // (commit ids, a restarting container), the agent's question has none.
    let yellow = [0xd9, 0xb5, 0x63];
    let Some(shop) = self::run("agent-shop", &["--demo"], None) else { return };
    assert!(near(&shop.pixels(), [64, 56, 700, 480], yellow) > 200, "the shop's terminal isn't as the fixture draws it");
    assert_eq!(near(&run.pixels(), [64, 56, 700, 480], yellow), 0, "the shop's terminal is still on the page");
    assert!(near(&run.pixels(), [64, 56, 700, 480], [0x7f, 0xc9, 0x8b]) > 50, "the agent's terminal drew nothing");
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

/// Where Settings' controls are at 1280 x 800: the gear is the sidebar's last button; the list
/// of sections starts under the search field, 30 points a row.
const GEAR: (f64, f64) = (26.0, 776.0);

fn tap(t: f64, (x, y): (f64, f64)) -> Value {
    json!({ "t": t, "type": "tap", "x": x, "y": y })
}

/// The centre of a section's row in Settings' list, by its place (Layout is 0).
fn section(n: usize) -> (f64, f64) {
    (168.0, 106.0 + 30.0 * n as f64)
}

/// The centre of the element keyed `key` in `run`.
fn centre(run: &Run, key: &str) -> (f64, f64) {
    let [x, y, w, h] = run.rect(key);
    (x + w / 2.0, y + h / 2.0)
}

#[test]
fn settings_is_a_page_in_the_window_in_both_appearances() {
    // (theme, the frame, the page, a box of rows, the section on screen)
    for (theme, frame, page, group) in [("dark", "#28282a", "#1c1c1e", "#242426"), ("light", "#ececee", "#ffffff", "#f5f5f7")] {
        let script = json!([tap(0.3, GEAR), { "t": 1.0, "type": "mark", "name": "end" }]);
        let Some(run) = run(&format!("settings-{theme}"), &["--demo", "--theme", theme], Some(script)) else { return };
        // The page takes the terminal's place; the bar, the sidebar and the status bar stay.
        assert!(run.element("term").is_none(), "{theme}: the terminal is still drawn");
        assert_eq!(run.rect("settings"), [52.0, 44.0, 1228.0, 732.0], "{theme}");
        assert_eq!(run.rect("status"), [52.0, 776.0, 1228.0, 24.0], "{theme}");
        assert_eq!(run.rect("settings-nav")[2], 232.0, "{theme}");
        assert_eq!(run.fill("window"), format!("{frame}ff"), "{theme}");
        // The list: ten sections, Layout first and picked in the selection's blue.
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
            let (x, y) = centre(&run, &format!("nav-{id}"));
            assert_eq!((x, y), section(n), "{theme}: {name}");
        }
        assert_eq!(run.fill("nav-layout"), "#0071e3ff", "{theme}");
        // Rows with their words and controls; one that comes later says so.
        for said in ["Name the server on every tab", "Sidebar", "Show sidebar", "Server vitals", "Compact mode", "Comes later.", "Search"] {
            assert!(run.tree_text.contains(said), "{theme}: {said} isn't in the tree");
        }
        // The picture: the page's colour, a box of rows on it, the picked section's blue, and
        // a switch that is on in the system's green.
        let px = run.pixels();
        assert_eq!(at(&px, 1250, 400), page, "{theme}: the page");
        assert_eq!(at(&px, 26, 500), frame, "{theme}: the sidebar");
        let [x, y, w, _] = run.rect("layout-tab-server");
        assert_eq!(at(&px, (x + w / 2.0) as u32, (y + 4.0) as u32), group, "{theme}: a row's box");
        assert_eq!(at(&px, 262, 106), "#0071e3", "{theme}: the picked section");
        let [sx, sy, sw, sh] = run.rect("layout-tab-server-control");
        let green = if theme == "dark" { [0x30, 0xd1, 0x58] } else { [0x34, 0xc7, 0x59] };
        assert!(near(&px, [sx as u32, sy as u32, (sx + sw) as u32, (sy + sh) as u32], green) > 100, "{theme}: the switch isn't green");
    }
}

#[test]
fn a_setting_changed_in_settings_takes_effect_at_once() {
    // The 0.10 shortcut opens it; Appearance; "Light" in a dark system.
    let first = json!([key(0.3, ",", "c"), tap(0.8, section(1)), { "t": 1.4, "type": "mark", "name": "end" }]);
    let Some(look) = run("settings-look", &["--demo", "--theme", "dark"], Some(first)) else { return };
    assert_eq!(look.fill("window"), "#28282aff");
    assert!(look.tree_text.contains("Match my system"));
    let light = centre(&look, "seg-1");
    let script = json!([
        key(0.3, ",", "c"),
        tap(0.8, section(1)),
        tap(1.4, light),
        // Then the terminal: a bigger cursor, and back to the tab with Esc.
        tap(2.0, section(2)),
        { "t": 2.6, "type": "key", "name": "Escape" },
        { "t": 3.2, "type": "mark", "name": "end" }
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
