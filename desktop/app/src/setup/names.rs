//! Where SSHeesh is installed on Windows, and under which names: the same as Tauri's NSIS
//! installer for 0.10 (`installMode: "currentUser"`, `productName` "SSHeesh", `publisher`
//! "SSHeesh contributors", main binary `ssheesh`), so 1.0 takes 0.10's place, its shortcuts lead
//! to 1.0, and "Installed apps" lists one SSHeesh (plan §4 "never changes", §6.2 step 1).
//!
//! NSIS's template (`installer.nsi` of tauri-bundler 2) writes:
//! - `HKCU\Software\<publisher>\<product>`, default value: the install folder;
//! - `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<product>`: `DisplayName`,
//!   `DisplayIcon` and `InstallLocation` and `UninstallString` in quotes, `DisplayVersion`,
//!   `Publisher`, `NoModify`, `NoRepair`, `EstimatedSize` (KB), and the homepage three times;
//! - the Start menu shortcut `<Programs>\<product>.lnk`, its AppUserModelID the identifier.
//!
//! and finds a previous install (`RestorePreviousInstallLocation`) by the first key's default
//! value, then `InstallLocation`, then `%LOCALAPPDATA%\<product>`.

use std::path::{Path, PathBuf};

pub const PRODUCT: &str = "SSHeesh";
pub const PUBLISHER: &str = "SSHeesh contributors";
pub const HOMEPAGE: &str = "https://github.com/footprintsntrails/ssheesh";
/// The program's name in the install folder (0.10's `mainBinaryName`).
pub const MAIN_EXE: &str = "ssheesh.exe";
/// 0.10's uninstaller, beside it. 0.10 tells installed from portable by it (`lib.rs`).
pub const NSIS_UNINSTALLER: &str = "uninstall.exe";
/// Windows' AppUserModelID for SSHeesh: notifications, the taskbar, the shortcut.
pub const APP_ID: &str = "io.github.footprintsntrails.ssheesh";

pub const PRODUCT_KEY: &str = r"Software\SSHeesh contributors\SSHeesh";
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SSHeesh";
pub const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
/// The `RunOnce` value that finishes a replacement cut off half way.
pub const RUN_ONCE_VALUE: &str = "SSHeesh finish update";

/// The version an update replaced, kept for going back (plan §6.2 step 4).
pub const PREVIOUS: &str = "previous";
/// What's in `previous\`, and since when.
pub const KEPT_NOTE: &str = "kept.txt";
/// The registry entries as they were before, in `previous\`.
pub const REGISTRY_NOTE: &str = "registry.txt";
/// How long `previous\` and its Start menu item stay.
pub const KEEP_DAYS: u64 = 30;

/// In the Start menu's Programs folder.
pub const SHORTCUT: &str = "SSHeesh.lnk";
pub const PREVIOUS_SHORTCUT: &str = "SSHeesh (previous version).lnk";

/// The new program while it's written, then while it waits for its name.
pub const NEW_SUFFIX: &str = ".new";
/// A program replaced while `previous\` already holds the version to go back to.
pub const OLD_SUFFIX: &str = ".old";

/// A registry value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Number(u32),
}

/// NSIS writes `InstallLocation` in quotes; a person or another tool may not.
pub fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s).trim()
}

/// Where an install was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// `HKCU\Software\SSHeesh contributors\SSHeesh`.
    ProductKey,
    /// `InstallLocation`.
    InstallLocation,
    /// `%LOCALAPPDATA%\SSHeesh`, with `ssheesh.exe` in it.
    Default,
    /// Nowhere: a fresh install goes to `%LOCALAPPDATA%\SSHeesh`.
    Nothing,
}

/// The install folder, found as NSIS finds it: the product key's folder, then
/// `InstallLocation`, then `%LOCALAPPDATA%\SSHeesh`, each only where `ssheesh.exe` is
/// (`has_program`). A folder from the registry without the program is passed over: 0.10's
/// uninstaller may have left the key.
pub fn install_dir(product_key: Option<&str>, install_location: Option<&str>, local_app_data: &Path, has_program: impl Fn(&Path) -> bool) -> (PathBuf, Found) {
    let usable = |s: Option<&str>| s.map(unquote).filter(|s| !s.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute() || looks_absolute_windows(p)).filter(|p| has_program(p));
    if let Some(dir) = usable(product_key) {
        return (dir, Found::ProductKey);
    }
    if let Some(dir) = usable(install_location) {
        return (dir, Found::InstallLocation);
    }
    let default = local_app_data.join(PRODUCT);
    if has_program(&default) {
        (default, Found::Default)
    } else {
        (default, Found::Nothing)
    }
}

/// `C:\…` or `\\server\…`, read on any system (tests run on Linux).
fn looks_absolute_windows(p: &Path) -> bool {
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || s.starts_with(r"\\")
}

/// `dir` joined with `name` with Windows' separator, whatever system builds the text.
pub fn in_dir(dir: &Path, name: &str) -> String {
    let d = dir.to_string_lossy();
    let d = d.trim_end_matches(['\\', '/']);
    format!(r"{d}\{name}")
}

/// What the uninstall key holds for an install of `version` in `dir`, `size_kb` big.
/// `uninstaller`: the command "Installed apps" runs, already quoted.
pub fn uninstall_values(dir: &Path, version: &str, size_kb: u32, uninstaller: &str, quiet_uninstaller: &str) -> Vec<(&'static str, Value)> {
    let exe = in_dir(dir, MAIN_EXE);
    let folder = dir.to_string_lossy().trim_end_matches(['\\', '/']).to_owned();
    vec![
        ("DisplayName", Value::Text(PRODUCT.into())),
        ("DisplayIcon", Value::Text(format!("\"{exe}\""))),
        ("DisplayVersion", Value::Text(version.into())),
        ("Publisher", Value::Text(PUBLISHER.into())),
        ("InstallLocation", Value::Text(format!("\"{folder}\""))),
        ("UninstallString", Value::Text(uninstaller.into())),
        ("QuietUninstallString", Value::Text(quiet_uninstaller.into())),
        ("NoModify", Value::Number(1)),
        ("NoRepair", Value::Number(1)),
        ("EstimatedSize", Value::Number(size_kb)),
        ("URLInfoAbout", Value::Text(HOMEPAGE.into())),
        ("URLUpdateInfo", Value::Text(HOMEPAGE.into())),
        ("HelpLink", Value::Text(HOMEPAGE.into())),
    ]
}

/// The uninstall commands for `dir`: 0.10's NSIS uninstaller while it's there (until 1.0 has
/// proved itself, 0.10 put back must still find it, plan §6.2 step 7), else this program's.
pub fn uninstallers(dir: &Path, nsis_uninstaller_there: bool) -> (String, String) {
    if nsis_uninstaller_there {
        let u = in_dir(dir, NSIS_UNINSTALLER);
        (format!("\"{u}\""), format!("\"{u}\" /S"))
    } else {
        let exe = in_dir(dir, MAIN_EXE);
        (format!("\"{exe}\" {}", super::flags::UNINSTALL), format!("\"{exe}\" {} /S", super::flags::UNINSTALL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_0_10s() {
        assert_eq!(PRODUCT_KEY, format!(r"Software\{PUBLISHER}\{PRODUCT}"));
        assert!(UNINSTALL_KEY.ends_with(r"\Uninstall\SSHeesh"));
        // The same as in 0.10's tauri.conf.json, which the NSIS installer was made from.
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../../../src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(conf["productName"], PRODUCT);
        assert_eq!(conf["bundle"]["publisher"], PUBLISHER);
        assert_eq!(conf["identifier"], APP_ID);
        assert_eq!(conf["bundle"]["homepage"], HOMEPAGE);
        assert_eq!(conf["bundle"]["windows"]["nsis"]["installMode"], "currentUser");
        assert_eq!(conf["plugins"]["updater"]["windows"]["installMode"], "passive");
    }

    #[test]
    fn the_folder_is_found_as_nsis_finds_it() {
        let local = Path::new(r"C:\Users\p\AppData\Local");
        let has = |want: &'static [&'static str]| move |p: &Path| want.contains(&p.to_string_lossy().as_ref());
        // The product key first, even when InstallLocation says otherwise.
        let found = install_dir(Some(r"D:\Apps\SSHeesh"), Some(r#""E:\Other""#), local, has(&[r"D:\Apps\SSHeesh", r"E:\Other"]));
        assert_eq!(found, (PathBuf::from(r"D:\Apps\SSHeesh"), Found::ProductKey));
        // InstallLocation, in NSIS's quotes.
        let found = install_dir(None, Some(r#""D:\Apps\SSHeesh""#), local, has(&[r"D:\Apps\SSHeesh"]));
        assert_eq!(found, (PathBuf::from(r"D:\Apps\SSHeesh"), Found::InstallLocation));
        // A key left behind without the program is passed over.
        let found = install_dir(Some(r"D:\Gone"), None, local, has(&[r"C:\Users\p\AppData\Local/SSHeesh", r"C:\Users\p\AppData\Local\SSHeesh"]));
        assert_eq!(found.1, Found::Default);
        let found = install_dir(Some("relative"), None, local, |_| true);
        assert_eq!(found.1, Found::Default, "a relative folder isn't taken");
        let found = install_dir(None, None, local, |_| false);
        assert_eq!(found, (local.join("SSHeesh"), Found::Nothing));
    }

    #[test]
    fn the_uninstall_entry_is_nsis_s() {
        let dir = Path::new(r"D:\Apps\SSHeesh");
        let (u, q) = uninstallers(dir, true);
        assert_eq!(u, r#""D:\Apps\SSHeesh\uninstall.exe""#);
        assert_eq!(q, r#""D:\Apps\SSHeesh\uninstall.exe" /S"#);
        let (u, q) = uninstallers(dir, false);
        assert_eq!(u, r#""D:\Apps\SSHeesh\ssheesh.exe" --uninstall"#);
        assert_eq!(q, r#""D:\Apps\SSHeesh\ssheesh.exe" --uninstall /S"#);
        let values = uninstall_values(Path::new(r"D:\Apps\SSHeesh\"), "1.0.0", 52_000, &u, &q);
        let get = |name: &str| values.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("DisplayName"), Value::Text("SSHeesh".into()));
        assert_eq!(get("DisplayVersion"), Value::Text("1.0.0".into()));
        assert_eq!(get("InstallLocation"), Value::Text(r#""D:\Apps\SSHeesh""#.into()));
        assert_eq!(get("DisplayIcon"), Value::Text(r#""D:\Apps\SSHeesh\ssheesh.exe""#.into()));
        assert_eq!(get("Publisher"), Value::Text("SSHeesh contributors".into()));
        assert_eq!(get("EstimatedSize"), Value::Number(52_000));
        assert_eq!(get("NoModify"), Value::Number(1));
        assert_eq!(unquote(r#" "C:\a b" "#), r"C:\a b");
    }
}
