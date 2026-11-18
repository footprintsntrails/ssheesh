//! The command palette without a screen: what it lists for the machine on screen, how typed
//! text narrows and orders the list, and which row Return picks.
//!
//! A port of 0.10's command bar (`CommandPalette` in `src/components/Overlays.tsx`), with the
//! 1.0 design's rules on top:
//!
//! - it lists what you can do on the server on screen first ("On web-7731": Files first, then
//!   your snippets), then where you can go (tabs waiting for you first, other tabs, servers,
//!   each server's workspace, files and stats, your bookmarks and recent folders), then the
//!   window's own commands and Settings;
//! - `srv:`, `cmd:`, `file:` or `set:` in front narrows it to one kind of thing; `set:`, or any
//!   word typed, also finds every row of Settings and every shortcut, and opens Settings there;
//! - every word typed must be in a row (its name, its note, or the extra words it's known
//!   by), in any order, whatever the case; within a group, a row whose name starts with what
//!   was typed comes before one that only holds it;
//! - **a row that runs a command on a server is never the one highlighted when the list
//!   changes**: Return on a fresh list never runs anything on a machine. You move to such a
//!   row yourself and press Return on it. On a Production server the row says so.
//!
//! The window keeps the typed text and the highlighted row; everything else is made here from
//! the [`Workbench`] each time, so the rules are tested without a window.

use crate::files::bookmarks;
use crate::model::{Bookmarks, Keys, Platform, Snippet};
use crate::settings_store::{catalog, keymap};
use crate::workbench::layout::Arrangement;
use crate::workbench::shortcuts::Shortcut;
use crate::workbench::{ServerKey, TabId, TabKind, Tor, Workbench};
use indexmap::IndexMap;

/// One kind of thing, asked for by its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Servers,
    Commands,
    Folders,
    Settings,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::Servers, Scope::Commands, Scope::Folders, Scope::Settings];

    /// What is typed before the colon.
    pub fn prefix(self) -> &'static str {
        match self {
            Scope::Servers => "srv",
            Scope::Commands => "cmd",
            Scope::Folders => "file",
            Scope::Settings => "set",
        }
    }

    /// The word on the chip in the search field.
    pub fn name(self) -> &'static str {
        match self {
            Scope::Servers => "Servers",
            Scope::Commands => "Commands",
            Scope::Folders => "Folders",
            Scope::Settings => "Settings",
        }
    }
}

/// What was typed, read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub scope: Option<Scope>,
    /// What follows the prefix, trimmed: a command to run is this, as typed.
    pub text: String,
    /// `text` in lower case, word by word.
    words: Vec<String>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// Reads the search field: a prefix (in any case, spaces around it allowed), then words.
pub fn parse(input: &str) -> Query {
    let trimmed = input.trim_start();
    let scoped = Scope::ALL.iter().find_map(|scope| {
        let prefix = scope.prefix();
        let head = trimmed.get(..prefix.len())?;
        let rest = trimmed[prefix.len()..].strip_prefix(':')?;
        head.eq_ignore_ascii_case(prefix).then_some((*scope, rest))
    });
    let (scope, rest) = match scoped {
        Some((scope, rest)) => (Some(scope), rest),
        None => (None, trimmed),
    };
    let text = rest.trim().to_owned();
    let words = text.to_lowercase().split_whitespace().map(str::to_owned).collect();
    Query { scope, text, words }
}

/// What picking a row does. The window carries it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A server's files, at `path` when one is known.
    Files { server: ServerKey, path: Option<String> },
    Stats { server: ServerKey },
    /// A machine's Docker and Podman containers.
    Containers { server: ServerKey },
    /// Go to a tab.
    Tab(TabId),
    /// Go to a machine: the tab you were last on there, or a new terminal.
    Server(ServerKey),
    /// Type `command` into a terminal on `server`; with `enter`, run it.
    Run { server: ServerKey, command: String, enter: bool },
    NewTerminal,
    CloseTab,
    ReopenTab,
    ToggleSidebar,
    /// A new terminal beside the tab on screen: to its right, or under it.
    SplitRight,
    SplitDown,
    /// One pane of a split fills the window, or all show again.
    Zoom,
    MainMenu,
    AddServer,
    /// Settings, at a section when one is named.
    Settings(Option<&'static str>),
    /// The security check of a server.
    Checkup { server: ServerKey },
    /// "Free up disk space": what takes a server's disks, and safe cleanups.
    Disk { server: ServerKey },
    /// The recipes, for a server.
    Recipes { server: ServerKey },
    /// The list of snippets, and a new one with this command in it.
    Snippets,
    NewSnippet(String),
    WhatsNew,
    Tour,
    /// Settings at one of its rows (`key`, the row's element key) in `section`.
    SettingsRow { section: &'static str, key: String },
    /// The tab on screen beside another, or one tab at a time again.
    SideBySide,
    /// The panes on screen in one of 0.10's shapes.
    Arrange(Arrangement),
    Compact,
    /// The find bar of the terminal on screen.
    Find,
    Streamer,
    /// Tor on or off.
    Tor,
    /// The list of agents on every server.
    Agents,
    /// A machine's terminal and its files, side by side.
    Workspace(ServerKey),
    /// Settings → Updates, where updates are looked for.
    CheckUpdates,
}

/// The palette's words for each of 0.10's shapes.
fn arrangement(how: Arrangement) -> (&'static str, &'static str) {
    match how {
        Arrangement::Main => ("Arrange: main + stack", "main stack"),
        Arrangement::Columns => ("Arrange: columns", "columns"),
        Arrangement::Rows => ("Arrange: rows", "rows"),
        Arrangement::Grid => ("Arrange: grid", "grid"),
    }
}

/// The picture at a row's start; the screen layer has the drawings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Glyph {
    Folder,
    Terminal,
    Agent,
    Stats,
    Containers,
    /// A server's own icon, by its id (`globe`, `database`, ...).
    Server(String),
    Plus,
    Close,
    Sidebar,
    Home,
    Reopen,
    Gear,
    Shield,
    /// A disk drive: "Free up disk space".
    Drive,
    Recipe,
    News,
    Help,
    SplitRight,
    SplitDown,
    Zoom,
    Compact,
    Find,
    /// Streamer mode, by what picking the row does: `true` hides.
    Eye(bool),
    Tor,
    /// A bookmarked folder.
    Star,
    Refresh,
}

/// The headings the list is cut into, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// "On web-7731": what you can do on the machine on screen.
    OnServer,
    GoTo,
    Window,
    Settings,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub group: Group,
    pub glyph: Glyph,
    pub label: String,
    /// The label is a command: drawn in the terminal's font.
    pub mono: bool,
    /// A quieter word or two after the label ("snippet", a server's name); may be empty.
    pub note: String,
    /// The red "Prod" goes on this row: it acts on a Production server.
    pub prod: bool,
    /// The tab waits for you.
    pub needs_you: bool,
    /// The shortcut that does the same, as key caps show it.
    pub keys: Option<String>,
    pub action: Action,
    /// More words to find it by, in lower case.
    words: String,
    /// It is what was typed, offered as a command: listed whatever it says.
    typed: bool,
}

impl Row {
    /// It types a command into a server's terminal: never highlighted by itself.
    pub fn runs(&self) -> bool {
        matches!(self.action, Action::Run { .. })
    }

    /// How well the row answers `query`: 0 not at all (it is left out), more is better.
    fn score(&self, query: &Query) -> u8 {
        if query.is_empty() {
            return 1;
        }
        let label = self.label.to_lowercase();
        let hay = format!("{label} {} {}", self.note.to_lowercase(), self.words);
        if !query.words.iter().all(|w| hay.contains(w.as_str())) {
            return 0;
        }
        let whole = query.words.join(" ");
        if label.starts_with(&whole) {
            4
        } else if label.split(|c: char| !c.is_alphanumeric()).any(|word| !word.is_empty() && word.starts_with(query.words[0].as_str())) {
            3
        } else if query.words.iter().all(|w| label.contains(w.as_str())) {
            2
        } else {
            1
        }
    }
}

/// What the palette lists from.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub bench: &'a Workbench,
    pub snippets: &'a [Snippet],
    /// Shortcuts as on a Mac (⌘) or as elsewhere (Ctrl+Shift).
    pub mac: bool,
    /// Shortcuts changed in Settings, by 0.10's ids.
    pub shortcuts: &'a IndexMap<String, Option<Keys>>,
    /// Every machine's pinned and recent folders.
    pub bookmarks: &'a Bookmarks,
    /// What is on now, for the commands that turn it off.
    pub now: Now,
}

/// What the window has on now: a command that turns something on or off says which it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub streaming: bool,
    pub compact: bool,
    /// The system whose Settings rows are listed.
    pub platform: Platform,
}

impl Default for Now {
    fn default() -> Self {
        Now { streaming: false, compact: false, platform: Platform::current() }
    }
}

/// Settings' sections, with words people might search for (0.10's `SETTINGS_SECTIONS`). The ids
/// are `settings_store::catalog::Section`'s, which opens them.
const SETTINGS_SECTIONS: [(&str, &str, &str); 10] = [
    ("layout", "Layout", "tabs side by side sidebar compact vitals"),
    ("appearance", "Appearance", "theme colors colours accent zoom size dark light"),
    ("terminal", "Terminal", "font cursor colors colours scheme copy gpu idle afk sleep disconnect reconnect highlight welcome"),
    ("keyboard", "Keyboard", "shortcuts keys hotkeys keybindings"),
    ("privacy", "Privacy", "hide ip address streamer mode secrets"),
    ("files", "Files", "metadata exif gps clean uploads"),
    ("agents", "Agents and notifications", "notification bell agent claude codex attention"),
    ("tor", "Tor", "tor onion anonymous vpn proxy typing rhythm"),
    ("updates", "Updates", "version download"),
    ("about", "About", "version license licence credits tour"),
];

/// A shortcut's keys as a key cap shows them, as Settings → Keyboard does
/// (`keymap::shortcut_label`): "⇧⌘T" on a Mac, "Ctrl+Shift+T" elsewhere; yours from Settings,
/// else the ones it comes with. `None` for one that is turned off or has none.
pub fn shortcut_label(shortcut: Shortcut, mac: bool, changed: &IndexMap<String, Option<Keys>>) -> Option<String> {
    let id = shortcut.id()?;
    Some(keymap::shortcut_label(id, keymap::Keyboard::of(mac), changed)).filter(|label| !label.is_empty())
}

/// Whether a snippet has blanks to fill in (`{{name}}`, `{{name:fallback}}`).
pub fn has_blanks(command: &str) -> bool {
    command.find("{{").is_some_and(|open| command[open..].contains("}}"))
}

/// What the empty search field says: "Search or run on web-7731", or on the main menu what
/// can be searched.
pub fn placeholder(bench: &Workbench) -> String {
    match bench.active() {
        Some(tab) => format!("Search or run on {}", bench.server(&tab.server).map_or("this server", |s| s.name.as_str())),
        None => "Servers, files, commands and settings".into(),
    }
}

/// The heading of a group. `server`: the machine on screen's name.
pub fn heading(group: Group, server: Option<&str>) -> String {
    match group {
        Group::OnServer => format!("On {}", server.unwrap_or("this server")),
        Group::GoTo => "Go to".into(),
        Group::Window => "Window".into(),
        Group::Settings => "Settings".into(),
    }
}

/// A server's latest folder in Files: of an open Files tab, else of one closed lately.
fn recent_folder(bench: &Workbench, server: &str) -> Option<String> {
    let open = bench.tabs_on(server).filter(|t| t.kind == TabKind::Files).filter_map(|t| t.path.clone()).last();
    open.or_else(|| bench.closed().iter().find(|c| c.server == server && c.kind == TabKind::Files).and_then(|c| c.path.clone()))
}

fn row(group: Group, glyph: Glyph, label: impl Into<String>, action: Action) -> Row {
    Row { group, glyph, label: label.into(), mono: false, note: String::new(), prod: false, needs_you: false, keys: None, action, words: String::new(), typed: false }
}

impl Row {
    fn note(mut self, note: impl Into<String>) -> Row {
        self.note = note.into();
        self
    }

    fn words(mut self, words: &str) -> Row {
        self.words = words.to_lowercase();
        self
    }

    fn keys(mut self, keys: Option<String>) -> Row {
        self.keys = keys;
        self
    }
}

/// Every row the palette could show now, before any typing narrows it.
fn all_rows(ctx: &Context, query: &Query) -> Vec<Row> {
    let bench = ctx.bench;
    let label = |s: Shortcut| shortcut_label(s, ctx.mac, ctx.shortcuts);
    let mut rows = Vec::new();

    // On the machine on screen: its files first, then what can be run there.
    if let Some(server) = bench.current_server() {
        let key = server.key();
        let prod = server.is_prod();
        let folder = recent_folder(bench, &key);
        let mut files = row(Group::OnServer, Glyph::Folder, "Files", Action::Files { server: key.clone(), path: folder.clone() }).words("files folders browse");
        if let Some(folder) = &folder {
            files = files.note(format!("recent: {folder}"));
        }
        rows.push(files);
        rows.push(row(Group::OnServer, Glyph::Stats, "Stats", Action::Stats { server: key.clone() }).words("stats metrics cpu memory traffic"));
        if !server.is_local() {
            rows.push(row(Group::OnServer, Glyph::Shield, "Security check", Action::Checkup { server: key.clone() }).words("security checkup audit firewall ports login updates safe"));
            rows.push(row(Group::OnServer, Glyph::Drive, "Free up disk space", Action::Disk { server: key.clone() }).words("disk space usage full clean up docker logs biggest folders"));
            rows.push(row(Group::OnServer, Glyph::Recipe, "Recipes", Action::Recipes { server: key.clone() }).words("recipes guide how to firewall fail2ban docker swap https caddy key sudo"));
        }
        rows.push(row(Group::OnServer, Glyph::Containers, "Containers", Action::Containers { server: key.clone() }).words("docker podman containers compose logs"));
        for snippet in ctx.snippets {
            let blanks = has_blanks(&snippet.command);
            let mut r = row(Group::OnServer, Glyph::Terminal, &snippet.command, Action::Run { server: key.clone(), command: snippet.command.clone(), enter: !blanks })
                .note(if blanks { "snippet with blanks" } else { "snippet" })
                .words(&format!("{} snippet command run", snippet.name));
            r.mono = true;
            r.prod = prod;
            rows.push(r);
        }
        // What was typed, as a command for this machine.
        if !query.is_empty() && matches!(query.scope, None | Some(Scope::Commands)) {
            let mut r = row(Group::OnServer, Glyph::Terminal, &query.text, Action::Run { server: key, command: query.text.clone(), enter: true })
                .note(format!("run on {}", server.name))
                .words(&query.text);
            r.mono = true;
            r.prod = prod;
            r.typed = true;
            rows.push(r);
        }
    }

    // Where to go: tabs that wait for you, then the other tabs, then every machine.
    let on_screen = bench.active_id();
    let mut tabs: Vec<_> = bench.tabs().iter().filter(|t| Some(t.id) != on_screen).collect();
    tabs.sort_by_key(|t| !t.needs_you);
    for tab in tabs {
        let server = bench.server(&tab.server);
        let glyph = match tab.kind {
            TabKind::Terminal if tab.agent.is_some() => Glyph::Agent,
            TabKind::Terminal if tab.container.is_some() => Glyph::Containers,
            TabKind::Terminal => Glyph::Terminal,
            TabKind::Files => Glyph::Folder,
            TabKind::Stats => Glyph::Stats,
            TabKind::Containers => Glyph::Containers,
        };
        let mut r = row(Group::GoTo, glyph, crate::agents::tab_label(tab), Action::Tab(tab.id))
            .note(server.map_or("Unknown server", |s| s.name.as_str()))
            .words(&format!("tab {} {}", tab.agent.as_deref().unwrap_or(""), tab.kind.label()));
        r.needs_you = tab.needs_you;
        rows.push(r);
    }
    for server in bench.servers() {
        let mut r = row(Group::GoTo, Glyph::Server(server.icon.clone()), &server.name, Action::Server(server.key()))
            .note(if server.is_local() { "local" } else { server.address.as_str() })
            .words("connect server terminal");
        r.prod = server.is_prod();
        rows.push(r);
    }
    // Each machine's terminal and files side by side, its files and its stats (the machine on
    // screen has its own Files and Stats above).
    for server in bench.servers() {
        rows.push(
            row(Group::GoTo, Glyph::SplitRight, format!("Workspace for {}", server.name), Action::Workspace(server.key()))
                .note("terminal · files")
                .words("workspace layout side by side"),
        );
    }
    for server in bench.servers() {
        let key = server.key();
        if bench.current_key() != Some(key.as_str()) {
            rows.push(row(Group::GoTo, Glyph::Folder, format!("Browse files on {}", server.name), Action::Files { server: key, path: None }).words("files folders"));
        }
    }
    for server in bench.servers() {
        let key = server.key();
        if bench.current_key() != Some(key.as_str()) {
            rows.push(row(Group::GoTo, Glyph::Stats, format!("Stats for {}", server.name), Action::Stats { server: key }).words("stats metrics cpu memory traffic"));
        }
    }
    // Every machine's bookmarks, then up to five of its recent folders that aren't bookmarks.
    for server in bench.servers() {
        let key = server.key();
        let pins = bookmarks::pins(ctx.bookmarks, &key);
        let folder = |path: &String, glyph: Glyph, kind: &str| {
            row(Group::GoTo, glyph, format!("{} on {}", crate::paths::name(path), server.name), Action::Files { server: key.clone(), path: Some(path.clone()) })
                .note(path.as_str())
                .words(&format!("{kind} folder files"))
        };
        rows.extend(pins.iter().map(|path| folder(path, Glyph::Star, "bookmark")));
        let recent = bookmarks::recent(ctx.bookmarks, &key).iter().filter(|path| !pins.contains(path)).take(5);
        rows.extend(recent.map(|path| folder(path, Glyph::Folder, "recent")));
    }

    // The window's own commands.
    if bench.active().is_some() {
        rows.push(row(Group::Window, Glyph::Plus, "New terminal", Action::NewTerminal).keys(label(Shortcut::NewTerminal)).words("tab shell"));
        rows.push(row(Group::Window, Glyph::Close, "Close tab", Action::CloseTab).keys(label(Shortcut::CloseTab)));
    }
    if !bench.closed().is_empty() {
        rows.push(row(Group::Window, Glyph::Reopen, "Reopen closed tab", Action::ReopenTab).keys(label(Shortcut::ReopenTab)).words("undo close"));
    }
    if bench.active().is_some() {
        rows.push(row(Group::Window, Glyph::SplitRight, "Split right", Action::SplitRight).keys(label(Shortcut::SplitRight)).words("pane side by side columns"));
        rows.push(row(Group::Window, Glyph::SplitDown, "Split down", Action::SplitDown).keys(label(Shortcut::SplitDown)).words("pane rows below"));
    }
    if bench.active_id().is_some_and(|id| bench.layout().shown_with(id).is_some()) {
        let words = if bench.split_on_screen().is_some() { "Fill the window with this pane" } else { "Show all the panes" };
        rows.push(row(Group::Window, Glyph::Zoom, words, Action::Zoom).keys(label(Shortcut::Zoom)).words("zoom maximize pane"));
    }
    if bench.active().is_some() {
        let side = if bench.split_on_screen().is_some() { "One tab at a time" } else { "Tabs side by side" };
        rows.push(row(Group::Window, Glyph::SplitRight, side, Action::SideBySide).keys(label(Shortcut::SideBySide)).words("tabs side by side split one at a time toggle"));
    }
    for how in [Arrangement::Main, Arrangement::Columns, Arrangement::Rows, Arrangement::Grid] {
        let (name, words) = arrangement(how);
        rows.push(row(Group::Window, Glyph::SplitRight, name, Action::Arrange(how)).words(&format!("arrange layout panes {words}")));
    }
    if bench.active().is_some_and(|t| t.kind == TabKind::Terminal) {
        rows.push(
            row(Group::Window, Glyph::Find, "Find in the terminal", Action::Find)
                .keys(label(Shortcut::Find { always: false }))
                .words("find search text in terminal output scrollback history"),
        );
    }
    rows.push(row(Group::Window, Glyph::Sidebar, "Toggle sidebar", Action::ToggleSidebar).keys(label(Shortcut::ToggleSidebar)).words("show hide"));
    if bench.active().is_some() {
        rows.push(row(Group::Window, Glyph::Home, "Main menu", Action::MainMenu).words("home start page new tab"));
    }
    let compact = if ctx.now.compact { "Turn off compact mode" } else { "Turn on compact mode" };
    rows.push(row(Group::Window, Glyph::Compact, compact, Action::Compact).keys(label(Shortcut::Compact)).words("compact mode focus hide sidebar toolbar zen"));
    let streamer = if ctx.now.streaming { "Turn off streamer mode" } else { "Turn on streamer mode" };
    rows.push(
        row(Group::Window, Glyph::Eye(!ctx.now.streaming), streamer, Action::Streamer)
            .keys(label(Shortcut::Streamer))
            .words("streamer mode hide names addresses privacy screen share recording"),
    );
    let tor = if bench.tor() == Tor::Off { "Connect through Tor" } else { "Turn off Tor" };
    rows.push(row(Group::Window, Glyph::Tor, tor, Action::Tor).words("tor onion anonymous hide ip address vpn proxy privacy"));
    rows.push(row(Group::Window, Glyph::Agent, "Agents at work", Action::Agents).keys(label(Shortcut::Agents)).words("agents claude codex opencode waiting needs you"));
    rows.push(row(Group::Window, Glyph::Plus, "Add server…", Action::AddServer).keys(label(Shortcut::AddServer)).words("new host import xpipe ssh config"));
    rows.push(row(Group::Window, Glyph::Terminal, "Snippets", Action::Snippets).words("snippets saved commands"));
    rows.push(row(Group::Window, Glyph::Plus, "New snippet…", Action::NewSnippet(String::new())).words("save command snippet add"));

    // Settings and its sections, then what 0.10 listed with them; and as soon as there's
    // something to look for, every row of Settings.
    rows.push(row(Group::Settings, Glyph::Gear, "Settings", Action::Settings(None)).keys(label(Shortcut::Settings)).words("preferences options"));
    rows.push(row(Group::Settings, Glyph::Gear, "Change theme and colors", Action::Settings(Some("appearance"))).words("theme appearance colors colours accent dark light"));
    rows.push(row(Group::Settings, Glyph::Terminal, "Terminal look", Action::Settings(Some("terminal"))).words("terminal theme font size cursor scheme"));
    rows.push(row(Group::Settings, Glyph::News, "What's new", Action::WhatsNew).words("changelog release notes version update"));
    rows.push(row(Group::Settings, Glyph::Help, "Take the tour", Action::Tour).words("tour help introduction welcome tips learn"));
    rows.push(row(Group::Settings, Glyph::Refresh, "Check for updates", Action::CheckUpdates).words("updates version download upgrade"));
    for (id, name, words) in SETTINGS_SECTIONS {
        rows.push(row(Group::Settings, Glyph::Gear, format!("Settings: {name}"), Action::Settings(Some(id))).words(words));
    }
    if !query.is_empty() || query.scope == Some(Scope::Settings) {
        rows.extend(settings_rows(ctx.now.platform));
    }
    rows
}

/// Every row of Settings shown on `platform`, then every shortcut, each opening Settings at
/// itself (0.10's `Settings: {label}`, one row deeper).
fn settings_rows(platform: Platform) -> Vec<Row> {
    let mut rows: Vec<Row> = catalog::rows()
        .iter()
        .filter(|r| !r.label.is_empty() && r.only.is_none_or(|only| only == platform))
        .map(|r| {
            let words = format!("setting {} {} {}", r.words, r.group.unwrap_or(""), r.section.label());
            row(Group::Settings, Glyph::Gear, r.label, Action::SettingsRow { section: r.section.id(), key: r.key.to_owned() }).note(r.section.label()).words(&words)
        })
        .collect();
    let mut named = Vec::new();
    for id in keymap::GROUPS.iter().flat_map(|(_, ids)| ids.iter()) {
        let Some(entry) = keymap::entry(id).filter(|e| !named.contains(&e.name)) else { continue };
        named.push(entry.name);
        let key = format!("shortcut-{id}");
        rows.push(row(Group::Settings, Glyph::Gear, entry.name, Action::SettingsRow { section: catalog::Section::Keyboard.id(), key }).note("Keyboard shortcut").words("shortcut keys hotkey keybinding"));
    }
    rows
}

/// Whether a group belongs to a scope (0.10's `shows`).
fn in_scope(row: &Row, scope: Scope) -> bool {
    match scope {
        // Places: tabs, machines, each one's workspace and stats.
        Scope::Servers => {
            matches!(row.action, Action::Tab(_) | Action::Server(_) | Action::Workspace(_)) || (row.group == Group::GoTo && matches!(row.action, Action::Stats { .. }))
        }
        Scope::Commands => row.runs() || row.group == Group::Window,
        Scope::Folders => matches!(row.action, Action::Files { .. }),
        Scope::Settings => row.group == Group::Settings,
    }
}

/// The list for what was typed: the rows that answer it, group by group, the best first
/// within each.
pub fn rows(ctx: &Context, query: &Query) -> Vec<Row> {
    let mut scored: Vec<(u8, Row)> = all_rows(ctx, query)
        .into_iter()
        .filter(|row| query.scope.is_none_or(|scope| in_scope(row, scope)))
        .filter_map(|row| {
            let score = if row.typed { 1 } else { row.score(query) };
            (score > 0).then_some((score, row))
        })
        .collect();
    // Stable: rows that answer equally well keep the order they were listed in.
    scored.sort_by(|(a_score, a), (b_score, b)| a.group.cmp(&b.group).then(b_score.cmp(a_score)));
    scored.into_iter().map(|(_, row)| row).collect()
}

/// The row highlighted when the list changes: the first that doesn't run a command on a
/// server. `None` when every row does: Return then does nothing until you move to one.
pub fn first_choice(rows: &[Row]) -> Option<usize> {
    rows.iter().position(|row| !row.runs())
}

/// The highlight after an arrow key (`1` down, `-1` up), around the ends. With nothing
/// highlighted, down is the first row and up the last.
pub fn step(len: usize, hot: Option<usize>, by: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let len = len as i32;
    Some(match hot {
        Some(at) => (at as i32 + by).rem_euclid(len) as usize,
        None if by >= 0 => 0,
        None => (len - 1) as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EnvLabel, Snippets};
    use crate::workbench::{Open, Server};

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    fn bench() -> Workbench {
        Workbench::new(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
        ])
    }

    fn snippets() -> Vec<Snippet> {
        Snippets::default().snippets
    }

    fn list(bench: &Workbench, snippets: &[Snippet], typed: &str) -> Vec<Row> {
        list_with(bench, snippets, &Bookmarks::default(), typed)
    }

    fn list_with(bench: &Workbench, snippets: &[Snippet], bookmarks: &Bookmarks, typed: &str) -> Vec<Row> {
        let shortcuts = IndexMap::new();
        let now = Now { platform: Platform::Linux, ..Now::default() };
        rows(&Context { bench, snippets, mac: false, shortcuts: &shortcuts, bookmarks, now }, &parse(typed))
    }

    fn labels(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.label.as_str()).collect()
    }

    #[test]
    fn a_prefix_narrows_to_one_kind_of_thing() {
        assert_eq!(parse("db main"), Query { scope: None, text: "db main".into(), words: vec!["db".into(), "main".into()] });
        assert_eq!(parse("srv:"), Query { scope: Some(Scope::Servers), text: String::new(), words: vec![] });
        assert_eq!(parse("  CMD:  Docker  PS "), Query { scope: Some(Scope::Commands), text: "Docker  PS".into(), words: vec!["docker".into(), "ps".into()] });
        assert_eq!(parse("file:shop").scope, Some(Scope::Folders));
        assert_eq!(parse("set: theme").scope, Some(Scope::Settings));
        // Not a prefix: a word that only starts like one, or one with no colon.
        assert_eq!(parse("server:x").scope, None);
        assert_eq!(parse("cmd").scope, None);
        assert_eq!(parse("ярлык:").scope, None);
        assert!(parse("  ").is_empty());
    }

    #[test]
    fn the_empty_list_starts_with_files_on_the_server_on_screen() {
        let mut w = bench();
        w.go_to(WEB);
        let agent = w.open_with(TabKind::Terminal, DB, Open { title: Some("Migration 0042".into()), agent: Some("Claude Code".into()), ..Open::default() });
        w.go_to("local");
        w.go_to(WEB);
        w.set_needs_you(agent, true);
        let rows = list(&w, &snippets(), "");
        assert_eq!(rows[0].label, "Files");
        assert_eq!(rows[0].group, Group::OnServer);
        assert_eq!(first_choice(&rows), Some(0));
        // Then what can be run there, then where to go: the tab that waits comes first.
        let go: Vec<&Row> = rows.iter().filter(|r| r.group == Group::GoTo).collect();
        assert_eq!((go[0].label.as_str(), go[0].note.as_str(), go[0].needs_you, &go[0].glyph), ("Migration 0042", "db-main", true, &Glyph::Agent));
        assert_eq!((go[1].label.as_str(), go[1].note.as_str()), ("Terminal", "This computer"));
        // The tab on screen isn't somewhere to go.
        assert!(!go.iter().any(|r| r.action == Action::Tab(w.active_id().unwrap())));
        // Every machine, with its address and its label.
        let db = go.iter().find(|r| r.action == Action::Server(DB.into())).unwrap();
        assert_eq!((db.note.as_str(), db.prod, &db.glyph), ("postgres@203.0.113.31", true, &Glyph::Server("database".into())));
        assert_eq!(heading(Group::OnServer, Some("web-7731")), "On web-7731");
        assert_eq!(placeholder(&w), "Search or run on web-7731");
        // Groups come in order.
        let groups: Vec<Group> = rows.iter().map(|r| r.group).collect();
        assert!(groups.windows(2).all(|g| g[0] <= g[1]), "{groups:?}");
    }

    #[test]
    fn files_remembers_the_folder_you_were_last_in() {
        let mut w = bench();
        w.go_to(WEB);
        assert_eq!(list(&w, &[], "")[0].note, "");
        let files = w.open_with(TabKind::Files, WEB, Open { path: Some("~/shop/releases".into()), ..Open::default() });
        w.go_to(WEB);
        let rows = list(&w, &[], "");
        assert_eq!(rows[0].note, "recent: ~/shop/releases");
        assert_eq!(rows[0].action, Action::Files { server: WEB.into(), path: Some("~/shop/releases".into()) });
        // Closed: it's still the folder to come back to.
        w.close(files);
        assert_eq!(list(&w, &[], "")[0].note, "recent: ~/shop/releases");
    }

    #[test]
    fn a_command_for_a_server_is_never_highlighted_by_itself() {
        let mut w = bench();
        w.go_to(WEB);
        let snippets = snippets();
        // Every row "disk" finds runs something on web-7731 (the snippet, and the text itself).
        let rows = list(&w, &snippets, "cmd: disk");
        assert!(!rows.is_empty());
        assert!(rows.iter().all(Row::runs), "{:?}", labels(&rows));
        assert_eq!(first_choice(&rows), None);
        // Down goes to the first, and Return there is yours to press.
        assert_eq!(step(rows.len(), None, 1), Some(0));
        // Without the prefix the same text finds nothing else either: still nothing chosen.
        let rows = list(&w, &snippets, "df -h");
        assert_eq!(first_choice(&rows), None);
        // "restart" finds a snippet and no place to go: nothing chosen. "term" finds a snippet
        // too ("-x tmpfs"), and "New terminal": that one is chosen, not the command.
        let rows = list(&w, &snippets, "term");
        let hot = first_choice(&rows).unwrap();
        assert!(!rows[hot].runs());
        assert!(rows[..hot].iter().all(Row::runs));
    }

    #[test]
    fn what_you_type_can_be_run_on_the_server_on_screen_and_production_says_so() {
        let mut w = bench();
        w.go_to(WEB);
        let rows = list(&w, &[], "docker compose ps");
        let typed = rows.iter().find(|r| r.runs()).expect("the typed command is a row");
        assert_eq!(typed.action, Action::Run { server: WEB.into(), command: "docker compose ps".into(), enter: true });
        assert_eq!((typed.note.as_str(), typed.mono, typed.prod), ("run on web-7731", true, true));
        // On a server that isn't Production: no label.
        w.go_to("host:staging");
        let rows = list(&w, &[], "uptime");
        assert!(!rows.iter().find(|r| r.runs()).unwrap().prod);
        // A snippet with blanks is typed, not run.
        let rows = list(&w, &snippets(), "follow log");
        let follow = rows.iter().find(|r| r.label.starts_with("docker logs")).unwrap();
        assert!(matches!(&follow.action, Action::Run { enter: false, .. }));
        assert!(has_blanks("docker logs {{container}}") && !has_blanks("echo }} {{") && !has_blanks("df -h"));
        // Nothing can be run from the main menu, or under a prefix that isn't commands.
        w.show_home();
        assert!(!list(&w, &snippets(), "uptime").iter().any(Row::runs));
        assert_eq!(placeholder(&w), "Servers, files, commands and settings");
        w.go_to(WEB);
        assert!(!list(&w, &snippets(), "srv: uptime").iter().any(Row::runs));
    }

    #[test]
    fn every_word_must_be_there_in_any_order_and_any_case() {
        let mut w = bench();
        w.go_to(WEB);
        let found = list(&w, &[], "srv: MAIN db");
        // The machine, then its workspace and its stats.
        assert_eq!(labels(&found), ["db-main", "Workspace for db-main", "Stats for db-main"]);
        assert!(list(&w, &[], "srv: db nowhere").is_empty());
        // By address, and by the extra words a row is known by.
        assert_eq!(labels(&list(&w, &[], "srv: 203.0.113.18")), ["web-staging"]);
        assert_eq!(list(&w, &[], "set: onion")[0].label, "Settings: Tor");
        // Nothing is closed yet: "undo" finds only itself, as a command to run.
        assert!(list(&w, &[], "undo").iter().all(Row::runs));
        let closed = w.new_terminal();
        w.close(closed);
        // Now the window's own command answers to it, and that is the row Return picks.
        let found = list(&w, &[], "cmd: undo");
        assert_eq!(labels(&found), ["undo", "Reopen closed tab"]);
        assert_eq!(first_choice(&found), Some(1));
    }

    #[test]
    fn a_name_that_starts_with_what_was_typed_comes_first() {
        let mut w = bench();
        w.go_to(DB);
        // "web" is the start of web-7731 and web-staging; db-main's tab is only on screen.
        let found = list(&w, &[], "srv: web");
        assert_eq!(labels(&found)[..2], ["web-7731", "web-staging"]);
        // "s": Settings rows start with it; "Close tab" and others only hold it.
        let found = list(&w, &[], "set: s");
        assert_eq!(found[0].label, "Settings");
        // In "Go to", "st" starts a word of web-staging; This computer only holds it in its words.
        // (The "Stats for …" rows start with it.)
        let found: Vec<Row> = list(&w, &[], "srv: st").into_iter().filter(|r| matches!(r.action, Action::Server(_))).collect();
        assert_eq!(found[0].label, "web-staging");
    }

    #[test]
    fn scopes_hold_what_their_names_say() {
        let mut w = bench();
        w.go_to(WEB);
        w.open(TabKind::Files, DB);
        w.go_to(WEB);
        let snippets = snippets();
        let of = |typed: &str| list(&w, &snippets, typed);
        // A tab, each machine, each one's workspace, and the stats of those not on screen.
        assert!(of("srv:").iter().all(|r| matches!(r.action, Action::Tab(_) | Action::Server(_) | Action::Workspace(_) | Action::Stats { .. })));
        assert_eq!(of("srv:").len(), 1 + 4 + 4 + 3);
        assert!(of("file:").iter().all(|r| matches!(r.action, Action::Files { .. })));
        assert_eq!(of("file:")[0].label, "Files");
        assert!(of("set:").iter().all(|r| r.group == Group::Settings));
        // "Settings", what 0.10 listed with it, its ten sections, then every row and shortcut.
        let set = of("set:");
        assert_eq!(set.iter().filter(|r| matches!(r.action, Action::Settings(Some(_)))).count(), 12);
        assert!(set.iter().filter(|r| matches!(r.action, Action::SettingsRow { .. })).count() > 60);
        assert!(!of("").iter().any(|r| matches!(r.action, Action::SettingsRow { .. })), "the empty list keeps to the sections");
        let commands = of("cmd:");
        assert_eq!(commands.iter().filter(|r| r.runs()).count(), snippets.len());
        assert!(commands.iter().any(|r| r.action == Action::NewTerminal));
        assert!(!commands.iter().any(|r| matches!(r.action, Action::Server(_) | Action::Settings(_))));
        // With no prefix everything is listed.
        assert!(of("").len() > commands.len() + 7);
    }

    #[test]
    fn shortcuts_show_as_their_system_writes_them() {
        let none = IndexMap::new();
        assert_eq!(shortcut_label(Shortcut::NewTerminal, false, &none).as_deref(), Some("Ctrl+Shift+T"));
        assert_eq!(shortcut_label(Shortcut::NewTerminal, true, &none).as_deref(), Some("⌘T"));
        assert_eq!(shortcut_label(Shortcut::ReopenTab, true, &none).as_deref(), Some("⇧⌘T"));
        assert_eq!(shortcut_label(Shortcut::Palette, false, &none).as_deref(), Some("Ctrl+Shift+L"));
        assert_eq!(shortcut_label(Shortcut::Palette, true, &none).as_deref(), Some("⌘L"));
        assert_eq!(shortcut_label(Shortcut::StepTab(1), false, &none), None);
        // Changed or turned off in Settings.
        let mut changed = IndexMap::new();
        changed.insert("newTerminal".to_owned(), Some(Keys { key: "n".into(), ctrl: Some(true), alt: Some(true), ..Keys::default() }));
        changed.insert("closePane".to_owned(), None);
        assert_eq!(shortcut_label(Shortcut::NewTerminal, false, &changed).as_deref(), Some("Ctrl+Alt+N"));
        assert_eq!(shortcut_label(Shortcut::CloseTab, false, &changed), None);
        let mut w = bench();
        w.go_to(WEB);
        let none = Bookmarks::default();
        let rows = rows(&Context { bench: &w, snippets: &[], mac: false, shortcuts: &changed, bookmarks: &none, now: Now::default() }, &parse("cmd:"));
        assert_eq!(rows.iter().find(|r| r.action == Action::NewTerminal).unwrap().keys.as_deref(), Some("Ctrl+Alt+N"));
        assert_eq!(rows.iter().find(|r| r.action == Action::CloseTab).unwrap().keys, None);
    }

    #[test]
    fn the_security_check_recipes_and_snippets_are_found() {
        let mut w = bench();
        w.go_to(WEB);
        let rows = list(&w, &[], "security");
        assert_eq!(rows[0].action, Action::Checkup { server: WEB.into() });
        assert_eq!(first_choice(&rows), Some(0));
        assert_eq!(list(&w, &[], "fail2ban")[0].action, Action::Recipes { server: WEB.into() });
        assert_eq!(list(&w, &[], "disk space")[0].action, Action::Disk { server: WEB.into() });
        assert!(list(&w, &[], "snippets").iter().any(|r| r.action == Action::Snippets));
        assert!(list(&w, &[], "changelog").iter().any(|r| r.action == Action::WhatsNew));
        assert!(list(&w, &[], "tour").iter().any(|r| r.action == Action::Tour));
        // This computer has no security check: it's for servers.
        w.go_to("local");
        assert!(!list(&w, &[], "").iter().any(|r| matches!(r.action, Action::Checkup { .. } | Action::Recipes { .. } | Action::Disk { .. })));
    }

    #[test]
    fn arrows_go_around_the_ends() {
        assert_eq!(step(0, None, 1), None);
        assert_eq!(step(3, None, -1), Some(2));
        assert_eq!(step(3, Some(2), 1), Some(0));
        assert_eq!(step(3, Some(0), -1), Some(2));
        assert_eq!(step(3, Some(1), 1), Some(2));
    }

    #[test]
    fn on_the_main_menu_it_lists_places_and_the_windows_commands() {
        let w = bench();
        let rows = list(&w, &snippets(), "");
        assert!(rows.iter().all(|r| r.group != Group::OnServer));
        assert_eq!(rows[0].label, "This computer");
        assert_eq!(first_choice(&rows), Some(0));
        // No tab is on screen: nothing to close, and the menu is where you are.
        assert!(!rows.iter().any(|r| matches!(r.action, Action::CloseTab | Action::MainMenu | Action::NewTerminal)));
        assert!(rows.iter().any(|r| r.action == Action::AddServer));
    }

    #[test]
    fn every_window_command_of_0_10_is_here_and_says_what_it_will_do() {
        let mut w = bench();
        w.go_to(WEB);
        let cmd = list(&w, &[], "cmd:");
        for label in [
            "Split right",
            "Split down",
            "Tabs side by side",
            "Arrange: main + stack",
            "Arrange: columns",
            "Arrange: rows",
            "Arrange: grid",
            "Find in the terminal",
            "Turn on compact mode",
            "Turn on streamer mode",
            "Connect through Tor",
            "Agents at work",
        ] {
            assert!(labels(&cmd).contains(&label), "{label}: {:?}", labels(&cmd));
        }
        let keys = |label: &str| cmd.iter().find(|r| r.label == label).and_then(|r| r.keys.clone());
        assert_eq!(keys("Split down").as_deref(), Some("Ctrl+Shift+E"));
        assert_eq!(keys("Turn on compact mode").as_deref(), Some("Ctrl+Shift+M"));
        assert_eq!(keys("Agents at work").as_deref(), Some("Ctrl+Shift+A"));
        // On now: the row turns it off.
        let shortcuts = IndexMap::new();
        let none = Bookmarks::default();
        let now = Now { streaming: true, compact: true, platform: Platform::Linux };
        w.set_tor(Tor::Starting);
        let on = rows(&Context { bench: &w, snippets: &[], mac: false, shortcuts: &shortcuts, bookmarks: &none, now }, &parse("cmd:"));
        for label in ["Turn off compact mode", "Turn off streamer mode", "Turn off Tor"] {
            assert!(labels(&on).contains(&label), "{label}");
        }
        // 0.10's App group, under set:.
        let set = list(&w, &[], "set:");
        for label in ["Change theme and colors", "Terminal look", "What's new", "Take the tour", "Check for updates"] {
            assert!(labels(&set).contains(&label), "{label}");
        }
        // Each machine's workspace and stats.
        let srv = list(&w, &[], "srv: db");
        assert!(srv.iter().any(|r| r.action == Action::Workspace(DB.into()) && r.label == "Workspace for db-main"));
        assert!(srv.iter().any(|r| r.action == Action::Stats { server: DB.into() } && r.label == "Stats for db-main"));
        // No terminal on screen, nothing to find in.
        w.show_home();
        assert!(!labels(&list(&w, &[], "cmd:")).contains(&"Find in the terminal"));
    }

    #[test]
    fn every_setting_and_shortcut_can_be_found_and_opens_at_its_row() {
        let w = bench();
        let found = list(&w, &[], "set: cursor blink");
        let blink = found.iter().find(|r| matches!(&r.action, Action::SettingsRow { section: "terminal", .. })).expect("the blinking cursor row");
        assert_eq!(blink.note, "Terminal");
        // Found without the prefix too, once something is typed; and shortcuts by their names.
        assert!(list(&w, &[], "split down").iter().any(|r| r.action == Action::SettingsRow { section: "keyboard", key: "shortcut-splitDown".into() }));
        assert!(list(&w, &[], "agents at work").iter().any(|r| r.action == Action::SettingsRow { section: "keyboard", key: "shortcut-agents".into() }));
    }

    #[test]
    fn folders_lists_every_machines_bookmarks_and_recent_folders() {
        let mut w = bench();
        w.go_to(WEB);
        let mut b = Bookmarks::default();
        bookmarks::toggle_pin(&mut b, DB, "/var/lib/postgresql");
        for path in ["/etc", "/var/lib/postgresql", "/srv/a", "/srv/b", "/srv/c", "/srv/d", "/srv/e"] {
            bookmarks::visit(&mut b, DB, path);
        }
        let found = list_with(&w, &[], &b, "file:");
        let on_db: Vec<&Row> = found.iter().filter(|r| matches!(&r.action, Action::Files { server, path: Some(_) } if server == DB)).collect();
        // The bookmark first, then five recent folders that aren't it.
        assert_eq!((on_db[0].label.as_str(), &on_db[0].glyph, on_db[0].note.as_str()), ("postgresql on db-main", &Glyph::Star, "/var/lib/postgresql"));
        assert_eq!(on_db.len(), 1 + 5);
        assert!(on_db[1..].iter().all(|r| r.glyph == Glyph::Folder && r.note != "/var/lib/postgresql"));
        assert!(list_with(&w, &[], &b, "file: postgres").iter().any(|r| r.label == "postgresql on db-main"));
    }
}
