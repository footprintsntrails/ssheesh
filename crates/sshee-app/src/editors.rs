//! The editors on this computer that "Open in <editor>" in Files can start: VS Code and the
//! editors built on it, Zed, Sublime Text, the desktops' own, and the system's default text
//! editor, each with its name and how to start it.
//!
//! How to start one is a program's full path and its arguments, in the form
//! `sshee_core::edit_bridge::EditorCommand` takes (`{ program, args }`, with `{file}` as a whole
//! argument where the file goes, else last). The bridge starts it that way and no other: no
//! shell, no `cmd.exe`, no `.cmd` shim (on Windows VS Code is `Code.exe`, never `code.cmd`), no
//! "open with whatever handles this type", because the file's name, extension and all, comes
//! from the server. So:
//! - macOS starts an app bundle with `/usr/bin/open -a <bundle> <file>`, and "the system's
//!   default" is `open -t <file>`, the default *text* editor, whatever the file's extension;
//! - Windows takes the program that opens `.txt` files (Notepad unless you chose another);
//! - Linux takes the desktop entry for `text/plain` (`xdg-mime query default text/plain`) and
//!   starts its program with the file as one argument, never `xdg-open`, which would go by the
//!   file's type and could run a `.desktop` file the server named.
//!
//! Which editor "Open in" uses is the setting `externalEditor`: an editor's id, or
//! `custom:<path>` for one picked with "Choose an app…". With no setting (or one whose editor is
//! gone), it's the first VS Code-like editor found, else the system's default.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The setting that holds the chosen editor's id.
pub const SETTING_KEY: &str = "externalEditor";
/// The id of the system's default text editor.
pub const SYSTEM: &str = "system";
/// The start of the setting for an editor picked with "Choose an app…": `custom:<path>`.
pub const CUSTOM_PREFIX: &str = "custom:";
/// Where the file goes among an editor's arguments: the same as
/// `sshee_core::edit_bridge::FILE_ARG`.
pub const FILE_ARG: &str = "{file}";

/// An editor that can be started on a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editor {
    /// Stable: what the setting stores.
    pub id: String,
    /// What the menu says: "Open in VS Code".
    pub name: String,
    /// The program, by its full path.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// VS Code or an editor built on it: the default when there's one.
    pub vscode_like: bool,
}

/// Which system's layout to look in. Tests look in each on any system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Where to look, as this computer has it.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// The folders in PATH.
    pub path: Vec<PathBuf>,
    pub home: Option<PathBuf>,
    /// Windows: %LOCALAPPDATA%, %ProgramFiles%, %ProgramFiles(x86)%, %SystemRoot%.
    pub local_app_data: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub system_root: Option<PathBuf>,
    /// macOS: /Applications, ~/Applications, /System/Applications.
    pub applications: Vec<PathBuf>,
    /// Linux: the folder the standard install paths (`/usr/share/code/bin/code`) are under:
    /// `/` on this computer, None to skip them.
    pub fs_root: Option<PathBuf>,
    /// Linux: where desktop entries are ($XDG_DATA_HOME and $XDG_DATA_DIRS, each + /applications).
    pub desktop_dirs: Vec<PathBuf>,
    /// Linux: the desktop entry for text/plain, as `xdg-mime query default text/plain` says.
    pub text_default: Option<String>,
    /// Windows: the program that opens .txt files.
    pub txt_handler: Option<PathBuf>,
}

impl Places {
    /// This computer's places.
    pub fn here() -> Places {
        let env_path = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from);
        let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
        let home = dirs::home_dir();
        let os = Os::current();
        let mut places = Places { path, home: home.clone(), ..Places::default() };
        match os {
            Os::Windows => {
                places.local_app_data = env_path("LOCALAPPDATA");
                places.program_files = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"].iter().filter_map(|k| env_path(k)).collect();
                places.program_files.dedup();
                places.system_root = env_path("SystemRoot").or_else(|| env_path("windir"));
                places.txt_handler = txt_handler();
            }
            Os::Mac => {
                places.applications = vec![PathBuf::from("/Applications"), PathBuf::from("/System/Applications")];
                if let Some(h) = &home {
                    places.applications.insert(1, h.join("Applications"));
                }
            }
            Os::Linux => {
                let data_home = env_path("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")));
                let data_dirs = std::env::var_os("XDG_DATA_DIRS").filter(|v| !v.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
                places.desktop_dirs = data_home.into_iter().chain(std::env::split_paths(&data_dirs)).filter(|d| d.is_absolute()).map(|d| d.join("applications")).collect();
                places.text_default = xdg_text_default();
                places.fs_root = Some(PathBuf::from("/"));
            }
        }
        places
    }
}

/// The editors on this computer, VS Code-like ones first, the system's default last.
pub fn installed() -> Vec<Editor> {
    find(Os::current(), &Places::here())
}

struct Known {
    id: &'static str,
    name: &'static str,
    vscode_like: bool,
    /// Linux: program names to look for in PATH, then full paths.
    linux: &'static [&'static str],
    linux_paths: &'static [&'static str],
    /// Windows: paths under %LOCALAPPDATA%, then under each Program Files.
    win_local: &'static [&'static str],
    win_programs: &'static [&'static str],
    /// macOS: app bundles in the Applications folders.
    mac: &'static [&'static str],
}

const KNOWN: &[Known] = &[
    Known {
        id: "vscode",
        name: "VS Code",
        vscode_like: true,
        linux: &["code"],
        linux_paths: &["/usr/share/code/bin/code", "/snap/bin/code"],
        win_local: &[r"Programs\Microsoft VS Code\Code.exe"],
        win_programs: &[r"Microsoft VS Code\Code.exe"],
        mac: &["Visual Studio Code.app"],
    },
    Known {
        id: "vscode-insiders",
        name: "VS Code Insiders",
        vscode_like: true,
        linux: &["code-insiders"],
        linux_paths: &["/usr/share/code-insiders/bin/code-insiders", "/snap/bin/code-insiders"],
        win_local: &[r"Programs\Microsoft VS Code Insiders\Code - Insiders.exe"],
        win_programs: &[r"Microsoft VS Code Insiders\Code - Insiders.exe"],
        mac: &["Visual Studio Code - Insiders.app"],
    },
    Known {
        id: "cursor",
        name: "Cursor",
        vscode_like: true,
        linux: &["cursor"],
        linux_paths: &["/usr/share/cursor/cursor", "/opt/Cursor/cursor"],
        win_local: &[r"Programs\cursor\Cursor.exe"],
        win_programs: &[r"Cursor\Cursor.exe"],
        mac: &["Cursor.app"],
    },
    Known {
        id: "windsurf",
        name: "Windsurf",
        vscode_like: true,
        linux: &["windsurf"],
        linux_paths: &["/usr/share/windsurf/bin/windsurf"],
        win_local: &[r"Programs\Windsurf\Windsurf.exe"],
        win_programs: &[r"Windsurf\Windsurf.exe"],
        mac: &["Windsurf.app"],
    },
    Known {
        id: "zed",
        name: "Zed",
        vscode_like: false,
        linux: &["zed", "zeditor"],
        linux_paths: &[],
        win_local: &[r"Programs\Zed\Zed.exe"],
        win_programs: &[r"Zed\Zed.exe"],
        mac: &["Zed.app"],
    },
    Known {
        id: "sublime",
        name: "Sublime Text",
        vscode_like: false,
        linux: &["subl", "sublime_text"],
        linux_paths: &["/opt/sublime_text/sublime_text"],
        win_local: &[],
        win_programs: &[r"Sublime Text\sublime_text.exe", r"Sublime Text 3\sublime_text.exe"],
        mac: &["Sublime Text.app"],
    },
    Known {
        id: "kate",
        name: "Kate",
        vscode_like: false,
        linux: &["kate"],
        linux_paths: &[],
        win_local: &[],
        win_programs: &[r"Kate\bin\kate.exe"],
        mac: &[],
    },
    Known { id: "gnome-text-editor", name: "Text Editor", vscode_like: false, linux: &["gnome-text-editor"], linux_paths: &[], win_local: &[], win_programs: &[], mac: &[] },
    Known { id: "gedit", name: "gedit", vscode_like: false, linux: &["gedit"], linux_paths: &[], win_local: &[], win_programs: &[], mac: &[] },
    Known {
        id: "notepad-plus-plus",
        name: "Notepad++",
        vscode_like: false,
        linux: &[],
        linux_paths: &[],
        win_local: &[],
        win_programs: &[r"Notepad++\notepad++.exe"],
        mac: &[],
    },
    Known { id: "bbedit", name: "BBEdit", vscode_like: false, linux: &[], linux_paths: &[], win_local: &[], win_programs: &[], mac: &["BBEdit.app"] },
    Known { id: "nova", name: "Nova", vscode_like: false, linux: &[], linux_paths: &[], win_local: &[], win_programs: &[], mac: &["Nova.app"] },
    Known { id: "textedit", name: "TextEdit", vscode_like: false, linux: &[], linux_paths: &[], win_local: &[], win_programs: &[], mac: &["TextEdit.app"] },
];

/// `rel` with Windows separators, joined under `base` on any system.
fn join_win(base: &Path, rel: &str) -> PathBuf {
    rel.split('\\').fold(base.to_path_buf(), |p, part| p.join(part))
}

/// A program that can be started: a file, executable on Unix, an .exe on Windows.
fn is_program(os: Os, p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else { return false };
    if !meta.is_file() {
        return false;
    }
    match os {
        Os::Windows => p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")),
        _ => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
    }
}

fn in_path(os: Os, places: &Places, name: &str) -> Option<PathBuf> {
    places.path.iter().map(|d| d.join(name)).find(|p| is_program(os, p))
}

/// `open -a <bundle> <file>`.
fn mac_app(id: &str, name: &str, vscode_like: bool, bundle: &Path) -> Editor {
    Editor {
        id: id.into(),
        name: name.into(),
        program: PathBuf::from("/usr/bin/open"),
        args: vec!["-a".into(), bundle.to_string_lossy().into_owned(), FILE_ARG.into()],
        vscode_like,
    }
}

fn find_known(os: Os, places: &Places, k: &Known) -> Option<Editor> {
    let editor = |program: PathBuf| Editor { id: k.id.into(), name: k.name.into(), program, args: Vec::new(), vscode_like: k.vscode_like };
    match os {
        Os::Linux => {
            let found = k.linux.iter().find_map(|n| in_path(os, places, n));
            let fixed = places.fs_root.iter().flat_map(|root| k.linux_paths.iter().map(move |p| root.join(p.trim_start_matches('/'))));
            let found = found.or_else(|| fixed.into_iter().find(|p| is_program(os, p)));
            found.map(editor)
        }
        Os::Windows => {
            let local = places.local_app_data.iter().flat_map(|base| k.win_local.iter().map(move |rel| join_win(base, rel)));
            let programs = places.program_files.iter().flat_map(|base| k.win_programs.iter().map(move |rel| join_win(base, rel)));
            local.chain(programs).find(|p| is_program(os, p)).map(editor)
        }
        Os::Mac => {
            let bundle = places.applications.iter().flat_map(|dir| k.mac.iter().map(move |b| dir.join(b))).find(|b| b.is_dir())?;
            Some(mac_app(k.id, k.name, k.vscode_like, &bundle))
        }
    }
}

/// The editors found in `places`, for `os`: the known ones in the order of the list (VS
/// Code-like first), then the system's default text editor.
pub fn find(os: Os, places: &Places) -> Vec<Editor> {
    let mut found: Vec<Editor> = KNOWN.iter().filter_map(|k| find_known(os, places, k)).collect();
    if let Some(system) = system_default(os, places) {
        found.push(system);
    }
    found
}

/// The system's default text editor.
fn system_default(os: Os, places: &Places) -> Option<Editor> {
    let system = |program: PathBuf, args: Vec<String>| Editor { id: SYSTEM.into(), name: "the default text editor".into(), program, args, vscode_like: false };
    match os {
        Os::Mac => Some(system(PathBuf::from("/usr/bin/open"), vec!["-t".into(), FILE_ARG.into()])),
        Os::Windows => {
            let notepad = places.system_root.as_ref().map(|r| r.join("System32").join("notepad.exe"));
            // A Store app's own folder can't be started directly; Notepad stands in for it.
            let handler = places.txt_handler.clone().filter(|p| is_program(os, p) && !p.to_string_lossy().to_ascii_lowercase().contains(r"\windowsapps\"));
            handler.or(notepad).filter(|p| is_program(os, p)).map(|p| system(p, Vec::new()))
        }
        Os::Linux => {
            let id = places.text_default.as_deref()?;
            let entry = places.desktop_dirs.iter().map(|d| d.join(id)).find(|p| p.is_file())?;
            let (program, args) = desktop_exec(os, places, &std::fs::read_to_string(entry).ok()?)?;
            Some(system(program, args))
        }
    }
}

/// The program and arguments of a desktop entry's `Exec`, with `%f`/`%F`/`%u`/`%U` as the file.
/// None for one the file can't safely go into: a field code inside a longer argument (`sh -c
/// "edit %f"`) would put the file's name into text another program reads again.
fn desktop_exec(os: Os, places: &Places, entry: &str) -> Option<(PathBuf, Vec<String>)> {
    let mut in_entry = false;
    let mut exec = None;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(value) = line.strip_prefix("Exec=") {
                exec = Some(value.to_string());
                break;
            }
        }
    }
    let words = split_exec(&exec?)?;
    let mut args = Vec::new();
    let mut placed = false;
    for word in words.iter().skip(1) {
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" if !placed => {
                args.push(FILE_ARG.to_string());
                placed = true;
            }
            "%f" | "%F" | "%u" | "%U" | "%i" | "%c" | "%k" => {}
            w if w.contains('%') && w.replace("%%", "").contains('%') => return None,
            w => args.push(w.replace("%%", "%")),
        }
    }
    if !placed {
        args.push(FILE_ARG.to_string());
    }
    let first = PathBuf::from(words.first()?);
    let program = if first.is_absolute() { Some(first).filter(|p| is_program(os, p)) } else { in_path(os, places, &first.to_string_lossy()) }?;
    // A shell would read the arguments again.
    let base = program.file_name()?.to_string_lossy().into_owned();
    if matches!(base.as_str(), "sh" | "bash" | "dash" | "zsh" | "env") {
        return None;
    }
    Some((program, args))
}

/// Splits an `Exec` value into words: spaces separate, double quotes group, a backslash
/// escapes the next character inside quotes.
fn split_exec(exec: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = exec.chars();
    let mut started = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => word.push(chars.next()?),
            ' ' | '\t' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

/// `xdg-mime query default text/plain`: run by its argument list, no shell.
fn xdg_text_default() -> Option<String> {
    let out = std::process::Command::new("xdg-mime").args(["query", "default", "text/plain"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    let id = String::from_utf8(out.stdout).ok()?.trim().to_string();
    // One desktop file's name, nothing that walks out of the applications folder.
    (id.ends_with(".desktop") && !id.contains(['/', '\\']) && !id.starts_with('.')).then_some(id)
}

/// The program Windows opens .txt files with.
#[cfg(windows)]
fn txt_handler() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::{w, PWSTR};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE};
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` u16s and outlives the call.
    unsafe { AssocQueryStringW(ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, w!(".txt"), w!("open"), Some(PWSTR(buf.as_mut_ptr())), &mut len) }.ok().ok()?;
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..end]))).filter(|p| p.is_absolute())
}

#[cfg(not(windows))]
fn txt_handler() -> Option<PathBuf> {
    None
}

/// "Choose an app…": an editor from a program you picked. On macOS an app bundle, started
/// with `open -a`; on Windows an .exe (never a .cmd or .bat); on Linux a program or a desktop
/// entry.
pub fn choose_app(os: Os, places: &Places, picked: &Path) -> Result<Editor, String> {
    if !picked.is_absolute() {
        return Err("Pick the editor's program.".into());
    }
    let name = picked.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "your editor".into());
    let id = format!("{CUSTOM_PREFIX}{}", picked.display());
    let custom = |program: PathBuf, args: Vec<String>| Editor { id: id.clone(), name: name.clone(), program, args, vscode_like: false };
    let ext = picked.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match os {
        Os::Mac if ext == "app" && picked.is_dir() => Ok(Editor { id: id.clone(), ..mac_app("", &name, false, picked) }),
        Os::Windows if ext != "exe" => Err("Pick the editor's program (an .exe file). A .cmd or .bat file can't be started safely.".into()),
        Os::Linux if ext == "desktop" => {
            let entry = std::fs::read_to_string(picked).map_err(|e| e.to_string())?;
            let (program, args) = desktop_exec(os, places, &entry).ok_or("This app can't be started safely with a file.")?;
            Ok(custom(program, args))
        }
        _ if is_program(os, picked) => Ok(custom(picked.to_path_buf(), Vec::new())),
        _ => Err("That isn't a program that can be started.".into()),
    }
}

/// The editor "Open in" uses when nothing is chosen: the first VS Code-like one, else the
/// system's default, else the first found.
pub fn default_id(found: &[Editor]) -> Option<&str> {
    found.iter().find(|e| e.vscode_like).or_else(|| found.iter().find(|e| e.id == SYSTEM)).or_else(|| found.first()).map(|e| e.id.as_str())
}

/// The editor for the setting `externalEditor` (`setting`), among `found`: the one it names,
/// or for `custom:<path>` the program picked; the default when it's unset or names one that
/// isn't there anymore.
pub fn chosen(os: Os, places: &Places, setting: Option<&str>, found: &[Editor]) -> Option<Editor> {
    let fallback = || default_id(found).and_then(|id| found.iter().find(|e| e.id == id)).cloned();
    match setting {
        Some(s) if s.starts_with(CUSTOM_PREFIX) => choose_app(os, places, Path::new(&s[CUSTOM_PREFIX.len()..])).ok().or_else(fallback),
        Some(id) => found.iter().find(|e| e.id == id).cloned().or_else(fallback),
        None => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(what: &str) -> Scratch {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("sshee-editors-{what}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// A file at `rel`, executable on Unix.
        fn program(&self, rel: &str) -> PathBuf {
            let p = rel.split(['/', '\\']).fold(self.0.clone(), |p, part| p.join(part));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            p
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ids(found: &[Editor]) -> Vec<&str> {
        found.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn linux_finds_editors_in_path_and_the_text_default_from_its_desktop_entry() {
        let s = Scratch::new("linux");
        let bin = s.0.join("bin");
        s.program("bin/zeditor");
        s.program("bin/code");
        s.program("bin/kate");
        s.program("bin/gnome-text-editor");
        s.program("opt/sublime_text/sublime_text");
        // Not executable: not a program.
        std::fs::write(bin.join("subl"), b"").unwrap();
        let apps = s.0.join("share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("org.gnome.TextEditor.desktop"),
            "[Desktop Entry]\nName=Text Editor\nExec=gnome-text-editor --new-window %U\n[Desktop Action new]\nExec=gnome-text-editor --other\n",
        )
        .unwrap();
        let places = Places {
            path: vec![bin.clone()],
            desktop_dirs: vec![apps],
            text_default: Some("org.gnome.TextEditor.desktop".into()),
            fs_root: Some(s.0.clone()),
            ..Places::default()
        };
        let found = find(Os::Linux, &places);
        assert_eq!(ids(&found), vec!["vscode", "zed", "sublime", "kate", "gnome-text-editor", SYSTEM]);
        assert_eq!(found[2].program, s.0.join("opt/sublime_text/sublime_text"));
        assert_eq!(found[0].program, bin.join("code"));
        assert_eq!(found[1].program, bin.join("zeditor"));
        let system = found.last().unwrap();
        assert_eq!(system.program, bin.join("gnome-text-editor"));
        assert_eq!(system.args, vec!["--new-window", FILE_ARG]);
        assert_eq!(default_id(&found), Some("vscode"));
    }

    #[test]
    fn a_desktop_entry_that_would_hand_the_name_to_a_shell_is_refused() {
        let s = Scratch::new("exec");
        let bin = s.0.join("bin");
        s.program("bin/sh");
        s.program("bin/edit");
        let places = Places { path: vec![bin.clone()], ..Places::default() };
        let entry = |exec: &str| format!("[Desktop Entry]\nExec={exec}\n");
        assert!(desktop_exec(Os::Linux, &places, &entry(r#"sh -c "edit %f""#)).is_none());
        assert!(desktop_exec(Os::Linux, &places, &entry("edit --file=%f")).is_none());
        assert!(desktop_exec(Os::Linux, &places, &entry("sh %f")).is_none());
        assert!(desktop_exec(Os::Linux, &places, &entry(r#"edit "unclosed"#)).is_none());
        assert!(desktop_exec(Os::Linux, &places, &entry("missing-program %f")).is_none());
        let (program, args) = desktop_exec(Os::Linux, &places, &entry(r#""edit" --title "100%% sure" %F %i"#)).unwrap();
        assert_eq!(program, bin.join("edit"));
        assert_eq!(args, vec!["--title", "100% sure", FILE_ARG]);
    }

    #[test]
    fn windows_finds_exe_programs_never_cmd_shims() {
        let s = Scratch::new("windows");
        let local = s.0.join("Local");
        let programs = s.0.join("Program Files");
        // VS Code's user install puts code.cmd in PATH; only Code.exe counts.
        s.program(r"Local\Programs\Microsoft VS Code\bin\code.cmd");
        s.program(r"Local\Programs\Microsoft VS Code\Code.exe");
        s.program(r"Program Files\Notepad++\notepad++.exe");
        s.program(r"Program Files\Sublime Text\sublime_text.exe");
        s.program(r"Windows\System32\notepad.exe");
        let places = Places {
            path: vec![local.join("Programs").join("Microsoft VS Code").join("bin")],
            local_app_data: Some(local.clone()),
            program_files: vec![programs.clone()],
            system_root: Some(s.0.join("Windows")),
            txt_handler: Some(programs.join("Notepad++").join("notepad++.exe")),
            ..Places::default()
        };
        let found = find(Os::Windows, &places);
        assert_eq!(ids(&found), vec!["vscode", "sublime", "notepad-plus-plus", SYSTEM]);
        assert_eq!(found[0].program, local.join("Programs").join("Microsoft VS Code").join("Code.exe"));
        assert_eq!(found.last().unwrap().program, programs.join("Notepad++").join("notepad++.exe"));
        // A Store app's handler falls back to Notepad.
        let store = Places { txt_handler: Some(PathBuf::from(r"C:\Program Files\WindowsApps\Notepad\Notepad.exe")), ..places.clone() };
        assert_eq!(find(Os::Windows, &store).last().unwrap().program, s.0.join("Windows").join("System32").join("notepad.exe"));
        let shim = local.join("Programs").join("Microsoft VS Code").join("bin").join("code.cmd");
        assert!(choose_app(Os::Windows, &places, &shim).is_err());
    }

    #[test]
    fn macos_starts_app_bundles_with_open_and_the_default_with_open_t() {
        let s = Scratch::new("mac");
        let apps = s.0.join("Applications");
        std::fs::create_dir_all(apps.join("Cursor.app").join("Contents")).unwrap();
        std::fs::create_dir_all(apps.join("BBEdit.app")).unwrap();
        let places = Places { applications: vec![apps.clone()], ..Places::default() };
        let found = find(Os::Mac, &places);
        assert_eq!(ids(&found), vec!["cursor", "bbedit", SYSTEM]);
        assert_eq!(found[0].program, PathBuf::from("/usr/bin/open"));
        assert_eq!(found[0].args, vec!["-a".to_string(), apps.join("Cursor.app").to_string_lossy().into_owned(), FILE_ARG.into()]);
        assert_eq!(found[2].args, vec!["-t", FILE_ARG]);
        assert_eq!(default_id(&found), Some("cursor"));
        let picked = choose_app(Os::Mac, &places, &apps.join("BBEdit.app")).unwrap();
        assert_eq!(picked.id, format!("{CUSTOM_PREFIX}{}", apps.join("BBEdit.app").display()));
        assert_eq!(picked.args[0], "-a");
    }

    #[test]
    fn the_setting_picks_an_editor_and_falls_back_to_the_default() {
        let s = Scratch::new("setting");
        let bin = s.0.join("bin");
        let mine = s.program("bin/my-editor");
        s.program("bin/zed");
        s.program("bin/cursor");
        let places = Places { path: vec![bin.clone()], ..Places::default() };
        let found = find(Os::Linux, &places);
        assert_eq!(chosen(Os::Linux, &places, None, &found).unwrap().id, "cursor");
        assert_eq!(chosen(Os::Linux, &places, Some("zed"), &found).unwrap().id, "zed");
        assert_eq!(chosen(Os::Linux, &places, Some("sublime"), &found).unwrap().id, "cursor", "not installed anymore");
        let custom = format!("{CUSTOM_PREFIX}{}", mine.display());
        let picked = chosen(Os::Linux, &places, Some(&custom), &found).unwrap();
        assert_eq!(picked.program, mine);
        assert_eq!(picked.id, custom);
        let gone = format!("{CUSTOM_PREFIX}{}", bin.join("gone").display());
        assert_eq!(chosen(Os::Linux, &places, Some(&gone), &found).unwrap().id, "cursor");
        // Nothing VS Code-like: the system's default, else the first.
        let no_vscode = Places { path: vec![], text_default: None, ..Places::default() };
        assert!(chosen(Os::Linux, &no_vscode, None, &[]).is_none());
        let only_zed: Vec<Editor> = found.iter().filter(|e| e.id == "zed").cloned().collect();
        assert_eq!(default_id(&only_zed), Some("zed"));
        assert!(choose_app(Os::Linux, &places, Path::new("relative/editor")).is_err());
    }
}
