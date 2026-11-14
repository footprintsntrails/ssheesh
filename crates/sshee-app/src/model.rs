//! The app's state, typed: the seven stores 0.10.x kept in the web view's localStorage, with
//! the same shapes, versions, defaults and migrations as 0.10.14's zustand stores.
//!
//! Every store is `{"state": {…}, "version": N}` in 0.10. [`Store`] says how each one is
//! read and written; [`AppState`] holds all seven. Fields this version doesn't know are kept
//! (`extra` on every object), and [`AppState::to_web_stores`] writes the exact 0.10 shapes
//! back, so a rollback to 0.10.14 finds what it left.
//!
//! Numbers 0.10 keeps as JavaScript numbers are `f64` where they can have a fraction (sizes,
//! zoom) and integers where they never do (ports, counts, ids). Either way they go back out
//! as JavaScript writes them.

pub mod json;
mod bookmarks;
mod labels;
mod settings;
mod snippets;
mod tunnels;
mod typing;
mod workspace;

pub use bookmarks::Bookmarks;
pub use json::{Fix, Json, JsonMap, Problem};
pub use labels::{AutoLook, EnvLabel, HostLook, Labels};
pub use settings::{
    CompactMode, CursorStyle, CustomTheme, HighlightColor, HighlightRule, IconPack, Keys, Logo, RailStatus, ReconnectMode,
    Settings, Side, SidebarMode, TabsPlacement, TerminalColors, UpdateMode, ViewMode, WelcomeMode, WelcomePart, DEFAULT_ACCENT,
    DEFAULT_THEME,
};
pub use snippets::{Snippet, Snippets};
pub use tunnels::{Forward, ForwardKind, SavedTunnel, SavedTunnels, Scheme};
pub use typing::{EchoTally, TypingCounts};
pub use workspace::{
    ClosedTab, ContainerRef, ContainerRun, Dir, Engine, LayoutNode, NodeKind, Pane, PaneKind, SessionRef, TargetKind, TargetRef,
    Workspace,
};
pub(crate) use workspace::{session_keys_in_text, sessions_in};

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;

/// The system 0.10 ran on. A few defaults and one migration depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Mac,
    Linux,
}

impl Platform {
    /// The system this runs on.
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Mac
        } else {
            Platform::Linux
        }
    }

    /// 0.10 drew terminals with the GPU by default on Windows and macOS. Linux's WebKitGTK
    /// may have no GPU at all.
    pub fn gpu_by_default(self) -> bool {
        matches!(self, Platform::Windows | Platform::Mac)
    }
}

/// One of 0.10's persisted zustand stores.
pub trait Store: Sized + Serialize + DeserializeOwned {
    /// Its localStorage key, one of [`crate::raw::STORE_KEYS`].
    const KEY: &'static str;
    /// The `version` 0.10.14 writes.
    const VERSION: u64;
    /// Every `version` a release ever wrote, 0.1.0 to 0.10.14.
    const SHIPPED: &'static [u64];

    /// As a fresh 0.10.14 starts it on `platform`.
    fn defaults(platform: Platform) -> Self;

    /// The migrations 0.10.14 runs on a state saved at `from`, an older version, in the
    /// same order and on the same raw JSON. The default has none.
    fn migrate(state: JsonMap, _from: u64, _platform: Platform) -> JsonMap {
        state
    }

    /// Sets what the saved state didn't have where the default depends on the system:
    /// serde's defaults are this machine's.
    fn complete(&mut self, _saved: &JsonMap, _platform: Platform) {}

    /// Reads a state at [`Self::VERSION`] (migrated first if it wasn't). What's missing
    /// takes the default, as zustand's merge does; what doesn't fit is left out and listed
    /// in `problems`.
    fn from_state(state: &Json, platform: Platform, problems: &mut Vec<Problem>) -> Self {
        let mut value = json::lenient::<Self>(state, problems).unwrap_or_else(|| Self::defaults(platform));
        if let Some(saved) = state.as_object() {
            value.complete(saved, platform);
        }
        value
    }

    /// The `state` 0.10 would save.
    fn to_state(&self) -> Json {
        Json::of(self)
    }

    /// The whole localStorage value, `{"state":…,"version":N}`, as zustand writes it.
    fn to_saved(&self) -> String {
        let mut saved = JsonMap::new();
        saved.insert("state".into(), self.to_state());
        saved.insert("version".into(), Json::from(Self::VERSION));
        Json::Object(saved).to_text()
    }
}

/// All seven stores.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub settings: Settings,
    pub labels: Labels,
    pub workspace: Workspace,
    pub snippets: Snippets,
    pub bookmarks: Bookmarks,
    pub tunnels: SavedTunnels,
    pub typing_counts: TypingCounts,
    /// Stores to give back to 0.10 exactly as found, by key: one saved by a newer version
    /// than this code knows (its typed copy above is only this version's best reading of it),
    /// and one that couldn't be read at all (its typed copy is the defaults; 0.10 gets back
    /// what it had, not defaults over it). [`AppState::to_web_stores`] writes them unchanged.
    pub kept: BTreeMap<String, String>,
    /// Stores that were in 0.10's web view storage but whose value couldn't be decoded from
    /// its files, by key. 0.10's own web view may still read them, so
    /// [`AppState::to_web_stores`] leaves them out rather than write defaults over them.
    pub undecoded: Vec<String>,
}

impl AppState {
    /// A fresh 0.10.14 on `platform`.
    pub fn defaults(platform: Platform) -> Self {
        AppState {
            settings: Settings::defaults(platform),
            labels: Labels::default(),
            workspace: Workspace::default(),
            snippets: Snippets::default(),
            bookmarks: Bookmarks::default(),
            tunnels: SavedTunnels::default(),
            typing_counts: TypingCounts::default(),
            kept: BTreeMap::new(),
            undecoded: vec![],
        }
    }

    /// The localStorage values 0.10.14 reads, by key: what 1.x hands back if a rollback has
    /// to give 0.10 its state again. All seven, but for any in [`AppState::undecoded`].
    pub fn to_web_stores(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut put = |key: &str, saved: String| {
            if !self.undecoded.iter().any(|k| k == key) {
                out.insert(key.to_owned(), self.kept.get(key).cloned().unwrap_or(saved));
            }
        };
        put(Settings::KEY, self.settings.to_saved());
        put(Labels::KEY, self.labels.to_saved());
        put(Workspace::KEY, self.workspace.to_saved());
        put(Snippets::KEY, self.snippets.to_saved());
        put(Bookmarks::KEY, self.bookmarks.to_saved());
        put(SavedTunnels::KEY, self.tunnels.to_saved());
        put(TypingCounts::KEY, self.typing_counts.to_saved());
        out
    }
}
