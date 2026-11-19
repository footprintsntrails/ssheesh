//! Windows: the one program is also its own installer, the update from 0.10, and its
//! uninstaller (plan §4, §6.2; Phase 7 item 2). `SSHeesh_1.0.0_x64-setup.exe` is this same
//! program under its installer's name:
//!
//! - **0.10's update** downloads it to `%TEMP%` and starts it with `/P /UPDATE /R /ARGS …`, then
//!   exits. It finds 0.10's install folder as NSIS would, keeps 0.10 in `previous\` with a
//!   "SSHeesh (previous version)" Start menu item for 30 days, puts 1.0 in its place (a power
//!   loss half way is finished by a `RunOnce` entry), updates "Installed apps", and starts 1.0,
//!   waiting for its window (`work.rs`, `files.rs`).
//! - **Double-clicked**, it asks once ("Install" or "Update") in a small window in the app's
//!   look (`sshee_ui::setup`), shows its progress, and opens the app.
//! - **1.x updating an installed copy** starts the new build as this installer, the same way.
//! - **"Uninstall"** in Windows' "Installed apps" runs `ssheesh.exe --uninstall`: it copies
//!   itself to the temporary folder and removes the program from there, the person's settings
//!   only when asked.
//!
//! The roles are chosen in `main` right after the ssh helpers', before anything of the app.
//! Not here yet (plan §6.2): the machine check and trial frame before replacing, the "holding"
//! launcher, writing "update manually" into 0.10's storage, and handing "Installed apps" to
//! this uninstaller once 1.0 has proved healthy (until then it stays 0.10's NSIS uninstaller,
//! which removes 1.0 just as well).

// The steps are Windows', and tested everywhere: elsewhere only the tests use them.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod files;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod flags;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod names;
#[cfg(windows)]
mod win;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod work;

use std::ffi::OsString;

/// The process an installer waits for before it starts the app: the 1.x that started it.
pub const WAIT_PID: &str = "SSHEESH_SETUP_WAIT_PID";

/// `--setup-demo <phase>`: the installer's window in one of its phases, doing nothing (for
/// looking at it, and Rune's `--snap`). On any system.
pub const DEMO: &str = "--setup-demo";

/// When this start is one of the setup roles: does its work and says how it ended. `None`
/// for the app.
pub fn main() -> Option<i32> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == DEMO) {
        let Some(phase) = args.get(1).and_then(|a| a.to_str()).and_then(sshee_ui::setup::demo) else {
            eprintln!("ssheesh: {DEMO} ask|update|working|done|failed|uninstall");
            return Some(2);
        };
        sshee_ui::setup::run(phase, Box::new(|_, _| {}));
        return Some(0);
    }
    #[cfg(windows)]
    {
        windows_roles::main(&args)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// At the app's start on Windows: once 30 days have passed, `previous\` and its Start menu
/// item go. In the background; nothing when this copy isn't the installed one.
pub fn app_started() {
    #[cfg(windows)]
    {
        let _ = std::thread::Builder::new().name("setup-tidy".into()).spawn(|| {
            if let Some(dir) = installed_dir() {
                work::tidy_previous(&win::Win, &dir, std::time::SystemTime::now());
            }
        });
    }
}

/// The install folder, when this program is the installed `ssheesh.exe` there (not a portable
/// copy, not a test build).
#[cfg(windows)]
pub fn installed_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let plan = work::plan(&win::Win, None);
    let same = |a: &std::path::Path, b: &std::path::Path| a.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'));
    (plan.is_update() && same(&plan.dir, dir)).then(|| plan.dir)
}

#[cfg(not(windows))]
pub fn installed_dir() -> Option<std::path::PathBuf> {
    None
}

/// Starts `program` (a checked new build) as the installer of an update of this installed
/// copy, as 0.10's updater does: `/P /UPDATE /R /ARGS <this start's arguments>`. The caller
/// closes the window; the installer waits for nothing but its own mutex.
pub fn start_update(program: &std::path::Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(["/P", "/UPDATE", "/R", "/ARGS"]).args(std::env::args_os().skip(1));
    // The new build must not start while this one still runs: it would hand its start to
    // this closing copy (one copy at a time) and end.
    cmd.env(WAIT_PID, std::process::id().to_string());
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map(|_| ()).map_err(|e| format!("the installer of the new build didn't start: {e}"))
}

#[cfg(windows)]
mod windows_roles {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use sshee_ui::setup::{Phase, Progress};

    use super::files::{Kept, Layout};
    use super::flags::{self, Install, Role, Ui};
    use super::names::{self, Found, RUN_ONCE_KEY, RUN_ONCE_VALUE};
    use super::win::{self, Win};
    use super::work::{self, Start, Step, System};

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    /// How long a first start is waited for while it's alive (a virus scan, RDP).
    const FIRST_START: Duration = Duration::from_secs(180);

    pub fn main(args: &[OsString]) -> Option<i32> {
        let exe = std::env::current_exe().ok()?;
        let name = exe.file_name()?.to_string_lossy().into_owned();
        let role = match flags::role(&name, args)? {
            Ok(role) => role,
            Err(e) => {
                eprintln!("ssheesh: {e}");
                return Some(2);
            }
        };
        Some(match role {
            Role::Install(how) => install(&exe, how),
            Role::Finish { dir } => finish(&dir),
            Role::Uninstall { quiet } => uninstall(&exe, quiet),
            Role::UninstallFrom { dir, delete_data, quiet } => uninstall_from(&exe, &dir, delete_data, quiet),
        })
    }

    fn install(exe: &Path, how: Install) -> i32 {
        // A second installer (a second click while one runs) leaves quietly.
        let Some(_only) = win::only_installer() else { return 0 };
        let plan = work::plan(&Win, how.dir.as_deref());
        let updating = plan.is_update();
        let title = if updating { "Updating SSHeesh" } else { "Installing SSHeesh" };
        let code = Arc::new(AtomicI32::new(1));
        let (exe, said) = (exe.to_path_buf(), code.clone());
        let ui = how.ui;
        let question = ask(&plan);
        let job = move |progress: Progress| {
            let c = run_install(&exe, &how, &plan, title, &progress);
            said.store(c, Ordering::SeqCst);
        };
        match ui {
            Ui::Silent => job(Progress::nowhere()),
            Ui::Passive => sshee_ui::setup::run(Phase::Working { title: title.into(), words: "Starting…".into(), at: 0.02 }, Box::new(move |_, p| job(p))),
            Ui::Full => sshee_ui::setup::run(question, Box::new(move |_, p| job(p))),
        }
        code.load(Ordering::SeqCst)
    }

    /// The question a double-clicked installer asks.
    fn ask(plan: &work::Plan) -> Phase {
        let folder = plan.dir.display().to_string();
        if plan.is_update() {
            let had = plan.installed.as_deref().map(|v| format!("SSHeesh {v}")).unwrap_or_else(|| "SSHeesh".into());
            Phase::Ask {
                title: "Update SSHeesh".into(),
                about: format!("{had} is installed here. Version {VERSION} takes its place; your servers, passwords and settings stay."),
                place: Some(("Where you installed it".into(), folder)),
                choice: None,
                go: "Update".into(),
                destructive: false,
            }
        } else {
            Phase::Ask {
                title: "Install SSHeesh".into(),
                about: format!("Servers, terminals and files that don't fall over. Version {VERSION}."),
                place: Some(("For you only, no administrator needed".into(), folder)),
                choice: None,
                go: "Install".into(),
                destructive: false,
            }
        }
    }

    fn run_install(exe: &Path, how: &Install, plan: &work::Plan, title: &str, progress: &Progress) -> i32 {
        let show = |s: Step| progress.show(Phase::Working { title: title.into(), words: s.words().into(), at: s.at() });
        let done = match work::install(&Win, plan, how, exe, VERSION, &mut |s| show(s)) {
            Ok(done) => done,
            Err(e) => {
                eprintln!("ssheesh: {e}");
                progress.show(Phase::Failed { title: "SSHeesh couldn't be installed".into(), line: format!("{e}. Nothing of yours was changed.") });
                return 1;
            }
        };
        let finished = if plan.is_update() { "SSHeesh is updated" } else { "SSHeesh is installed" };
        // A double-clicked installer always opens the app; 0.10's update asks with /R.
        if !(how.run_after || how.ui == Ui::Full) {
            progress.show(Phase::Done { title: finished.into(), line: "It's in the Start menu.".into(), closes: true });
            std::thread::sleep(Duration::from_millis(900));
            progress.close();
            return 0;
        }
        show(Step::Starting);
        match start_app(&done.layout, &how.app_args) {
            Start::Ready | Start::Slow => {
                progress.show(Phase::Done { title: finished.into(), line: "Opening it now…".into(), closes: true });
                std::thread::sleep(Duration::from_millis(900));
                progress.close();
                0
            }
            Start::Ended(code) => {
                let back = if done.kept == Kept::Previous { " The version you had is kept: “SSHeesh (previous version)” in the Start menu opens it." } else { "" };
                let code = code.map(|c| format!(" (code {c})")).unwrap_or_default();
                progress.show(Phase::Failed {
                    title: "SSHeesh didn't start".into(),
                    line: format!("It's installed, but it closed before its window opened{code}. Your servers, passwords and settings are untouched.{back}"),
                });
                1
            }
        }
    }

    /// Starts the installed app with `args`, and waits for its window.
    fn start_app(layout: &Layout, args: &[OsString]) -> Start {
        if let Some(pid) = std::env::var(super::WAIT_PID).ok().and_then(|p| p.trim().parse::<u32>().ok()) {
            win::wait_for_exit(pid, Duration::from_secs(30));
        }
        let ready = std::env::temp_dir().join(format!("ssheesh-ready-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ready);
        if std::fs::create_dir_all(&ready).is_err() {
            return Start::Slow;
        }
        let mut cmd = std::process::Command::new(&layout.exe);
        cmd.args(args).current_dir(&layout.dir).env("RUNE_READY_DIR", &ready).env_remove(super::WAIT_PID);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let start = match cmd.spawn() {
            Ok(mut child) => work::wait_ready(&mut child, &ready, FIRST_START),
            Err(_) => Start::Ended(None),
        };
        let _ = std::fs::remove_dir_all(&ready);
        start
    }

    /// The `RunOnce` entry: the replacement a power loss cut off.
    fn finish(dir: &Path) -> i32 {
        let layout = Layout::of(dir);
        let code = match super::files::finish(&layout) {
            Ok(_) => {
                let _ = work::register(&Win, &layout, VERSION);
                0
            }
            Err(e) => {
                eprintln!("ssheesh: {e}");
                1
            }
        };
        Win.remove_value(RUN_ONCE_KEY, RUN_ONCE_VALUE);
        code
    }

    /// "Uninstall" from Installed apps: the work runs from a copy in the temporary folder, so
    /// the program itself can go.
    fn uninstall(exe: &Path, quiet: bool) -> i32 {
        let plan = work::plan(&Win, None);
        if plan.found == Found::Nothing {
            eprintln!("ssheesh: SSHeesh isn't installed");
            return 1;
        }
        let temp = std::env::temp_dir().join(format!("SSHeesh-uninstall-{}", std::process::id()));
        let copy = temp.join("ssheesh-uninstall.exe");
        if let Err(e) = std::fs::create_dir_all(&temp).and_then(|_| std::fs::copy(exe, &copy)) {
            eprintln!("ssheesh: the uninstaller couldn't be copied: {e}");
            return 1;
        }
        let mut cmd = std::process::Command::new(&copy);
        cmd.arg(flags::UNINSTALL_FROM).arg(&plan.dir);
        if quiet {
            cmd.arg("/S");
        }
        match cmd.spawn() {
            Ok(mut child) if quiet => child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1),
            Ok(_) => 0,
            Err(e) => {
                eprintln!("ssheesh: the uninstaller didn't start: {e}");
                1
            }
        }
    }

    /// The folders the app keeps the person's things in.
    fn data_folders() -> Vec<PathBuf> {
        let mut all = Vec::new();
        if let Some(home) = sshee_core::AppIdentity::standard().dir() {
            all.push(home.to_path_buf());
        }
        all.extend(dirs::data_dir().map(|d| d.join(names::APP_ID)));
        all.extend(dirs::data_local_dir().map(|d| d.join(names::APP_ID)));
        all
    }

    fn uninstall_from(exe: &Path, dir: &Path, delete_data: bool, quiet: bool) -> i32 {
        // Only ever the folder SSHeesh is installed in.
        let plan = work::plan(&Win, None);
        if plan.found == Found::Nothing || !plan.dir.as_os_str().eq_ignore_ascii_case(dir.as_os_str()) {
            eprintln!("ssheesh: {} isn't where SSHeesh is installed", dir.display());
            return 2;
        }
        // The ssheesh.exe that started this is on its way out.
        std::thread::sleep(Duration::from_millis(800));
        let code = Arc::new(AtomicI32::new(1));
        let said = code.clone();
        let dir = dir.to_path_buf();
        let job = move |delete: bool, progress: Progress| {
            let data = if delete { data_folders() } else { Vec::new() };
            let result = work::uninstall(&Win, &dir, &data, &mut |s| progress.show(Phase::Working { title: "Removing SSHeesh".into(), words: s.words().into(), at: s.at() }));
            match result {
                Ok(()) => {
                    said.store(0, Ordering::SeqCst);
                    let line = if delete { "Your settings went with it. Saved passwords stay in Windows' Credential Manager." } else { "Your settings stay, for when you come back." };
                    progress.show(Phase::Done { title: "SSHeesh is removed".into(), line: line.into(), closes: false });
                }
                Err(e) => {
                    eprintln!("ssheesh: {e}");
                    progress.show(Phase::Failed { title: "SSHeesh couldn't be removed".into(), line: e });
                }
            }
        };
        if quiet {
            job(delete_data, Progress::nowhere());
        } else {
            let ask = Phase::Ask {
                title: "Remove SSHeesh".into(),
                about: format!("SSHeesh {} goes from this computer: the program, its Start menu item and its entry in Installed apps.", plan.installed.as_deref().unwrap_or(VERSION)),
                place: Some(("Installed in".into(), plan.dir.display().to_string())),
                choice: Some("Also delete my settings, servers and saved tabs".into()),
                go: "Remove".into(),
                destructive: true,
            };
            sshee_ui::setup::run(ask, Box::new(job));
        }
        // This copy, in the temporary folder, goes at the next start of Windows.
        win::delete_at_restart(exe);
        if let Some(temp) = exe.parent() {
            win::delete_at_restart(temp);
        }
        code.load(Ordering::SeqCst)
    }
}
