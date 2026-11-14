//! `Places::this_computer` on Linux follows `XDG_DATA_HOME` and `XDG_CONFIG_HOME`, as Tauri
//! did for 0.10. Alone in its own test binary, because it changes the process's environment.

#[cfg(target_os = "linux")]
#[test]
fn linux_follows_the_xdg_folders_tauri_followed() {
    use sshee_app::legacy::Places;
    use std::path::Path;

    let data = "/home/ann/.var/data";
    let config = "/home/ann/.var/config";
    std::env::set_var("XDG_DATA_HOME", data);
    std::env::set_var("XDG_CONFIG_HOME", config);
    let here = Places::this_computer();
    let linux = Places::linux(Path::new(data), Path::new(config));
    assert_eq!((&here.web_roots, &here.window_state), (&linux.web_roots, &linux.window_state));
    assert_eq!(here.web_roots[0], Path::new("/home/ann/.var/data/io.github.footprintsntrails.ssheesh"));
    // The copies are made in 1.0's own data folder, not the shared `/tmp`.
    assert_eq!(
        here.scratch.as_deref(),
        Some(Path::new("/home/ann/.var/data/io.github.footprintsntrails.ssheesh/import-scratch"))
    );
}
