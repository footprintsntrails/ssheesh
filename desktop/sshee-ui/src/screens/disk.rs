//! "Free up disk space" (0.10's `DiskDialog` in `ServerTools.tsx`): opened from the sidebar's
//! drive, the top row's, the palette, a server's menu and the Stats page. It reads the server
//! and shows how full each disk is, the safe cleanups (each with its command in full and
//! roughly what it frees), the biggest folders one level at a time (click one to look inside),
//! Docker and the journal. A cleanup runs only after "Run…" and a yes, and on a Production
//! server only after a second yes; the keyboard starts on Cancel and Return never says yes.
//! After a cleanup that worked the disks are read again and what it freed is said under it.
//!
//! ```text
//! disk-scrim   dims the window; the dialog is modal (Esc closes it)
//! └── disk-dialog  disk-again, disk-close · disk-root
//!     ├── disk-pick-<i>   no server on screen: which one
//!     ├── disk-vol-<i>    a disk: its mount, used of total, a meter
//!     ├── clean-<i>       a cleanup: its strengths (seg-<j>), its command (clean-cmd-<i>),
//!     │                   clean-run-<i>, clean-copy-<i>, the questions clean-yes-<i> /
//!     │                   clean-prod-<i> / clean-no-<i>, then how it went (clean-out-<i>)
//!     ├── disk-crumb-<j>  where "Biggest here" is; disk-open-files
//!     ├── big-<i>         an entry, biggest first; a folder opens on a press
//!     └── disk-docker, disk-journal
//! ```
//!
//! The rules are `sshee_app::disk` (and the security check's `Gate`); the looks and the
//! cleanup run in the backend (`Backend::disk`) and come back through
//! [`crate::shell::Remote::disk`] ([`deliver`]). Names, mounts, paths, commands and output go
//! through `Shell::server_shown` and `Shell::text_shown` (streamer mode).

use std::collections::HashMap;

use rune::{col, el, row, spacer, text, Align, El, ScrollSpec, Tx};
use sshee_app::disk::{self as rules, Ask, Cleanup, Fill, Reply, Report, Usage, Volume};
use sshee_app::recipes::checkup::{self as gate, Gate};
use sshee_app::recipes::Outcome;

use super::checkup::{body, close_button, command_block, dialog, header, headed, link, note};
use crate::icons::{icon, Icon};
use crate::kit::chip::chip;
use crate::kit::graph::meter;
use crate::kit::list::{list_row, ListRow};
use crate::kit::push::{push_button, Push};
use crate::kit::segmented::segmented;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The dialog's width.
pub const WIDTH: f32 = 620.0;

/// A cleanup, once Run… was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Waiting for the yes (or the second yes, on Production), with the command exactly as it
    /// was shown when Run… was pressed: only that one ever runs.
    Asking(Gate, String),
    /// Running; the disks as they were before, to say what it freed.
    Running(Vec<Volume>),
    /// How it went; `before` until the disks were read again, then what it freed.
    Ran { outcome: Outcome, before: Option<Vec<Volume>>, freed: Option<u64> },
}

/// The dialog's own state.
#[derive(Default)]
pub struct State {
    /// Open, for this server (`Some("")`: open, a server still to pick).
    pub open: Option<String>,
    pub loading: bool,
    /// The last look, kept while it looks again.
    pub report: Option<Report>,
    pub error: Option<String>,
    /// The folder "Biggest here" is in.
    pub path: String,
    pub listing: bool,
    /// The last listing, and the folder it is of.
    pub biggest: Option<(String, Vec<Usage>)>,
    pub list_error: Option<String>,
    /// The strength picked for each cleanup that has some, by id.
    pub picks: HashMap<String, usize>,
    /// Cleanups run or asked about, by id.
    pub runs: HashMap<String, Run>,
    /// The names of cleanups that ran, by id: a look after one usually no longer offers it,
    /// and how it went is still said, under its name.
    pub ran: HashMap<String, String>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    fn running(&self) -> bool {
        self.runs.values().any(|r| matches!(r, Run::Running(_)))
    }

    /// The command a cleanup offers now, at the strength picked.
    fn command(&self, id: &str) -> Option<String> {
        let c = self.report.as_ref()?.cleanup(id)?;
        Some(c.command_for(self.pick(c)).to_owned())
    }

    fn pick(&self, c: &Cleanup) -> usize {
        self.picks.get(&c.id).copied().unwrap_or_else(|| c.first_pick())
    }
}

impl Shell {
    /// The sidebar's drive: the server on screen, or a list to pick one from.
    pub fn open_disk_here(&mut self, tx: &mut Tx) {
        let server = self.bench.current_server().filter(|s| !s.is_local()).map(|s| s.key()).unwrap_or_default();
        self.open_disk(&server, tx);
    }

    /// Opens "Free up disk space" for `server` (empty: pick one) and starts looking.
    pub fn open_disk(&mut self, server: &str, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.disk = State { open: Some(server.to_owned()), path: "/".into(), ..State::default() };
        if !server.is_empty() {
            self.disk_again();
        }
        tx.instant();
    }

    pub fn close_disk(&mut self, tx: &mut Tx) {
        self.screens.disk = State::default();
        self.settle(tx);
    }

    /// "Look again", and the first look: the disks and the folder on screen.
    pub fn disk_again(&mut self) {
        self.disk_report();
        self.disk_list();
    }

    fn disk_report(&mut self) {
        let Some(server) = self.screens.disk.open.clone().filter(|s| !s.is_empty()) else { return };
        self.screens.disk.loading = true;
        self.screens.disk.error = None;
        self.backend_mut().disk(Ask::Report { server });
    }

    fn disk_list(&mut self) {
        let Some(server) = self.screens.disk.open.clone().filter(|s| !s.is_empty()) else { return };
        let path = self.screens.disk.path.clone();
        self.screens.disk.listing = true;
        self.screens.disk.list_error = None;
        self.backend_mut().disk(Ask::Biggest { server, path });
    }

    /// "Biggest here" goes to `path`.
    fn disk_go(&mut self, path: String) {
        if self.screens.disk.path == path && self.screens.disk.listing {
            return;
        }
        self.screens.disk.path = path;
        self.disk_list();
    }

    /// A strength picked: a question about the old command goes.
    pub(crate) fn disk_pick(&mut self, id: &str, pick: usize) {
        let state = &mut self.screens.disk;
        state.picks.insert(id.to_owned(), pick);
        if matches!(state.runs.get(id), Some(Run::Asking(..) | Run::Ran { .. })) {
            state.runs.remove(id);
        }
    }

    /// Run…: the first question, under the `i`th card, with the keyboard on its Cancel.
    fn disk_run(&mut self, id: &str, i: usize, tx: &mut Tx) {
        let Some(command) = self.screens.disk.command(id) else { return };
        self.screens.disk.runs.insert(id.to_owned(), Run::Asking(Gate::Confirm, command));
        ask_here(i, tx);
    }

    /// A yes: the next question (Production), or the cleanup runs now. What runs is the
    /// command you were asked about; if a look that came back meanwhile offers another one,
    /// nothing runs, and the new command is there to be run (and asked about) afresh.
    fn disk_yes(&mut self, id: &str, i: usize, tx: &mut Tx) {
        let state = &self.screens.disk;
        let (Some(server), Some(Run::Asking(at, command))) = (state.open.clone(), state.runs.get(id).cloned()) else { return };
        let production = self.bench.server(&server).is_some_and(|s| s.is_prod());
        match gate::after_yes(at, production) {
            Some(next) => {
                self.screens.disk.runs.insert(id.to_owned(), Run::Asking(next, command));
                ask_here(i, tx);
            }
            None => {
                let state = &mut self.screens.disk;
                if state.command(id).as_deref() != Some(command.as_str()) {
                    state.runs.remove(id);
                    return;
                }
                let before = state.report.as_ref().map(|r| r.volumes.clone()).unwrap_or_default();
                if let Some(c) = state.report.as_ref().and_then(|r| r.cleanup(id)) {
                    state.ran.insert(id.to_owned(), c.title.clone());
                }
                state.runs.insert(id.to_owned(), Run::Running(before));
                self.backend_mut().disk(Ask::Clean { server, id: id.to_owned(), command });
            }
        }
    }

    /// Cancel: the question goes, and the keyboard goes back to the Run… it came from.
    fn disk_no(&mut self, id: &str, i: usize, tx: &mut Tx) {
        self.screens.disk.runs.remove(id);
        tx.focus(format!("clean-run-{i}"), true);
    }

    /// A cleanup that needs you (sudo's password): a new terminal with the command you said
    /// yes to, typed and run there.
    fn disk_in_terminal(&mut self, command: String, tx: &mut Tx) {
        let Some(server) = self.screens.disk.open.clone() else { return };
        self.screens.disk = State::default();
        self.run_in_new_terminal(&server, &command, true, tx);
    }
}

/// A question under the `i`th card: in sight (a bottom card's opens below the fold), with the
/// keyboard on its Cancel.
fn ask_here(i: usize, tx: &mut Tx) {
    tx.focus(format!("clean-no-{i}"), true);
    tx.scroll_into_view_in("disk-body", format!("clean-ask-{i}"));
}

/// The backend's answer, on the UI thread.
pub fn deliver(s: &mut Shell, reply: Reply) {
    let state = &mut s.screens.disk;
    if state.open.as_deref() != Some(reply_server(&reply)) {
        return;
    }
    match reply {
        Reply::Report { result, .. } => {
            state.loading = false;
            match result {
                Ok(report) => {
                    // A cleanup that ran: what it freed, now the disks were read again.
                    for run in state.runs.values_mut() {
                        if let Run::Ran { before: before @ Some(_), freed, .. } = run {
                            *freed = before.take().map(|b| rules::freed(&b, &report.volumes));
                        }
                    }
                    state.report = Some(report);
                }
                Err(why) => state.error = Some(why),
            }
        }
        Reply::Biggest { path, result, .. } => {
            if path != state.path {
                return;
            }
            state.listing = false;
            match result {
                Ok(list) => state.biggest = Some((path, list)),
                Err(why) => state.list_error = Some(why),
            }
        }
        Reply::Cleaned { id, outcome, .. } => {
            let before = match state.runs.remove(&id) {
                Some(Run::Running(before)) => before,
                _ => Vec::new(),
            };
            let worked = matches!(outcome, Outcome::Done { .. });
            state.runs.insert(id, Run::Ran { outcome, before: worked.then_some(before), freed: None });
            // It changed the server: look again (0.10's onRan).
            if worked {
                s.disk_again();
            }
        }
    }
}

fn reply_server(reply: &Reply) -> &str {
    match reply {
        Reply::Report { server, .. } | Reply::Biggest { server, .. } | Reply::Cleaned { server, .. } => server,
    }
}

/// A section's heading, and what sits at its end.
fn section(t: &Theme, title: &str, aside: Option<El>) -> El {
    row().w_full().items_center().gap(12.0).child(text(title).font(theme::GROUP_TITLE).color(t.l2).single_line()).child(spacer()).children(aside)
}

/// A disk: its mount, used of total, and a meter.
fn volume(t: &Theme, s: &Shell, v: &Volume, i: usize) -> El {
    let ink = match v.fill() {
        Fill::Full => t.red,
        Fill::Warn => t.orange,
        Fill::Fine => t.l2on,
    };
    let mount = s.text_shown(&v.mount).into_owned();
    let words = v.words();
    col()
        .key(format!("disk-vol-{i}"))
        .w_full()
        .gap(5.0)
        .role("group")
        .label(format!("{mount}: {words}"))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(text(mount).font(theme::CODE).color(t.l1).single_line().truncate().grow(1.0).shrink().min_w(0.0))
                .child(text(words).font(theme::LIST_NUMBER).color(ink).single_line()),
        )
        .child(meter(t, v.fraction(), v.fill() != Fill::Fine, 6.0))
}

/// The question a cleanup asks before it runs, inside its card under the command. Neither
/// yes is the default button: Return never says yes to deleting.
fn question(t: &Theme, server_name: &str, at: Gate, i: usize, id: &str) -> El {
    let (title, line, yes, kind, yes_key) = match at {
        Gate::Confirm => (gate::confirm_title(server_name), None, "Run", Push::Caution, format!("clean-yes-{i}")),
        Gate::Production => (gate::production_title(server_name), Some(gate::PRODUCTION_LINE), "Run on Production", Push::Destructive, format!("clean-prod-{i}")),
    };
    let production = at == Gate::Production;
    let (yes_id, no_id) = (id.to_owned(), id.to_owned());
    let mut words = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(title.clone()).font(theme::TEXT_BOLD).color(if production { t.red } else { t.l1 }).wrap());
    if let Some(line) = line {
        words = words.child(text(line).font(theme::NOTE).color(t.l2on).wrap());
    }
    row()
        .key(format!("clean-ask-{i}"))
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
        .child(push_button(t, "Cancel", Push::Quiet, true).key(format!("clean-no-{i}")).on_press_tx(move |s: &mut Shell, tx| s.disk_no(&no_id, i, tx)))
        .child(push_button(t, yes, kind, true).key(yes_key).on_press_tx(move |s: &mut Shell, tx| s.disk_yes(&yes_id, i, tx)))
}

/// How a cleanup went: what it freed, what it printed, or that it needs a terminal.
fn outcome(t: &Theme, s: &Shell, run: &Run, command: &str, i: usize) -> Option<El> {
    let Run::Ran { outcome, freed, before } = run else { return None };
    let (glyph, ink) = match outcome {
        Outcome::Done { .. } => (Icon::Check, t.green),
        Outcome::NeedsYou { .. } => (Icon::Terminal, t.orange),
        Outcome::Failed { .. } => (Icon::Warning, t.red),
    };
    let headline = match (outcome, freed, before) {
        (Outcome::Done { .. }, Some(freed), _) => rules::freed_words(*freed),
        (Outcome::Done { .. }, None, Some(_)) => rules::MEASURING_FREED.to_owned(),
        _ => outcome.headline(),
    };
    let mut out = col().key(format!("clean-out-{i}")).w_full().mt(8.0).gap(6.0);
    out = out.child(row().gap(6.0).items_center().role("status").label(headline.clone()).child(icon(glyph, 14.0, ink)).child(text(headline).font(theme::TAB_NAME).color(ink).single_line()));
    match outcome {
        Outcome::Done { output } | Outcome::Failed { output, .. } if !output.is_empty() => {
            out = out.child(
                el().w_full().max_h(120.0).px(10.0).py(8.0).radius(theme::RADIUS_ROW).fill(t.hov2).scroll(ScrollSpec { stick_bottom: true }).child(
                    text(s.text_shown(output).into_owned()).font(theme::CODE).color(t.l2on).wrap().selectable(true).w_full(),
                ),
            );
        }
        Outcome::NeedsYou { ask } => {
            let command = command.to_owned();
            out = out.child(text(ask.as_str()).font(theme::NOTE).color(t.l2on).wrap()).child(
                row().child(push_button(t, "Open a terminal for it", Push::Primary, true).key(format!("clean-terminal-{i}")).on_press_tx(move |s: &mut Shell, tx| s.disk_in_terminal(command.clone(), tx))),
            );
        }
        _ => {}
    }
    Some(out)
}

/// One cleanup's card.
fn cleanup(t: &Theme, s: &Shell, server_name: &str, c: &Cleanup, i: usize) -> El {
    let state = &s.screens.disk;
    let pick = state.pick(c);
    let command = c.command_for(pick).to_owned();
    let run = state.runs.get(&c.id);
    let mut head = row().w_full().gap(12.0).items_center().child(text(c.title.as_str()).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate().grow(1.0).shrink().min_w(0.0));
    if let Some(frees) = c.frees(pick) {
        let ink = if c.bytes_for(pick).unwrap_or(0) > 0 { t.green } else { t.l2 };
        head = head.child(text(frees).font(theme::LIST_NUMBER).color(ink).single_line());
    }
    let mut words = col().grow(1.0).shrink().min_w(0.0).gap(4.0).child(head).child(text(s.text_shown(&c.detail).into_owned()).font(theme::NOTE).color(t.l2on).wrap().selectable(true));
    let labels = c.labels();
    if !labels.is_empty() {
        let id = c.id.clone();
        words = words.child(
            row().mt(2.0).child(segmented(t, &format!("{}: how old", c.title), &labels, Some(pick), !matches!(run, Some(Run::Running(_))), move |s: &mut Shell, p, _| s.disk_pick(&id, p)).key(format!("clean-pick-{i}"))),
        );
    }
    words = words.child(el().h(2.0)).child(command_block(t, &s.text_shown(&command)).key(format!("clean-cmd-{i}")));
    let running = matches!(run, Some(Run::Running(_)));
    let mut buttons = row().mt(4.0).gap(8.0).items_center();
    if !matches!(run, Some(Run::Asking(..))) {
        let id = c.id.clone();
        buttons = buttons.child(
            push_button(t, if running { "Running…" } else { "Run…" }, Push::Quiet, !running && !state.loading && !state.running())
                .key(format!("clean-run-{i}"))
                .help(format!("Asks first, then runs it on {server_name}"))
                .on_press_tx(move |s: &mut Shell, tx| s.disk_run(&id, i, tx)),
        );
    }
    let copy = command.clone();
    buttons = buttons.child(push_button(t, "Copy", Push::Quiet, true).key(format!("clean-copy-{i}")).on_press_tx(move |s: &mut Shell, tx| {
        tx.copy(copy.clone());
        s.toast(crate::shell::Toast::info("Copied the command"));
    }));
    words = words.child(buttons);
    if let Some(Run::Asking(at, _)) = run {
        words = words.child(question(t, server_name, *at, i, &c.id));
    }
    if let Some(done) = run.and_then(|r| outcome(t, s, r, &command, i)) {
        words = words.child(done);
    }
    col().key(format!("clean-{i}")).w_full().px(14.0).py(12.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.hair2).child(words)
}

/// A cleanup that ran and that the last look no longer offers: its name and how it went.
fn ran(t: &Theme, s: &Shell, id: &str, title: &str, run: &Run) -> Option<El> {
    if !matches!(run, Run::Ran { outcome: Outcome::Done { .. }, .. }) {
        return None;
    }
    let done = outcome(t, s, run, "", 0)?.key(format!("clean-done-{id}")).mt(4.0);
    Some(col().w_full().px(14.0).py(12.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.hair2).child(text(title).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate()).child(done))
}

/// Where "Biggest here" is: the root, then each folder down to this one.
fn crumbs(t: &Theme, s: &Shell, path: &str) -> El {
    let parts = rules::crumbs(path);
    let last = parts.len() - 1;
    let mut out = row().key("disk-crumbs").w_full().gap(2.0).items_center().role("navigation").label_static("Folder");
    for (j, (name, to)) in parts.into_iter().enumerate() {
        if j > 1 {
            out = out.child(text("/").font(theme::CODE).color(t.l2).single_line());
        }
        let ink = if j == last { t.l1 } else { t.l2 };
        let shown = s.text_shown(&name).into_owned();
        out = out.child(
            el().key(format!("disk-crumb-{j}"))
                .px(4.0)
                .py(2.0)
                .radius(4.0)
                .hover(t.hov2)
                .focusable(true)
                .role("button")
                .label(shown.clone())
                .on_press_tx(move |s: &mut Shell, _| s.disk_go(to.clone()))
                .child(text(shown).font(theme::CODE).color(ink).single_line().a11y_hidden()),
        );
    }
    out
}

/// The biggest entries in the folder, biggest first; a folder opens on a press.
fn biggest(t: &Theme, s: &Shell, server: &str) -> El {
    let state = &s.screens.disk;
    let open_files = {
        let (server, path) = (server.to_owned(), state.path.clone());
        link(t, "Open in Files").key("disk-open-files").on_press_tx(move |s: &mut Shell, tx| {
            s.screens.disk = State::default();
            s.open_files_on(&server, Some(&path), tx);
        })
    };
    let mut out = col().key("disk-biggest").w_full().gap(6.0).child(section(t, "Biggest here", Some(open_files))).child(crumbs(t, s, &state.path));
    if state.listing && state.biggest.is_none() {
        out = out.child(waiting(t, rules::MEASURING));
    }
    if let Some(why) = &state.list_error {
        out = out.child(note(t, &s.text_shown(why), true).key("disk-list-error").role("alert"));
    }
    let Some((_, list)) = &state.biggest else { return out };
    let mut rows = col().key("disk-list").w_full().mx(-8.0).gap(0.0);
    if state.listing {
        rows = rows.opacity(0.5);
    }
    if list.is_empty() {
        rows = rows.child(text(rules::EMPTY).font(theme::NOTE).color(t.l2).single_line().px(8.0).py(6.0));
    }
    let top = list.first().map_or(0, |u| u.bytes);
    for (i, u) in list.iter().enumerate() {
        let name = s.text_shown(&u.name).into_owned();
        let amount = sshee_app::workbench::vitals::bytes(u.bytes as f64);
        let mut line = row()
            .key(format!("big-{i}"))
            .w_full()
            .h(30.0)
            .px(8.0)
            .gap(10.0)
            .items_center()
            .radius(theme::RADIUS_ROW)
            .role(if u.is_dir { "button" } else { "row" })
            .label(format!("{name}, {amount}"))
            .child(icon(if u.is_dir { Icon::Folder } else { Icon::File }, 14.0, if u.is_dir { t.blue } else { t.l2 }))
            .child(text(name).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).shrink().min_w(0.0))
            .child(el().w(96.0).child(meter(t, rules::share(u.bytes, top), false, 5.0)))
            .child(text(amount).font(theme::LIST_NUMBER).color(t.l2on).w(64.0).align(Align::End).single_line());
        if u.is_dir && !state.listing {
            let to = u.path.clone();
            let look = format!("Look inside {}", s.text_shown(&u.path));
            line = line.hover(t.hov2).focusable(true).help(look).on_press_tx(move |s: &mut Shell, _| s.disk_go(to.clone()));
        }
        rows = rows.child(line);
    }
    out.child(rows)
}

/// Docker's images, containers, volumes and build cache, two tiles a row.
fn docker(t: &Theme, s: &Shell, d: &rules::Docker) -> El {
    let tile = |(label, total, unused): (&str, u64, u64)| {
        col()
            .grow(1.0)
            .min_w(0.0)
            .px(12.0)
            .py(8.0)
            .gap(2.0)
            .radius(theme::RADIUS_ROW)
            .fill(t.group)
            .border(1.0, t.hair2)
            .child(text(label).font(theme::SMALL).color(t.l2).single_line())
            .child(text(rules::docker_words(total, unused)).font(theme::LIST_NUMBER).color(t.l1).single_line().truncate())
    };
    let [a, b, c, e] = d.rows();
    let head = row().w_full().gap(2.0).items_center().child(text("Docker").font(theme::GROUP_TITLE).color(t.l2).single_line()).child(super::help::tip(s, t, "disk-docker-about", "Docker's disk use", super::help::DOCKER_DISK));
    col().key("disk-docker").w_full().gap(8.0).child(head).child(row().w_full().gap(8.0).child(tile(a)).child(tile(b))).child(row().w_full().gap(8.0).child(tile(c)).child(tile(e)))
}

fn waiting(t: &Theme, words: &str) -> El {
    row().w_full().py(16.0).gap(8.0).items_center().role("status").child(crate::kit::progress_ring(14.0, 1.6, t.l2)).child(text(words).font(theme::NOTE).color(t.l2).single_line())
}

/// Which server, when none was on screen.
fn picker(t: &Theme, s: &Shell) -> El {
    let servers: Vec<_> = s.bench.servers().iter().filter(|sv| !sv.is_local()).collect();
    if servers.is_empty() {
        return note(t, "Add a server first: this looks at a server's disks over SSH.", false);
    }
    let mut list = col().key("disk-pick").w_full().mx(-8.0).gap(2.0);
    for (i, server) in servers.into_iter().enumerate() {
        let key = server.key();
        let name = s.server_shown(&key, &server.name).into_owned();
        let address = if s.streaming() { String::new() } else { server.address.clone() };
        let spec = ListRow { icon: Some(Icon::for_server(&server.icon)), label: &name, note: &address, prod: server.is_prod(), ..ListRow::default() };
        list = list.child(list_row(t, &spec).key(format!("disk-pick-{i}")).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, tx| s.open_disk(&key, tx)));
    }
    col().w_full().gap(8.0).child(text("Which server?").font(theme::GROUP_TITLE).color(t.l2).single_line()).child(list)
}

/// The dialog, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.disk;
    let server = state.open.as_deref()?;
    let found = s.bench.server(server);
    let server_name = found.map(|sv| s.server_shown(server, &sv.name).into_owned()).unwrap_or_default();
    let prod = found.is_some_and(|sv| sv.is_prod());

    let mut end = Vec::new();
    if !server.is_empty() {
        let busy = state.loading || state.listing;
        end.push(push_button(t, if busy { "Looking…" } else { "Look again" }, Push::Quiet, !busy && !state.running()).key("disk-again").on_press_tx(|s: &mut Shell, _| s.disk_again()));
    }
    end.push(close_button(t, "disk-close").on_press_tx(|s: &mut Shell, tx| s.close_disk(tx)));
    let sub = if server.is_empty() { "Pick a server: nothing is deleted until you run a cleanup".to_owned() } else { format!("{server_name} · {}", rules::PROMISE) };
    let mut head = header(t, Icon::Drive, "Free up disk space", &sub, end);
    if prod {
        head = col().w_full().gap(8.0).child(head).child(row().ml(44.0).gap(6.0).items_center().child(chip(t, "Prod", t.red)).child(text("Cleanups here ask twice.").font(theme::SMALL).color(t.l2).single_line()));
    }
    let mut page = body("disk-body").gap(18.0);
    if server.is_empty() {
        page = page.child(picker(t, s));
    } else {
        if state.loading && state.report.is_none() {
            page = page.child(waiting(t, rules::LOOKING).key("disk-waiting"));
        }
        if let Some(why) = &state.error {
            page = page.child(note(t, &s.text_shown(why), true).key("disk-error").role("alert"));
        }
        if let Some(report) = &state.report {
            if !report.as_root {
                page = page.child(note(t, gate::NEEDS_ROOT, false).key("disk-root"));
            }
            if !report.volumes.is_empty() {
                let mut disks = col().key("disk-volumes").w_full().gap(10.0).child(section(t, "Disks", None));
                for (i, v) in report.volumes.iter().enumerate() {
                    disks = disks.child(volume(t, s, v, i));
                }
                page = page.child(disks);
            }
            let mut gone: Vec<(&String, &String)> = state.ran.iter().filter(|(id, _)| report.cleanup(id).is_none()).collect();
            gone.sort();
            let gone: Vec<El> = gone.into_iter().filter_map(|(id, title)| ran(t, s, id, title, state.runs.get(id)?)).collect();
            if !report.cleanups.is_empty() || !gone.is_empty() {
                let mut cards = col().key("disk-cleanups").w_full().gap(8.0).child(section(t, "Safe to clean up", None)).children(gone);
                if state.loading {
                    cards = cards.opacity(0.6);
                }
                for (i, c) in report.cleanups.iter().enumerate() {
                    cards = cards.child(cleanup(t, s, &server_name, c, i));
                }
                page = page.child(cards);
            }
        }
        page = page.child(biggest(t, s, server));
        if let Some(report) = &state.report {
            if let Some(d) = &report.docker {
                page = page.child(docker(t, s, d));
            }
            if let Some(journal) = report.journal {
                page = page.child(text(rules::journal_words(journal)).key("disk-journal").font(theme::NOTE).color(t.l2).single_line());
            }
        }
    }
    Some(dialog(t, "disk-scrim", "disk-dialog", "Free up disk space", WIDTH, headed(t, head, page, 18.0), |s: &mut Shell, tx| s.close_disk(tx)))
}
