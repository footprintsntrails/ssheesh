//! What the Files screen asks the window's backend for, and what comes back, as plain data.
//! The screen never waits: it sends an [`Ask`] (`Shell::files_ask` in sshee-ui), the backend
//! does the work on a thread of its own (sshee-core in the program, a made-up tree in the
//! demo), and the [`Reply`] arrives later on the UI thread (`Remote::files`), where
//! `screens::files::deliver` hands it to the tab that asked.
//!
//! A reply names the tab and the machine of its request. Where a newer request makes an
//! older answer wrong (listings, reads, saves), both carry the request's ticket and the asker
//! drops answers to tickets it no longer waits for.

use std::path::PathBuf;

use super::{Clock, Failure, Listing, Stamp, TextFile};
use crate::transfers::{Progress, TransferId};
use crate::workbench::{ServerKey, TabId};

/// A request from one Files tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// The tab that asks; its answer goes back to it.
    pub tab: TabId,
    /// The machine, by its key in the sidebar ("local", "host:<id>").
    pub server: ServerKey,
    pub want: Want,
}

/// A file or folder to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its full path on the server.
    pub path: String,
    pub is_dir: bool,
}

/// What is asked for. Paths are full paths on the machine, with `/`.
#[derive(Debug, Clone, PartialEq)]
pub enum Want {
    /// A folder's contents. `path` may be "~".
    List { ticket: u64, path: String },
    /// A file as text, up to [`super::MAX_TEXT`], with the stamp to save it back with.
    Read { ticket: u64, path: String },
    /// A picture's bytes, up to [`super::MAX_IMAGE`].
    ReadImage { ticket: u64, path: String },
    /// Saves `text` as the file at `path`, only over the file `expected` was taken of; with no
    /// stamp, only where there's no file yet.
    Save { ticket: u64, path: String, text: String, expected: Option<Stamp> },
    MakeDir { path: String },
    /// Never over something that's there: that's [`super::FailureKind::Exists`].
    Rename { from: String, to: String },
    /// Deletes `names` in `dir`, folders with all that's in them. One that fails doesn't stop
    /// the rest.
    Delete { dir: String, names: Vec<String> },
    /// Unpacks the archive `name` next to itself, into a folder of its own.
    Extract { dir: String, name: String },
    /// Moves `names` from the folder `from` into the folder `to` on the same machine. Nothing
    /// there is replaced: a name that's taken stays where it was.
    Move { from: String, names: Vec<String>, to: String },
    /// Downloads into this computer's Downloads folder, one after another, under one card.
    Download { id: TransferId, items: Vec<Item> },
    /// Sends files and folders of this computer into `dir`. Without `overwrite`, nothing is
    /// sent if any name is taken there ([`super::FailureKind::Exists`]): the screen asks, and
    /// sends the same again with `overwrite`.
    Upload { id: TransferId, paths: Vec<PathBuf>, dir: String, overwrite: bool },
    /// Stops a transfer; its end still arrives, as [`super::FailureKind::Cancelled`].
    CancelTransfer { id: TransferId },
}

/// Why a save wrote nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveFailure {
    /// The file isn't what it was when it was opened. `now` is what's there, to save over it
    /// knowingly; `None` when nothing is (deleted or moved).
    Conflict { now: Option<Stamp> },
    Failed(Failure),
}

/// The backend's answer to an [`Ask`], or news of a transfer.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub tab: TabId,
    pub server: ServerKey,
    pub answer: Answer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// `asked` is the request's path; the listing's own may differ ("~").
    Listed { ticket: u64, asked: String, result: Result<(Listing, Clock), Failure> },
    Read { ticket: u64, path: String, result: Result<(TextFile, Stamp), Failure> },
    Image { ticket: u64, path: String, result: Result<Vec<u8>, Failure> },
    /// The stamp of what was saved, for the next save.
    Saved { ticket: u64, path: String, result: Result<Stamp, SaveFailure> },
    MadeDir { path: String, result: Result<(), Failure> },
    Renamed { from: String, to: String, result: Result<(), Failure> },
    /// What went, and what didn't with why.
    Deleted { dir: String, deleted: Vec<String>, failed: Vec<(String, Failure)> },
    /// The name of the folder (or file) that came out, in `dir`.
    Extracted { dir: String, name: String, result: Result<String, Failure> },
    /// `taken`: names that stayed because `to` has one of that name. `failure`: what stopped
    /// the rest, if anything did.
    Moved { from: String, to: String, moved: Vec<String>, taken: Vec<String>, failure: Option<Failure> },
    /// A transfer moved on.
    Progress { id: TransferId, progress: Progress },
    /// A download ended: where each item landed on this computer, in the request's order.
    Downloaded { id: TransferId, result: Result<Vec<String>, Failure> },
    /// An upload ended. The request's fields come back so it can be sent again to replace.
    Uploaded { id: TransferId, paths: Vec<PathBuf>, dir: String, overwrite: bool, result: Result<(), Failure> },
}

impl Ask {
    /// The answer to this request, addressed back to its tab.
    pub fn reply(&self, answer: Answer) -> Reply {
        Reply { tab: self.tab, server: self.server.clone(), answer }
    }
}
