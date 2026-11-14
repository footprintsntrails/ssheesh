//! AI coding agents in terminals: which one a terminal runs, what its tab is called, and
//! where it stands (working, waiting for you, done, quiet). Without a screen.
//!
//! A port of 0.10's `src/lib/agents.ts` (which program is an agent, what a tab calls it),
//! `tabLabel` in `src/components/Tabs.tsx` ("Terminal 2 (Claude Code)") and the attention
//! parts of `src/lib/terminals.ts` as the window keeps them per tab. The same signals drive
//! it as in 0.10: the foreground program of the terminal (asked from tmux on the server every
//! few seconds), the bell, OSC 9 and OSC 777, and output that stops after a long run nobody
//! watched (`sshee_term::attention`).
//!
//! New in 1.0: [`answer`], the rules of answering an agent's question from its card without
//! going to its tab, and [`Agents::rows`], the list of every agent on every server.

pub mod answer;

use std::collections::HashMap;

use sshee_term::attention::{notice, place_name, Attention, AttentionKind, Notification, NoticeContext, Place, RUN_GAP_MS};
use sshee_term::present::Look;
use sshee_term::question::Asker;

use crate::workbench::{Tab, TabId, TabKind, Workbench};

/// An agent SSHeesh knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Omp,
}

impl AgentKind {
    /// In the order they show up, as in 0.10.
    pub const ALL: [AgentKind; 4] = [AgentKind::Claude, AgentKind::Codex, AgentKind::OpenCode, AgentKind::Omp];

    /// The command that starts it, which is also what tmux names as the terminal's program.
    pub fn command(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::OpenCode => "opencode",
            AgentKind::Omp => "omp",
        }
    }

    /// Its full name: "oh-my-pi".
    pub fn name(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Omp => "oh-my-pi",
        }
    }

    /// What a tab calls it: "Terminal (OMP)".
    pub fn short(self) -> &'static str {
        match self {
            AgentKind::Omp => "OMP",
            other => other.name(),
        }
    }

    /// The agent a terminal runs, judging by its foreground program (0.10's `agentRunning`).
    pub fn running(command: Option<&str>) -> Option<AgentKind> {
        let command = command?.to_lowercase();
        AgentKind::ALL.into_iter().find(|a| a.command() == command)
    }

    /// From the name a tab keeps ([`Tab::agent`]).
    pub fn named(short: &str) -> Option<AgentKind> {
        AgentKind::ALL.into_iter().find(|a| a.short() == short)
    }

    /// The agent in a tab, if it runs one SSHeesh knows.
    pub fn of(tab: &Tab) -> Option<AgentKind> {
        tab.agent.as_deref().and_then(AgentKind::named)
    }

    /// Whose layout its questions are read by.
    pub fn asker(self) -> Asker {
        match self {
            AgentKind::Claude => Asker::ClaudeCode,
            AgentKind::Codex => Asker::Codex,
            AgentKind::OpenCode => Asker::OpenCode,
            AgentKind::Omp => Asker::Omp,
        }
    }
}

/// A tab's name in the row. A terminal with an agent in front says so in brackets, as in
/// 0.10: "Terminal 2 (Claude Code)". A tab with a name of its own ("Migration 0042") keeps
/// it: the agent's mark is then its icon.
pub fn tab_label(tab: &Tab) -> String {
    match (&tab.title, &tab.agent) {
        (None, Some(agent)) if tab.kind == TabKind::Terminal => format!("{} ({agent})", tab.label()),
        _ => tab.label(),
    }
}

/// Where an agent stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentState {
    /// Output is coming.
    Working,
    /// It asked for you: the bell, a notification, a question.
    Waiting,
    /// A long run went quiet while nobody looked: it's probably done.
    Done,
    /// It runs, and nothing is happening.
    Quiet,
}

impl AgentState {
    /// As the list of agents words it.
    pub fn label(self) -> &'static str {
        match self {
            AgentState::Working => "working",
            AgentState::Waiting => "Needs you",
            AgentState::Done => "done",
            AgentState::Quiet => "quiet",
        }
    }
}

/// Where the window stands when a terminal wants attention.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    /// The tab is on screen and the window is in front.
    pub watched: bool,
    /// The window is in front.
    pub app_focused: bool,
    /// Settings: notify when a terminal needs you.
    pub notify: bool,
    pub look: &'a Look,
}

/// What is kept about one terminal.
#[derive(Debug, Clone, Default)]
struct Watched {
    attention: Option<Attention>,
    quiet: sshee_term::attention::QuietWatch,
    last_output: Option<u64>,
}

/// One line of the list of agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub tab: TabId,
    /// The tab's name, as the row of tabs has it.
    pub label: String,
    pub server: String,
    pub prod: bool,
    pub agent: Option<AgentKind>,
    pub state: AgentState,
    /// When it began to wait, in ms.
    pub waiting_since: Option<u64>,
}

/// Every terminal's agent and attention. The window owns one; times are milliseconds on any
/// clock that only goes forward.
#[derive(Debug, Clone, Default)]
pub struct Agents {
    tabs: HashMap<TabId, Watched>,
}

/// The machine a tab is on, for a notification's title: a server is named by its place among
/// the servers while streaming ("Server 2").
fn place_of<'a>(bench: &'a Workbench, tab: &Tab) -> Place<'a> {
    if tab.server == "local" {
        return Place::Local;
    }
    let hosts = bench.servers().iter().filter(|s| !s.is_local());
    match hosts.enumerate().find(|(_, s)| s.key() == tab.server) {
        Some((index, server)) => Place::Host { index, name: &server.name },
        None => Place::Unknown,
    }
}

/// A tab's machine as it may be named: its alias while streaming.
pub fn server_shown(bench: &Workbench, tab: &Tab, streaming: bool) -> String {
    place_name(place_of(bench, tab), streaming)
}

/// "waiting 2 min": how long, in words. Nothing under a minute, when "waiting" is all there
/// is to say.
pub fn waited(since: u64, now: u64) -> String {
    let secs = now.saturating_sub(since) / 1000;
    match secs {
        0..=59 => "waiting".to_owned(),
        60..=3599 => format!("waiting {} min", secs / 60),
        _ => format!("waiting {} h", secs / 3600),
    }
}

impl Agents {
    /// A terminal's foreground program, as tmux on its server names it (`None`: unknown, or
    /// this terminal has no tmux). The tab is named after the agent, or loses the name.
    /// True when the tab changed.
    pub fn set_foreground(&mut self, bench: &mut Workbench, tab: TabId, command: Option<&str>) -> bool {
        let agent = AgentKind::running(command).map(|a| a.short().to_owned());
        let Some(now) = bench.tab(tab) else { return false };
        if now.agent == agent {
            return false;
        }
        bench.set_agent(tab, agent);
        true
    }

    /// Output came in a terminal at `now`; `watched`: you're looking at it.
    pub fn output(&mut self, tab: TabId, now: u64, watched: bool) {
        let w = self.tabs.entry(tab).or_default();
        w.quiet.output(now, watched);
        w.last_output = Some(now);
    }

    /// A program asked for you (`Input`), or a run went quiet (`Done`). The tab says so
    /// unless you're looking at it; the notification, when one is due, is returned for the
    /// system to show (already safe to leave the app: `sshee_term::attention`).
    pub fn ask(&mut self, bench: &mut Workbench, tab: TabId, kind: AttentionKind, message: Option<String>, now: u64, at: &Standing) -> Option<Notification> {
        let place = server_shown(bench, bench.tab(tab)?, at.look.streaming);
        let w = self.tabs.entry(tab).or_default();
        let previous = w.attention.as_ref().map(|a| a.kind);
        let cx = NoticeContext { watched: at.watched, app_focused: at.app_focused, notify: at.notify, previous, place: &place, look: at.look };
        let said = notice(kind, message, now, &cx)?;
        // A second ring while it waits doesn't restart the wait.
        let since = w.attention.as_ref().filter(|a| a.kind == kind).map_or(now, |a| a.at);
        w.attention = Some(Attention { at: since, ..said.attention });
        if kind == AttentionKind::Input {
            bench.set_needs_you(tab, true);
        }
        said.notification
    }

    /// Called every few seconds: the runs that went quiet since, each with its notification
    /// if one is due. `standing` tells where the window stands for a tab.
    pub fn check_quiet<'a>(&mut self, bench: &mut Workbench, now: u64, standing: impl Fn(TabId) -> Standing<'a>) -> Vec<(TabId, Option<Notification>)> {
        let quiet: Vec<TabId> = self.tabs.iter_mut().filter_map(|(id, w)| w.quiet.check(now).then_some(*id)).collect();
        let mut out = Vec::new();
        for tab in quiet {
            // A terminal that waits for you already says the more urgent thing.
            if self.attention(tab).is_some_and(|a| a.kind == AttentionKind::Input) {
                continue;
            }
            let at = standing(tab);
            if at.watched {
                continue;
            }
            out.push((tab, self.ask(bench, tab, AttentionKind::Done, None, now, &at)));
        }
        out
    }

    /// You landed on a tab: what it asked for is seen.
    pub fn seen(&mut self, tab: TabId) {
        if let Some(w) = self.tabs.get_mut(&tab) {
            w.attention = None;
        }
    }

    /// The tabs that are gone are forgotten.
    pub fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.tabs.retain(|id, _| open(*id));
    }

    pub fn attention(&self, tab: TabId) -> Option<&Attention> {
        self.tabs.get(&tab).and_then(|w| w.attention.as_ref())
    }

    /// A tab that says "Needs you" has been waiting since `since` (a scene set up from
    /// outside: a demo's).
    pub fn waiting_from(&mut self, tab: TabId, since: u64) {
        self.tabs.entry(tab).or_default().attention = Some(Attention { kind: AttentionKind::Input, message: None, at: since });
    }

    /// When a tab began to wait for you.
    pub fn waiting_since(&self, tab: TabId) -> Option<u64> {
        self.attention(tab).filter(|a| a.kind == AttentionKind::Input).map(|a| a.at)
    }

    /// Output came in the last few seconds.
    fn busy(&self, tab: TabId, now: u64) -> bool {
        self.tabs.get(&tab).and_then(|w| w.last_output).is_some_and(|at| now.saturating_sub(at) <= RUN_GAP_MS)
    }

    /// When a tab that is working now will count as quiet, if nothing more comes.
    pub fn busy_until(&self, tab: TabId, now: u64) -> Option<u64> {
        let at = self.tabs.get(&tab)?.last_output?;
        (now.saturating_sub(at) <= RUN_GAP_MS).then_some(at + RUN_GAP_MS + 1)
    }

    /// Where a tab's agent stands; `None` for a tab with no agent that asks for nothing.
    pub fn state(&self, bench: &Workbench, tab: TabId, now: u64) -> Option<AgentState> {
        let t = bench.tab(tab)?;
        if t.needs_you {
            return Some(AgentState::Waiting);
        }
        t.agent.as_ref()?;
        if self.busy(tab, now) {
            return Some(AgentState::Working);
        }
        Some(match self.attention(tab).map(|a| a.kind) {
            Some(AttentionKind::Done) => AgentState::Done,
            _ => AgentState::Quiet,
        })
    }

    /// The quiet working mark on a tab: an agent runs there and output is coming.
    pub fn working(&self, bench: &Workbench, tab: TabId, now: u64) -> bool {
        self.state(bench, tab, now) == Some(AgentState::Working)
    }

    /// Every agent on every server, and every terminal that waits for you (0.10's "Running
    /// now"): the waiting ones first, the longest wait at the top, then the rest in the row's
    /// order.
    pub fn rows(&self, bench: &Workbench, now: u64) -> Vec<AgentRow> {
        let mut rows: Vec<AgentRow> = bench
            .tabs()
            .iter()
            .filter(|t| t.kind == TabKind::Terminal)
            .filter_map(|t| {
                let state = self.state(bench, t.id, now)?;
                let server = bench.server(&t.server);
                Some(AgentRow {
                    tab: t.id,
                    label: tab_label(t),
                    server: server.map_or("Unknown server", |s| s.name.as_str()).to_owned(),
                    prod: server.is_some_and(|s| s.is_prod()),
                    agent: AgentKind::of(t),
                    state,
                    waiting_since: (state == AgentState::Waiting).then(|| self.waiting_since(t.id)).flatten(),
                })
            })
            .collect();
        // A stable sort: the row's order holds within each group.
        rows.sort_by_key(|r| (r.state != AgentState::Waiting, r.waiting_since.unwrap_or(u64::MAX)));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EnvLabel;
    use crate::workbench::{Open, Server};
    use sshee_term::attention::QUIET_MS;

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    fn bench() -> Workbench {
        Workbench::new(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod),
            Server::host("staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod),
        ])
    }

    fn away(look: &Look) -> Standing<'_> {
        Standing { watched: false, app_focused: false, notify: true, look }
    }

    #[test]
    fn agents_are_known_by_the_command_that_starts_them() {
        assert_eq!(AgentKind::running(Some("claude")), Some(AgentKind::Claude));
        assert_eq!(AgentKind::running(Some("Codex")), Some(AgentKind::Codex));
        assert_eq!(AgentKind::running(Some("opencode")), Some(AgentKind::OpenCode));
        assert_eq!(AgentKind::running(Some("omp")), Some(AgentKind::Omp));
        for other in ["zsh", "vim", "node", "claude-old", ""] {
            assert_eq!(AgentKind::running(Some(other)), None, "{other}");
        }
        assert_eq!(AgentKind::running(None), None);
        // A tab calls oh-my-pi by its short name, and the name reads back.
        assert_eq!((AgentKind::Omp.name(), AgentKind::Omp.short()), ("oh-my-pi", "OMP"));
        for agent in AgentKind::ALL {
            assert_eq!(AgentKind::named(agent.short()), Some(agent));
        }
        assert_eq!(AgentKind::named("Someone else"), None);
    }

    #[test]
    fn a_tab_is_named_after_its_agent_as_in_0_10() {
        let mut w = bench();
        let mut agents = Agents::default();
        let first = w.go_to(WEB);
        let second = w.new_terminal();
        let label = |w: &Workbench, id| tab_label(w.tab(id).unwrap());
        assert_eq!(label(&w, second), "Terminal 2");
        assert!(agents.set_foreground(&mut w, second, Some("claude")));
        assert_eq!(label(&w, second), "Terminal 2 (Claude Code)");
        assert_eq!(label(&w, first), "Terminal");
        assert!(agents.set_foreground(&mut w, first, Some("omp")));
        assert_eq!(label(&w, first), "Terminal (OMP)");
        // The same again changes nothing; back at the shell the name goes.
        assert!(!agents.set_foreground(&mut w, first, Some("omp")));
        assert!(agents.set_foreground(&mut w, first, Some("zsh")));
        assert_eq!(label(&w, first), "Terminal");
        assert!(!agents.set_foreground(&mut w, first, None));
        // A tab with a name of its own keeps it; its icon says there's an agent.
        let named = w.open_with(TabKind::Terminal, DB, Open { title: Some("Migration 0042".into()), agent: Some("Claude Code".into()), ..Open::default() });
        assert_eq!(label(&w, named), "Migration 0042");
        assert_eq!(AgentKind::of(w.tab(named).unwrap()), Some(AgentKind::Claude));
        // Files is never an agent's.
        let files = w.open(TabKind::Files, WEB);
        assert_eq!(label(&w, files), "Files");
        // A tab that's gone is ignored.
        assert!(!agents.set_foreground(&mut w, TabId(999), Some("claude")));
    }

    #[test]
    fn a_bell_makes_the_tab_wait_and_notifies_once() {
        let mut w = bench();
        let mut agents = Agents::default();
        let look = Look::default();
        let db = w.go_to(DB);
        w.go_to(WEB);
        agents.set_foreground(&mut w, db, Some("claude"));
        let note = agents.ask(&mut w, db, AttentionKind::Input, Some("Claude Code: Run the migration?".into()), 1_000, &away(&look)).unwrap();
        assert_eq!(note.title, "db-main needs you");
        assert_eq!(note.body, "Claude Code: Run the migration?");
        assert!(w.tab(db).unwrap().needs_you && w.server_needs_you(DB));
        assert_eq!(agents.state(&w, db, 1_000), Some(AgentState::Waiting));
        // It rings again while it waits: no second notification, and the wait isn't restarted.
        assert_eq!(agents.ask(&mut w, db, AttentionKind::Input, None, 61_000, &away(&look)), None);
        assert_eq!(agents.waiting_since(db), Some(1_000));
        assert_eq!(waited(agents.waiting_since(db).unwrap(), 125_000), "waiting 2 min");
        // You land on the tab: seen.
        w.go_to(DB);
        agents.seen(db);
        assert_eq!(agents.attention(db), None);
        assert_eq!(agents.state(&w, db, 125_000), Some(AgentState::Quiet));
    }

    #[test]
    fn nothing_is_said_about_a_tab_you_are_looking_at() {
        let mut w = bench();
        let mut agents = Agents::default();
        let look = Look::default();
        let db = w.go_to(DB);
        let here = Standing { watched: true, app_focused: true, notify: true, look: &look };
        assert_eq!(agents.ask(&mut w, db, AttentionKind::Input, None, 5, &here), None);
        assert_eq!(agents.attention(db), None);
        assert!(!w.tab(db).unwrap().needs_you);
        // In front but on another tab: the tab says it, the system doesn't.
        w.go_to(WEB);
        let elsewhere = Standing { watched: false, app_focused: true, notify: true, look: &look };
        assert_eq!(agents.ask(&mut w, db, AttentionKind::Input, None, 6, &elsewhere), None);
        assert!(w.tab(db).unwrap().needs_you);
        // Notifications turned off in Settings: the same.
        let web = w.active_id().unwrap();
        w.go_to(DB);
        let off = Standing { watched: false, app_focused: false, notify: false, look: &look };
        assert_eq!(agents.ask(&mut w, web, AttentionKind::Input, None, 7, &off), None);
        assert!(w.tab(web).unwrap().needs_you);
    }

    #[test]
    fn a_notification_never_carries_a_secret_or_a_name_while_streaming() {
        let mut w = bench();
        let mut agents = Agents::default();
        let db = w.go_to(DB);
        w.go_to(WEB);
        // Made at run time: no secret is written in this file.
        let token = format!("ghp_{}", "a1B2c3D4e5".repeat(4));
        let look = Look::default();
        let note = agents.ask(&mut w, db, AttentionKind::Input, Some(format!("push with {token}?")), 1, &away(&look)).unwrap();
        assert!(!note.body.contains(&token), "{}", note.body);
        assert!(note.body.contains('•'));
        // Streaming: the server is "Server 3" (its place among the servers).
        agents.seen(db);
        let streaming = Look { streaming: true, ..Look::default() };
        let note = agents.ask(&mut w, db, AttentionKind::Input, None, 2, &away(&streaming)).unwrap();
        assert_eq!(note.title, "Server 3 needs you");
        assert_eq!(server_shown(&w, w.tab(db).unwrap(), true), "Server 3");
        assert_eq!(server_shown(&w, w.tab(db).unwrap(), false), "db-main");
        let local = w.go_to("local");
        assert_eq!(server_shown(&w, w.tab(local).unwrap(), true), "This computer");
    }

    #[test]
    fn an_agent_works_while_output_comes_and_goes_quiet_after() {
        let mut w = bench();
        let mut agents = Agents::default();
        let look = Look::default();
        let db = w.go_to(DB);
        w.go_to(WEB);
        // No agent: no state, whatever it prints.
        agents.output(db, 0, false);
        assert_eq!(agents.state(&w, db, 0), None);
        agents.set_foreground(&mut w, db, Some("codex"));
        assert_eq!(agents.state(&w, db, 100), Some(AgentState::Working));
        assert!(agents.working(&w, db, 100));
        assert_eq!(agents.busy_until(db, 100), Some(RUN_GAP_MS + 1));
        // A long run: output every second for twenty seconds, then nothing.
        for t in (1_000..=20_000).step_by(1_000) {
            agents.output(db, t, false);
            assert!(agents.check_quiet(&mut w, t, |_| away(&look)).is_empty());
        }
        assert_eq!(agents.state(&w, db, 20_000 + RUN_GAP_MS + 1), Some(AgentState::Quiet));
        assert_eq!(agents.busy_until(db, 20_000 + RUN_GAP_MS + 1), None);
        let quiet = agents.check_quiet(&mut w, 20_000 + QUIET_MS, |_| away(&look));
        assert_eq!(quiet.len(), 1);
        assert_eq!(quiet[0].0, db);
        assert_eq!(quiet[0].1.as_ref().map(|n| n.title.as_str()), Some("db-main went quiet"));
        assert_eq!(agents.state(&w, db, 20_000 + QUIET_MS), Some(AgentState::Done));
        // Said once; "went quiet" isn't "needs you".
        assert!(agents.check_quiet(&mut w, 40_000, |_| away(&look)).is_empty());
        assert!(!w.tab(db).unwrap().needs_you);
        // Output again: working.
        agents.output(db, 50_000, false);
        assert_eq!(agents.state(&w, db, 50_001), Some(AgentState::Working));
    }

    #[test]
    fn a_run_you_watched_never_goes_quiet() {
        let mut w = bench();
        let mut agents = Agents::default();
        let look = Look::default();
        let db = w.go_to(DB);
        for t in (0..=20_000).step_by(1_000) {
            agents.output(db, t, true);
        }
        assert!(agents.check_quiet(&mut w, 60_000, |_| away(&look)).is_empty());
    }

    #[test]
    fn the_list_has_every_agent_and_the_waiting_ones_first() {
        let mut w = bench();
        let mut agents = Agents::default();
        let look = Look::default();
        let shell = w.go_to(WEB);
        let codex = w.new_terminal();
        let claude = w.open_with(TabKind::Terminal, DB, Open { title: Some("Migration 0042".into()), ..Open::default() });
        let omp = w.go_to("host:staging");
        w.open(TabKind::Files, WEB);
        let plain = w.go_to("local");
        agents.set_foreground(&mut w, codex, Some("codex"));
        agents.set_foreground(&mut w, claude, Some("claude"));
        agents.set_foreground(&mut w, omp, Some("omp"));
        agents.output(codex, 9_000, false);
        // oh-my-pi asked first, Claude Code a minute later; a plain shell rang its bell last.
        agents.ask(&mut w, omp, AttentionKind::Input, None, 1_000, &away(&look));
        agents.ask(&mut w, claude, AttentionKind::Input, None, 61_000, &away(&look));
        w.go_to(WEB);
        agents.ask(&mut w, plain, AttentionKind::Input, None, 62_000, &away(&look));
        let rows = agents.rows(&w, 10_000);
        let seen: Vec<(TabId, &str, &str, AgentState)> = rows.iter().map(|r| (r.tab, r.label.as_str(), r.server.as_str(), r.state)).collect();
        assert_eq!(
            seen,
            [
                (omp, "Terminal (OMP)", "web-staging", AgentState::Waiting),
                (claude, "Migration 0042", "db-main", AgentState::Waiting),
                (plain, "Terminal", "This computer", AgentState::Waiting),
                (codex, "Terminal 2 (Codex)", "web-7731", AgentState::Working),
            ]
        );
        assert_eq!(rows[1].agent, Some(AgentKind::Claude));
        assert_eq!((rows[1].prod, rows[0].prod), (true, false));
        assert_eq!(rows[2].agent, None);
        // The shell that asks for nothing isn't in it.
        assert!(rows.iter().all(|r| r.tab != shell));
        // Closed tabs are forgotten.
        w.close(omp);
        agents.retain(|id| w.tab(id).is_some());
        assert_eq!(agents.rows(&w, 10_000).len(), 3);
        assert_eq!(agents.attention(omp), None);
    }

    #[test]
    fn waiting_reads_in_minutes_and_hours() {
        assert_eq!(waited(0, 59_000), "waiting");
        assert_eq!(waited(0, 60_000), "waiting 1 min");
        assert_eq!(waited(1_000, 125_000), "waiting 2 min");
        assert_eq!(waited(0, 2 * 3_600_000 + 5), "waiting 2 h");
        assert_eq!(waited(9, 3), "waiting");
    }
}
