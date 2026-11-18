//! Changing what's in a folder: the questions asked first, what may be dropped where, and the
//! words when it's done or didn't work. Ports of `remove`, `moveInto`, `finishRename`,
//! `finishCreate`, `unpack` and `download` in 0.10's `FilesPane.tsx`, and of `sending` and
//! `uploadTo` in src/lib/upload.ts. The work itself is the backend's ([`super::Want`]).

use super::{Entry, Failure, FailureKind, Listing, Notice, Tone};
use crate::paths;
use crate::transfers::{cleaning_note, Kind, Transfer};
use crate::workbench::{ServerKey, TabId};

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

/// Whether rows of the folder `dir` (their full paths, `dragged`) may move into `folder` on
/// the same machine: not into the folder they're in, and a folder never into itself or into a
/// folder of its own (0.10's `dropFor`). For drops that aren't on a row of the same list: a
/// folder of the path, another Files tab's folder or a row there.
pub fn may_move_into(dir: &str, dragged: &[String], folder: &str) -> bool {
    let trimmed = |p: &str| if p.len() > 1 { p.trim_end_matches('/').to_owned() } else { p.to_owned() };
    let folder = trimmed(folder);
    if dragged.is_empty() || folder == trimmed(dir) {
        return false;
    }
    !dragged.iter().map(|p| trimmed(p)).any(|p| folder == p || folder.starts_with(&format!("{}/", p.trim_end_matches('/'))))
}

/// Why "Copy to another folder or server" can't copy rows of `from_dir` (their full paths,
/// `paths`) into `dest` on the same server: they're in it already, or a folder would go into
/// itself (0.10's `CopyToServer`). `None`: it can. Another server takes anything.
pub fn copy_blocked(same_server: bool, from_dir: &str, paths: &[String], dest: &str) -> Option<String> {
    if !same_server {
        return None;
    }
    let trimmed = |p: &str| if p.len() > 1 { p.trim_end_matches('/').to_owned() } else { p.to_owned() };
    let dest = trimmed(dest);
    if dest == trimmed(from_dir) {
        return Some("They're already in this folder. Pick another one.".into());
    }
    let inside = paths.iter().map(|p| trimmed(p)).find(|p| dest == *p || dest.starts_with(&format!("{}/", p.trim_end_matches('/'))))?;
    Some(format!("Can't copy {} into itself. Pick a folder outside it.", paths::name(&inside)))
}

/// Before rows dragged out of the window download (0.10.14): letting go out there is easy to do
/// by accident, so it asks, naming where they'll land. `into` is the folder under the pointer
/// when the system said (the desktop, a folder's window), else they go to Downloads. A folder
/// on this computer may be split by "\" (Windows).
pub fn download_out_question(names: &[&str], server: &str, into: Option<&str>) -> Confirm {
    let place = into.map(|p| {
        let p = p.trim_end_matches(['/', '\\']);
        p.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(p).to_owned()
    });
    let body = match into {
        Some(into) => format!("From {server}, into {into}. Nothing there is replaced."),
        None => format!("From {server}, into your Downloads folder."),
    };
    Confirm { title: format!("Download {} to {}?", what(names), place.as_deref().unwrap_or("Downloads")), body, lines: Vec::new(), confirm_label: "Download", danger: false }
}

/// "Undo" on the note after a move: moves `names` from `from` back into `to`, on the machine
/// the move was made on, whatever tab is on screen when it's pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undo {
    /// The Files tab that moved them: its answer goes there (or only to the folder cache, if
    /// it closed meanwhile).
    pub tab: TabId,
    /// The machine they were moved on.
    pub server: ServerKey,
    /// Where they are now.
    pub from: String,
    /// Where they were.
    pub to: String,
    pub names: Vec<String>,
}

/// The notes after moving `moved` from `from` into `to` on `server`, from the Files tab `tab`:
/// what went (with Undo), what stayed because its name is taken there, and what stopped the
/// rest.
pub fn move_notices(tab: TabId, server: &str, from: &str, to: &str, moved: &[String], taken: &[String], failure: Option<&Failure>) -> Vec<Notice> {
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
        notice.undo = Some(Undo { tab, server: server.to_owned(), from: to.to_owned(), to: from.to_owned(), names: moved.to_vec() });
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
            let mut notice = Notice::new(Tone::Success, format!("Downloaded {label}"));
            // "Show in folder": where they landed.
            notice.folder = at.first().map(|first| std::path::PathBuf::from(paths::parent(&first.replace('\\', "/"))));
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

// ---- new files, archives, copies

/// What the name field starts with for a new file.
pub const NEW_FILE: &str = "untitled.txt";
/// What the name field starts with for a new folder.
pub const NEW_FOLDER: &str = "New folder";

/// The name an archive of `names` in `dir` is offered under: the one item's name, else the
/// folder's ("archive" at the top of the disk), then ".tar.gz".
pub fn archive_name(names: &[String], dir: &str) -> String {
    let stem = match names {
        [one] => one.as_str(),
        _ => match paths::name(dir) {
            "" | "/" | "~" => "archive",
            folder => folder,
        },
    };
    format!("{stem}.tar.gz")
}

/// The name typed for an archive, with ".tar.gz" put on when it has neither that nor ".tgz".
pub fn archive_file(typed: &str) -> String {
    if typed.ends_with(".tar.gz") || typed.ends_with(".tgz") {
        typed.to_owned()
    } else {
        format!("{typed}.tar.gz")
    }
}

/// The status line while an archive is made.
pub fn packing(archive: &str) -> String {
    format!("Packing {archive}…")
}

pub fn made_file_failed(failure: &Failure) -> Notice {
    Notice::failed("Couldn't create the file", failure)
}

/// An archive of `count` items ended.
pub fn archived(archive: &str, count: usize, result: &Result<(), Failure>) -> Notice {
    match result {
        Ok(()) => Notice::new(Tone::Success, format!("Made {archive}")).body(format!("{count} item{} packed", if count == 1 { "" } else { "s" })),
        Err(failure) => Notice::failed("Couldn't create the archive", failure),
    }
}

/// A copy to another server ended. `card` is its card as it stood; `server` the name of the
/// server it went to.
pub fn copied_to(card: &Transfer, server: &str, dir: &str, result: &Result<(), Failure>) -> Notice {
    match result {
        Ok(()) => {
            let body = match card.cleaning.as_ref().filter(|c| !c.removed.is_empty()) {
                Some(cleaning) => format!("{} · {dir}", cleaning_note(cleaning)),
                None => dir.to_owned(),
            };
            Notice::new(Tone::Success, format!("Copied {} to {server}", card.label)).body(body)
        }
        Err(f) if f.kind == FailureKind::Cancelled => Notice::new(Tone::Info, "Copy stopped").body(card.label.clone()),
        Err(f) => Notice::failed("Copy failed", f),
    }
}

/// The title of "Copy to another server": `“notes.md”` or `3 items`, and where from.
pub fn copy_title(names: &[String], from_server: bool) -> String {
    let shown: Vec<&str> = names.iter().map(String::as_str).collect();
    format!("Copy {} to {}", what(&shown), if from_server { "another folder or server" } else { "a server" })
}

// ---- files copied on this computer, pasted into a folder

/// Whether a path of this computer looks private: keys, credentials and where they're kept
/// (`.ssh`, `.env`, `id_ed25519`, `*.pem`...). 0.10's `PRIVATE` in src/lib/upload.ts.
pub fn is_private(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| {
        let p = part.to_ascii_lowercase();
        let named = |stem: &str| p == stem || p.strip_prefix(stem).is_some_and(|rest| rest.starts_with('.'));
        let ends = |e: &str| p.len() > e.len() && p.ends_with(e);
        matches!(p.as_str(), ".ssh" | ".gnupg" | ".aws" | ".azure" | ".kube" | ".docker" | ".password-store" | ".netrc" | ".npmrc" | ".pgpass")
            || named(".env")
            || ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].iter().any(|k| p.starts_with(k))
            || [".pem", ".key", ".p12", ".pfx", ".kdbx"].iter().any(|e| ends(e))
            || p.starts_with("credentials")
            || named("secret")
            || named("secrets")
    })
}

/// What the clipboard's file list really holds of files somebody copied (sshee-core's
/// `copied_files`, for the screen): only full paths; and none when the clipboard's text says
/// the same as a link (`file:///…`) or a path, because a server can put text on the
/// clipboard (OSC 52) and that must never pick a file to send. A file copied in a file
/// manager has its name for text, not its path.
pub fn copied_files(paths: &[std::path::PathBuf], text: &str) -> Vec<std::path::PathBuf> {
    let full: Vec<std::path::PathBuf> = paths.iter().filter(|p| is_full(&p.to_string_lossy())).cloned().collect();
    let text = text.trim();
    let said_as_text = text.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) || full.iter().any(|p| p.to_string_lossy() == text);
    if said_as_text {
        return Vec::new();
    }
    full
}

/// A full path on any of the three systems: `/x`, `C:\x`, `C:/x`, `\\server\share`.
fn is_full(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with('/') || path.starts_with("\\\\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// The question before files copied on this computer go to a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteQuestion {
    pub confirm: Confirm,
    /// Something looks private: Cancel is the button Return presses.
    pub risky: bool,
}

/// Before files on the clipboard go to `server` into `dir`. Picked or dropped files were
/// chosen there and then; these were copied some time ago, maybe by another program, and
/// what's sent can't be taken back: it names them in full (six, then a count), where they
/// go, and what of them looks private. `files`: each path and whether it's a folder.
pub fn paste_question(files: &[(String, bool)], server: &str, dir: &str) -> PasteQuestion {
    let risky: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).filter(|p| is_private(p)).collect();
    let mut lines: Vec<String> = files.iter().take(NAMED).map(|(p, d)| if *d { format!("{p} (folder, with all that's in it)") } else { p.clone() }).collect();
    if files.len() > NAMED {
        lines.push(format!("and {} more", files.len() - NAMED));
    }
    let mut body = format!("Into {dir}");
    if !risky.is_empty() {
        let named = risky.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        body.push_str(&format!("\n\nThis looks private: {named}{}. Once on the server, it can't be taken back.", if risky.len() > 3 { "…" } else { "" }));
    }
    let title = match files.len() {
        1 => format!("Upload this to {server}?"),
        n => format!("Upload these {n} items to {server}?"),
    };
    let confirm = Confirm { title, body, lines, confirm_label: if risky.is_empty() { "Upload" } else { "Upload anyway" }, danger: !risky.is_empty() };
    PasteQuestion { confirm, risky: !risky.is_empty() }
}

/// Ctrl+V with no files on the clipboard.
pub fn nothing_to_paste(key: &str) -> Notice {
    Notice::new(Tone::Info, "No files to paste").body(format!("Copy files on your computer first, then {key} here puts them in this folder."))
}

// ---- a terminal in a folder

/// The shell a command is typed into: sh and its kin on servers, PowerShell on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Posix,
    PowerShell,
}

/// `s` quoted for `shell`, or `None` when it has a control character, which a terminal would
/// take as a key (0.10's `typedQuote`).
pub fn typed_quote(s: &str, shell: Shell) -> Option<String> {
    if s.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(match shell {
        // PowerShell closes a string at any of these, and doubling one escapes it.
        Shell::PowerShell => format!("'{}'", s.chars().flat_map(|c| if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') { vec![c, c] } else { vec![c] }).collect::<String>()),
        Shell::Posix if s.is_empty() => "''".into(),
        Shell::Posix => {
            let mut out = String::new();
            let mut run = String::new();
            for c in s.chars() {
                if c == '\'' || c == '\\' {
                    if !run.is_empty() {
                        out.push_str(&format!("'{run}'"));
                        run.clear();
                    }
                    out.push('\\');
                    out.push(c);
                } else {
                    run.push(c);
                }
            }
            if !run.is_empty() {
                out.push_str(&format!("'{run}'"));
            }
            out
        }
    })
}

/// Typed into a shell: go into `dir`, then run `next` if that worked. `None` when `dir`
/// can't be typed ([`typed_quote`]). Windows PowerShell 5.1 has no `&&`, and -LiteralPath
/// keeps `[` `]` in a path from acting as wildcards (0.10's `typedCd`).
pub fn typed_cd(dir: &str, shell: Shell, next: Option<&str>) -> Option<String> {
    let quoted = typed_quote(dir, shell)?;
    Some(match (shell, next) {
        (Shell::PowerShell, Some(next)) => format!("Set-Location -LiteralPath {quoted}; if ($?) {{ {next} }}"),
        (Shell::PowerShell, None) => format!("Set-Location -LiteralPath {quoted}"),
        (Shell::Posix, Some(next)) => format!("cd -- {quoted} && {next}"),
        (Shell::Posix, None) => format!("cd -- {quoted}"),
    })
}

/// A folder whose name would act as keys in a terminal.
pub fn cant_open_terminal() -> Notice {
    Notice::new(Tone::Error, "Can't open a terminal there").body("This folder's name has control characters in it, which a terminal would take as keystrokes.")
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
    fn rows_move_into_any_folder_but_their_own_and_their_insides() {
        let dragged = names(&["/home/deploy/shop", "/home/deploy/notes.md"]);
        assert!(may_move_into("/home/deploy", &dragged, "/home"));
        assert!(may_move_into("/home/deploy", &dragged, "/"));
        assert!(may_move_into("/home/deploy", &dragged, "/home/deploy/logs"));
        assert!(!may_move_into("/home/deploy", &dragged, "/home/deploy"), "where they are");
        assert!(!may_move_into("/home/deploy", &dragged, "/home/deploy/"), "where they are");
        assert!(!may_move_into("/home/deploy", &dragged, "/home/deploy/shop"), "a folder into itself");
        assert!(!may_move_into("/home/deploy", &dragged, "/home/deploy/shop/assets"), "into its own folder");
        assert!(may_move_into("/home/deploy", &dragged, "/home/deploy/shopping"), "only a whole name is the same folder");
        assert!(!may_move_into("/home/deploy", &[], "/srv"));
    }

    #[test]
    fn a_copy_on_its_own_server_goes_elsewhere_and_never_into_itself() {
        let paths = names(&["/home/deploy/shop", "/home/deploy/notes.md"]);
        assert_eq!(copy_blocked(true, "/home/deploy", &paths, "/home/deploy/").as_deref(), Some("They're already in this folder. Pick another one."));
        assert_eq!(copy_blocked(true, "/home/deploy", &paths, "/home/deploy/shop/assets").as_deref(), Some("Can't copy shop into itself. Pick a folder outside it."));
        assert_eq!(copy_blocked(true, "/home/deploy", &paths, "/home/deploy/logs"), None);
        assert_eq!(copy_blocked(true, "/home/deploy", &paths, "/home/deploy/shopping"), None);
        assert_eq!(copy_blocked(false, "/home/deploy", &paths, "/home/deploy"), None, "another server takes anything");
    }

    #[test]
    fn dragging_out_asks_where_it_lands() {
        let q = download_out_question(&["shop-db.sql.gz"], "web-7731", Some("C:\\Users\\dev\\Desktop"));
        assert_eq!(q.title, "Download “shop-db.sql.gz” to Desktop?");
        assert_eq!(q.body, "From web-7731, into C:\\Users\\dev\\Desktop. Nothing there is replaced.");
        assert_eq!((q.confirm_label, q.danger), ("Download", false));
        let q = download_out_question(&["a", "b"], "web-7731", None);
        assert_eq!((q.title.as_str(), q.body.as_str()), ("Download 2 items to Downloads?", "From web-7731, into your Downloads folder."));
        assert_eq!(download_out_question(&["a"], "web-7731", Some("/home/dev/Desktop/")).title, "Download “a” to Desktop?");
    }

    #[test]
    fn a_move_says_what_went_what_stayed_and_can_be_undone() {
        let (from, to) = ("/home/deploy", "/home/deploy/shop");
        let all = move_notices(TabId(3), "host:web", from, to, &names(&["a.txt"]), &[], None);
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].tone, all[0].title.as_str(), all[0].body.as_deref()), (Tone::Success, "Moved a.txt to shop", None));
        // Undo goes to the tab and machine of the move, not to whatever is on screen then.
        assert_eq!(all[0].undo, Some(Undo { tab: TabId(3), server: "host:web".into(), from: to.into(), to: from.into(), names: names(&["a.txt"]) }));

        let some = move_notices(TabId(3), "host:web", from, to, &names(&["a.txt", "b.txt"]), &names(&["c.txt"]), None);
        assert_eq!((some[0].tone, some[0].title.as_str()), (Tone::Info, "Moved 2 items to shop"));
        assert_eq!(some[0].body.as_deref(), Some("c.txt stayed: that name is taken there."));
        let two = move_notices(TabId(3), "host:web", from, to, &names(&["a.txt"]), &names(&["c.txt", "d.txt"]), None);
        assert_eq!(two[0].body.as_deref(), Some("c.txt, d.txt stayed: those names are taken there."));

        let none = move_notices(TabId(3), "host:web", from, to, &[], &names(&["c.txt", "d.txt"]), None);
        assert_eq!((none[0].tone, none[0].title.as_str(), none[0].body.as_deref()), (Tone::Error, "Nothing moved", Some("shop already has c.txt, d.txt.")));
        assert!(none[0].undo.is_none());
        // Into the top of the disk.
        assert_eq!(move_notices(TabId(3), "host:web", "/srv", "/", &names(&["a"]), &[], None)[0].title, "Moved a to /");
        // Something broke halfway: both are said.
        let broke = move_notices(TabId(3), "host:web", from, to, &names(&["a.txt"]), &[], Some(&Failure::new(FailureKind::Network, "Can't reach the server.")));
        assert_eq!(broke.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Couldn't move", "Moved a.txt to shop"]);
        assert!(move_notices(TabId(3), "host:web", from, to, &[], &[], None).is_empty());
    }

    #[test]
    fn notes_for_what_finished_or_failed() {
        let denied = Failure::new(FailureKind::Failed, "Permission denied");
        assert_eq!(made_dir_failed(&denied), Notice::new(Tone::Error, "Couldn't create the folder").body("Permission denied"));
        assert_eq!(rename_failed(&denied).title, "Couldn't rename");
        assert_eq!(delete_failed("notes.md", &denied).title, "Couldn't delete notes.md");
        assert_eq!(unpacked("site.zip", &Ok("site".into())), Notice::new(Tone::Success, "Unpacked site.zip").body("Into “site”"));
        assert_eq!(unpacked("site.zip", &Err(denied.clone())).title, "Couldn't unpack site.zip");

        let one = downloaded("notes.md", &Ok(vec!["/home/dev/Downloads/notes.md".into()]));
        assert_eq!((one.title.as_str(), one.body.as_deref()), ("Downloaded notes.md", Some("/home/dev/Downloads/notes.md")));
        assert_eq!(one.folder, Some(std::path::PathBuf::from("/home/dev/Downloads")), "Show in folder opens where it landed");
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

    #[test]
    fn archives_are_named_after_what_goes_in() {
        assert_eq!(archive_name(&names(&["site"]), "/srv/www"), "site.tar.gz");
        assert_eq!(archive_name(&names(&["a", "b"]), "/srv/www"), "www.tar.gz");
        assert_eq!(archive_name(&[], "/"), "archive.tar.gz");
        assert_eq!(archive_file("backup"), "backup.tar.gz");
        assert_eq!(archive_file("backup.tgz"), "backup.tgz");
        assert_eq!(archive_file("b.tar.gz"), "b.tar.gz");
        assert_eq!(archived("www.tar.gz", 3, &Ok(())), Notice::new(Tone::Success, "Made www.tar.gz").body("3 items packed"));
        assert_eq!(archived("a.tar.gz", 1, &Ok(())).body.as_deref(), Some("1 item packed"));
        assert_eq!(packing("a.tar.gz"), "Packing a.tar.gz…");
        assert_eq!(copy_title(&names(&["a.txt"]), true), "Copy “a.txt” to another folder or server");
        assert_eq!(copy_title(&names(&["a", "b"]), false), "Copy 2 items to a server");
    }

    #[test]
    fn private_files_are_told_apart() {
        for p in ["/home/dev/.ssh", "/home/dev/.ssh/config", "C:\\Users\\dev\\.env", "/srv/.env.production", "/k/id_ed25519.pub", "/k/server.PEM", "/k/credentials.json", "/k/secrets", "/k/secret.txt", "/home/dev/.aws/config", "/d/vault.kdbx"] {
            assert!(is_private(p), "{p}");
        }
        for p in ["/home/dev/notes.md", "/srv/environment", "/k/keys.txt", "/home/dev/.envrc", "/k/my.key.txt", "/k/.key"] {
            assert!(!is_private(p), "{p}");
        }
    }

    #[test]
    fn only_files_somebody_copied_are_pasted() {
        use std::path::PathBuf;
        let copied = vec![PathBuf::from("/home/dev/a.txt"), PathBuf::from("relative/b.txt"), PathBuf::from("C:\\Users\\dev\\c.txt")];
        assert_eq!(copied_files(&copied, "a.txt"), [PathBuf::from("/home/dev/a.txt"), PathBuf::from("C:\\Users\\dev\\c.txt")]);
        // Text a server could have put there says the same as the list: nothing.
        assert!(copied_files(&copied, "file:///home/dev/.ssh/id_ed25519").is_empty());
        assert!(copied_files(&[PathBuf::from("/home/dev/a.txt")], " /home/dev/a.txt\n").is_empty());
    }

    #[test]
    fn pasting_asks_with_every_name_and_warns_of_private_ones() {
        let plain = paste_question(&[("/home/dev/a.txt".into(), false)], "web-7731", "/srv/www");
        assert_eq!(plain.confirm.title, "Upload this to web-7731?");
        assert_eq!(plain.confirm.body, "Into /srv/www");
        assert_eq!((plain.confirm.confirm_label, plain.risky, plain.confirm.danger), ("Upload", false, false));
        let files: Vec<(String, bool)> = (1..=8).map(|n| (format!("/home/dev/f{n}"), n == 1)).chain([("/home/dev/.ssh".to_owned(), true)]).collect();
        let risky = paste_question(&files, "web-7731", "/srv/www");
        assert_eq!(risky.confirm.title, "Upload these 9 items to web-7731?");
        assert_eq!(risky.confirm.lines[0], "/home/dev/f1 (folder, with all that's in it)");
        assert_eq!(risky.confirm.lines.last().map(String::as_str), Some("and 3 more"));
        assert!(risky.risky && risky.confirm.danger);
        assert_eq!(risky.confirm.confirm_label, "Upload anyway");
        assert!(risky.confirm.body.ends_with("This looks private: /home/dev/.ssh. Once on the server, it can't be taken back."));
    }

    #[test]
    fn a_terminal_goes_into_a_folder_typed_safely() {
        assert_eq!(typed_cd("/srv/www", Shell::Posix, None).as_deref(), Some("cd -- '/srv/www'"));
        assert_eq!(typed_cd("/srv/it's", Shell::Posix, Some("clear")).as_deref(), Some("cd -- '/srv/it'\\''s' && clear"));
        assert_eq!(typed_quote("a\\b", Shell::Posix).as_deref(), Some("'a'\\\\'b'"));
        assert_eq!(typed_quote("", Shell::Posix).as_deref(), Some("''"));
        assert_eq!(typed_cd("C:/Users/it's", Shell::PowerShell, Some("clear")).as_deref(), Some("Set-Location -LiteralPath 'C:/Users/it''s'; if ($?) { clear }"));
        assert_eq!(typed_quote("a’b", Shell::PowerShell).as_deref(), Some("'a’’b'"));
        // A name that would press keys in the terminal.
        assert_eq!(typed_cd("/srv/a\nrm -rf ~", Shell::Posix, None), None);
        assert_eq!(typed_cd("/srv/\u{1b}[2J", Shell::Posix, None), None);
        assert_eq!(typed_cd("/srv/\u{9b}x", Shell::PowerShell, None), None);
        assert_eq!(cant_open_terminal().title, "Can't open a terminal there");
    }
}
