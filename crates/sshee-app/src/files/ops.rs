//! Changing what's in a folder: the questions asked first, what may be dropped where, and the
//! words when it's done or didn't work. Ports of `remove`, `moveInto`, `finishRename`,
//! `finishCreate`, `unpack` and `download` in 0.10's `FilesPane.tsx`, and of `sending` and
//! `uploadTo` in src/lib/upload.ts. The work itself is the backend's ([`super::Want`]).

use super::{Entry, Failure, FailureKind, Listing, Notice, Tone};
use crate::paths;
use crate::transfers::{cleaning_note, Kind, Transfer};

/// A question asked before something that can't be taken back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    /// What it's about, one per line, under the body. Empty when the title says it all.
    pub lines: Vec<String>,
    /// The button that goes ahead: "Delete", "Discard", "Replace".
    pub confirm_label: &'static str,
    /// That button is drawn as dangerous.
    pub danger: bool,
}

/// The most names listed in a question; the rest are counted.
pub const NAMED: usize = 6;

/// "“notes.md”" or "3 items".
fn what(names: &[&str]) -> String {
    match names {
        [one] => format!("“{one}”"),
        _ => format!("{} items", names.len()),
    }
}

/// Before deleting: what goes, by name (the first six, then "and N more"), and that it can't
/// be undone. On a server it adds that it happens there, at once: there's no bin.
pub fn delete_question(entries: &[&Entry], on_server: bool) -> Confirm {
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut lines: Vec<String> =
        entries.iter().take(NAMED).map(|e| if e.is_dir() { format!("{} (folder, with all that's in it)", e.name) } else { e.name.clone() }).collect();
    if entries.len() > NAMED {
        lines.push(format!("and {} more", entries.len() - NAMED));
    }
    Confirm {
        title: format!("Delete {}?", what(&names)),
        body: if on_server { "It's deleted on the server right away. This can't be undone." } else { "This can't be undone." }.into(),
        lines,
        confirm_label: "Delete",
        danger: true,
    }
}

/// Before leaving a file with changes that aren't saved (another file, another folder, closing).
pub fn discard_question(name: &str) -> Confirm {
    Confirm { title: format!("Discard your changes to {name}?"), body: String::new(), lines: Vec::new(), confirm_label: "Discard", danger: true }
}

/// An upload found names that are taken: nothing was sent. `message` is the backend's, naming
/// them. Yes sends the same again, replacing.
pub fn replace_question(message: &str, kind: Kind) -> Confirm {
    let verb = if kind == Kind::Copy { "copied" } else { "uploaded" };
    Confirm { title: "Replace existing files?".into(), body: format!("{message} If you cancel, nothing is {verb}."), lines: Vec::new(), confirm_label: "Replace", danger: false }
}

/// The status line while something runs: "Deleting “notes.md”…", "Moving 3 items…".
pub fn busy(verb: &str, names: &[&str]) -> String {
    match (verb, names) {
        // 0.10 names what moves and what's unpacked without quotes.
        ("Moving" | "Unpacking", [one]) => format!("{verb} {one}…"),
        _ => format!("{verb} {}…", what(names)),
    }
}

/// Whether `name` can be given to a new folder or a renamed entry in `listing`: not empty, one
/// name and not a path, and not taken. The error is the note to show.
pub fn check_name(name: &str, listing: Option<&Listing>) -> Result<(), Notice> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(Notice::new(Tone::Error, "That can't be a name").body("A name can't be empty, “.” or “..”, or have “/” in it."));
    }
    if listing.is_some_and(|l| l.entries.iter().any(|e| e.name == name)) {
        return Err(Notice::new(Tone::Error, format!("“{name}” already exists here")).body("Pick another name."));
    }
    Ok(())
}

// ---- moving by dragging

/// What dragged rows are over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropOn<'a> {
    /// A row of the list.
    Row(&'a Entry),
    /// The ".." row: the folder above.
    UpRow,
}

/// The folder that rows dragged from `dir` would move into if let go here, or `None` when
/// they can't be dropped: on a file, on one of themselves (a folder can't go into itself),
/// or on ".." at the top of the disk.
pub fn drop_folder(dir: &str, dragged: &[String], on: DropOn<'_>) -> Option<String> {
    if dragged.is_empty() {
        return None;
    }
    match on {
        DropOn::Row(entry) if entry.is_dir() && !dragged.contains(&entry.name) => Some(paths::join(dir, &entry.name)),
        DropOn::Row(_) => None,
        DropOn::UpRow => paths::has_parent(dir).then(|| paths::parent(dir)),
    }
}

/// "Undo" on the note after a move: moves `names` from `from` back into `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undo {
    /// Where they are now.
    pub from: String,
    /// Where they were.
    pub to: String,
    pub names: Vec<String>,
}

/// The notes after moving `moved` from `from` into `to`: what went (with Undo), what stayed
/// because its name is taken there, and what stopped the rest.
pub fn move_notices(from: &str, to: &str, moved: &[String], taken: &[String], failure: Option<&Failure>) -> Vec<Notice> {
    let mut notices = Vec::new();
    if let Some(failure) = failure {
        notices.push(Notice::failed("Couldn't move", failure));
    }
    let folder = paths::name(to);
    let place = if folder.is_empty() { to } else { folder };
    if !moved.is_empty() {
        let names: Vec<&str> = moved.iter().map(String::as_str).collect();
        let what = if let [one] = names[..] { one.to_owned() } else { format!("{} items", names.len()) };
        let mut notice = Notice::new(if taken.is_empty() { Tone::Success } else { Tone::Info }, format!("Moved {what} to {place}"));
        if !taken.is_empty() {
            notice = notice.body(format!("{} stayed: {} taken there.", taken.join(", "), if taken.len() == 1 { "that name is" } else { "those names are" }));
        }
        notice.undo = Some(Undo { from: to.to_owned(), to: from.to_owned(), names: moved.to_vec() });
        notices.push(notice);
    } else if !taken.is_empty() {
        notices.push(Notice::new(Tone::Error, "Nothing moved").body(format!("{place} already has {}.", taken.join(", "))));
    }
    notices
}

// ---- the words when something is done or didn't work

pub fn made_dir_failed(failure: &Failure) -> Notice {
    Notice::failed("Couldn't create the folder", failure)
}

pub fn rename_failed(failure: &Failure) -> Notice {
    Notice::failed("Couldn't rename", failure)
}

pub fn delete_failed(name: &str, failure: &Failure) -> Notice {
    Notice::failed(format!("Couldn't delete {name}"), failure)
}

/// `made`: the folder (or file) that came out, `Err` when nothing did.
pub fn unpacked(name: &str, made: &Result<String, Failure>) -> Notice {
    match made {
        Ok(made) => Notice::new(Tone::Success, format!("Unpacked {name}")).body(format!("Into “{made}”")),
        Err(failure) => Notice::failed(format!("Couldn't unpack {name}"), failure),
    }
}

/// A download ended. `saved`: where each item landed on this computer.
pub fn downloaded(label: &str, saved: &Result<Vec<String>, Failure>) -> Notice {
    match saved {
        Ok(at) => {
            let notice = Notice::new(Tone::Success, format!("Downloaded {label}"));
            match at.as_slice() {
                [] => notice,
                [one] => notice.body(one.clone()),
                // Several: the folder they're in.
                [first, ..] => notice.body(paths::parent(&first.replace('\\', "/"))),
            }
        }
        Err(f) if f.kind == FailureKind::Cancelled => Notice::new(Tone::Info, "Download stopped").body(label),
        Err(f) => Notice::failed(format!("Couldn't download {label}"), f),
    }
}

/// An upload (or a copy on this computer) into `dir` ended. `card` is its card as it stood,
/// from [`crate::transfers::Transfers::end`]. Not for [`FailureKind::Exists`] on a first try:
/// that's [`replace_question`].
pub fn uploaded(card: &Transfer, dir: &str, result: &Result<(), Failure>) -> Notice {
    let copy = card.kind == Kind::Copy;
    match result {
        Ok(()) => {
            // What came out of the files first, when anything did.
            let body = match card.cleaning.as_ref().filter(|c| !c.removed.is_empty()) {
                Some(cleaning) => format!("{} · {dir}", cleaning_note(cleaning)),
                None => dir.to_owned(),
            };
            Notice::new(Tone::Success, format!("{} {}", if copy { "Copied" } else { "Uploaded" }, card.label)).body(body)
        }
        Err(f) if f.kind == FailureKind::Cancelled => Notice::new(Tone::Info, if copy { "Copy stopped" } else { "Upload stopped" }).body(card.label.clone()),
        Err(f) => Notice::failed(if copy { "Copy failed" } else { "Upload failed" }, f),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{dir, file, listing};
    use super::*;
    use crate::transfers::{Cleaning, Progress, Transfers};
    use crate::workbench::TabId;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn deleting_asks_with_the_names() {
        let (readme, shop) = (file("README.md", 1, 1), dir("shop"));
        let one = delete_question(&[&readme], true);
        assert_eq!(one.title, "Delete “README.md”?");
        assert_eq!(one.body, "It's deleted on the server right away. This can't be undone.");
        assert_eq!(one.lines, ["README.md"]);
        assert_eq!((one.confirm_label, one.danger), ("Delete", true));
        let here = delete_question(&[&shop, &readme], false);
        assert_eq!((here.title.as_str(), here.body.as_str()), ("Delete 2 items?", "This can't be undone."));
        assert_eq!(here.lines, ["shop (folder, with all that's in it)", "README.md"]);
    }

    #[test]
    fn a_long_list_names_six_and_counts_the_rest() {
        let files: Vec<Entry> = (1..=9).map(|n| file(&format!("f{n}"), 1, 1)).collect();
        let refs: Vec<&Entry> = files.iter().collect();
        let q = delete_question(&refs, true);
        assert_eq!(q.title, "Delete 9 items?");
        assert_eq!(q.lines, ["f1", "f2", "f3", "f4", "f5", "f6", "and 3 more"]);
        assert_eq!(delete_question(&refs[..6], true).lines.len(), 6);
    }

    #[test]
    fn other_questions() {
        assert_eq!(discard_question("notes.md").title, "Discard your changes to notes.md?");
        assert_eq!(discard_question("notes.md").confirm_label, "Discard");
        let q = replace_question("notes.md is already there.", Kind::Upload);
        assert_eq!((q.title.as_str(), q.body.as_str(), q.confirm_label, q.danger), ("Replace existing files?", "notes.md is already there. If you cancel, nothing is uploaded.", "Replace", false));
        assert!(replace_question("x", Kind::Copy).body.ends_with("nothing is copied."));
    }

    #[test]
    fn what_runs_says_so_in_the_status_line() {
        assert_eq!(busy("Deleting", &["notes.md"]), "Deleting “notes.md”…");
        assert_eq!(busy("Deleting", &["a", "b", "c"]), "Deleting 3 items…");
        assert_eq!(busy("Moving", &["notes.md"]), "Moving notes.md…");
        assert_eq!(busy("Moving", &["a", "b"]), "Moving 2 items…");
        assert_eq!(busy("Unpacking", &["site.zip"]), "Unpacking site.zip…");
    }

    #[test]
    fn a_new_name_must_be_one_name_and_free() {
        let home = listing("/home/deploy", vec![dir("shop"), file("README.md", 1, 1)]);
        assert_eq!(check_name("notes", Some(&home)), Ok(()));
        assert_eq!(check_name("notes", None), Ok(()));
        let taken = check_name("shop", Some(&home)).unwrap_err();
        assert_eq!((taken.title.as_str(), taken.body.as_deref()), ("“shop” already exists here", Some("Pick another name.")));
        // Names differ by case on a server.
        assert_eq!(check_name("Shop", Some(&home)), Ok(()));
        for bad in ["", ".", "..", "a/b", "/etc"] {
            assert_eq!(check_name(bad, Some(&home)).unwrap_err().title, "That can't be a name", "{bad:?}");
        }
    }

    #[test]
    fn rows_drop_on_folders_but_not_on_themselves() {
        let (shop, logs, readme) = (dir("shop"), dir("logs"), file("README.md", 1, 1));
        let dragged = names(&["README.md", "logs"]);
        assert_eq!(drop_folder("/home/deploy", &dragged, DropOn::Row(&shop)), Some("/home/deploy/shop".into()));
        // A folder into itself, onto a file, onto one of the dragged.
        assert_eq!(drop_folder("/home/deploy", &dragged, DropOn::Row(&logs)), None);
        assert_eq!(drop_folder("/home/deploy", &dragged, DropOn::Row(&readme)), None);
        assert_eq!(drop_folder("/home/deploy", &names(&["shop"]), DropOn::Row(&shop)), None);
        // ".." is the folder above, where there is one.
        assert_eq!(drop_folder("/home/deploy", &dragged, DropOn::UpRow), Some("/home".into()));
        assert_eq!(drop_folder("/home", &dragged, DropOn::UpRow), Some("/".into()));
        assert_eq!(drop_folder("/", &dragged, DropOn::UpRow), None);
        assert_eq!(drop_folder("/home/deploy", &[], DropOn::Row(&shop)), None);
    }

    #[test]
    fn a_move_says_what_went_what_stayed_and_can_be_undone() {
        let (from, to) = ("/home/deploy", "/home/deploy/shop");
        let all = move_notices(from, to, &names(&["a.txt"]), &[], None);
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].tone, all[0].title.as_str(), all[0].body.as_deref()), (Tone::Success, "Moved a.txt to shop", None));
        assert_eq!(all[0].undo, Some(Undo { from: to.into(), to: from.into(), names: names(&["a.txt"]) }));

        let some = move_notices(from, to, &names(&["a.txt", "b.txt"]), &names(&["c.txt"]), None);
        assert_eq!((some[0].tone, some[0].title.as_str()), (Tone::Info, "Moved 2 items to shop"));
        assert_eq!(some[0].body.as_deref(), Some("c.txt stayed: that name is taken there."));
        let two = move_notices(from, to, &names(&["a.txt"]), &names(&["c.txt", "d.txt"]), None);
        assert_eq!(two[0].body.as_deref(), Some("c.txt, d.txt stayed: those names are taken there."));

        let none = move_notices(from, to, &[], &names(&["c.txt", "d.txt"]), None);
        assert_eq!((none[0].tone, none[0].title.as_str(), none[0].body.as_deref()), (Tone::Error, "Nothing moved", Some("shop already has c.txt, d.txt.")));
        assert!(none[0].undo.is_none());
        // Into the top of the disk.
        assert_eq!(move_notices("/srv", "/", &names(&["a"]), &[], None)[0].title, "Moved a to /");
        // Something broke halfway: both are said.
        let broke = move_notices(from, to, &names(&["a.txt"]), &[], Some(&Failure::new(FailureKind::Network, "Can't reach the server.")));
        assert_eq!(broke.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Couldn't move", "Moved a.txt to shop"]);
        assert!(move_notices(from, to, &[], &[], None).is_empty());
    }

    #[test]
    fn notes_for_what_finished_or_failed() {
        let denied = Failure::new(FailureKind::Failed, "Permission denied");
        assert_eq!(made_dir_failed(&denied), Notice::new(Tone::Error, "Couldn't create the folder").body("Permission denied"));
        assert_eq!(rename_failed(&denied).title, "Couldn't rename");
        assert_eq!(delete_failed("notes.md", &denied).title, "Couldn't delete notes.md");
        assert_eq!(unpacked("site.zip", &Ok("site".into())), Notice::new(Tone::Success, "Unpacked site.zip").body("Into “site”"));
        assert_eq!(unpacked("site.zip", &Err(denied.clone())).title, "Couldn't unpack site.zip");

        assert_eq!(downloaded("notes.md", &Ok(vec!["/home/dev/Downloads/notes.md".into()])), Notice::new(Tone::Success, "Downloaded notes.md").body("/home/dev/Downloads/notes.md"));
        let two = downloaded("2 items", &Ok(vec!["C:\\Users\\dev\\Downloads\\a".into(), "C:\\Users\\dev\\Downloads\\b".into()]));
        assert_eq!(two.body.as_deref(), Some("C:/Users/dev/Downloads"));
        let stopped = Failure::new(FailureKind::Cancelled, "Cancelled.");
        assert_eq!(downloaded("notes.md", &Err(stopped.clone())), Notice::new(Tone::Info, "Download stopped").body("notes.md"));
        assert_eq!(downloaded("notes.md", &Err(denied.clone())).title, "Couldn't download notes.md");
    }

    #[test]
    fn an_upload_says_where_it_went_and_what_was_taken_out() {
        let mut list = Transfers::new();
        let id = list.begin(Kind::Upload, "photo.jpg", TabId(1), "host:web");
        let plain = list.get(id).unwrap().clone();
        assert_eq!(uploaded(&plain, "/srv/www", &Ok(())), Notice::new(Tone::Success, "Uploaded photo.jpg").body("/srv/www"));
        let cleaning = Cleaning { cleaned: 1, removed: vec!["location".into(), "camera".into()], ..Cleaning::default() };
        list.progress(id, &Progress { cleaning: Some(cleaning), ..Progress::default() });
        let cleaned = list.end(id).unwrap();
        assert_eq!(uploaded(&cleaned, "/srv/www", &Ok(())).body.as_deref(), Some("Removed location, camera · /srv/www"));
        let stopped = Failure::new(FailureKind::Cancelled, "Cancelled.");
        assert_eq!(uploaded(&cleaned, "/srv/www", &Err(stopped.clone())), Notice::new(Tone::Info, "Upload stopped").body("photo.jpg"));
        let denied = Failure::new(FailureKind::Failed, "Permission denied");
        assert_eq!(uploaded(&cleaned, "/srv/www", &Err(denied.clone())), Notice::new(Tone::Error, "Upload failed").body("Permission denied"));
        // On this computer it's a copy.
        let id = list.begin(Kind::Copy, "2 items", TabId(1), "host:web");
        let copy = list.end(id).unwrap();
        assert_eq!(uploaded(&copy, "/tmp/x", &Ok(())).title, "Copied 2 items");
        assert_eq!(uploaded(&copy, "/tmp/x", &Err(stopped)).title, "Copy stopped");
        assert_eq!(uploaded(&copy, "/tmp/x", &Err(denied)).title, "Copy failed");
    }
}
