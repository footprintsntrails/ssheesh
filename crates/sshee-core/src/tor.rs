//! Tor: connections to servers that hide where you are.
//!
//! With Tor on, a server (and its logs) sees the address of a Tor exit, not
//! yours, and your internet provider sees Tor, not which servers you use.
//! SSHeesh runs its own Tor client (Arti, the Tor Project's Tor in Rust)
//! inside the app, so there's nothing to install.
//!
//! How a connection gets there: every ssh the app starts gets a
//! ProxyCommand, this same executable with `--tor-proxy`. That small helper
//! hands the connection to the app through a door on 127.0.0.1 (its port and
//! a secret are in ~/.sshee/tor/door, readable by you only; each side proves
//! it knows the secret without sending it), and the app carries it over Tor.
//! When Tor can't carry it, the connection fails: nothing goes around Tor,
//! not even a name lookup. Servers on your own network (192.168.x.x, a .local
//! name) are the exception: Tor can't reach them, and they never see the
//! internet. A server reached through a jump host or a ProxyCommand of your
//! own isn't connected at all: Tor would replace that route without a word.
//!
//! Fast, because a slow route is the difference between typing and waiting:
//! - Tor starts with the app, from what it saved last time (a fraction of a second).
//! - Routes to exits that let ssh out are built ahead of time.
//! - Before a server's first connection, twelve routes to it are timed and
//!   the fastest is kept; the server's later connections reuse it. Measured
//!   over many tries, a keystroke's round trip: ~70 ms (110 at worst) this
//!   way, against ~116 ms (235 at worst) on one random route, as plain Tor
//!   does. Racing routes to the server's first word did worse (~94 ms).
//! - Keystrokes go out at once, never held back to fill a packet.
//!
//! Each server gets routes of its own, so a Tor exit can't tell that two
//! servers are used by the same person.

use arti_client::config::{BoolOrAuto, TorClientConfigBuilder};
use arti_client::{BootstrapBehavior, DataStream, ErrorKind, HasKind, IsolationToken, StreamPrefs, TorClient};
use futures_util::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream as StdTcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tor_rtcompat::PreferredRuntime;

/// ssh starts this executable with it as its ProxyCommand; see `proxy_main`.
pub const PROXY_FLAG: &str = "--tor-proxy";
/// The ProxyCommand for a server that mustn't connect while Tor is on, with why.
const REFUSE_FLAG: &str = "--tor-refuse";

/// Routes timed before a server's first connection.
const CANDIDATES: usize = 12;
/// Once this many are timed and this long has passed, the fastest so far wins.
const ENOUGH: usize = 4;
const PICK_TIME: Duration = Duration::from_millis(1200);
/// Routes kept built ahead of time for each port your servers use.
const READY_ROUTES: usize = 4;
/// A route takes no new connections after this (Tor's own default is 10
/// minutes; each server has routes of its own, so longer gives nothing away).
const ROUTE_LIFE: Duration = Duration::from_secs(30 * 60);
/// How long a server keeps the route it picked: a little less than the
/// route lives, so the next connection picks again instead of landing on
/// whatever route Tor builds next.
const KEEP_ROUTE: Duration = Duration::from_secs(25 * 60);
/// Longest wait for Tor to start before a connection gives up.
const START_WAIT: Duration = Duration::from_secs(45);
const CONNECT_WAIT: Duration = Duration::from_secs(30);
/// The first line of a request at the door.
const HELLO: &str = "SSHEESH-TOR2";
/// How long what `check` found about a server holds.
const CHECK_LIFE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TorStatus {
    Off,
    /// Starting, `percent` of the way.
    Starting { percent: u8 },
    On,
    /// Couldn't start (no internet, say). It tries again by itself, and
    /// meanwhile nothing connects.
    Failed { message: String },
}

type Client = Arc<TorClient<PreferredRuntime>>;

struct Route {
    token: IsolationToken,
    since: Instant,
    /// A round trip on it when it was picked.
    ms: u128,
}

struct Engine {
    rt: tokio::runtime::Runtime,
    client: Mutex<Option<Client>>,
    status: Mutex<TorStatus>,
    /// The route each server won, by "host:port".
    routes: Mutex<HashMap<String, Route>>,
    /// One pick at a time per server: connections opened together (a
    /// terminal, Files, the stats) share the route the first one picks.
    picking: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Connections carried right now. Turned off, Tor stays up until they end:
    /// a server without tmux would lose what its shell was doing.
    carrying: AtomicUsize,
    /// Bumped by every start and stop, so a start that's overtaken gives up.
    run: AtomicU64,
    /// Why connections through Tor failed, by host, and the latest of all.
    failures: Mutex<HashMap<String, (Instant, String)>>,
    failure: Mutex<Option<(Instant, String)>>,
    secret: String,
    ports: Mutex<Vec<u16>>,
}

static ON: AtomicBool = AtomicBool::new(false);
static EXE: OnceLock<PathBuf> = OnceLock::new();
static ENGINE: OnceLock<Engine> = OnceLock::new();
static MAKING: Mutex<()> = Mutex::new(());
static WATCHER: OnceLock<Box<dyn Fn(TorStatus) + Send + Sync>> = OnceLock::new();
/// Why the engine couldn't be made, when it couldn't.
static BROKEN: Mutex<Option<String>> = Mutex::new(None);

/// Where Tor keeps what it learned (so it starts fast) and the door. Never a
/// shared folder such as /tmp: without a home folder, Tor doesn't start.
fn dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".sshee").join("tor"))
}

/// Called once at startup, before anything connects: if Tor was left on,
/// every connection from here on goes through it. `exe` is what ssh starts
/// as its ProxyCommand, and `ports` the ports your servers use.
pub fn init(exe: PathBuf, ports: Vec<u16>) {
    let _ = EXE.set(exe);
    if dir().is_some_and(|d| d.join("on").exists()) {
        let _ = set_on(true, ports);
    }
}

/// Called as the app quits: nothing answers at the door anymore.
pub fn shutdown() {
    if let Some(d) = dir() {
        let _ = std::fs::remove_file(d.join("door"));
    }
}

/// Hears every change of `status`.
pub fn watch(f: impl Fn(TorStatus) + Send + Sync + 'static) {
    let _ = WATCHER.set(Box::new(f));
}

pub fn is_on() -> bool {
    ON.load(Ordering::SeqCst)
}

pub fn status() -> TorStatus {
    // Never on, starting or failed while connections go direct.
    if !is_on() {
        return TorStatus::Off;
    }
    if let Some(message) = BROKEN.lock().unwrap().clone() {
        return TorStatus::Failed { message };
    }
    ENGINE.get().map(|e| e.status.lock().unwrap().clone()).unwrap_or(TorStatus::Off)
}

/// Turns Tor on or off, and remembers it for next time. Connections already
/// open stay as they are; see `SessionManager::reroute` for moving them.
pub fn set_on(on: bool, ports: Vec<u16>) -> Result<(), String> {
    let saved = match dir() {
        Some(d) if on => make_dir().and_then(|_| std::fs::write(d.join("on"), b"")),
        Some(d) => std::fs::remove_file(d.join("on")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }),
        None => Err(std::io::Error::other("no home folder")),
    };
    // From here every ssh the app starts goes through Tor, or fails.
    ON.store(on, Ordering::SeqCst);
    let engine = match engine() {
        Ok(e) => e,
        Err(message) => {
            shutdown();
            *BROKEN.lock().unwrap() = Some(message.clone());
            notify(status());
            return Err(message);
        }
    };
    if on {
        *engine.ports.lock().unwrap() = ports;
        start(engine);
    } else {
        stop(engine);
    }
    saved.map_err(|e| {
        if on {
            format!("Tor is on, but SSHeesh couldn't note that for next time ({e}), so it would start without Tor.")
        } else {
            format!("Tor is off, but SSHeesh couldn't note that for next time ({e}).")
        }
    })
}

/// Forgets the routes servers picked, so their next connections pick
/// again: for a route that turned slow.
pub fn new_routes() {
    if let Some(e) = ENGINE.get() {
        e.routes.lock().unwrap().clear();
    }
}

const RECENT: Duration = Duration::from_secs(20);

/// Why a connection through Tor failed a moment ago, if one did. ssh itself
/// only sees the connection close.
pub fn recent_failure() -> Option<String> {
    if !is_on() {
        return None;
    }
    if let TorStatus::Failed { message } = status() {
        return Some(message);
    }
    let e = ENGINE.get()?;
    let failure = e.failure.lock().unwrap();
    failure.as_ref().filter(|(at, _)| at.elapsed() < RECENT).map(|(_, m)| m.clone())
}

/// Like `recent_failure`, for connections to `t` only.
pub fn recent_failure_for(t: &crate::ssh::SshTarget) -> Option<String> {
    if !is_on() {
        return None;
    }
    if let TorStatus::Failed { message } = status() {
        return Some(message);
    }
    let host = checked(t).host?;
    let e = ENGINE.get()?;
    let failures = e.failures.lock().unwrap();
    failures.get(&host.to_ascii_lowercase()).filter(|(at, _)| at.elapsed() < RECENT).map(|(_, m)| m.clone())
}

fn note_failure(host: &str, message: &str) {
    if let Some(e) = ENGINE.get() {
        e.failures.lock().unwrap().insert(host.to_ascii_lowercase(), (Instant::now(), message.to_string()));
        *e.failure.lock().unwrap() = Some((Instant::now(), message.to_string()));
    }
}

/// Why `t` can't connect while Tor is on, if it can't.
pub fn refusal(t: &crate::ssh::SshTarget) -> Option<String> {
    if !is_on() {
        return None;
    }
    checked(t).refusal.map(|r| r.message().to_string())
}

/// Whether a connection to `t` started now goes through Tor: Tor is on and
/// the server isn't on your own network.
pub fn carries(t: &crate::ssh::SshTarget) -> bool {
    if !is_on() {
        return false;
    }
    let found = checked(t);
    found.refusal.is_none() && found.host.is_some_and(|h| !matches!(way(&h, 22), Way::Near(_)))
}

fn exe() -> String {
    let exe = EXE.get().cloned().or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| PathBuf::from("ssheesh"));
    exe.display().to_string()
}

/// The ProxyCommand that sends an ssh through Tor, while Tor is on. For
/// reading the config with `ssh -G`: with a ProxyCommand ssh looks no names
/// up, not even for `CanonicalizePermittedCNAMEs`.
pub fn proxy_command() -> Option<String> {
    is_on().then(|| command_line(&exe()))
}

/// The ProxyCommand for connecting to `t` while Tor is on: through Tor, or
/// one that refuses with why (see `check`), so nothing goes around it.
pub fn proxy_command_for(t: &crate::ssh::SshTarget) -> Option<String> {
    if !is_on() {
        return None;
    }
    let found = checked(t);
    Some(match found.refusal {
        None => command_line(&exe()),
        Some(why) => {
            note_failure(found.host.as_deref().unwrap_or(&t.destination), why.message());
            refusing_line(&exe(), why)
        }
    })
}

/// Why a server can't go through Tor.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Refusal {
    /// A jump host or a ProxyCommand of your own in ~/.ssh/config.
    Hop,
    /// An address with characters a shell would act on.
    Address,
}

impl Refusal {
    fn code(self) -> &'static str {
        match self {
            Refusal::Hop => "hop",
            Refusal::Address => "address",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Refusal::Hop, Refusal::Address].into_iter().find(|r| r.code() == code)
    }

    fn message(self) -> &'static str {
        match self {
            Refusal::Hop => "This server is reached through a jump host or a ProxyCommand in your ~/.ssh/config, which can't go through Tor, so SSHeesh didn't connect. Turn Tor off to reach it.",
            Refusal::Address => "This server's address has characters SSHeesh won't hand to Tor, so it didn't connect.",
        }
    }
}

#[derive(Clone)]
struct Checked {
    at: Instant,
    /// The name or address ssh connects to (`HostName`).
    host: Option<String>,
    refusal: Option<Refusal>,
}

/// What `check` found about `t` lately, or finds now.
fn checked(t: &crate::ssh::SshTarget) -> Checked {
    static SEEN: OnceLock<Mutex<HashMap<String, Checked>>> = OnceLock::new();
    let key = format!("{}\0{:?}\0{:?}\0{:?}", t.destination, t.user, t.port, t.extra_args);
    let seen = SEEN.get_or_init(Default::default);
    if let Some(found) = seen.lock().unwrap().get(&key).filter(|c| c.at.elapsed() < CHECK_LIFE) {
        return found.clone();
    }
    let found = check(t);
    seen.lock().unwrap().insert(key, found.clone());
    found
}

/// Reads your config for `t` without looking anything up: the address ssh
/// will hand the ProxyCommand (it must hold nothing a shell acts on, as ssh
/// before 9.6 passes it on as it is), and whether a ProxyJump or ProxyCommand
/// of yours would route it, which Tor's ProxyCommand would silently replace.
fn check(t: &crate::ssh::SshTarget) -> Checked {
    let at = Instant::now();
    let Ok(config) = crate::ssh::ssh_config(t) else {
        return Checked { at, host: None, refusal: Some(Refusal::Address) };
    };
    let get = |c: &str, k: &str| crate::ssh::config_value(c, k).filter(|v| v != "none");
    let host = get(&config, "hostname").unwrap_or_else(|| t.destination.clone());
    let refusal = if !valid_host(&host) {
        Some(Refusal::Address)
    } else if get(&config, "canonicalizepermittedcnames").is_some() {
        // Read without Tor's ProxyCommand, ssh would look the name up here.
        None
    } else {
        let own = crate::ssh::ssh_config_as_written(t).unwrap_or_default();
        (get(&own, "proxyjump").is_some() || get(&own, "proxycommand").is_some()).then_some(Refusal::Hop)
    };
    Checked { at, host: Some(host), refusal }
}

/// The ProxyCommand for a server that mustn't connect: it says why and fails.
fn refusing_line(exe: &str, why: Refusal) -> String {
    let exe = exe.replace('%', "%%");
    if cfg!(windows) {
        format!("\"{exe}\" {REFUSE_FLAG} {}", why.code())
    } else {
        format!("{} {REFUSE_FLAG} {}", crate::ssh::shell_quote(&exe), why.code())
    }
}

/// Options for an ssh you type into a terminal on this computer (a recipe's
/// check), sending it through Tor too. On Windows the path goes unquoted:
/// PowerShell 5.1 would mangle the quotes, and ssh quotes a path up to
/// ".exe" by itself.
pub fn typed_ssh_options() -> Option<Vec<String>> {
    if !is_on() {
        return None;
    }
    let exe = exe().replace('%', "%%");
    let line = if cfg!(windows) { format!("{exe} {PROXY_FLAG} %h %p") } else { format!("{} {PROXY_FLAG} %h %p", crate::ssh::shell_quote(&exe)) };
    Some(ssh_options(&line))
}

/// ssh fills in %h and %p, so a % in the path is written %%.
fn command_line(exe: &str) -> String {
    let exe = exe.replace('%', "%%");
    if cfg!(windows) {
        // Windows' ssh starts it without a shell, and takes a quoted path as the program.
        format!("\"{exe}\" {PROXY_FLAG} %h %p")
    } else {
        // Run with your shell; sh, bash, zsh and fish all read '…' the same.
        format!("{} {PROXY_FLAG} %h %p", crate::ssh::shell_quote(&exe))
    }
}

/// The ssh options for a connection through Tor.
pub fn ssh_options(proxy_command: &str) -> Vec<String> {
    [
        format!("ProxyCommand={proxy_command}"),
        // Names go to Tor as they are: nothing on this computer looks them up.
        "CanonicalizeHostname=no".into(),
        "VerifyHostKeyDNS=no".into(),
        "CheckHostIP=no".into(),
    ]
    .into_iter()
    .flat_map(|o| ["-o".to_string(), o])
    .collect()
}

fn notify(s: TorStatus) {
    if let Some(w) = WATCHER.get() {
        w(s);
    }
}

fn make_dir() -> std::io::Result<()> {
    let d = dir().ok_or_else(|| std::io::Error::other("no home folder"))?;
    std::fs::create_dir_all(&d)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn engine() -> Result<&'static Engine, String> {
    let _making = MAKING.lock().unwrap();
    if let Some(e) = ENGINE.get() {
        return Ok(e);
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("tor")
        .enable_all()
        .build()
        .map_err(|e| format!("Couldn't start Tor: {e}"))?;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("Couldn't start Tor: {e}"))?;
    let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("Couldn't start Tor: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| format!("Couldn't start Tor: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    write_door(port, &secret).map_err(|e| format!("Couldn't start Tor: it can't save its files in your home folder ({e})."))?;
    let e = ENGINE.get_or_init(|| Engine {
        rt,
        client: Mutex::new(None),
        status: Mutex::new(TorStatus::Off),
        routes: Mutex::default(),
        picking: Mutex::default(),
        carrying: AtomicUsize::new(0),
        run: AtomicU64::new(0),
        failures: Mutex::default(),
        failure: Mutex::new(None),
        secret,
        ports: Mutex::default(),
    });
    e.rt.spawn(door(e, listener));
    Ok(e)
}

/// The door's port and secret, in a file only you can read.
fn write_door(port: u16, secret: &str) -> std::io::Result<()> {
    make_dir()?;
    let d = dir().ok_or_else(|| std::io::Error::other("no home folder"))?;
    let (path, tmp) = (d.join("door"), d.join("door.new"));
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&tmp)?.write_all(format!("{port} {secret}\n").as_bytes())?;
    std::fs::rename(&tmp, &path)
}

/// Changes the status, and tells the UI, under the status lock: starts,
/// stops and a start's progress never cross, and the UI hears them in order.
fn put_status(now: &mut TorStatus, s: TorStatus) {
    if *now != s {
        *now = s.clone();
        notify(s);
    }
}

/// Sets the status (and the client) if `run` is still the current start.
fn set_status_if(e: &Engine, run: u64, s: TorStatus, client: Option<Client>) -> bool {
    let mut now = e.status.lock().unwrap();
    if e.run.load(Ordering::SeqCst) != run {
        return false;
    }
    if let Some(c) = client {
        *e.client.lock().unwrap() = Some(c);
    }
    put_status(&mut now, s);
    true
}

fn start(e: &'static Engine) {
    *BROKEN.lock().unwrap() = None;
    let run = {
        let mut now = e.status.lock().unwrap();
        let run = e.run.fetch_add(1, Ordering::SeqCst) + 1;
        if e.client.lock().unwrap().is_some() {
            put_status(&mut now, TorStatus::On);
            return;
        }
        put_status(&mut now, TorStatus::Starting { percent: 0 });
        run
    };
    e.rt.spawn(async move {
        let mut wait = Duration::from_secs(5);
        while e.run.load(Ordering::SeqCst) == run {
            match bootstrap(e, run).await {
                Ok(client) => {
                    set_status_if(e, run, TorStatus::On, Some(client));
                    return;
                }
                Err(message) => {
                    if !set_status_if(e, run, TorStatus::Failed { message }, None) {
                        return;
                    }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(60));
                }
            }
        }
    });
}

fn stop(e: &'static Engine) {
    let mut now = e.status.lock().unwrap();
    e.run.fetch_add(1, Ordering::SeqCst);
    e.routes.lock().unwrap().clear();
    if e.carrying.load(Ordering::SeqCst) == 0 {
        e.client.lock().unwrap().take();
    }
    put_status(&mut now, TorStatus::Off);
}

async fn bootstrap(e: &'static Engine, run: u64) -> Result<Client, String> {
    // Tor's connections are TLS; the app has one way to do it, set here.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base = dir().ok_or("Couldn't start Tor: there's no home folder to keep its files in.")?;
    let mut b = TorClientConfigBuilder::from_directories(base.join("state"), base.join("cache"));
    {
        let ready = b.preemptive_circuits();
        let ports = ready.initial_predicted_ports();
        ports.clear();
        ports.push(22);
        for p in e.ports.lock().unwrap().iter() {
            if !ports.contains(p) {
                ports.push(*p);
            }
        }
        ready.min_exit_circs_for_port(READY_ROUTES);
    }
    b.circuit_timing().max_dirtiness(ROUTE_LIFE);
    b.address_filter().allow_onion_addrs(true);
    let config = b.build().map_err(|e| format!("Couldn't start Tor: {e}"))?;
    let client = TorClient::builder()
        .config(config)
        .bootstrap_behavior(BootstrapBehavior::Manual)
        .create_unbootstrapped()
        .map_err(|e| format!("Couldn't start Tor: {e}"))?;
    let mut events = client.bootstrap_events();
    let progress = tokio::spawn(async move {
        while let Some(s) = events.next().await {
            if e.run.load(Ordering::SeqCst) != run {
                break;
            }
            let percent = (s.as_frac() * 100.0) as u8;
            if percent < 100 {
                set_status_if(e, run, TorStatus::Starting { percent }, None);
            }
        }
    });
    let started = client.bootstrap().await;
    progress.abort();
    started.map_err(|err| match err.kind() {
        ErrorKind::TorAccessFailed | ErrorKind::LocalNetworkError | ErrorKind::TorNetworkTimeout => {
            "Can't reach the Tor network. Check your internet connection.".to_string()
        }
        _ => format!("Couldn't start Tor: {err}"),
    })?;
    Ok(client)
}

/// Lets in the helper's requests. The helper says "SSHEESH-TOR2 <nonce>"; the
/// app answers with its proof of the secret for that nonce; the helper sends
/// its own proof with "<host> <port>", answered "OK" and then the connection,
/// or "ERR <why>". The secret never crosses: whatever else might listen on
/// the door's port learns nothing, not even which server.
async fn door(e: &'static Engine, listener: std::net::TcpListener) {
    let Ok(listener) = TcpListener::from_std(listener) else { return };
    loop {
        match listener.accept().await {
            Ok((conn, _)) => {
                tokio::spawn(admit(e, conn));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn admit(e: &'static Engine, mut conn: TcpStream) {
    // A keystroke goes out on its own, not after the next one.
    let _ = conn.set_nodelay(true);
    let Ok(Some(hello)) = timeout(Duration::from_secs(5), read_line(&mut conn)).await else { return };
    let Some(nonce) = parse_hello(&hello) else { return };
    if conn.write_all(format!("{}\n", proof(&e.secret, "app", &nonce)).as_bytes()).await.is_err() {
        return;
    }
    let Ok(Some(line)) = timeout(Duration::from_secs(5), read_line(&mut conn)).await else { return };
    let Some((host, port)) = parse_request(&line, &proof(&e.secret, "helper", &nonce)) else { return };
    let _carrying = Carrying::new(e);
    match open(e, &host, port).await {
        Ok(mut stream) => {
            if conn.write_all(b"OK\n").await.is_err() {
                return;
            }
            // Writes what's read as soon as nothing more is waiting: a key
            // at once, a file in full cells.
            let _ = tokio::io::copy_bidirectional(&mut conn, &mut stream).await;
        }
        Err(message) => {
            note_failure(&host, &message);
            let _ = conn.write_all(format!("ERR {message}\n").as_bytes()).await;
        }
    }
}

async fn read_line(conn: &mut TcpStream) -> Option<String> {
    let mut line = Vec::new();
    loop {
        let b = conn.read_u8().await.ok()?;
        if b == b'\n' {
            return String::from_utf8(line).ok();
        }
        if line.len() > 600 {
            return None;
        }
        line.push(b);
    }
}

/// The nonce in the helper's first line: 64 hex digits.
fn parse_hello(line: &str) -> Option<String> {
    let nonce = line.trim_end_matches('\r').strip_prefix(HELLO)?.strip_prefix(' ')?;
    (nonce.len() == 64 && nonce.bytes().all(|b| b.is_ascii_hexdigit())).then(|| nonce.to_string())
}

/// What proves `side` ("app" or "helper") knows the secret, for this nonce.
fn proof(secret: &str, side: &str, nonce: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, format!("{side}\n{secret}\n{nonce}").as_bytes());
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Compared in full, whatever differs first.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// "<proof> <host> <port>", with the helper's proof for this nonce.
fn parse_request(line: &str, expected: &str) -> Option<(String, u16)> {
    let mut parts = line.trim_end_matches('\r').split(' ');
    if !same(parts.next()?, expected) {
        return None;
    }
    let host = parts.next()?;
    let port = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !valid_host(host) {
        return None;
    }
    Some((host.to_string(), port))
}

/// A host name or address, nothing a shell or a line could break on.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 255 && host.chars().all(|c| c.is_ascii_alphanumeric() || "-._:".contains(c))
}

struct Carrying(&'static Engine);

impl Carrying {
    fn new(e: &'static Engine) -> Self {
        e.carrying.fetch_add(1, Ordering::SeqCst);
        Self(e)
    }
}

impl Drop for Carrying {
    fn drop(&mut self) {
        // The last connection Tor carried after it was turned off: let go of
        // Tor. Under the status lock, so a start at the same moment either
        // sees Tor still there or starts it again.
        let _now = self.0.status.lock().unwrap();
        if self.0.carrying.fetch_sub(1, Ordering::SeqCst) == 1 && !is_on() {
            self.0.client.lock().unwrap().take();
        }
    }
}

async fn ready(e: &'static Engine) -> Result<Client, String> {
    let deadline = Instant::now() + START_WAIT;
    loop {
        if !is_on() {
            return Err("Tor was turned off, so SSHeesh stopped this connection instead of making it without Tor.".into());
        }
        if let Some(c) = e.client.lock().unwrap().clone() {
            return Ok(c);
        }
        if Instant::now() > deadline {
            return Err(match &*e.status.lock().unwrap() {
                TorStatus::Failed { message } => message.clone(),
                _ => "Tor is taking too long to start. Check your internet connection.".into(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A connection to `host` through Tor, on the server's fastest route.
async fn open(e: &'static Engine, host: &str, port: u16) -> Result<DataStream, String> {
    let client = ready(e).await?;
    let key = format!("{}:{port}", host.to_ascii_lowercase());
    let lane = e.picking.lock().unwrap().entry(key.clone()).or_default().clone();
    let _turn = lane.lock().await;
    let kept = e.routes.lock().unwrap().get(&key).filter(|r| r.since.elapsed() < KEEP_ROUTE).map(|r| (r.token, r.ms));
    if let Some((token, ms)) = kept {
        let started = Instant::now();
        if let Ok(Ok(stream)) = timeout(CONNECT_WAIT, connect(&client, host, port, token)).await {
            // Tor rebuilt the route somewhere slower: pick again next time.
            if started.elapsed().as_millis() > (ms * 3).max(300) {
                e.routes.lock().unwrap().remove(&key);
            }
            return Ok(stream);
        }
        e.routes.lock().unwrap().remove(&key);
    }
    // Meeting an onion service takes seconds of its own, and its route is
    // made for it: nothing to pick from.
    if host.ends_with(".onion") {
        let token = IsolationToken::new();
        let stream = timeout(CONNECT_WAIT, connect(&client, host, port, token))
            .await
            .map_err(|_| explain(Some(Lost::TimedOut), port))?
            .map_err(|err| explain(Some(Lost::Tor(err)), port))?;
        e.routes.lock().unwrap().insert(key, Route { token, since: Instant::now(), ms: 1000 });
        return Ok(stream);
    }
    let (token, ms) = pick(&client, host, port).await?;
    let stream = timeout(CONNECT_WAIT, connect(&client, host, port, token))
        .await
        .map_err(|_| explain(Some(Lost::TimedOut), port))?
        .map_err(|err| explain(Some(Lost::Tor(err)), port))?;
    e.routes.lock().unwrap().insert(key, Route { token, since: Instant::now(), ms });
    Ok(stream)
}

async fn connect(client: &Client, host: &str, port: u16, token: IsolationToken) -> arti_client::Result<DataStream> {
    let mut prefs = StreamPrefs::new();
    prefs.set_isolation(token);
    if host.ends_with(".onion") {
        prefs.connect_to_onion_services(BoolOrAuto::Explicit(true));
    }
    client.connect_with_prefs((host, port), &prefs).await
}

enum Lost {
    Tor(arti_client::Error),
    TimedOut,
}

/// Opens a connection to the server on `CANDIDATES` new routes at once, so
/// each is built; then times one more on each, which is one round trip
/// through the route and on to the server: what a keystroke waits. The
/// fastest wins, with its round trip. The server sees each candidate
/// connect and leave without a word, two from each of a dozen Tor exits.
async fn pick(client: &Client, host: &str, port: u16) -> Result<(IsolationToken, u128), String> {
    let mut candidates = tokio::task::JoinSet::new();
    for _ in 0..CANDIDATES {
        let (client, host) = (client.clone(), host.to_string());
        candidates.spawn(async move {
            let token = IsolationToken::new();
            let mut timed = None;
            for _ in 0..2 {
                let started = Instant::now();
                match timeout(CONNECT_WAIT, connect(&client, &host, port, token)).await {
                    Ok(Ok(_)) => timed = Some(started.elapsed().as_millis()),
                    Ok(Err(err)) => return Err(Lost::Tor(err)),
                    Err(_) => return Err(Lost::TimedOut),
                }
            }
            Ok((token, timed.unwrap_or(u128::MAX)))
        });
    }
    let started = Instant::now();
    let (mut timed, mut lost): (Vec<(IsolationToken, u128)>, Option<Lost>) = (Vec::new(), None);
    loop {
        // Enough timed and time's up: the rest are slow anyway.
        let wait = if timed.len() >= ENOUGH { PICK_TIME.saturating_sub(started.elapsed()) } else { CONNECT_WAIT * 2 };
        match timeout(wait, candidates.join_next()).await {
            Ok(Some(Ok(Ok(t)))) => timed.push(t),
            Ok(Some(Ok(Err(why)))) => {
                // A reason from Tor says more than a timeout.
                if !matches!(lost, Some(Lost::Tor(_))) {
                    lost = Some(why);
                }
            }
            Ok(Some(Err(_))) => {}
            // All done, or time's up (dropping the set ends the rest).
            Ok(None) | Err(_) => break,
        }
    }
    timed.into_iter().min_by_key(|t| t.1).ok_or_else(|| explain(lost, port))
}

/// Why a server couldn't be reached through Tor, without naming it (the
/// message may be on screen while you stream).
fn explain(lost: Option<Lost>, port: u16) -> String {
    match lost {
        Some(Lost::Tor(e)) => match e.kind() {
            ErrorKind::RemoteHostNotFound | ErrorKind::RemoteHostResolutionFailed => {
                "Tor couldn't find the server. Check its address; a name only your own network knows can't go through Tor, its IP address can.".into()
            }
            ErrorKind::RemoteConnectionRefused => format!("The server refused the connection on port {port}."),
            ErrorKind::ExitPolicyRejected | ErrorKind::NoExit => {
                format!("No Tor exit lets connections out to port {port}. ssh's usual port, 22, works best.")
            }
            ErrorKind::RemoteNetworkTimeout | ErrorKind::ExitTimeout | ErrorKind::RemoteNetworkFailed => "The server didn't answer through Tor.".into(),
            ErrorKind::TorAccessFailed | ErrorKind::LocalNetworkError | ErrorKind::TorNetworkTimeout => {
                "Can't reach the Tor network. Check your internet connection.".into()
            }
            k if format!("{k:?}").starts_with("OnionService") => "That .onion server can't be reached right now.".into(),
            _ => format!("Tor couldn't connect: {}", e.kind()),
        },
        Some(Lost::TimedOut) | None => "The server didn't answer through Tor.".into(),
    }
}

// ---------------------------------------------------------------- the helper

/// When ssh started this executable as its ProxyCommand: carries the
/// connection through the app's Tor until it ends. Called first thing in
/// `main`, before askpass, since askpass's variables reach this too.
pub fn proxy_main() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [PROXY_FLAG, host, port] => port.parse::<u16>().map_err(|_| format!("{port} isn't a port")).and_then(|port| proxy(host, port)),
        [PROXY_FLAG, ..] => Err(format!("usage: {PROXY_FLAG} <host> <port>")),
        [REFUSE_FLAG, code] => Err(Refusal::from_code(code).map_or("SSHeesh didn't connect this server through Tor.", Refusal::message).to_string()),
        [REFUSE_FLAG, ..] => Err("SSHeesh didn't connect this server through Tor.".into()),
        _ => return None,
    };
    Some(match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("SSHeesh: {message}");
            1
        }
    })
}

/// How a host is reached.
#[derive(Debug, PartialEq)]
enum Way {
    /// On your own network, which Tor can't reach.
    Near(Vec<SocketAddr>),
    /// Through Tor, by this name or address.
    Tor(String),
}

fn way(host: &str, port: u16) -> Way {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_near(ip) { Way::Near(vec![SocketAddr::new(ip, port)]) } else { Way::Tor(host.to_string()) };
    }
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if !near_name(&name) {
        // A public name: Tor looks it up, never this computer.
        return Way::Tor(host.to_string());
    }
    // A name only your network knows: looked up here. On your network it
    // stays there; a public address it stands for goes through Tor.
    let found: Vec<SocketAddr> = (name.as_str(), port).to_socket_addrs().map(Iterator::collect).unwrap_or_default();
    if !found.is_empty() && found.iter().all(|a| is_near(a.ip())) {
        return Way::Near(found);
    }
    match found.iter().find(|a| !is_near(a.ip())) {
        Some(public) => Way::Tor(public.ip().to_string()),
        None => Way::Tor(host.to_string()),
    }
}

/// Names that only mean something on your own network, asked of it alone:
/// localhost, and .local (mDNS).
fn near_name(name: &str) -> bool {
    // Only names that are never asked of a DNS server out there: a short
    // name or a .lan one may well be, with your search domain, so they go
    // to Tor (which can't find them: use their address).
    name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
}

/// Addresses that aren't on the internet: this computer, your network, a
/// tailnet or carrier-grade NAT.
fn is_near(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, ..] = v.octets();
            v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || (a == 100 && (b & 0xc0) == 64)
        }
        IpAddr::V6(v) => {
            let first = v.segments()[0];
            v.is_loopback() || v.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 || v.to_ipv4_mapped().is_some_and(|m| is_near(IpAddr::V4(m)))
        }
    }
}

fn proxy(host: &str, port: u16) -> Result<(), String> {
    let sock = match way(host, port) {
        Way::Near(addrs) => StdTcpStream::connect(&addrs[..]).map_err(|e| format!("Couldn't reach the server on your network: {e}"))?,
        Way::Tor(name) => through_door(&name, port)?,
    };
    let _ = sock.set_nodelay(true);
    carry(sock);
    Ok(())
}

/// Asks the app for a connection through Tor.
fn through_door(host: &str, port: u16) -> Result<StdTcpStream, String> {
    let not_running = || "Tor isn't running in SSHeesh. Open SSHeesh and try again.".to_string();
    let door = dir().and_then(|d| std::fs::read_to_string(d.join("door")).ok()).ok_or_else(not_running)?;
    let (door_port, secret) = door.trim().split_once(' ').ok_or_else(not_running)?;
    let door_port: u16 = door_port.parse().map_err(|_| not_running())?;
    let mut sock = StdTcpStream::connect((Ipv4Addr::LOCALHOST, door_port)).map_err(|_| not_running())?;
    let _ = sock.set_nodelay(true);
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| not_running())?;
    let nonce: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    sock.write_all(format!("{HELLO} {nonce}\n").as_bytes()).map_err(|_| not_running())?;
    let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
    // Whatever answers must know the secret before it hears the server's name.
    let answer = read_line_sync(&mut sock).ok_or_else(not_running)?;
    if !same(&answer, &proof(secret, "app", &nonce)) {
        return Err("Something other than SSHeesh answers at Tor's door, so SSHeesh didn't connect. Open SSHeesh and try again.".into());
    }
    sock.write_all(format!("{} {host} {port}\n", proof(secret, "helper", &nonce)).as_bytes()).map_err(|_| not_running())?;
    let _ = sock.set_read_timeout(Some(START_WAIT + CONNECT_WAIT + Duration::from_secs(15)));
    let line = read_line_sync(&mut sock).ok_or_else(not_running)?;
    let _ = sock.set_read_timeout(None);
    match line.as_str() {
        "OK" => Ok(sock),
        _ => Err(line.strip_prefix("ERR ").unwrap_or(&line).to_string()),
    }
}

/// A line, read a byte at a time: what follows it is the connection's.
fn read_line_sync(sock: &mut StdTcpStream) -> Option<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match sock.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => return String::from_utf8(line).ok(),
            Ok(1) if line.len() < 1000 => line.push(byte[0]),
            _ => return None,
        }
    }
}

/// Copies ssh's input to the connection and the connection to ssh, each as
/// soon as it arrives.
fn carry(sock: StdTcpStream) {
    if let Ok(mut up) = sock.try_clone() {
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 32 * 1024];
            while let Ok(n @ 1..) = stdin.read(&mut buf) {
                if up.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            let _ = up.shutdown(Shutdown::Write);
        });
    }
    let mut down = sock;
    let mut out = std::io::stdout().lock();
    let mut buf = [0u8; 32 * 1024];
    while let Ok(n @ 1..) = down.read(&mut buf) {
        if out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_door_lets_in_only_its_own_requests() {
        let (secret, nonce) = ("ab".repeat(32), "0f".repeat(32));
        assert_eq!(parse_hello(&format!("{HELLO} {nonce}\r")), Some(nonce.clone()));
        assert_eq!(parse_hello(&format!("{HELLO} {}", "0f".repeat(8))), None);
        assert_eq!(parse_hello(&format!("SSHEESH-TOR1 {nonce}")), None);
        let helper = proof(&secret, "helper", &nonce);
        assert_eq!(parse_request(&format!("{helper} example.com 22"), &helper), Some(("example.com".into(), 22)));
        assert_eq!(parse_request(&format!("{helper} 2001:db8::1 2222\r"), &helper), Some(("2001:db8::1".into(), 2222)));
        // The app's proof isn't the helper's, nor another nonce's; a bad port
        // or a name a line could break on is refused.
        assert_eq!(parse_request(&format!("{} example.com 22", proof(&secret, "app", &nonce)), &helper), None);
        assert_eq!(parse_request(&format!("{} example.com 22", proof(&secret, "helper", &"1a".repeat(32))), &helper), None);
        assert_eq!(parse_request(&format!("{secret} example.com 22"), &helper), None);
        assert_eq!(parse_request(&format!("{helper} example.com 99999"), &helper), None);
        assert_eq!(parse_request(&format!("{helper} a;b 22"), &helper), None);
        assert_eq!(parse_request(&format!("{helper} example.com 22 extra"), &helper), None);
    }

    #[test]
    fn servers_with_a_hop_of_their_own_or_a_strange_address_are_refused() {
        if std::process::Command::new(crate::ssh::ssh_program()).arg("-V").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("sshee-tor-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config");
        std::fs::write(
            &config,
            "Host jumpy\n  HostName 10.0.0.5\n  ProxyJump bastion.example.com\nHost own\n  HostName 203.0.113.8\n  ProxyCommand nc %h %p\nHost plain\n  HostName 203.0.113.9\nHost evil\n  HostName x$(touch-it)y\n",
        )
        .unwrap();
        let target = |name: &str| crate::ssh::SshTarget { destination: name.into(), extra_args: vec!["-F".into(), config.display().to_string()], ..Default::default() };
        assert_eq!(check(&target("jumpy")).refusal, Some(Refusal::Hop));
        assert_eq!(check(&target("own")).refusal, Some(Refusal::Hop));
        assert_eq!(check(&target("evil")).refusal, Some(Refusal::Address));
        let plain = check(&target("plain"));
        assert_eq!((plain.refusal, plain.host.as_deref()), (None, Some("203.0.113.9")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refused_server_never_reaches_the_shell_by_name() {
        let line = refusing_line("/opt/SSHeesh/ssheesh", Refusal::Hop);
        assert!(!line.contains("%h"), "{line}");
        assert!(line.ends_with("--tor-refuse hop"), "{line}");
        assert_eq!(Refusal::from_code("address"), Some(Refusal::Address));
        assert_eq!(Refusal::from_code("other"), None);
    }

    #[test]
    fn addresses_a_shell_would_act_on_are_refused() {
        for bad in ["x$(touch /tmp/p)y", "a;b", "a`b`", "a b", "a|b", "a'b", "a&b", ""] {
            assert!(!valid_host(bad), "{bad}");
        }
        for good in ["server.example.com", "203.0.113.7", "2001:db8::1", "pi.local", "my_host-1"] {
            assert!(valid_host(good), "{good}");
        }
    }

    #[test]
    fn public_servers_go_through_tor_by_name() {
        // Never looked up here: the name itself goes to Tor.
        assert_eq!(way("server.example.com", 22), Way::Tor("server.example.com".into()));
        assert_eq!(way("203.0.113.7", 22), Way::Tor("203.0.113.7".into()));
        assert_eq!(way("2001:db8::1", 22), Way::Tor("2001:db8::1".into()));
        assert_eq!(way("[2001:db8::1]", 22), Way::Tor("2001:db8::1".into()));
        assert_eq!(way("abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwx.onion", 22), Way::Tor("abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwx.onion".into()));
    }

    #[test]
    fn servers_on_your_network_stay_there() {
        for ip in ["192.168.1.10", "10.0.0.5", "172.16.3.4", "127.0.0.1", "100.101.102.103", "169.254.1.1", "::1", "fd00::5", "fe80::1"] {
            let parsed: IpAddr = ip.parse().unwrap();
            assert_eq!(way(ip, 22), Way::Near(vec![SocketAddr::new(parsed, 22)]), "{ip}");
        }
        assert!(matches!(way("localhost", 22), Way::Near(_)));
        // Just outside the private ranges.
        assert_eq!(way("100.128.0.1", 22), Way::Tor("100.128.0.1".into()));
        assert_eq!(way("172.32.0.1", 22), Way::Tor("172.32.0.1".into()));
    }

    #[test]
    fn local_names_are_only_the_ones_never_asked_of_dns_out_there() {
        for n in ["localhost", "pi.local", "dev.localhost"] {
            assert!(near_name(n), "{n}");
        }
        // A short name or a .lan one can reach your provider's DNS with a search domain.
        for n in ["nas", "box.lan", "router.home.arpa", "db.internal", "dev.tailnet-1234.ts.net", "example.com", "local.example.com"] {
            assert!(!near_name(n), "{n}");
        }
        assert_eq!(way("nas", 22), Way::Tor("nas".into()));
    }

    #[test]
    fn ssh_starts_the_helper_with_its_path_quoted() {
        let line = command_line("/Applications/SSHeesh.app/Contents/MacOS/SSHeesh");
        if cfg!(windows) {
            assert_eq!(line, "\"/Applications/SSHeesh.app/Contents/MacOS/SSHeesh\" --tor-proxy %h %p");
        } else {
            assert_eq!(line, "'/Applications/SSHeesh.app/Contents/MacOS/SSHeesh' --tor-proxy %h %p");
            assert_eq!(command_line("/home/me/it's 100%/ssheesh"), r"'/home/me/it'\''s 100%%/ssheesh' --tor-proxy %h %p");
        }
    }

    #[test]
    fn nothing_is_looked_up_on_this_computer() {
        let o = ssh_options("x %h %p");
        assert_eq!(o.first().map(String::as_str), Some("-o"));
        for want in ["ProxyCommand=x %h %p", "CanonicalizeHostname=no", "VerifyHostKeyDNS=no", "CheckHostIP=no"] {
            assert!(o.iter().any(|a| a == want), "{want}");
        }
    }

    #[test]
    fn failures_dont_name_the_server() {
        assert_eq!(explain(Some(Lost::TimedOut), 22), "The server didn't answer through Tor.");
        assert_eq!(explain(None, 22), "The server didn't answer through Tor.");
    }
}
