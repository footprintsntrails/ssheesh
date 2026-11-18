//! A machine's statistics (0.10's `StatsPane`): CPU, load, memory, network and every disk, each
//! on a line with its graph over the last five minutes or its meter, the machine's name,
//! kernel and uptime under them, and the programs using the most CPU now. Also the load on a
//! server's card in the sidebar (0.10's `CardLoad` in `ServerLoad.tsx`).
//!
//! ```text
//! stats          column, scrolls
//! ├── toolbar    40 pt: Stats                                          Last 5 minutes
//! ├── table      CPU       38%           ╱╲__╱╲___╱‾‾╲_____
//! │              Memory    60%           ███████████░░░░░░░
//! │              …
//! ├── about      web-7731, Linux 6.8.0, up 37 d 5 h
//! └── programs   Busiest programs: name, CPU, memory
//! ```
//!
//! The numbers are the window's vital signs (`Workbench::vitals`, `Workbench::history`) and
//! the facts of the same looks ([`set_facts`]); the words are `sshee_app::stats`. Programs are
//! asked for while a Stats tab is on screen ([`tick`], `Backend::stats`). The machine's own
//! name never shows while streaming; program names go through `Shell::text_shown`.

use std::collections::HashMap;

use rune::{col, el, row, spacer, text, Align, Cx, El, ScrollSpec};
use sshee_app::stats::{self as rules, Facts, Graph, Line, Load, Programs, Reply, PROCESSES_EVERY};
use sshee_app::workbench::vitals::HISTORY;
use sshee_app::workbench::{Link, ServerKey, Tab, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::graph::{graph, meter, Series};
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

const TOOLBAR: f32 = 40.0;
const LABEL_W: f32 = 84.0;
const VALUE_W: f32 = 260.0;
const GRAPH_H: f32 = 34.0;
const LINE_H: f32 = 58.0;

/// One Stats tab's own state: the programs, and when to look at them next.
#[derive(Default)]
pub struct State {
    pub programs: Programs,
    due: Option<f64>,
}

/// What the looks at each machine tell beyond its vital signs, or why it can't be asked.
pub type Machines = HashMap<ServerKey, Result<Facts, String>>;

/// The facts of a machine's last look (`Remote::set_facts`); `Err`: it can't be asked (a
/// machine without `/proc`).
pub fn set_facts(s: &mut Shell, server: ServerKey, facts: Result<Facts, String>) {
    s.screens.machines.insert(server, facts);
}

/// The programs on a machine came back (`Remote::stats`).
pub fn deliver(s: &mut Shell, reply: Reply, tx: &mut rune::Tx) {
    let Some(state) = s.screens.stats.get_mut(&reply.tab) else { return };
    if state.programs.answered(reply.ticket, reply.result) {
        let due = tx.clock() + PROCESSES_EVERY;
        state.due = Some(due);
        tx.wake_at(due);
    }
}

/// The machine is being looked at: this computer, or a server with a connected terminal.
fn watched(s: &Shell, server: &str) -> bool {
    server == "local" || s.bench.tabs_on(server).any(|t| matches!(t.link, Link::Connected { .. }))
}

/// Every frame: a Stats tab on screen asks for the programs when they're due. True when the
/// page changed.
pub(crate) fn tick(s: &mut Shell, clock: f64) -> bool {
    let shown: Vec<(TabId, ServerKey)> = s
        .tabs_on_screen()
        .into_iter()
        .filter_map(|id| s.bench.tab(id).filter(|t| t.kind == TabKind::Stats).map(|t| (id, t.server.clone())))
        .collect();
    let mut changed = false;
    for (id, server) in shown {
        let can = watched(s, &server) && !matches!(s.screens.machines.get(&server), Some(Err(_)));
        let state = s.screens.stats.entry(id).or_default();
        if !can || state.due.is_some_and(|due| clock < due) {
            continue;
        }
        if let Some(ticket) = state.programs.ask() {
            changed = true;
            state.due = None;
            s.backend_mut().stats(sshee_app::stats::Ask { tab: id, server, ticket });
        }
    }
    changed
}

/// A graph with the value at its top written small in its top right corner.
fn scaled(t: &Theme, line: El, top: &str) -> El {
    el().w_full()
        .child(line)
        .child(text(top.to_owned()).key("graph-top").font(theme::LIST_NUMBER).color(t.l2).single_line().pin(None, Some(-2.0), Some(0.0), None).label(format!("Graph's top: {top}")))
}

fn line_view(s: &Shell, server: &str, t: &'static Theme, l: &Line) -> El {
    let value = if l.warn {
        row().gap(5.0).items_center().child(icon(Icon::Warning, 12.0, t.orange)).child(text(&l.value).font(theme::STAT_WARN).color(t.l1).single_line())
    } else {
        row().child(text(&l.value).font(theme::STAT_VALUE).color(t.l1).single_line())
    };
    let mut middle = col().w(VALUE_W).min_w(0.0).gap(2.0).child(value);
    if let Some(detail) = &l.detail {
        middle = middle.child(text(s.text_shown(detail).into_owned()).font(theme::LIST_NUMBER).color(t.l2).single_line().truncate());
    }
    let history = s.bench.history(server);
    // In the accent with its wash, as 0.10 drew them: a grey line on grey read as a dead graph.
    // The CPU graph's top follows a quiet server (10, 25, 50 or 100%), and says so in its corner.
    let beside = match (l.graph, history) {
        (Some(Graph::Cpu), Some(h)) => {
            let top = h.cpu_scale();
            let line = graph(t, &[Series { values: &h.series(|x| x.cpu), ink: t.blue, wash: true }], HISTORY, top, GRAPH_H);
            Some(if top < 1.0 { scaled(t, line, &format!("{:.0}%", top * 100.0)) } else { line })
        }
        (Some(Graph::Memory), Some(h)) => Some(graph(t, &[Series { values: &h.series(|x| Some(x.memory)), ink: t.blue, wash: true }], HISTORY, 1.0, GRAPH_H)),
        (Some(Graph::Network), Some(h)) => {
            let (down, up) = (h.series(|x| x.down), h.series(|x| x.up));
            Some(graph(t, &[Series { values: &down, ink: t.blue, wash: true }, Series { values: &up, ink: t.green, wash: false }], HISTORY, h.net_scale(), GRAPH_H))
        }
        _ => None,
    };
    let beside = beside.or_else(|| l.meter.map(|v| el().w_full().h(GRAPH_H).justify_center().child(meter(t, v, l.warn, 6.0))));
    row()
        .key(format!("stat-{}", l.key))
        .w_full()
        .min_h(LINE_H)
        .items_center()
        .gap(16.0)
        .px(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .role("row")
        .label(format!("{} {}", l.label, l.value))
        .child(text(&l.label).font(theme::NOTE).color(t.l2).w(LABEL_W).single_line())
        .child(middle)
        .child(el().grow(1.0).min_w(0.0).children(beside))
}

fn programs(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let mut out = col().key("stats-programs").w_full().px(16.0).pt(18.0).pb(16.0).gap(2.0).child(
        row()
            .h(24.0)
            .items_center()
            .gap(12.0)
            .child(text("Busiest programs").font(theme::GROUP_TITLE).color(t.l1).single_line().grow(1.0))
            .child(text("CPU").font(theme::SMALL).color(t.l2).w(64.0).align(Align::End).single_line())
            .child(text("Memory").font(theme::SMALL).color(t.l2).w(76.0).align(Align::End).single_line()),
    );
    let Some(state) = s.screens.stats.get(&id) else { return out };
    if let Some(note) = state.programs.note() {
        out = out.child(text(note).key("stats-programs-note").font(theme::NOTE).color(t.l2).single_line().pt(4.0));
    }
    for (i, p) in state.programs.list().unwrap_or_default().iter().enumerate() {
        let (name, cpu, memory) = rules::program(p);
        out = out.child(
            row()
                .key(format!("proc-{i}"))
                .h(24.0)
                .items_center()
                .gap(12.0)
                .border_sides([1.0, 0.0, 0.0, 0.0], t.hair2)
                .child(text(s.text_shown(&name).into_owned()).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).min_w(0.0))
                .child(text(cpu).font(theme::LIST_NUMBER).color(t.l2on).w(64.0).align(Align::End).single_line())
                .child(text(memory).font(theme::LIST_NUMBER).color(t.l2on).w(76.0).align(Align::End).single_line()),
        );
    }
    out
}

fn centered(children: Vec<El>) -> El {
    col().key("stats-page").w_full().grow(1.0).min_h(0.0).items_center().justify_center().gap(8.0).px(24.0).children(children)
}

/// The tab's page.
pub fn view(s: &Shell, tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let (id, server) = (tab.id, tab.server.clone());
    let mut bar = row()
        .key("stats-toolbar")
        .w_full()
        .h(TOOLBAR)
        .items_center()
        .gap(10.0)
        .pl(16.0)
        .pr(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .child(text("Stats").font(theme::TEXT_BOLD).color(t.l1).single_line())
        .child(spacer());
    let vitals = s.bench.vitals(&server);
    let facts = match s.screens.machines.get(&server) {
        Some(Ok(f)) => Some(f),
        _ => None,
    };
    if vitals.is_some() {
        bar = bar.child(text("Last 5 minutes").font(theme::NOTE).color(t.l2).single_line());
    }
    let page = col().key("stats").w_full().h_full().child(bar);
    // Why there are no numbers: a machine that can't say, one not connected yet.
    let Some(v) = vitals else {
        let (title, message, terminal) = match s.screens.machines.get(&server) {
            Some(Err(message)) => ("No stats here", message.clone(), false),
            _ if !watched(s, &server) => ("Connect first", "Stats come over the server's own connection. Open a terminal to connect, and they show up here.".to_owned(), true),
            _ => ("Looking…", "The first numbers come in a moment.".to_owned(), false),
        };
        let mut parts = vec![icon(Icon::Stats, 28.0, t.l2), text(title).font(theme::TEXT_BOLD).color(t.l1).single_line(), text(message).font(theme::NOTE).color(t.l2).max_w(380.0).align(Align::Center)];
        if terminal {
            parts.push(push_button(t, "Open a terminal", Push::Primary, true).key("stats-open-terminal").mt(6.0).on_press_tx(move |s: &mut Shell, tx| {
                s.bench.open(TabKind::Terminal, &server);
                s.settle(tx);
            }));
        }
        return page.child(centered(parts));
    };
    let mut table = col().key("stats-table").w_full().role("table").label_static("Stats");
    for l in rules::lines(v, facts) {
        table = table.child(line_view(s, &server, t, &l));
    }
    let name = s.bench.server(&server).map_or_else(|| "This machine".to_owned(), |sv| s.server_shown(&server, &sv.name).into_owned());
    let about = rules::about(v, facts, &name, !s.streaming());
    let body = col()
        .key("stats-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(table)
        .child(text(s.text_shown(&about).into_owned()).key("stats-about").font(theme::NOTE).color(t.l2).wrap().px(16.0).pt(12.0))
        .child(programs(s, id, t));
    page.child(body)
}

/// The load on a server's card in the sidebar: CPU, memory and the fullest disk as meters, a
/// warning past 90%, the uptime. Only while the machine is being looked at, so it's never
/// stale: this computer, or a server with a connected terminal.
pub fn card_load(s: &Shell, server: &str, t: &Theme) -> Option<El> {
    let v = s.bench.vitals(server).filter(|_| watched(s, server))?;
    let load = Load::of(v);
    let mut grid = col().key("card-load").w_full().gap(7.0).mt(4.0).pt(8.0).border_sides([1.0, 0.0, 0.0, 0.0], t.hair);
    for (label, value) in &load.meters {
        let warn = *value >= sshee_app::workbench::vitals::FULL;
        grid = grid.child(
            row()
                .w_full()
                .gap(9.0)
                .items_center()
                .child(text(label).font(theme::SMALL).color(t.l2).w(64.0).single_line().truncate())
                .child(el().grow(1.0).child(meter(t, *value, warn, 4.0)))
                .child(text(sshee_app::workbench::vitals::percent(*value)).font(theme::STATUS_VALUE).color(t.l2on).w(34.0).align(Align::End).single_line()),
        );
    }
    if let Some(warning) = &load.warning {
        grid = grid.child(row().gap(5.0).items_center().child(icon(Icon::Warning, 11.0, t.orange)).child(text(warning).font(theme::SMALL).color(t.l1).single_line().truncate()));
    }
    Some(grid.child(text(load.uptime).font(theme::SMALL).color(t.l2).single_line()))
}
