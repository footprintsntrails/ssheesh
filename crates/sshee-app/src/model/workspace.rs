//! Tabs, panes and how they're arranged: `sshee-workspace`, version 2 (`src/lib/store.ts`).
//!
//! Each machine has a screen of its own with a layout of its tabs, kept in `layouts` by the
//! machine's target key (`local` or `host:<host id>`). A tab put beside another machine's tab
//! is a guest in that machine's layout but still its own machine's tab. A terminal pane's
//! `session_key` names the tmux session on the server: losing it orphans a running shell, so
//! nothing here ever drops one. A terminal on this computer has a `session_key` too, but no
//! tmux: it's a plain shell that ends with the app that opened it.
//!
//! Only the main window's workspace is in localStorage. A second window keeps its own in
//! sessionStorage, gone when it closes.

use super::json::{string_enum, Json, JsonMap};
use super::{Platform, Store};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workspace {
    /// Each machine's layout, by target key, in the order of the tab strip.
    pub layouts: IndexMap<String, LayoutNode>,
    /// Machines shown side by side (true) or one tab at a time (false); the rest follow Settings.
    pub split_on: IndexMap<String, bool>,
    /// Every pane, by id.
    pub panes: IndexMap<String, Pane>,
    /// The pane in use; on the main menu, the one to come back to. Empty for none.
    pub focused: String,
    /// Panes switched off: kept, with their sessions running, but not drawn.
    pub hidden: Vec<String>,
    /// Tabs in the order they were dragged to; the rest follow in layout order.
    pub tab_order: Vec<String>,
    /// The main menu is showing instead of a machine's tabs.
    pub home: bool,
    /// The tab last used on each machine, by target key.
    pub last_focused_for: IndexMap<String, String>,
    /// The tab last used on each screen, guests included.
    pub front_of: IndexMap<String, String>,
    /// Tabs closed lately, newest first (20 at most).
    pub closed_tabs: Vec<ClosedTab>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            layouts: IndexMap::new(),
            split_on: IndexMap::new(),
            panes: IndexMap::new(),
            focused: String::new(),
            hidden: vec![],
            tab_order: vec![],
            home: true,
            last_focused_for: IndexMap::new(),
            front_of: IndexMap::new(),
            closed_tabs: vec![],
            extra: JsonMap::new(),
        }
    }
}

impl Workspace {
    /// Every pane's session, in pane order: terminals on servers (`durable`) and on this
    /// computer (not).
    pub fn sessions(&self) -> Vec<SessionRef> {
        self.panes
            .iter()
            .filter_map(|(id, pane)| {
                let machine = pane.target.as_ref().map(TargetRef::key);
                Some(SessionRef::new(id.clone(), machine, pane.session_key.clone()?))
            })
            .collect()
    }

    /// Takes in the terminals opened in 0.10.14 after a return to it (plan, section 6.5):
    /// each pane of `from` whose session is new, that is neither among this workspace's panes
    /// nor in `seen`. `seen` is every session an earlier import brought over, so that a tab
    /// closed since in 1.x doesn't come back. Everything already here stays as it is.
    ///
    /// A pane taken in keeps its id and its number ("Terminal 2") where they're free here, and
    /// gets the next free ones where not. It goes on its machine's screen as 0.10's `openIn`
    /// puts a new tab there: beside the tab last used on that machine (else its first), after
    /// it, or before it when that one isn't a terminal; a machine without a screen gets one.
    /// Only terminals on a server come: one on this computer ended with 0.10.14, and a pane
    /// without a machine has no screen to go on (the tmux search finds its session).
    ///
    /// Returns the sessions taken in, in `from`'s pane order, with their ids here.
    pub fn adopt_new_sessions(&mut self, from: &Workspace, seen: &[String]) -> Vec<SessionRef> {
        let mut known: HashSet<String> = self.panes.values().filter_map(|p| p.session_key.clone()).collect();
        known.extend(seen.iter().cloned());
        let mut ids: HashSet<String> = self.panes.keys().cloned().collect();
        for layout in self.layouts.values() {
            layout.collect_ids(&mut ids);
        }
        let mut adopted = vec![];
        for (from_id, pane) in &from.panes {
            let (Some(key), Some(target)) = (pane.session_key.as_deref(), pane.target.as_ref()) else { continue };
            if target.kind == TargetKind::Local || !known.insert(key.to_owned()) {
                continue;
            }
            let id = fresh_id(from_id, "p", key, &ids);
            ids.insert(id.clone());
            let mut new = pane.clone();
            new.id = id.clone();
            new.n = Some(self.free_number(&new.kind, target, new.n));
            let machine = target.key();
            self.put_on_screen(&machine, &id, new.kind == PaneKind::Terminal, &mut ids, key);
            self.panes.insert(id.clone(), new);
            adopted.push(SessionRef::new(id, Some(machine), key.to_owned()));
        }
        adopted
    }

    /// `freeNumber`: `wanted` if no pane of this kind on this machine has it, else the lowest
    /// such number that's free. A pane without a number counts as 1, as in 0.10.
    fn free_number(&self, kind: &PaneKind, target: &TargetRef, wanted: Option<u32>) -> u32 {
        let machine = target.key();
        let taken: HashSet<u32> = self
            .panes
            .values()
            .filter(|p| p.kind == *kind && p.target.as_ref().is_some_and(|t| t.key() == machine))
            .map(|p| p.n.unwrap_or(1))
            .collect();
        match wanted {
            Some(n) if n > 0 && !taken.contains(&n) => n,
            _ => (1..).find(|n| !taken.contains(n)).unwrap_or(1),
        }
    }

    /// A new tab `id` on `machine`'s screen, as `openIn` places one.
    fn put_on_screen(&mut self, machine: &str, id: &str, terminal: bool, ids: &mut HashSet<String>, seed: &str) {
        let anchor = self.layouts.get(machine).and_then(|layout| {
            let on_screen = layout.pane_ids();
            let last = self.last_focused_for.get(machine).filter(|last| on_screen.contains(&last.as_str()));
            last.map(String::as_str).or(on_screen.first().copied()).map(str::to_owned)
        });
        let Some(anchor) = anchor else {
            self.layouts.insert(machine.to_owned(), LayoutNode::pane(id));
            return;
        };
        // A terminal opened next to files or stats goes on the left, where you read first.
        let before = terminal && self.panes.get(&anchor).is_some_and(|p| p.kind != PaneKind::Terminal);
        let split_id = fresh_id("", "s", seed, ids);
        ids.insert(split_id.clone());
        let layout = &self.layouts[machine];
        let placed = layout.insert_at(&anchor, Dir::Row, id, before, &split_id);
        self.layouts.insert(machine.to_owned(), placed);
    }
}

/// An id that isn't in `taken`: `wanted` if it's free, else `<prefix>_` and the first
/// letters and digits of `seed`, with a number on the end if that's taken too. 0.10's ids are
/// `<prefix>_` and eight random letters and digits; nothing reads them but as names.
fn fresh_id(wanted: &str, prefix: &str, seed: &str, taken: &HashSet<String>) -> String {
    if !wanted.is_empty() && !taken.contains(wanted) {
        return wanted.to_owned();
    }
    let letters: String = seed.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>().to_ascii_lowercase();
    let base = format!("{prefix}_{}", if letters.is_empty() { "new" } else { &letters });
    (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}{n}") }).find(|id| !taken.contains(id)).unwrap_or(base)
}

/// A pane's session and where it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The pane's id. Empty for a session found in a damaged workspace's text where its pane
    /// couldn't be read.
    pub pane: String,
    /// The machine's target key (`local`, `host:<host id>`); `None` when not known.
    pub machine: Option<String>,
    /// Names the tmux session on the machine.
    pub session_key: String,
    /// Whether the session can outlive 0.10, so 1.x can reattach to it: `false` for a
    /// terminal on this computer (a plain shell, no tmux, gone when 0.10 closed), `true` for
    /// one on a server or on a machine that isn't known.
    pub durable: bool,
}

impl SessionRef {
    pub(crate) fn new(pane: String, machine: Option<String>, session_key: String) -> Self {
        let durable = machine.as_deref() != Some("local");
        SessionRef { pane, machine, session_key, durable }
    }
}

/// Every object in a raw workspace, at any depth, with a `sessionKey` string, read without the
/// model: its `id` (else the key it's filed under), its machine and its key. What 0.10 had,
/// to check against what came over.
pub(crate) fn sessions_in(raw: &Json) -> Vec<SessionRef> {
    fn walk(value: &Json, filed_as: &str, out: &mut Vec<SessionRef>) {
        match value {
            Json::Array(items) => items.iter().for_each(|item| walk(item, "", out)),
            Json::Object(fields) => {
                if let Some(key) = fields.get("sessionKey").and_then(Json::as_str) {
                    let pane = fields.get("id").and_then(Json::as_str).unwrap_or(filed_as);
                    out.push(SessionRef::new(pane.to_owned(), machine_of(value), key.to_owned()));
                }
                for (name, field) in fields {
                    walk(field, name, out);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    walk(raw, "", &mut out);
    out
}

/// Every `sessionKey` in a workspace's text, found by looking for `"sessionKey":"…"` and
/// nothing else, so damage anywhere doesn't stop the search. Text inside a string can't be
/// mistaken for it: a quote there is escaped. A key whose text is cut off is left out.
pub(crate) fn session_keys_in_text(text: &str) -> Vec<String> {
    const NAME: &str = "\"sessionKey\"";
    let bytes = text.as_bytes();
    let skip_spaces = |mut at: usize| {
        while matches!(bytes.get(at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            at += 1;
        }
        at
    };
    let mut keys = vec![];
    for (found, _) in text.match_indices(NAME) {
        let colon = skip_spaces(found + NAME.len());
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        let start = skip_spaces(colon + 1);
        if bytes.get(start) != Some(&b'"') {
            continue;
        }
        let mut at = start + 1;
        let end = loop {
            match bytes.get(at) {
                None => break None,
                Some(b'\\') => at += 2,
                Some(b'"') => break Some(at),
                Some(_) => at += 1,
            }
        };
        let key = end.and_then(|end| text.get(start..=end)).and_then(|quoted| Json::parse(quoted).ok());
        if let Some(Json::String(key)) = key {
            keys.push(key);
        }
    }
    keys
}

impl Store for Workspace {
    const KEY: &'static str = "sshee-workspace";
    const VERSION: u64 = 2;
    const SHIPPED: &'static [u64] = &[1, 2];

    fn defaults(_platform: Platform) -> Self {
        Workspace::default()
    }

    fn migrate(state: JsonMap, from: u64, _platform: Platform) -> JsonMap {
        if from < 2 {
            split_by_machine(state)
        } else {
            state
        }
    }
}

/// A pane: a terminal, a file manager, stats, or an empty one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pane {
    pub id: String,
    pub kind: PaneKind,
    /// The machine it's on. `None` only for 0.5's empty start page, or a terminal 0.5 left
    /// without one, kept for its session.
    pub target: Option<TargetRef>,
    /// Names the tmux session on the server; stable, so reopening reattaches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    /// Files pane: the folder it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Terminal pane: typed once after the first connect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_command: Option<String>,
    /// Terminal pane: opened inside this container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerRef>,
    /// "Terminal 2": its number among the machine's panes of its kind. It never changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Files pane: a file in that folder to pick out and preview, once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<String>,
    /// Files pane: the first and last line of that file to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal_lines: Option<[u32; 2]>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

string_enum! {
    pub enum PaneKind { Empty = "empty", Terminal = "terminal", Files = "files", Stats = "stats", }
    default Empty
}

/// Where something runs: this computer or a server from the list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TargetRef {
    pub kind: TargetKind,
    /// For `Host`: the server's id (`saved:<random>` or `cfg:<Host alias>`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl TargetRef {
    pub fn local() -> Self {
        TargetRef { kind: TargetKind::Local, host_id: None, extra: JsonMap::new() }
    }

    pub fn host(host_id: impl Into<String>) -> Self {
        TargetRef { kind: TargetKind::Host, host_id: Some(host_id.into()), extra: JsonMap::new() }
    }

    /// The key 0.10 files a machine's things under: `local` or `host:<host id>`.
    pub fn key(&self) -> String {
        match self.kind {
            TargetKind::Local => "local".into(),
            _ => format!("host:{}", self.host_id.as_deref().unwrap_or("undefined")),
        }
    }
}

string_enum! {
    /// 0.10 reads anything but "local" as a server, so that's the default.
    pub enum TargetKind { Local = "local", Host = "host", }
    default Host
}

/// A terminal opened into a container: a shell, or following its logs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerRef {
    pub engine: Engine,
    pub id: String,
    pub name: String,
    pub run: ContainerRun,
    #[serde(flatten)]
    pub extra: JsonMap,
}

string_enum! {
    pub enum Engine { Docker = "docker", Podman = "podman", }
    default Docker
}

string_enum! {
    pub enum ContainerRun { Shell = "shell", Logs = "logs", }
    default Shell
}

/// A machine's layout: panes as leaves, splits in a row or a column between them. Split
/// sizes aren't saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutNode {
    pub kind: NodeKind,
    /// A pane's id, or the split's own.
    pub id: String,
    /// For a split.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<Dir>,
    /// For a split.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<LayoutNode>>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl LayoutNode {
    /// A layout of one pane.
    pub fn pane(id: &str) -> Self {
        LayoutNode { kind: NodeKind::Pane, id: id.to_owned(), dir: None, children: None, extra: JsonMap::new() }
    }

    /// The panes in it, in order.
    pub fn pane_ids(&self) -> Vec<&str> {
        let mut out = vec![];
        self.collect_panes(&mut out);
        out
    }

    fn collect_panes<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self.kind {
            NodeKind::Pane => out.push(&self.id),
            _ => self.children.iter().flatten().for_each(|child| child.collect_panes(out)),
        }
    }

    /// The ids of every node in it, panes and splits.
    fn collect_ids(&self, out: &mut HashSet<String>) {
        out.insert(self.id.clone());
        self.children.iter().flatten().for_each(|child| child.collect_ids(out));
    }

    /// `insertAt` in 0.10's layout.ts: the pane `new_id` next to the pane `target`, after it
    /// or `before` it. Beside a pane in a split going the same way it joins that split;
    /// otherwise the target becomes a new split (`split_id`) of the two.
    pub fn insert_at(&self, target: &str, dir: Dir, new_id: &str, before: bool, split_id: &str) -> LayoutNode {
        let leaf = LayoutNode::pane(new_id);
        if self.kind == NodeKind::Pane {
            if self.id != target {
                return self.clone();
            }
            let children = if before { vec![leaf, self.clone()] } else { vec![self.clone(), leaf] };
            return LayoutNode { kind: NodeKind::Split, id: split_id.to_owned(), dir: Some(dir), children: Some(children), extra: JsonMap::new() };
        }
        let children = self.children.as_deref().unwrap_or_default();
        let mut node = self.clone();
        match children.iter().position(|c| c.kind == NodeKind::Pane && c.id == target) {
            Some(at) if self.dir.as_ref() == Some(&dir) => {
                node.children.get_or_insert_default().insert(if before { at } else { at + 1 }, leaf);
            }
            _ => node.children = self.children.as_ref().map(|c| c.iter().map(|c| c.insert_at(target, dir.clone(), new_id, before, split_id)).collect()),
        }
        node
    }
}

string_enum! {
    pub enum NodeKind { Pane = "pane", Split = "split", }
    default Pane
}

string_enum! {
    /// A row puts its children side by side; a column, one above the other.
    pub enum Dir { Row = "row", Col = "col", }
    default Row
}

/// A tab that was closed, to bring back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClosedTab {
    pub kind: PaneKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerRef>,
    /// Milliseconds since 1970.
    pub closed_at: u64,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// From 0.5's one layout for every machine to one per machine (`splitByMachine` in
/// store.ts), keeping each machine's arrangement. The start page's empty pane goes, and
/// every tab gets the number it shows as.
///
/// Two differences from 0.10, both about not losing anything: a pane with a `sessionKey`
/// stays even without a machine (0.10 drops panes without one), and a malformed split is
/// read as empty where 0.10 would throw and start over with no tabs at all.
pub(crate) fn split_by_machine(saved: JsonMap) -> JsonMap {
    let mut rest = saved.clone();
    let root = rest.shift_remove("root").filter(Json::truthy).and_then(|root| repair(&root));
    let mut panes = JsonMap::new();
    if let Some(Json::Object(old)) = saved.get("panes") {
        for (key, pane) in old {
            let Some(fields) = pane.as_object() else { continue };
            let kept = fields.get("target").is_some_and(Json::truthy) || fields.get("sessionKey").is_some_and(Json::truthy);
            if kept {
                let id = fields.get("id").and_then(Json::as_str).unwrap_or(key);
                panes.insert(id.to_owned(), pane.clone());
            }
        }
    }
    let order = root.as_ref().map(json_pane_ids).unwrap_or_default();
    // Each pane's machine, worked out once, and the machines in the order their panes come.
    let machine_by_pane: HashMap<String, String> = panes.iter().filter_map(|(id, pane)| Some((id.clone(), machine_of(pane)?))).collect();
    let machines: IndexSet<String> = panes.keys().filter_map(|id| machine_by_pane.get(id).cloned()).collect();
    let mut layouts = JsonMap::new();
    for key in machines {
        let others: HashSet<&str> = order.iter().filter(|id| machine_by_pane.get(*id) != Some(&key)).map(String::as_str).collect();
        // `visibleTree`: the tree without the other machines' panes. Without any, it's the
        // tree as it is (a split of one stays, as in 0.10).
        let tree = root.as_ref().and_then(|root| if others.is_empty() { Some(root.clone()) } else { without_panes(root, &others) });
        let ids = tree.as_ref().map(json_pane_ids).unwrap_or_default();
        if let Some(tree) = tree {
            layouts.insert(key, tree);
        }
        let numbered = |pane: &Json| -> Option<String> {
            let n = pane.as_object()?.get("n").filter(|n| n.truthy())?;
            Some(format!("{}:{}", js_text(pane.as_object()?.get("kind")), js_text(Some(n))))
        };
        let mut taken: HashSet<String> = ids.iter().filter_map(|id| panes.get(id).and_then(numbered)).collect();
        for id in &ids {
            let Some(Json::Object(pane)) = panes.get_mut(id) else { continue };
            if pane.get("n").is_some_and(Json::truthy) {
                continue;
            }
            let kind = js_text(pane.get("kind"));
            let mut n = 1u64;
            while taken.contains(&format!("{kind}:{n}")) {
                n += 1;
            }
            taken.insert(format!("{kind}:{n}"));
            pane.insert("n".into(), Json::from(n));
        }
    }
    let focused = match saved.get("focused").and_then(Json::as_str) {
        Some(id) if !id.is_empty() && panes.contains_key(id) => id.to_owned(),
        _ => String::new(),
    };
    let home = focused.is_empty();
    rest.insert("layouts".into(), Json::Object(layouts));
    rest.insert("splitOn".into(), Json::Object(JsonMap::new()));
    rest.insert("panes".into(), Json::Object(panes));
    rest.insert("focused".into(), Json::from(focused.as_str()));
    rest.insert("home".into(), Json::Bool(home));
    rest
}

/// `targetKey` of a pane's machine, as 0.10 works it out; `None` without one.
fn machine_of(pane: &Json) -> Option<String> {
    let target = pane.as_object()?.get("target").filter(|t| t.truthy())?;
    let fields = target.as_object();
    if fields.and_then(|f| f.get("kind")).and_then(Json::as_str) == Some("local") {
        return Some("local".into());
    }
    Some(format!("host:{}", js_text(fields.and_then(|f| f.get("hostId")))))
}

/// A JSON value as JavaScript's template strings print it, for the few kinds 0.10 has here.
fn js_text(value: Option<&Json>) -> String {
    match value {
        None => "undefined".into(),
        Some(Json::Null) => "null".into(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => match n.as_f64() {
            Some(x) if n.is_f64() && x.fract() == 0.0 && x.abs() < 1e21 => format!("{x:.0}"),
            _ => n.to_string(),
        },
        Some(Json::Array(_)) => "[array]".into(),
        Some(Json::Object(_)) => "[object Object]".into(),
    }
}

/// `paneIds` on raw JSON: a pane gives its id, anything else its children's.
fn json_pane_ids(node: &Json) -> Vec<String> {
    let Some(fields) = node.as_object() else { return vec![] };
    if fields.get("kind").and_then(Json::as_str) == Some("pane") {
        return fields.get("id").and_then(Json::as_str).map(|id| vec![id.to_owned()]).unwrap_or_default();
    }
    match fields.get("children") {
        Some(Json::Array(children)) => children.iter().flat_map(json_pane_ids).collect(),
        _ => vec![],
    }
}

/// The tree without splits that have no list of children (or no children left after
/// that), which 0.10 can't read either; a split left with one child becomes that child, as
/// `removePane` does. A sound tree comes back unchanged.
fn repair(node: &Json) -> Option<Json> {
    let fields = node.as_object()?;
    if fields.get("kind").and_then(Json::as_str) == Some("pane") {
        return Some(node.clone());
    }
    let Some(Json::Array(children)) = fields.get("children") else { return None };
    let kept: Vec<Json> = children.iter().filter_map(repair).collect();
    if kept == *children {
        return Some(node.clone());
    }
    match kept.len() {
        0 => None,
        1 => kept.into_iter().next(),
        _ => {
            let mut split = fields.clone();
            split.insert("children".into(), Json::Array(kept));
            Some(Json::Object(split))
        }
    }
}

/// The tree without the panes in `gone`, in one pass; `None` if nothing is left. Splits left
/// with one child collapse into it. The same as 0.10's `removePane` once for each of them
/// (`visibleTree`), without copying the tree for every one.
fn without_panes(node: &Json, gone: &HashSet<&str>) -> Option<Json> {
    let fields = node.as_object()?;
    if fields.get("kind").and_then(Json::as_str) == Some("pane") {
        let id = fields.get("id").and_then(Json::as_str);
        return (!id.is_some_and(|id| gone.contains(id))).then(|| node.clone());
    }
    let children: Vec<Json> = match fields.get("children") {
        Some(Json::Array(children)) => children.iter().filter_map(|c| without_panes(c, gone)).collect(),
        _ => vec![],
    };
    match children.len() {
        0 => None,
        1 => children.into_iter().next(),
        _ => {
            let mut split = fields.clone();
            split.insert("children".into(), Json::Array(children));
            Some(Json::Object(split))
        }
    }
}
