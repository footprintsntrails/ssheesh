//! Bringing 0.10's stores over: a [`RawWebState`] into an [`AppState`], store by store.
//!
//! A store that's broken never takes the others with it: it falls back to its defaults, and
//! a broken field or list item falls back alone. A store saved by a version newer than this
//! code is read as well as it can be, and it and a store that couldn't be read at all are
//! kept as found ([`AppState::kept`]), so writing back gives 0.10 what it had.
//!
//! Two rules on top:
//!
//! - **No tmux session is lost.** Every `sessionKey` in the raw workspace is checked to be in
//!   the typed workspace, the one 1.x reattaches from, and the report lists every one that
//!   isn't. A workspace too damaged to parse is searched as text, so its sessions are listed
//!   too.
//! - **When in doubt, private.** If the settings or the tabs aren't there, or any store can't
//!   be read, comes from a newer version or has a version no release wrote, or a privacy
//!   setting can't be read or is missing, or (from [`import_found`]) the web view's storage
//!   wasn't read cleanly, streamer mode and secret hiding start on, and with Tor left on,
//!   typing rhythm protection too. [`ImportReport::privacy`] says why. Other stores missing
//!   is no doubt: 0.10 saves a store only after its first change.

use crate::legacy::{Found, LegacyError};
use crate::model::{
    session_keys_in_text, sessions_in, AppState, Bookmarks, Json, JsonMap, Labels, Platform, Problem, SavedTunnels, SessionRef, Settings,
    Snippets, Store, TypingCounts, Workspace,
};
use crate::raw::{RawWebState, STORE_KEYS};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// What the import needs to know besides the stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOptions {
    /// The system 0.10 ran on: a few defaults and one migration depend on it.
    pub platform: Platform,
    /// Tor was left on in 0.10 (`~/.sshee/tor/on` exists). If the import is in doubt,
    /// typing rhythm protection goes on as well.
    pub tor_on: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { platform: Platform::current(), tor_on: false }
    }
}

/// The typed state and what happened on the way.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub state: AppState,
    pub report: ImportReport,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    /// One per store, in [`STORE_KEYS`] order.
    pub stores: Vec<StoreReport>,
    /// Other keys that were among the raw stores, left alone.
    pub ignored: Vec<String>,
    /// Every pane's session that came over, in pane order. The `durable` ones are tmux
    /// sessions 1.x reattaches to; the others were terminals on this computer, which ended
    /// with 0.10.
    pub sessions: Vec<SessionRef>,
    /// Sessions in 0.10's raw workspace that the typed workspace doesn't have, with their pane
    /// and machine where those could be read, for the tmux search to look for. Empty when the
    /// workspace came over whole: a broken pane keeps its session, so only a workspace that
    /// couldn't be read at all, one from a newer version, or a bug here leaves any out.
    pub lost_sessions: Vec<SessionRef>,
    /// Set when anything was in doubt: why, and what was turned on for it.
    pub privacy: Option<PrivacyOverride>,
}

impl ImportReport {
    /// The report for one store, by key.
    pub fn store(&self, key: &str) -> Option<&StoreReport> {
        self.stores.iter().find(|s| s.key == key)
    }
}

/// What happened to one store.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreReport {
    pub key: &'static str,
    pub outcome: Outcome,
    /// Fields and items that couldn't be read as they were: left out, or mended.
    pub problems: Vec<Problem>,
    /// What came over, in counts: never names, addresses or commands.
    pub summary: String,
    /// Why this store puts the import in doubt, if it does.
    pub doubt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Read as saved, at `version`.
    Read { version: u64 },
    /// Saved by an older version and brought up to date as 0.10.14 does.
    Migrated { from: u64, to: u64 },
    /// Not there. 0.10 saves a store only after its first change, so for most stores
    /// that's normal: defaults, as 0.10 would have.
    Missing,
    /// There, but not readable at all: defaults instead, and kept as found for writing back.
    Unreadable { why: String },
    /// Saved by a version newer than this code: read as well as it could be, and kept as
    /// found for writing back.
    Newer { version: u64 },
}

/// Privacy settings turned on because the import was in doubt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyOverride {
    /// Why, one line each.
    pub reasons: Vec<String>,
    /// The settings that were off and are now on, by their 0.10 names.
    pub turned_on: Vec<&'static str>,
}

/// Settings whose loss would show what someone meant to hide.
const PRIVACY_FIELDS: [&str; 3] = ["streamerMode", "hideSecrets", "torHideTyping"];

/// Privacy settings every save of the settings has had since a version of the store. 0.10
/// saves the whole store each time, so a state at that version or later without one has lost
/// it. `streamerMode` came in 0.3.0, before version 2 (0.6.8); `hideSecrets` came in 0.7.0 with
/// version 3. `torHideTyping` came in 0.10.4 without a new version, so a version 4 store left by
/// 0.8.0 to 0.10.3 rightly has none.
const PRIVACY_SINCE: [(&str, u64); 2] = [("streamerMode", 2), ("hideSecrets", 3)];

/// Stores whose absence puts the import in doubt, with what the report says. 0.10 saves a
/// store only after its first change, so the others are often missing for good reason
/// (typing counts are off by default, and few people make tunnels). Settings change on the
/// first launch's hello, and the main window saves its tabs every time it starts: without
/// them, the storage that was found isn't the whole story.
const MUST_BE_THERE: [(&str, &str); 2] = [(Settings::KEY, "no settings were found"), (Workspace::KEY, "no tabs were found")];

/// Turns 0.10's raw stores into the typed state. Never fails: whatever couldn't be read is
/// in the report, and the rest came over.
pub fn import(raw: &RawWebState, options: &ImportOptions) -> Imported {
    run(&raw.stores, options, &Leftovers::default())
}

/// Brings over what [`crate::legacy::find`] turned up: its best store, as [`import`] does, with
/// what reading the web view's storage couldn't settle counted as doubts as well (plan, section
/// 6.5: a store that's incomplete or unreadable is a doubt). A value that was there but
/// couldn't be decoded is unreadable, not missing, and isn't written back over
/// ([`AppState::undecoded`]). The best store being a developer build's, or being written to
/// while it was copied, damage that reading went past, and any store that couldn't be read
/// put the import in doubt: anything [`Found::is_complete`] counts against the reading.
pub fn import_found(found: &Found, options: &ImportOptions) -> Imported {
    let best = found.best();
    let mut leftovers = Leftovers::default();
    if let Some(best) = best {
        leftovers.undecoded = best.unreadable_keys.clone();
        if !best.is_app_origin() {
            leftovers.doubts.push(
                match best.origin {
                    Some(_) => "the storage found is a developer build's",
                    None => "the storage's origin couldn't be read",
                }
                .to_owned(),
            );
        }
        if !best.settled {
            leftovers.doubts.push("0.10 was writing to its storage while it was read".into());
        }
    }
    // Counted, not named: the reasons show in a report, and paths name the user.
    let damaged = found.problems.iter().filter(|p| matches!(p, LegacyError::ReadPastDamage { .. })).count();
    if damaged > 0 {
        leftovers.doubts.push(format!("{damaged} web view storage(s) had damage that reading went past"));
    }
    if found.problems.len() > damaged {
        leftovers.doubts.push(format!("{} web view storage(s) couldn't be read", found.problems.len() - damaged));
    }
    // Whatever else `legacy` counts against a reading counts here too.
    if best.is_some() && !found.is_complete() && leftovers.undecoded.is_empty() && leftovers.doubts.is_empty() {
        leftovers.doubts.push("0.10's storage wasn't read cleanly".into());
    }
    let nothing = BTreeMap::new();
    run(best.map_or(&nothing, |b| &b.stores), options, &leftovers)
}

/// What the reading of the web view's storage left unsettled.
#[derive(Default)]
struct Leftovers {
    /// Keys whose value was there but couldn't be decoded.
    undecoded: Vec<String>,
    /// Reasons for doubt about the storage as a whole, one line each.
    doubts: Vec<String>,
}

fn run(raw: &BTreeMap<String, String>, options: &ImportOptions, leftovers: &Leftovers) -> Imported {
    let mut kept = BTreeMap::new();
    let mut stores = vec![];
    let undecoded = &leftovers.undecoded;
    let settings: Settings = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let labels: Labels = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let workspace: Workspace = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let snippets: Snippets = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let bookmarks: Bookmarks = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let tunnels: SavedTunnels = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let typing_counts: TypingCounts = read_store(raw, options, undecoded, &mut kept, &mut stores);
    let undecoded = STORE_KEYS.iter().filter(|key| !raw.contains_key(**key) && undecoded.iter().any(|u| u == *key)).map(|key| (*key).to_owned()).collect();
    let mut state = AppState { settings, labels, workspace, snippets, bookmarks, tunnels, typing_counts, kept, undecoded };

    let mut reasons = leftovers.doubts.clone();
    reasons.extend(stores.iter().filter_map(|s| s.doubt.clone()));
    let lost_sessions = lost_sessions(raw.get(Workspace::KEY), &state.workspace, &mut reasons);
    let privacy = (!reasons.is_empty()).then(|| be_private(&mut state.settings, options, reasons));
    summarize(&state, &mut stores);

    let ignored = raw.keys().filter(|k| !STORE_KEYS.contains(&k.as_str())).cloned().collect();
    let sessions = state.workspace.sessions();
    Imported { state, report: ImportReport { stores, ignored, sessions, lost_sessions, privacy } }
}

/// One store, read, migrated and typed, with its report pushed onto `reports`.
fn read_store<T: Store>(
    raw: &BTreeMap<String, String>,
    options: &ImportOptions,
    undecoded: &[String],
    kept: &mut BTreeMap<String, String>,
    reports: &mut Vec<StoreReport>,
) -> T {
    let platform = options.platform;
    let mut report = StoreReport { key: T::KEY, outcome: Outcome::Missing, problems: vec![], summary: String::new(), doubt: None };
    let value = match raw.get(T::KEY) {
        None if undecoded.iter().any(|key| key == T::KEY) => {
            let why = "couldn't be decoded".to_owned();
            report.doubt = Some(format!("{} couldn't be read ({why})", T::KEY));
            report.outcome = Outcome::Unreadable { why };
            T::defaults(platform)
        }
        None => {
            report.doubt = MUST_BE_THERE.iter().find(|(key, _)| *key == T::KEY).map(|(_, why)| (*why).to_owned());
            T::defaults(platform)
        }
        Some(text) => match unwrap_saved(text) {
            Err(why) => {
                report.doubt = Some(format!("{} couldn't be read ({why})", T::KEY));
                report.outcome = Outcome::Unreadable { why };
                // 0.10 gets back what it had, not defaults over it.
                kept.insert(T::KEY.to_owned(), text.clone());
                T::defaults(platform)
            }
            Ok(Saved { state, version, mended }) => {
                report.problems = mended;
                let privacy_missing = T::KEY == Settings::KEY && privacy_missing(&state, &version);
                let value = match version {
                    Version::Known(v) if v == T::VERSION => {
                        report.outcome = Outcome::Read { version: v };
                        T::from_state(&Json::Object(state), platform, &mut report.problems)
                    }
                    Version::Known(v) if v > T::VERSION => {
                        report.outcome = Outcome::Newer { version: v };
                        report.doubt = Some(format!("{} was saved by a newer version ({v})", T::KEY));
                        kept.insert(T::KEY.to_owned(), text.clone());
                        T::from_state(&Json::Object(state), platform, &mut report.problems)
                    }
                    Version::Known(v) => {
                        report.outcome = Outcome::Migrated { from: v, to: T::VERSION };
                        if !T::SHIPPED.contains(&v) {
                            report.doubt = Some(format!("{} has version {v}, which no release wrote", T::KEY));
                        }
                        T::from_state(&Json::Object(T::migrate(state, v, platform)), platform, &mut report.problems)
                    }
                    // zustand reads a state without a version number as current.
                    Version::Missing => {
                        report.outcome = Outcome::Read { version: T::VERSION };
                        report.doubt = Some(format!("{} has no version number", T::KEY));
                        T::from_state(&Json::Object(state), platform, &mut report.problems)
                    }
                };
                if T::KEY == Settings::KEY && report.problems.iter().any(|p| PRIVACY_FIELDS.contains(&p.at.as_str())) {
                    report.doubt.get_or_insert_with(|| "a privacy setting couldn't be read".into());
                }
                if privacy_missing {
                    report.doubt.get_or_insert_with(|| "a privacy setting was missing".into());
                }
                value
            }
        },
    };
    reports.push(report);
    value
}

enum Version {
    Known(u64),
    Missing,
}

/// A store's value, unwrapped.
struct Saved {
    state: JsonMap,
    version: Version,
    /// Strings with half a character, mended on the way, by their place in `state`.
    mended: Vec<Problem>,
}

/// The `state` object and `version` out of zustand's `{"state":…,"version":N}`.
fn unwrap_saved(text: &str) -> Result<Saved, String> {
    let mut mended = vec![];
    let saved = Json::parse_noting(text, &mut mended).map_err(|_| "not JSON".to_owned())?;
    let Json::Object(mut fields) = saved else { return Err(format!("{} instead of an object", saved.kind())) };
    let state = match fields.shift_remove("state") {
        Some(Json::Object(state)) => state,
        Some(other) => return Err(format!("its state is {}", other.kind())),
        None => return Err("no state".into()),
    };
    let version = match fields.get("version") {
        Some(Json::Number(n)) => match (n.as_u64(), n.as_f64()) {
            (Some(v), _) => Version::Known(v),
            (None, Some(x)) if x.fract() == 0.0 && (0.0..=1e9).contains(&x) => Version::Known(x as u64),
            _ => Version::Missing,
        },
        _ => Version::Missing,
    };
    let mended = mended.into_iter().map(|p| p.inside("state")).collect();
    Ok(Saved { state, version, mended })
}

/// Whether a settings state saved at `version` lacks a privacy setting every save at that
/// version had ([`PRIVACY_SINCE`]). A newer version's, or one without a version number, is
/// in doubt already.
fn privacy_missing(state: &JsonMap, version: &Version) -> bool {
    let Version::Known(v) = *version else { return false };
    v <= Settings::VERSION && PRIVACY_SINCE.iter().any(|(name, since)| v >= *since && !state.contains_key(*name))
}

/// Turns the privacy settings on and says why.
fn be_private(settings: &mut Settings, options: &ImportOptions, reasons: Vec<String>) -> PrivacyOverride {
    let mut turned_on = vec![];
    let mut switch = |name: &'static str, setting: &mut bool| {
        if !*setting {
            *setting = true;
            turned_on.push(name);
        }
    };
    switch("streamerMode", &mut settings.streamer_mode);
    switch("hideSecrets", &mut settings.hide_secrets);
    if options.tor_on {
        switch("torHideTyping", &mut settings.tor_hide_typing);
    }
    PrivacyOverride { reasons, turned_on }
}

/// Sessions in the raw workspace (at any depth) that the typed `workspace`, the one 1.x
/// reattaches from, doesn't have, each once. Says why in `reasons` when there are any, and
/// when the raw text couldn't be parsed.
fn lost_sessions(raw: Option<&String>, workspace: &Workspace, reasons: &mut Vec<String>) -> Vec<SessionRef> {
    let Some(text) = raw else { return vec![] };
    let before = match Json::parse(text) {
        Ok(json) => sessions_in(&json),
        Err(_) => {
            reasons.push(format!("{}'s terminal sessions couldn't all be checked", Workspace::KEY));
            sessions_in_damaged(text)
        }
    };
    let came_over: HashSet<&str> = workspace.panes.values().filter_map(|p| p.session_key.as_deref()).collect();
    let mut seen = HashSet::new();
    let lost: Vec<SessionRef> =
        before.into_iter().filter(|s| !came_over.contains(s.session_key.as_str()) && seen.insert(s.session_key.clone())).collect();
    if !lost.is_empty() {
        reasons.push(format!("{} terminal session(s) didn't come over", lost.len()));
    }
    lost
}

/// The sessions of a workspace whose text can't be parsed: every `sessionKey` in the text,
/// with its pane and machine where the text is whole up to it.
fn sessions_in_damaged(text: &str) -> Vec<SessionRef> {
    let mut placed: HashMap<String, SessionRef> = HashMap::new();
    for session in Json::parse_partial(text).map(|whole| sessions_in(&whole)).unwrap_or_default() {
        placed.entry(session.session_key.clone()).or_insert(session);
    }
    session_keys_in_text(text)
        .into_iter()
        .map(|key| placed.get(&key).cloned().unwrap_or_else(|| SessionRef::new(String::new(), None, key)))
        .collect()
}

/// What came over, in counts, for each store's report.
fn summarize(state: &AppState, reports: &mut [StoreReport]) {
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    for report in reports.iter_mut() {
        report.summary = match report.key {
            "sshee-settings" => {
                let s = &state.settings;
                let on_off = |b: bool| if b { "on" } else { "off" };
                format!(
                    "streamer mode {}, secrets hidden {}, {}, {}, {}, {} from older versions",
                    on_off(s.streamer_mode),
                    on_off(s.hide_secrets),
                    plural(s.custom_themes.len(), "theme of your own", "themes of your own"),
                    plural(s.shortcuts.len(), "changed shortcut", "changed shortcuts"),
                    plural(s.highlight_rules.len(), "highlight rule", "highlight rules"),
                    plural(s.extra.len(), "setting kept", "settings kept"),
                )
            }
            "sshee-labels" => {
                let l = &state.labels;
                format!(
                    "{}, {} picked, {} guessed, {} hidden",
                    plural(l.env.len(), "label", "labels"),
                    plural(l.looks.len(), "look", "looks"),
                    l.auto_looks.len(),
                    plural(l.hidden.len(), "server", "servers"),
                )
            }
            "sshee-workspace" => {
                let w = &state.workspace;
                let tabs = w.panes.values().filter(|p| p.kind != crate::model::PaneKind::Empty).count();
                let sessions = w.sessions();
                let local = sessions.iter().filter(|s| !s.durable).count();
                let mut text = format!(
                    "{} on {}, {} with a tmux session",
                    plural(tabs, "tab", "tabs"),
                    plural(w.layouts.len(), "screen", "screens"),
                    plural(sessions.len() - local, "terminal", "terminals"),
                );
                // A terminal on this computer had no tmux: it ended with 0.10.
                if local > 0 {
                    text += &format!(", {} (gone with 0.10)", plural(local, "local terminal", "local terminals"));
                }
                text + &format!(", {}", plural(w.closed_tabs.len(), "closed tab", "closed tabs"))
            }
            "sshee-snippets" => plural(state.snippets.snippets.len(), "snippet", "snippets"),
            "sshee-bookmarks" => {
                let b = &state.bookmarks;
                format!(
                    "{}, {}",
                    plural(b.pins.values().map(Vec::len).sum(), "bookmark", "bookmarks"),
                    plural(b.recent.values().map(Vec::len).sum(), "recent folder", "recent folders"),
                )
            }
            "sshee-tunnels" => {
                let all: Vec<_> = state.tunnels.by_host.values().flatten().collect();
                format!(
                    "{} on {}, {} switched on",
                    plural(all.len(), "tunnel", "tunnels"),
                    plural(state.tunnels.by_host.len(), "server", "servers"),
                    all.iter().filter(|t| t.on).count(),
                )
            }
            "sshee-typing-counts" => {
                // Saturating: JavaScript can save counts no u64 sum holds.
                let keys = state.typing_counts.saved.values().flat_map(|places| places.values()).fold(0u64, |sum, t| sum.saturating_add(t.count("keys")));
                format!("{keys} keys counted")
            }
            _ => String::new(),
        };
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Outcome::Read { version } => write!(f, "read (version {version})"),
            Outcome::Migrated { from, to } => write!(f, "brought from version {from} to {to}"),
            Outcome::Missing => f.write_str("not there, defaults"),
            Outcome::Unreadable { why } => write!(f, "unreadable ({why}), defaults"),
            Outcome::Newer { version } => write!(f, "from a newer version ({version}), kept as is"),
        }
    }
}

/// The report in plain words, one store a line, for `ssheesh-import --dry-run`.
impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for store in &self.stores {
            writeln!(f, "{}: {}. {}", store.key, store.outcome, store.summary)?;
            for problem in &store.problems {
                writeln!(f, "  {problem}")?;
            }
        }
        if !self.ignored.is_empty() {
            writeln!(f, "Left alone: {}", self.ignored.join(", "))?;
        }
        if !self.lost_sessions.is_empty() {
            let on_servers = self.lost_sessions.iter().filter(|s| s.durable).count();
            writeln!(f, "Terminal sessions that didn't come over: {} ({on_servers} may still run on a server)", self.lost_sessions.len())?;
        }
        match &self.privacy {
            None => writeln!(f, "Privacy settings came over as they were."),
            Some(p) => {
                let what = if p.turned_on.is_empty() { "were already on".to_owned() } else { format!("{} turned on", p.turned_on.join(", ")) };
                writeln!(f, "Privacy settings {what}, because {}.", p.reasons.join("; "))
            }
        }
    }
}
