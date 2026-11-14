//! What a right click in Files offers: on a row, on several selected rows, and on the empty
//! space of the folder. A port of `menuItems` in 0.10's `FilesPane.tsx`, as far as 1.0 goes in
//! this step: no new file, archive, paste, copy to another server or "open terminal here" yet.

use super::kinds::can_unpack;
use super::Entry;
use crate::paths;

/// What a menu item does. The screen runs it on [`targets`] (the row's actions) or on the
/// folder (the background's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Goes into the folder.
    Open,
    /// Opens the file beside the list.
    OpenPreview,
    Download,
    /// The row's name becomes a field.
    Rename,
    Unpack,
    /// The row's full path to the clipboard.
    CopyPath,
    /// Pins the clicked folder, or takes its pin away.
    ToggleBookmark,
    /// Asks first ([`super::ops::delete_question`]).
    Delete,
    /// A new row with a name field.
    NewFolder,
    /// The system's file picker, then an upload into this folder.
    Upload,
    /// Pins the folder on screen, or takes its pin away.
    BookmarkHere,
    CopyFolderPath,
    ToggleHidden,
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub action: Action,
    pub label: String,
    /// The key that does the same, shown at the right: "↵", "F2".
    pub hint: Option<&'static str>,
    /// Drawn in the warning colour: it destroys something.
    pub destructive: bool,
    /// A tick in front, on or off; `None` for an item that isn't a switch.
    pub checked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    Item(MenuItem),
    Separator,
}

fn item(action: Action, label: impl Into<String>) -> MenuItem {
    MenuItem { action, label: label.into(), hint: None, destructive: false, checked: None }
}

/// Where the menu opened and what it knows.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The folder on screen ([`super::Browser::dir`]).
    pub dir: &'a str,
    /// The row clicked; `None` for the empty space under the rows.
    pub clicked: Option<&'a Entry>,
    /// The selected rows ([`super::Browser::selected_entries`]), after
    /// [`super::Browser::context_click`].
    pub selected: &'a [&'a Entry],
    /// A server, not this computer: there is something to download from and upload to.
    pub on_server: bool,
    /// This computer, and it runs Windows (it unpacks only .zip and .tar itself).
    pub on_this_windows: bool,
    /// The machine's pinned folders ([`super::bookmarks::pins`]).
    pub pins: &'a [String],
    pub show_hidden: bool,
}

/// The rows an action from a row's menu applies to: all the selected ones when the click
/// landed among several, else the clicked row alone.
pub fn targets<'a>(clicked: &'a Entry, selected: &[&'a Entry]) -> Vec<&'a Entry> {
    if selected.len() > 1 && selected.iter().any(|e| e.name == clicked.name) {
        selected.to_vec()
    } else {
        vec![clicked]
    }
}

/// The menu, top to bottom.
pub fn menu(cx: &Context<'_>) -> Vec<MenuRow> {
    let pinned = |path: &str| cx.pins.iter().any(|p| p == path);
    // `None` is the line between groups.
    let mut items: Vec<Option<MenuItem>> = Vec::new();
    if let Some(entry) = cx.clicked {
        let count = targets(entry, cx.selected).len();
        let many = count > 1;
        if !many {
            let open = if entry.is_dir() { item(Action::Open, "Open") } else { item(Action::OpenPreview, "Open preview") };
            items.push(Some(MenuItem { hint: Some("↵"), ..open }));
        }
        if cx.on_server {
            items.push(Some(item(Action::Download, if many { format!("Download {count} items") } else { "Download".into() })));
        }
        if !many {
            items.push(Some(MenuItem { hint: Some("F2"), ..item(Action::Rename, "Rename") }));
            if !entry.is_dir() && can_unpack(&entry.name, cx.on_this_windows) {
                items.push(Some(item(Action::Unpack, format!("Unpack “{}”", entry.name))));
            }
            items.push(Some(item(Action::CopyPath, "Copy path")));
            if entry.is_dir() {
                items.push(Some(item(Action::ToggleBookmark, if pinned(&paths::join(cx.dir, &entry.name)) { "Remove bookmark" } else { "Add to bookmarks" })));
            }
        }
        items.push(None);
        items.push(Some(MenuItem { destructive: true, ..item(Action::Delete, if many { format!("Delete {count} items") } else { "Delete".into() }) }));
    } else {
        items.push(Some(item(Action::NewFolder, "New folder")));
        if cx.on_server {
            items.push(Some(item(Action::Upload, "Upload files…")));
        }
        items.push(None);
        items.push(Some(item(Action::BookmarkHere, if pinned(cx.dir) { "Remove bookmark" } else { "Bookmark this folder" })));
        items.push(Some(item(Action::CopyFolderPath, "Copy folder path")));
        items.push(Some(MenuItem { checked: Some(cx.show_hidden), ..item(Action::ToggleHidden, "Show hidden files") }));
        items.push(Some(item(Action::Refresh, "Refresh")));
    }
    items.into_iter().map(|i| i.map_or(MenuRow::Separator, MenuRow::Item)).collect()
}

#[cfg(test)]
mod tests {
    use super::super::testing::{dir, file};
    use super::*;

    /// The menu as lines: labels, "---" for a separator, a hint in brackets, "!" when
    /// destructive, a tick or a space for a switch.
    fn lines(cx: &Context<'_>) -> Vec<String> {
        menu(cx)
            .into_iter()
            .map(|row| match row {
                MenuRow::Separator => "---".into(),
                MenuRow::Item(i) => {
                    let tick = match i.checked {
                        Some(true) => "✓ ",
                        Some(false) => "  ",
                        None => "",
                    };
                    format!("{tick}{}{}{}", i.label, i.hint.map(|h| format!(" [{h}]")).unwrap_or_default(), if i.destructive { " !" } else { "" })
                }
            })
            .collect()
    }

    fn base<'a>() -> Context<'a> {
        Context { dir: "/home/deploy", clicked: None, selected: &[], on_server: true, on_this_windows: false, pins: &[], show_hidden: false }
    }

    #[test]
    fn on_a_file() {
        let readme = file("README.md", 1, 1);
        let cx = Context { clicked: Some(&readme), selected: &[&readme], ..base() };
        assert_eq!(lines(&cx), ["Open preview [↵]", "Download", "Rename [F2]", "Copy path", "---", "Delete !"]);
        // This computer has nothing to download from.
        assert_eq!(lines(&Context { on_server: false, ..cx }), ["Open preview [↵]", "Rename [F2]", "Copy path", "---", "Delete !"]);
    }

    #[test]
    fn on_a_folder_with_and_without_a_bookmark() {
        let shop = dir("shop");
        let cx = Context { clicked: Some(&shop), selected: &[&shop], ..base() };
        assert_eq!(lines(&cx), ["Open [↵]", "Download", "Rename [F2]", "Copy path", "Add to bookmarks", "---", "Delete !"]);
        let pins = ["/home/deploy/shop".to_owned()];
        assert_eq!(lines(&Context { pins: &pins, ..cx })[4], "Remove bookmark");
        // A folder named like an archive isn't one.
        let odd = dir("old.zip");
        assert!(!lines(&Context { clicked: Some(&odd), selected: &[&odd], ..base() }).iter().any(|l| l.starts_with("Unpack")));
    }

    #[test]
    fn an_archive_can_be_unpacked_where_the_machine_can() {
        let site = file("site.tar.gz", 1, 1);
        let cx = Context { clicked: Some(&site), selected: &[&site], ..base() };
        assert_eq!(lines(&cx), ["Open preview [↵]", "Download", "Rename [F2]", "Unpack “site.tar.gz”", "Copy path", "---", "Delete !"]);
        let squeezed = file("dump.sql.gz", 1, 1);
        let here = Context { clicked: Some(&squeezed), selected: &[&squeezed], on_server: false, on_this_windows: true, ..base() };
        assert!(!lines(&here).iter().any(|l| l.starts_with("Unpack")));
        assert!(lines(&Context { on_this_windows: false, ..here }).contains(&"Unpack “dump.sql.gz”".to_owned()));
    }

    #[test]
    fn on_one_of_several_selected_rows_it_is_about_all_of_them() {
        let (a, b, c) = (file("a.txt", 1, 1), dir("b"), file("c.txt", 1, 1));
        let selected = [&a, &b];
        let cx = Context { clicked: Some(&a), selected: &selected, ..base() };
        assert_eq!(lines(&cx), ["Download 2 items", "---", "Delete 2 items !"]);
        assert_eq!(lines(&Context { on_server: false, ..cx }), ["---", "Delete 2 items !"]);
        assert_eq!(targets(&a, &selected).len(), 2);
        // A row outside the selection is about itself (the browser selects it first).
        assert_eq!(targets(&c, &selected), [&c]);
        assert_eq!(lines(&Context { clicked: Some(&c), selected: &selected, ..base() })[0], "Open preview [↵]");
    }

    #[test]
    fn on_the_empty_space() {
        assert_eq!(lines(&base()), ["New folder", "Upload files…", "---", "Bookmark this folder", "Copy folder path", "  Show hidden files", "Refresh"]);
        let pins = ["/home/deploy".to_owned()];
        let here = Context { on_server: false, pins: &pins, show_hidden: true, ..base() };
        assert_eq!(lines(&here), ["New folder", "---", "Remove bookmark", "Copy folder path", "✓ Show hidden files", "Refresh"]);
    }
}
