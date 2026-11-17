//! Toasts and confirmations: the window's notes that leave by themselves, and its questions
//! that wait for an answer (0.10's `Overlays.tsx`). The cards are the kit's (`kit::toast`,
//! `kit::dialog`); what is open, for how long, and what a press does are here.
//!
//! ```text
//! s.toast(Toast::success("Downloaded site.zip").body("In Downloads").action("Show in folder", |s| ..));
//! s.confirm(Confirm::new("Remove web-7731?", "Its tabs close.", "Remove").destructive(), |s, tx| ..);
//! ```
//!
//! - **Toasts** stack in the bottom right corner, over the status bar, under uploads and
//!   downloads in progress (`screens::transfers` draws them in one column). Each stays for
//!   its tone's time (`kit::toast::Tone::stays`), longer while the pointer rests on one, and
//!   at most [`KEPT`] are shown: a new one pushes the oldest out.
//! - **A confirmation** is one at a time, over everything, and takes the keyboard: no shortcut
//!   fires and no key reaches a terminal behind it (`States::any_open`). Return presses the
//!   default button, Esc cancels. Asking again while one is open replaces it (the first
//!   counts as cancelled).
//! - Both are drawn through `Shell::text_shown`: names and addresses in them are dotted in
//!   streamer mode.

use std::rc::Rc;

use rune::{El, Tx};

use super::state::Shell;
use crate::kit::dialog::{self, Look};
use crate::kit::toast::{self, Tone};
use crate::theme::Theme;

/// The most toasts on screen.
pub const KEPT: usize = 4;

/// Seconds a toast stays after the pointer leaves it, at least.
const HELD_AFTER: f64 = 2.0;

/// A toast's number, for taking it away before its time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToastId(pub u64);

/// What a toast's button does.
pub type Act = Rc<dyn Fn(&mut Shell)>;

/// A short note.
#[derive(Clone)]
pub struct Toast {
    pub tone: Tone,
    pub title: String,
    pub body: Option<String>,
    /// The button beside the title, and what it does; the toast goes when it's pressed.
    pub action: Option<(String, Act)>,
}

impl Toast {
    pub fn new(tone: Tone, title: impl Into<String>) -> Toast {
        Toast { tone, title: title.into(), body: None, action: None }
    }

    pub fn info(title: impl Into<String>) -> Toast {
        Toast::new(Tone::Info, title)
    }

    pub fn success(title: impl Into<String>) -> Toast {
        Toast::new(Tone::Success, title)
    }

    pub fn error(title: impl Into<String>) -> Toast {
        Toast::new(Tone::Error, title)
    }

    pub fn body(mut self, body: impl Into<String>) -> Toast {
        self.body = Some(body.into());
        self
    }

    pub fn action(mut self, label: impl Into<String>, act: impl Fn(&mut Shell) + 'static) -> Toast {
        self.action = Some((label.into(), Rc::new(act)));
        self
    }
}

impl std::fmt::Debug for Toast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toast").field("tone", &self.tone).field("title", &self.title).field("body", &self.body).field("action", &self.action.as_ref().map(|(l, _)| l)).finish()
    }
}

/// A toast on screen.
struct Shown {
    id: ToastId,
    toast: Toast,
    /// When it leaves, on the window's clock; set at the first poll after it came.
    until: Option<f64>,
}

/// A question: "Cancel" and the answer asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    /// What it's about, one per line.
    pub lines: Vec<String>,
    /// The button that goes ahead ("Delete").
    pub yes: String,
    /// It takes something away: red words on the button.
    pub destructive: bool,
    /// It can't be undone: Return means Cancel.
    pub safe_default: bool,
}

impl Confirm {
    pub fn new(title: impl Into<String>, body: impl Into<String>, yes: impl Into<String>) -> Confirm {
        Confirm { title: title.into(), body: body.into(), lines: Vec::new(), yes: yes.into(), destructive: false, safe_default: false }
    }

    /// It takes something away; what can't be undone also defaults to Cancel.
    pub fn destructive(mut self, undoable: bool) -> Confirm {
        self.destructive = true;
        self.safe_default = !undoable;
        self
    }

    pub fn lines(mut self, lines: Vec<String>) -> Confirm {
        self.lines = lines;
        self
    }
}

/// What a yes does.
type Answer = Box<dyn FnOnce(&mut Shell, &mut Tx)>;

/// The window's toasts and its open question (in `Shell::screens`).
#[derive(Default)]
pub struct Notes {
    toasts: Vec<Shown>,
    next: u64,
    asked: Option<(Confirm, Answer)>,
    /// The pointer rests on the toasts: none leaves until it goes.
    held: bool,
}

impl Notes {
    /// A question waits for an answer.
    pub fn asking(&self) -> bool {
        self.asked.is_some()
    }

    /// The question open, if any.
    pub fn question(&self) -> Option<&Confirm> {
        self.asked.as_ref().map(|(q, _)| q)
    }

    /// The toasts on screen, oldest first.
    pub fn toasts(&self) -> impl Iterator<Item = (ToastId, &Toast)> {
        self.toasts.iter().map(|s| (s.id, &s.toast))
    }

    /// Their titles, oldest first.
    pub fn titles(&self) -> Vec<&str> {
        self.toasts.iter().map(|s| s.toast.title.as_str()).collect()
    }

    pub(crate) fn close(&mut self) {
        // The question counts as cancelled: what a yes would do is dropped.
        self.asked = None;
    }
}

impl Shell {
    /// Shows a toast; it leaves by itself.
    pub fn toast(&mut self, toast: Toast) -> ToastId {
        let notes = &mut self.screens.notes;
        notes.next += 1;
        let id = ToastId(notes.next);
        notes.toasts.push(Shown { id, toast, until: None });
        if notes.toasts.len() > KEPT {
            notes.toasts.remove(0);
        }
        id
    }

    /// Takes a toast away (its cross, its button, or its news is out of date).
    pub fn dismiss_toast(&mut self, id: ToastId) {
        self.screens.notes.toasts.retain(|s| s.id != id);
    }

    /// Presses a toast's button: what it does, and the toast goes.
    pub fn act_on_toast(&mut self, id: ToastId) {
        let act = self.screens.notes.toasts.iter().find(|s| s.id == id).and_then(|s| s.toast.action.as_ref()).map(|(_, act)| act.clone());
        self.dismiss_toast(id);
        if let Some(act) = act {
            act(self);
        }
    }

    /// Asks `question`; `yes` runs on its answer button, nothing on Cancel or Esc.
    pub fn confirm(&mut self, question: Confirm, yes: impl FnOnce(&mut Shell, &mut Tx) + 'static) {
        self.screens.notes.asked = Some((question, Box::new(yes)));
    }

    /// The question's answer: `true` for its answer button.
    pub fn answer(&mut self, yes: bool, tx: &mut Tx) {
        let Some((_, then)) = self.screens.notes.asked.take() else { return };
        if yes {
            then(self, tx);
        }
        // Keys go back to where they were: the terminal on screen.
        self.settle_in_place(tx);
        tx.instant();
    }
}

/// Every frame (`Shell::poll`): toasts that came get their time, those whose time is up go.
/// True when one went.
pub(crate) fn tick(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    let notes = &mut s.screens.notes;
    if notes.toasts.is_empty() {
        return false;
    }
    let (before, held) = (notes.toasts.len(), notes.held);
    for shown in &mut notes.toasts {
        let stays = shown.toast.tone.stays(shown.toast.action.is_some()).as_secs_f64();
        shown.until = Some(match shown.until {
            None => clock + stays,
            // Under the pointer, and for a moment after it leaves.
            Some(until) if held => until.max(clock + HELD_AFTER),
            Some(until) => until,
        });
    }
    notes.toasts.retain(|s| s.until.is_some_and(|u| u > clock));
    if let Some(next) = notes.toasts.iter().filter_map(|s| s.until).reduce(f64::min) {
        tx.wake_at(next);
    }
    notes.toasts.len() != before
}

/// The toast cards, oldest first, for the corner's column (`screens::transfers`).
pub fn toast_cards(s: &Shell, t: &'static Theme) -> Vec<El> {
    s.screens
        .notes
        .toasts
        .iter()
        .map(|shown| {
            let id = shown.id;
            let title = s.text_shown(&shown.toast.title);
            let body = shown.toast.body.as_deref().map(|b| s.text_shown(b));
            let look = toast::Look { tone: shown.toast.tone, title: &title, body: body.as_deref(), action: shown.toast.action.as_ref().map(|(l, _)| l.as_str()) };
            toast::toast(
                t,
                &format!("toast-{}", id.0),
                &look,
                move |s: &mut Shell, tx: &mut Tx| {
                    s.act_on_toast(id);
                    tx.instant();
                },
                move |s: &mut Shell, tx: &mut Tx| {
                    s.dismiss_toast(id);
                    tx.instant();
                },
            )
            // While the pointer rests on a toast, none leaves: there's time to read and press.
            .on_hover_change(|s: &mut Shell, inside, _| s.screens.notes.held = inside)
        })
        .collect()
}

/// The open question over the window, keyed `dialog` (its buttons `dialog-no`, `dialog-yes`).
pub fn dialog_view(s: &Shell, t: &'static Theme) -> Option<El> {
    let q = s.screens.notes.question()?;
    let look = Look::yes_no(s.text_shown(&q.title), s.text_shown(&q.body), &q.yes, q.destructive, q.safe_default).lines(q.lines.iter().map(|l| s.text_shown(l).into_owned()).collect());
    Some(dialog::dialog(t, "dialog", &look, |s: &mut Shell, tx: &mut Tx, i| s.answer(i == 1, tx)))
}
