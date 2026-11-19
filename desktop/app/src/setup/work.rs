//! The installer's and the uninstaller's work, step by step, over a [`System`]: Windows'
//! registry, Start menu and folders (`win.rs`), or a made-up one in the tests.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use super::files::{self, Kept, Layout};
use super::flags::{self, Install};
use super::names::{self, Found, Value, MAIN_EXE, NSIS_UNINSTALLER, PREVIOUS_SHORTCUT, PRODUCT_KEY, REGISTRY_NOTE, RUN_ONCE_KEY, RUN_ONCE_VALUE, SHORTCUT, UNINSTALL_KEY};

/// What the installer needs of the system.
pub trait System {
    /// A text value under `HKCU\<key>` (`""`: the key's default value).
    fn read(&self, key: &str, name: &str) -> Option<String>;
    fn write(&self, key: &str, values: &[(&str, Value)]) -> Result<(), String>;
    fn remove_value(&self, key: &str, name: &str);
    fn remove_key(&self, key: &str);
    /// `%LOCALAPPDATA%`.
    fn local_app_data(&self) -> PathBuf;
    /// The Start menu's Programs folder (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`).
    fn programs_menu(&self) -> PathBuf;
    /// The desktop, where 0.10's installer may have put a shortcut.
    fn desktop(&self) -> Option<PathBuf>;
    /// A shortcut at `link` to `target`, carrying `app_id` as its AppUserModelID.
    fn shortcut(&self, link: &Path, target: &Path, app_id: Option<&str>) -> Result<(), String>;
}

/// Where the work is, for the window's line (drawn on Windows only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Copying,
    Keeping,
    Registering,
    Shortcuts,
    Starting,
    Removing,
}

impl Step {
    pub fn words(self) -> &'static str {
        match self {
            Step::Copying => "Copying SSHeesh…",
            Step::Keeping => "Keeping the version you have, to go back to…",
            Step::Registering => "Adding SSHeesh to Installed apps…",
            Step::Shortcuts => "Adding SSHeesh to the Start menu…",
            Step::Starting => "Opening SSHeesh…",
            Step::Removing => "Removing SSHeesh…",
        }
    }

    /// How far along the bar is when this step starts, 0 to 1.
    pub fn at(self) -> f32 {
        match self {
            Step::Copying => 0.1,
            Step::Keeping => 0.45,
            Step::Registering => 0.6,
            Step::Shortcuts => 0.75,
            Step::Starting => 0.9,
            Step::Removing => 0.3,
        }
    }
}

/// What the installer found before doing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub dir: PathBuf,
    pub found: Found,
    /// The version installed there, as "Installed apps" lists it.
    pub installed: Option<String>,
}

impl Plan {
    pub fn is_update(&self) -> bool {
        self.found != Found::Nothing
    }
}

/// Where to install: `/D=` when given, else where SSHeesh already is, else
/// `%LOCALAPPDATA%\SSHeesh`.
pub fn plan(sys: &dyn System, asked: Option<&Path>) -> Plan {
    let installed = sys.read(UNINSTALL_KEY, "DisplayVersion").filter(|v| !v.trim().is_empty());
    let has_program = |dir: &Path| dir.join(MAIN_EXE).is_file();
    if let Some(dir) = asked {
        let found = if has_program(dir) { Found::ProductKey } else { Found::Nothing };
        return Plan { dir: dir.to_path_buf(), found, installed: installed.filter(|_| found != Found::Nothing) };
    }
    let (dir, found) = names::install_dir(sys.read(PRODUCT_KEY, "").as_deref(), sys.read(UNINSTALL_KEY, "InstallLocation").as_deref(), &sys.local_app_data(), has_program);
    Plan { dir, found, installed: installed.filter(|_| found != Found::Nothing) }
}

/// What an install left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub layout: Layout,
    pub kept: Kept,
}

/// The `RunOnce` command that finishes the replacement after a power loss.
pub fn run_once_command(layout: &Layout) -> String {
    // Backslashes right before a closing quote are doubled, as Windows reads a command line
    // (`CommandLineToArgvW`): a folder like `C:\Apps\` or a drive's root `C:\` stays itself
    // instead of escaping the quote.
    let dir = layout.dir.display().to_string();
    let trailing = dir.len() - dir.trim_end_matches('\\').len();
    format!("\"{}\" {} \"{}{}\"", layout.new.display(), flags::FINISH, dir, "\\".repeat(trailing))
}

/// Installs `program` (this installer's own file) of `version` as `plan` says.
pub fn install(sys: &dyn System, plan: &Plan, how: &Install, program: &Path, version: &str, step: &mut dyn FnMut(Step)) -> Result<Done, String> {
    let layout = Layout::of(&plan.dir);
    step(Step::Copying);
    files::stage(&layout, program).map_err(|e| format!("SSHeesh couldn't be copied to {}: {e}", plan.dir.display()))?;
    sys.write(RUN_ONCE_KEY, &[(RUN_ONCE_VALUE, Value::Text(run_once_command(&layout)))])?;
    step(Step::Keeping);
    let before = registry_note(sys);
    let kept = files::keep_previous(&layout, plan.installed.as_deref()).map_err(|e| format!("the version you have couldn't be moved aside: {e}"))?;
    if kept == Kept::Previous {
        let _ = std::fs::write(layout.previous.join(REGISTRY_NOTE), before);
    }
    files::take_name(&layout).map_err(|e| format!("SSHeesh couldn't take its name: {e}"))?;
    sys.remove_value(RUN_ONCE_KEY, RUN_ONCE_VALUE);
    step(Step::Registering);
    register(sys, &layout, version)?;
    step(Step::Shortcuts);
    shortcuts(sys, &layout, how, kept == Kept::Previous)?;
    Ok(Done { layout, kept })
}

/// The registry entries as they were, for `previous\` (and for a person who wants to see).
fn registry_note(sys: &dyn System) -> String {
    let mut note = format!("[HKCU\\{PRODUCT_KEY}]\n@={}\n\n[HKCU\\{UNINSTALL_KEY}]\n", sys.read(PRODUCT_KEY, "").unwrap_or_default());
    for name in ["DisplayName", "DisplayVersion", "DisplayIcon", "InstallLocation", "UninstallString", "QuietUninstallString", "Publisher"] {
        if let Some(v) = sys.read(UNINSTALL_KEY, name) {
            note.push_str(&format!("{name}={v}\n"));
        }
    }
    note
}

/// The product key and "Installed apps"' entry, as NSIS writes them.
pub fn register(sys: &dyn System, layout: &Layout, version: &str) -> Result<(), String> {
    let folder = layout.dir.to_string_lossy().trim_end_matches(['\\', '/']).to_owned();
    sys.write(PRODUCT_KEY, &[("", Value::Text(folder))])?;
    let (uninstall, quiet) = names::uninstallers(&layout.dir, layout.dir.join(NSIS_UNINSTALLER).is_file());
    sys.write(UNINSTALL_KEY, &names::uninstall_values(&layout.dir, version, files::size_kb(layout), &uninstall, &quiet))
}

/// The Start menu item: made by a fresh install; on an update only renewed where it still is
/// (as NSIS's `/UPDATE`: one the person removed stays removed). And "SSHeesh (previous
/// version)" when the update kept one.
fn shortcuts(sys: &dyn System, layout: &Layout, how: &Install, kept_previous: bool) -> Result<(), String> {
    let menu = sys.programs_menu();
    let main = menu.join(SHORTCUT);
    if !how.no_shortcuts && (!how.update || main.exists()) {
        sys.shortcut(&main, &layout.exe, Some(names::APP_ID))?;
    }
    if kept_previous {
        // Not under SSHeesh's own id: it's another program, and mustn't take 1.0's place on the
        // taskbar.
        sys.shortcut(&menu.join(PREVIOUS_SHORTCUT), &layout.previous_exe(), None)?;
    }
    Ok(())
}

/// Removes `previous\` and its Start menu item once they're 30 days old (run by the app at
/// its start on Windows).
pub fn tidy_previous(sys: &dyn System, dir: &Path, now: std::time::SystemTime) {
    let layout = Layout::of(dir);
    let _ = std::fs::remove_file(&layout.old);
    if files::previous_due(&layout, now) && files::remove_previous(&layout).is_ok() {
        let _ = std::fs::remove_file(sys.programs_menu().join(PREVIOUS_SHORTCUT));
    }
}

/// Uninstalls from `dir`: shortcuts, the registry entries, the program's files, and with
/// `data` the folders the app keeps its own things in. What couldn't go is said.
pub fn uninstall(sys: &dyn System, dir: &Path, data: &[PathBuf], step: &mut dyn FnMut(Step)) -> Result<(), String> {
    step(Step::Removing);
    let layout = Layout::of(dir);
    let menu = sys.programs_menu();
    for link in [menu.join(SHORTCUT), menu.join(PREVIOUS_SHORTCUT)].into_iter().chain(sys.desktop().map(|d| d.join(SHORTCUT))) {
        let _ = std::fs::remove_file(link);
    }
    let mut left = Vec::new();
    for file in [&layout.exe, &layout.new, &layout.old, &dir.join(NSIS_UNINSTALLER)] {
        match std::fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => left.push(format!("{}: {e}", file.display())),
            _ => {}
        }
    }
    if let Err(e) = files::remove_previous(&layout) {
        left.push(format!("{}: {e}", layout.previous.display()));
    }
    let _ = std::fs::remove_dir(dir);
    if left.is_empty() {
        sys.remove_key(UNINSTALL_KEY);
        sys.remove_key(PRODUCT_KEY);
        sys.remove_value(RUN_ONCE_KEY, RUN_ONCE_VALUE);
    }
    for d in data {
        match std::fs::remove_dir_all(d) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => left.push(format!("{}: {e}", d.display())),
            _ => {}
        }
    }
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!("Some files couldn't be removed (is SSHeesh still open?): {}", left.join("; ")))
    }
}

/// How the app's first start after an install went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Its window said it's ready (Rune's `RUNE_READY_DIR` note), or it handed the start to a
    /// copy already open and ended well.
    Ready,
    /// Still starting when the wait ran out, but alive: left to it.
    Slow,
    /// It ended before its window was ready.
    Ended(Option<i32>),
}

/// Waits for `child`, started with `RUNE_READY_DIR=ready`, to be ready: any file appearing
/// there (Rune names it after the program and removes it at a normal exit). No fixed short
/// limit: a first start under a virus scan or over RDP is slow, so a program that's alive is
/// waited for up to `most`.
pub fn wait_ready(child: &mut Child, ready: &Path, most: Duration) -> Start {
    let until = Instant::now() + most;
    loop {
        if std::fs::read_dir(ready).is_ok_and(|mut d| d.next().is_some()) {
            return Start::Ready;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Start::Ready,
            Ok(Some(status)) => return Start::Ended(status.code()),
            Ok(None) => {}
            Err(_) => return Start::Slow,
        }
        if Instant::now() >= until {
            return Start::Slow;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use crate::setup::flags::Ui;

    /// A made-up Windows: a registry in memory, and folders under a temporary one.
    pub struct Fake {
        pub root: PathBuf,
        pub registry: RefCell<BTreeMap<(String, String), Value>>,
        pub links: RefCell<BTreeMap<PathBuf, (PathBuf, Option<String>)>>,
    }

    impl Fake {
        pub fn new(root: &Path) -> Fake {
            Fake { root: root.to_path_buf(), registry: RefCell::default(), links: RefCell::default() }
        }

        fn get(&self, key: &str, name: &str) -> Option<Value> {
            self.registry.borrow().get(&(key.to_owned(), name.to_owned())).cloned()
        }
    }

    impl System for Fake {
        fn read(&self, key: &str, name: &str) -> Option<String> {
            match self.get(key, name)? {
                Value::Text(t) => Some(t),
                Value::Number(_) => None,
            }
        }
        fn write(&self, key: &str, values: &[(&str, Value)]) -> Result<(), String> {
            for (name, v) in values {
                self.registry.borrow_mut().insert((key.to_owned(), (*name).to_owned()), v.clone());
            }
            Ok(())
        }
        fn remove_value(&self, key: &str, name: &str) {
            self.registry.borrow_mut().remove(&(key.to_owned(), name.to_owned()));
        }
        fn remove_key(&self, key: &str) {
            self.registry.borrow_mut().retain(|(k, _), _| k != key);
        }
        fn local_app_data(&self) -> PathBuf {
            self.root.join("Local")
        }
        fn programs_menu(&self) -> PathBuf {
            self.root.join("Programs")
        }
        fn desktop(&self) -> Option<PathBuf> {
            Some(self.root.join("Desktop"))
        }
        fn shortcut(&self, link: &Path, target: &Path, app_id: Option<&str>) -> Result<(), String> {
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::fs::write(link, b"lnk").unwrap();
            self.links.borrow_mut().insert(link.to_path_buf(), (target.to_path_buf(), app_id.map(str::to_owned)));
            Ok(())
        }
    }

    fn update_flags() -> Install {
        flags::install_flags(&["/P", "/UPDATE", "/R", "/ARGS"].map(std::ffi::OsString::from)).unwrap()
    }

    /// 0.10.14 as Tauri's NSIS installer left it, in a folder of the person's choosing.
    fn nsis_0_10(fake: &Fake, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MAIN_EXE), b"0.10.14").unwrap();
        std::fs::write(dir.join(NSIS_UNINSTALLER), b"nsis").unwrap();
        let folder = dir.to_string_lossy().into_owned();
        fake.write(PRODUCT_KEY, &[("", Value::Text(folder.clone()))]).unwrap();
        let values = names::uninstall_values(dir, "0.10.14", 30_000, &format!("\"{folder}/uninstall.exe\""), "");
        fake.write(UNINSTALL_KEY, &values).unwrap();
        fake.shortcut(&fake.programs_menu().join(SHORTCUT), &dir.join(MAIN_EXE), Some(names::APP_ID)).unwrap();
    }

    #[test]
    fn the_recovery_command_keeps_a_folder_ending_in_a_backslash_whole() {
        // Linux paths stand in for Windows ones: only the text of the command is checked.
        for (dir, ends) in [("C:\\Apps\\", "\"C:\\Apps\\\\\""), ("C:\\", "\"C:\\\\\""), ("C:\\Apps", "\"C:\\Apps\"")] {
            let command = run_once_command(&Layout::of(Path::new(dir)));
            assert!(command.ends_with(ends), "{dir}: {command}");
        }
    }

    #[test]
    fn the_update_from_0_10_takes_its_place_where_the_person_put_it() {
        let root = tempfile::tempdir().unwrap();
        let fake = Fake::new(root.path());
        let dir = root.path().join("Apps").join("SSHeesh");
        nsis_0_10(&fake, &dir);
        let installer = root.path().join("Temp").join("SSHeesh-1.0.0-installer.exe");
        std::fs::create_dir_all(installer.parent().unwrap()).unwrap();
        std::fs::write(&installer, b"1.0.0").unwrap();

        let plan = plan(&fake, None);
        assert_eq!((plan.dir.as_path(), plan.found, plan.installed.as_deref()), (dir.as_path(), Found::ProductKey, Some("0.10.14")));
        let mut steps = Vec::new();
        let done = install(&fake, &plan, &update_flags(), &installer, "1.0.0", &mut |s| steps.push(s)).unwrap();
        assert_eq!(steps, [Step::Copying, Step::Keeping, Step::Registering, Step::Shortcuts]);
        assert_eq!(done.kept, Kept::Previous);
        assert_eq!(std::fs::read(dir.join(MAIN_EXE)).unwrap(), b"1.0.0");
        assert_eq!(std::fs::read(dir.join("previous").join(MAIN_EXE)).unwrap(), b"0.10.14");
        // "Installed apps" says 1.0.0, still with 0.10's uninstaller.
        assert_eq!(fake.read(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some("1.0.0"));
        assert!(fake.read(UNINSTALL_KEY, "UninstallString").unwrap().ends_with("uninstall.exe\""));
        assert_eq!(fake.read(RUN_ONCE_KEY, RUN_ONCE_VALUE), None, "the RunOnce entry is gone once done");
        let note = std::fs::read_to_string(dir.join("previous").join(REGISTRY_NOTE)).unwrap();
        assert!(note.contains("DisplayVersion=0.10.14"), "{note}");
        // The Start menu item leads to 1.0, with SSHeesh's id; the previous version has its own.
        let links = fake.links.borrow();
        assert_eq!(links[&fake.programs_menu().join(SHORTCUT)], (dir.join(MAIN_EXE), Some(names::APP_ID.to_owned())));
        assert_eq!(links[&fake.programs_menu().join(PREVIOUS_SHORTCUT)], (dir.join("previous").join(MAIN_EXE), None));
    }

    #[test]
    fn an_update_leaves_a_removed_shortcut_removed() {
        let root = tempfile::tempdir().unwrap();
        let fake = Fake::new(root.path());
        let dir = fake.local_app_data().join("SSHeesh");
        nsis_0_10(&fake, &dir);
        std::fs::remove_file(fake.programs_menu().join(SHORTCUT)).unwrap();
        fake.links.borrow_mut().clear();
        let installer = root.path().join("setup.exe");
        std::fs::write(&installer, b"1.0.0").unwrap();
        install(&fake, &plan(&fake, None), &update_flags(), &installer, "1.0.0", &mut |_| {}).unwrap();
        assert!(!fake.programs_menu().join(SHORTCUT).exists());
        assert!(fake.programs_menu().join(PREVIOUS_SHORTCUT).exists());
    }

    #[test]
    fn a_fresh_install_then_uninstall_leaves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let fake = Fake::new(root.path());
        let installer = root.path().join("SSHeesh_1.0.0_x64-setup.exe");
        std::fs::write(&installer, b"1.0.0").unwrap();
        let plan = plan(&fake, None);
        assert_eq!((plan.found, plan.dir.clone()), (Found::Nothing, fake.local_app_data().join("SSHeesh")));
        let how = Install { ui: Ui::Full, ..update_flags() };
        let how = Install { update: false, ..how };
        let done = install(&fake, &plan, &how, &installer, "1.0.0", &mut |_| {}).unwrap();
        assert_eq!(done.kept, Kept::Nothing);
        let exe = plan.dir.join(MAIN_EXE);
        assert_eq!(fake.read(UNINSTALL_KEY, "UninstallString").unwrap(), format!("\"{}\\ssheesh.exe\" --uninstall", plan.dir.display()));
        assert!(fake.programs_menu().join(SHORTCUT).exists());
        assert!(!fake.programs_menu().join(PREVIOUS_SHORTCUT).exists());
        assert!(exe.exists());
        // Found again by the next installer, as an update.
        assert!(super::plan(&fake, None).is_update());

        let data = root.path().join(".sshee");
        std::fs::create_dir_all(&data).unwrap();
        uninstall(&fake, &plan.dir, &[], &mut |_| {}).unwrap();
        assert!(!plan.dir.exists() && !fake.programs_menu().join(SHORTCUT).exists());
        assert!(fake.registry.borrow().is_empty(), "{:?}", fake.registry.borrow());
        assert!(data.exists(), "the person's things stay unless asked");
        uninstall(&fake, &plan.dir, std::slice::from_ref(&data), &mut |_| {}).unwrap();
        assert!(!data.exists());
    }

    #[test]
    fn the_folder_asked_for_wins() {
        let root = tempfile::tempdir().unwrap();
        let fake = Fake::new(root.path());
        nsis_0_10(&fake, &fake.local_app_data().join("SSHeesh"));
        let asked = root.path().join("Elsewhere");
        let p = plan(&fake, Some(&asked));
        assert_eq!((p.dir, p.found, p.installed), (asked, Found::Nothing, None));
    }

    #[test]
    fn previous_goes_after_30_days_with_its_shortcut() {
        let root = tempfile::tempdir().unwrap();
        let fake = Fake::new(root.path());
        let dir = fake.local_app_data().join("SSHeesh");
        nsis_0_10(&fake, &dir);
        let installer = root.path().join("setup.exe");
        std::fs::write(&installer, b"1.0.0").unwrap();
        install(&fake, &plan(&fake, None), &update_flags(), &installer, "1.0.0", &mut |_| {}).unwrap();
        let since = files::kept_since(&Layout::of(&dir)).unwrap();
        tidy_previous(&fake, &dir, since + Duration::from_secs(3600));
        assert!(dir.join("previous").exists());
        tidy_previous(&fake, &dir, since + Duration::from_secs(31 * 24 * 3600));
        assert!(!dir.join("previous").exists() && !fake.programs_menu().join(PREVIOUS_SHORTCUT).exists());
        assert!(fake.programs_menu().join(SHORTCUT).exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_first_start_is_waited_for_by_its_ready_note() {
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("ready");
        std::fs::create_dir_all(&ready).unwrap();
        let note = ready.join("ssheesh.exe.1");
        let mut child = std::process::Command::new("sh").arg("-c").arg(format!("sleep 0.3; touch '{}'; sleep 5", note.display())).spawn().unwrap();
        assert_eq!(wait_ready(&mut child, &ready, Duration::from_secs(10)), Start::Ready);
        let _ = child.kill();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let mut child = std::process::Command::new("sh").arg("-c").arg("exit 3").spawn().unwrap();
        assert_eq!(wait_ready(&mut child, &empty, Duration::from_secs(10)), Start::Ended(Some(3)));
        let mut child = std::process::Command::new("sh").arg("-c").arg("sleep 5").spawn().unwrap();
        assert_eq!(wait_ready(&mut child, &empty, Duration::from_millis(300)), Start::Slow);
        let _ = child.kill();
    }
}
