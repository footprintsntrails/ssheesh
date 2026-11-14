//! One open file beside the list: read, shown, edited and saved. Like [`super::Browser`] it
//! does no I/O: what it needs comes back as a [`Want`] to send, and the answers come in
//! through [`Editor::read`], [`Editor::image`] and [`Editor::saved`].
//!
//! The text being typed (the draft) lives in the screen's text field, not here: the editor
//! holds the text as last read or saved, and is handed the draft when it matters (is it
//! changed? save it). Whenever the editor's own text is replaced from the server,
//! [`Editor::revision`] goes up: the screen then puts [`Editor::text`] into its field.
//!
//! Saving never writes over a change made on the server meanwhile (sshee-core's
//! `save_guarded`): the save carries the stamp of what was read, and when the file is no
//! longer that, nothing is written and the editor holds a [`Conflict`] with the ways out.

use std::sync::Arc;

use super::kinds::{opens_as, Opens};
use super::protocol::{SaveFailure, Want};
use super::{Failure, Notice, Stamp, TextFile, Tickets, Tone, MAX_EDIT};
use crate::paths;

/// Why a text is shown but can't be changed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnly {
    /// Not UTF-8: it's shown with stand-ins for what couldn't be read, and saving would write
    /// the stand-ins.
    NotUtf8,
    /// Over [`MAX_EDIT`]: shown as lines, not in a text field.
    TooBigToEdit,
}

impl ReadOnly {
    /// The line above the text.
    pub fn note(self) -> &'static str {
        match self {
            ReadOnly::NotUtf8 => "Read-only: this file isn't UTF-8 text, so saving it here would change the characters it couldn't read.",
            ReadOnly::TooBigToEdit => "Read-only: this file is too big to edit here (over 128 KB).",
        }
    }
}

/// What the editor shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Being read.
    Loading,
    /// Text, with `\n` between lines whatever the file has. `saved` is the file as last read
    /// or saved. Markdown is text too.
    Text { saved: String, read_only: Option<ReadOnly> },
    /// Not text: [`Editor::message`] says so.
    Binary,
    /// Text over sshee-core's 2 MB.
    TooBig,
    /// A picture's bytes as the file has them (png, jpg, gif, webp, bmp, ico), for Rune to
    /// decode.
    Image(Arc<[u8]>),
    /// A video, an SVG, a picture Rune can't decode: nothing was read.
    NoPreview,
    /// It couldn't be read.
    Failed(Failure),
}

/// A way out of a [`Conflict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Save what you have over what's there now, knowingly. When the file is gone, save it
    /// as a new one.
    Overwrite,
    /// Read the file again and drop what you typed.
    LoadTheirs,
    /// Save what you have under a new name beside it ("notes (my copy).md"), leaving the
    /// file as it is. The editor carries on with the copy.
    SaveCopy,
}

/// The file on the server isn't what was opened, so a save wrote nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// What's there now; `None` when nothing is (deleted or moved).
    pub now: Option<Stamp>,
}

impl Conflict {
    /// The notice's text, in sshee-core's words (`files::Conflict::message`).
    pub fn message(&self) -> &'static str {
        match self.now {
            Some(_) => "This file changed on the server since you opened it, so it wasn't saved over.",
            None => "This file isn't on the server anymore (it was deleted or moved), so it wasn't saved.",
        }
    }

    /// The buttons, in order, the safest last. A file that's gone has nothing to load.
    pub fn choices(&self) -> Vec<(Choice, &'static str)> {
        match self.now {
            Some(_) => vec![(Choice::Overwrite, "Save over it"), (Choice::LoadTheirs, "Load what's there"), (Choice::SaveCopy, "Save mine as a copy")],
            None => vec![(Choice::Overwrite, "Save it anyway"), (Choice::SaveCopy, "Save mine as a copy")],
        }
    }
}

/// How a save ended, for the screen to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum AfterSave {
    /// An answer nobody waits for.
    Stale,
    /// Saved. The folder's listing is out of date now. `copy` is the new file's name when it
    /// went beside the original: the list picks it out.
    Done { notice: Notice, copy: Option<String> },
    /// Nothing was written: [`Editor::conflict`] has the ways out.
    Conflict,
    /// A copy's name was taken in the meantime: send this, with the next name.
    Again(Want),
    Failed(Notice),
}

/// A save on its way.
#[derive(Debug, Clone, PartialEq)]
struct Saving {
    ticket: u64,
    /// The draft as it was sent, with `\n`: what the file holds once this is saved.
    sent: String,
    /// The name it goes under when that isn't the file's own, and which try this is.
    copy: Option<(String, u32)>,
}

/// One open file. See the module's docs.
#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    dir: String,
    name: String,
    body: Body,
    /// The file's lines end in `\r\n`.
    crlf: bool,
    /// What the file was when read or last saved.
    stamp: Option<Stamp>,
    /// The ticket of the read on its way; 0 when none is.
    reading: u64,
    saving: Option<Saving>,
    conflict: Option<Conflict>,
    revision: u64,
}

/// The most names tried for a copy before giving up.
const COPY_TRIES: u32 = 50;

/// "notes.md" → "notes (my copy).md", then "notes (my copy 2).md". What follows the last dot
/// stays the ending (with ".tar" before it: "site (my copy).tar.gz"); a dot file has none.
pub fn copy_name(name: &str, n: u32) -> String {
    let split = match name.rfind('.') {
        Some(0) | None => name.len(),
        Some(i) => match name[..i].strip_suffix(".tar") {
            Some(stem) if !stem.is_empty() => stem.len(),
            _ => i,
        },
    };
    let (stem, ending) = name.split_at(split);
    match n {
        0 | 1 => format!("{stem} (my copy){ending}"),
        n => format!("{stem} (my copy {n}){ending}"),
    }
}

/// The text as the file wants it: `\r\n` put back where the file had them.
fn for_file(text: &str, crlf: bool) -> String {
    match crlf {
        true => text.replace("\r\n", "\n").replace('\n', "\r\n"),
        false => text.to_owned(),
    }
}

impl Editor {
    /// Opens `name` of the folder `dir`. By its name it's read as text, read as a picture, or
    /// not read at all; the request to send comes back with the editor.
    pub fn open(dir: &str, name: &str, tickets: &mut Tickets) -> (Editor, Option<Want>) {
        let mut editor =
            Editor { dir: dir.to_owned(), name: name.to_owned(), body: Body::Loading, crlf: false, stamp: None, reading: 0, saving: None, conflict: None, revision: 0 };
        let want = match opens_as(name) {
            Opens::Nothing => {
                editor.body = Body::NoPreview;
                None
            }
            Opens::Image => {
                editor.reading = tickets.next();
                Some(Want::ReadImage { ticket: editor.reading, path: editor.path() })
            }
            Opens::Text => Some(editor.read_again(tickets)),
        };
        (editor, want)
    }

    fn read_again(&mut self, tickets: &mut Tickets) -> Want {
        self.body = Body::Loading;
        self.reading = tickets.next();
        Want::Read { ticket: self.reading, path: self.path() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The folder the file is in.
    pub fn dir(&self) -> &str {
        &self.dir
    }

    pub fn path(&self) -> String {
        paths::join(&self.dir, &self.name)
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    /// The file's text as last read or saved.
    pub fn text(&self) -> Option<&str> {
        match &self.body {
            Body::Text { saved, .. } => Some(saved),
            _ => None,
        }
    }

    pub fn read_only(&self) -> Option<ReadOnly> {
        match &self.body {
            Body::Text { read_only, .. } => *read_only,
            _ => None,
        }
    }

    /// Text in a field you can type in.
    pub fn can_edit(&self) -> bool {
        matches!(self.body, Body::Text { read_only: None, .. })
    }

    /// Goes up whenever [`Editor::text`] was replaced by what the server has (the first read,
    /// "Load what's there"): the screen's field takes the text again. A save doesn't count:
    /// the field already holds what was saved, and maybe more typed since.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_loading(&self) -> bool {
        self.body == Body::Loading
    }

    pub fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    /// Whether `draft` (the field's text) differs from the file as last read or saved: there
    /// is something to save, and closing would lose it.
    pub fn is_dirty(&self, draft: &str) -> bool {
        match &self.body {
            Body::Text { saved, read_only: None } => saved != draft,
            _ => false,
        }
    }

    /// What to say instead of showing the file, when it can't be shown: a title and a line
    /// under it. On a server the screen offers the download beside it.
    pub fn message(&self, on_server: bool) -> Option<(String, String)> {
        let plain = |text: &str| Some((text.to_owned(), String::new()));
        match &self.body {
            Body::Binary => plain("This is a binary file."),
            Body::TooBig => plain("This file is too big to edit here (over 2 MB)."),
            Body::NoPreview => Some(("This kind of file can't be shown here.".into(), if on_server { "Download it to open it.".into() } else { String::new() })),
            Body::Failed(failure) => Some((format!("Couldn't open {}", self.name), failure.message.clone())),
            Body::Loading | Body::Text { .. } | Body::Image(_) => None,
        }
    }

    /// The open file was renamed in the list: the editor follows it, with what was typed. Not
    /// while it's still being read (the answer would be for the old name).
    pub fn renamed(&mut self, name: &str) -> bool {
        if self.reading != 0 || self.saving.is_some() {
            return false;
        }
        self.name = name.to_owned();
        true
    }

    /// The answer to a read. False when it's for another file or an older request.
    pub fn read(&mut self, ticket: u64, path: &str, result: Result<(TextFile, Stamp), Failure>) -> bool {
        if ticket == 0 || ticket != self.reading || path != self.path() {
            return false;
        }
        self.reading = 0;
        self.revision += 1;
        match result {
            Ok((file, stamp)) => {
                self.crlf = file.crlf;
                self.stamp = Some(stamp);
                self.body = match file.content {
                    Some(text) => {
                        let read_only = if file.lossy {
                            Some(ReadOnly::NotUtf8)
                        } else if text.len() as u64 > MAX_EDIT {
                            Some(ReadOnly::TooBigToEdit)
                        } else {
                            None
                        };
                        Body::Text { saved: text, read_only }
                    }
                    None if file.binary => Body::Binary,
                    None => Body::TooBig,
                };
            }
            Err(failure) => self.body = Body::Failed(failure),
        }
        true
    }

    /// The answer to a picture's read. False when it's for another file or an older request.
    pub fn image(&mut self, ticket: u64, path: &str, result: Result<Vec<u8>, Failure>) -> bool {
        if ticket == 0 || ticket != self.reading || path != self.path() {
            return false;
        }
        self.reading = 0;
        self.body = match result {
            Ok(bytes) => Body::Image(bytes.into()),
            Err(failure) => Body::Failed(failure),
        };
        true
    }

    fn send(&mut self, draft: &str, expected: Option<Stamp>, copy: Option<(String, u32)>, tickets: &mut Tickets) -> Want {
        let ticket = tickets.next();
        let path = match &copy {
            Some((name, _)) => paths::join(&self.dir, name),
            None => self.path(),
        };
        self.saving = Some(Saving { ticket, sent: draft.to_owned(), copy });
        Want::Save { ticket, path, text: for_file(draft, self.crlf), expected }
    }

    /// Saves `draft` over the file that was opened. `None` when there's nothing to send: the
    /// file isn't text that can be edited here, a save is on its way, or a conflict waits for
    /// its answer.
    pub fn save(&mut self, draft: &str, tickets: &mut Tickets) -> Option<Want> {
        if !self.can_edit() || self.saving.is_some() || self.conflict.is_some() {
            return None;
        }
        let expected = self.stamp.clone();
        Some(self.send(draft, expected, None, tickets))
    }

    /// The conflict a save ran into, until one of its [`Conflict::choices`] is taken.
    pub fn conflict(&self) -> Option<&Conflict> {
        self.conflict.as_ref()
    }

    /// Takes a way out of the conflict. `draft` is the field's text; `taken` says whether the
    /// folder has a name (for the copy's). Returns what to send.
    pub fn resolve(&mut self, choice: Choice, draft: &str, taken: impl Fn(&str) -> bool, tickets: &mut Tickets) -> Option<Want> {
        if self.saving.is_some() {
            return None;
        }
        let conflict = self.conflict.take()?;
        Some(match choice {
            // Over the file as it is now, and only that one: if it changes yet again before
            // this lands, it's a conflict again. Gone: only where there's still nothing.
            Choice::Overwrite => self.send(draft, conflict.now, None, tickets),
            Choice::LoadTheirs if conflict.now.is_some() => self.read_again(tickets),
            // Nothing to load: the conflict stays.
            Choice::LoadTheirs => {
                self.conflict = Some(conflict);
                return None;
            }
            Choice::SaveCopy => {
                let n = (1..COPY_TRIES).find(|n| !taken(&copy_name(&self.name, *n))).unwrap_or(COPY_TRIES);
                // No stamp: it lands only where there's no file, so it never replaces one.
                self.send(draft, None, Some((copy_name(&self.name, n), n)), tickets)
            }
        })
    }

    /// The answer to a save.
    pub fn saved(&mut self, ticket: u64, result: Result<Stamp, SaveFailure>, tickets: &mut Tickets) -> AfterSave {
        let Some(saving) = self.saving.take_if(|s| s.ticket == ticket) else { return AfterSave::Stale };
        match (result, saving.copy) {
            (Ok(stamp), copy) => {
                // What was saved is what was sent; anything typed meanwhile is still unsaved.
                if let Body::Text { saved, .. } = &mut self.body {
                    *saved = saving.sent;
                }
                self.stamp = Some(stamp);
                let notice = match &copy {
                    Some((name, _)) => {
                        let notice = Notice::new(Tone::Success, format!("Saved your version as “{name}”")).body(format!("{} is as it was on the server.", self.name));
                        self.name = name.clone();
                        notice
                    }
                    None => Notice::new(Tone::Success, format!("Saved {}", self.name)),
                };
                AfterSave::Done { notice, copy: copy.map(|(name, _)| name) }
            }
            // The copy's name was taken between the listing and the save: the next one.
            (Err(SaveFailure::Conflict { now: Some(_) }), Some((_, n))) if n < COPY_TRIES => {
                let name = copy_name(&self.name, n + 1);
                AfterSave::Again(self.send(&saving.sent, None, Some((name, n + 1)), tickets))
            }
            (Err(SaveFailure::Conflict { .. }), Some((name, _))) => AfterSave::Failed(Notice::new(Tone::Error, format!("Couldn't save {name}")).body("That name is taken. Nothing was saved.")),
            (Err(SaveFailure::Conflict { now }), None) => {
                self.conflict = Some(Conflict { now });
                AfterSave::Conflict
            }
            (Err(SaveFailure::Failed(failure)), copy) => {
                let name = copy.map_or_else(|| self.name.clone(), |(name, _)| name);
                AfterSave::Failed(Notice::failed(format!("Couldn't save {name}"), &failure))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::FailureKind;
    use super::*;

    const DIR: &str = "/home/deploy";

    fn stamp(n: u64) -> Stamp {
        Stamp { size: n, modified: 1_700_000_000 + n as i64, sha256: Some(format!("{n:064x}")) }
    }

    fn text_file(content: &str) -> TextFile {
        TextFile { path: format!("{DIR}/notes.md"), content: Some(content.into()), binary: false, size: content.len() as u64, lossy: false, crlf: false }
    }

    /// "notes.md" open with `content`, stamp 1.
    fn opened(content: &str) -> (Editor, Tickets) {
        let mut tickets = Tickets::default();
        let (mut e, want) = Editor::open(DIR, "notes.md", &mut tickets);
        let Some(Want::Read { ticket, path }) = want else { panic!("{want:?}") };
        assert_eq!(path, "/home/deploy/notes.md");
        assert!(e.is_loading());
        assert!(e.read(ticket, &path, Ok((text_file(content), stamp(1)))));
        (e, tickets)
    }

    #[test]
    fn a_file_opens_by_its_name() {
        let mut tickets = Tickets::default();
        let (e, want) = Editor::open(DIR, "logo.png", &mut tickets);
        assert_eq!(want, Some(Want::ReadImage { ticket: 1, path: "/home/deploy/logo.png".into() }));
        assert!(e.is_loading());
        let (e, want) = Editor::open(DIR, "intro.mp4", &mut tickets);
        assert_eq!((want, e.body()), (None, &Body::NoPreview));
        assert_eq!(e.message(true), Some(("This kind of file can't be shown here.".into(), "Download it to open it.".into())));
        assert_eq!(e.message(false).unwrap().1, "");
        let (e, want) = Editor::open("/", "fstab", &mut tickets);
        assert_eq!(want, Some(Want::Read { ticket: 2, path: "/fstab".into() }));
        assert_eq!((e.name(), e.dir(), e.path().as_str()), ("fstab", "/", "/fstab"));
    }

    #[test]
    fn text_opens_for_editing_and_knows_when_it_changed() {
        let (e, _) = opened("one\ntwo\n");
        assert_eq!(e.text(), Some("one\ntwo\n"));
        assert!(e.can_edit() && e.read_only().is_none() && !e.is_loading());
        assert_eq!(e.revision(), 1);
        assert!(!e.is_dirty("one\ntwo\n") && e.is_dirty("one\ntwo\nthree\n"));
        assert_eq!(e.message(true), None);
    }

    #[test]
    fn what_cant_be_edited_says_why() {
        let mut tickets = Tickets::default();
        let open = |file: TextFile, tickets: &mut Tickets| {
            let (mut e, want) = Editor::open(DIR, "notes.md", tickets);
            let Some(Want::Read { ticket, path }) = want else { panic!() };
            e.read(ticket, &path, Ok((file, stamp(1))));
            e
        };
        let lossy = open(TextFile { lossy: true, ..text_file("caf\u{fffd}") }, &mut tickets);
        assert_eq!(lossy.read_only(), Some(ReadOnly::NotUtf8));
        assert!(!lossy.can_edit() && !lossy.is_dirty("anything"));
        assert!(lossy.read_only().unwrap().note().starts_with("Read-only: this file isn't UTF-8 text"));

        let big = "x".repeat(MAX_EDIT as usize + 1);
        let mut long = open(text_file(&big), &mut tickets);
        assert_eq!(long.read_only(), Some(ReadOnly::TooBigToEdit));
        assert_eq!(long.text().map(str::len), Some(big.len()));
        assert_eq!(long.save(&big, &mut tickets), None);
        assert!(open(text_file(&"x".repeat(MAX_EDIT as usize)), &mut tickets).can_edit());

        let binary = open(TextFile { content: None, binary: true, ..text_file("") }, &mut tickets);
        assert_eq!(binary.message(true), Some(("This is a binary file.".into(), String::new())));
        let huge = open(TextFile { content: None, size: 3 << 20, ..text_file("") }, &mut tickets);
        assert_eq!((huge.body(), huge.message(false).unwrap().0.as_str()), (&Body::TooBig, "This file is too big to edit here (over 2 MB)."));

        let (mut gone, want) = Editor::open(DIR, "notes.md", &mut tickets);
        let Some(Want::Read { ticket, path }) = want else { panic!() };
        gone.read(ticket, &path, Err(Failure::new(FailureKind::NotFound, "No such file or directory")));
        assert_eq!(gone.message(true), Some(("Couldn't open notes.md".into(), "No such file or directory".into())));
    }

    #[test]
    fn answers_for_another_file_or_an_older_read_are_dropped() {
        let mut tickets = Tickets::default();
        let (mut e, want) = Editor::open(DIR, "notes.md", &mut tickets);
        let Some(Want::Read { ticket, path }) = want else { panic!() };
        assert!(!e.read(ticket, "/home/deploy/other.md", Ok((text_file("x"), stamp(1)))));
        assert!(!e.read(ticket + 1, &path, Ok((text_file("x"), stamp(1)))));
        assert!(e.is_loading());
        let (mut pic, want) = Editor::open(DIR, "logo.png", &mut tickets);
        let Some(Want::ReadImage { ticket, path }) = want else { panic!() };
        assert!(!pic.image(ticket - 1, &path, Ok(vec![1, 2, 3])));
        assert!(pic.image(ticket, &path, Ok(vec![1, 2, 3])));
        assert_eq!(pic.body(), &Body::Image(vec![1u8, 2, 3].into()));
        assert!(!pic.image(ticket, &path, Ok(vec![9])), "once");
        let (mut big, want) = Editor::open(DIR, "photo.jpg", &mut tickets);
        let Some(Want::ReadImage { ticket, path }) = want else { panic!() };
        big.image(ticket, &path, Err(Failure::new(FailureKind::TooLarge, "This file is too big to open here.")));
        assert_eq!(big.message(true).unwrap(), ("Couldn't open photo.jpg".into(), "This file is too big to open here.".into()));
    }

    #[test]
    fn a_save_carries_the_stamp_of_what_was_read_and_the_next_one_the_stamp_it_got_back() {
        let (mut e, mut tickets) = opened("one\n");
        let want = e.save("one\ntwo\n", &mut tickets).unwrap();
        let Want::Save { ticket, path, text, expected } = want else { panic!() };
        assert_eq!((path.as_str(), text.as_str(), expected), ("/home/deploy/notes.md", "one\ntwo\n", Some(stamp(1))));
        assert!(e.is_saving());
        // One save at a time.
        assert_eq!(e.save("one\ntwo\nthree\n", &mut tickets), None);
        let after = e.saved(ticket, Ok(stamp(2)), &mut tickets);
        assert_eq!(after, AfterSave::Done { notice: Notice::new(Tone::Success, "Saved notes.md"), copy: None });
        // What was sent is saved; what was typed since isn't.
        assert!(!e.is_saving() && !e.is_dirty("one\ntwo\n") && e.is_dirty("one\ntwo\nthree\n"));
        assert_eq!(e.revision(), 1, "a save doesn't put the text back into the field");
        let Some(Want::Save { expected, .. }) = e.save("one\ntwo\nthree\n", &mut tickets) else { panic!() };
        assert_eq!(expected, Some(stamp(2)));
        // An answer to a save nobody waits for.
        assert_eq!(e.saved(ticket, Ok(stamp(9)), &mut tickets), AfterSave::Stale);
    }

    #[test]
    fn windows_line_endings_go_back_as_they_came() {
        let mut tickets = Tickets::default();
        let (mut e, want) = Editor::open(DIR, "notes.md", &mut tickets);
        let Some(Want::Read { ticket, path }) = want else { panic!() };
        e.read(ticket, &path, Ok((TextFile { crlf: true, ..text_file("one\ntwo\n") }, stamp(1))));
        assert_eq!(e.text(), Some("one\ntwo\n"));
        // A pasted "\r\n" isn't doubled.
        let Some(Want::Save { text, .. }) = e.save("one\ntwo\r\nthree", &mut tickets) else { panic!() };
        assert_eq!(text, "one\r\ntwo\r\nthree");
    }

    #[test]
    fn a_failed_save_keeps_everything_for_another_try() {
        let (mut e, mut tickets) = opened("one\n");
        let Some(Want::Save { ticket, .. }) = e.save("two\n", &mut tickets) else { panic!() };
        let after = e.saved(ticket, Err(SaveFailure::Failed(Failure::new(FailureKind::Network, "Can't reach the server."))), &mut tickets);
        assert_eq!(after, AfterSave::Failed(Notice::new(Tone::Error, "Couldn't save notes.md").body("Can't reach the server.")));
        assert!(!e.is_saving() && e.is_dirty("two\n") && e.conflict().is_none());
        let Some(Want::Save { expected, .. }) = e.save("two\n", &mut tickets) else { panic!() };
        assert_eq!(expected, Some(stamp(1)));
    }

    /// An editor whose save of "mine\n" ran into `now` on the server.
    fn conflicted(now: Option<Stamp>) -> (Editor, Tickets) {
        let (mut e, mut tickets) = opened("one\n");
        let Some(Want::Save { ticket, .. }) = e.save("mine\n", &mut tickets) else { panic!() };
        assert_eq!(e.saved(ticket, Err(SaveFailure::Conflict { now }), &mut tickets), AfterSave::Conflict);
        (e, tickets)
    }

    #[test]
    fn a_change_on_the_server_stops_the_save_and_offers_three_ways_out() {
        let (mut e, mut tickets) = conflicted(Some(stamp(7)));
        let conflict = e.conflict().unwrap().clone();
        assert_eq!(conflict.message(), "This file changed on the server since you opened it, so it wasn't saved over.");
        assert_eq!(conflict.choices().iter().map(|c| c.0).collect::<Vec<_>>(), [Choice::Overwrite, Choice::LoadTheirs, Choice::SaveCopy]);
        // Nothing was saved, and saving again waits for the answer.
        assert!(e.is_dirty("mine\n") && !e.is_saving());
        assert_eq!(e.save("mine\n", &mut tickets), None);
    }

    #[test]
    fn overwrite_saves_over_the_file_as_it_is_now() {
        let (mut e, mut tickets) = conflicted(Some(stamp(7)));
        let Some(Want::Save { ticket, path, text, expected }) = e.resolve(Choice::Overwrite, "mine\n", |_| false, &mut tickets) else { panic!() };
        assert_eq!((path.as_str(), text.as_str(), expected), ("/home/deploy/notes.md", "mine\n", Some(stamp(7))));
        assert!(e.conflict().is_none());
        // It changed yet again meanwhile: asked again, with the newer stamp.
        assert_eq!(e.saved(ticket, Err(SaveFailure::Conflict { now: Some(stamp(8)) }), &mut tickets), AfterSave::Conflict);
        let Some(Want::Save { ticket, expected, .. }) = e.resolve(Choice::Overwrite, "mine\n", |_| false, &mut tickets) else { panic!() };
        assert_eq!(expected, Some(stamp(8)));
        assert!(matches!(e.saved(ticket, Ok(stamp(9)), &mut tickets), AfterSave::Done { copy: None, .. }));
        assert!(!e.is_dirty("mine\n"));
    }

    #[test]
    fn load_theirs_reads_again_and_the_field_takes_the_text() {
        let (mut e, mut tickets) = conflicted(Some(stamp(7)));
        let Some(Want::Read { ticket, path }) = e.resolve(Choice::LoadTheirs, "mine\n", |_| false, &mut tickets) else { panic!() };
        assert!(e.is_loading() && e.conflict().is_none());
        assert!(e.read(ticket, &path, Ok((text_file("theirs\n"), stamp(7)))));
        assert_eq!((e.text(), e.revision()), (Some("theirs\n"), 2));
        let Some(Want::Save { expected, .. }) = e.save("theirs\nmore\n", &mut tickets) else { panic!() };
        assert_eq!(expected, Some(stamp(7)));
    }

    #[test]
    fn a_copy_goes_beside_the_file_under_a_free_name_and_never_replaces() {
        let (mut e, mut tickets) = conflicted(Some(stamp(7)));
        let taken = |name: &str| name == "notes (my copy).md";
        let Some(Want::Save { ticket, path, text, expected }) = e.resolve(Choice::SaveCopy, "mine\n", taken, &mut tickets) else { panic!() };
        assert_eq!((path.as_str(), text.as_str(), expected), ("/home/deploy/notes (my copy 2).md", "mine\n", None));
        // Someone took that name in the meantime: the next one, by itself.
        let AfterSave::Again(Want::Save { ticket: again, path, expected, .. }) = e.saved(ticket, Err(SaveFailure::Conflict { now: Some(stamp(3)) }), &mut tickets) else { panic!() };
        assert_eq!((path.as_str(), expected), ("/home/deploy/notes (my copy 3).md", None));
        let after = e.saved(again, Ok(stamp(4)), &mut tickets);
        let AfterSave::Done { notice, copy } = after else { panic!("{after:?}") };
        assert_eq!(copy.as_deref(), Some("notes (my copy 3).md"));
        assert_eq!((notice.title.as_str(), notice.body.as_deref()), ("Saved your version as “notes (my copy 3).md”", Some("notes.md is as it was on the server.")));
        // The editor carries on with the copy, saved.
        assert_eq!((e.name(), e.path().as_str()), ("notes (my copy 3).md", "/home/deploy/notes (my copy 3).md"));
        assert!(!e.is_dirty("mine\n") && e.conflict().is_none());
        let Some(Want::Save { path, expected, .. }) = e.save("mine\nmore\n", &mut tickets) else { panic!() };
        assert_eq!((path.as_str(), expected), ("/home/deploy/notes (my copy 3).md", Some(stamp(4))));
    }

    #[test]
    fn a_file_that_is_gone_can_be_saved_anew_or_as_a_copy_but_not_loaded() {
        let (mut e, mut tickets) = conflicted(None);
        let conflict = e.conflict().unwrap().clone();
        assert_eq!(conflict.message(), "This file isn't on the server anymore (it was deleted or moved), so it wasn't saved.");
        assert_eq!(conflict.choices(), [(Choice::Overwrite, "Save it anyway"), (Choice::SaveCopy, "Save mine as a copy")]);
        assert_eq!(e.resolve(Choice::LoadTheirs, "mine\n", |_| false, &mut tickets), None);
        assert!(e.conflict().is_some());
        // As a new file: no stamp, so it lands only if there's still nothing there.
        let Some(Want::Save { path, expected, .. }) = e.resolve(Choice::Overwrite, "mine\n", |_| false, &mut tickets) else { panic!() };
        assert_eq!((path.as_str(), expected), ("/home/deploy/notes.md", None));
    }

    #[test]
    fn a_copy_that_fails_says_which_name() {
        let (mut e, mut tickets) = conflicted(Some(stamp(7)));
        let Some(Want::Save { ticket, .. }) = e.resolve(Choice::SaveCopy, "mine\n", |_| false, &mut tickets) else { panic!() };
        let after = e.saved(ticket, Err(SaveFailure::Failed(Failure::new(FailureKind::Failed, "Permission denied"))), &mut tickets);
        assert_eq!(after, AfterSave::Failed(Notice::new(Tone::Error, "Couldn't save notes (my copy).md").body("Permission denied")));
        // Still the original, still unsaved.
        assert_eq!(e.name(), "notes.md");
        assert!(e.is_dirty("mine\n"));
    }

    #[test]
    fn copy_names() {
        assert_eq!(copy_name("notes.md", 1), "notes (my copy).md");
        assert_eq!(copy_name("notes.md", 2), "notes (my copy 2).md");
        assert_eq!(copy_name("Makefile", 1), "Makefile (my copy)");
        assert_eq!(copy_name(".env", 1), ".env (my copy)");
        assert_eq!(copy_name(".env.production", 3), ".env (my copy 3).production");
        assert_eq!(copy_name("site.tar.gz", 1), "site (my copy).tar.gz");
        assert_eq!(copy_name(".tar.gz", 1), ".tar (my copy).gz");
    }
}
