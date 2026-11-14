//! The live settings of a running 1.0: every 0.10 setting with its default, read from and
//! saved to the copy's own folder, and told to whoever asked to hear of changes.
//!
//! ```text
//! start ─ open(dir) ─┬─ settings.json reads ───────────────▶ those settings
//!                    ├─ it is there but doesn't read ──────▶ defaults, private, a notice
//!                    └─ no file: the first start of 1.0 here
//!                         ├─ until the program knows what 0.10 left: defaults, private
//!                         └─ seed(..): what `import` brought over, or a new install's defaults
//! change ─ update(|s| ..) ─ listeners ─ the file, whole or not at all, on a thread of its own
//! ```
//!
//! **When in doubt, private** (the plan, section 6.5). If the settings from before can't be
//! read with certainty (the file is damaged, a privacy setting in it is missing or isn't a
//! switch, the import from 0.10 was in doubt, or it hasn't answered yet), streamer mode and
//! secret hiding are on, and with Tor left on, typing rhythm protection too. The store says
//! so in [`SettingsStore::notice`] until the notice is dismissed; the window shows it.
//!
//! The file keeps 0.10's own shape (`{"state": …, "version": 4}`) under `settings`, read
//! through the same code that reads 0.10's store, so fields this version doesn't know stay,
//! and a rollback can hand 0.10 what it understands. What 1.0 added is beside it, in `more`.
//!
//! Nothing here names a screen. The folder is the program's to give: its
//! `AppIdentity::dir()`, so a Preview never writes the released app's settings.

pub mod catalog;
pub mod keymap;
pub mod schemes;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use serde::{Deserialize, Serialize};

use crate::import::Imported;
use crate::model::{Json, JsonMap, Platform, Settings, Store};

/// The file in the copy's folder.
pub const FILE: &str = "settings.json";
/// Where a file that didn't read is moved, so the next save doesn't write over what might
/// still be mended by hand.
pub const DAMAGED_FILE: &str = "settings.damaged.json";
/// The file's own version: the wrapping, not the settings inside (those carry 0.10's).
const FORMAT: u64 = 1;

/// The privacy settings, by their 0.10 names: the two a doubt turns on, and the one it turns
/// on while Tor is.
const STREAMER: &str = "streamerMode";
const SECRETS: &str = "hideSecrets";
const TYPING: &str = "torHideTyping";

/// Settings 1.0 has that 0.10 didn't. They live beside 0.10's in the file, so 0.10's shape
/// stays exactly 0.10's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Additions {
    /// Every tab in the top row names its server after its own name. Off, a server is named
    /// once, before its group of tabs.
    pub tabs_name_server: bool,
    /// A program in a terminal (tmux, an agent) may put text on the clipboard (OSC 52). On by
    /// default, as 0.10 always allowed it.
    pub program_clipboard: bool,
    /// Fields a newer 1.x wrote, as saved.
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Default for Additions {
    fn default() -> Self {
        Additions { tabs_name_server: true, program_clipboard: true, extra: JsonMap::new() }
    }
}

/// Where the settings in memory came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// The copy's own file.
    File,
    /// Brought over from 0.10 at the first start of 1.0 here.
    Imported,
    /// Nothing was here before: a new install's defaults.
    Fresh,
    /// What was here couldn't be read: defaults, private.
    Doubt,
    /// No file, and the program hasn't said yet what 0.10 left ([`SettingsStore::seed`]).
    Waiting,
}

/// Privacy settings that were turned on because what was saved couldn't be trusted, for the
/// window to say so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyNotice {
    /// Why, one plain line each. Never a name, an address or a path.
    pub reasons: Vec<String>,
    /// The settings that were off and are now on, by their 0.10 names.
    pub turned_on: Vec<String>,
}

impl PrivacyNotice {
    /// One sentence for the window: what is on now and that it was a precaution.
    pub fn headline(&self) -> String {
        let name = |key: &str| match key {
            STREAMER => "Streamer mode",
            SECRETS => "Hide secrets",
            TYPING => "Hide my typing rhythm",
            other => return other.to_owned(),
        }
        .to_owned();
        let names: Vec<String> = self.turned_on.iter().map(|k| name(k)).collect();
        let what = match names.as_slice() {
            [] => return "Your settings from before couldn't be read with certainty. Your privacy settings were already on.".into(),
            [one] => format!("{one} is on"),
            [first, last] => format!("{first} and {last} are on"),
            [first @ .., last] => format!("{} and {last} are on", first.join(", ")),
        };
        format!("{what}, because your settings from before couldn't be read with certainty. Turn off what you don't need.")
    }
}

/// What the first start of 1.0 here begins from, once the program has looked.
#[derive(Debug, Clone, PartialEq)]
pub enum Seed {
    /// 0.10 never ran on this computer: a new install, its defaults, nothing in doubt.
    Fresh,
    /// What [`crate::import`] made of 0.10's settings. Its own doubts come along.
    Imported { settings: Box<Settings>, privacy: Option<PrivacyNotice> },
    /// 0.10 was here, but nothing of its settings could be had (the import didn't run, or
    /// stopped): defaults, private, and this line says why.
    Doubt(String),
}

impl From<&Imported> for Seed {
    fn from(imported: &Imported) -> Self {
        let privacy = imported.report.privacy.as_ref().map(|p| PrivacyNotice {
            reasons: p.reasons.clone(),
            turned_on: p.turned_on.iter().map(|name| (*name).to_owned()).collect(),
        });
        Seed::Imported { settings: Box::new(imported.state.settings.clone()), privacy }
    }
}

/// What opening needs to know besides the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOptions {
    /// The system this runs on: one default depends on it.
    pub platform: Platform,
    /// Tor was left on in this copy. A doubt then turns typing rhythm protection on too.
    pub tor_on: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions { platform: Platform::current(), tor_on: false }
    }
}

/// Heard after every change, with the settings as they are now. Called on the thread that
/// made the change (the window's).
pub type Listener = Box<dyn FnMut(&Settings, &Additions) + Send>;

/// A listener's ticket, to stop listening with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscription(u64);

/// The settings of a running app.
pub struct SettingsStore {
    settings: Settings,
    more: Additions,
    /// The version of 0.10's store the settings are at: ours, or a newer one a newer SSHeesh
    /// wrote, kept so its file isn't stamped older than it is.
    version: u64,
    origin: Origin,
    notice: Option<PrivacyNotice>,
    options: OpenOptions,
    /// `None`: kept in memory only (a demo, a test).
    saver: Option<Saver>,
    listeners: Vec<(u64, Listener)>,
    next_listener: u64,
    revision: u64,
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the settings themselves: a debug line may end up in a report.
        f.debug_struct("SettingsStore").field("origin", &self.origin).field("revision", &self.revision).finish_non_exhaustive()
    }
}

impl SettingsStore {
    /// Settings that live in memory and are never written: a demo's, a test's.
    pub fn in_memory(settings: Settings) -> SettingsStore {
        SettingsStore {
            settings,
            more: Additions::default(),
            version: Settings::VERSION,
            origin: Origin::Fresh,
            notice: None,
            options: OpenOptions::default(),
            saver: None,
            listeners: vec![],
            next_listener: 0,
            revision: 0,
        }
    }

    /// The settings kept in `dir` (the copy's own folder). Reads one small file and returns.
    /// With no file there, the store starts private and waits for [`SettingsStore::seed`]. A
    /// file that was in doubt is written again before this returns (the one write that isn't
    /// left to the saving thread: it must be there even if the program ends a moment later),
    /// so the next start reads it with certainty and never asks 0.10 a second time.
    pub fn open(dir: &Path, options: OpenOptions) -> SettingsStore {
        let path = dir.join(FILE);
        let mut store = SettingsStore { options, saver: Some(Saver::new(path.clone())), ..SettingsStore::in_memory(Settings::defaults(options.platform)) };
        match std::fs::read_to_string(&path) {
            Ok(text) => match read_file(&text, options.platform) {
                Ok(read) => {
                    store.settings = read.settings;
                    store.more = read.more;
                    store.version = read.version;
                    store.origin = Origin::File;
                    store.notice = read.notice;
                    if !read.doubts.is_empty() {
                        store.be_private(read.doubts);
                        store.save_now();
                    }
                }
                Err(why) => {
                    // Kept aside: the next save would otherwise write defaults over it.
                    let _ = std::fs::rename(&path, dir.join(DAMAGED_FILE));
                    store.origin = Origin::Doubt;
                    store.be_private(vec![format!("the settings file couldn't be read ({why})")]);
                    store.save_now();
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                store.origin = Origin::Waiting;
                // Not a notice yet: nothing is known to be wrong, the answer just isn't here.
                store.settings.streamer_mode = true;
                store.settings.hide_secrets = true;
            }
            // There, but it can't be opened (permissions, a folder in its place): it stays
            // where it is, and nothing is saved over it until a change is made.
            Err(_) => {
                store.origin = Origin::Doubt;
                store.be_private(vec!["the settings file couldn't be opened".into()]);
            }
        }
        store
    }

    /// What the first start begins from. Only a store that is [`Origin::Waiting`] takes it
    /// (returns false otherwise): a file that was read is never replaced by an import.
    ///
    /// A change made in the moment before the seed came is replaced with it: what 0.10 held
    /// for years outweighs a click in the first second.
    pub fn seed(&mut self, seed: Seed) -> bool {
        if self.origin != Origin::Waiting {
            return false;
        }
        self.notice = None;
        match seed {
            Seed::Fresh => {
                self.settings = Settings::defaults(self.options.platform);
                self.origin = Origin::Fresh;
            }
            Seed::Imported { settings, privacy } => {
                self.settings = *settings;
                self.origin = Origin::Imported;
                self.notice = privacy;
            }
            Seed::Doubt(why) => {
                self.settings = Settings::defaults(self.options.platform);
                self.origin = Origin::Doubt;
                self.be_private(vec![why]);
            }
        }
        self.changed();
        true
    }

    /// Turns the privacy settings on and notes why, adding to a notice already there.
    fn be_private(&mut self, reasons: Vec<String>) {
        let mut turned_on = vec![];
        let mut switch = |name: &str, setting: &mut bool| {
            if !*setting {
                *setting = true;
                turned_on.push(name.to_owned());
            }
        };
        switch(STREAMER, &mut self.settings.streamer_mode);
        switch(SECRETS, &mut self.settings.hide_secrets);
        if self.options.tor_on {
            switch(TYPING, &mut self.settings.tor_hide_typing);
        }
        match &mut self.notice {
            Some(notice) => {
                notice.reasons.extend(reasons);
                notice.turned_on.extend(turned_on);
            }
            None => self.notice = Some(PrivacyNotice { reasons, turned_on }),
        }
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// The settings 1.0 added.
    pub fn more(&self) -> &Additions {
        &self.more
    }

    /// Selecting text in a terminal copies it (0.10's `copyOnSelect`, on by default). Off,
    /// only the copy keys do: Ctrl+Shift+C, or ⌘C on a Mac.
    pub fn copy_on_select(&self) -> bool {
        self.settings.copy_on_select
    }

    /// A program in a terminal may put text on the clipboard (OSC 52 from tmux or an agent).
    /// On by default.
    pub fn program_clipboard(&self) -> bool {
        self.more.program_clipboard
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Counts changes: a view that drew at one revision is current while it stays.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Set while privacy settings are on as a precaution and nobody has said "I know".
    pub fn notice(&self) -> Option<&PrivacyNotice> {
        self.notice.as_ref()
    }

    /// "I know": the notice goes; the settings stay as they are now.
    pub fn dismiss_notice(&mut self) {
        if self.notice.take().is_some() {
            self.revision += 1;
            self.save();
        }
    }

    /// Changes settings. Listeners hear and the file is written only if something changed.
    /// Returns whether it did.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> bool {
        let before = self.settings.clone();
        change(&mut self.settings);
        let changed = self.settings != before;
        if changed {
            self.changed();
        }
        changed
    }

    /// [`SettingsStore::update`] for a caller that doesn't ask whether anything changed: a
    /// control's setter.
    pub fn set(&mut self, change: impl FnOnce(&mut Settings)) {
        self.update(change);
    }

    /// [`SettingsStore::update_more`], likewise.
    pub fn set_more(&mut self, change: impl FnOnce(&mut Additions)) {
        self.update_more(change);
    }

    /// [`SettingsStore::update`] for the settings 1.0 added.
    pub fn update_more(&mut self, change: impl FnOnce(&mut Additions)) -> bool {
        let before = self.more.clone();
        change(&mut self.more);
        let changed = self.more != before;
        if changed {
            self.changed();
        }
        changed
    }

    /// Puts settings back to how a fresh install has them, by their names in the file
    /// (0.10's `terminalFontSize`, 1.0's `tabsNameServer`): what a section's Reset does. Names
    /// nobody knows are skipped.
    pub fn reset(&mut self, keys: &[&str]) -> bool {
        /// `now` with `keys` taken from `defaults`, where both have them.
        fn put_back(now: Json, defaults: Json, keys: &[&str]) -> Option<Json> {
            let (Json::Object(mut now), Json::Object(defaults)) = (now, defaults) else { return None };
            for key in keys {
                if let Some(value) = defaults.get(*key) {
                    now.insert((*key).to_owned(), value.clone());
                }
            }
            Some(Json::Object(now))
        }
        // Both sides were typed a moment ago, so each mix reads.
        let settings = put_back(Json::of(&self.settings), Json::of(&Settings::defaults(self.options.platform)), keys).and_then(|j| j.typed::<Settings>().ok());
        let more = put_back(Json::of(&self.more), Json::of(&Additions::default()), keys).and_then(|j| j.typed::<Additions>().ok());
        let (Some(settings), Some(more)) = (settings, more) else { return false };
        if settings == self.settings && more == self.more {
            return false;
        }
        self.settings = settings;
        self.more = more;
        self.changed();
        true
    }

    /// Hears every change from now on.
    pub fn subscribe(&mut self, listener: Listener) -> Subscription {
        self.next_listener += 1;
        self.listeners.push((self.next_listener, listener));
        Subscription(self.next_listener)
    }

    pub fn unsubscribe(&mut self, subscription: Subscription) {
        self.listeners.retain(|(id, _)| *id != subscription.0);
    }

    fn changed(&mut self) {
        self.revision += 1;
        for (_, listener) in &mut self.listeners {
            listener(&self.settings, &self.more);
        }
        self.save();
    }

    /// Hands the file's new text to the thread that writes it. While the store waits for its
    /// seed nothing is written: the file's absence is how the next start knows to ask again.
    fn save(&self) {
        if self.origin == Origin::Waiting {
            return;
        }
        if let Some(saver) = &self.saver {
            saver.write(self.file_text().into_bytes());
        }
    }

    /// Writes the file on this thread, before returning: only where it can't wait (see
    /// [`SettingsStore::open`]).
    fn save_now(&self) {
        if let Some(saver) = &self.saver {
            saver.write_now(&self.file_text().into_bytes());
        }
    }

    /// The file as it would be written now.
    pub fn file_text(&self) -> String {
        let mut saved = JsonMap::new();
        saved.insert("state".into(), self.settings.to_state());
        saved.insert("version".into(), Json::from(self.version));
        let mut file = JsonMap::new();
        file.insert("format".into(), Json::from(FORMAT));
        file.insert("settings".into(), Json::Object(saved));
        file.insert("more".into(), Json::of(&self.more));
        file.insert("from".into(), Json::of(&self.origin));
        if let Some(notice) = &self.notice {
            file.insert("notice".into(), Json::of(notice));
        }
        Json::Object(file).to_text()
    }

    /// Waits until every change so far is in the file (or has failed to get there). For
    /// leaving the program, and for tests.
    pub fn flush(&self) {
        if let Some(saver) = &self.saver {
            saver.flush();
        }
    }

    /// Why the last save didn't reach the file, if it didn't. One plain line, no path.
    pub fn save_error(&self) -> Option<String> {
        self.saver.as_ref().and_then(Saver::error)
    }
}

/// What a settings file held.
struct ReadFile {
    settings: Settings,
    more: Additions,
    version: u64,
    notice: Option<PrivacyNotice>,
    /// Why the privacy settings in it can't be trusted, if they can't.
    doubts: Vec<String>,
}

/// Reads the copy's own file. `Err`: nothing in it can be trusted, with why in a few words.
fn read_file(text: &str, platform: Platform) -> Result<ReadFile, String> {
    let Json::Object(mut file) = Json::parse(text).map_err(|_| "it isn't JSON".to_owned())? else {
        return Err("it isn't an object".into());
    };
    let Some(Json::Object(mut saved)) = file.shift_remove("settings") else { return Err("it has no settings".into()) };
    let Some(Json::Object(state)) = saved.shift_remove("state") else { return Err("its settings have no state".into()) };
    let found = saved.get("version").and_then(Json::as_u64);
    let mut doubts = vec![];
    let mut problems = vec![];
    // The file's own settings are always written at the current version or by a newer
    // SSHeesh; an older number means somebody put 0.10's store here by hand, which reads too.
    let (state, version) = match found {
        Some(v) if v > Settings::VERSION => {
            doubts.push(format!("the settings were saved by a newer SSHeesh (version {v})"));
            (state, v)
        }
        Some(v) if v < Settings::VERSION => (Settings::migrate(state, v, platform), Settings::VERSION),
        Some(v) => (state, v),
        None => {
            doubts.push("the settings have no version number".into());
            (state, Settings::VERSION)
        }
    };
    for name in [STREAMER, SECRETS] {
        if !state.contains_key(name) {
            doubts.push(format!("the privacy setting {name} was missing"));
        }
    }
    let settings = Settings::from_state(&Json::Object(state), platform, &mut problems);
    if problems.iter().any(|p| [STREAMER, SECRETS, TYPING].contains(&p.at.as_str())) {
        doubts.push("a privacy setting couldn't be read".into());
    }
    let more = file.get("more").and_then(|more| crate::model::json::lenient::<Additions>(more, &mut vec![])).unwrap_or_default();
    let notice = file.get("notice").and_then(|n| n.typed::<PrivacyNotice>().ok());
    Ok(ReadFile { settings, more, version, notice, doubts })
}

/// Writes `bytes` to `path`, whole or not at all: to a file beside it first, synced, then
/// over the old one. The file is the user's alone (names of themes and words to highlight are
/// their own business).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(FILE);
    let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// What the store and its writing thread share.
#[derive(Default)]
struct Desk {
    /// The newest text nobody has written yet. A newer one replaces it: only the last counts.
    waiting: Option<Vec<u8>>,
    /// A write is under way.
    writing: bool,
    /// The store is gone: write what waits and end.
    closing: bool,
    error: Option<String>,
    /// The thread is running.
    started: bool,
}

/// Writes the file away from the window's thread (a sync to disk can take a moment), one
/// write at a time, always the newest text. The thread starts with the first save.
struct Saver {
    path: PathBuf,
    desk: Arc<(Mutex<Desk>, Condvar)>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Saver {
    fn new(path: PathBuf) -> Saver {
        Saver { path, desk: Arc::new((Mutex::new(Desk::default()), Condvar::new())), thread: Mutex::new(None) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Desk> {
        // A writer that panicked left nothing half-changed here: carry on with what there is.
        self.desk.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self, bytes: Vec<u8>) {
        let start = {
            let mut desk = self.lock();
            desk.waiting = Some(bytes);
            !std::mem::replace(&mut desk.started, true)
        };
        if start {
            let (path, shared) = (self.path.clone(), self.desk.clone());
            let spawned = std::thread::Builder::new().name("settings-save".into()).spawn(move || write_loop(&path, &shared));
            match spawned {
                Ok(handle) => *self.thread.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle),
                Err(_) => {
                    // No thread to be had: write here, late rather than never.
                    let mut desk = self.lock();
                    desk.started = false;
                    if let Some(bytes) = desk.waiting.take() {
                        desk.error = write_atomic(&self.path, &bytes).err().map(|e| plain_error(&e));
                    }
                }
            }
        }
        self.desk.1.notify_all();
    }

    /// Writes on the caller's thread. Only before the saving thread has anything to do.
    fn write_now(&self, bytes: &[u8]) {
        let result = write_atomic(&self.path, bytes);
        self.lock().error = result.err().map(|e| plain_error(&e));
    }

    fn flush(&self) {
        let mut desk = self.lock();
        while desk.started && (desk.waiting.is_some() || desk.writing) {
            desk = self.desk.1.wait(desk).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        self.lock().closing = true;
        self.desk.1.notify_all();
        if let Some(handle) = self.thread.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

fn write_loop(path: &Path, shared: &(Mutex<Desk>, Condvar)) {
    let (lock, wake) = shared;
    let mut desk = lock.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        match desk.waiting.take() {
            Some(bytes) => {
                desk.writing = true;
                drop(desk);
                let result = write_atomic(path, &bytes);
                desk = lock.lock().unwrap_or_else(|p| p.into_inner());
                desk.writing = false;
                desk.error = result.err().map(|e| plain_error(&e));
                wake.notify_all();
            }
            None if desk.closing => return,
            None => desk = wake.wait(desk).unwrap_or_else(|p| p.into_inner()),
        }
    }
}

/// An I/O error in words that name no path.
fn plain_error(e: &std::io::Error) -> String {
    format!("the settings couldn't be saved ({})", e.kind())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import, ImportOptions};
    use crate::model::{CursorStyle, Keys, Side};
    use crate::raw::{RawSource, RawWebState};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder of this test's own under the build's temporary folder.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn linux() -> OpenOptions {
        OpenOptions { platform: Platform::Linux, tor_on: false }
    }

    /// A store that already has its seed: a new install.
    fn fresh(dir: &Path) -> SettingsStore {
        let mut store = SettingsStore::open(dir, linux());
        assert!(store.seed(Seed::Fresh));
        store
    }

    fn file(dir: &Path) -> Json {
        Json::parse(&std::fs::read_to_string(dir.join(FILE)).unwrap()).unwrap()
    }

    #[test]
    fn every_0_10_setting_is_there_with_its_default() {
        let store = SettingsStore::in_memory(Settings::defaults(Platform::Linux));
        let Json::Object(state) = store.get().to_state() else { panic!() };
        // The sixty keys of 0.10.14's DEFAULT_SETTINGS (src/lib/settings.ts), in its order.
        let keys: Vec<&str> = state.keys().map(String::as_str).collect();
        assert_eq!(keys.len(), 60, "{keys:?}");
        for (key, value) in [
            ("theme", r#""ssheesh""#),
            ("themeFollowsSystem", "false"),
            ("uiZoom", "100"),
            ("terminalFontSize", "13"),
            ("terminalLineHeight", "1.22"),
            ("cursorStyle", r#""bar""#),
            ("cursorBlink", "true"),
            ("gpuRendering", "false"),
            ("sidebarSide", r#""left""#),
            ("sidebarMode", r#""narrow""#),
            ("streamerMode", "false"),
            ("hideSecrets", "true"),
            ("cleanUploads", "true"),
            ("torHideTyping", "false"),
            ("updateMode", r#""auto""#),
            ("idleSleep", "20"),
            ("welcomeParts", r#"["logo","system","usage","status"]"#),
            ("shortcuts", "{}"),
            ("nativeTitleBar", "false"),
        ] {
            assert_eq!(state.get(key).map(Json::to_text).as_deref(), Some(value), "{key}");
        }
        // And what 1.0 added: tabs name their server unless told not to.
        assert!(store.more().tabs_name_server);
    }

    #[test]
    fn copying_is_two_plain_answers_for_the_terminal() {
        // As 0.10 came: selecting copies, and a program may copy too.
        let mut store = SettingsStore::in_memory(Settings::defaults(Platform::Linux));
        assert!(store.copy_on_select() && store.program_clipboard());
        // "Only with Ctrl+Shift+C" is 0.10's `copyOnSelect` off: the key and its meaning stay.
        store.set(|s| s.copy_on_select = false);
        assert!(!store.copy_on_select() && store.program_clipboard());
        store.set_more(|m| m.program_clipboard = false);
        assert!(!store.program_clipboard());
        let Json::Object(file) = Json::parse(&store.file_text()).unwrap() else { panic!() };
        assert_eq!(file["settings"].as_object().unwrap()["state"].as_object().unwrap()["copyOnSelect"], Json::Bool(false));
        assert_eq!(file["more"].as_object().unwrap()["programClipboard"], Json::Bool(false));
        // A section's Reset puts both back, by their names in the file.
        assert!(store.reset(&["copyOnSelect", "programClipboard"]));
        assert!(store.copy_on_select() && store.program_clipboard());

        // 0.10's choice comes over with the import, and survives a restart.
        let settings = r#"{"state":{"copyOnSelect":false,"streamerMode":false,"hideSecrets":true},"version":4}"#;
        let workspace = r#"{"state":{},"version":2}"#;
        let imported = import(&raw(&[("sshee-settings", settings), ("sshee-workspace", workspace)]), &ImportOptions { platform: Platform::Linux, tor_on: false });
        let dir = folder("copying");
        let mut store = SettingsStore::open(&dir, linux());
        store.seed(Seed::from(&imported));
        assert!(!store.copy_on_select() && store.program_clipboard());
        store.set_more(|m| m.program_clipboard = false);
        drop(store);
        let again = SettingsStore::open(&dir, linux());
        assert!(!again.copy_on_select() && !again.program_clipboard());
        // A file from before the switch existed has no `programClipboard`: allowed, as it was.
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap().replace(r#""programClipboard":false,"#, "").replace(r#","programClipboard":false"#, "");
        assert!(!text.contains("programClipboard"));
        std::fs::write(dir.join(FILE), text).unwrap();
        assert!(SettingsStore::open(&dir, linux()).program_clipboard());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_start_waits_private_and_writes_nothing_until_it_is_seeded() {
        let dir = folder("waiting");
        let mut store = SettingsStore::open(&dir, linux());
        assert_eq!(store.origin(), Origin::Waiting);
        // Not known yet what 0.10 left: names and secrets are hidden meanwhile, without a
        // notice (nothing is known to be wrong).
        assert!(store.get().streamer_mode && store.get().hide_secrets);
        assert_eq!(store.notice(), None);
        store.update(|s| s.terminal_font_size = 15.0);
        store.flush();
        assert!(!dir.join(FILE).exists(), "a file written now would stop the next start from asking 0.10");
        // A new install: the defaults, streamer mode off as 0.10 has it.
        assert!(store.seed(Seed::Fresh));
        store.flush();
        assert_eq!((store.origin(), store.get().streamer_mode, store.get().hide_secrets), (Origin::Fresh, false, true));
        assert_eq!(store.get().terminal_font_size, 13.0);
        assert_eq!(store.notice(), None);
        assert_eq!(file(&dir).as_object().unwrap()["from"], Json::from("fresh"));
        // Seeded once: an import that comes late changes nothing.
        assert!(!store.seed(Seed::Doubt("late".into())));
        assert!(!store.get().streamer_mode);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changes_are_saved_whole_and_read_back() {
        let dir = folder("round");
        let mut store = fresh(&dir);
        assert!(store.update(|s| {
            s.terminal_font_size = 16.0;
            s.cursor_style = CursorStyle::Block;
            s.sidebar_side = Side::Right;
            s.shortcuts.insert("newTerminal".into(), Some(Keys { key: "n".into(), ctrl: Some(true), shift: Some(true), ..Keys::default() }));
            s.extra.insert("placesWidth".into(), Json::from(180u64));
        }));
        assert!(store.update_more(|m| m.tabs_name_server = false));
        // The same value again is no change: nobody hears, nothing is written.
        let revision = store.revision();
        assert!(!store.update(|s| s.terminal_font_size = 16.0));
        assert_eq!(store.revision(), revision);
        store.flush();
        assert_eq!(store.save_error(), None);
        // 0.10's own shape inside, so its reader (and a rollback) understands it.
        let saved = file(&dir);
        let settings = saved.as_object().unwrap()["settings"].as_object().unwrap();
        assert_eq!(settings["version"], Json::from(4u64));
        assert_eq!(settings["state"].as_object().unwrap()["terminalFontSize"].to_text(), "16");
        // Only the file is in the folder: the one written beside it took its place.
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from(FILE)]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(store);
        let again = SettingsStore::open(&dir, linux());
        assert_eq!(again.origin(), Origin::File);
        assert_eq!(again.notice(), None);
        let s = again.get();
        assert_eq!((s.terminal_font_size, s.cursor_style.as_str(), s.sidebar_side.as_str()), (16.0, "block", "right"));
        assert_eq!(s.shortcuts["newTerminal"].as_ref().unwrap().key, "n");
        // A field this version doesn't know came back as saved.
        assert_eq!(s.extra["placesWidth"], Json::from(180u64));
        assert!(!again.more().tabs_name_server);
        // Privacy as it was left: read with certainty, so nothing is forced.
        assert!(!s.streamer_mode && s.hide_secrets);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn many_quick_changes_end_with_the_last_one_in_the_file() {
        let dir = folder("burst");
        let mut store = fresh(&dir);
        // A slider being dragged.
        for size in 0..200 {
            store.update(|s| s.terminal_padding = f64::from(size));
        }
        store.flush();
        let saved = file(&dir);
        let state = saved.as_object().unwrap()["settings"].as_object().unwrap()["state"].as_object().unwrap();
        assert_eq!(state["terminalPadding"].to_text(), "199");
        // Dropping the store writes what still waits.
        store.update(|s| s.terminal_padding = 7.0);
        drop(store);
        assert_eq!(SettingsStore::open(&dir, linux()).get().terminal_padding, 7.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn listeners_hear_each_change_until_they_stop_listening() {
        let mut store = SettingsStore::in_memory(Settings::defaults(Platform::Linux));
        let heard = Arc::new(AtomicUsize::new(0));
        let sizes = Arc::new(Mutex::new(vec![]));
        let (h, z) = (heard.clone(), sizes.clone());
        let ticket = store.subscribe(Box::new(move |s, more| {
            h.fetch_add(1, Ordering::SeqCst);
            z.lock().unwrap().push((s.terminal_font_size, more.tabs_name_server));
        }));
        store.update(|s| s.terminal_font_size = 14.0);
        store.update(|s| s.terminal_font_size = 14.0);
        store.update_more(|m| m.tabs_name_server = false);
        assert_eq!(heard.load(Ordering::SeqCst), 2);
        assert_eq!(*sizes.lock().unwrap(), [(14.0, true), (14.0, false)]);
        store.unsubscribe(ticket);
        store.update(|s| s.terminal_font_size = 15.0);
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_sections_reset_puts_only_its_settings_back() {
        let mut store = SettingsStore::in_memory(Settings::defaults(Platform::Linux));
        store.update(|s| {
            s.terminal_font_size = 18.0;
            s.cursor_blink = false;
            s.sidebar_side = Side::Right;
            s.streamer_mode = true;
        });
        assert!(store.reset(&["terminalFontSize", "cursorBlink", "nobodyKnowsThis"]));
        let s = store.get();
        assert_eq!((s.terminal_font_size, s.cursor_blink), (13.0, true));
        // What the section doesn't own stays.
        assert_eq!((s.sidebar_side.as_str(), s.streamer_mode), ("right", true));
        assert!(!store.reset(&["terminalFontSize"]), "already at its default");
    }

    #[test]
    fn a_file_that_does_not_read_starts_private_says_so_and_is_kept_aside() {
        for (name, text) in [
            ("cut", r#"{"format":1,"settings":{"state":{"streamerMode":fal"#),
            ("empty", ""),
            ("list", "[1,2,3]"),
            ("no-settings", r#"{"format":1}"#),
            ("no-state", r#"{"format":1,"settings":{"version":4}}"#),
        ] {
            let dir = folder(&format!("bad-{name}"));
            std::fs::write(dir.join(FILE), text).unwrap();
            let store = SettingsStore::open(&dir, linux());
            assert_eq!(store.origin(), Origin::Doubt, "{name}");
            assert!(store.get().streamer_mode && store.get().hide_secrets, "{name}");
            let notice = store.notice().unwrap_or_else(|| panic!("{name}: nothing says why"));
            // hideSecrets is on by default, so only streamer mode was turned on.
            assert_eq!(notice.turned_on, [STREAMER], "{name}");
            assert!(notice.headline().starts_with("Streamer mode is on, because"), "{name}: {}", notice.headline());
            // The new file is there already, without waiting for the saving thread: a program
            // that ends now still starts private, and certain, next time.
            assert!(dir.join(FILE).is_file(), "{name}");
            // What was there is beside the new file, as it was.
            assert_eq!(std::fs::read_to_string(dir.join(DAMAGED_FILE)).unwrap(), text, "{name}");
            // The next start reads the new file with certainty, and still says why.
            drop(store);
            let mut again = SettingsStore::open(&dir, linux());
            assert_eq!(again.origin(), Origin::File, "{name}");
            assert!(again.get().streamer_mode && again.notice().is_some(), "{name}");
            // "I know", and streamer mode off again: both stick.
            again.dismiss_notice();
            again.update(|s| s.streamer_mode = false);
            drop(again);
            let last = SettingsStore::open(&dir, linux());
            assert!(last.notice().is_none() && !last.get().streamer_mode, "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn each_privacy_setting_that_is_missing_or_not_a_switch_starts_private() {
        let whole = || {
            let Json::Object(mut state) = Settings::defaults(Platform::Linux).to_state() else { panic!() };
            state.insert(STREAMER.into(), Json::Bool(false));
            state.insert(SECRETS.into(), Json::Bool(false));
            state.insert(TYPING.into(), Json::Bool(false));
            state
        };
        let write = |dir: &Path, state: JsonMap, version: Option<u64>| {
            let mut saved = JsonMap::new();
            saved.insert("state".into(), Json::Object(state));
            if let Some(v) = version {
                saved.insert("version".into(), Json::from(v));
            }
            let mut file = JsonMap::new();
            file.insert("format".into(), Json::from(1u64));
            file.insert("settings".into(), Json::Object(saved));
            std::fs::write(dir.join(FILE), Json::Object(file).to_text()).unwrap();
        };
        // Read with certainty: both stay off, as saved.
        let dir = folder("privacy-whole");
        write(&dir, whole(), Some(4));
        let store = SettingsStore::open(&dir, linux());
        assert!(!store.get().streamer_mode && !store.get().hide_secrets && store.notice().is_none());
        let _ = std::fs::remove_dir_all(&dir);

        type Damage = fn(&mut JsonMap);
        let cases: [(&str, Damage, Option<u64>); 6] = [
            ("streamer-missing", |s| drop(s.shift_remove(STREAMER)), Some(4)),
            ("secrets-missing", |s| drop(s.shift_remove(SECRETS)), Some(4)),
            ("streamer-text", |s| drop(s.insert(STREAMER.into(), Json::from("no"))), Some(4)),
            ("secrets-number", |s| drop(s.insert(SECRETS.into(), Json::from(0u64))), Some(4)),
            ("newer-version", |_| {}, Some(9)),
            ("no-version", |_| {}, None),
        ];
        for (name, damage, version) in cases {
            let dir = folder(&format!("privacy-{name}"));
            let mut state = whole();
            damage(&mut state);
            write(&dir, state, version);
            let store = SettingsStore::open(&dir, OpenOptions { platform: Platform::Linux, tor_on: true });
            let s = store.get();
            assert!(s.streamer_mode && s.hide_secrets, "{name}");
            // Tor was left on: typing rhythm protection too.
            assert!(s.tor_hide_typing, "{name}");
            let notice = store.notice().unwrap_or_else(|| panic!("{name}: nothing says why"));
            assert!(!notice.reasons.is_empty(), "{name}");
            for turned in [STREAMER, TYPING] {
                assert!(notice.turned_on.iter().any(|t| t == turned), "{name}: {turned} isn't listed in {:?}", notice.turned_on);
            }
            // The file was readable, so it isn't moved aside; the rest of it is kept.
            assert!(!dir.join(DAMAGED_FILE).exists(), "{name}");
            assert_eq!(s.terminal_font_size, 13.0, "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        // Without Tor, typing rhythm protection is left as it was.
        let dir = folder("privacy-no-tor");
        let mut state = whole();
        state.shift_remove(STREAMER);
        write(&dir, state, Some(4));
        let store = SettingsStore::open(&dir, linux());
        assert!(store.get().streamer_mode && !store.get().tor_hide_typing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_setting_outside_privacy_that_does_not_fit_takes_its_default_alone() {
        let dir = folder("one-bad");
        let Json::Object(mut state) = Settings::defaults(Platform::Linux).to_state() else { panic!() };
        state.insert("terminalFontSize".into(), Json::from("huge"));
        state.insert("cursorStyle".into(), Json::from("block"));
        let text = format!(r#"{{"format":1,"settings":{{"state":{},"version":4}}}}"#, Json::Object(state).to_text());
        std::fs::write(dir.join(FILE), text).unwrap();
        let store = SettingsStore::open(&dir, linux());
        assert_eq!((store.get().terminal_font_size, store.get().cursor_style.as_str()), (13.0, "block"));
        // No doubt about privacy: nothing is forced, nothing is said.
        assert!(!store.get().streamer_mode && store.notice().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn raw(stores: &[(&str, &str)]) -> RawWebState {
        let stores: BTreeMap<String, String> = stores.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        RawWebState { stores, window_state: None, source: RawSource::Fixture }
    }

    #[test]
    fn the_first_start_takes_what_the_import_brought_and_its_doubts() {
        // 0.10's stores, read cleanly: its settings come over as they were.
        let settings = r#"{"state":{"theme":"daylight","terminalFontSize":15,"streamerMode":false,"hideSecrets":false,"cursorStyle":"underline"},"version":4}"#;
        let workspace = r#"{"state":{},"version":2}"#;
        let imported = import(&raw(&[("sshee-settings", settings), ("sshee-workspace", workspace)]), &ImportOptions { platform: Platform::Linux, tor_on: false });
        assert_eq!(imported.report.privacy, None, "{:?}", imported.report);
        let dir = folder("import-clean");
        let mut store = SettingsStore::open(&dir, linux());
        assert!(store.seed(Seed::from(&imported)));
        assert_eq!(store.origin(), Origin::Imported);
        let s = store.get();
        assert_eq!((s.theme.as_str(), s.terminal_font_size, s.cursor_style.as_str()), ("daylight", 15.0, "underline"));
        assert!(!s.streamer_mode && !s.hide_secrets && store.notice().is_none());
        drop(store);
        // The second start reads the file; 0.10 isn't asked again.
        let again = SettingsStore::open(&dir, linux());
        assert_eq!((again.origin(), again.get().terminal_font_size), (Origin::File, 15.0));
        let _ = std::fs::remove_dir_all(&dir);

        // In doubt (no tabs were found): what could be read comes over, private, and it says so.
        let doubtful = import(&raw(&[("sshee-settings", settings)]), &ImportOptions { platform: Platform::Linux, tor_on: false });
        let dir = folder("import-doubt");
        let mut store = SettingsStore::open(&dir, linux());
        store.seed(Seed::from(&doubtful));
        let s = store.get();
        assert!(s.streamer_mode && s.hide_secrets);
        assert_eq!(s.terminal_font_size, 15.0);
        let notice = store.notice().expect("the doubt is told");
        assert_eq!(notice.turned_on, [STREAMER, SECRETS]);
        assert_eq!(notice.headline(), "Streamer mode and Hide secrets are on, because your settings from before couldn't be read with certainty. Turn off what you don't need.");
        drop(store);
        // And it still says so after a restart, until somebody says "I know".
        assert!(SettingsStore::open(&dir, linux()).notice().is_some());
        let _ = std::fs::remove_dir_all(&dir);

        // 0.10 was here and nothing could be had of it.
        let dir = folder("import-none");
        let mut store = SettingsStore::open(&dir, OpenOptions { platform: Platform::Linux, tor_on: true });
        store.seed(Seed::Doubt("the settings of 0.10 couldn't be looked for".into()));
        assert_eq!(store.origin(), Origin::Doubt);
        assert!(store.get().streamer_mode && store.get().hide_secrets && store.get().tor_hide_typing);
        assert_eq!(store.notice().unwrap().turned_on, [STREAMER, TYPING]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_that_fails_is_told_without_a_path() {
        let dir = folder("unwritable");
        // A folder where the file should be: the rename can't land.
        std::fs::create_dir_all(dir.join(FILE)).unwrap();
        let mut store = SettingsStore::open(&dir, linux());
        // It couldn't be opened as a file: private, and the folder is left where it is.
        assert_eq!(store.origin(), Origin::Doubt);
        store.update(|s| s.terminal_font_size = 14.0);
        store.flush();
        let error = store.save_error().expect("the failure is kept");
        assert!(error.starts_with("the settings couldn't be saved"), "{error}");
        assert!(!error.contains(dir.to_str().unwrap()), "{error}");
        assert!(dir.join(FILE).is_dir());
        // In memory the change stands.
        assert_eq!(store.get().terminal_font_size, 14.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_in_memory_keeps_nothing_on_disk() {
        let mut store = SettingsStore::in_memory(Settings::defaults(Platform::Mac));
        assert!(store.get().gpu_rendering);
        store.update(|s| s.streamer_mode = true);
        store.flush();
        assert_eq!(store.save_error(), None);
        assert!(format!("{store:?}").contains("Fresh"));
    }
}
