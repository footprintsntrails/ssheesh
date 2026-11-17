//! Test builds updating themselves. A build the canary workflow made (it knows its run number,
//! `build.rs`) looks for a newer one when it starts and every few hours, downloads it in the
//! background, checks it, and offers to restart into it. 0.10 never sees any of this: test
//! builds live in one pre-release tagged `canary` (never "latest", never a `v*` tag), signed
//! with a key of their own.
//!
//! End to end:
//! 1. `canary.json` ([`MANIFEST_URL`], `manifest.rs`) names the newest build, by its run.
//!    Fetched through Tor while Tor is on, as every connection the app makes (`http.rs`).
//! 2. A newer one for this system is downloaded (from this repository's releases only), its
//!    size and SHA-256 checked, then its minisign signature by [`CANARY_PUBLIC_KEY`], made
//!    for a name that carries its version (`verify.rs`). Unsigned, or with no key compiled
//!    in, or where the program's folder can't be written: the notice only offers the
//!    download page.
//! 3. The program comes out of the archive (`archive.rs`) and waits next to this one as
//!    `<name>.new` (`swap.rs`); the notice says "A new test build is ready" (`sshee-ui`'s
//!    `screens::canary`).
//! 4. "Restart to update": the new build takes the program's name, this one keeps a second
//!    name, and a helper (this same old program, [`watch_main`]) waits for the window to
//!    close, starts the new build, and puts the old one back if the new one doesn't say it
//!    started within a minute. The tabs come back as at any start (`tabs.json`).

pub mod archive;
pub mod http;
pub mod manifest;
pub mod swap;
pub mod verify;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sshee_app::canary::{Build, ByHand, Notice};
use sshee_app::model::UpdateMode;
use sshee_app::settings_store::SettingsStore;
use sshee_ui::shell::Remote;

use manifest::{Manifest, Platform};
use swap::Places;

/// Where the canary workflow publishes what it built last. Never `releases/latest`: that's 0.10's.
pub const MANIFEST_URL: &str = "https://github.com/footprintsntrails/ssheesh/releases/download/canary/canary.json";

/// The canary key's public half, as `tauri signer generate` prints it (base64, one line; the
/// `.key.pub` file). Its private half is the repository secret `CANARY_SIGNING_KEY`, with its
/// password in `CANARY_SIGNING_KEY_PASSWORD` (how to make them: .github/workflows/canary.yml).
/// Never 0.10's updater key. Empty until the owner makes the key: with no key nothing is
/// installed, and a newer build is only offered for download.
pub const CANARY_PUBLIC_KEY: &str = "";

/// The most `canary.json` may take.
const MOST_MANIFEST: u64 = 256 * 1024;
/// The first look, once the window has settled and Tor has had a moment.
const FIRST_LOOK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(4 * 3600);
/// After a look that failed (offline, Tor still starting).
const AGAIN: Duration = Duration::from_secs(20 * 60);
/// While "Check for test builds" is off: how often to see whether it's back on.
const OFF: Duration = Duration::from_secs(60);
/// How long the new build's window runs before it says it started.
const SETTLED: Duration = Duration::from_secs(5);
/// The helper waits at most this long for the window to close before it starts the new build.
const CLOSING: Duration = Duration::from_secs(30);

/// The helper's flag: `--canary-watch <program> <new run> -- <the program's arguments>`.
const WATCH_FLAG: &str = "--canary-watch";
/// Told to a new build by the helper: the run on trial.
const TRIAL_VAR: &str = "SSHEESH_CANARY_TRIAL";
/// Told to the old build put back: the run that didn't start.
const BACK_VAR: &str = "SSHEESH_CANARY_ROLLED_BACK";

/// Which build this is (`build.rs`).
pub fn build() -> Build {
    Build { version: env!("CARGO_PKG_VERSION").into(), sha: env!("SSHEESH_BUILD_SHA").into(), run: env!("SSHEESH_CANARY_RUN").parse().ok() }
}

/// The copy's own folder for updates: `~/.sshee-preview/canary`.
fn folder() -> Option<PathBuf> {
    sshee_core::app_identity::current().dir().map(|d| d.join("canary"))
}

fn marker(folder: &Path, run: u64) -> PathBuf {
    folder.join(format!("started-{run}"))
}

/// Runs that were tried here and didn't start: never offered again.
fn failed_runs(folder: &Path) -> Vec<u64> {
    std::fs::read(folder.join("failed.json")).ok().and_then(|b| serde_json::from_slice::<Vec<u64>>(&b).ok()).unwrap_or_default()
}

fn note_failed(folder: &Path, run: u64) {
    let mut runs = failed_runs(folder);
    if !runs.contains(&run) {
        runs.push(run);
    }
    let start = runs.len().saturating_sub(50);
    let _ = std::fs::create_dir_all(folder);
    let _ = sshee_app::settings_store::write_atomic(&folder.join("failed.json"), json!(runs[start..]).to_string().as_bytes());
}

/// The build waiting next to the program: its run, version and the SHA-256 of the program.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Staged {
    run: u64,
    version: String,
    sha256: String,
}

fn staged(folder: &Path) -> Option<Staged> {
    let v: Value = serde_json::from_slice(&std::fs::read(folder.join("staged.json")).ok()?).ok()?;
    Some(Staged { run: v["run"].as_u64()?, version: v["version"].as_str()?.to_owned(), sha256: v["sha256"].as_str()?.to_owned() })
}

fn save_staged(folder: &Path, s: &Staged) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    sshee_app::settings_store::write_atomic(&folder.join("staged.json"), json!({ "run": s.run, "version": s.version, "sha256": s.sha256 }).to_string().as_bytes())
}

/// Whether the build at `places.new` is still the one that was checked.
fn still_staged(places: &Places, s: &Staged) -> bool {
    std::fs::read(&places.new).is_ok_and(|b| verify::sha256_hex(&b) == s.sha256)
}

/// What this start was told by the update before it (read once, while the program is still
/// one thread, and taken out of the environment so the shells it starts don't inherit it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Arrival {
    /// This is a new build on trial: it says so once its window has run a while.
    trial: Option<u64>,
    /// The build of this run didn't start, and this one was put back.
    rolled_back: Option<u64>,
}

pub fn arrival() -> Arrival {
    let take = |var: &str| {
        let value = std::env::var(var).ok().and_then(|v| v.parse::<u64>().ok());
        std::env::remove_var(var);
        value
    };
    Arrival { trial: take(TRIAL_VAR), rolled_back: take(BACK_VAR) }
}

/// What the program holds for updates: whether to look, and what it was told at start.
pub struct Updater {
    build: Build,
    arrival: Arrival,
    /// Settings → Updates → "Check for test builds".
    enabled: Arc<AtomicBool>,
}

/// The helper's stdin, held open until this process ends: its end tells the helper the
/// window has closed.
static HELPER: Mutex<Option<std::process::ChildStdin>> = Mutex::new(None);

impl Updater {
    pub fn new(arrival: Arrival) -> Updater {
        Updater { build: build(), arrival, enabled: Arc::new(AtomicBool::new(true)) }
    }

    /// Follows "Check for test builds" and "Install updates" in `store`: either one off
    /// ("Never check") stops the looking.
    pub fn listen(&self, store: &mut SettingsStore) {
        let on = |mode: &UpdateMode, canary: bool| canary && *mode != UpdateMode::Manual;
        self.enabled.store(on(&store.update_mode(), store.canary_updates()), Ordering::Relaxed);
        let enabled = self.enabled.clone();
        store.subscribe(Box::new(move |settings, more| enabled.store(on(&settings.update_mode, more.canary_updates), Ordering::Relaxed)));
    }

    fn places() -> Option<Places> {
        std::env::current_exe().ok().map(|exe| Places::of(&exe))
    }

    /// Once the window runs: a new build says it started; one put back says why; and a build
    /// from the canary workflow starts looking for newer ones.
    pub fn start(&self, remote: Remote) {
        let (Some(folder), Some(places)) = (folder(), Updater::places()) else { return };
        if let Some(run) = self.arrival.rolled_back {
            let version = format!("{}.{run}", self.build.version);
            remote.update(move |s| sshee_ui::screens::canary::show(s, Notice::RolledBack { version }));
        }
        let (trial, own, enabled) = (self.arrival.trial, self.build.run, self.enabled.clone());
        let spawned = std::thread::Builder::new().name("canary".into()).spawn(move || {
            if let Some(run) = trial {
                // Said from the window's own thread, once it has run a while: it started.
                std::thread::sleep(SETTLED);
                let (said, heard) = std::sync::mpsc::channel();
                let at = marker(&folder, run);
                remote.update(move |_| {
                    let _ = std::fs::create_dir_all(at.parent().unwrap_or(Path::new(".")));
                    let _ = said.send(std::fs::write(&at, b"").is_ok());
                });
                if !heard.recv_timeout(swap::TRIAL).unwrap_or(false) {
                    return;
                }
            }
            // What an update before left: once the helper has seen this one start.
            std::thread::sleep(Duration::from_secs(10));
            tidy(&folder, &places, own);
            let (Some(own), Some(platform)) = (own, Platform::current()) else { return };
            looks(&folder, &places, own, platform, &enabled, &remote);
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: test builds aren't looked for: {e}");
        }
    }

    /// "Restart to update": the checked build takes this one's place, and the helper starts
    /// it once the window has closed. On failure nothing has changed.
    pub fn restart(&self) -> Result<(), String> {
        let (Some(folder), Some(places), Some(own)) = (folder(), Updater::places(), self.build.run) else {
            return Err("this copy doesn't update itself".into());
        };
        let staged = staged(&folder).filter(|s| s.run > own).ok_or("there's no newer build waiting")?;
        if !still_staged(&places, &staged) {
            return Err("the new build changed after it was checked".into());
        }
        swap::install(&places)?;
        let mut helper = Command::new(&places.old);
        helper.arg(WATCH_FLAG).arg(&places.exe).arg(staged.run.to_string()).arg("--").args(std::env::args_os().skip(1));
        helper.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        detach(&mut helper);
        match helper.spawn() {
            Ok(mut child) => {
                *HELPER.lock().unwrap_or_else(|e| e.into_inner()) = child.stdin.take();
                Ok(())
            }
            Err(e) => {
                let _ = swap::roll_back(&places);
                Err(format!("the helper that starts it didn't start: {e}"))
            }
        }
    }
}

/// Clears what an update left: the build from before, a write cut off, markers, and a staged
/// build that's no longer newer than this one.
fn tidy(folder: &Path, places: &Places, own: Option<u64>) {
    let keep = staged(folder).is_some_and(|s| own.is_some_and(|own| s.run > own) && !failed_runs(folder).contains(&s.run));
    swap::clean(places, keep);
    if !keep {
        let _ = std::fs::remove_file(folder.join("staged.json"));
    }
    if let Ok(entries) = std::fs::read_dir(folder) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("started-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Looks now and every few hours, while "Check for test builds" is on.
fn looks(folder: &Path, places: &Places, own: u64, platform: Platform, enabled: &AtomicBool, remote: &Remote) {
    let mut wait = FIRST_LOOK;
    let mut said: Option<Notice> = None;
    loop {
        std::thread::sleep(wait);
        if !enabled.load(Ordering::Relaxed) {
            wait = OFF;
            continue;
        }
        wait = match look(folder, places, own, platform) {
            Ok(notice) => {
                // Each notice once (after "Later" it stays away); a newer build says so again.
                if let Some(notice) = notice.filter(|n| said.as_ref() != Some(n)) {
                    said = Some(notice.clone());
                    remote.update(move |s| sshee_ui::screens::canary::show(s, notice));
                }
                EVERY
            }
            Err(e) => {
                eprintln!("ssheesh-preview: looking for a newer test build: {e}");
                AGAIN
            }
        };
    }
}

/// One look: what to say, if anything.
fn look(folder: &Path, places: &Places, own: u64, platform: Platform) -> Result<Option<Notice>, String> {
    let route = http::Route::now();
    let manifest = Manifest::parse(&http::get(MANIFEST_URL, MOST_MANIFEST, route)?)?;
    let Some(asset) = manifest::offer(&manifest, own, platform, &failed_runs(folder)) else { return Ok(None) };
    let version = manifest.version.clone();
    // Already downloaded and checked.
    if staged(folder).is_some_and(|s| s.run == manifest.run && still_staged(places, &s)) {
        return Ok(Some(Notice::Ready { version }));
    }
    let by_hand = |why| Ok(Some(Notice::ByHand { version: version.clone(), why }));
    if verify::public_key(CANARY_PUBLIC_KEY).is_none() {
        return by_hand(ByHand::NoKey);
    }
    if asset.signature.is_none() {
        return by_hand(ByHand::Unsigned);
    }
    if !swap::writable(places) {
        return by_hand(ByHand::Folder);
    }
    let archive = http::get(&asset.url, asset.size, route)?;
    if let Err(refused) = verify::check(&archive, asset, &platform.signed_name(&manifest.version), CANARY_PUBLIC_KEY) {
        eprintln!("ssheesh-preview: {version} wasn't installed: {refused}");
        return by_hand(ByHand::Mismatch);
    }
    let program = archive::program(&archive, platform.packing(), platform.program())?;
    drop(archive);
    if let Err(e) = swap::stage(places, &program) {
        return by_hand(ByHand::Failed(e.to_string()));
    }
    save_staged(folder, &Staged { run: manifest.run, version: version.clone(), sha256: verify::sha256_hex(&program) }).map_err(|e| e.to_string())?;
    Ok(Some(Notice::Ready { version }))
}

/// A helper outlives the window: on Windows, without a console and outside its process group.
fn detach(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// What the helper was asked: the program, the run on trial, the program's own arguments.
#[derive(Debug, PartialEq)]
struct Watch {
    exe: PathBuf,
    run: u64,
    args: Vec<OsString>,
}

fn watch_request(args: &[OsString]) -> Option<Result<Watch, String>> {
    if args.first().is_none_or(|a| a != WATCH_FLAG) {
        return None;
    }
    Some(match args {
        [_, exe, run, dashes, rest @ ..] if dashes == "--" => match run.to_str().and_then(|r| r.parse::<u64>().ok()) {
            Some(run) => Ok(Watch { exe: PathBuf::from(exe), run, args: rest.to_vec() }),
            None => Err("the run isn't a number".into()),
        },
        _ => Err(format!("usage: {WATCH_FLAG} <program> <run> -- <arguments>")),
    })
}

/// When this program was started as the update's helper: does its work and says how it
/// ended. `None` for a normal start.
pub fn watch_main() -> Option<i32> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let watch = match watch_request(&args)? {
        Ok(watch) => watch,
        Err(e) => {
            eprintln!("ssheesh-preview: {e}");
            return Some(2);
        }
    };
    // The window that started this holds its stdin: it ends when the window's process does.
    let (done, closed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        let _ = done.send(());
    });
    let _ = closed.recv_timeout(CLOSING);
    std::thread::sleep(Duration::from_millis(300));
    let Some(folder) = folder() else { return Some(1) };
    let places = Places::of(&watch.exe);
    let at = marker(&folder, watch.run);
    let _ = std::fs::remove_file(&at);
    let start = |exe: &Path, var: &str, run: u64| {
        let mut cmd = Command::new(exe);
        cmd.args(&watch.args).env(var, run.to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        detach(&mut cmd);
        cmd.spawn()
    };
    match swap::watch(&places, &at, swap::TRIAL, Duration::from_millis(250), |exe| start(exe, TRIAL_VAR, watch.run)) {
        swap::Trial::Started => Some(0),
        swap::Trial::RolledBack(why) => {
            eprintln!("ssheesh-preview: the new test build {why}; the one from before is back");
            note_failed(&folder, watch.run);
            let _ = std::fs::remove_file(folder.join("staged.json"));
            Some(if start(&places.exe, BACK_VAR, watch.run).is_ok() { 0 } else { 1 })
        }
        swap::Trial::Stuck(why) => {
            eprintln!("ssheesh-preview: the new test build {why}");
            note_failed(&folder, watch.run);
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_helper_is_asked_with_its_own_flag() {
        assert_eq!(watch_request(&os(&[])), None);
        assert_eq!(watch_request(&os(&["--demo"])), None);
        let w = watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview", "125", "--", "--ssh-config", "cfg"])).unwrap().unwrap();
        assert_eq!(w, Watch { exe: "apps/ssheesh-preview".into(), run: 125, args: os(&["--ssh-config", "cfg"]) });
        assert!(watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview", "x", "--"])).unwrap().is_err());
        assert!(watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview"])).unwrap().is_err());
    }

    #[test]
    fn the_placeholder_key_never_installs() {
        // Until the owner sets the canary key, `look` only offers the download page.
        if CANARY_PUBLIC_KEY.is_empty() {
            assert!(verify::public_key(CANARY_PUBLIC_KEY).is_none());
        } else {
            assert!(verify::public_key(CANARY_PUBLIC_KEY).is_some(), "CANARY_PUBLIC_KEY isn't a minisign public key");
        }
    }

    #[test]
    fn this_build_knows_what_it_is() {
        let b = build();
        assert_eq!(b.version, env!("CARGO_PKG_VERSION"));
        assert!(b.sha.is_empty() || (b.sha.len() == 7 && b.sha.chars().all(|c| c.is_ascii_hexdigit())));
    }

    #[test]
    fn failed_and_staged_builds_are_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("canary");
        assert!(failed_runs(&folder).is_empty() && staged(&folder).is_none());
        note_failed(&folder, 125);
        note_failed(&folder, 125);
        note_failed(&folder, 130);
        assert_eq!(failed_runs(&folder), [125, 130]);
        let s = Staged { run: 131, version: "1.0.0-canary.131".into(), sha256: verify::sha256_hex(b"the new build") };
        save_staged(&folder, &s).unwrap();
        assert_eq!(staged(&folder), Some(s.clone()));
        // The staged build is kept while it's newer than this one and next to the program.
        let exe = dir.path().join("ssheesh-preview");
        std::fs::write(&exe, b"the build from before").unwrap();
        let places = Places::of(&exe);
        swap::stage(&places, b"the new build").unwrap();
        assert!(still_staged(&places, &s));
        std::fs::write(marker(&folder, 125), b"").unwrap();
        tidy(&folder, &places, Some(124));
        assert!(places.new.exists() && staged(&folder).is_some() && !marker(&folder, 125).exists());
        // Once this build is as new, it goes.
        tidy(&folder, &places, Some(131));
        assert!(!places.new.exists() && staged(&folder).is_none());
    }

    /// canary.json as the workflow's own step wrote it (run on made-up archives, "linux" and
    /// "win", signed with a throwaway key by `tauri signer`): read and checked by the app.
    #[test]
    fn what_the_workflow_publishes_is_what_the_app_checks() {
        const THROWAWAY_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ5NkRDRjQ5REIyQkRFMTUKUldRVjNpdmJTYzl0U1NHRFRXTzc5SE5sM2loSUFlN0xBMXJ6YjdNcW9oUFhlY0JRbkI4cmltWloK";
        let m = Manifest::parse(include_bytes!("../fixtures/canary.json")).unwrap();
        assert_eq!((m.run, m.version.as_str(), m.sha.as_str()), (125, "1.0.0-canary.125", "4edfea3"));
        for (platform, bytes) in [(Platform::LinuxX64, b"linux".as_slice()), (Platform::WindowsX64, b"win")] {
            let asset = manifest::offer(&m, 124, platform, &[]).unwrap();
            assert_eq!(verify::check(bytes, asset, &platform.signed_name(&m.version), THROWAWAY_KEY), Ok(()));
        }
        // Linux's signature on Windows' entry: signed for another name.
        let mut swapped = m.asset(Platform::WindowsX64).unwrap().clone();
        swapped.signature.clone_from(&m.asset(Platform::LinuxX64).unwrap().signature);
        (swapped.size, swapped.sha256) = (5, verify::sha256_hex(b"linux"));
        assert!(matches!(verify::check(b"linux", &swapped, &Platform::WindowsX64.signed_name(&m.version), THROWAWAY_KEY), Err(verify::Refused::Name { .. })));
        // Nothing published for macOS in this one.
        assert!(manifest::offer(&m, 124, Platform::MacArm64, &[]).is_none());
    }

    /// From a downloaded archive to the program waiting next to this one: the checks a look
    /// makes, on made-up bytes, with a key made here.
    #[test]
    fn a_checked_archive_is_staged_and_a_changed_one_is_not() {
        let key = verify::tests::TestKey::new(3);
        let platform = Platform::current().unwrap_or(Platform::LinuxX64);
        let program = b"made-up program, version 125".as_slice();
        let archive = match platform.packing() {
            manifest::Packing::TarGz => archive::tests::tar_gz(&[(platform.program(), program)]),
            manifest::Packing::Zip => archive::tests::zip_of(&[(platform.program(), program)]),
        };
        let name = platform.signed_name("1.0.0-canary.125");
        let asset = verify::tests::asset_of(&archive, Some(key.sign(&archive, &name)));
        assert_eq!(verify::check(&archive, &asset, &name, &key.public()), Ok(()));
        assert_eq!(archive::program(&archive, platform.packing(), platform.program()).unwrap(), program);
        // One byte changed on the way: the hash says so before anything is unpacked.
        let mut changed = archive.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert_eq!(verify::check(&changed, &asset, &name, &key.public()), Err(verify::Refused::Hash));
    }
}
