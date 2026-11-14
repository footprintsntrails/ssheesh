//! Files without a screen: what a folder's list shows and in what order, what is selected, what
//! a right click offers, how an open file is saved without writing over someone else's change,
//! and the words for all of it. A port of 0.10's `FilesPane.tsx` and the `src/lib` modules it
//! leans on, as rules that can be tested without a window or a server.
//!
//! Nothing here reads a disk, a clock or a network. The screen (`desktop/sshee-ui`'s
//! `screens::files`) holds a [`Browser`] and an [`Editor`] per Files tab, sends what they ask
//! for to the window's backend as an [`Ask`], and hands every [`Reply`] back to them. The types
//! below mirror sshee-core's (`files::FileEntry`, `OpError`, `Stamp`): this crate doesn't
//! depend on sshee-core, so the program converts at its edge (`desktop/app/src/files.rs`).

pub mod bookmarks;
pub mod browser;
pub mod editor;
pub mod format;
pub mod kinds;
pub mod menu;
pub mod ops;
pub mod protocol;

pub use browser::{Browser, Load, Problem, Row, Sort, SortKey};
pub use editor::{Body, Choice, Editor, ReadOnly};
pub use format::{bytes, modified_text, percent, rate};
pub use kinds::{archive_kind, can_unpack, glyph, media_kind, opens_as, ArchiveKind, Glyph, MediaKind, Opens};
pub use protocol::{Answer, Ask, Reply, SaveFailure, Want};

/// The biggest text the editor opens for editing. Rune's text field shapes the whole text
/// again on every key (about 12 ms at 50 KB, 46 ms at 200 KB), so above this a file is shown
/// as lines, read-only.
pub const MAX_EDIT: u64 = 128 * 1024;
/// The biggest text shown at all: sshee-core's `MAX_TEXT`.
pub const MAX_TEXT: u64 = 2 * 1024 * 1024;
/// The biggest picture read to be shown. Bigger ones are for downloading.
pub const MAX_IMAGE: u64 = 16 * 1024 * 1024;

/// What an entry of a folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A folder, or a link to one.
    Dir,
    File,
    /// A link whose target is missing.
    BrokenLink,
    /// A socket, a device, a pipe.
    Other,
}

/// One row of a folder, as the machine listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    /// A link; `kind` is what it points at.
    pub is_link: bool,
    pub size: u64,
    /// Unix seconds; 0 when the machine didn't say.
    pub modified: i64,
    /// Like `drwxr-xr-x`.
    pub mode: String,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// Dot files are hidden unless "Show hidden files" is on.
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}

/// A folder's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The folder's full path: "~" asked for comes back as "/home/deploy".
    pub path: String,
    pub entries: Vec<Entry>,
}

/// When a listing was taken, on the computer the app runs on. The list's dates ("Today 14:02")
/// are worked out from it, so drawing never reads a clock and a demo draws the same every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Clock {
    /// Unix seconds.
    pub now: i64,
    /// This computer's distance from UTC then, in seconds east.
    pub utc_offset: i32,
}

/// What kind of thing went wrong, so the screen can offer the right way out (sshee-core's
/// `ErrorCode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// The server wants a password or a key the app doesn't have.
    Auth,
    /// The server's fingerprint isn't known yet.
    HostKey,
    /// The server couldn't be reached.
    Network,
    NotFound,
    Exists,
    TooLarge,
    Unsupported,
    /// You stopped it.
    Cancelled,
    /// The file changed on the server since it was opened.
    Conflict,
    Failed,
}

/// Something that didn't work, in words for the person (sshee-core's `OpError`; its messages
/// name no host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Failure {
        Failure { kind, message: message.into() }
    }

    /// The machine can't be asked right now, which says nothing about the folder: what was on
    /// screen stays (0.10's offline rule).
    pub fn is_offline(&self) -> bool {
        matches!(self.kind, FailureKind::Network | FailureKind::Auth | FailureKind::HostKey)
    }

    /// Signing in or accepting the host key happens in a terminal: the screen offers one.
    pub fn needs_terminal(&self) -> bool {
        matches!(self.kind, FailureKind::Auth | FailureKind::HostKey)
    }
}

/// What a file was when it was read or saved (sshee-core's `Stamp`). A save hands it back, and
/// the server writes only over that very file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: i64,
    /// SHA-256 of the content in hex, when the server could tell.
    pub sha256: Option<String>,
}

/// A file read for the editor (sshee-core's `TextFile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub path: String,
    /// `None` when the file is binary or over [`MAX_TEXT`]. Lines end in `\n`.
    pub content: Option<String>,
    pub binary: bool,
    pub size: u64,
    /// Not UTF-8: shown with stand-ins, so saving would change it.
    pub lossy: bool,
    /// The file's lines end in `\r\n`; saving puts them back.
    pub crlf: bool,
}

/// How a note to the person sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Success,
    Info,
    Error,
}

/// A short note about something that finished or failed (0.10's toast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub tone: Tone,
    pub title: String,
    pub body: Option<String>,
    /// "Undo" on a move: the names to move back, see [`ops::Undo`].
    pub undo: Option<ops::Undo>,
}

impl Notice {
    pub fn new(tone: Tone, title: impl Into<String>) -> Notice {
        Notice { tone, title: title.into(), body: None, undo: None }
    }

    pub fn body(mut self, body: impl Into<String>) -> Notice {
        self.body = Some(body.into());
        self
    }

    /// "Couldn't rename" with the machine's own words under it.
    pub fn failed(title: impl Into<String>, failure: &Failure) -> Notice {
        Notice::new(Tone::Error, title).body(failure.message.clone())
    }
}

/// Numbers that tell one request's answer from an older one's: each request takes the next,
/// and an answer is used only if it carries the number its asker still waits for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tickets(u64);

impl Tickets {
    /// The next number; never 0, so 0 can mean "none".
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub fn file(name: &str, size: u64, modified: i64) -> Entry {
        Entry { name: name.into(), kind: EntryKind::File, is_link: false, size, modified, mode: "-rw-r--r--".into() }
    }

    pub fn dir(name: &str) -> Entry {
        Entry { name: name.into(), kind: EntryKind::Dir, is_link: false, size: 4096, modified: 1_700_000_000, mode: "drwxr-xr-x".into() }
    }

    pub fn listing(path: &str, entries: Vec<Entry>) -> Listing {
        Listing { path: path.into(), entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_failures_keep_the_folder_and_two_of_them_want_a_terminal() {
        let of = |kind| Failure::new(kind, "x");
        assert!(of(FailureKind::Network).is_offline() && of(FailureKind::Auth).is_offline() && of(FailureKind::HostKey).is_offline());
        assert!(!of(FailureKind::NotFound).is_offline() && !of(FailureKind::Failed).is_offline());
        assert!(of(FailureKind::Auth).needs_terminal() && of(FailureKind::HostKey).needs_terminal());
        assert!(!of(FailureKind::Network).needs_terminal());
    }

    #[test]
    fn tickets_count_up_from_one() {
        let mut t = Tickets::default();
        assert_eq!((t.next(), t.next(), t.next()), (1, 2, 3));
    }
}
