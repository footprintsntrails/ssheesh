//! The security check (0.10's `CheckupDialog` in `ServerTools.tsx`): opened from the
//! sidebar's shield and the palette, it reads the server and lists what it found, worst
//! first, each in plain words. A finding may offer a fix: its command is shown in full, it
//! runs only after "Run" and a yes, and on a Production server only after a second yes that
//! says so. A fix that needs you (sudo's password) opens in a terminal, with what to answer.
//!
//! ```text
//! check-scrim   dims the window; the dialog is modal (Esc closes it)
//! └── check-dialog  check-again, check-close · check-summary · check-root
//!     ├── check-pick-<i>          no server on screen: which one to check
//!     └── find-<i>  a finding: its fix (fix-<i>), fix-run-<i>, fix-copy-<i>, recipe-<i>,
//!                   the questions fix-yes-<i> / fix-no-<i>, then what it printed (fix-out-<i>)
//! ```
//!
//! The rules (order, summary, the two yeses) are `sshee_app::recipes::checkup`; the check and
//! the fix run in the backend (`Backend::recipes`), and come back through
//! [`crate::shell::Remote::recipes`] ([`deliver`]). Names, details, commands and output are
//! drawn through `Shell::server_shown` and `Shell::text_shown` (streamer mode).

use std::collections::HashMap;

use rune::{col, el, row, text, El, ScrollSpec, Tx};
use sshee_app::recipes::checkup::{self, Finding, Gate, Report, Verdict};
use sshee_app::recipes::{Ask, Outcome, Reply};
use sshee_app::workbench::{Open, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::chip::chip;
use crate::kit::list::{list_row, ListRow};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The dialog's width.
pub const WIDTH: f32 = 600.0;

/// A fix, once Run was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Waiting for the yes (or the second yes, on Production), with the command exactly as
    /// it was shown when Run was pressed: only that one ever runs.
    Asking(Gate, String),
    Running,
    Ran(Outcome),
}

/// The check's own state.
#[derive(Default)]
pub struct State {
    /// Open, for this server (`Some("")`: open, a server still to pick).
    pub open: Option<String>,
    pub loading: bool,
    /// The last check of the server, kept while it checks again.
    pub report: Option<Report>,
    pub error: Option<String>,
    /// Fixes run or asked about, by finding id.
    pub runs: HashMap<String, Run>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

impl Shell {
    /// The sidebar's shield: the server on screen, or a list to pick one from.
    pub fn open_checkup_here(&mut self, tx: &mut Tx) {
        let server = self.bench.current_server().filter(|s| !s.is_local()).map(|s| s.key()).unwrap_or_default();
        self.open_checkup(&server, tx);
    }

    /// Opens the check for `server` (empty: pick one) and starts it.
    pub fn open_checkup(&mut self, server: &str, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.checkup = State { open: Some(server.to_owned()), ..State::default() };
        if !server.is_empty() {
            self.checkup_again();
        }
        tx.instant();
    }

    pub fn close_checkup(&mut self, tx: &mut Tx) {
        self.screens.checkup = State::default();
        self.settle(tx);
    }

    /// "Check again", and the first look.
    pub fn checkup_again(&mut self) {
        let Some(server) = self.screens.checkup.open.clone().filter(|s| !s.is_empty()) else { return };
        let state = &mut self.screens.checkup;
        state.loading = true;
        state.error = None;
        self.backend_mut().recipes(Ask::Check { server });
    }

    /// The fix a finding of the last check offers now.
    fn checkup_fix(&self, id: &str) -> Option<String> {
        self.screens.checkup.report.as_ref()?.findings.iter().find(|f| f.id == id)?.fix.clone()
    }

    /// The finding's Run: asks first, about the command on screen now.
    /// Run…: the first question, under the `i`th card, with the keyboard on its Cancel.
    fn checkup_run(&mut self, id: &str, i: usize, tx: &mut Tx) {
        let Some(command) = self.checkup_fix(id) else { return };
        self.screens.checkup.runs.insert(id.to_owned(), Run::Asking(Gate::Confirm, command));
        tx.focus(format!("fix-no-{i}"), true);
    }

    /// A yes to the question: the next one (Production), or the fix runs now. What runs is
    /// the command you were asked about; if a check that came back meanwhile offers another
    /// one, nothing runs, and the new command is shown to be run (and asked about) afresh.
    fn checkup_yes(&mut self, id: &str, i: usize, tx: &mut Tx) {
        let state = &self.screens.checkup;
        let (Some(server), Some(Run::Asking(gate, command))) = (state.open.clone(), state.runs.get(id).cloned()) else { return };
        let production = self.bench.server(&server).is_some_and(|s| s.is_prod());
        match checkup::after_yes(gate, production) {
            Some(next) => {
                self.screens.checkup.runs.insert(id.to_owned(), Run::Asking(next, command));
                // The second question starts on Cancel, not where the first one's yes was.
                tx.focus(format!("fix-no-{i}"), true);
            }
            None => {
                if self.checkup_fix(id).as_deref() != Some(command.as_str()) {
                    self.screens.checkup.runs.remove(id);
                    return;
                }
                self.screens.checkup.runs.insert(id.to_owned(), Run::Running);
                self.backend_mut().recipes(Ask::Fix { server, check: id.to_owned(), command });
            }
        }
    }

    /// Cancel: the question goes, and the keyboard goes back to the Run… it came from.
    fn checkup_no(&mut self, id: &str, i: usize, tx: &mut Tx) {
        self.screens.checkup.runs.remove(id);
        tx.focus(format!("fix-run-{i}"), true);
    }

    /// A fix that needs you: a new terminal on the server, the command typed and run there.
    fn checkup_in_terminal(&mut self, id: &str, tx: &mut Tx) {
        let state = &self.screens.checkup;
        let Some(server) = state.open.clone() else { return };
        let Some(command) = state.report.as_ref().and_then(|r| r.findings.iter().find(|f| f.id == id)).and_then(|f| f.fix.clone()) else { return };
        self.screens.checkup = State::default();
        self.run_in_new_terminal(&server, &command, true, tx);
    }

    /// A new terminal on `server` with `command` typed into it once it's connected (and run,
    /// with `enter`): for what needs you to answer.
    pub fn run_in_new_terminal(&mut self, server: &str, command: &str, enter: bool, tx: &mut Tx) {
        let id = self.bench.open_with(TabKind::Terminal, server, Open::default());
        self.settle(tx);
        self.screens.palette.pending.insert(id, (command.to_owned(), enter));
    }
}

/// The backend's answer, on the UI thread.
pub fn deliver(s: &mut Shell, reply: Reply) {
    match reply {
        Reply::Checked { server, result } => {
            let state = &mut s.screens.checkup;
            if state.open.as_deref() != Some(server.as_str()) {
                return;
            }
            state.loading = false;
            match result {
                // What a fix printed stays under its finding, as in 0.10.
                Ok(report) => state.report = Some(report),
                Err(why) => state.error = Some(why),
            }
        }
        Reply::Fixed { server, check, outcome } => {
            let state = &mut s.screens.checkup;
            if state.open.as_deref() != Some(server.as_str()) {
                return;
            }
            let worked = matches!(outcome, Outcome::Done { .. });
            state.runs.insert(check, Run::Ran(outcome));
            // It changed the server: look again (0.10's onRan).
            if worked {
                s.checkup_again();
            }
        }
        other => super::recipes::deliver(s, other),
    }
}

/// The icon and colour of a verdict.
fn verdict_look(t: &Theme, v: Verdict) -> (Icon, rune::Color) {
    match v {
        Verdict::Bad => (Icon::Warning, t.red),
        Verdict::Warn => (Icon::Warning, t.orange),
        Verdict::Unknown => (Icon::Info, t.l2),
        Verdict::Good => (Icon::Check, t.green),
    }
}

/// A dialog's header: its sign in a soft circle, the title and a line under it, then the
/// buttons at its end.
pub(crate) fn header(t: &Theme, glyph: Icon, title: &str, sub: &str, end: Vec<El>) -> El {
    let sign = el().size(32.0, 32.0).radius(16.0).fill(t.blue.alpha(0.14)).items_center().justify_center().child(icon(glyph, theme::ICON, t.blue));
    let words = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(text(title).font(theme::TITLE).color(t.l1).single_line().truncate().role("heading").level(1))
        .child(text(sub).font(theme::NOTE).color(t.l2).single_line().truncate());
    row().w_full().gap(12.0).items_center().child(sign).child(words).children(end)
}

/// The close button at a header's end.
pub(crate) fn close_button(t: &Theme, key: &'static str) -> El {
    toolbar_button(t, key, Icon::Close, "Close")
}

/// A command shown in full, as it will run: the terminal's font, selectable.
pub(crate) fn command_block(t: &Theme, command: &str) -> El {
    el().w_full()
        .px(10.0)
        .py(8.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.hov2)
        .border(1.0, t.hair2)
        .child(text(command).font(theme::CODE).color(t.l1).wrap().selectable(true).w_full())
}

/// A quiet line in a soft box; red when it says what went wrong.
pub(crate) fn note(t: &Theme, words: &str, bad: bool) -> El {
    let (fill, ink) = if bad { (t.red.alpha(0.1), t.red) } else { (t.hov2, t.l2on) };
    row().w_full().px(12.0).py(8.0).radius(theme::RADIUS_ROW).fill(fill).child(text(words).font(theme::NOTE).color(ink).wrap().grow(1.0).shrink().min_w(0.0))
}

/// Text that reads as a link: blue, and a button.
pub(crate) fn link(t: &Theme, words: &str) -> El {
    text(words).font(theme::CONTROL_TEXT).color(t.blue).single_line().role("button").label(words).focusable(true)
}

/// The space a dialog keeps from the window's top and bottom edges.
const DIALOG_MARGIN: f32 = 24.0;

/// The scrim and the dialog in the middle of the window, at most 680 points tall and never
/// taller than the window less a margin (its body scrolls instead). Esc runs `close`.
pub(crate) fn dialog(t: &Theme, scrim_key: &'static str, key: &'static str, label: &'static str, width: f32, content: El, close: impl Fn(&mut Shell, &mut Tx) + 'static) -> El {
    let surface = popover(t, width).key(key).label_static(label).max_h(680.0).shrink().min_h(0.0).modal_tx(false, close).child(content);
    col().key(scrim_key).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).py(DIALOG_MARGIN).fill(t.scrim()).items_center().justify_center().child(surface)
}

/// A dialog's body: padded, and scrolling when it's long.
pub(crate) fn body(key: &'static str) -> El {
    col().key(key).w_full().shrink().min_h(0.0).px(20.0).pt(18.0).pb(20.0).gap(14.0).scroll(ScrollSpec { stick_bottom: false })
}

/// A dialog's content: `head` (its title and close button) stays at the top while `body` (a
/// [`body`], whose gap is `gap`) scrolls under it. A line under the head marks where the body
/// goes under it, and the body ends with room under its last card: a scroll's own bottom
/// padding isn't scrolled to.
pub(crate) fn headed(t: &Theme, head: El, body: El, gap: f32) -> El {
    col()
        .w_full()
        .shrink()
        .min_h(0.0)
        .child(col().w_full().px(20.0).pt(18.0).pb(14.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair).child(head))
        .child(body.pt(gap).child(el().key("dialog-end").w_full().h(6.0).a11y_hidden()))
}

/// The question a fix asks before it runs: inside its card, so the command it is about stays
/// in sight above it.
fn question(t: &Theme, server_name: &str, gate: Gate, i: usize, id: &str) -> El {
    // Each question's yes has its own key, so the keyboard on the first one's yes doesn't stay
    // on the second's.
    let (title, line, yes, kind, yes_key) = match gate {
        Gate::Confirm => (checkup::confirm_title(server_name), None, "Run", Push::Primary, format!("fix-yes-{i}")),
        Gate::Production => (checkup::production_title(server_name), Some(checkup::PRODUCTION_LINE), "Run on Production", Push::Destructive, format!("fix-prod-{i}")),
    };
    let production = gate == Gate::Production;
    let (yes_id, no_id) = (id.to_owned(), id.to_owned());
    let mut words = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(title.clone()).font(theme::TEXT_BOLD).color(if production { t.red } else { t.l1 }).wrap());
    if let Some(line) = line {
        words = words.child(text(line).font(theme::NOTE).color(t.l2on).wrap());
    }
    let mut yes_button = push_button(t, yes, kind, true).key(yes_key).on_press_tx(move |s: &mut Shell, tx| s.checkup_yes(&yes_id, i, tx));
    if !production {
        yes_button = yes_button.default_button();
    }
    row()
        .key(format!("fix-ask-{i}"))
        .w_full()
        .mt(8.0)
        .px(12.0)
        .py(10.0)
        .gap(10.0)
        .items_center()
        .radius(theme::RADIUS_ROW)
        .fill(if production { t.red.alpha(0.08) } else { t.blue.alpha(0.07) })
        .border(1.0, if production { t.red.alpha(0.35) } else { t.blue.alpha(0.3) })
        .role("alertdialog")
        .label(title)
        .child(words)
        .child(push_button(t, "Cancel", Push::Quiet, true).key(format!("fix-no-{i}")).on_press_tx(move |s: &mut Shell, tx| s.checkup_no(&no_id, i, tx)))
        .child(yes_button)
}

/// What a fix printed, and how it went.
fn outcome(t: &Theme, s: &Shell, outcome: &Outcome, i: usize, id: &str) -> El {
    let mut out = col().key(format!("fix-out-{i}")).w_full().mt(8.0).gap(6.0);
    let (glyph, ink) = match outcome {
        Outcome::Done { .. } => (Icon::Check, t.green),
        Outcome::NeedsYou { .. } => (Icon::Terminal, t.orange),
        Outcome::Failed { .. } => (Icon::Warning, t.red),
    };
    out = out.child(row().gap(6.0).items_center().role("status").child(icon(glyph, 14.0, ink)).child(text(outcome.headline()).font(theme::TAB_NAME).color(ink).single_line()));
    match outcome {
        Outcome::Done { output } | Outcome::Failed { output, .. } if !output.is_empty() => {
            out = out.child(
                el().w_full().max_h(140.0).px(10.0).py(8.0).radius(theme::RADIUS_ROW).fill(t.hov2).scroll(ScrollSpec { stick_bottom: true }).child(
                    text(s.text_shown(output).into_owned()).font(theme::CODE).color(t.l2on).wrap().selectable(true).w_full(),
                ),
            );
        }
        Outcome::NeedsYou { ask } => {
            let id = id.to_owned();
            out = out.child(text(ask.as_str()).font(theme::NOTE).color(t.l2on).wrap()).child(
                row().child(push_button(t, "Open a terminal for it", Push::Primary, true).key(format!("fix-terminal-{i}")).on_press_tx(move |s: &mut Shell, tx| s.checkup_in_terminal(&id, tx))),
            );
        }
        _ => {}
    }
    out
}

/// One finding's card.
fn card(t: &Theme, s: &Shell, server: &str, server_name: &str, f: &Finding, i: usize) -> El {
    let (glyph, ink) = verdict_look(t, f.verdict);
    let mut words = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(3.0)
        .child(text(f.title.as_str()).font(theme::TEXT_BOLD).color(t.l1).wrap().label(format!("{}: {}", f.verdict.word(), f.title)))
        .child(text(s.text_shown(&f.detail).into_owned()).font(theme::NOTE).color(t.l2on).wrap().selectable(true));
    let run = s.screens.checkup.runs.get(&f.id);
    if f.offers_fix() {
        let command = f.fix.clone().unwrap_or_default();
        words = words.child(el().h(4.0)).child(command_block(t, &s.text_shown(&command)).key(format!("fix-{i}")));
        if let Some(why) = &f.fix_note {
            words = words.child(text(why.as_str()).font(theme::SMALL).color(t.l2).wrap());
        }
        let running = run == Some(&Run::Running);
        let id = f.id.clone();
        let copy = command.clone();
        let mut buttons = row().mt(4.0).gap(8.0).items_center();
        if !matches!(run, Some(Run::Asking(..))) {
            buttons = buttons.child(
                push_button(t, if running { "Running…" } else { "Run…" }, Push::Quiet, !running && !s.screens.checkup.loading)
                    .key(format!("fix-run-{i}"))
                    .help(format!("Asks first, then runs it on {server_name}"))
                    .on_press_tx(move |s: &mut Shell, tx| s.checkup_run(&id, i, tx)),
            );
        }
        buttons = buttons.child(push_button(t, "Copy", Push::Quiet, true).key(format!("fix-copy-{i}")).on_press_tx(move |_: &mut Shell, tx| tx.copy(copy.clone())));
        words = words.child(buttons);
        if let Some(Run::Asking(gate, _)) = run {
            words = words.child(question(t, server_name, *gate, i, &f.id));
        }
    } else if let Some(why) = f.fix_note.as_ref().filter(|_| f.verdict != Verdict::Good) {
        words = words.child(text(why.as_str()).font(theme::SMALL).color(t.l2).wrap());
    }
    if let Some(Run::Ran(done)) = run {
        words = words.child(outcome(t, s, done, i, &f.id));
    }
    if let Some(recipe) = f.recipe() {
        let server = server.to_owned();
        words = words.child(
            link(t, "Open the recipe").key(format!("recipe-{i}")).mt(4.0).on_press_tx(move |s: &mut Shell, tx| s.open_recipes(&server, Some(recipe), tx)),
        );
    }
    row()
        .key(format!("find-{i}"))
        .w_full()
        .px(14.0)
        .py(12.0)
        .gap(10.0)
        .items_start()
        .radius(theme::RADIUS_GROUP)
        .fill(t.group)
        .border(1.0, t.hair2)
        .child(icon(glyph, theme::ICON, ink).mt(1.0))
        .child(words)
}

/// Which server to check, when none was on screen.
fn picker(t: &Theme, s: &Shell) -> El {
    let mut list = col().key("check-pick").w_full().mx(-8.0).gap(2.0);
    let servers: Vec<_> = s.bench.servers().iter().filter(|sv| !sv.is_local()).collect();
    if servers.is_empty() {
        return note(t, "Add a server first: the check reads a server over SSH, and this computer isn't one.", false);
    }
    for (i, server) in servers.into_iter().enumerate() {
        let key = server.key();
        let name = s.server_shown(&key, &server.name).into_owned();
        let address = if s.streaming() { String::new() } else { server.address.clone() };
        let spec = ListRow { icon: Some(Icon::for_server(&server.icon)), label: &name, note: &address, prod: server.is_prod(), ..ListRow::default() };
        list = list.child(list_row(t, &spec).key(format!("check-pick-{i}")).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, tx| s.open_checkup(&key, tx)));
    }
    col().w_full().gap(8.0).child(text("Which server?").font(theme::GROUP_TITLE).color(t.l2).single_line()).child(list)
}

/// The check, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.checkup;
    let server = state.open.as_deref()?;
    let found = s.bench.server(server);
    let server_name = found.map(|sv| s.server_shown(server, &sv.name).into_owned()).unwrap_or_default();
    let prod = found.is_some_and(|sv| sv.is_prod());

    let mut end = Vec::new();
    if !server.is_empty() {
        end.push(
            push_button(t, if state.loading { "Checking…" } else { "Check again" }, Push::Quiet, !state.loading)
                .key("check-again")
                .on_press_tx(|s: &mut Shell, _| s.checkup_again()),
        );
    }
    end.push(close_button(t, "check-close").on_press_tx(|s: &mut Shell, tx| s.close_checkup(tx)));
    let sub = if server.is_empty() { "Pick a server: nothing changes until you run a fix".to_owned() } else { format!("{server_name} · {}", checkup::PROMISE) };
    let mut head = header(t, Icon::Shield, "Security check", &sub, end);
    if prod {
        head = col().w_full().gap(8.0).child(head).child(row().ml(44.0).gap(6.0).items_center().child(chip(t, "Prod", t.red)).child(text("Fixes here ask twice.").font(theme::SMALL).color(t.l2).single_line()));
    }
    let mut page = body("check-body");

    if server.is_empty() {
        page = page.child(picker(t, s));
    } else {
        if state.loading && state.report.is_none() {
            page = page.child(row().key("check-waiting").w_full().py(24.0).gap(8.0).items_center().role("status").child(crate::kit::progress_ring(14.0, 1.6, t.l2)).child(text(checkup::CHECKING).font(theme::NOTE).color(t.l2).single_line()));
        }
        if let Some(why) = &state.error {
            page = page.child(note(t, &s.text_shown(why), true).key("check-error").role("alert"));
        }
        if let Some(report) = &state.report {
            let mut summary = row().key("check-summary").gap(14.0).items_center();
            for (verdict, words) in report.summary() {
                let ink = match verdict {
                    Verdict::Bad => t.red,
                    Verdict::Warn => t.orange,
                    Verdict::Good => t.green,
                    Verdict::Unknown => t.l2,
                };
                summary = summary.child(text(words).font(theme::TAB_NAME).color(ink).single_line());
            }
            page = page.child(summary);
            if !report.as_root {
                page = page.child(note(t, checkup::NEEDS_ROOT, false).key("check-root"));
            }
            let mut cards = col().key("check-list").w_full().gap(8.0);
            if state.loading {
                cards = cards.opacity(0.5);
            }
            for (i, f) in report.sorted().into_iter().enumerate() {
                cards = cards.child(card(t, s, server, &server_name, f, i));
            }
            page = page.child(cards);
        }
    }
    Some(dialog(t, "check-scrim", "check-dialog", "Security check", WIDTH, headed(t, head, page, 14.0), |s: &mut Shell, tx| s.close_checkup(tx)))
}
