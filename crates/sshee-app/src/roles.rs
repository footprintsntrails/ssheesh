//! What a server is for, so its icon says it as a website's favicon does: a globe for a
//! website, a cylinder for a database, a box for containers. A port of 0.10's
//! `src/lib/roles.ts`.
//!
//! It's guessed once per server, the first time a terminal on it connects, from what listens
//! on its ports and whether it runs containers. An icon you pick yourself always wins;
//! "Automatic" in the look picker goes back to the guess. Nothing here talks to a server: the
//! backend looks (sshee-core's scan of listening ports and its container list) and hands the
//! answers to [`classify`]; [`Watch`] says when to look.

use std::collections::{HashMap, HashSet};

/// Programs that serve websites (or sit in front of them), by the name `ss` shows.
const WEB_SERVERS: [&str; 8] = ["nginx", "openresty", "angie", "caddy", "apache2", "httpd", "lighttpd", "traefik"];

/// What the scan calls databases (its `known_as`). Memcached is a cache, so it isn't one.
const DATABASES: [&str; 13] =
    ["PostgreSQL", "MySQL", "MariaDB", "Redis", "MongoDB", "ClickHouse", "Elasticsearch", "OpenSearch", "CouchDB", "InfluxDB", "Cassandra", "SQL Server", "Oracle"];

/// Database programs the scan has no name for. `ss` cuts names at 15 letters
/// ("clickhouse-serv").
const DATABASE_PROGRAMS: [&str; 14] = [
    "postgres",
    "postmaster",
    "mysqld",
    "mariadbd",
    "redis-server",
    "valkey-server",
    "keydb-server",
    "mongod",
    "mongos",
    "clickhouse",
    "clickhouse-serv",
    "clickhouse-server",
    "influxd",
    "sqlservr",
];

/// Database ports, for when Docker's proxy stands in front: the scan then only says "Docker".
const DATABASE_PORTS: [u16; 11] = [1433, 1521, 3306, 5432, 5984, 6379, 8086, 9042, 9200, 27017, 33060];

/// One thing listening on the server, as sshee-core's scan tells it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listening {
    pub port: u16,
    /// The program, as `ss` names it.
    pub process: Option<String>,
    /// What the scan recognised it as ("PostgreSQL", "Docker").
    pub known_as: Option<String>,
    /// It answered as a web page (`http`, `https`).
    pub scheme: Option<String>,
    /// It looks like a web server, without asking it (no curl on the server).
    pub web: bool,
}

/// The scan of a server's ports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub apps: Vec<Listening>,
    /// The server has curl, so pages were asked for.
    pub curl: bool,
}

/// A website: a web server program, or a page on 80 or 443 (anything there, when the server
/// has no curl to ask).
fn is_website(app: &Listening, curl: bool) -> bool {
    if app.process.as_deref().is_some_and(|p| WEB_SERVERS.contains(&p.to_lowercase().as_str())) {
        return true;
    }
    (app.port == 80 || app.port == 443) && (app.scheme.is_some() || (!curl && app.web))
}

fn is_database(app: &Listening) -> bool {
    if app.known_as.as_deref().is_some_and(|k| DATABASES.contains(&k)) {
        return true;
    }
    let program = app.process.as_deref().map(str::to_lowercase);
    if program.as_deref().is_some_and(|p| DATABASE_PROGRAMS.contains(&p)) {
        return true;
    }
    matches!(program.as_deref(), None | Some("docker-proxy")) && DATABASE_PORTS.contains(&app.port)
}

fn is_docker(app: &Listening) -> bool {
    app.process.as_deref() == Some("docker-proxy") || matches!(app.known_as.as_deref(), Some("Docker" | "Docker API"))
}

/// What a server is for, from its scan and whether a container runs there; either may be
/// unknown. A website wins: it's what people come to the server for, even with a database
/// behind it. A database without a website is the server's job too. Containers come last:
/// Docker says how things run, not what for. Everything else is a plain server. The answer is
/// an icon id of [`crate::labels::HOST_ICONS`].
pub fn classify(scan: Option<&Scan>, containers_running: Option<bool>) -> &'static str {
    let apps = scan.map(|s| s.apps.as_slice()).unwrap_or_default();
    let curl = scan.is_none_or(|s| s.curl);
    if apps.iter().any(|a| is_website(a, curl)) {
        "globe"
    } else if apps.iter().any(is_database) {
        "database"
    } else if containers_running == Some(true) || apps.iter().any(is_docker) {
        "box"
    } else {
        "server"
    }
}

/// What a look at the server found: the scan (`None`: the server has neither `ss` nor
/// `netstat`; `Err`: it couldn't be had) and its containers (`Err`: couldn't be listed).
pub type Found = (Result<Option<Scan>, ()>, Result<bool, ()>);

/// The guess from what a look found, or `None` when it could still change and the server
/// should be looked at again next time (0.10's `detectRole`): without a scan, or with only
/// the scan and nothing in it.
pub fn guess(found: &Found) -> Option<&'static str> {
    let (scan, containers) = found;
    let scan = scan.as_ref().ok()?;
    let role = classify(scan.as_ref(), containers.as_ref().ok().copied());
    (role != "server" || containers.is_ok()).then_some(role)
}

/// How long after a terminal connects the server is looked at: the shell you just opened goes
/// first.
pub const AFTER_CONNECT_MS: u64 = 4000;

/// How many times a server is looked at in one run when it can't be read.
pub const TRIES: u32 = 2;

/// When to look at a server (0.10's `startRoleDetection`): the first time a terminal on it
/// connects, unless you picked its look or it was guessed before; a look that fails is tried
/// once more in the run, not on every reconnect.
#[derive(Debug, Default)]
pub struct Watch {
    tries: HashMap<String, u32>,
    looking: HashSet<String>,
}

impl Watch {
    /// A terminal on `server` (a key: `host:<id>`) connected. True: look at it now (call
    /// [`Watch::done`] with what came of it). `wants` is [`crate::labels::Looks::wants_guess`]
    /// for it, as the window knows it.
    pub fn connected(&mut self, server: &str, wants: bool) -> bool {
        if !wants || !server.starts_with("host:") || self.looking.contains(server) {
            return false;
        }
        if self.tries.get(server).copied().unwrap_or(0) >= TRIES {
            return false;
        }
        *self.tries.entry(server.to_owned()).or_default() += 1;
        self.looking.insert(server.to_owned());
        true
    }

    /// The look is over.
    pub fn done(&mut self, server: &str) {
        self.looking.remove(server);
    }

    /// "Check the server now", from the look picker: a look that isn't counted.
    pub fn check_now(&mut self, server: &str) -> bool {
        self.looking.insert(server.to_owned())
    }

    /// A look at `server` is under way: "Checking…" in the picker.
    pub fn is_looking(&self, server: &str) -> bool {
        self.looking.contains(server)
    }
}

/// "Automatic: website": the picker's words for what the guess is.
pub fn automatic_words(guessed: Option<&str>, looking: bool, failed: bool) -> String {
    let what = if looking {
        "checking…".to_owned()
    } else if failed {
        "couldn't check".to_owned()
    } else {
        match guessed {
            Some(icon) => crate::labels::icon_name(icon).to_lowercase(),
            None => "not checked yet".to_owned(),
        }
    };
    format!("Automatic: {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(port: u16, process: Option<&str>, known_as: Option<&str>, scheme: Option<&str>) -> Listening {
        Listening { port, process: process.map(Into::into), known_as: known_as.map(Into::into), scheme: scheme.map(Into::into), web: false }
    }

    fn scan(apps: Vec<Listening>) -> Scan {
        Scan { apps, curl: true }
    }

    #[test]
    fn a_website_wins_then_a_database_then_containers() {
        let site = scan(vec![app(5432, Some("postgres"), None, None), app(443, Some("nginx"), None, Some("https"))]);
        assert_eq!(classify(Some(&site), Some(true)), "globe");
        let db = scan(vec![app(5432, Some("postgres"), None, None)]);
        assert_eq!(classify(Some(&db), Some(true)), "database");
        assert_eq!(classify(Some(&scan(vec![app(22, Some("sshd"), None, None)])), Some(true)), "box");
        assert_eq!(classify(Some(&scan(vec![app(22, Some("sshd"), None, None)])), Some(false)), "server");
        assert_eq!(classify(None, None), "server");
    }

    #[test]
    fn what_counts_as_a_website_or_a_database() {
        // A page on 80 or 443 from any program; without curl, anything that looks like one.
        assert_eq!(classify(Some(&scan(vec![app(80, Some("node"), None, Some("http"))])), None), "globe");
        assert_eq!(classify(Some(&scan(vec![app(80, Some("node"), None, None)])), None), "server");
        let guessed = Scan { apps: vec![Listening { port: 443, web: true, ..Listening::default() }], curl: false };
        assert_eq!(classify(Some(&guessed), None), "globe");
        // Programs by name, cut at 15 letters as ss does, in any case.
        assert_eq!(classify(Some(&scan(vec![app(8123, Some("clickhouse-serv"), None, None)])), None), "database");
        assert_eq!(classify(Some(&scan(vec![app(8080, Some("Caddy"), None, None)])), None), "globe");
        // A database port behind Docker's proxy; a cache isn't a database.
        assert_eq!(classify(Some(&scan(vec![app(6379, Some("docker-proxy"), None, None)])), None), "database");
        assert_eq!(classify(Some(&scan(vec![app(11211, None, Some("Memcached"), None)])), None), "server");
        assert_eq!(classify(Some(&scan(vec![app(2375, None, Some("Docker API"), None)])), Some(false)), "box");
    }

    #[test]
    fn a_guess_that_could_still_change_is_asked_again() {
        let empty = scan(vec![]);
        // Nothing found, and the containers couldn't be listed: next time.
        assert_eq!(guess(&(Ok(Some(empty.clone())), Err(()))), None);
        assert_eq!(guess(&(Ok(Some(empty)), Ok(false))), Some("server"));
        // No scan to be had: try again.
        assert_eq!(guess(&(Err(()), Ok(true))), None);
        // No ss or netstat at all: the containers alone say it.
        assert_eq!(guess(&(Ok(None), Ok(true))), Some("box"));
        assert_eq!(guess(&(Ok(Some(scan(vec![app(443, Some("nginx"), None, None)]))), Err(()))), Some("globe"));
    }

    #[test]
    fn a_server_is_looked_at_once_when_its_terminal_connects() {
        let mut watch = Watch::default();
        assert!(!watch.connected("local", true));
        assert!(!watch.connected("host:saved:1", false));
        assert!(watch.connected("host:saved:1", true));
        // A second terminal while the first look runs.
        assert!(!watch.connected("host:saved:1", true));
        assert!(watch.is_looking("host:saved:1"));
        watch.done("host:saved:1");
        // It couldn't be read: once more, then not again this run.
        assert!(watch.connected("host:saved:1", true));
        watch.done("host:saved:1");
        assert!(!watch.connected("host:saved:1", true));
        // Asked for by hand, it looks whatever the count.
        assert!(watch.check_now("host:saved:1"));
        assert!(!watch.check_now("host:saved:1"));
    }

    #[test]
    fn automatic_says_what_it_guessed() {
        assert_eq!(automatic_words(Some("globe"), false, false), "Automatic: website");
        assert_eq!(automatic_words(None, false, false), "Automatic: not checked yet");
        assert_eq!(automatic_words(Some("globe"), true, false), "Automatic: checking…");
        assert_eq!(automatic_words(None, false, true), "Automatic: couldn't check");
    }
}
