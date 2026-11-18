//! What Files' menus offer: a right click on a row, on several selected rows and on the empty
//! space of the folder, and the toolbar's "+" (new) and "⋯" (more) menus, which hold every
//! button a narrow pane has no room for. A port of `menuItems`, `newItems` and `moreItems` in
//! 0.10's `FilesPane.tsx`.

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
    /// "Copy to another folder or server…": a server and a folder there to pick, then a copy.
    CopyToServer,
    /// The row's name becomes a field.
    Rename,
    Unpack,
    /// A name field for a .tar.gz of the clicked rows (or of the selection, from "+").
    Archive,
    /// The row's full path to the clipboard.
    CopyPath,
    /// Pins the clicked folder, or takes its pin away.
    ToggleBookmark,
    /// Asks first ([`super::ops::delete_question`]).
    Delete,
    /// A new row with a name field.
    NewFile,
    NewFolder,
    /// The system's file picker, then an upload into this folder.
    Upload,
    /// Files copied on this computer, into this folder (asks first).
    PasteFiles,
    /// The terminal you already use on this machine goes into the folder.
    MainTerminalHere,
    /// A new terminal on this machine, in the folder.
    TerminalHere,
    /// Pins the folder on screen, or takes its pin away.
    BookmarkHere,
    CopyFolderPath,
    ToggleHidden,
    Refresh,
    /// The ⋯ menu's Back, Forward and Up one folder.
    Back,
    Forward,
    Up,
    /// "Open in <editor>": the chosen editor (`super::edit::Edits::chosen`).
    OpenInEditor,
    /// "Open in…": the editors found, to pick one ([`editor_menu`]).
    OpenInOther,
    /// One of [`editor_menu`]'s: the editor at this place among the found ones.
    OpenWith(u8),
    /// "Choose an app…": the system's file picker, then that program.
    ChooseApp,
    /// The file is open in an editor elsewhere: that session ends.
    StopEditing,
}

impl Action {
    /// It opens more choices in the menu's place (drawn with a chevron), not an action.
    pub fn opens_more(self) -> bool {
        matches!(self, Action::OpenInOther)
    }
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
    /// Drawn faint, does nothing: Back with nowhere to go back to.
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    Item(MenuItem),
    Separator,
}

fn item(action: Action, label: impl Into<String>) -> MenuItem {
    MenuItem { action, label: label.into(), hint: None, destructive: false, checked: None, disabled: false }
}

/// Where the menu opened and what it knows.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The folder on screen ([`super::Browser::dir`]).
    pub dir: &'a str,
    /// The row clicked; `None` for the empty space under the rows and the toolbar's menus.
    pub clicked: Option<&'a Entry>,
    /// The selected rows ([`super::Browser::selected_entries`]), after
    /// [`super::Browser::context_click`].
    pub selected: &'a [&'a Entry],
    /// How many rows are visible: "Archive everything here" needs one.
    pub visible: usize,
    /// A server, not this computer: there is something to download from and upload to.
    pub on_server: bool,
    /// This computer, and it runs Windows (it unpacks only .zip and .tar itself).
    pub on_this_windows: bool,
    /// There's a server to copy to besides this machine.
    pub can_copy_out: bool,
    /// The machine's pinned folders ([`super::bookmarks::pins`]).
    pub pins: &'a [String],
    pub show_hidden: bool,
    /// The name of the editor "Open in" means; `None` when none was found.
    pub editor: Option<&'a str>,
    /// The clicked file is open in an editor elsewhere.
    pub editing: bool,
    /// For the ⋯ menu: Back and Forward have somewhere to go.
    pub can_back: bool,
    pub can_forward: bool,
    /// The paste shortcut as this system writes it: "Ctrl+V", "⌘V".
    pub paste_key: &'static str,
    /// The key that deletes the selected rows as this system writes it: "Del", "⌘⌫"
    /// ([`super::browser::key_does`]).
    pub delete_key: &'static str,
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

/// "Open in…": every editor found, by name, and "Choose an app…" after a line.
pub fn editor_menu(found: &[crate::editors::Editor]) -> Vec<MenuRow> {
    let mut rows: Vec<MenuRow> = found.iter().take(u8::MAX as usize).enumerate().map(|(i, e)| MenuRow::Item(item(Action::OpenWith(i as u8), super::edit::listed(&e.name)))).collect();
    if !rows.is_empty() {
        rows.push(MenuRow::Separator);
    }
    rows.push(MenuRow::Item(item(Action::ChooseApp, "Choose an app…")));
    rows
}

/// `None` is the line between groups.
fn rows(items: Vec<Option<MenuItem>>) -> Vec<MenuRow> {
    items.into_iter().map(|i| i.map_or(MenuRow::Separator, MenuRow::Item)).collect()
}

fn copy_out(count: usize, on_server: bool) -> MenuItem {
    let many = if count > 1 { format!("{count} items ") } else { String::new() };
    item(Action::CopyToServer, format!("Copy {many}to {}…", if on_server { "another folder or server" } else { "a server" }))
}

fn terminals() -> [Option<MenuItem>; 2] {
    [Some(item(Action::MainTerminalHere, "Open in main terminal")), Some(item(Action::TerminalHere, "Open new terminal here"))]
}

fn paste(cx: &Context<'_>) -> MenuItem {
    MenuItem { hint: Some(cx.paste_key), ..item(Action::PasteFiles, "Paste files") }
}

fn hidden_switch(cx: &Context<'_>) -> MenuItem {
    MenuItem { checked: Some(cx.show_hidden), ..item(Action::ToggleHidden, "Show hidden files") }
}

fn bookmark_here(cx: &Context<'_>) -> MenuItem {
    item(Action::BookmarkHere, if cx.pins.iter().any(|p| p == cx.dir) { "Remove bookmark" } else { "Bookmark this folder" })
}

/// The menu of a right click, top to bottom.
pub fn menu(cx: &Context<'_>) -> Vec<MenuRow> {
    let pinned = |path: &str| cx.pins.iter().any(|p| p == path);
    let mut items: Vec<Option<MenuItem>> = Vec::new();
    let Some(entry) = cx.clicked else {
        items.push(Some(item(Action::NewFile, "New file")));
        items.push(Some(item(Action::NewFolder, "New folder")));
        if cx.on_server {
            items.push(Some(item(Action::Upload, "Upload files…")));
        }
        items.push(Some(paste(cx)));
        items.extend(terminals());
        items.push(None);
        items.push(Some(bookmark_here(cx)));
        items.push(Some(item(Action::CopyFolderPath, "Copy folder path")));
        items.push(Some(hidden_switch(cx)));
        items.push(Some(item(Action::Refresh, "Refresh")));
        return rows(items);
    };
    let count = targets(entry, cx.selected).len();
    let many = count > 1;
    if !many {
        let open = if entry.is_dir() { item(Action::Open, "Open") } else { item(Action::OpenPreview, "Open preview") };
        items.push(Some(MenuItem { hint: Some("↵"), ..open }));
        if !entry.is_dir() {
            if let Some(name) = cx.editor {
                items.push(Some(item(Action::OpenInEditor, format!("Open in {name}"))));
            }
            items.push(Some(item(Action::OpenInOther, "Open in…")));
            if cx.editing {
                items.push(Some(item(Action::StopEditing, "Stop editing")));
            }
            // Opening stands apart from what changes the file.
            items.push(None);
        }
    }
    if cx.on_server {
        items.push(Some(item(Action::Download, if many { format!("Download {count} items") } else { "Download".into() })));
    }
    if cx.can_copy_out {
        items.push(Some(copy_out(count, cx.on_server)));
    }
    if many {
        items.push(Some(item(Action::Archive, format!("Archive {count} items"))));
    } else {
        items.push(Some(MenuItem { hint: Some("F2"), ..item(Action::Rename, "Rename") }));
        // An archive is unpacked, not packed again.
        if !entry.is_dir() && can_unpack(&entry.name, cx.on_this_windows) {
            items.push(Some(item(Action::Unpack, format!("Unpack “{}”", entry.name))));
        } else {
            items.push(Some(item(Action::Archive, format!("Archive “{}”", entry.name))));
        }
        items.push(Some(item(Action::CopyPath, "Copy path")));
        if entry.is_dir() {
            items.push(Some(item(Action::ToggleBookmark, if pinned(&paths::join(cx.dir, &entry.name)) { "Remove bookmark" } else { "Add to bookmarks" })));
            items.extend(terminals());
        }
    }
    items.push(None);
    items.push(Some(MenuItem { destructive: true, hint: Some(cx.delete_key), ..item(Action::Delete, if many { format!("Delete {count} items") } else { "Delete".into() }) }));
    rows(items)
}

/// The toolbar's "+": a new file or folder, and an archive of what's selected (of everything
/// in the folder when nothing is).
pub fn new_menu(cx: &Context<'_>) -> Vec<MenuRow> {
    let n = cx.selected.len();
    let archive = match cx.selected {
        [one] => item(Action::Archive, format!("Archive “{}”", one.name)),
        [] => MenuItem { disabled: cx.visible == 0, ..item(Action::Archive, "Archive everything here") },
        _ => item(Action::Archive, format!("Archive {n} items")),
    };
    rows(vec![Some(item(Action::NewFile, "New file")), Some(item(Action::NewFolder, "New folder")), None, Some(archive)])
}

/// The toolbar's "⋯": everything the toolbar holds, for a pane too narrow to show it all.
pub fn more_menu(cx: &Context<'_>) -> Vec<MenuRow> {
    let mut items = vec![
        Some(MenuItem { disabled: !cx.can_back, ..item(Action::Back, "Back") }),
        Some(MenuItem { disabled: !cx.can_forward, ..item(Action::Forward, "Forward") }),
        Some(MenuItem { disabled: !paths::has_parent(cx.dir), ..item(Action::Up, "Up one folder") }),
        None,
        Some(hidden_switch(cx)),
        None,
        Some(bookmark_here(cx)),
    ];
    if cx.on_server {
        items.push(Some(item(Action::Upload, "Upload files…")));
    }
    items.push(Some(paste(cx)));
    items.push(Some(item(Action::Refresh, "Refresh")));
    rows(items)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{dir, file};
    use super::*;

    /// The menu as lines: labels, "---" for a separator, a hint in brackets, "!" when
    /// destructive, "~" when disabled, a tick or a space for a switch.
    fn lines(menu: Vec<MenuRow>) -> Vec<String> {
        menu.into_iter()
            .map(|row| match row {
                MenuRow::Separator => "---".into(),
                MenuRow::Item(i) => {
                    let tick = match i.checked {
                        Some(true) => "✓ ",
                        Some(false) => "  ",
                        None => "",
                    };
                    let hint = i.hint.map(|h| format!(" [{h}]")).unwrap_or_default();
                    let more = if i.action.opens_more() { " >" } else { "" };
                    format!("{tick}{}{more}{hint}{}{}", i.label, if i.destructive { " !" } else { "" }, if i.disabled { " ~" } else { "" })
                }
            })
            .collect()
    }

    fn base<'a>() -> Context<'a> {
        Context {
            dir: "/home/deploy",
            clicked: None,
            selected: &[],
            visible: 3,
            on_server: true,
            on_this_windows: false,
            can_copy_out: false,
            pins: &[],
            show_hidden: false,
            editor: None,
            editing: false,
            can_back: false,
            can_forward: false,
            paste_key: "Ctrl+V",
            delete_key: "Del",
        }
    }

    #[test]
    fn on_a_file() {
        let readme = file("README.md", 1, 1);
        let cx = Context { clicked: Some(&readme), selected: &[&readme], ..base() };
        assert_eq!(lines(menu(&cx)), ["Open preview [↵]", "Open in… >", "---", "Download", "Rename [F2]", "Archive “README.md”", "Copy path", "---", "Delete [Del] !"]);
        // This computer has nothing to download from; another server to copy to is offered.
        assert_eq!(
            lines(menu(&Context { on_server: false, can_copy_out: true, ..cx })),
            ["Open preview [↵]", "Open in… >", "---", "Copy to a server…", "Rename [F2]", "Archive “README.md”", "Copy path", "---", "Delete [Del] !"]
        );
        assert_eq!(lines(menu(&Context { can_copy_out: true, ..cx }))[4], "Copy to another folder or server…");
    }

    #[test]
    fn a_file_opens_in_an_editor_elsewhere() {
        let env = file(".env", 1, 1);
        let cx = Context { clicked: Some(&env), selected: &[&env], editor: Some("VS Code"), ..base() };
        assert_eq!(lines(menu(&cx))[..3], ["Open preview [↵]", "Open in VS Code", "Open in… >"]);
        assert_eq!(lines(menu(&Context { editing: true, ..cx }))[3], "Stop editing");
        // Never for a folder, nor for several files at once.
        let shop = dir("shop");
        assert!(!lines(menu(&Context { clicked: Some(&shop), selected: &[&shop], ..cx })).iter().any(|l| l.starts_with("Open in VS") || l == "Open in… >"));
        let other = file("b", 1, 1);
        assert!(!lines(menu(&Context { selected: &[&env, &other], ..cx })).iter().any(|l| l.starts_with("Open in")));
        // "Open in…": every editor found, then the picker.
        let found = [crate::editors::Editor { id: "zed".into(), name: "Zed".into(), program: "/usr/bin/zed".into(), args: vec![], vscode_like: false }];
        let rows = editor_menu(&found);
        assert!(matches!(&rows[..], [MenuRow::Item(a), MenuRow::Separator, MenuRow::Item(b)] if a.label == "Zed" && a.action == Action::OpenWith(0) && b.action == Action::ChooseApp));
        assert!(matches!(&editor_menu(&[])[..], [MenuRow::Item(b)] if b.label == "Choose an app…"));
    }

    #[test]
    fn on_a_folder_with_and_without_a_bookmark() {
        let shop = dir("shop");
        let cx = Context { clicked: Some(&shop), selected: &[&shop], ..base() };
        assert_eq!(
            lines(menu(&cx)),
            ["Open [↵]", "Download", "Rename [F2]", "Archive “shop”", "Copy path", "Add to bookmarks", "Open in main terminal", "Open new terminal here", "---", "Delete [Del] !"]
        );
        let pins = ["/home/deploy/shop".to_owned()];
        assert_eq!(lines(menu(&Context { pins: &pins, ..cx }))[5], "Remove bookmark");
        // A folder named like an archive isn't one.
        let odd = dir("old.zip");
        assert!(!lines(menu(&Context { clicked: Some(&odd), selected: &[&odd], ..base() })).iter().any(|l| l.starts_with("Unpack")));
    }

    #[test]
    fn an_archive_is_unpacked_where_the_machine_can_and_not_packed_again() {
        let site = file("site.tar.gz", 1, 1);
        let cx = Context { clicked: Some(&site), selected: &[&site], ..base() };
        assert_eq!(lines(menu(&cx)), ["Open preview [↵]", "Open in… >", "---", "Download", "Rename [F2]", "Unpack “site.tar.gz”", "Copy path", "---", "Delete [Del] !"]);
        let squeezed = file("dump.sql.gz", 1, 1);
        let here = Context { clicked: Some(&squeezed), selected: &[&squeezed], on_server: false, on_this_windows: true, ..base() };
        assert!(!lines(menu(&here)).iter().any(|l| l.starts_with("Unpack")));
        assert!(lines(menu(&here)).contains(&"Archive “dump.sql.gz”".to_owned()));
        assert!(lines(menu(&Context { on_this_windows: false, ..here })).contains(&"Unpack “dump.sql.gz”".to_owned()));
    }

    #[test]
    fn on_one_of_several_selected_rows_it_is_about_all_of_them() {
        let (a, b, c) = (file("a.txt", 1, 1), dir("b"), file("c.txt", 1, 1));
        let selected = [&a, &b];
        let cx = Context { clicked: Some(&a), selected: &selected, can_copy_out: true, ..base() };
        assert_eq!(lines(menu(&cx)), ["Download 2 items", "Copy 2 items to another folder or server…", "Archive 2 items", "---", "Delete 2 items [Del] !"]);
        assert_eq!(lines(menu(&Context { on_server: false, can_copy_out: false, ..cx })), ["Archive 2 items", "---", "Delete 2 items [Del] !"]);
        assert_eq!(targets(&a, &selected).len(), 2);
        // A row outside the selection is about itself (the browser selects it first).
        assert_eq!(targets(&c, &selected), [&c]);
        assert_eq!(lines(menu(&Context { clicked: Some(&c), selected: &selected, ..base() }))[0], "Open preview [↵]");
    }

    #[test]
    fn on_the_empty_space() {
        assert_eq!(
            lines(menu(&base())),
            [
                "New file",
                "New folder",
                "Upload files…",
                "Paste files [Ctrl+V]",
                "Open in main terminal",
                "Open new terminal here",
                "---",
                "Bookmark this folder",
                "Copy folder path",
                "  Show hidden files",
                "Refresh"
            ]
        );
        let pins = ["/home/deploy".to_owned()];
        let here = Context { on_server: false, pins: &pins, show_hidden: true, paste_key: "⌘V", ..base() };
        let here = lines(menu(&here));
        assert_eq!(here[..3], ["New file", "New folder", "Paste files [⌘V]"]);
        assert_eq!(here[6..], ["Remove bookmark", "Copy folder path", "✓ Show hidden files", "Refresh"]);
    }

    #[test]
    fn the_new_menu_archives_the_selection_or_everything() {
        assert_eq!(lines(new_menu(&base())), ["New file", "New folder", "---", "Archive everything here"]);
        assert_eq!(lines(new_menu(&Context { visible: 0, ..base() }))[3], "Archive everything here ~");
        let (a, b) = (file("a.txt", 1, 1), dir("b"));
        assert_eq!(lines(new_menu(&Context { selected: &[&a], ..base() }))[3], "Archive “a.txt”");
        assert_eq!(lines(new_menu(&Context { selected: &[&a, &b], ..base() }))[3], "Archive 2 items");
    }

    #[test]
    fn the_more_menu_holds_every_button() {
        assert_eq!(
            lines(more_menu(&base())),
            ["Back ~", "Forward ~", "Up one folder", "---", "  Show hidden files", "---", "Bookmark this folder", "Upload files…", "Paste files [Ctrl+V]", "Refresh"]
        );
        let top = Context { dir: "/", can_back: true, on_server: false, ..base() };
        assert_eq!(lines(more_menu(&top))[..3], ["Back", "Forward ~", "Up one folder ~"]);
        assert!(!lines(more_menu(&top)).contains(&"Upload files…".to_owned()));
    }
}
