//! The Windows installer's window (and the uninstaller's): one small window in the app's own
//! look, a single question, honest progress, and the app opening by itself at the end (plan,
//! Phase 7 item 2). The program decides what's asked and does the work (`app/src/setup`); this
//! only draws it and hands back the answer.
//!
//! ```text
//! ┌────────────────────────────────────────────────┐
//! │  [logo]                                        │
//! │  Install SSHeesh                               │  title
//! │  Servers, terminals and files that don't       │  what it is
//! │  fall over. Version 1.0.0.                     │
//! │  ┌──────────────────────────────────────────┐  │
//! │  │ For you only, no administrator needed    │  │  where it goes
//! │  │ C:\Users\…\AppData\Local\SSHeesh         │  │
//! │  └──────────────────────────────────────────┘  │
//! │  [ ] Also delete my settings…   (uninstall)    │
//! ├────────────────────────────────────────────────┤
//! │                           [Cancel] [Install]   │
//! └────────────────────────────────────────────────┘
//! ```
//!
//! While it works the box gives way to a bar and the step's words; at the end, a line that it's
//! done (and, installing, the app opens and the window closes); on a failure, what went wrong
//! in plain words and Close.

use std::sync::{Arc, Mutex};

use rune::{col, el, row, spacer, text, App, AppSpec, Cx, El, Tx, WindowOptions};

use crate::icons::{icon, logo, Icon};
use crate::kit::push::{check_box, push_button, Push};
use crate::theme::{self, Theme};

/// The window's title.
pub const TITLE: &str = "SSHeesh Setup";
/// Its size: one column of words, the bar, the buttons.
pub const SIZE: (f32, f32) = (480.0, 360.0);
const PAD: f32 = 28.0;
const BAR_WIDTH: f32 = SIZE.0 - 2.0 * PAD;

/// What the window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Before anything: what will happen, and the button that does it.
    Ask {
        title: String,
        /// Under the title: what SSHeesh is, its version.
        about: String,
        /// The box: where it goes, in words, then the folder itself.
        place: Option<(String, String)>,
        /// A check box under the box ("Also delete my settings"), off at first.
        choice: Option<String>,
        /// "Install", "Update", "Remove".
        go: String,
        /// The button takes something away (Remove): red words.
        destructive: bool,
    },
    /// At work: the step's words and how far along, 0 to 1.
    Working { title: String, words: String, at: f32 },
    /// Done: a line, and Close unless the window goes by itself.
    Done { title: String, line: String, closes: bool },
    /// What went wrong, and Close.
    Failed { title: String, line: String },
}

impl Phase {
    fn title(&self) -> &str {
        match self {
            Phase::Ask { title, .. } | Phase::Working { title, .. } | Phase::Done { title, .. } | Phase::Failed { title, .. } => title,
        }
    }
}

/// How the work reports back to the window, from its own thread.
#[derive(Clone)]
pub struct Progress {
    tell: Arc<dyn Fn(Phase) + Send + Sync>,
    quit: Arc<dyn Fn() + Send + Sync>,
}

impl Progress {
    pub fn show(&self, phase: Phase) {
        (self.tell)(phase);
    }

    /// Closes the window (and so ends `run`).
    pub fn close(&self) {
        (self.quit)();
    }

    /// One that shows nothing: the silent installer, and tests.
    pub fn nowhere() -> Progress {
        Progress { tell: Arc::new(|_| {}), quit: Arc::new(|| {}) }
    }
}

/// The work an answer starts: given whether the check box was on, run on a thread of its own.
pub type Work = Box<dyn FnOnce(bool, Progress) + Send>;

struct State {
    phase: Phase,
    checked: bool,
    work: Arc<Mutex<Option<Work>>>,
    progress: Option<Progress>,
    /// Start the work at the first frame, without asking.
    auto: bool,
}

fn blank() -> State {
    State { phase: Phase::Working { title: String::new(), words: String::new(), at: 0.0 }, checked: false, work: Arc::new(Mutex::new(None)), progress: None, auto: false }
}

impl State {
    /// The answer: the work starts on its own thread, at most once.
    fn go(&mut self, tx: &mut Tx) {
        let Some(work) = self.work.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        let Some(progress) = self.progress.clone() else { return };
        let checked = self.checked;
        if let Phase::Ask { title, .. } = &self.phase {
            self.phase = Phase::Working { title: title.clone(), words: "Starting…".into(), at: 0.02 };
        }
        let spawned = std::thread::Builder::new().name("setup".into()).spawn(move || work(checked, progress));
        if let Err(e) = spawned {
            self.phase = Phase::Failed { title: self.phase.title().to_owned(), line: format!("Setup couldn't start its work: {e}") };
        }
        tx.instant();
    }
}

/// Opens the window on `phase` and returns when it closes. With a phase other than
/// [`Phase::Ask`] the work starts at once (an update from 0.10: nothing to ask).
pub fn run(phase: Phase, work: Work) {
    crate::fonts::declare();
    let spec = AppSpec::<State> {
        fonts: crate::fonts::FONTS,
        families: crate::fonts::SYSTEM_FAMILIES,
        min_size: Some(SIZE),
        poll: Some(|s, _, tx| std::mem::take(&mut s.auto) && {
            s.go(tx);
            true
        }),
        ..AppSpec::new(TITLE, SIZE, blank, view)
    };
    let app = App::new(spec).window_options(WindowOptions { resizable: false, ..WindowOptions::default() });
    let (shown, closer) = (app.handle(), app.handle());
    let progress = Progress {
        tell: Arc::new(move |phase| shown.update(move |s: &mut State, tx| {
            s.phase = phase;
            tx.instant();
        })),
        quit: Arc::new(move || closer.quit()),
    };
    let starts_now = !matches!(phase, Phase::Ask { .. });
    let work = Arc::new(Mutex::new(Some(work)));
    app.init(move || State { phase, checked: false, work, progress: Some(progress), auto: starts_now }).run();
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let t = Theme::of(cx.env().system_dark);
    let head = col()
        .w_full()
        .gap(6.0)
        .child(el().pb(10.0).child(logo(44.0)))
        .child(text(s.phase.title()).key("setup-title").font(theme::PAGE_TITLE).color(t.l1).single_line().role("heading").level(1));
    let body = match &s.phase {
        Phase::Ask { about, place, choice, .. } => {
            let mut b = col().w_full().gap(16.0).child(text(about.as_str()).key("setup-about").font(theme::TEXT).color(t.l2on).wrap());
            if let Some((words, folder)) = place {
                b = b.child(
                    col()
                        .key("setup-place")
                        .w_full()
                        .gap(4.0)
                        .padding(12.0)
                        .radius(theme::RADIUS)
                        .fill(t.field())
                        .border(1.0, t.hair)
                        .child(text(words.as_str()).font(theme::TEXT_BOLD).color(t.l1).wrap())
                        .child(text(folder.as_str()).key("setup-folder").font(theme::CODE).color(t.l2).wrap_anywhere()),
                );
            }
            if let Some(label) = choice {
                b = b.child(check_box(t, label, s.checked).key("setup-choice").on_press_tx(|s: &mut State, tx| {
                    s.checked = !s.checked;
                    tx.instant();
                }));
            }
            b
        }
        Phase::Working { words, at, .. } => col().w_full().gap(10.0).pt(8.0).child(bar(t, *at)).child(text(words.as_str()).key("setup-step").font(theme::TEXT).color(t.l2on).single_line()).role("status").label(words.as_str()),
        Phase::Done { line, .. } => row()
            .w_full()
            .gap(10.0)
            .pt(8.0)
            .items_start()
            .child(el().size(20.0, 20.0).radius(10.0).fill(t.green).items_center().justify_center().child(icon(Icon::Check, 14.0, t.on_color)))
            .child(text(line.as_str()).key("setup-done").font(theme::TEXT).color(t.l1).wrap().grow(1.0).shrink().min_w(0.0)),
        Phase::Failed { line, .. } => row()
            .w_full()
            .gap(10.0)
            .pt(8.0)
            .items_start()
            .child(icon(Icon::Warning, 18.0, t.red))
            .child(text(line.as_str()).key("setup-failed").font(theme::TEXT).color(t.l1).wrap().grow(1.0).shrink().min_w(0.0)),
    };
    col()
        .w_full()
        .h_full()
        .fill(t.page)
        .child(col().w_full().grow(1.0).px(PAD).pt(PAD).gap(14.0).child(head).child(body))
        .child(foot(s, t))
}

/// The bar: a quiet track, the part done in blue.
fn bar(t: &Theme, at: f32) -> El {
    let done = (at.clamp(0.0, 1.0) * BAR_WIDTH).max(6.0);
    el().key("setup-bar")
        .w(BAR_WIDTH)
        .h(6.0)
        .radius(3.0)
        .fill(t.hov2)
        .role("progressbar")
        .label(format!("{}%", (at.clamp(0.0, 1.0) * 100.0).round()))
        .child(el().w(done).h(6.0).radius(3.0).fill(t.blue))
}

fn foot(s: &State, t: &Theme) -> El {
    let mut foot = row().w_full().px(20.0).py(14.0).gap(8.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], t.hair).child(spacer());
    match &s.phase {
        Phase::Ask { go, destructive, .. } => {
            foot = foot
                .child(push_button(t, "Cancel", Push::Quiet, true).key("setup-cancel").on_press_tx(|_: &mut State, tx| tx.quit()))
                .child(push_button(t, go, if *destructive { Push::Destructive } else { Push::Primary }, true).key("setup-go").default_button().on_press_tx(|s: &mut State, tx| s.go(tx)));
        }
        Phase::Working { .. } | Phase::Done { closes: true, .. } => {
            // Nothing to press: a step can't be stopped half way. The row keeps its height.
            foot = foot.child(el().h(theme::FIELD));
        }
        Phase::Done { closes: false, .. } | Phase::Failed { .. } => {
            foot = foot.child(push_button(t, "Close", Push::Primary, true).key("setup-close").default_button().on_press_tx(|_: &mut State, tx| tx.quit()));
        }
    }
    foot
}

/// Shows each phase without doing anything (`--setup-demo <phase>`): for looking at the
/// window, and its snapshots.
pub fn demo(which: &str) -> Option<Phase> {
    let title = "Install SSHeesh".to_owned();
    let about = "Servers, terminals and files that don't fall over. Version 1.0.0.".to_owned();
    Some(match which {
        "ask" => Phase::Ask { title, about, place: Some(("For you only, no administrator needed".into(), r"C:\Users\you\AppData\Local\SSHeesh".into())), choice: None, go: "Install".into(), destructive: false },
        "update" => Phase::Ask {
            title: "Update SSHeesh".into(),
            about: "SSHeesh 0.10.14 is here. Version 1.0.0 takes its place; your servers, passwords and settings stay.".into(),
            place: Some(("Where you installed it".into(), r"D:\Apps\SSHeesh".into())),
            choice: None,
            go: "Update".into(),
            destructive: false,
        },
        "working" => Phase::Working { title: "Updating SSHeesh".into(), words: "Keeping the version you have, to go back to…".into(), at: 0.45 },
        "done" => Phase::Done { title: "SSHeesh is installed".into(), line: "It's in the Start menu. Opening it now…".into(), closes: true },
        "failed" => Phase::Failed { title, line: r"SSHeesh couldn't be copied to C:\Users\you\AppData\Local\SSHeesh: the disk is full.".into() },
        "uninstall" => Phase::Ask {
            title: "Remove SSHeesh".into(),
            about: "SSHeesh 1.0.0 goes from this computer: the program, its Start menu item and its entry in Installed apps.".into(),
            place: Some(("Installed in".into(), r"C:\Users\you\AppData\Local\SSHeesh".into())),
            choice: Some("Also delete my settings, servers and saved tabs".into()),
            go: "Remove".into(),
            destructive: true,
        },
        _ => return None,
    })
}
