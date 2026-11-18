//! Tor's popover and a server's tunnels in SSHeesh Preview's window, headless: `--demo` has Tor
//! on at 84 ms and made-up saved tunnels. Rune's `--script` is the pointer; what the window
//! holds afterwards is read from Rune's element tree (`--tree`) and from the picture (`--snap`),
//! in both appearances. `SSHEE_PICTURES=<folder>` keeps a copy of each picture there.
//!
//! Needs a Vulkan driver, as `window.rs` does; with none the tests say so and pass.
//!
//! The one test that starts the real Tor needs the network, so it runs only with
//! `SSHEE_TOR_NETWORK=1`: `cargo test -p ssheesh-preview --test tor -- --nocapture`.

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

struct Run {
    tree: Value,
    tree_text: String,
}

impl Run {
    fn element(&self, key: &str) -> Option<&Value> {
        self.tree["elements"].as_array()?.iter().find(|e| e["key"] == key)
    }

    fn rect(&self, key: &str) -> [f64; 4] {
        let r = &self.element(key).unwrap_or_else(|| panic!("no element {key}"))["rect"];
        [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap())
    }

    fn says(&self, text: &str) -> bool {
        self.tree_text.contains(text)
    }
}

/// Runs `--demo` in `theme` with `script` (ending in a mark named "end").
fn run(name: &str, theme: &str, script: Value) -> Option<Run> {
    if !vulkan() {
        eprintln!("{name}: skipped, no Vulkan driver here");
        return None;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tor").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let (png, tree, script_path) = (dir.join("end.png"), dir.join("tree.json"), dir.join("script.json"));
    std::fs::write(&script_path, serde_json::to_string_pretty(&script).unwrap()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    cmd.env("HOME", &home).env("XDG_DATA_HOME", home.join("data")).env("XDG_CONFIG_HOME", home.join("config"));
    cmd.env("SSHEE_PREVIEW_DIR", home.join(".sshee-preview"));
    cmd.args(["--demo", "--theme", theme]).arg("--snap").arg(&png).arg("--tree").arg(&tree).args(["--scale", "2", "--size", "1280x800"]);
    cmd.arg("--script").arg(&script_path).args(["--at", "mark:end"]);
    if cfg!(target_os = "linux") && std::env::var_os("VK_ICD_FILENAMES").is_none() && Path::new(LVP).exists() {
        cmd.env("VK_ICD_FILENAMES", LVP);
    }
    let result = cmd.output().expect("the preview runs");
    assert!(result.status.success(), "{name}: {}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
    if let Some(keep) = std::env::var_os("SSHEE_PICTURES") {
        let keep = PathBuf::from(keep);
        let _ = std::fs::create_dir_all(&keep);
        let _ = std::fs::copy(&png, keep.join(format!("{name}.png")));
    }
    let tree_text = std::fs::read_to_string(&tree).unwrap();
    Some(Run { tree: serde_json::from_str(&tree_text).unwrap(), tree_text })
}

fn tap(t: f64, key: &str) -> Value {
    json!({ "t": t, "type": "tap", "key": key })
}

fn end(t: f64) -> Value {
    json!({ "t": t, "type": "mark", "name": "end" })
}

#[test]
fn the_onion_opens_tor_beside_the_sidebar_in_both_appearances() {
    for theme in ["dark", "light"] {
        let Some(run) = run(&format!("tor-on-{theme}"), theme, json!([{ "t": 0.3, "type": "tap", "key": "tor", "button": "secondary" }, end(1.0)])) else { return };
        assert!(run.says("Tor is on") && run.says("84 ms round trip"), "{theme}: {}", run.tree_text);
        // "Tor 84 ms" in the status bar, as the design has it.
        assert!(run.element("tor-time").is_some(), "{theme}");
        // The made-up log and what can be done.
        for key in ["tor-switch", "tor-log", "tor-new-routes", "tor-show-log"] {
            assert!(run.element(key).is_some(), "{theme}: no {key}");
        }
        // Beside the sidebar, its foot level with the onion's.
        let (panel, onion) = (run.rect("tor-panel"), run.rect("tor"));
        assert!(panel[0] >= onion[0] + onion[2], "{theme}: {panel:?} over the sidebar");
        assert!((panel[1] + panel[3] - (onion[1] + onion[3])).abs() < 80.0, "{theme}: {panel:?} far from the onion {onion:?}");
        assert!(panel[1] >= 0.0 && panel[1] + panel[3] <= 800.0, "{theme}: {panel:?} out of the window");
    }
}

#[test]
fn switched_off_tor_says_so_and_the_status_bar_lets_go() {
    let Some(run) = run("tor-off-dark", "dark", json!([tap(0.3, "tor"), { "t": 0.6, "type": "tap", "key": "tor", "button": "secondary" }, end(1.2)])) else { return };
    assert!(run.says("Tor is off") && run.says("your own address"), "{}", run.tree_text);
    assert!(run.element("tor-time").is_none() && run.element("tor-log").is_none());
}

#[test]
fn a_servers_tunnels_open_from_its_menu() {
    for theme in ["dark", "light"] {
        let script = json!([{ "t": 0.3, "type": "tap", "key": "server-1", "button": "secondary" }, tap(0.6, "sm-5"), end(1.2)]);
        let Some(run) = run(&format!("tunnels-{theme}"), theme, script) else { return };
        assert!(run.element("tunnels-dialog").is_some(), "{theme}");
        // web-7731's three: Grafana on, the proxy off, the dev site on the server.
        assert!(run.says("Grafana") && run.says("On at") && run.says("Copy localhost:3000"), "{theme}: {}", run.tree_text);
        // What the server runs, each with its button: Grafana through its saved tunnel.
        assert!(run.says("Web apps on this server") && run.says("Shop: storefront") && run.says("Other ports (2)"), "{theme}: {}", run.tree_text);
        assert!(run.element("app-1-open").is_some() && run.element("app-0-stop").is_some(), "{theme}");
        assert!(run.says("Browse through the server") && run.says("Off"));
        assert!(run.says("On at port 9000 on the server") && run.says("2 on"));
    }
}

#[test]
fn a_tunnel_that_cant_get_its_port_says_so() {
    let script = json!([{ "t": 0.3, "type": "tap", "key": "server-4", "button": "secondary" }, tap(0.6, "sm-5"), tap(0.9, "tunnel-tun-4-edit"), end(1.4)]);
    let Some(run) = run("tunnels-form-dark", "dark", script) else { return };
    assert!(run.element("tunnels-form").is_some());
    assert!(run.says("localhost:5432 on this computer opens the server's port 5432."), "{}", run.tree_text);
}

/// The real thing: a copy of its own turns Tor on, as Settings' switch or the popover's does
/// (`tor/on` in its folder), and Tor has to come on within three minutes. Needs the network:
/// only with `SSHEE_TOR_NETWORK=1`.
#[test]
fn the_real_tor_starts_in_the_preview() {
    if std::env::var_os("SSHEE_TOR_NETWORK").is_none() {
        eprintln!("the_real_tor_starts_in_the_preview: skipped (SSHEE_TOR_NETWORK=1 runs it; it needs the network)");
        return;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tor").join("real");
    let _ = std::fs::remove_dir_all(&dir);
    let copy = dir.join("copy with a space");
    std::fs::create_dir_all(copy.join("tor")).unwrap();
    std::fs::write(copy.join("tor").join("on"), b"").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-preview"));
    cmd.env("SSHEE_PREVIEW_DIR", &copy).arg("--tor-check");
    let result = cmd.output().expect("the preview runs");
    let said = String::from_utf8_lossy(&result.stdout);
    assert!(result.status.success(), "{}\n{said}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
    assert!(said.contains("Tor is on"), "{said}");
}
