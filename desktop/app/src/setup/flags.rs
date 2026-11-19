//! Which of the setup roles a start asks for, from the program's name and its arguments.
//!
//! The installer understands the flags Tauri's NSIS installer took, because 0.10's updater
//! starts it with them (tauri-plugin-updater 2.14.1, `updater_parameters`): `/P /UPDATE /R
//! /ARGS <0.10's own arguments>` (`installMode: "passive"` in `tauri.conf.json`). NSIS's own:
//!
//! - `/S` silent: no window at all; `/P` passive: a window with progress, nothing to answer;
//! - `/UPDATE`: an update of an installed copy: shortcuts the person removed stay removed;
//! - `/R`: start the app when done, with what follows `/ARGS` as its arguments;
//! - `/NS`: no shortcuts; `/D=<folder>`: where to install (NSIS: the last flag, unquoted, may
//!   hold spaces).
//!
//! Started with none of them, under a name ending in `-setup.exe` (`SSHeesh_1.0.0_x64-setup.exe`
//! as downloaded), it's the installer with its window. Our own roles: `--uninstall` (what
//! Windows' "Installed apps" starts), `--finish-install <folder>` (the `RunOnce` entry that
//! completes a replacement cut off by a power loss), and `--uninstall-from <folder>` (the
//! uninstaller again, from a copy in the temporary folder, so it can delete the program).

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// How much the installer shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ui {
    /// Its window, asking before it installs.
    Full,
    /// Its window, with progress only (`/P`).
    Passive,
    /// Nothing (`/S`).
    Silent,
}

/// An install or an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub ui: Ui,
    /// `/UPDATE`.
    pub update: bool,
    /// `/R`: start the app when done.
    pub run_after: bool,
    /// After `/ARGS`: the app's arguments for that start.
    pub app_args: Vec<OsString>,
    /// `/D=`: the folder asked for.
    pub dir: Option<PathBuf>,
    /// `/NS`.
    pub no_shortcuts: bool,
}

/// What this start is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Install(Install),
    /// `--finish-install <folder>`.
    Finish { dir: PathBuf },
    /// `--uninstall [/S]`.
    Uninstall { quiet: bool },
    /// `--uninstall-from <folder> [--delete-data] [/S]`: the uninstaller's second half.
    UninstallFrom { dir: PathBuf, delete_data: bool, quiet: bool },
}

pub const FINISH: &str = "--finish-install";
pub const UNINSTALL: &str = "--uninstall";
pub const UNINSTALL_FROM: &str = "--uninstall-from";
pub const DELETE_DATA: &str = "--delete-data";

/// NSIS's flags: what tells an installer's start from the app's.
fn is_nsis_flag(arg: &OsStr) -> bool {
    let Some(a) = arg.to_str() else { return false };
    let upper = a.to_ascii_uppercase();
    matches!(upper.as_str(), "/S" | "/P" | "/R" | "/UPDATE" | "/NS" | "/ARGS") || upper.starts_with("/D=")
}

/// Whether the program's file name says it's the downloaded installer.
pub fn named_as_installer(exe_name: &str) -> bool {
    let lower = exe_name.to_ascii_lowercase();
    lower.ends_with("-setup.exe") || lower.ends_with("_setup.exe") || lower == "setup.exe"
}

/// The role of a start of the program named `exe_name` with `args` (after the program's own
/// name). `None`: the app (or a helper). `Some(Err)`: a setup role asked for wrongly.
pub fn role(exe_name: &str, args: &[OsString]) -> Option<Result<Role, String>> {
    let first = args.first().map(OsString::as_os_str);
    match first.and_then(OsStr::to_str) {
        Some(FINISH) => {
            return Some(match args {
                [_, dir] if !dir.is_empty() => Ok(Role::Finish { dir: dir.into() }),
                _ => Err(format!("usage: {FINISH} <folder>")),
            })
        }
        Some(UNINSTALL) => {
            return Some(match &args[1..] {
                [] => Ok(Role::Uninstall { quiet: false }),
                [s] if s.eq_ignore_ascii_case("/S") => Ok(Role::Uninstall { quiet: true }),
                _ => Err(format!("usage: {UNINSTALL} [/S]")),
            })
        }
        Some(UNINSTALL_FROM) => {
            let Some(dir) = args.get(1).filter(|d| !d.is_empty()) else { return Some(Err(format!("usage: {UNINSTALL_FROM} <folder> [{DELETE_DATA}] [/S]"))) };
            let (mut delete_data, mut quiet) = (false, false);
            for a in &args[2..] {
                if a == DELETE_DATA {
                    delete_data = true;
                } else if a.eq_ignore_ascii_case("/S") {
                    quiet = true;
                } else {
                    return Some(Err(format!("unknown argument {}", a.to_string_lossy())));
                }
            }
            return Some(Ok(Role::UninstallFrom { dir: dir.into(), delete_data, quiet }));
        }
        _ => {}
    }
    let flagged = first.is_some_and(is_nsis_flag);
    if !flagged && !named_as_installer(exe_name) {
        return None;
    }
    Some(install_flags(args).map(Role::Install))
}

/// NSIS's flags, in any order, until `/ARGS` (whatever follows is the app's).
pub fn install_flags(args: &[OsString]) -> Result<Install, String> {
    let mut install = Install { ui: Ui::Full, update: false, run_after: false, app_args: Vec::new(), dir: None, no_shortcuts: false };
    let mut it = args.iter().enumerate();
    while let Some((at, arg)) = it.next() {
        let text = arg.to_str().ok_or("an argument that isn't text")?;
        let upper = text.to_ascii_uppercase();
        match upper.as_str() {
            "/S" => install.ui = Ui::Silent,
            // Silent wins over passive, as in NSIS (`/P /S` stays silent).
            "/P" => {
                if install.ui != Ui::Silent {
                    install.ui = Ui::Passive;
                }
            }
            "/R" => install.run_after = true,
            "/UPDATE" => install.update = true,
            "/NS" => install.no_shortcuts = true,
            "/ARGS" => {
                install.app_args = args[at + 1..].to_vec();
                break;
            }
            _ if upper.starts_with("/D=") => {
                // NSIS: the rest of the line, spaces and all; the system split it at them.
                let mut words = vec![text[3..].to_owned()];
                for (_, more) in it.by_ref() {
                    let more = more.to_str().ok_or("an argument that isn't text")?;
                    if is_nsis_flag(OsStr::new(more)) {
                        return Err("/D= goes last".into());
                    }
                    words.push(more.to_owned());
                }
                let dir = words.join(" ");
                let dir = dir.trim().trim_matches('"');
                if dir.is_empty() {
                    return Err("/D= needs a folder".into());
                }
                install.dir = Some(dir.into());
                break;
            }
            _ => return Err(format!("unknown argument {text}")),
        }
    }
    // /S with /R still starts the app; with neither /P nor /S the window asks first.
    Ok(install)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// Exactly what 0.10's updater passes (`installMode: "passive"`, restart after install),
    /// as Windows splits it back into arguments: `/ARGS` and then 0.10's own arguments.
    #[test]
    fn the_flags_0_10s_updater_passes_are_an_update() {
        let installer = "SSHeesh-1.0.0-installer.exe";
        let r = role(installer, &os(&["/P", "/UPDATE", "/R", "/ARGS"])).unwrap().unwrap();
        assert_eq!(r, Role::Install(Install { ui: Ui::Passive, update: true, run_after: true, app_args: vec![], dir: None, no_shortcuts: false }));
        // 0.10 started from a notification's link: its arguments go on to 1.0.
        let r = role(installer, &os(&["/P", "/UPDATE", "/R", "/ARGS", "ssheesh://notification/3", "/S"])).unwrap().unwrap();
        let Role::Install(i) = r else { panic!() };
        assert_eq!(i.app_args, os(&["ssheesh://notification/3", "/S"]), "after /ARGS nothing is a flag");
        assert_eq!(i.ui, Ui::Passive);
    }

    #[test]
    fn nsis_flags_read_as_nsis_reads_them() {
        let i = install_flags(&os(&["/S", "/P"])).unwrap();
        assert_eq!(i.ui, Ui::Silent);
        let i = install_flags(&os(&["/p", "/update", "/ns"])).unwrap();
        assert!(i.update && i.no_shortcuts && !i.run_after && i.ui == Ui::Passive);
        let i = install_flags(&os(&["/S", r"/D=D:\Apps\My", "SSHeesh"])).unwrap();
        assert_eq!(i.dir, Some(PathBuf::from(r"D:\Apps\My SSHeesh")));
        assert!(install_flags(&os(&[r"/D=C:\x", "/S"])).is_err(), "/D= goes last");
        assert!(install_flags(&os(&["/D="])).is_err());
        assert!(install_flags(&os(&["/X"])).is_err());
        assert_eq!(install_flags(&os(&[])).unwrap().ui, Ui::Full);
    }

    #[test]
    fn the_app_is_never_mistaken_for_the_installer() {
        for args in [&[][..], &["--demo"], &["ssheesh://notification/1"], &["--canary-watch", "x", "1", "--"], &["--tor-proxy", "h", "1"]] {
            assert_eq!(role("ssheesh.exe", &os(args)), None, "{args:?}");
            assert_eq!(role("SSHeesh.exe", &os(args)), None, "{args:?}");
        }
        // Downloaded under its installer name, it's the installer, asking first.
        assert_eq!(role("SSHeesh_1.0.0_x64-setup.exe", &os(&[])), Some(Ok(Role::Install(install_flags(&[]).unwrap()))));
        assert!(named_as_installer("SSHEESH_1.0.1_X64-SETUP.EXE") && named_as_installer("setup.exe"));
        assert!(!named_as_installer("ssheesh.exe.new") && !named_as_installer("ssheesh-preview.exe"));
    }

    #[test]
    fn our_own_roles() {
        assert_eq!(role("ssheesh.exe", &os(&[UNINSTALL])), Some(Ok(Role::Uninstall { quiet: false })));
        assert_eq!(role("ssheesh.exe", &os(&[UNINSTALL, "/S"])), Some(Ok(Role::Uninstall { quiet: true })));
        assert!(role("ssheesh.exe", &os(&[UNINSTALL, "x"])).unwrap().is_err());
        assert_eq!(role("ssheesh.exe.new", &os(&[FINISH, r"C:\Users\p\AppData\Local\SSHeesh"])), Some(Ok(Role::Finish { dir: r"C:\Users\p\AppData\Local\SSHeesh".into() })));
        assert!(role("ssheesh.exe", &os(&[FINISH])).unwrap().is_err());
        assert_eq!(
            role("ssheesh-uninstall.exe", &os(&[UNINSTALL_FROM, r"D:\Apps\SSHeesh", DELETE_DATA])),
            Some(Ok(Role::UninstallFrom { dir: r"D:\Apps\SSHeesh".into(), delete_data: true, quiet: false }))
        );
        assert!(role("x.exe", &os(&[UNINSTALL_FROM])).unwrap().is_err());
        assert!(role("x.exe", &os(&[UNINSTALL_FROM, "d", "--what"])).unwrap().is_err());
    }
}
