//! 1.x updating itself, from one of three channels (`sshee_app::canary::Channel`, `channel.rs`):
//! test builds (a build the canary workflow made, which knows its run number), releases, and
//! release candidates (a build the release workflow made, `build.rs`). Such a build looks for a
//! newer one when it starts and every few hours, downloads it in the background, checks it,
//! and offers to restart into it. 0.10 never sees any of this: it reads `latest.json` from
//! GitHub's "latest" release, and 1.x never does.
//!
//! End to end:
//! 1. **Which channel** follows Settings → Updates: "Never check" looks nowhere; a test build
//!    looks for test builds while "Check for test builds" is on and for releases when it's
//!    off; a release looks for releases (a release candidate for candidates); a build made
//!    on someone's computer never looks.
//! 2. **The manifest.** Test builds: `canary.json` (`manifest.rs`), compared by run.
//!    Releases: `update-v1.json` (`release.rs`) from the channel's own release, whose signature
//!    by the release key (the one 0.10 trusts) must name it and its version; compared by
//!    semver, never down unless this version was withdrawn. Fetched through Tor while Tor is
//!    on, as every connection the app makes (`http.rs`).
//! 3. **"Ask me"** stops here: the line says a version is out, with a link to its page.
//! 4. **The archive** for this system is downloaded (from this repository's releases only),
//!    its size and SHA-256 checked, then its minisign signature by the channel's key, made for
//!    a name (and for releases a version) its trusted comment carries (`verify.rs`). Unsigned,
//!    or with no key compiled in, or where the program's folder can't be written: the line
//!    only offers the download page.
//! 5. The program comes out of the archive (`archive.rs`) and waits next to this one as
//!    `<name>.new` (`swap.rs`); the line says it's ready (`sshee-ui`'s `screens::canary`).
//! 6. "Restart to update": the new build takes the program's name, this one keeps a second
//!    name, and a helper (this same old program, [`watch_main`]) waits for the window to
//!    close, starts the new build, and puts the old one back if the new one doesn't say it
//!    started within a minute. The tabs come back as at any start (`tabs.json`).
//!
//! What is staged, and what failed here, is kept by a *token*: the run for a test build, the
//! version for a release. Older test builds pass a run as the token, so the two still talk.

pub mod archive;
pub mod channel;
pub mod http;
pub mod manifest;
pub mod release;
pub mod swap;
pub mod verify;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sshee_app::canary::{Act, Build, ByHand, Channel, Notice};
use sshee_app::model::UpdateMode;
use sshee_app::settings_store::SettingsStore;
use sshee_ui::shell::Remote;

use channel::Source;
use manifest::{Asset, Manifest, Platform};
use release::{Offer, Release, Seen};
use swap::Places;

/// The canary key's public half, as `tauri signer generate` prints it (base64, one line; the
/// `.key.pub` file). Its private half is the repository secret `CANARY_SIGNING_KEY`, with its
/// password in `CANARY_SIGNING_KEY_PASSWORD` (how to make them: .github/workflows/canary.yml).
/// Never the release key (`channel::RELEASE_PUBLIC_KEY`). Empty until the owner makes the key:
/// with no key no test build is installed, and a newer one is only offered for download.
pub const CANARY_PUBLIC_KEY: &str = "";

/// The most a manifest may take.
const MOST_MANIFEST: u64 = 256 * 1024;
/// The most a manifest's signature may take.
const MOST_SIGNATURE: u64 = 8 * 1024;
/// The first look, once the window has settled and Tor has had a moment.
const FIRST_LOOK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(4 * 3600);
/// After a look that failed (offline, Tor still starting).
const AGAIN: Duration = Duration::from_secs(20 * 60);
/// While nothing is looked for ("Never check"): how often to see whether that changed.
const OFF: Duration = Duration::from_secs(60);
/// How long the new build's window runs before it says it started.
const SETTLED: Duration = Duration::from_secs(5);
/// The helper waits at most this long for the window to close before it starts the new build.
const CLOSING: Duration = Duration::from_secs(30);

/// The helper's flag: `--canary-watch <program> <token> -- <the program's arguments>`.
const WATCH_FLAG: &str = "--canary-watch";
/// Told to a new build by the helper: the token on trial.
const TRIAL_VAR: &str = "SSHEESH_CANARY_TRIAL";
/// Told to the old build put back: the token that didn't start.
const BACK_VAR: &str = "SSHEESH_CANARY_ROLLED_BACK";

/// Which build this is (`build.rs`).
pub fn build() -> Build {
    Build {
        version: env!("CARGO_PKG_VERSION").into(),
        sha: env!("SSHEESH_BUILD_SHA").into(),
        run: env!("SSHEESH_CANARY_RUN").parse().ok(),
        release: matches!(env!("SSHEESH_RELEASE"), "1"),
    }
}

/// The copy's own folder for updates: `~/.sshee-preview/canary` (the name from when only test
/// builds updated; every channel keeps its state there).
fn folder() -> Option<PathBuf> {
    sshee_core::app_identity::current().dir().map(|d| d.join("canary"))
}

/// Whether `token` is one: a run or a version, plain enough to be part of a file's name.
fn token_ok(token: &str) -> bool {
    !token.is_empty() && token.len() <= 64 && token.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b)) && !token.starts_with('.')
}

/// The version a token stands for: a test build's run under this build's version, or the
/// release's version itself.
fn version_of(build: &Build, token: &str) -> String {
    if token.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}.{token}", build.version)
    } else {
        token.to_owned()
    }
}

fn marker(folder: &Path, token: &str) -> PathBuf {
    folder.join(format!("started-{token}"))
}

/// Tokens that were tried here and didn't start: never offered again. Runs written by older
/// test builds as numbers are read as their tokens.
fn failed(folder: &Path) -> Vec<String> {
    let list = std::fs::read(folder.join("failed.json")).ok().and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()).unwrap_or_default();
    list.iter().filter_map(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_owned))).filter(|t| token_ok(t)).collect()
}

fn note_failed(folder: &Path, token: &str) {
    let mut tokens = failed(folder);
    if !tokens.iter().any(|t| t == token) {
        tokens.push(token.to_owned());
    }
    let start = tokens.len().saturating_sub(50);
    let _ = std::fs::create_dir_all(folder);
    let _ = sshee_app::settings_store::write_atomic(&folder.join("failed.json"), json!(tokens[start..]).to_string().as_bytes());
}

/// The build waiting next to the program.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Staged {
    token: String,
    version: String,
    /// The program's SHA-256.
    sha256: String,
    /// The withdrawn version it replaces, when it's older than that.
    replaces: Option<String>,
}

impl Staged {
    /// Whether it's still for a copy of version `own`: later, or replacing it.
    fn fits(&self, own: &str) -> bool {
        release::newer(&self.version, own) || self.replaces.as_deref() == Some(own)
    }
}

fn staged(folder: &Path) -> Option<Staged> {
    let v: Value = serde_json::from_slice(&std::fs::read(folder.join("staged.json")).ok()?).ok()?;
    // Older test builds wrote the run as "run".
    let token = v["token"].as_str().map(str::to_owned).or_else(|| v["run"].as_u64().map(|r| r.to_string())).filter(|t| token_ok(t))?;
    Some(Staged { token, version: v["version"].as_str()?.to_owned(), sha256: v["sha256"].as_str()?.to_owned(), replaces: v["replaces"].as_str().map(str::to_owned) })
}

fn save_staged(folder: &Path, s: &Staged) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let v = json!({ "token": s.token, "version": s.version, "sha256": s.sha256, "replaces": s.replaces });
    sshee_app::settings_store::write_atomic(&folder.join("staged.json"), v.to_string().as_bytes())
}

/// Whether the build at `places.new` is still the one that was checked.
fn still_staged(places: &Places, s: &Staged) -> bool {
    std::fs::read(&places.new).is_ok_and(|b| verify::sha256_hex(&b) == s.sha256)
}

/// What this start was told by the update before it (read once, while the program is still
/// one thread, and taken out of the environment so the shells it starts don't inherit it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Arrival {
    /// This is a new build on trial: it says so once its window has run a while.
    trial: Option<String>,
    /// The build of this token didn't start, and this one was put back.
    rolled_back: Option<String>,
}

pub fn arrival() -> Arrival {
    let take = |var: &str| {
        let value = std::env::var(var).ok().filter(|v| token_ok(v));
        std::env::remove_var(var);
        value
    };
    Arrival { trial: take(TRIAL_VAR), rolled_back: take(BACK_VAR) }
}

/// Settings → Updates, as the looking thread reads it: "Install updates" and "Check for test
/// builds".
#[derive(Debug, Clone)]
struct Wish {
    mode: UpdateMode,
    test_builds: bool,
}

/// What the program holds for updates: what Settings wants, and what it was told at start.
pub struct Updater {
    build: Build,
    arrival: Arrival,
    wish: Arc<Mutex<Wish>>,
}

/// The helper's stdin, held open until this process ends: its end tells the helper the
/// window has closed.
static HELPER: Mutex<Option<std::process::ChildStdin>> = Mutex::new(None);

impl Updater {
    pub fn new(arrival: Arrival) -> Updater {
        Updater { build: build(), arrival, wish: Arc::new(Mutex::new(Wish { mode: UpdateMode::Auto, test_builds: true })) }
    }

    /// Follows "Install updates" and "Check for test builds" in `store`.
    pub fn listen(&self, store: &mut SettingsStore) {
        *self.wish.lock().unwrap_or_else(|e| e.into_inner()) = Wish { mode: store.update_mode(), test_builds: store.canary_updates() };
        let wish = self.wish.clone();
        store.subscribe(Box::new(move |settings, more| {
            *wish.lock().unwrap_or_else(|e| e.into_inner()) = Wish { mode: settings.update_mode.clone(), test_builds: more.canary_updates };
        }));
    }

    fn places() -> Option<Places> {
        std::env::current_exe().ok().map(|exe| Places::of(&exe))
    }

    /// Once the window runs: a new build says it started; one put back says why; and a build
    /// that updates itself starts looking for newer ones.
    pub fn start(&self, remote: Remote) {
        let (Some(folder), Some(places)) = (folder(), Updater::places()) else { return };
        if let Some(token) = &self.arrival.rolled_back {
            let version = version_of(&self.build, token);
            remote.update(move |s| sshee_ui::screens::canary::show(s, Notice::RolledBack { version }));
        }
        let (trial, build, wish) = (self.arrival.trial.clone(), self.build.clone(), self.wish.clone());
        let spawned = std::thread::Builder::new().name("update".into()).spawn(move || {
            if let Some(token) = trial {
                // Said from the window's own thread, once it has run a while: it started.
                std::thread::sleep(SETTLED);
                let (said, heard) = std::sync::mpsc::channel();
                let at = marker(&folder, &token);
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
            let own = build.updates_itself().then(|| build.full_version());
            tidy(&folder, &places, own.as_deref());
            let Some(platform) = Platform::current().filter(|_| build.updates_itself()) else { return };
            looks(&folder, &places, &build, platform, &wish, &remote);
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: updates aren't looked for: {e}");
        }
    }

    /// "Restart to update": the checked build takes this one's place, and the helper starts
    /// it once the window has closed. On failure nothing has changed.
    pub fn restart(&self) -> Result<(), String> {
        let (Some(folder), Some(places)) = (folder(), Updater::places()) else {
            return Err("this copy doesn't update itself".into());
        };
        if !self.build.updates_itself() {
            return Err("this copy doesn't update itself".into());
        }
        let own = self.build.full_version();
        let staged = staged(&folder).filter(|s| s.fits(&own)).ok_or("there's no newer build waiting")?;
        if !still_staged(&places, &staged) {
            return Err("the new build changed after it was checked".into());
        }
        swap::install(&places, &staged.sha256)?;
        let mut helper = Command::new(&places.old);
        helper.arg(WATCH_FLAG).arg(&places.exe).arg(&staged.token).arg("--").args(std::env::args_os().skip(1));
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
/// build that's no longer for this one (version `own`; `None` for a copy that doesn't update).
fn tidy(folder: &Path, places: &Places, own: Option<&str>) {
    let keep = staged(folder).is_some_and(|s| own.is_some_and(|own| s.fits(own)) && !failed(folder).contains(&s.token));
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

/// Looks now and every few hours, in the channel Settings picks at each look.
fn looks(folder: &Path, places: &Places, build: &Build, platform: Platform, wish: &Mutex<Wish>, remote: &Remote) {
    let mut wait = FIRST_LOOK;
    let mut said: Option<Notice> = None;
    loop {
        std::thread::sleep(wait);
        let now = wish.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some((channel, act)) = Channel::choose(build, &now.mode, now.test_builds) else {
            wait = OFF;
            continue;
        };
        wait = match look(folder, places, build, channel, act, platform) {
            Ok(notice) => {
                // Each notice once (after "Later" it stays away); a newer build says so again.
                if let Some(notice) = notice.filter(|n| said.as_ref() != Some(n)) {
                    said = Some(notice.clone());
                    remote.update(move |s| sshee_ui::screens::canary::show(s, notice));
                }
                EVERY
            }
            Err(e) => {
                eprintln!("ssheesh-preview: looking for a newer build ({channel:?}): {e}");
                AGAIN
            }
        };
    }
}

/// A newer build a manifest offers this copy, and how to check its archive.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    token: String,
    version: String,
    asset: Asset,
    /// The archive's name as signed, as `verify::check_with` takes it.
    parts: Vec<String>,
    /// The withdrawn version (this copy's) it replaces.
    replaces: Option<String>,
    key: &'static str,
}

/// Where a channel of releases keeps the newest manifest it took (`release::Seen`).
fn seen_file(folder: &Path, channel: Channel) -> Option<PathBuf> {
    match channel {
        Channel::Stable => Some(folder.join("seen-stable.json")),
        Channel::Rc => Some(folder.join("seen-rc.json")),
        Channel::Canary => None,
    }
}

/// What `channel` offers a copy of `build` on `platform`, from a manifest already fetched:
/// `manifest` and, for releases, its signature, taken only when no older than what `seen`
/// says was taken before (which it then notes).
fn found(channel: Channel, build: &Build, platform: Platform, failed: &[String], manifest: &[u8], signature: Option<&str>, seen: &mut Seen) -> Result<Option<Found>, String> {
    let source = Source::of(channel);
    match channel {
        Channel::Canary => {
            let Some(own) = build.run else { return Ok(None) };
            let manifest = Manifest::parse(manifest)?;
            let runs: Vec<u64> = failed.iter().filter_map(|t| t.parse().ok()).collect();
            let Some(asset) = manifest::offer(&manifest, own, platform, &runs) else { return Ok(None) };
            let parts = vec![format!("file:{}", platform.signed_name(&manifest.version))];
            Ok(Some(Found { token: manifest.run.to_string(), version: manifest.version.clone(), asset: asset.clone(), parts, replaces: None, key: source.key }))
        }
        Channel::Stable | Channel::Rc => {
            let release = Release::verified(manifest, signature.unwrap_or_default(), source.key)?;
            seen.admits(&release)?;
            seen.note(&release);
            let own = build.full_version();
            let Some(offer) = release::offer(&release, &own, channel, platform, failed) else { return Ok(None) };
            let asset = offer.asset().clone();
            let parts = vec![format!("file:{}", release::signed_name(&asset)), format!("version:{}", release.text)];
            let replaces = matches!(offer, Offer::Replaces(_)).then_some(own);
            Ok(Some(Found { token: release.text.clone(), version: release.text, asset, parts, replaces, key: source.key }))
        }
    }
}

/// One look: what to say, if anything.
fn look(folder: &Path, places: &Places, build: &Build, channel: Channel, act: Act, platform: Platform) -> Result<Option<Notice>, String> {
    let route = http::Route::now();
    let source = Source::of(channel);
    let manifest = http::get(source.manifest, MOST_MANIFEST, route)?;
    let signature = match source.signature {
        Some(url) => Some(String::from_utf8(http::get(url, MOST_SIGNATURE, route)?).map_err(|_| "the manifest's signature isn't text")?),
        None => None,
    };
    let seen_at = seen_file(folder, channel);
    let mut seen = seen_at.as_ref().and_then(|f| std::fs::read(f).ok()).map(|b| Seen::read(&b)).unwrap_or_default();
    let before = seen.clone();
    let found = found(channel, build, platform, &failed(folder), &manifest, signature.as_deref(), &mut seen)?;
    if let Some(at) = seen_at.filter(|_| seen != before) {
        std::fs::create_dir_all(folder).and_then(|_| sshee_app::settings_store::write_atomic(&at, &seen.write())).map_err(|e| format!("couldn't note the manifest taken: {e}"))?;
    }
    let Some(found) = found else { return Ok(None) };
    let version = found.version.clone();
    if act == Act::Tell {
        return Ok(Some(Notice::Out { version }));
    }
    let ready = |version: String| if found.replaces.is_some() { Notice::Replaces { version } } else { Notice::Ready { version } };
    // Already downloaded and checked.
    if staged(folder).is_some_and(|s| s.token == found.token && still_staged(places, &s)) {
        return Ok(Some(ready(version)));
    }
    let by_hand = |why| Ok(Some(Notice::ByHand { version: version.clone(), why }));
    if verify::public_key(found.key).is_none() {
        return by_hand(ByHand::NoKey);
    }
    if found.asset.signature.is_none() {
        return by_hand(ByHand::Unsigned);
    }
    if !swap::writable(places) {
        return by_hand(ByHand::Folder);
    }
    let archive = http::get(&found.asset.url, found.asset.size, route)?;
    if let Err(refused) = verify::check_with(&archive, &found.asset, found.key, &found.parts) {
        eprintln!("ssheesh-preview: {version} wasn't installed: {refused}");
        return by_hand(ByHand::Mismatch);
    }
    let program = archive::program(&archive, platform.packing(), platform.program())?;
    drop(archive);
    if let Err(e) = swap::stage(places, &program) {
        return by_hand(ByHand::Failed(e.to_string()));
    }
    save_staged(folder, &Staged { token: found.token.clone(), version: version.clone(), sha256: verify::sha256_hex(&program), replaces: found.replaces.clone() }).map_err(|e| e.to_string())?;
    Ok(Some(ready(version)))
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

/// What the helper was asked: the program, the token on trial, the program's own arguments.
#[derive(Debug, PartialEq)]
struct Watch {
    exe: PathBuf,
    token: String,
    args: Vec<OsString>,
}

fn watch_request(args: &[OsString]) -> Option<Result<Watch, String>> {
    if args.first().is_none_or(|a| a != WATCH_FLAG) {
        return None;
    }
    Some(match args {
        [_, exe, token, dashes, rest @ ..] if dashes == "--" => match token.to_str().filter(|t| token_ok(t)) {
            Some(token) => Ok(Watch { exe: PathBuf::from(exe), token: token.to_owned(), args: rest.to_vec() }),
            None => Err("the build on trial isn't named plainly".into()),
        },
        _ => Err(format!("usage: {WATCH_FLAG} <program> <run or version> -- <arguments>")),
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
    let at = marker(&folder, &watch.token);
    let _ = std::fs::remove_file(&at);
    let start = |exe: &Path, var: &str| {
        let mut cmd = Command::new(exe);
        cmd.args(&watch.args).env(var, &watch.token).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        detach(&mut cmd);
        cmd.spawn()
    };
    match swap::watch(&places, &at, swap::TRIAL, Duration::from_millis(250), |exe| start(exe, TRIAL_VAR)) {
        swap::Trial::Started => Some(0),
        swap::Trial::RolledBack(why) => {
            eprintln!("ssheesh-preview: the new build {why}; the one from before is back");
            note_failed(&folder, &watch.token);
            let _ = std::fs::remove_file(folder.join("staged.json"));
            Some(if start(&places.exe, BACK_VAR).is_ok() { 0 } else { 1 })
        }
        swap::Trial::Stuck(why) => {
            eprintln!("ssheesh-preview: the new build {why}");
            note_failed(&folder, &watch.token);
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verify::tests::TestKey;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn release_build(version: &str) -> Build {
        Build { version: version.into(), sha: "4edfea3".into(), run: None, release: true }
    }

    #[test]
    fn the_helper_is_asked_with_its_own_flag() {
        assert_eq!(watch_request(&os(&[])), None);
        assert_eq!(watch_request(&os(&["--demo"])), None);
        let w = watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview", "125", "--", "--ssh-config", "cfg"])).unwrap().unwrap();
        assert_eq!(w, Watch { exe: "apps/ssheesh-preview".into(), token: "125".into(), args: os(&["--ssh-config", "cfg"]) });
        // A release goes by its version.
        let w = watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview", "1.0.1", "--"])).unwrap().unwrap();
        assert_eq!(w.token, "1.0.1");
        for bad in ["../x", "a/b", "", ".hidden", "1.0.1 x"] {
            assert!(watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview", bad, "--"])).unwrap().is_err(), "{bad}");
        }
        assert!(watch_request(&os(&[WATCH_FLAG, "apps/ssheesh-preview"])).unwrap().is_err());
    }

    #[test]
    fn this_build_knows_what_it_is() {
        let b = build();
        assert_eq!(b.version, env!("CARGO_PKG_VERSION"));
        assert!(b.sha.is_empty() || (b.sha.len() == 7 && b.sha.chars().all(|c| c.is_ascii_hexdigit())));
        // Tests are never built as what the workflows publish.
        assert!(!b.release || b.run.is_none());
    }

    #[test]
    fn a_token_names_its_version() {
        let test = Build { version: "1.0.0-canary".into(), sha: String::new(), run: Some(124), release: false };
        assert_eq!(version_of(&test, "125"), "1.0.0-canary.125");
        assert_eq!(version_of(&release_build("1.0.0"), "1.0.1"), "1.0.1");
    }

    #[test]
    fn failed_and_staged_builds_are_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("canary");
        assert!(failed(&folder).is_empty() && staged(&folder).is_none());
        note_failed(&folder, "125");
        note_failed(&folder, "125");
        note_failed(&folder, "1.0.1");
        assert_eq!(failed(&folder), ["125", "1.0.1"]);
        // Runs an older test build wrote as numbers read as their tokens.
        std::fs::write(folder.join("failed.json"), b"[120, \"1.0.1\", \"../x\"]").unwrap();
        assert_eq!(failed(&folder), ["120", "1.0.1"]);
        let s = Staged { token: "131".into(), version: "1.0.0-canary.131".into(), sha256: verify::sha256_hex(b"the new build"), replaces: None };
        save_staged(&folder, &s).unwrap();
        assert_eq!(staged(&folder), Some(s.clone()));
        // The staged build is kept while it's newer than this one and next to the program.
        let exe = dir.path().join("ssheesh-preview");
        std::fs::write(&exe, b"the build from before").unwrap();
        let places = Places::of(&exe);
        swap::stage(&places, b"the new build").unwrap();
        assert!(still_staged(&places, &s));
        std::fs::write(marker(&folder, "125"), b"").unwrap();
        tidy(&folder, &places, Some("1.0.0-canary.124"));
        assert!(places.new.exists() && staged(&folder).is_some() && !marker(&folder, "125").exists());
        // Once this build is as new, it goes.
        tidy(&folder, &places, Some("1.0.0-canary.131"));
        assert!(!places.new.exists() && staged(&folder).is_none());
    }

    #[test]
    fn a_staged_build_from_an_older_test_build_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("staged.json"), br#"{"run":131,"version":"1.0.0-canary.131","sha256":"ab"}"#).unwrap();
        let s = staged(dir.path()).unwrap();
        assert_eq!((s.token.as_str(), s.replaces.as_deref()), ("131", None));
        assert!(s.fits("1.0.0-canary.130") && !s.fits("1.0.0-canary.131"));
    }

    #[test]
    fn a_staged_replacement_fits_only_the_version_it_replaces() {
        let s = Staged { token: "1.0.0".into(), version: "1.0.0".into(), sha256: String::new(), replaces: Some("1.0.1".into()) };
        assert!(s.fits("1.0.1"));
        assert!(!s.fits("1.0.2") && !s.fits("1.0.0"));
        assert!(s.fits("0.9.0"), "and anything older, as any later version");
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
        // The same manifest through the channel: a test build of run 124 is offered run 125.
        let test = Build { version: "1.0.0-canary".into(), sha: String::new(), run: Some(124), release: false };
        let found = found(Channel::Canary, &test, Platform::LinuxX64, &[], include_bytes!("../fixtures/canary.json"), None, &mut Seen::default()).unwrap().unwrap();
        assert_eq!((found.token.as_str(), found.version.as_str(), found.key), ("125", "1.0.0-canary.125", CANARY_PUBLIC_KEY));
        assert_eq!(found.parts, ["file:ssheesh-1.0.0-canary.125-linux-x64.tar.gz"]);
        // Not after it failed here.
        assert_eq!(canary_found(Channel::Canary, &test, &["125".into()]), None);
    }

    fn canary_found(channel: Channel, build: &Build, failed: &[String]) -> Option<Found> {
        super::found(channel, build, Platform::LinuxX64, failed, include_bytes!("../fixtures/canary.json"), None, &mut Seen::default()).unwrap()
    }

    /// From a downloaded archive to the program waiting next to this one: the checks a look
    /// makes, on made-up bytes, with a key made here.
    #[test]
    fn a_checked_archive_is_staged_and_a_changed_one_is_not() {
        let key = TestKey::new(3);
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

    /// A release end to end on made-up bytes: the manifest and its signature as the release
    /// workflow would publish them (signed here with a throwaway key, never the release key),
    /// what a release of 1.0.0 is offered, and the archive's own checks.
    #[test]
    fn a_signed_release_is_offered_and_its_archive_checked() {
        let key = TestKey::new(21);
        let program = b"made-up program, version 1.0.1".as_slice();
        let archive = archive::tests::tar_gz(&[("ssheesh-preview", program)]);
        let file = "ssheesh-1.0.1-linux-x64.tar.gz";
        let signature = key.sign_comment(&archive, &format!("timestamp:1794688787\tfile:{file}\tversion:1.0.1"));
        let manifest = json!({
            "schema": 1, "version": "1.0.1", "pub_date": "2026-12-01T10:00:00Z", "pulled": [],
            "platforms": { "linux-x64": { "url": format!("{}v1.0.1/{file}", manifest::DOWNLOADS), "size": archive.len(), "sha256": verify::sha256_hex(&archive), "signature": signature } }
        })
        .to_string();
        let minisig = key.minisig(manifest.as_bytes(), "timestamp:1794688787\tfile:update-v1.json\tversion:1.0.1");
        let release = Release::verified(manifest.as_bytes(), &minisig, &key.public()).unwrap();
        let offer = release::offer(&release, "1.0.0", Channel::Stable, Platform::LinuxX64, &[]).unwrap();
        let parts = [format!("file:{}", release::signed_name(offer.asset())), "version:1.0.1".to_owned()];
        assert_eq!(verify::check_with(&archive, offer.asset(), &key.public(), &parts), Ok(()));
        assert_eq!(archive::program(&archive, manifest::Packing::TarGz, "ssheesh-preview").unwrap(), program);
        // The same archive checked as another version's: refused.
        let parts = [format!("file:{file}"), "version:1.0.2".to_owned()];
        assert!(matches!(verify::check_with(&archive, offer.asset(), &key.public(), &parts), Err(verify::Refused::Name { .. })));
        // Signed by a key that isn't the release key: the real release key refuses the manifest.
        let real = found(Channel::Stable, &release_build("1.0.0"), Platform::LinuxX64, &[], manifest.as_bytes(), Some(&minisig), &mut Seen::default());
        assert!(real.is_err(), "a manifest not signed by the release key is refused");
        // No signature at all: refused too.
        assert!(found(Channel::Stable, &release_build("1.0.0"), Platform::LinuxX64, &[], manifest.as_bytes(), None, &mut Seen::default()).is_err());
    }
}
