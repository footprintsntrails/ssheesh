//! A machine's Docker and Podman containers without a screen: what a Containers tab asks the
//! backend for and hears back, what it keeps between two looks, and every sentence it shows.
//!
//! A port of 0.10's container tree in the sidebar (`ContainerTree` and `ContainerRow` in
//! `src/components/Sidebar.tsx`, on sshee-core's `containers`), as a tab of its own:
//!
//! - the containers come grouped by Compose project, projects first, running before stopped;
//! - a look is taken when the tab comes up and every 10 seconds while it's on screen, later
//!   while looks keep failing (up to a minute), at once after a start, stop or restart;
//! - a running container offers Restart and Stop, a stopped one Start; any of them opens its
//!   logs (followed) or, while it runs, a shell inside it, each in a new terminal tab;
//! - what goes wrong is said in plain words with what to do: no Docker (and the command that
//!   installs it), a user who isn't allowed (and the command that allows it), Docker that
//!   isn't running (and the command that starts it), a server that wants you to sign in first.
//!
//! sshee-core's types are mirrored here; `desktop/app` converts them. Nothing here waits.

use crate::files::{Failure, FailureKind};
use crate::model::{ContainerRef, ContainerRun, Engine, JsonMap};
use crate::workbench::{ServerKey, TabId};

/// Between two looks while the tab is on screen, in seconds (0.10's `POLL_MS`).
pub const POLL_SECS: f64 = 10.0;
/// The longest wait between looks while they keep failing (0.10's `MAX_POLL_MS`).
pub const MAX_POLL_SECS: f64 = 60.0;
/// Docker's own install script, for a server without Docker or Podman (0.10's).
pub const DOCKER_INSTALL: &str = "curl -fsSL https://get.docker.com | sudo sh";
/// The button that offers a terminal to sign in or confirm the server in.
pub const OPEN_TERMINAL: &str = "Open a terminal";
/// How long a look may go unanswered before the tab says so, in seconds: a look that never
/// answers ends in "Couldn't look for containers" and Try again, never an endless wait.
pub const LOOK_TIMEOUT_SECS: f64 = 30.0;
/// What a look that went unanswered says.
pub const NO_ANSWER: &str = "The server didn't answer within 30 seconds. It may be busy, or the connection dropped.";

/// The short guide an empty list offers: each step's title, its command and what it does.
pub const FIRST_CONTAINER: [(&str, &str, &str); 2] = [
    (
        "Start one container",
        "docker run -d --name hello -p 8080:80 nginx",
        "Downloads the nginx web server and runs it in the background under the name hello. Port 8080 on the server leads to port 80 inside it, so http://your-server:8080 shows its welcome page.",
    ),
    (
        "Or start a whole project",
        "docker compose up -d",
        "Run it in a folder with a docker-compose.yml file: it starts every container the file describes, in the background, and they show up here under the project's name.",
    ),
];
/// Under the guide's steps.
pub const FIRST_CONTAINER_AFTER: &str = "New containers show up here within 10 seconds. To remove hello again: docker rm -f hello";

/// One container, as sshee-core's `containers::list` gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub engine: Engine,
    /// The first 12 characters of its id.
    pub id: String,
    pub name: String,
    pub image: String,
    pub running: bool,
    /// The engine's word for its state: running, exited, paused, restarting, created...
    pub state: String,
    /// The engine's own line: "Up 3 days (healthy)", "Exited (0) 6 hours ago".
    pub status: String,
    /// The engine's own list: "0.0.0.0:8080->8080/tcp, 5432/tcp".
    pub ports: String,
    /// Its Docker Compose project, when it belongs to one.
    pub project: Option<String>,
}

/// What a look at a machine found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerList {
    /// The engines installed there.
    pub engines: Vec<Engine>,
    pub containers: Vec<Container>,
    /// Engines installed but not usable, and why (sshee-core's sentences).
    pub problems: Vec<String>,
}

/// What a button does to a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    /// The button's word.
    pub fn label(self) -> &'static str {
        match self {
            Action::Start => "Start",
            Action::Stop => "Stop",
            Action::Restart => "Restart",
        }
    }

    /// While it runs, in place of the row's state.
    pub fn doing(self) -> &'static str {
        match self {
            Action::Start => "Starting…",
            Action::Stop => "Stopping…",
            Action::Restart => "Restarting…",
        }
    }

    fn done(self) -> &'static str {
        match self {
            Action::Start => "Started",
            Action::Stop => "Stopped",
            Action::Restart => "Restarted",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        }
    }
}

/// The terminal a container opens in: a shell inside it, or its logs as they come.
pub fn inside(c: &Container, run: ContainerRun) -> ContainerRef {
    ContainerRef { engine: c.engine.clone(), id: c.id.clone(), name: c.name.clone(), run, extra: JsonMap::new() }
}

/// The name of a tab opened into a container: "app-api-1", "app-api-1 logs".
pub fn tab_title(c: &ContainerRef) -> String {
    match c.run {
        ContainerRun::Logs => format!("{} logs", c.name),
        _ => c.name.clone(),
    }
}

/// The engine's name in a sentence.
pub fn engine_name(engine: &Engine) -> &str {
    match engine {
        Engine::Docker => "Docker",
        Engine::Podman => "Podman",
        Engine::Other(other) => other,
    }
}

// ---- what the tab asks and hears

/// A request from one Containers tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub tab: TabId,
    pub server: ServerKey,
    pub want: Want,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Want {
    /// A look at every container. Only the answer to the newest ticket counts.
    List { ticket: u64 },
    /// Starts, stops or restarts one container.
    Act { engine: Engine, id: String, name: String, action: Action },
}

/// The backend's answer, addressed to the tab that asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub tab: TabId,
    pub server: ServerKey,
    pub answer: Answer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Listed { ticket: u64, result: Result<ContainerList, Failure> },
    Acted { id: String, name: String, action: Action, result: Result<(), Failure> },
}

impl Ask {
    pub fn reply(&self, answer: Answer) -> Reply {
        Reply { tab: self.tab, server: self.server.clone(), answer }
    }
}

// ---- one container as a row

/// Where a container is, in words a row can colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Paused,
    Restarting,
    Stopped,
    /// Made, never started.
    Created,
}

/// What a running container's health check says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Unhealthy,
    Starting,
}

/// A container as its row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: State,
    /// "Up 3 days", "Stopped 6 hours ago", "Stopped 2 days ago, exit code 137", "Starting…".
    pub status: String,
    pub health: Option<Health>,
    /// "8080 → 8080", "127.0.0.1:5432 → 5432", "6379 inside".
    pub ports: Vec<String>,
    /// A start, stop or restart on its way.
    pub busy: Option<Action>,
    /// The buttons it offers, in order.
    pub actions: Vec<Action>,
    /// A shell can be opened in it: it runs.
    pub can_shell: bool,
}

/// Reads the engine's status line: (how long, or since when), its health, its exit code.
fn read_status(status: &str) -> (String, Option<Health>, Option<i32>) {
    let s = status.trim();
    let mut health = None;
    let mut rest = s.to_owned();
    for (mark, h) in [("(healthy)", Health::Healthy), ("(unhealthy)", Health::Unhealthy), ("(health: starting)", Health::Starting)] {
        if let Some(at) = rest.find(mark) {
            health = Some(h);
            rest.replace_range(at..at + mark.len(), "");
        }
    }
    let rest = rest.replace("(Paused)", "");
    let rest = rest.trim();
    if let Some(up) = rest.strip_prefix("Up ") {
        // Podman says "Up 3 days ago"; "About an hour" reads better in lower case.
        let up = up.trim().trim_end_matches(" ago").trim();
        let up = lower_first(up);
        return (up, health, None);
    }
    for word in ["Exited", "Restarting"] {
        if let Some(after) = rest.strip_prefix(word) {
            let after = after.trim();
            let (code, when) = match after.strip_prefix('(').and_then(|a| a.split_once(')')) {
                Some((code, when)) => (code.trim().parse().ok(), when.trim()),
                None => (None, after),
            };
            return (lower_first(when), health, code);
        }
    }
    (String::new(), health, None)
}

fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The engine's port list, short: published ports as "host → container", the rest "inside".
/// The same port published on IPv4 and IPv6 shows once.
pub fn ports(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (spec, proto) = match part.rsplit_once('/') {
            Some((spec, proto)) => (spec, proto),
            None => (part, "tcp"),
        };
        let proto = if proto == "tcp" { String::new() } else { format!("/{proto}") };
        let shown = match spec.split_once("->") {
            Some((public, private)) => {
                // "0.0.0.0:8080", "[::]:8080", ":::8080", "127.0.0.1:5432".
                let (address, port) = public.rsplit_once(':').unwrap_or(("", public));
                let address = address.trim_matches(['[', ']']);
                let everywhere = matches!(address, "" | "0.0.0.0" | "::" | ":");
                if everywhere {
                    format!("{port} → {private}{proto}")
                } else {
                    format!("{address}:{port} → {private}{proto}")
                }
            }
            None => format!("{spec}{proto} inside"),
        };
        if !out.contains(&shown) {
            out.push(shown);
        }
    }
    out
}

impl Row {
    pub fn of(c: &Container, busy: Option<Action>) -> Row {
        let (when, health, code) = read_status(&c.status);
        let state = match c.state.as_str() {
            "running" if c.status.contains("(Paused)") => State::Paused,
            "running" => State::Running,
            "paused" => State::Paused,
            "restarting" => State::Restarting,
            "created" => State::Created,
            _ if c.running => State::Running,
            _ => State::Stopped,
        };
        let status = match busy {
            Some(action) => action.doing().to_owned(),
            None => match state {
                State::Running if when.is_empty() => "Running".to_owned(),
                State::Running => format!("Up {when}"),
                State::Paused => "Paused".to_owned(),
                State::Restarting => "Restarting".to_owned(),
                State::Created => "Never started".to_owned(),
                State::Stopped => {
                    let mut s = if when.is_empty() { "Stopped".to_owned() } else { format!("Stopped {when}") };
                    if let Some(code) = code.filter(|c| *c != 0) {
                        s.push_str(&format!(", exit code {code}"));
                    }
                    s
                }
            },
        };
        let actions = match state {
            State::Running => vec![Action::Restart, Action::Stop],
            State::Paused | State::Restarting => vec![Action::Stop],
            State::Stopped | State::Created => vec![Action::Start],
        };
        Row {
            id: c.id.clone(),
            name: c.name.clone(),
            image: c.image.clone(),
            state,
            status,
            health: health.filter(|_| state == State::Running),
            ports: ports(&c.ports),
            busy,
            actions,
            can_shell: state == State::Running,
        }
    }
}

/// A Compose project's containers, or those in none.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The project's name; `None`: containers in no project.
    pub project: Option<String>,
    /// "3 running", "2 of 3 running", "Stopped".
    pub summary: String,
    pub rows: Vec<Row>,
}

/// How a tab says something went wrong: a page in place of the list, or a line over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Nothing to list. `command` is one to copy or type into a terminal there; with
    /// `open_terminal`, a terminal is where it's fixed (signing in, the host key).
    Page { title: String, message: String, command: Option<String>, open_terminal: bool },
    /// A list stays (or there is none yet) and this is said over it.
    Note { text: String, command: Option<String> },
}

/// An engine's trouble, from sshee-core's sentence, with the command that fixes it.
pub fn explain(problem: &str) -> (String, Option<String>) {
    let l = problem.to_ascii_lowercase();
    let bin = if l.starts_with("podman") { "podman" } else { "docker" };
    if l.contains("usermod") {
        // sshee-core's own sentence already says what to do; the command is what to copy.
        return (problem.to_owned(), Some(format!("sudo usermod -aG {bin} $USER")));
    }
    if l.contains("not running") {
        let name = if bin == "podman" { "Podman" } else { "Docker" };
        let unit = if bin == "podman" { "podman.socket" } else { "docker" };
        return (format!("{name} is installed but not running. Start it, and it starts with the server from then on:"), Some(format!("sudo systemctl enable --now {unit}")));
    }
    (problem.to_owned(), None)
}

/// How a last start, stop or restart went, until the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub ok: bool,
    pub text: String,
}

/// One Containers tab: the last look, the one on its way, what's being done to which container.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    list: Option<ContainerList>,
    failure: Option<Failure>,
    /// The newest look asked for, and whether its answer is still to come.
    ticket: u64,
    loading: bool,
    /// Looks that failed in a row.
    failures: u32,
    /// Starts, stops and restarts on their way, by container id.
    busy: Vec<(String, Action)>,
    note: Option<Note>,
}

impl Board {
    /// A look starts: what to ask for.
    pub fn list(&mut self) -> Want {
        self.ticket += 1;
        self.loading = true;
        Want::List { ticket: self.ticket }
    }

    /// No look has come back yet.
    pub fn is_first_look(&self) -> bool {
        self.list.is_none() && self.failure.is_none()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// A look came back. An answer to an older ticket is dropped; one to the newest ticket
    /// counts even after [`Board::no_answer`] gave up on it. A failure keeps the list
    /// shown last: the machine can't be asked right now, which says nothing of its containers.
    pub fn listed(&mut self, ticket: u64, result: Result<ContainerList, Failure>) -> bool {
        if ticket != self.ticket {
            return false;
        }
        self.loading = false;
        match result {
            Ok(list) => {
                self.list = Some(list);
                self.failure = None;
                self.failures = 0;
            }
            Err(failure) => {
                self.failure = Some(failure);
                self.failures += 1;
            }
        }
        true
    }

    /// The look on its way went unanswered for [`LOOK_TIMEOUT_SECS`]: it counts as a failed
    /// one, so the tab says so and offers Try again. Its answer still counts if it comes late.
    /// False when no look was on its way.
    pub fn no_answer(&mut self) -> bool {
        if !self.loading {
            return false;
        }
        self.loading = false;
        self.failure = Some(Failure::new(FailureKind::Failed, NO_ANSWER));
        self.failures += 1;
        true
    }

    /// How long until the next look: 10 seconds, twice as long after each failure, a minute at most.
    pub fn next_look_in(&self) -> f64 {
        (POLL_SECS * 2f64.powi(self.failures.min(8) as i32)).min(MAX_POLL_SECS)
    }

    /// A button was pressed on a container: what to ask for, unless something is already
    /// being done to it or it isn't in the list.
    pub fn act(&mut self, id: &str, action: Action) -> Option<Want> {
        if self.busy.iter().any(|(b, _)| b == id) {
            return None;
        }
        let c = self.list.as_ref()?.containers.iter().find(|c| c.id == id)?;
        self.busy.push((id.to_owned(), action));
        self.note = None;
        Some(Want::Act { engine: c.engine.clone(), id: c.id.clone(), name: c.name.clone(), action })
    }

    /// It's done, or it failed: the note says which. Look again after it, either way.
    pub fn acted(&mut self, id: &str, name: &str, action: Action, result: Result<(), Failure>) {
        self.busy.retain(|(b, _)| b != id);
        self.note = Some(match result {
            Ok(()) => Note { ok: true, text: format!("{} {name}", action.done()) },
            Err(f) => Note { ok: false, text: format!("Couldn't {} {name}: {}", action.verb(), f.message.trim()) },
        });
    }

    pub fn note(&self) -> Option<&Note> {
        self.note.as_ref()
    }

    pub fn dismiss_note(&mut self) {
        self.note = None;
    }

    pub fn container(&self, id: &str) -> Option<&Container> {
        self.list.as_ref()?.containers.iter().find(|c| c.id == id)
    }

    /// The containers by project: projects in alphabetical order, then those in none; within
    /// each, running first, then by name (sshee-core's order).
    pub fn groups(&self) -> Vec<Group> {
        let Some(list) = &self.list else { return Vec::new() };
        let mut projects: Vec<Option<String>> = Vec::new();
        for c in &list.containers {
            if !projects.contains(&c.project) {
                projects.push(c.project.clone());
            }
        }
        projects.sort_by(|a, b| (a.is_none(), a.as_deref().map(str::to_lowercase)).cmp(&(b.is_none(), b.as_deref().map(str::to_lowercase))));
        projects
            .into_iter()
            .map(|project| {
                let mut members: Vec<&Container> = list.containers.iter().filter(|c| c.project == project).collect();
                members.sort_by_key(|c| (!c.running, c.name.to_lowercase()));
                let running = members.iter().filter(|c| c.running).count();
                let summary = match (running, members.len()) {
                    (0, _) => "Stopped".to_owned(),
                    (r, n) if r == n => format!("{n} running"),
                    (r, n) => format!("{r} of {n} running"),
                };
                let rows = members.into_iter().map(|c| Row::of(c, self.busy.iter().find(|(b, _)| *b == c.id).map(|(_, a)| *a))).collect();
                Group { project, summary, rows }
            })
            .collect()
    }

    /// "4 containers, 3 running", once a look came back with some.
    pub fn summary(&self) -> Option<String> {
        let list = self.list.as_ref().filter(|l| !l.containers.is_empty())?;
        let n = list.containers.len();
        let running = list.containers.iter().filter(|c| c.running).count();
        let what = if n == 1 { "1 container".to_owned() } else { format!("{n} containers") };
        Some(format!("{what}, {running} running"))
    }

    /// What to say instead of the list, or over it.
    pub fn problem(&self) -> Option<Problem> {
        if let Some(f) = &self.failure {
            let page = |title: &str, message: &str, open_terminal: bool| Problem::Page { title: title.into(), message: message.into(), command: None, open_terminal };
            let shown = match f.kind {
                FailureKind::Auth => page("Sign in first", "Open a terminal to sign in, then containers show up here.", true),
                FailureKind::HostKey => page("Confirm this server first", "Open a terminal to confirm this server's identity, then containers show up here.", true),
                FailureKind::Network if self.list.is_some() => Problem::Note { text: "Can't reach the server right now. These are the containers seen last.".into(), command: None },
                FailureKind::Network => page("Can't reach the server", f.message.trim(), false),
                _ if self.list.is_some() => Problem::Note { text: format!("Couldn't look again: {}", f.message.trim()), command: None },
                _ => page("Couldn't look for containers", f.message.trim(), false),
            };
            // An Auth or HostKey page wins over a list seen before: it can't be refreshed.
            if matches!(shown, Problem::Page { .. }) || self.list.is_some() {
                return Some(shown);
            }
        }
        let list = self.list.as_ref()?;
        if list.engines.is_empty() && list.problems.is_empty() {
            return Some(Problem::Page {
                title: "No Docker here".into(),
                message: "Neither Docker nor Podman is installed on this machine. To install Docker, run this in a terminal there:".into(),
                command: Some(DOCKER_INSTALL.into()),
                open_terminal: false,
            });
        }
        if let Some(first) = list.problems.first() {
            let (text, command) = explain(first);
            if list.containers.is_empty() {
                let title = if text.contains("not running") { "Docker isn't running" } else { "Docker can't be used yet" };
                let title = if first.to_ascii_lowercase().starts_with("podman") { title.replace("Docker", "Podman") } else { title.to_owned() };
                return Some(Problem::Page { title, message: text, command, open_terminal: false });
            }
            return Some(Problem::Note { text, command });
        }
        None
    }

    /// Said in place of an empty list: an engine is there, with no containers yet.
    pub fn empty(&self) -> Option<(&'static str, &'static str)> {
        let list = self.list.as_ref()?;
        (list.containers.is_empty() && !list.engines.is_empty() && list.problems.is_empty())
            .then_some(("No containers yet", "Start some with docker compose up -d, and they show up here."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, project: Option<&str>, state: &str, status: &str, ports: &str) -> Container {
        Container {
            engine: Engine::Docker,
            id: format!("{:0>12}", name.len()),
            name: name.into(),
            image: format!("{name}:latest"),
            running: state == "running",
            state: state.into(),
            status: status.into(),
            ports: ports.into(),
            project: project.map(str::to_owned),
        }
    }

    fn with_ids(mut list: Vec<Container>) -> Vec<Container> {
        for (i, c) in list.iter_mut().enumerate() {
            c.id = format!("a1b2c3d4e5f{i}");
        }
        list
    }

    fn board(containers: Vec<Container>) -> Board {
        let mut b = Board::default();
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Ok(ContainerList { engines: vec![Engine::Docker], containers: with_ids(containers), problems: vec![] }));
        b
    }

    #[test]
    fn statuses_read_as_words() {
        let row = |state: &str, status: &str| {
            let r = Row::of(&c("x", None, state, status, ""), None);
            (r.state, r.status, r.health)
        };
        assert_eq!(row("running", "Up 3 days (healthy)"), (State::Running, "Up 3 days".into(), Some(Health::Healthy)));
        assert_eq!(row("running", "Up About an hour"), (State::Running, "Up about an hour".into(), None));
        assert_eq!(row("running", "Up 5 minutes (health: starting)"), (State::Running, "Up 5 minutes".into(), Some(Health::Starting)));
        assert_eq!(row("running", "Up 2 hours (unhealthy)"), (State::Running, "Up 2 hours".into(), Some(Health::Unhealthy)));
        // Podman's way of saying it.
        assert_eq!(row("running", "Up 3 days ago"), (State::Running, "Up 3 days".into(), None));
        assert_eq!(row("exited", "Exited (0) 6 hours ago"), (State::Stopped, "Stopped 6 hours ago".into(), None));
        assert_eq!(row("exited", "Exited (137) 2 days ago"), (State::Stopped, "Stopped 2 days ago, exit code 137".into(), None));
        assert_eq!(row("running", "Up 2 hours (Paused)"), (State::Paused, "Paused".into(), None));
        assert_eq!(row("paused", "Up 2 hours (Paused)"), (State::Paused, "Paused".into(), None));
        assert_eq!(row("restarting", "Restarting (1) 5 seconds ago"), (State::Restarting, "Restarting".into(), None));
        assert_eq!(row("created", "Created"), (State::Created, "Never started".into(), None));
        assert_eq!(row("running", ""), (State::Running, "Running".into(), None));
    }

    #[test]
    fn each_state_offers_its_buttons() {
        let actions = |state: &str| {
            let r = Row::of(&c("x", None, state, "", ""), None);
            (r.actions, r.can_shell)
        };
        assert_eq!(actions("running"), (vec![Action::Restart, Action::Stop], true));
        assert_eq!(actions("exited"), (vec![Action::Start], false));
        assert_eq!(actions("created"), (vec![Action::Start], false));
        assert_eq!(actions("restarting"), (vec![Action::Stop], false));
        assert_eq!(actions("paused"), (vec![Action::Stop], false));
    }

    #[test]
    fn ports_read_short_and_once() {
        assert_eq!(ports("0.0.0.0:8080->8080/tcp, [::]:8080->8080/tcp"), ["8080 → 8080"]);
        assert_eq!(ports("0.0.0.0:8080->8080/tcp, :::8080->8080/tcp, 5432/tcp"), ["8080 → 8080", "5432 inside"]);
        assert_eq!(ports("127.0.0.1:5432->5432/tcp"), ["127.0.0.1:5432 → 5432"]);
        assert_eq!(ports("53/udp, 0.0.0.0:8000-8001->8000-8001/tcp"), ["53/udp inside", "8000-8001 → 8000-8001"]);
        assert!(ports("").is_empty());
    }

    #[test]
    fn containers_come_grouped_by_project_running_first() {
        let b = board(vec![
            c("nightly-backup", None, "exited", "Exited (0) 6 hours ago", ""),
            c("app-db-1", Some("app"), "running", "Up 3 days", "5432/tcp"),
            c("worker", Some("app"), "exited", "Exited (1) 1 hour ago", ""),
            c("app-api-1", Some("app"), "running", "Up 3 days (healthy)", "0.0.0.0:8080->8080/tcp"),
            c("jellyfin", Some("Media"), "running", "Up 12 days", ""),
        ]);
        let groups = b.groups();
        let shape: Vec<(Option<&str>, &str, Vec<&str>)> =
            groups.iter().map(|g| (g.project.as_deref(), g.summary.as_str(), g.rows.iter().map(|r| r.name.as_str()).collect())).collect();
        assert_eq!(
            shape,
            [
                (Some("app"), "2 of 3 running", vec!["app-api-1", "app-db-1", "worker"]),
                (Some("Media"), "1 running", vec!["jellyfin"]),
                (None, "Stopped", vec!["nightly-backup"]),
            ]
        );
        assert_eq!(b.summary().as_deref(), Some("5 containers, 3 running"));
        assert_eq!(b.problem(), None);
        assert_eq!(b.empty(), None);
    }

    #[test]
    fn a_button_runs_once_and_says_how_it_went() {
        let mut b = board(vec![c("app-api-1", Some("app"), "running", "Up 3 days", "")]);
        let id = b.groups()[0].rows[0].id.clone();
        let want = b.act(&id, Action::Restart).unwrap();
        assert_eq!(want, Want::Act { engine: Engine::Docker, id: id.clone(), name: "app-api-1".into(), action: Action::Restart });
        // A second press while it runs does nothing; the row says what's on its way.
        assert_eq!(b.act(&id, Action::Stop), None);
        assert_eq!((b.groups()[0].rows[0].status.as_str(), b.groups()[0].rows[0].busy), ("Restarting…", Some(Action::Restart)));
        b.acted(&id, "app-api-1", Action::Restart, Ok(()));
        assert_eq!(b.note(), Some(&Note { ok: true, text: "Restarted app-api-1".into() }));
        assert_eq!(b.groups()[0].rows[0].busy, None);
        b.act(&id, Action::Stop).unwrap();
        b.acted(&id, "app-api-1", Action::Stop, Err(Failure::new(FailureKind::Failed, "Error response from daemon: permission denied\n")));
        assert_eq!(b.note().unwrap().text, "Couldn't stop app-api-1: Error response from daemon: permission denied");
        assert!(!b.note().unwrap().ok);
        // One that isn't there can't be acted on.
        assert_eq!(b.act("ffffffffffff", Action::Start), None);
    }

    #[test]
    fn only_the_newest_look_counts_and_failures_wait_longer() {
        let mut b = Board::default();
        assert!(b.is_first_look());
        let Want::List { ticket: old } = b.list() else { unreachable!() };
        let Want::List { ticket: new } = b.list() else { unreachable!() };
        assert!(!b.listed(old, Ok(ContainerList::default())));
        assert!(b.is_loading() && b.is_first_look());
        assert_eq!(b.next_look_in(), 10.0);
        let down = || Err(Failure::new(FailureKind::Network, "Connection timed out"));
        assert!(b.listed(new, down()));
        assert_eq!(b.next_look_in(), 20.0);
        for _ in 0..5 {
            let Want::List { ticket } = b.list() else { unreachable!() };
            b.listed(ticket, down());
        }
        assert_eq!(b.next_look_in(), 60.0);
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Ok(ContainerList { engines: vec![Engine::Docker], ..ContainerList::default() }));
        assert_eq!(b.next_look_in(), 10.0);
    }

    #[test]
    fn trouble_is_said_with_what_to_do() {
        let fail = |kind: FailureKind, message: &str| {
            let mut b = Board::default();
            let Want::List { ticket } = b.list() else { unreachable!() };
            b.listed(ticket, Err(Failure::new(kind, message)));
            b.problem().unwrap()
        };
        assert_eq!(
            fail(FailureKind::Auth, "Permission denied (publickey)."),
            Problem::Page { title: "Sign in first".into(), message: "Open a terminal to sign in, then containers show up here.".into(), command: None, open_terminal: true }
        );
        assert!(matches!(fail(FailureKind::HostKey, "x"), Problem::Page { open_terminal: true, .. }));
        assert!(matches!(fail(FailureKind::Network, "No route to host"), Problem::Page { ref message, open_terminal: false, .. } if message == "No route to host"));

        let listed = |list: ContainerList| {
            let mut b = Board::default();
            let Want::List { ticket } = b.list() else { unreachable!() };
            b.listed(ticket, Ok(list));
            b
        };
        // Neither engine: how to install Docker.
        let b = listed(ContainerList::default());
        assert!(matches!(b.problem(), Some(Problem::Page { ref title, command: Some(ref cmd), .. }) if title == "No Docker here" && cmd == DOCKER_INSTALL));
        // Installed, but this user isn't allowed: sshee-core's sentence, and the command.
        let denied = "Docker is installed, but this user can't use it. Add the user to the docker group (sudo usermod -aG docker $USER), then sign in again.";
        let b = listed(ContainerList { engines: vec![Engine::Docker], containers: vec![], problems: vec![denied.into()] });
        assert_eq!(
            b.problem(),
            Some(Problem::Page { title: "Docker can't be used yet".into(), message: denied.into(), command: Some("sudo usermod -aG docker $USER".into()), open_terminal: false })
        );
        // Installed, not running.
        let b = listed(ContainerList { engines: vec![Engine::Podman], containers: vec![], problems: vec!["Podman is installed but not running.".into()] });
        match b.problem() {
            Some(Problem::Page { title, command, .. }) => {
                assert_eq!(title, "Podman isn't running");
                assert_eq!(command.as_deref(), Some("sudo systemctl enable --now podman.socket"));
            }
            other => panic!("{other:?}"),
        }
        // Docker works and Podman doesn't: the list, with a note over it.
        let b = listed(ContainerList { engines: vec![Engine::Docker, Engine::Podman], containers: with_ids(vec![c("web", None, "running", "Up 1 hour", "")]), problems: vec!["Podman is installed but not running.".into()] });
        assert!(matches!(b.problem(), Some(Problem::Note { .. })));
        // An engine and nothing in it.
        let b = listed(ContainerList { engines: vec![Engine::Docker], ..ContainerList::default() });
        assert_eq!(b.problem(), None);
        assert_eq!(b.empty().map(|e| e.0), Some("No containers yet"));
    }

    #[test]
    fn a_failed_look_keeps_the_list_seen_last() {
        let mut b = board(vec![c("web", None, "running", "Up 1 hour", "")]);
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Err(Failure::new(FailureKind::Network, "Connection reset")));
        assert_eq!(b.groups().len(), 1);
        assert!(matches!(b.problem(), Some(Problem::Note { ref text, .. }) if text.contains("seen last")));
        // A server that now wants a password: a terminal is where to go, the list can't help.
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Err(Failure::new(FailureKind::Auth, "Permission denied")));
        assert!(matches!(b.problem(), Some(Problem::Page { open_terminal: true, .. })));
    }

    #[test]
    fn docker_with_no_containers_is_an_empty_list_not_a_wait() {
        let b = board(vec![]);
        assert!(!b.is_first_look() && !b.is_loading());
        assert_eq!(b.problem(), None);
        assert_eq!(b.empty().map(|(said, _)| said), Some("No containers yet"));
        assert_eq!(b.summary(), None);
    }

    #[test]
    fn a_look_that_never_answers_ends_in_a_page_with_try_again() {
        let mut b = Board::default();
        assert!(!b.no_answer(), "nothing on its way");
        let Want::List { ticket } = b.list() else { unreachable!() };
        assert!(b.no_answer());
        assert!(!b.is_first_look() && !b.is_loading());
        assert!(matches!(b.problem(), Some(Problem::Page { ref title, ref message, .. }) if title == "Couldn't look for containers" && message == NO_ANSWER));
        assert_eq!(b.next_look_in(), 2.0 * POLL_SECS, "it waits longer, as after any failure");
        // The answer, late, still counts.
        assert!(b.listed(ticket, Ok(ContainerList { engines: vec![Engine::Docker], ..ContainerList::default() })));
        assert_eq!(b.problem(), None);
        assert!(b.empty().is_some());
        // Over a list seen before, it's a line, not a page.
        let mut b = board(vec![c("web", None, "running", "Up 1 hour", "")]);
        b.list();
        b.no_answer();
        assert!(matches!(b.problem(), Some(Problem::Note { ref text, .. }) if text.contains(NO_ANSWER)));
    }

    #[test]
    fn a_terminal_into_a_container_is_named_after_it() {
        let web = c("app-api-1", Some("app"), "running", "Up 3 days", "");
        let shell = inside(&web, ContainerRun::Shell);
        assert_eq!((shell.engine.clone(), shell.name.as_str(), tab_title(&shell)), (Engine::Docker, "app-api-1", "app-api-1".to_owned()));
        assert_eq!(tab_title(&inside(&web, ContainerRun::Logs)), "app-api-1 logs");
        assert_eq!(engine_name(&Engine::Podman), "Podman");
    }
}
