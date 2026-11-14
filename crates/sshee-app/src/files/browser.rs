//! One Files tab's folder: which one was asked for, what is listed, in what order, what is
//! selected, and where Back leads. A state machine with no I/O: a method that needs a folder
//! listed returns a [`Load`], the screen sends it to the backend, and the answer comes back
//! through [`Browser::listed`] or [`Browser::failed`]. A port of the folder half of 0.10's
//! `FilesPane.tsx`.
//!
//! As in 0.10, the folder on screen stays while the next one loads: [`Browser::dir`] is the
//! folder whose rows are shown, [`Browser::asked`] the one on its way.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use super::format::bytes;
use super::{Clock, Entry, Failure, Listing, Tickets};
use crate::dir_cache::DirCache;
use crate::paths;

/// Keys typed this soon after one another spell one name (type-to-find).
pub const TYPE_AHEAD_MS: u64 = 700;

/// On every error: lists the folder again.
pub const TRY_AGAIN: &str = "Try again";
/// On an error that signing in fixes: a terminal on that machine, where ssh asks.
pub const OPEN_TERMINAL: &str = "Open terminal";
/// Under "This folder is empty" on a server.
pub const DROP_HINT: &str = "Drop files here to upload them.";

/// The column the list is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    /// Smallest, oldest or "a" first.
    pub asc: bool,
}

impl Default for Sort {
    fn default() -> Sort {
        Sort { key: SortKey::Name, asc: true }
    }
}

impl Sort {
    /// A click on a column's heading: the same column turns around; another starts with names
    /// from "a" and with sizes and dates from the biggest and newest.
    pub fn clicked(self, key: SortKey) -> Sort {
        Sort { key, asc: if self.key == key { !self.asc } else { key == SortKey::Name } }
    }
}

/// A run of digits, and what follows it.
fn digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at(end)
}

/// "file2" before "file10", case aside (0.10 sorted with `Intl.Collator`, numeric, base
/// sensitivity). Names that differ only by case keep a fixed order.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (Some(p), Some(q)) = (x.chars().next(), y.chars().next()) else {
            return x.len().cmp(&y.len()).then_with(|| a.cmp(b));
        };
        if p.is_ascii_digit() && q.is_ascii_digit() {
            let ((n, rest_x), (m, rest_y)) = (digits(x), digits(y));
            // As numbers, however long: without the zeros in front, the longer is the bigger.
            let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
            let by_value = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
            if by_value != Ordering::Equal {
                return by_value;
            }
            (x, y) = (rest_x, rest_y);
        } else {
            let by_letter = p.to_lowercase().cmp(q.to_lowercase());
            if by_letter != Ordering::Equal {
                return by_letter;
            }
            (x, y) = (&x[p.len_utf8()..], &y[q.len_utf8()..]);
        }
    }
}

/// The order a folder's entries are shown in, as places in `entries`: folders first, then by
/// the column; what the column can't tell apart goes by name.
pub fn sorted(entries: &[Entry], sort: Sort) -> Vec<usize> {
    let turn = |o: Ordering| if sort.asc { o } else { o.reverse() };
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&i, &j| {
        let (a, b) = (&entries[i], &entries[j]);
        b.is_dir().cmp(&a.is_dir()).then_with(|| match sort.key {
            SortKey::Size if a.size != b.size => turn(a.size.cmp(&b.size)),
            SortKey::Modified if a.modified != b.modified => turn(a.modified.cmp(&b.modified)),
            SortKey::Name => turn(natural_cmp(&a.name, &b.name)),
            _ => natural_cmp(&a.name, &b.name),
        })
    });
    order
}

/// A folder to list: send it to the backend with this ticket, and bring the answer back with
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    pub ticket: u64,
    pub path: String,
}

/// How a click picks rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// This row alone.
    Plain,
    /// Ctrl (⌘ on a Mac): this row in or out, the rest as they were.
    Toggle,
    /// Shift: every row from the last one clicked to this one.
    Range,
}

/// One visible row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row<'a> {
    /// Its place among the visible rows.
    pub index: usize,
    pub entry: &'a Entry,
    pub selected: bool,
}

/// What went wrong with the folder, as the screen shows it. Both offer [`TRY_AGAIN`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem<'a> {
    /// Instead of the list: the folder can't be shown.
    Page {
        /// "Sign in first" or "Can't open this folder".
        title: &'static str,
        message: &'a str,
        /// Offer [`OPEN_TERMINAL`].
        open_terminal: bool,
    },
    /// Above the list: the machine can't be asked, and this is the folder as it was last seen.
    Note { text: String, open_terminal: bool },
}

/// Why no rows are shown though the folder was listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Empty {
    /// "This folder is empty" (on a server, with [`DROP_HINT`] under it).
    Folder,
    /// Nothing has the filter's text in its name.
    NoMatch(String),
}

impl Empty {
    pub fn text(&self) -> String {
        match self {
            Empty::Folder => "This folder is empty".into(),
            Empty::NoMatch(filter) => format!("Nothing matches “{filter}”"),
        }
    }
}

/// The line under the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// "12 items, 3 hidden".
    pub items: String,
    /// "2 selected, 1.5 KB": the size is of the files among them.
    pub selected: Option<String>,
}

/// A key in the list, as far as Files cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Delete,
    F2,
    Escape,
    /// A key that types a character.
    Char(char),
}

/// What a key in the list does (0.10's `onListKey`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDoes {
    /// Alt+Left.
    Back,
    /// Alt+Right.
    Forward,
    /// The arrows: one row down (1) or up (-1), [`Browser::move_by`].
    Move(i32),
    /// Backspace, or the main modifier with Up: the folder above.
    FolderAbove,
    /// Enter on one selected row.
    Open,
    /// Delete, or the main modifier with Backspace: asks first.
    Delete,
    /// F2 on one selected row.
    Rename,
    /// The main modifier with F: the filter field takes the keyboard.
    Filter,
    /// The main modifier with A.
    SelectAll,
    /// Escape.
    ClearSelection,
    /// A letter: [`Browser::type_to_find`].
    Type(char),
}

/// What `key` does in the list. `primary` is ⌘ on a Mac and Ctrl elsewhere; `selected` is how
/// many rows are selected. `None`: the key isn't the list's.
pub fn key_does(key: ListKey, primary: bool, alt: bool, selected: usize) -> Option<KeyDoes> {
    Some(match key {
        ListKey::Left if alt => KeyDoes::Back,
        ListKey::Right if alt => KeyDoes::Forward,
        ListKey::Down => KeyDoes::Move(1),
        ListKey::Up if primary => KeyDoes::FolderAbove,
        ListKey::Up => KeyDoes::Move(-1),
        ListKey::Enter if selected == 1 => KeyDoes::Open,
        ListKey::Backspace if primary && selected > 0 => KeyDoes::Delete,
        ListKey::Delete if selected > 0 => KeyDoes::Delete,
        ListKey::Backspace if !primary => KeyDoes::FolderAbove,
        ListKey::F2 if selected == 1 => KeyDoes::Rename,
        ListKey::Char(c) if primary && !alt && c.eq_ignore_ascii_case(&'f') => KeyDoes::Filter,
        ListKey::Char(c) if primary && !alt && c.eq_ignore_ascii_case(&'a') => KeyDoes::SelectAll,
        ListKey::Escape => KeyDoes::ClearSelection,
        ListKey::Char(c) if !primary && !alt && !c.is_control() => KeyDoes::Type(c),
        _ => return None,
    })
}

/// One Files tab's folder state. See the module's docs.
#[derive(Debug)]
pub struct Browser {
    server: String,
    asked: String,
    listing: Option<Arc<Listing>>,
    clock: Clock,
    loading: bool,
    error: Option<Failure>,
    tickets: Tickets,
    /// The ticket of the load whose answer is still wanted; 0 when none is.
    waiting: u64,
    history: Vec<String>,
    future: Vec<String>,
    sort: Sort,
    show_hidden: bool,
    filter: String,
    /// The listing's entries in the sort's order (places in `listing.entries`).
    order: Vec<usize>,
    /// Of `order`, the rows that pass the hidden-files rule and the filter.
    rows: Vec<usize>,
    selected: Vec<String>,
    picked: HashSet<String>,
    anchor: Option<String>,
    typed: (String, u64),
    /// A name to pick out once a listing has it (just created, renamed, unpacked).
    reveal: Option<String>,
    scroll_to: Option<usize>,
}

impl Browser {
    /// A tab on `server` (its key) that will show `path`. Nothing is listed until
    /// [`Browser::start`].
    pub fn new(server: impl Into<String>, path: impl Into<String>) -> Browser {
        Browser {
            server: server.into(),
            asked: path.into(),
            listing: None,
            clock: Clock::default(),
            loading: false,
            error: None,
            tickets: Tickets::default(),
            waiting: 0,
            history: Vec::new(),
            future: Vec::new(),
            sort: Sort::default(),
            show_hidden: false,
            filter: String::new(),
            order: Vec::new(),
            rows: Vec::new(),
            selected: Vec::new(),
            picked: HashSet::new(),
            anchor: None,
            typed: (String::new(), 0),
            reveal: None,
            scroll_to: None,
        }
    }

    // ---- what is shown

    /// The machine's key.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The folder asked for: where the tab is heading while [`Browser::is_loading`].
    pub fn asked(&self) -> &str {
        &self.asked
    }

    /// The folder whose rows are on screen, by its full path; the one asked for while nothing
    /// is listed. Names in the list are joined to this.
    pub fn dir(&self) -> &str {
        self.listing.as_ref().map_or(&self.asked, |l| &l.path)
    }

    /// A name of the folder on screen as a full path.
    pub fn path_of(&self, name: &str) -> String {
        paths::join(self.dir(), name)
    }

    pub fn listing(&self) -> Option<&Arc<Listing>> {
        self.listing.as_ref()
    }

    /// When the listing on screen was taken, for [`super::modified_text`].
    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// A listing is on its way. With rows on screen it's a thin line; without, the page waits.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&Failure> {
        self.error.as_ref()
    }

    /// The error as the screen shows it: a page when there's no folder to show, a note above
    /// the folder as it was when only the machine can't be asked (0.10's offline rule).
    pub fn problem(&self) -> Option<Problem<'_>> {
        let error = self.error.as_ref()?;
        let open_terminal = error.needs_terminal();
        Some(match self.listing {
            Some(_) => Problem::Note { text: format!("Showing this folder as it was. {}", error.message), open_terminal },
            None => Problem::Page { title: if open_terminal { "Sign in first" } else { "Can't open this folder" }, message: &error.message, open_terminal },
        })
    }

    /// The ".." row: shown above the rows of every folder that has one above it.
    pub fn has_up_row(&self) -> bool {
        self.listing.is_some() && paths::has_parent(self.dir())
    }

    /// How many rows are visible (sorted, without hidden files unless shown, filtered).
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The visible row at `index`.
    pub fn row(&self, index: usize) -> Option<Row<'_>> {
        let entry = &self.listing.as_ref()?.entries[*self.rows.get(index)?];
        Some(Row { index, entry, selected: self.picked.contains(&entry.name) })
    }

    /// The visible rows, in order.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> + '_ {
        (0..self.rows.len()).filter_map(|i| self.row(i))
    }

    /// The place of a name among the visible rows.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        let entries = &self.listing.as_ref()?.entries;
        self.rows.iter().position(|&i| entries[i].name == name)
    }

    /// An entry of the folder on screen, visible or not.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.listing.as_ref()?.entries.iter().find(|e| e.name == name)
    }

    /// Why there are no rows: `None` while there are some, or before the folder is listed.
    pub fn empty(&self) -> Option<Empty> {
        if self.listing.is_none() || !self.rows.is_empty() {
            return None;
        }
        Some(if self.filter.trim().is_empty() { Empty::Folder } else { Empty::NoMatch(self.filter.clone()) })
    }

    /// How many dot files the folder has, shown or not.
    pub fn hidden_count(&self) -> usize {
        self.listing.as_ref().map_or(0, |l| l.entries.iter().filter(|e| e.is_hidden()).count())
    }

    /// The line under the list; `None` before the folder is listed.
    pub fn status(&self) -> Option<Status> {
        self.listing.as_ref()?;
        let n = self.rows.len();
        let mut items = format!("{n} {}", if n == 1 { "item" } else { "items" });
        let hidden = self.hidden_count();
        if !self.show_hidden && hidden > 0 {
            items.push_str(&format!(", {hidden} hidden"));
        }
        let picked = self.selected_entries();
        let selected = (!picked.is_empty()).then(|| {
            let files: Vec<&&Entry> = picked.iter().filter(|e| !e.is_dir()).collect();
            match files.is_empty() {
                true => format!("{} selected", picked.len()),
                false => format!("{} selected, {}", picked.len(), bytes(files.iter().map(|e| e.size).sum())),
            }
        });
        Some(Status { items, selected })
    }

    // ---- going places

    /// Lists the folder asked for. When it was seen a moment ago that listing is shown and
    /// nothing is asked (`None`), unless `force` or a name waits to be picked out.
    fn begin(&mut self, force: bool, cache: &DirCache, now_ms: u64) -> Option<Load> {
        self.error = None;
        let cached = cache.get(&self.server, &self.asked).map(|c| (c.listing.clone(), c.clock));
        let known = cached.is_some();
        if let Some((listing, clock)) = cached {
            self.show(listing, clock);
        }
        // Whatever was on its way is for a folder no longer wanted, or older than this.
        self.waiting = 0;
        if known && !force && self.reveal.is_none() && cache.is_fresh(&self.server, &self.asked, now_ms) {
            self.loading = false;
            return None;
        }
        self.loading = true;
        self.waiting = self.tickets.next();
        Some(Load { ticket: self.waiting, path: self.asked.clone() })
    }

    /// The tab is shown for the first time: lists its folder.
    pub fn start(&mut self, cache: &DirCache, now_ms: u64) -> Option<Load> {
        self.begin(false, cache, now_ms)
    }

    /// Lists the folder on screen again, whatever the cache has (the button, "Try again", and
    /// after anything changed in it).
    pub fn refresh(&mut self, cache: &DirCache, now_ms: u64) -> Option<Load> {
        self.begin(true, cache, now_ms)
    }

    fn head_to(&mut self, to: &str, cache: &DirCache, now_ms: u64) -> Option<Load> {
        self.set_selected(Vec::new());
        self.anchor = None;
        self.reveal = None;
        self.set_filter("");
        self.asked = to.to_owned();
        self.begin(false, cache, now_ms)
    }

    /// Shows another folder (a double click, the address bar, a bookmark). The folder left is
    /// where Back leads. Going where you are lists it again.
    pub fn go(&mut self, to: &str, cache: &DirCache, now_ms: u64) -> Option<Load> {
        if to == self.dir() && self.listing.is_some() {
            return self.refresh(cache, now_ms);
        }
        self.history.push(self.dir().to_owned());
        self.future.clear();
        self.head_to(to, cache, now_ms)
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn back(&mut self, cache: &DirCache, now_ms: u64) -> Option<Load> {
        let to = self.history.pop()?;
        self.future.push(self.dir().to_owned());
        self.head_to(&to, cache, now_ms)
    }

    pub fn forward(&mut self, cache: &DirCache, now_ms: u64) -> Option<Load> {
        let to = self.future.pop()?;
        self.history.push(self.dir().to_owned());
        self.head_to(&to, cache, now_ms)
    }

    /// The folder above (the ".." row, Backspace). Nothing at the top of the disk.
    pub fn up(&mut self, cache: &DirCache, now_ms: u64) -> Option<Load> {
        if !paths::has_parent(self.dir()) {
            return None;
        }
        let above = paths::parent(self.dir());
        self.go(&above, cache, now_ms)
    }

    /// Whether the answer with this ticket is the one still wanted. An answer to an older
    /// request is dropped before it reaches the cache: it may predate a change.
    pub fn waits_for(&self, ticket: u64) -> bool {
        ticket != 0 && ticket == self.waiting
    }

    /// A listing arrived, already through [`DirCache::remember`]. False when it answers an
    /// older request: then nothing changes. After true, [`Browser::dir`] is the folder's full
    /// path, which the tab and the recent folders should keep.
    pub fn listed(&mut self, ticket: u64, listing: Arc<Listing>, clock: Clock) -> bool {
        if ticket == 0 || ticket != self.waiting {
            return false;
        }
        self.waiting = 0;
        self.loading = false;
        self.error = None;
        // "~" turned out to be /home/deploy: that's the folder this tab is in.
        self.asked = listing.path.clone();
        self.show(listing, clock);
        if let Some(name) = self.reveal.take() {
            self.scroll_to = self.index_of(&name);
        }
        true
    }

    /// Listing failed. False when it answers an older request. When only the machine can't be
    /// asked (no network, not signed in) the folder as it was stays on screen under a note;
    /// otherwise the folder is gone or closed: its listing is forgotten and the error shows
    /// instead.
    pub fn failed(&mut self, ticket: u64, failure: Failure, cache: &mut DirCache) -> bool {
        if ticket == 0 || ticket != self.waiting {
            return false;
        }
        self.waiting = 0;
        self.loading = false;
        let offline = failure.is_offline();
        if !offline {
            cache.forget(&self.server, &self.asked);
        }
        // The folder still on screen may be the one before: that one isn't "as it was".
        let same = self.listing.as_ref().is_some_and(|l| l.path == self.asked || cache.get(&self.server, &self.asked).is_some_and(|c| Arc::ptr_eq(&c.listing, l)));
        if !(offline && same) {
            self.listing = None;
            self.order.clear();
            self.rows.clear();
        }
        self.error = Some(failure);
        true
    }

    fn show(&mut self, listing: Arc<Listing>, clock: Clock) {
        self.clock = clock;
        // The very same listing: nothing to sort again.
        if self.listing.as_ref().is_some_and(|l| Arc::ptr_eq(l, &listing)) {
            return;
        }
        self.order = sorted(&listing.entries, self.sort);
        self.listing = Some(listing);
        self.sift();
    }

    /// The visible rows again, from the sorted ones.
    fn sift(&mut self) {
        let Some(listing) = &self.listing else { return self.rows.clear() };
        let q = self.filter.trim().to_lowercase();
        self.rows = self
            .order
            .iter()
            .copied()
            .filter(|&i| {
                let e = &listing.entries[i];
                (self.show_hidden || !e.is_hidden()) && (q.is_empty() || e.name.to_lowercase().contains(&q))
            })
            .collect();
    }

    // ---- order and filters

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn set_sort(&mut self, sort: Sort) {
        if sort == self.sort {
            return;
        }
        self.sort = sort;
        if let Some(listing) = &self.listing {
            self.order = sorted(&listing.entries, sort);
        }
        self.sift();
    }

    /// A click on a column's heading ([`Sort::clicked`]).
    pub fn sort_by(&mut self, key: SortKey) {
        self.set_sort(self.sort.clicked(key));
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// "Show hidden files": dot files are listed too.
    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        self.sift();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The filter field's text: only names that hold it are shown, case aside.
    pub fn set_filter(&mut self, text: &str) {
        if text != self.filter {
            self.filter = text.to_owned();
            self.sift();
        }
    }

    // ---- selection

    fn set_selected(&mut self, names: Vec<String>) {
        self.picked = names.iter().cloned().collect();
        self.selected = names;
    }

    fn name_at(&self, index: usize) -> Option<&str> {
        self.row(index).map(|r| r.entry.name.as_str())
    }

    /// The selected names, in the order they were picked. A name that left the list (hidden,
    /// filtered out, deleted) may still be among them: see [`Browser::selected_entries`].
    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    pub fn is_selected(&self, name: &str) -> bool {
        self.picked.contains(name)
    }

    /// The selected rows that are visible, in the list's order: what an action applies to.
    pub fn selected_entries(&self) -> Vec<&Entry> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.rows().filter(|r| r.selected).map(|r| r.entry).collect()
    }

    /// A click on a row.
    pub fn click(&mut self, name: &str, click: Click) {
        let range = match (click, &self.anchor) {
            (Click::Range, Some(anchor)) => self.index_of(anchor).zip(self.index_of(name)),
            _ => None,
        };
        match (click, range) {
            (Click::Range, Some((a, b))) => {
                let names = (a.min(b)..=a.max(b)).filter_map(|i| self.name_at(i).map(str::to_owned)).collect();
                self.set_selected(names);
            }
            (Click::Toggle, _) => {
                let mut names = std::mem::take(&mut self.selected);
                match names.iter().position(|n| n == name) {
                    Some(i) => drop(names.remove(i)),
                    None => names.push(name.to_owned()),
                }
                self.set_selected(names);
                self.anchor = Some(name.to_owned());
            }
            // Shift with nothing to start from is a plain click.
            _ => self.select_only(name),
        }
    }

    /// A right click: a row outside the selection becomes the selection; one inside leaves it,
    /// so the menu is about all of them.
    pub fn context_click(&mut self, name: &str) {
        if !self.picked.contains(name) {
            self.select_only(name);
        }
    }

    /// This row alone, and ranges start from it.
    pub fn select_only(&mut self, name: &str) {
        self.set_selected(vec![name.to_owned()]);
        self.anchor = Some(name.to_owned());
    }

    /// Every visible row.
    pub fn select_all(&mut self) {
        let names = self.rows().map(|r| r.entry.name.clone()).collect();
        self.set_selected(names);
    }

    pub fn clear_selection(&mut self) {
        self.set_selected(Vec::new());
    }

    /// Picks `name` out, also when the list doesn't have it yet: the next listing is asked
    /// for even if one is fresh, and once it has the name the row is scrolled to
    /// ([`Browser::take_scroll`]). For what was just made, renamed or unpacked.
    pub fn select_when_listed(&mut self, name: &str) {
        self.select_only(name);
        self.reveal = Some(name.to_owned());
    }

    fn pick(&mut self, index: usize) -> Option<usize> {
        let index = index.min(self.rows.len().checked_sub(1)?);
        let name = self.name_at(index)?.to_owned();
        self.select_only(&name);
        self.scroll_to = Some(index);
        Some(index)
    }

    /// The arrows: the row `step` below the one last clicked (above for a negative step),
    /// stopping at the ends. With nothing selected, Down and Up both start at the top. Returns
    /// the row now selected.
    pub fn move_by(&mut self, step: i32) -> Option<usize> {
        let from = self.anchor.as_deref().or(self.selected.last().map(String::as_str)).and_then(|n| self.index_of(n));
        let to = match from {
            Some(i) => i.saturating_add_signed(step as isize),
            None => 0,
        };
        self.pick(to)
    }

    /// Selects the row at `index` alone (Home, End, a page up or down).
    pub fn move_to(&mut self, index: usize) -> Option<usize> {
        self.pick(index)
    }

    /// Type-to-find: letters typed within [`TYPE_AHEAD_MS`] of one another spell the start of
    /// a name, case aside, and the first visible row that starts so is selected. `now_ms` is
    /// any clock that only goes forward.
    pub fn type_to_find(&mut self, c: char, now_ms: u64) -> Option<usize> {
        if now_ms.saturating_sub(self.typed.1) >= TYPE_AHEAD_MS {
            self.typed.0.clear();
        }
        self.typed.0.extend(c.to_lowercase());
        self.typed.1 = now_ms;
        let hit = self.rows().position(|r| r.entry.name.to_lowercase().starts_with(&self.typed.0))?;
        self.pick(hit)
    }

    /// The row to bring into view, once: set by the arrows, type-to-find and a name picked out
    /// when its listing arrived.
    pub fn take_scroll(&mut self) -> Option<usize> {
        self.scroll_to.take()
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{dir, file, listing};
    use super::super::FailureKind;
    use super::*;

    const WEB: &str = "host:web";

    fn names(b: &Browser) -> Vec<&str> {
        b.rows().map(|r| r.entry.name.as_str()).collect()
    }

    fn home() -> Listing {
        listing(
            "/home/deploy",
            vec![
                file("README.md", 812, 300),
                dir("shop"),
                file(".env", 96, 200),
                file("deploy.sh", 1_536, 100),
                dir(".ssh"),
                file("notes10.txt", 10, 400),
                file("notes2.txt", 2_048, 500),
                dir("logs"),
            ],
        )
    }

    /// A browser with `home()` on screen, and the cache it came through.
    fn at_home() -> (Browser, DirCache) {
        let mut cache = DirCache::new();
        let mut b = Browser::new(WEB, "~");
        let load = b.start(&cache, 0).unwrap();
        assert_eq!(load, Load { ticket: 1, path: "~".into() });
        let shown = cache.remember(WEB, "~", home(), Clock { now: 1_000, utc_offset: 0 }, 0);
        assert!(b.listed(load.ticket, shown, Clock { now: 1_000, utc_offset: 0 }));
        (b, cache)
    }

    #[test]
    fn names_sort_with_their_numbers_case_aside() {
        let mut list = vec!["file10", "File2", "file1", "b", "A", "file2", "a", "007", "7a", "10", "_x", "é", "z"];
        list.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(list, ["007", "7a", "10", "_x", "A", "a", "b", "file1", "File2", "file2", "file10", "z", "é"]);
        assert_eq!(natural_cmp("a", "a"), Ordering::Equal);
        assert_eq!(natural_cmp("a1", "a01"), Ordering::Greater);
        assert_eq!(natural_cmp("v1.9", "v1.10"), Ordering::Less);
        assert_eq!(natural_cmp("x", "x1"), Ordering::Less);
        // Numbers longer than any integer type still compare.
        assert_eq!(natural_cmp("n99999999999999999999999999", "n100000000000000000000000000"), Ordering::Less);
    }

    #[test]
    fn folders_come_first_and_every_column_sorts_both_ways() {
        let (mut b, _) = at_home();
        b.set_show_hidden(true);
        assert_eq!(names(&b), [".ssh", "logs", "shop", ".env", "deploy.sh", "notes2.txt", "notes10.txt", "README.md"]);
        b.sort_by(SortKey::Name);
        assert_eq!(b.sort(), Sort { key: SortKey::Name, asc: false });
        assert_eq!(names(&b), ["shop", "logs", ".ssh", "README.md", "notes10.txt", "notes2.txt", "deploy.sh", ".env"]);
        // Another column starts from the biggest; folders (all one size) stay by name, a to z.
        b.sort_by(SortKey::Size);
        assert_eq!(b.sort(), Sort { key: SortKey::Size, asc: false });
        assert_eq!(names(&b), [".ssh", "logs", "shop", "notes2.txt", "deploy.sh", "README.md", ".env", "notes10.txt"]);
        b.sort_by(SortKey::Size);
        assert_eq!(names(&b), [".ssh", "logs", "shop", "notes10.txt", ".env", "README.md", "deploy.sh", "notes2.txt"]);
        b.sort_by(SortKey::Modified);
        assert_eq!(names(&b), [".ssh", "logs", "shop", "notes2.txt", "notes10.txt", "README.md", ".env", "deploy.sh"]);
        assert_eq!(Sort::default().clicked(SortKey::Modified).clicked(SortKey::Name), Sort::default());
    }

    #[test]
    fn dot_files_are_hidden_until_asked_for_and_counted() {
        let (mut b, _) = at_home();
        assert_eq!(names(&b), ["logs", "shop", "deploy.sh", "notes2.txt", "notes10.txt", "README.md"]);
        assert_eq!(b.hidden_count(), 2);
        assert_eq!(b.status().unwrap(), Status { items: "6 items, 2 hidden".into(), selected: None });
        b.set_show_hidden(true);
        assert_eq!(b.len(), 8);
        assert_eq!(b.status().unwrap().items, "8 items");
        // An entry that isn't shown is still an entry.
        b.set_show_hidden(false);
        assert!(b.entry(".env").is_some() && b.index_of(".env").is_none());
    }

    #[test]
    fn the_filter_narrows_the_rows_and_says_when_nothing_matches() {
        let (mut b, _) = at_home();
        b.set_filter(" NOTES ");
        assert_eq!(names(&b), ["notes2.txt", "notes10.txt"]);
        assert_eq!(b.status().unwrap().items, "2 items, 2 hidden");
        b.set_filter("sh");
        assert_eq!(names(&b), ["shop", "deploy.sh"]);
        b.set_filter("zzz");
        assert_eq!(b.empty(), Some(Empty::NoMatch("zzz".into())));
        assert_eq!(b.empty().unwrap().text(), "Nothing matches “zzz”");
        assert_eq!(b.status().unwrap().items, "0 items, 2 hidden");
        b.set_filter("");
        assert_eq!(b.empty(), None);
    }

    #[test]
    fn an_empty_folder_says_so_once_it_is_listed() {
        let mut cache = DirCache::new();
        let mut b = Browser::new(WEB, "/srv/empty");
        assert_eq!((b.empty(), b.status()), (None, None));
        let load = b.start(&cache, 0).unwrap();
        assert!(b.is_loading() && b.empty().is_none());
        let shown = cache.remember(WEB, "/srv/empty", listing("/srv/empty", vec![file(".keep", 0, 1)]), Clock::default(), 0);
        b.listed(load.ticket, shown, Clock::default());
        assert_eq!(b.empty().unwrap().text(), "This folder is empty");
        assert_eq!(b.status().unwrap().items, "0 items, 1 hidden");
        assert!(b.has_up_row());
    }

    #[test]
    fn clicks_select_one_toggle_and_ranges() {
        let (mut b, _) = at_home();
        b.click("shop", Click::Plain);
        assert_eq!(b.selected(), ["shop"]);
        b.click("notes2.txt", Click::Range);
        assert_eq!(b.selected(), ["shop", "deploy.sh", "notes2.txt"]);
        // The range starts where the last plain click was, in either direction.
        b.click("logs", Click::Range);
        assert_eq!(b.selected(), ["logs", "shop"]);
        b.click("README.md", Click::Toggle);
        assert_eq!(b.selected(), ["logs", "shop", "README.md"]);
        b.click("shop", Click::Toggle);
        assert_eq!(b.selected(), ["logs", "README.md"]);
        assert!(b.is_selected("logs") && !b.is_selected("shop"));
        // A toggled row is where the next range starts.
        b.click("deploy.sh", Click::Range);
        assert_eq!(b.selected(), ["shop", "deploy.sh"]);
        b.click("README.md", Click::Plain);
        assert_eq!(b.selected(), ["README.md"]);
        assert_eq!(b.row(5).map(|r| (r.entry.name.as_str(), r.selected)), Some(("README.md", true)));
        b.clear_selection();
        assert!(b.selected().is_empty());
        // Shift with nowhere to start from selects the row.
        let mut fresh = at_home().0;
        fresh.click("logs", Click::Range);
        assert_eq!(fresh.selected(), ["logs"]);
    }

    #[test]
    fn select_all_takes_what_is_visible_and_the_status_adds_up_the_files() {
        let (mut b, _) = at_home();
        b.select_all();
        assert_eq!(b.selected().len(), 6);
        assert_eq!(b.status().unwrap().selected.as_deref(), Some("6 selected, 4.3 KB"));
        b.click("shop", Click::Plain);
        assert_eq!(b.status().unwrap().selected.as_deref(), Some("1 selected"));
        b.click("deploy.sh", Click::Toggle);
        assert_eq!(b.status().unwrap().selected.as_deref(), Some("2 selected, 1.5 KB"));
        // What the filter hides isn't acted on, though it stays picked.
        b.set_filter("shop");
        assert_eq!(b.selected_entries().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["shop"]);
        assert_eq!(b.selected().len(), 2);
    }

    #[test]
    fn a_right_click_keeps_a_selection_it_lands_in() {
        let (mut b, _) = at_home();
        b.click("shop", Click::Plain);
        b.click("logs", Click::Toggle);
        b.context_click("logs");
        assert_eq!(b.selected(), ["shop", "logs"]);
        b.context_click("README.md");
        assert_eq!(b.selected(), ["README.md"]);
    }

    #[test]
    fn arrows_walk_the_rows_and_stop_at_the_ends() {
        let (mut b, _) = at_home();
        assert_eq!(b.move_by(1), Some(0));
        assert_eq!(b.selected(), ["logs"]);
        assert_eq!(b.move_by(1), Some(1));
        assert_eq!(b.move_by(-1), Some(0));
        assert_eq!(b.move_by(-1), Some(0));
        assert_eq!(b.move_to(usize::MAX), Some(5));
        assert_eq!(b.move_by(1), Some(5));
        assert_eq!(b.selected(), ["README.md"]);
        assert_eq!(b.take_scroll(), Some(5));
        assert_eq!(b.take_scroll(), None);
        // Up with nothing selected starts at the top, as in 0.10.
        b.clear_selection();
        b.anchor = None;
        assert_eq!(b.move_by(-1), Some(0));
        // No rows, nothing to walk.
        b.set_filter("zzz");
        assert_eq!(b.move_by(1), None);
    }

    #[test]
    fn typing_finds_a_name_and_quick_keys_spell_more_of_it() {
        let (mut b, _) = at_home();
        assert_eq!(b.type_to_find('n', 10_000), Some(3));
        assert_eq!(b.selected(), ["notes2.txt"]);
        assert_eq!(b.type_to_find('O', 10_300), Some(3));
        // "notes1" is notes10.txt.
        for (i, c) in "tes1".chars().enumerate() {
            b.type_to_find(c, 10_400 + i as u64 * 100);
        }
        assert_eq!(b.selected(), ["notes10.txt"]);
        // After a pause the next key starts over.
        assert_eq!(b.type_to_find('s', 11_400), Some(1));
        assert_eq!(b.selected(), ["shop"]);
        // Nothing starts with it: the selection stays.
        assert_eq!(b.type_to_find('q', 20_000), None);
        assert_eq!(b.selected(), ["shop"]);
    }

    #[test]
    fn keys_in_the_list() {
        use KeyDoes::*;
        assert_eq!(key_does(ListKey::Left, false, true, 0), Some(Back));
        assert_eq!(key_does(ListKey::Right, false, true, 0), Some(Forward));
        assert_eq!(key_does(ListKey::Right, false, false, 1), None);
        assert_eq!(key_does(ListKey::Down, false, false, 0), Some(Move(1)));
        assert_eq!(key_does(ListKey::Up, false, false, 0), Some(Move(-1)));
        assert_eq!(key_does(ListKey::Up, true, false, 0), Some(FolderAbove));
        assert_eq!(key_does(ListKey::Backspace, false, false, 2), Some(FolderAbove));
        assert_eq!(key_does(ListKey::Backspace, true, false, 2), Some(Delete));
        assert_eq!(key_does(ListKey::Backspace, true, false, 0), None);
        assert_eq!(key_does(ListKey::Delete, false, false, 3), Some(Delete));
        assert_eq!(key_does(ListKey::Delete, false, false, 0), None);
        assert_eq!(key_does(ListKey::Enter, false, false, 1), Some(Open));
        assert_eq!(key_does(ListKey::Enter, false, false, 2), None);
        assert_eq!(key_does(ListKey::F2, false, false, 1), Some(Rename));
        assert_eq!(key_does(ListKey::F2, false, false, 0), None);
        assert_eq!(key_does(ListKey::Escape, false, false, 0), Some(ClearSelection));
        assert_eq!(key_does(ListKey::Char('F'), true, false, 0), Some(Filter));
        assert_eq!(key_does(ListKey::Char('a'), true, false, 0), Some(SelectAll));
        assert_eq!(key_does(ListKey::Char('a'), false, false, 0), Some(Type('a')));
        assert_eq!(key_does(ListKey::Char('a'), false, true, 0), None);
        assert_eq!(key_does(ListKey::Char('c'), true, false, 0), None);
    }

    #[test]
    fn the_home_folder_becomes_its_full_path_and_the_folder_on_screen_stays_while_the_next_loads() {
        let (mut b, cache) = at_home();
        assert_eq!((b.asked(), b.dir()), ("/home/deploy", "/home/deploy"));
        assert_eq!(b.path_of("shop"), "/home/deploy/shop");
        assert!(!b.is_loading() && b.has_up_row());
        assert_eq!(b.clock().now, 1_000);
        b.click("shop", Click::Plain);
        b.set_filter("s");
        let load = b.go("/home/deploy/shop", &cache, 100).unwrap();
        assert_eq!(load, Load { ticket: 2, path: "/home/deploy/shop".into() });
        // Still the home folder's rows, with nothing selected and no filter.
        assert_eq!((b.asked(), b.dir(), b.is_loading()), ("/home/deploy/shop", "/home/deploy", true));
        assert!(b.selected().is_empty() && b.filter().is_empty() && b.len() == 6);
    }

    #[test]
    fn back_forward_and_up() {
        let (mut b, mut cache) = at_home();
        assert!(!b.can_go_back() && !b.can_go_forward());
        let land = |b: &mut Browser, cache: &mut DirCache, load: Load, path: &str| {
            let shown = cache.remember(WEB, &load.path, listing(path, vec![]), Clock::default(), 0);
            assert!(b.listed(load.ticket, shown, Clock::default()));
        };
        let load = b.go("/home/deploy/shop", &cache, 10_000).unwrap();
        land(&mut b, &mut cache, load, "/home/deploy/shop");
        let load = b.go("/etc", &cache, 20_000).unwrap();
        land(&mut b, &mut cache, load, "/etc");
        assert!(b.can_go_back() && !b.can_go_forward());

        // Back, long after: the folder as it was is shown at once, and listed again.
        let load = b.back(&cache, 30_000).unwrap();
        assert_eq!((load.path.as_str(), b.dir(), b.is_loading()), ("/home/deploy/shop", "/home/deploy/shop", true));
        land(&mut b, &mut cache, load, "/home/deploy/shop");
        assert!(b.can_go_forward());
        let load = b.forward(&cache, 40_000).unwrap();
        land(&mut b, &mut cache, load, "/etc");
        assert!(!b.can_go_forward());
        // Going somewhere new forgets where Forward led.
        b.back(&cache, 40_000);
        let load = b.go("/var", &cache, 40_000).unwrap();
        land(&mut b, &mut cache, load, "/var");
        assert!(!b.can_go_forward());

        // Up from /var is the top, and the top has nothing above.
        let load = b.up(&cache, 50_000).unwrap();
        assert_eq!(load.path, "/");
        land(&mut b, &mut cache, load, "/");
        assert!(!b.has_up_row());
        assert_eq!(b.up(&cache, 50_000), None);
        // Up remembers where it came from like any other move.
        let load = b.back(&cache, 60_000).unwrap();
        assert_eq!(load.path, "/var");
    }

    #[test]
    fn a_folder_seen_a_moment_ago_is_shown_without_asking_and_refresh_asks_anyway() {
        let (mut b, mut cache) = at_home();
        let load = b.go("/etc", &cache, 1_000).unwrap();
        let shown = cache.remember(WEB, "/etc", listing("/etc", vec![file("hosts", 1, 1)]), Clock::default(), 1_000);
        b.listed(load.ticket, shown, Clock::default());
        // Back within three seconds of the home folder's listing.
        assert_eq!(b.back(&cache, 2_500), None);
        assert_eq!((b.dir(), b.is_loading(), b.len()), ("/home/deploy", false, 6));
        // Going where you are, and the button, list again.
        assert!(b.go("/home/deploy", &cache, 2_600).is_some());
        assert!(b.refresh(&cache, 2_600).is_some());
        assert!(b.is_loading());
    }

    #[test]
    fn an_answer_to_an_older_request_is_ignored() {
        let (mut b, mut cache) = at_home();
        let first = b.go("/etc", &cache, 10_000).unwrap();
        let second = b.go("/var", &cache, 10_000).unwrap();
        assert_ne!(first.ticket, second.ticket);
        let etc = cache.remember(WEB, "/etc", listing("/etc", vec![]), Clock::default(), 10_000);
        assert!(!b.listed(first.ticket, etc, Clock::default()));
        assert_eq!((b.dir(), b.is_loading()), ("/home/deploy", true));
        assert!(!b.failed(first.ticket, Failure::new(FailureKind::Failed, "old"), &mut cache));
        assert!(b.error().is_none());
        let var = cache.remember(WEB, "/var", listing("/var", vec![]), Clock::default(), 10_000);
        assert!(b.listed(second.ticket, var.clone(), Clock::default()));
        assert_eq!(b.dir(), "/var");
        // The same answer twice, or one nobody asked for.
        assert!(!b.listed(second.ticket, var.clone(), Clock::default()));
        assert!(!b.listed(0, var, Clock::default()));
        // A fresh folder shown from the cache cancels what was on its way.
        let slow = b.refresh(&cache, 10_000).unwrap();
        assert_eq!(b.go("/etc", &cache, 10_100), None);
        assert!(!b.listed(slow.ticket, cache.get(WEB, "/var").unwrap().listing.clone(), Clock::default()));
        assert_eq!(b.dir(), "/etc");
    }

    #[test]
    fn the_same_listing_again_keeps_rows_and_takes_the_new_clock() {
        let (mut b, mut cache) = at_home();
        b.click("shop", Click::Plain);
        let load = b.refresh(&cache, 60_000).unwrap();
        let shown = cache.remember(WEB, "/home/deploy", home(), Clock { now: 2_000, utc_offset: 0 }, 60_000);
        assert!(Arc::ptr_eq(&shown, b.listing().unwrap()));
        assert!(b.listed(load.ticket, shown, Clock { now: 2_000, utc_offset: 0 }));
        assert_eq!((b.clock().now, b.selected().len(), b.len()), (2_000, 1, 6));
    }

    #[test]
    fn offline_the_folder_as_it_was_stays_under_a_note() {
        let (mut b, mut cache) = at_home();
        let load = b.refresh(&cache, 60_000).unwrap();
        assert!(b.failed(load.ticket, Failure::new(FailureKind::Network, "Can't reach the server."), &mut cache));
        assert!(!b.is_loading() && b.len() == 6);
        assert_eq!(b.problem(), Some(Problem::Note { text: "Showing this folder as it was. Can't reach the server.".into(), open_terminal: false }));
        assert!(cache.get(WEB, "/home/deploy").is_some());
        // Not signed in: the same, with a way to sign in.
        let load = b.refresh(&cache, 61_000).unwrap();
        assert!(b.problem().is_none(), "asking again clears the note");
        b.failed(load.ticket, Failure::new(FailureKind::Auth, "The server wants a password."), &mut cache);
        assert!(matches!(b.problem(), Some(Problem::Note { open_terminal: true, .. })));
        // It works again: the note goes.
        let load = b.refresh(&cache, 62_000).unwrap();
        let shown = cache.remember(WEB, "/home/deploy", home(), Clock::default(), 62_000);
        b.listed(load.ticket, shown, Clock::default());
        assert!(b.problem().is_none());
    }

    #[test]
    fn a_folder_that_is_gone_shows_the_error_instead_and_is_forgotten() {
        let (mut b, mut cache) = at_home();
        let load = b.refresh(&cache, 60_000).unwrap();
        b.failed(load.ticket, Failure::new(FailureKind::NotFound, "This folder doesn't exist or you can't open it."), &mut cache);
        assert_eq!(b.problem(), Some(Problem::Page { title: "Can't open this folder", message: "This folder doesn't exist or you can't open it.", open_terminal: false }));
        assert!(b.listing().is_none() && b.is_empty() && b.status().is_none());
        assert!(cache.get(WEB, "/home/deploy").is_none() && cache.get(WEB, "~").is_none());
        assert_eq!(b.dir(), "/home/deploy");
    }

    #[test]
    fn offline_on_the_way_to_another_folder_shows_the_error_not_the_folder_before() {
        let (mut b, mut cache) = at_home();
        let load = b.go("/etc", &cache, 60_000).unwrap();
        b.failed(load.ticket, Failure::new(FailureKind::HostKey, "The server's key isn't known yet."), &mut cache);
        assert_eq!(b.problem(), Some(Problem::Page { title: "Sign in first", message: "The server's key isn't known yet.", open_terminal: true }));
        assert_eq!(b.dir(), "/etc");
        // Back still leads to the folder before, which the cache has.
        assert!(b.back(&cache, 60_100).is_some());
        assert_eq!(b.dir(), "/home/deploy");
        // Offline with the folder known from the cache: it stays.
        let load = b.go("/etc", &cache, 70_000).unwrap();
        let etc = cache.remember(WEB, "/etc", listing("/etc", vec![]), Clock::default(), 70_000);
        b.listed(load.ticket, etc, Clock::default());
        b.back(&cache, 80_000);
        let load = b.forward(&cache, 90_000).unwrap();
        b.failed(load.ticket, Failure::new(FailureKind::Network, "No network."), &mut cache);
        assert!(matches!(b.problem(), Some(Problem::Note { .. })));
        assert_eq!(b.dir(), "/etc");
    }

    #[test]
    fn a_name_is_picked_out_once_the_listing_has_it() {
        let (mut b, mut cache) = at_home();
        b.select_when_listed("new folder");
        assert_eq!(b.selected(), ["new folder"]);
        assert!(b.selected_entries().is_empty());
        // Fresh or not, the folder is listed again: the name may be newer than the listing.
        let load = b.refresh(&cache, 100).unwrap();
        let mut grown = home();
        grown.entries.push(dir("new folder"));
        let shown = cache.remember(WEB, "/home/deploy", grown, Clock::default(), 200);
        b.listed(load.ticket, shown, Clock::default());
        assert_eq!(b.selected_entries()[0].name, "new folder");
        assert_eq!(b.take_scroll(), b.index_of("new folder"));
        assert!(b.take_scroll().is_none());
        // Without the refresh, a fresh cache alone doesn't satisfy a name waiting.
        b.select_when_listed("other");
        assert!(b.start(&cache, 300).is_some());
    }
}
