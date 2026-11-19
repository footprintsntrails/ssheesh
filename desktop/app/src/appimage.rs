//! What 0.10's AppImage leaves in the environment of the program that follows it (plan §6.4).
//!
//! 0.10's updater writes 1.x over its AppImage, and its "Restart to update" starts the new file
//! with the old process's environment. The AppImage's GTK wrapper had pointed `GTK_PATH`,
//! `GIO_MODULE_DIR`, `XDG_DATA_DIRS` and others into its own mount, `/tmp/.mount_…`, which is
//! gone with 0.10. 1.x isn't an AppImage: those entries would only reach its local terminals
//! and whatever they start. So, at the start, every variable pointing into an AppImage mount
//! that isn't this program's goes, and lists such as `XDG_DATA_DIRS` lose just those parts.

use std::path::Path;

/// Variables an AppImage sets about itself.
const OWN: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// Variables that may point into the AppImage's mount: whole paths, or lists of them.
const PATHS: [&str; 16] = [
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GIO_MODULE_DIR",
    "GDK_PIXBUF_MODULE_FILE",
    "GSETTINGS_SCHEMA_DIR",
    "GI_TYPELIB_PATH",
    "GTK_IM_MODULE_FILE",
    "XDG_DATA_DIRS",
    "LD_LIBRARY_PATH",
    "PATH",
    "PERLLIB",
    "PYTHONPATH",
    "PYTHONHOME",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
];

/// What to change: a variable and its new value (`None`: remove it). Nothing when this program
/// runs from inside the AppImage `APPDIR` names (a real AppImage of a later 1.x), or when no
/// AppImage left anything.
pub fn changes(vars: &[(String, String)], exe: &Path) -> Vec<(String, Option<String>)> {
    let get = |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let Some(appdir) = get("APPDIR").map(|d| d.trim_end_matches('/')).filter(|d| !d.is_empty()) else { return Vec::new() };
    if exe.starts_with(appdir) {
        return Vec::new();
    }
    let inside = |part: &str| part == appdir || part.starts_with(&format!("{appdir}/"));
    let mut out = Vec::new();
    for name in OWN {
        if get(name).is_some() {
            out.push((name.to_owned(), None));
        }
    }
    for name in PATHS {
        let Some(value) = get(name) else { continue };
        let kept: Vec<&str> = value.split(':').filter(|p| !inside(p)).collect();
        if kept.len() == value.split(':').count() {
            continue;
        }
        let joined = kept.join(":");
        out.push((name.to_owned(), (!joined.is_empty()).then_some(joined)));
    }
    out
}

/// Applies [`changes`] to this process. Call while it's still one thread.
pub fn clean() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let vars: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
    let Ok(exe) = std::env::current_exe() else { return };
    for (name, value) in changes(&vars, &exe) {
        match value {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn what_0_10s_appimage_left_goes() {
        let left = vars(&[
            ("APPDIR", "/tmp/.mount_SSHeesAbCd"),
            ("APPIMAGE", "/home/p/Apps/SSHeesh_0.10.14_amd64.AppImage"),
            ("GTK_PATH", "/tmp/.mount_SSHeesAbCd/usr/lib/gtk-3.0"),
            ("XDG_DATA_DIRS", "/tmp/.mount_SSHeesAbCd/usr/share:/usr/local/share:/usr/share"),
            ("PATH", "/tmp/.mount_SSHeesAbCd/usr/bin:/usr/bin:/bin"),
            ("HOME", "/home/p"),
            ("GIO_MODULE_DIR", "/usr/lib/gio/modules"),
        ]);
        let got = changes(&left, Path::new("/home/p/Apps/SSHeesh_0.10.14_amd64.AppImage"));
        let get = |n: &str| got.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        assert_eq!(get("APPDIR"), Some(None));
        assert_eq!(get("APPIMAGE"), Some(None));
        assert_eq!(get("GTK_PATH"), Some(None));
        assert_eq!(get("XDG_DATA_DIRS"), Some(Some("/usr/local/share:/usr/share".into())));
        assert_eq!(get("PATH"), Some(Some("/usr/bin:/bin".into())));
        // A person's own settings stay.
        assert_eq!(get("HOME"), None);
        assert_eq!(get("GIO_MODULE_DIR"), None);
    }

    #[test]
    fn nothing_changes_without_an_appimage_or_inside_one() {
        assert!(changes(&vars(&[("PATH", "/usr/bin")]), Path::new("/usr/bin/ssheesh")).is_empty());
        let own = vars(&[("APPDIR", "/tmp/.mount_X"), ("PATH", "/tmp/.mount_X/usr/bin:/usr/bin")]);
        assert!(changes(&own, Path::new("/tmp/.mount_X/usr/bin/ssheesh")).is_empty());
        // A folder whose name only starts the same isn't inside.
        let near = vars(&[("APPDIR", "/tmp/.mount_X"), ("PATH", "/tmp/.mount_XY/bin")]);
        assert_eq!(changes(&near, Path::new("/opt/ssheesh")), [("APPDIR".to_owned(), None)]);
    }
}
