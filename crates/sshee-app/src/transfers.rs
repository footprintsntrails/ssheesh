//! Uploads and downloads in progress, shown as cards in the corner: how far each got, how
//! fast it goes, whether it waits for a dropped connection, and a button to stop it. A port of
//! 0.10's src/lib/transfers.ts (the list and the speed), src/lib/cleaning.ts (what cleaning
//! took out, in a line) and the wording of src/components/Transfers.tsx.
//!
//! The window owns one [`Transfers`]. The screen adds a card when it asks the backend for a
//! transfer, the backend's progress goes through [`Transfers::progress`], and the card leaves
//! when the transfer ends, whichever way.

use std::path::Path;

use crate::files::format::{bytes, percent, rate};
use crate::paths::local_name;
use crate::workbench::{ServerKey, TabId};

/// Names one transfer between the screen and the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransferId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// From this computer to a server.
    Upload,
    /// From a server to this computer.
    Download,
    /// Within this computer (files dropped on one of its own folders).
    Copy,
}

impl Kind {
    /// "Uploading", for the card and its Stop button.
    pub fn verb(self) -> &'static str {
        match self {
            Kind::Upload => "Uploading",
            Kind::Download => "Downloading",
            Kind::Copy => "Copying",
        }
    }
}

/// What came out of uploaded files on the way (sshee-core's `scrub::Cleaning`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaning {
    /// Files something came out of.
    pub cleaned: u32,
    /// Files of a kind it cleans that had nothing to take out.
    pub clean: u32,
    /// Files sent as they are.
    pub as_is: u32,
    /// Folders: the files inside go as they are.
    pub folders: u32,
    /// What came out, across all files, each once: "location", "camera".
    pub removed: Vec<String>,
}

/// "Removed location, camera", "No metadata to remove", or "" when there's nothing to say yet.
pub fn cleaning_note(c: &Cleaning) -> String {
    let files = c.cleaned + c.clean + c.as_is;
    if !c.removed.is_empty() {
        let what = format!("Removed {}", c.removed.join(", "));
        return if files > 1 { format!("{what} · {} of {files} files", c.cleaned) } else { what };
    }
    if files == 0 {
        return if c.folders > 0 { "Files inside folders go as they are".into() } else { String::new() };
    }
    if c.as_is == 0 {
        return "No metadata to remove".into();
    }
    if files == 1 { "As is: SSHeesh doesn't clean this kind of file" } else { "Nothing removed" }.into()
}

/// "notes.txt" or "3 items", for cards and messages. The paths are this computer's or a
/// server's; either may use `\` or `/`.
pub fn files_label<P: AsRef<Path>>(paths: &[P]) -> String {
    match paths {
        [one] => {
            let path = one.as_ref().to_string_lossy();
            local_name(&path).to_owned()
        }
        _ => format!("{} items", paths.len()),
    }
}

/// Where a transfer stands, as the backend reports it a few times a second.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    /// 0 while the size isn't known.
    pub total: u64,
    /// The connection dropped; it carries on once the server answers again.
    pub waiting: bool,
    /// What came out of the files so far; `None` when they go uncleaned.
    pub cleaning: Option<Cleaning>,
    /// When this was measured, in milliseconds on the backend's own steady clock: speed is
    /// worked out from these, so it doesn't depend on when the window got round to it.
    pub at_ms: u64,
}

/// One upload or download in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    pub id: TransferId,
    /// The Files tab that started it and its machine: where its end is told, and how Stop
    /// reaches the backend also after that tab closed.
    pub tab: TabId,
    pub server: ServerKey,
    pub kind: Kind,
    /// A file's name, or "3 items".
    pub label: String,
    pub done: u64,
    /// 0 while the size isn't known.
    pub total: u64,
    /// Bytes per second, smoothed; 0 until measured.
    pub rate: f64,
    pub waiting: bool,
    /// Stop was pressed; the backend hasn't said it ended yet.
    pub cancelling: bool,
    pub cleaning: Option<Cleaning>,
    /// The progress speed is measured from: when, and how far it was.
    sample: Option<(u64, u64)>,
}

/// A card's texts and numbers, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The file's name or "3 items".
    pub label: String,
    /// The Stop button's name for a screen reader: "Stop uploading notes.txt".
    pub stop_label: String,
    /// False once Stop was pressed.
    pub can_stop: bool,
    /// How full the bar is; `None` while the size isn't known (the bar sweeps).
    pub fraction: Option<f64>,
    /// "1.5 MB of 4.0 MB · 38%", "Uploading… 1.5 MB", "Stopping…", or that the connection
    /// was lost.
    pub line: String,
    /// "1.2 MB/s", when it's moving and measured.
    pub rate: Option<String>,
    /// What cleaning did so far, when there's something to say.
    pub cleaned: Option<String>,
    /// The connection dropped: the card turns to its warning colours.
    pub waiting: bool,
}

impl Transfer {
    pub fn card(&self) -> Card {
        let known = self.total > 0;
        // A folder's size is an estimate, so the count can run a little past it.
        let done = if known { self.done.min(self.total) } else { self.done };
        let fraction = if known { done as f64 / self.total as f64 } else { 0.0 };
        let line = if self.cancelling {
            "Stopping…".into()
        } else if self.waiting {
            "Connection lost. Goes on once the server's back".into()
        } else if known {
            format!("{} of {} · {}", bytes(done), bytes(self.total), percent(fraction))
        } else {
            format!("{}… {}", self.kind.verb(), bytes(done))
        };
        Card {
            label: self.label.clone(),
            stop_label: format!("Stop {} {}", self.kind.verb().to_lowercase(), self.label),
            can_stop: !self.cancelling,
            fraction: known.then_some(fraction),
            line,
            rate: (!self.cancelling && !self.waiting && self.rate > 0.0).then(|| rate(self.rate)),
            cleaned: self.cleaning.as_ref().map(cleaning_note).filter(|note| !note.is_empty()),
            waiting: self.waiting,
        }
    }
}

/// Speed is measured over at least this long, so it doesn't jitter.
const SAMPLE_MS: u64 = 250;

/// The transfers in progress, oldest first.
#[derive(Debug, Default)]
pub struct Transfers {
    items: Vec<Transfer>,
    last_id: u64,
}

impl Transfers {
    pub fn new() -> Transfers {
        Transfers::default()
    }

    pub fn items(&self) -> &[Transfer] {
        &self.items
    }

    pub fn get(&self, id: TransferId) -> Option<&Transfer> {
        self.items.iter().find(|t| t.id == id)
    }

    /// A new card, before anything moved. The id goes to the backend with the request.
    pub fn begin(&mut self, kind: Kind, label: impl Into<String>, tab: TabId, server: &str) -> TransferId {
        self.last_id += 1;
        let id = TransferId(self.last_id);
        self.items.push(Transfer { id, tab, server: server.to_owned(), kind, label: label.into(), done: 0, total: 0, rate: 0.0, waiting: false, cancelling: false, cleaning: None, sample: None });
        id
    }

    /// The backend's progress. Speed is what moved over the last quarter second or more,
    /// smoothed (six parts old, four new); while the connection is down there's none, and it's
    /// measured afresh once it's back. Progress for a card that left is dropped.
    pub fn progress(&mut self, id: TransferId, p: &Progress) {
        let Some(t) = self.items.iter_mut().find(|t| t.id == id) else { return };
        if let Some(cleaning) = &p.cleaning {
            t.cleaning = Some(cleaning.clone());
        }
        t.done = p.done;
        t.total = p.total;
        if p.waiting || t.waiting {
            t.sample = Some((p.at_ms, p.done));
            t.waiting = p.waiting;
            t.rate = 0.0;
            return;
        }
        let Some((at, done)) = t.sample else {
            t.sample = Some((p.at_ms, p.done));
            return;
        };
        let ms = p.at_ms.saturating_sub(at);
        if ms >= SAMPLE_MS {
            let recent = p.done.saturating_sub(done) as f64 / (ms as f64 / 1_000.0);
            t.rate = if t.rate > 0.0 { t.rate * 0.6 + recent * 0.4 } else { recent };
            t.sample = Some((p.at_ms, p.done));
        }
    }

    /// Stop was pressed: the card says "Stopping…" until the backend says it ended. False when
    /// there's no such card or it's stopping already (then the backend needn't hear again).
    pub fn stopping(&mut self, id: TransferId) -> bool {
        match self.items.iter_mut().find(|t| t.id == id && !t.cancelling) {
            Some(t) => {
                t.cancelling = true;
                true
            }
            None => false,
        }
    }

    /// The transfer ended, whichever way: its card leaves. Returns it as it stood, for the
    /// note that follows (its label, what cleaning took out).
    pub fn end(&mut self, id: TransferId) -> Option<Transfer> {
        let at = self.items.iter().position(|t| t.id == id)?;
        Some(self.items.remove(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(done: u64, total: u64, at_ms: u64) -> Progress {
        Progress { done, total, at_ms, ..Progress::default() }
    }

    #[test]
    fn labels_for_one_and_for_many() {
        assert_eq!(files_label(&["/home/dev/notes.txt"]), "notes.txt");
        assert_eq!(files_label(&["C:\\Users\\dev\\photo.jpg"]), "photo.jpg");
        assert_eq!(files_label(&["/home/dev/photos/"]), "photos");
        assert_eq!(files_label(&["a", "b", "c"]), "3 items");
        assert_eq!(files_label::<&str>(&[]), "0 items");
        assert_eq!(files_label(&[std::path::PathBuf::from("/tmp/a.png")]), "a.png");
    }

    #[test]
    fn what_cleaning_did_in_a_line() {
        let c = |cleaned, clean, as_is, folders, removed: &[&str]| Cleaning { cleaned, clean, as_is, folders, removed: removed.iter().map(|s| (*s).to_owned()).collect() };
        assert_eq!(cleaning_note(&c(0, 0, 0, 0, &[])), "");
        assert_eq!(cleaning_note(&c(1, 0, 0, 0, &["location", "camera"])), "Removed location, camera");
        assert_eq!(cleaning_note(&c(2, 1, 1, 0, &["location"])), "Removed location · 2 of 4 files");
        assert_eq!(cleaning_note(&c(0, 0, 0, 2, &[])), "Files inside folders go as they are");
        assert_eq!(cleaning_note(&c(0, 3, 0, 0, &[])), "No metadata to remove");
        assert_eq!(cleaning_note(&c(0, 0, 1, 0, &[])), "As is: SSHeesh doesn't clean this kind of file");
        assert_eq!(cleaning_note(&c(0, 1, 1, 0, &[])), "Nothing removed");
    }

    #[test]
    fn a_card_from_start_to_end() {
        let mut list = Transfers::new();
        let id = list.begin(Kind::Download, "backup.tar.gz", TabId(1), "host:web");
        let other = list.begin(Kind::Upload, "3 items", TabId(1), "host:web");
        assert_ne!(id, other);
        // Before the size is known the bar sweeps.
        let card = list.get(id).unwrap().card();
        assert_eq!((card.line.as_str(), card.fraction, card.rate.as_deref()), ("Downloading… 0 B", None, None));
        assert_eq!(card.stop_label, "Stop downloading backup.tar.gz");
        assert!(card.can_stop && !card.waiting && card.cleaned.is_none());

        list.progress(id, &at(0, 4 * 1_048_576, 1_000));
        list.progress(id, &at(1_572_864, 4 * 1_048_576, 2_000));
        let card = list.get(id).unwrap().card();
        assert_eq!(card.line, "1.5 MB of 4.0 MB · 38%");
        assert_eq!(card.fraction, Some(0.375));
        assert_eq!(card.rate.as_deref(), Some("1.5 MB/s"));

        assert!(list.stopping(id));
        assert!(!list.stopping(id), "pressed twice, told once");
        let card = list.get(id).unwrap().card();
        assert_eq!((card.line.as_str(), card.can_stop, card.rate), ("Stopping…", false, None));

        assert_eq!(list.end(id).map(|t| t.label), Some("backup.tar.gz".into()));
        assert_eq!(list.items().len(), 1);
        assert!(list.end(id).is_none());
        // Late progress for a card that left changes nothing.
        list.progress(id, &at(9, 9, 3_000));
        assert!(!list.stopping(id));
        assert_eq!(list.items()[0].id, other);
    }

    #[test]
    fn speed_is_measured_over_a_quarter_second_and_smoothed() {
        let mut list = Transfers::new();
        let id = list.begin(Kind::Upload, "site.zip", TabId(1), "host:web");
        let rate = |list: &Transfers| list.get(id).unwrap().rate.round();
        list.progress(id, &at(0, 10_000_000, 0));
        // Too soon to tell.
        list.progress(id, &at(100_000, 10_000_000, 100));
        assert_eq!(rate(&list), 0.0);
        // 500,000 bytes in half a second since the first measure.
        list.progress(id, &at(500_000, 10_000_000, 500));
        assert_eq!(rate(&list), 1_000_000.0);
        // Then twice as fast: six parts old, four parts new.
        list.progress(id, &at(1_500_000, 10_000_000, 1_000));
        assert_eq!(rate(&list), 1_400_000.0);
        // A count that went back (a file sent again) is no speed, not a negative one.
        list.progress(id, &at(1_000_000, 10_000_000, 2_000));
        assert_eq!(rate(&list), 840_000.0);
    }

    #[test]
    fn a_dropped_connection_shows_no_speed_and_measures_afresh() {
        let mut list = Transfers::new();
        let id = list.begin(Kind::Upload, "site.zip", TabId(1), "host:web");
        list.progress(id, &at(0, 1_000_000, 0));
        list.progress(id, &at(400_000, 1_000_000, 1_000));
        assert!(list.get(id).unwrap().rate > 0.0);
        list.progress(id, &Progress { waiting: true, ..at(400_000, 1_000_000, 1_500) });
        let card = list.get(id).unwrap().card();
        assert_eq!((card.line.as_str(), card.rate, card.waiting), ("Connection lost. Goes on once the server's back", None, true));
        // Back after a minute: the minute isn't counted into the speed.
        list.progress(id, &at(400_000, 1_000_000, 61_500));
        assert!(!list.get(id).unwrap().waiting);
        assert_eq!(list.get(id).unwrap().rate, 0.0);
        list.progress(id, &at(600_000, 1_000_000, 62_000));
        assert_eq!(list.get(id).unwrap().rate, 400_000.0);
    }

    #[test]
    fn a_folders_count_can_run_past_its_estimate_and_cleaning_shows() {
        let mut list = Transfers::new();
        let id = list.begin(Kind::Copy, "photos", TabId(1), "host:web");
        let cleaning = Cleaning { cleaned: 1, removed: vec!["location".into()], ..Cleaning::default() };
        list.progress(id, &Progress { cleaning: Some(cleaning), ..at(1_200, 1_000, 0) });
        let card = list.get(id).unwrap().card();
        assert_eq!(card.line, "1.0 KB of 1.0 KB · 100%");
        assert_eq!(card.fraction, Some(1.0));
        assert_eq!(card.cleaned.as_deref(), Some("Removed location"));
        assert_eq!(card.stop_label, "Stop copying photos");
        // Progress without news of cleaning keeps what was known.
        list.progress(id, &at(1_300, 1_000, 10));
        assert!(list.get(id).unwrap().cleaning.is_some());
        // Nothing to say about cleaning yet: no line.
        let quiet = list.begin(Kind::Upload, "a", TabId(1), "host:web");
        list.progress(quiet, &Progress { cleaning: Some(Cleaning::default()), ..at(0, 0, 0) });
        assert_eq!(list.get(quiet).unwrap().card().cleaned, None);
    }
}
