//! A machine's Docker and Podman containers, as a tab of their own (0.10's container tree in
//! the sidebar): grouped by Compose project, each with its state, ports, image and how long it
//! has been up; Start, Stop and Restart; its logs, followed, or a shell inside it, each in a
//! new terminal tab.
//!
//! ```text
//! containers    column
//! ├── toolbar   40 pt: Containers  4 containers, 3 running                     ⟳
//! ├── notes     what went wrong over the list (with the command that fixes it), how the last
//! │             start, stop or restart went
//! └── list      app  2 of 3 running
//!               ▣ app-api-1      Up 3 days · healthy   8080 → 8080     Logs Shell Restart Stop
//!                 ghcr.io/…/api                                        (shown under the pointer)
//! ```
//!
//! The rules and every sentence are `sshee_app::containers`; this module keeps one `Board` per
//! tab, asks the backend (`Backend::containers`) and draws. A look is taken when the tab comes
//! on screen and again every 10 seconds while it's there ([`tick`]); answers come back through
//! `Remote::containers` ([`deliver`]). Names, images and projects are drawn through
//! `Shell::text_shown` (streamer mode).

use rune::{col, el, row, spacer, text, Align, Cx, El, ScrollSpec, Tx};
use sshee_app::containers::{self as rules, Action, Answer, Ask, Board, Health, Problem, Reply, Row, State as Run, Want};
use sshee_app::model::ContainerRun;
use sshee_app::workbench::{Open, Tab, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::push::{icon_button, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

const TOOLBAR: f32 = 40.0;
/// A container's row: its name over its image.
const ROW: f32 = 44.0;
const STATUS_W: f32 = 168.0;
const PORTS_W: f32 = 172.0;
/// The buttons under the pointer: a fixed width, so every row's columns line up.
const ACTIONS_W: f32 = 232.0;

/// One Containers tab's own state.
#[derive(Default)]
pub struct State {
    pub board: Board,
    /// When the next look is due, on the window's clock; `None`: now (or one is on its way).
    due: Option<f64>,
    /// When the look on its way was first seen waiting, on the window's clock: past
    /// [`rules::LOOK_TIMEOUT_SECS`] it counts as unanswered.
    asked: Option<f64>,
    /// The empty list's guide to a first container is open.
    guide: bool,
}

fn ask(s: &mut Shell, tab: TabId, want: Want) {
    let Some(server) = s.bench.tab(tab).map(|t| t.server.clone()) else { return };
    s.backend_mut().containers(Ask { tab, server, want });
}

/// Looks again now.
pub fn refresh(s: &mut Shell, tab: TabId) {
    let Some(state) = s.screens.containers.get_mut(&tab) else { return };
    let want = state.board.list();
    state.due = None;
    state.asked = None;
    ask(s, tab, want);
}

/// Every frame: a Containers tab on screen that has no look yet, or whose next look is due,
/// asks for one; a look unanswered for [`rules::LOOK_TIMEOUT_SECS`] gives up and says so.
/// True when something changed on screen.
pub(crate) fn tick(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    let shown: Vec<TabId> = s.tabs_on_screen().into_iter().filter(|id| s.bench.tab(*id).is_some_and(|t| t.kind == TabKind::Containers)).collect();
    let mut changed = false;
    for id in shown {
        let state = s.screens.containers.entry(id).or_default();
        if state.board.is_loading() {
            // A look asked by a button is timed from the first frame that sees it waiting.
            let Some(asked) = state.asked else {
                state.asked = Some(clock);
                tx.wake_at(clock + rules::LOOK_TIMEOUT_SECS);
                continue;
            };
            if clock >= asked + rules::LOOK_TIMEOUT_SECS && state.board.no_answer() {
                let due = clock + state.board.next_look_in();
                (state.asked, state.due) = (None, Some(due));
                tx.wake_at(due);
                changed = true;
            }
        } else if state.due.is_none_or(|due| clock >= due) {
            changed |= state.board.is_first_look();
            refresh(s, id);
            if let Some(st) = s.screens.containers.get_mut(&id).filter(|st| st.board.is_loading()) {
                st.asked = Some(clock);
                tx.wake_at(clock + rules::LOOK_TIMEOUT_SECS);
            }
        }
    }
    changed
}

/// A button on a container: start, stop or restart it.
pub fn act(s: &mut Shell, tab: TabId, id: &str, action: Action) {
    let Some(want) = s.screens.containers.get_mut(&tab).and_then(|st| st.board.act(id, action)) else { return };
    ask(s, tab, want);
}

/// Logs or a shell: a new terminal tab on the same machine, running inside the container.
pub fn open_into(s: &mut Shell, tab: TabId, id: &str, run: ContainerRun, tx: &mut Tx) {
    let Some(server) = s.bench.tab(tab).map(|t| t.server.clone()) else { return };
    let Some(inside) = s.screens.containers.get(&tab).and_then(|st| st.board.container(id)).map(|c| rules::inside(c, run)) else { return };
    s.bench.open_with(TabKind::Terminal, &server, Open { title: Some(rules::tab_title(&inside)), container: Some(inside), ..Open::default() });
    s.settle(tx);
}

/// The backend's answer (`Remote::containers`): a look, or how a button went. The next look
/// is timed from here.
pub fn deliver(s: &mut Shell, reply: Reply, tx: &mut Tx) {
    // A tree under a server in the sidebar asked (`screens::rail_tree`).
    if super::rail_tree::deliver(s, &reply, tx) {
        return;
    }
    let Some(state) = s.screens.containers.get_mut(&reply.tab) else { return };
    match reply.answer {
        Answer::Listed { ticket, result } => {
            if state.board.listed(ticket, result) {
                state.asked = None;
                let due = tx.clock() + state.board.next_look_in();
                state.due = Some(due);
                tx.wake_at(due);
            }
        }
        Answer::Acted { id, name, action, result } => {
            state.board.acted(&id, &name, action, result);
            // What it did shows at once.
            if !state.board.is_loading() {
                refresh(s, reply.tab);
            }
        }
    }
}

// ---- drawing

fn toolbar(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let board = s.screens.containers.get(&id).map(|st| &st.board);
    let mut bar = row()
        .key("containers-toolbar")
        .w_full()
        .h(TOOLBAR)
        .items_center()
        .gap(10.0)
        .pl(16.0)
        .pr(8.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .child(row().gap(2.0).items_center().child(text("Containers").font(theme::TEXT_BOLD).color(t.l1).single_line()).child(super::help::tip(s, t, "containers-about", "containers", super::help::CONTAINERS)));
    if let Some(summary) = board.and_then(Board::summary) {
        bar = bar.child(text(summary).key("containers-summary").font(theme::NOTE).color(t.l2).single_line());
    }
    bar = bar.child(spacer());
    if board.is_some_and(Board::is_loading) {
        bar = bar.child(text("Looking…").key("containers-busy").font(theme::STATUS).color(t.l2).single_line());
    }
    bar.child(icon_button(t, Icon::Refresh, "Look again", true).key("containers-refresh").on_press_tx(move |s: &mut Shell, tx| {
        refresh(s, id);
        tx.instant();
    }))
}

/// A command to copy or to type into a terminal there (it isn't run: you press Return).
fn command_box(s: &Shell, id: TabId, t: &'static Theme, command: &str, key: &str) -> El {
    let (copy, typed) = (command.to_owned(), command.to_owned());
    let server = s.bench.tab(id).map(|t| t.server.clone()).unwrap_or_default();
    col()
        .key(key.to_owned())
        .gap(8.0)
        .items_center()
        .child(
            el().px(10.0)
                .py(6.0)
                .radius(theme::RADIUS_ROW)
                .fill(t.code)
                .border(1.0, t.hair2)
                .max_w(460.0)
                .child(text(s.text_shown(command).into_owned()).font(theme::CODE).color(t.l1).selectable(true)),
        )
        .child(
            row()
                .gap(8.0)
                .child(push_button(t, "Copy", Push::Quiet, true).key(format!("{key}-copy")).on_press_tx(move |_: &mut Shell, tx| tx.copy(copy.clone())))
                .child(push_button(t, "Type it in a terminal", Push::Quiet, true).key(format!("{key}-type")).help("Types it at a terminal's prompt there; you press Return").on_press_tx(
                    move |s: &mut Shell, tx| s.run_on(&server, &typed, false, tx),
                )),
        )
}

fn centered(children: Vec<El>) -> El {
    col().key("containers-page").w_full().grow(1.0).min_h(0.0).items_center().justify_center().gap(8.0).px(24.0).children(children)
}

/// What a page says in place of the list.
fn page(s: &Shell, id: TabId, t: &'static Theme, title: &str, message: &str, command: Option<&str>, open_terminal: bool) -> El {
    let mut parts = vec![
        icon(Icon::Cube, 28.0, t.l2),
        text(s.text_shown(title).into_owned()).font(theme::TEXT_BOLD).color(t.l1).single_line(),
        text(s.text_shown(message).into_owned()).font(theme::NOTE).color(t.l2).max_w(420.0).align(Align::Center),
    ];
    if let Some(command) = command {
        parts.push(command_box(s, id, t, command, "containers-fix").mt(6.0));
    }
    let mut buttons = row().gap(8.0).mt(6.0);
    if open_terminal {
        buttons = buttons.child(push_button(t, rules::OPEN_TERMINAL, Push::Primary, true).key("containers-open-terminal").on_press_tx(move |s: &mut Shell, tx| {
            if let Some(server) = s.bench.tab(id).map(|t| t.server.clone()) {
                s.bench.open(TabKind::Terminal, &server);
                s.settle(tx);
            }
        }));
    }
    buttons = buttons.child(push_button(t, "Try again", Push::Quiet, true).key("containers-try-again").on_press_tx(move |s: &mut Shell, tx| {
        refresh(s, id);
        tx.instant();
    }));
    parts.push(buttons);
    centered(parts)
}

/// A line over the list: what's wrong with an engine, or how a button went.
fn note_line(t: &'static Theme, key: &str, glyph: Icon, ink: rune::Color, words: String) -> El {
    row()
        .key(key.to_owned())
        .w_full()
        .min_h(32.0)
        .items_center()
        .gap(8.0)
        .px(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .child(icon(glyph, 13.0, ink))
        .child(text(words).font(theme::NOTE).color(t.l2on).wrap().grow(1.0).min_w(0.0))
}

fn health_word(h: Health) -> &'static str {
    match h {
        Health::Healthy => "healthy",
        Health::Unhealthy => "unhealthy",
        Health::Starting => "checking health",
    }
}

/// One container.
fn container_row(s: &Shell, id: TabId, t: &'static Theme, n: usize, r: &Row) -> El {
    let live = r.state == Run::Running;
    let name_ink = if live { t.l1 } else { t.l2on };
    let mut status = row().w(STATUS_W).gap(6.0).items_center().child(text(&r.status).font(theme::NOTE).color(if r.busy.is_some() { t.l1 } else { t.l2 }).single_line());
    if let Some(h) = r.health.filter(|_| r.busy.is_none()) {
        // An unhealthy container is an error; the rest is said quietly.
        status = status.child(text(health_word(h)).font(theme::NOTE).color(if h == Health::Unhealthy { t.red } else { t.l2 }).single_line());
    }
    let ports = r.ports.join(", ");
    let mut actions = row().key(format!("ctr-{n}-actions")).w(ACTIONS_W).justify_end().gap(4.0).items_center().group_opacity(0.0, 1.0);
    let cid = r.id.clone();
    actions = actions.child(small_button(t, "Logs", true).key(format!("ctr-{n}-logs")).help("Follow its logs in a new tab").on_press_tx({
        let cid = cid.clone();
        move |s: &mut Shell, tx| open_into(s, id, &cid, ContainerRun::Logs, tx)
    }));
    if r.can_shell {
        actions = actions.child(small_button(t, "Shell", true).key(format!("ctr-{n}-shell")).help("Open a shell inside it in a new tab").on_press_tx({
            let cid = cid.clone();
            move |s: &mut Shell, tx| open_into(s, id, &cid, ContainerRun::Shell, tx)
        }));
    }
    for action in &r.actions {
        let (cid, action) = (cid.clone(), *action);
        let word = action.label();
        actions = actions.child(small_button(t, word, r.busy.is_none()).key(format!("ctr-{n}-{}", word.to_lowercase())).on_press_tx(move |s: &mut Shell, tx| {
            act(s, id, &cid, action);
            tx.instant();
        }));
    }
    let open = cid.clone();
    let shell_first = r.can_shell;
    row()
        .key(format!("ctr-{n}"))
        .h(ROW)
        .mx(6.0)
        .px(10.0)
        .gap(10.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .hover(t.hov)
        .hover_group()
        .role("row")
        .label(format!("{}, {}", s.text_shown(&r.name), r.status))
        .child(icon(Icon::Cube, theme::ICON, if live { t.l1 } else { t.l2 }))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(text(s.text_shown(&r.name).into_owned()).font(theme::TAB_NAME).color(name_ink).single_line().truncate())
                .child(text(s.text_shown(&r.image).into_owned()).font(theme::SMALL).color(t.l2).single_line().truncate()),
        )
        .child(status)
        .child(el().w(PORTS_W).child(text(ports).font(theme::LIST_NUMBER).color(t.l2).single_line().truncate()))
        .child(actions)
        // Double-click: inside it while it runs, else its logs (0.10 opened a shell on a click).
        .on_double_press_tx(move |s: &mut Shell, tx| open_into(s, id, &open, if shell_first { ContainerRun::Shell } else { ContainerRun::Logs }, tx))
}

/// A word on a quiet button, small enough to sit in a row.
fn small_button(t: &Theme, label: &str, enabled: bool) -> El {
    push_button(t, label, Push::Quiet, enabled).h(theme::FORM_CONTROL).px(9.0)
}

fn list(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(board) = s.screens.containers.get(&id).map(|st| &st.board) else { return el() };
    let mut out = col().key("containers-list").w_full().grow(1.0).min_h(0.0).py(4.0).role("list").label_static("Containers").scroll(ScrollSpec { stick_bottom: false });
    let mut n = 0;
    for (g, group) in board.groups().iter().enumerate() {
        let heading = match &group.project {
            Some(project) => s.text_shown(project).into_owned(),
            None if board.groups().len() > 1 => "Not in a project".to_owned(),
            None => String::new(),
        };
        if !heading.is_empty() {
            out = out.child(
                row()
                    .key(format!("ctr-group-{g}"))
                    .h(28.0)
                    .mt(if g == 0 { 4.0 } else { 12.0 })
                    .px(16.0)
                    .gap(8.0)
                    .items_center()
                    .help(if group.project.is_some() { "A Docker Compose project" } else { "" })
                    .child(text(heading).font(theme::GROUP_TITLE).color(t.l1).single_line())
                    .child(text(&group.summary).font(theme::NOTE).color(t.l2).single_line()),
            );
        }
        for r in &group.rows {
            out = out.child(container_row(s, id, t, n, r));
            n += 1;
        }
    }
    if let Some((said, how)) = board.empty() {
        let open = s.screens.containers.get(&id).is_some_and(|st| st.guide);
        let mut empty = col()
            .key("containers-empty")
            .w_full()
            .items_center()
            .gap(4.0)
            .pt(64.0)
            .child(text(said).font(theme::TEXT).color(t.l2on).single_line())
            .child(text(how).font(theme::NOTE).color(t.l2).single_line())
            .child(push_button(t, if open { "Hide the guide" } else { "How to run a first container" }, Push::Quiet, true).key("containers-guide").mt(8.0).on_press_tx(move |s: &mut Shell, tx| {
                if let Some(st) = s.screens.containers.get_mut(&id) {
                    st.guide = !st.guide;
                }
                tx.instant();
            }));
        if open {
            empty = empty.child(guide(s, id, t));
        }
        out = out.child(empty);
    }
    out
}

/// The empty list's short guide: one container, or a Compose project, each with its command.
fn guide(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let mut steps = col().key("containers-guide-steps").gap(16.0).mt(12.0).items_center().max_w(480.0);
    for (n, (title, command, explain)) in rules::FIRST_CONTAINER.iter().enumerate() {
        steps = steps.child(
            col()
                .gap(6.0)
                .items_center()
                .child(text(*title).font(theme::TEXT_BOLD).color(t.l1).single_line())
                .child(text(*explain).font(theme::NOTE).color(t.l2).max_w(460.0).align(Align::Center))
                .child(command_box(s, id, t, command, &format!("containers-guide-{n}"))),
        );
    }
    steps.child(text(rules::FIRST_CONTAINER_AFTER).font(theme::NOTE).color(t.l2).max_w(460.0).align(Align::Center))
}

/// The tab's page.
pub fn view(s: &Shell, tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let id = tab.id;
    let mut pane = col().key("containers").w_full().h_full().child(toolbar(s, id, t));
    let Some(board) = s.screens.containers.get(&id).map(|st| &st.board) else { return pane };
    if board.is_first_look() {
        // Not looked yet: the shape of a list, so nothing jumps when it comes.
        let mut ghost = col().key("containers-loading").w_full().py(8.0).gap(4.0);
        for i in 0..5 {
            let w = 140.0 + ((i * 37) % 45) as f32 * 3.0;
            ghost = ghost.child(
                row().key(format!("ghost-{i}")).h(ROW).mx(6.0).px(10.0).gap(10.0).items_center().child(el().size(theme::ICON, theme::ICON).radius(4.0).fill(t.hov2)).child(el().w(w).h(8.0).radius(4.0).fill(t.hov2)),
            );
        }
        return pane.child(ghost);
    }
    match board.problem() {
        Some(Problem::Page { title, message, command, open_terminal }) => return pane.child(page(s, id, t, &title, &message, command.as_deref(), open_terminal)),
        Some(Problem::Note { text: note, command }) => {
            let mut line = note_line(t, "containers-problem", Icon::Warning, t.orange, s.text_shown(&note).into_owned());
            if let Some(command) = command {
                let (copy, typed) = (command.clone(), command.clone());
                let server = tab.server.clone();
                line = line
                    .child(text(s.text_shown(&command).into_owned()).font(theme::CODE).color(t.l1).single_line().selectable(true))
                    .child(push_button(t, "Copy", Push::Quiet, true).key("containers-problem-copy").on_press_tx(move |_: &mut Shell, tx| tx.copy(copy.clone())))
                    .child(push_button(t, "Type it in a terminal", Push::Quiet, true).key("containers-problem-type").on_press_tx(move |s: &mut Shell, tx| s.run_on(&server, &typed, false, tx)));
            }
            pane = pane.child(line);
        }
        None => {}
    }
    if let Some(note) = board.note() {
        let (glyph, ink) = if note.ok { (Icon::Check, t.l2on) } else { (Icon::Warning, t.red) };
        pane = pane.child(note_line(t, "containers-note", glyph, ink, s.text_shown(&note.text).into_owned()).child(
            icon_button(t, Icon::Close, "Dismiss", true).key("containers-note-close").on_press_tx(move |s: &mut Shell, tx| {
                if let Some(st) = s.screens.containers.get_mut(&id) {
                    st.board.dismiss_note();
                }
                tx.instant();
            }),
        ));
    }
    pane.child(list(s, id, t))
}

impl Shell {
    /// A machine's containers: its Containers tab, or a new one (the palette).
    pub fn open_containers(&mut self, server: &str, tx: &mut Tx) {
        self.show_tab(TabKind::Containers, server, None, tx);
    }
}

#[cfg(test)]
mod tests;
