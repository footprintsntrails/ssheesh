//! What sits over a terminal's text, and the state behind it: the find bar, the scroll bar,
//! "jump to the bottom", a link's hint, the pane's menu and the "While you were away" line.
//!
//! ```text
//! term-box   column: the pane's whole area
//! ├── term           the grid, cursor and input method (`view.rs`); the away line sits between
//! │                  its rows
//! ├── scroll         the scroll bar, over the right edge
//! ├── to-bottom      "Jump to the bottom", while scrolled up with new output below
//! ├── link-hint      what a click on the link under the pointer does
//! ├── find           the find bar, top right
//! └── menu           the pane's menu, where it was asked for
//! ```
//!
//! The overlays are siblings of the terminal, not children: a press on the scroll bar never
//! starts a selection, and a key typed into the find bar never reaches the program.
//!
//! Anything that takes a while (reading tmux's history over ssh, searching ten thousand rows)
//! runs on a thread of its own and comes back through the pane's [`Postbox`], which wakes the
//! window; `TermPane::poll` takes it from there. A replayed session (a demo, a test) does the
//! same work at once, on the pane's thread, so every run draws the same frames.

use std::sync::{Arc, Mutex};

use rune::{Cx, El, Tx};
use sshee_term::engine::TermEvent;
use sshee_term::find::{FindMatch, FindRow};
use sshee_term::osc::OscEvent;
use sshee_term::scroll::TmuxScroll;

use super::metrics::CellMetrics;
use super::pane::TermPane;
use super::wake::Coalesce;
use super::{away, find, links, menu, scrollbar};
use crate::theme::Theme;

/// What a pane asks its session's server for ([`super::TermLink::ask`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// tmux's rows, history first, for a find.
    Rows,
    /// The last lines of tmux's history, joined where tmux wrapped them, for "while you were
    /// away".
    History { lines: u32 },
    /// Scroll tmux so `position` lines of history sit below the screen (`None`: only say where
    /// it is).
    Scroll { position: Option<u32> },
}

/// The server's answer to an [`Ask`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Rows(String),
    /// `away_ms`: how long the connection was down, when the link knows better than the
    /// pane's clock (a replay, whose clock runs for seconds).
    History { text: String, away_ms: Option<u64> },
    Scroll(TmuxScroll),
}

/// Where a link puts its answer, from any thread. `None`: it couldn't be had.
pub type Answer = Box<dyn FnOnce(Option<Reply>) + Send>;

/// What background work brings back to the pane.
pub(crate) enum Note {
    /// The rows a find searches, made safe to search (`Look::safe_rows`), for find `epoch`.
    FindRows { epoch: u64, rows: Option<Vec<FindRow>> },
    /// The matches of search `ticket`.
    Found { ticket: u64, matches: Vec<FindMatch> },
    /// tmux said where it is scrolled to.
    Scrolled(Option<TmuxScroll>),
    /// tmux's history after a reconnect.
    History(Option<(String, Option<u64>)>),
}

/// Notes from other threads, waiting for the pane's next poll.
#[derive(Default)]
pub(crate) struct Postbox {
    notes: Mutex<Vec<Note>>,
    wake: Mutex<Option<Arc<Coalesce>>>,
}

impl Postbox {
    pub(crate) fn set_waker(&self, wake: Arc<Coalesce>) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
    }

    /// From any thread: leaves `note` and wakes the window.
    pub(crate) fn post(&self, note: Note) {
        self.notes.lock().unwrap_or_else(|e| e.into_inner()).push(note);
        let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = wake {
            // Past the output flag: a note isn't output, and must not wait for any.
            wake.wake_now();
        }
    }

    fn take(&self) -> Vec<Note> {
        std::mem::take(&mut *self.notes.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// What a pane asks of the window it is in, which the window takes after each poll
/// (`TermPane::take_wishes`): things only the window can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wish {
    /// "New terminal here": another terminal on this pane's machine.
    NewTerminal,
    /// A click on a file's path: open it in Files (`line`: where in the file). The window
    /// copies the path until Files takes this.
    OpenPath { path: String, line: Option<u32> },
    /// The shell is in this folder now (OSC 7, or its title): the tab's name.
    Folder(String),
    /// The pane was pressed: side by side, it becomes the tab you're on.
    Pressed,
}

/// A pane's overlays' state.
#[derive(Default)]
pub struct Overlays {
    pub(crate) find: find::Find,
    pub(crate) links: links::Links,
    pub(crate) away: away::Away,
    pub(crate) menu: menu::Menu,
    pub(crate) scroll: scrollbar::Scroll,
    pub(crate) post: Arc<Postbox>,
    wishes: Vec<Wish>,
    /// The program in the foreground, when something tells (the agents' watch will):
    /// who gets Ctrl+F depends on it.
    pub(crate) foreground: Option<String>,
    /// The key of the element that gives this pane the keyboard (see [`TermPane::set_focus_key`]).
    home: String,
}

impl Overlays {
    pub(crate) fn wish(&mut self, wish: Wish) {
        self.wishes.push(wish);
    }

    /// Runs `job` off the UI thread; at once for a replayed session (`inline`), whose frames
    /// must not depend on how fast a thread was.
    pub(crate) fn work(inline: bool, job: impl FnOnce() + Send + 'static) {
        if inline {
            job();
        } else if let Err(e) = std::thread::Builder::new().name("pane-work".into()).spawn(job) {
            // No thread to be had: the search just doesn't finish, and the bar says so.
            eprintln!("ssheesh: a terminal's background work didn't start: {e}");
        }
    }
}

impl TermPane {
    /// The key that gives this pane's terminal the keyboard with `Tx::focus`: the terminal
    /// itself where there is one pane ("term", the default), or an element around it with a
    /// key of its own where a window shows two.
    pub fn set_focus_key(&mut self, key: String) {
        self.over.home = key;
    }

    /// Gives the keyboard back to this pane's terminal.
    pub(crate) fn focus_terminal(&self, tx: &mut Tx) {
        tx.focus(if self.over.home.is_empty() { "term" } else { self.over.home.as_str() }, false);
    }

    /// What the pane wants of its window since this was last called.
    pub fn take_wishes(&mut self) -> Vec<Wish> {
        std::mem::take(&mut self.over.wishes)
    }

    /// The program in the foreground of this terminal (`vim`, `less`), or `None` when nobody
    /// knows: decides who gets Ctrl+F.
    pub fn set_foreground(&mut self, command: Option<String>) {
        self.over.foreground = command;
    }

    /// New settings for what the terminal may show (streamer mode). What is on screen
    /// changes at once; a find looks again, in rows made safe under the new settings.
    pub fn set_look(&mut self, look: sshee_term::present::Look, tx: &mut Tx) {
        self.view.borrow_mut().set_look(look);
        self.links_left();
        self.find_look_changed(tx);
        tx.instant();
    }

    /// Asks the session's server; false when the history is the terminal's own.
    pub(crate) fn ask(&mut self, ask: Ask, note: impl FnOnce(Option<Reply>) -> Note + Send + 'static) -> bool {
        let post = self.over.post.clone();
        self.link.ask(ask, Box::new(move |reply| post.post(note(reply))))
    }

    /// The terminal's events the overlays care about: the shell's folder.
    pub(crate) fn heard(&mut self, event: &TermEvent) {
        match event {
            TermEvent::Osc(OscEvent::Cwd(path)) => {
                self.over.links.cwd = Some(path.clone());
                self.over.wish(Wish::Folder(path.clone()));
            }
            TermEvent::Title(Some(title)) => {
                if let Some(folder) = sshee_app::workbench::titles::folder_in_title(title) {
                    self.over.wish(Wish::Folder(folder.to_owned()));
                }
            }
            _ => {}
        }
    }

    /// The overlays' part of every poll: what came back from other threads, and timers.
    pub(crate) fn poll_overlays(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let mut changed = false;
        for note in self.over.post.take() {
            changed = true;
            match note {
                Note::FindRows { epoch, rows } => self.find_rows_came(epoch, rows, tx),
                Note::Found { ticket, matches } => self.find_matches_came(ticket, matches, tx),
                Note::Scrolled(report) => self.tmux_answered(report, tx),
                Note::History(history) => self.history_came(history),
            }
        }
        changed |= self.poll_away(clock, tx);
        changed |= self.poll_links(clock, tx);
        changed |= self.poll_scroll();
        changed
    }
}

/// The pane's area with its overlays: `term` (from `view.rs`) and everything over it.
pub fn layer<S: 'static>(term: El, pane: &TermPane, cx: &mut Cx<S>, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static, m: &CellMetrics) -> El {
    let t = Theme::of(cx.env().system_dark);
    let mut layer = rune::col().key("term-box").w_full().grow(1.0).min_h(0.0).child(term);
    layer = layer.children(scrollbar::bar(pane, t, get));
    layer = layer.children(scrollbar::to_bottom(pane, t, get));
    layer = layer.children(links::hint(pane, t, m));
    layer = layer.children(find::bar(pane, t, get));
    layer.children(menu::view(pane, t, get))
}
