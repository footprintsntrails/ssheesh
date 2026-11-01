//! Tunnels: a server's port, here on this computer.
//!
//! A local forward makes `localhost:PORT` on this computer reach a port as
//! seen from the server; a SOCKS proxy sends whatever uses it out through the
//! server. Each tunnel is its own `ssh -N` process, restarted with backoff
//! when the connection drops. Tunnels only listen on 127.0.0.1, so nothing
//! else on your network can use them.
//!
//! `scan` lists what listens on TCP on the server and, when curl is there,
//! which of those answer like a website and their page titles, all in one
//! ssh run.

use crate::checkup::{is_local, listeners, Listener, PORTS, PROGRAMS};
use crate::error::{ErrorCode, OpError};
use crate::session::backoff_ms;
use crate::ssh::{common_args, hide_console, password_env, remote_command, spawn_error, ssh_program, SshEnv, SshTarget, Target};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- forwards

/// What a tunnel does. Serialized as `{ kind: "local" | "socks", ... }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Forward {
    /// `localhost:local_port` here reaches `remote_host:remote_port` as seen
    /// from the server. An empty `remote_host` means the server itself.
    Local { local_port: u16, remote_host: String, remote_port: u16 },
    /// A SOCKS proxy on `localhost:local_port`: apps set to use it reach the
    /// internet (and the server's network) through the server.
    Socks { local_port: u16 },
}

impl Forward {
    pub fn local_port(&self) -> u16 {
        match self {
            Forward::Local { local_port, .. } | Forward::Socks { local_port } => *local_port,
        }
    }

    /// ssh's `-L` or `-D` arguments, bound to 127.0.0.1 only.
    pub fn args(&self) -> Vec<String> {
        match self {
            Forward::Local { local_port, remote_host, remote_port } => {
                let host = bracket(or_server(remote_host));
                vec!["-L".into(), format!("127.0.0.1:{local_port}:{host}:{remote_port}")]
            }
            Forward::Socks { local_port } => vec!["-D".into(), format!("127.0.0.1:{local_port}")],
        }
    }

    fn check(&self) -> Result<(), OpError> {
        if self.local_port() == 0 {
            return Err(invalid("Pick a port on this computer, from 1 to 65535."));
        }
        if let Forward::Local { remote_host, remote_port, .. } = self {
            if *remote_port == 0 {
                return Err(invalid("Pick the server's port, from 1 to 65535."));
            }
            let host = remote_host.trim();
            if !host.is_empty() && !valid_host(host) {
                return Err(invalid(format!("\"{host}\" isn't an address SSHeesh can use.")));
            }
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> OpError {
    OpError::failed(message)
}

/// The address to reach, with the server itself as the default.
fn or_server(host: &str) -> &str {
    match host.trim() {
        "" => "127.0.0.1",
        h => h,
    }
}

/// A hostname or IP address, and nothing ssh could read as an option.
fn valid_host(host: &str) -> bool {
    host.len() <= 253
        && !host.starts_with('-')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%' | '[' | ']'))
}

/// IPv6 addresses go in brackets so ssh can tell their colons from its own.
fn bracket(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// The whole `ssh` command line for a tunnel, after the program name.
///
/// Each tunnel signs in on its own: through a shared connection the forward
/// would live in the sharing process and outlast Stop. `-v` is there because
/// ssh says nothing once a tunnel is up otherwise; the debug lines are only
/// read here, never shown.
pub fn tunnel_args(t: &SshTarget, env: &SshEnv, forward: &Forward) -> Vec<String> {
    let solo = SshEnv { control_dir: None, ..env.clone() };
    let mut a = common_args(t, &solo);
    for opt in ["ControlMaster=no", "ControlPath=none", "ExitOnForwardFailure=yes"] {
        a.push("-o".into());
        a.push(opt.into());
    }
    // Same as one-off commands: askpass answers a saved password, and
    // without one ssh mustn't stop to ask anything.
    if password_env(t, env).is_none() {
        a.push("-o".into());
        a.push("BatchMode=yes".into());
    }
    a.push("-v".into());
    a.extend(forward.args());
    a.extend(["-N".into(), "-T".into(), "--".into(), t.destination.clone()]);
    a
}

fn tunnel_command(t: &SshTarget, env: &SshEnv, forward: &Forward) -> Command {
    let mut cmd = Command::new(ssh_program());
    cmd.args(tunnel_args(t, env, forward));
    if let Some(vars) = password_env(t, env) {
        cmd.envs(vars);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    hide_console(&mut cmd);
    cmd
}

// ---------------------------------------------------------------- local ports

/// Why a tunnel can't listen on `port` on this computer, if it can't.
pub fn port_problem(port: u16) -> Option<OpError> {
    if port == 0 {
        return Some(invalid("Pick a port on this computer, from 1 to 65535."));
    }
    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            return Some(OpError::new(
                ErrorCode::Failed,
                format!("Port {port} on this computer needs administrator rights. Pick one above 1023."),
            ))
        }
        Err(_) => return Some(in_use(port)),
        Ok(_) => {}
    }
    // Browsers try localhost's IPv6 address first, so a program listening
    // there would answer instead of the tunnel.
    match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
        Err(e) if e.kind() == ErrorKind::AddrInUse => Some(in_use(port)),
        _ => None,
    }
}

pub fn port_free(port: u16) -> bool {
    port_problem(port).is_none()
}

pub fn in_use(port: u16) -> OpError {
    OpError::new(ErrorCode::Exists, format!("Port {port} on this computer is already in use."))
}

/// A free port on this computer for a server's `preferred` port: the same
/// number when it's free, else a look-alike (80 → 8080, 3000 → 13000), else
/// any the system hands out. `avoid` are ports other tunnels have claimed.
pub fn pick_local_port(preferred: u16, avoid: &[u16]) -> Result<u16, OpError> {
    let ok = |p: u16| p != 0 && !avoid.contains(&p) && port_free(p);
    let mut candidates = vec![preferred];
    if preferred < 1024 {
        candidates.push(preferred + 8000);
    }
    candidates.extend(preferred.checked_add(10000));
    if let Some(p) = candidates.into_iter().find(|&p| ok(p)) {
        return Ok(p);
    }
    for _ in 0..20 {
        let p = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port();
        if ok(p) {
            return Ok(p);
        }
    }
    Err(OpError::failed("Couldn't find a free port on this computer."))
}

// ---------------------------------------------------------------- running tunnels

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TunnelState {
    Starting,
    /// `warning` says why the last connection through it went nowhere.
    On { warning: Option<String> },
    Reconnecting { attempt: u32, retry_in_ms: u64, reason: String },
    /// Stopped for good: retrying can't help.
    Failed { message: String },
}

pub type StateSink = Arc<dyn Fn(TunnelState) + Send + Sync>;

/// The running tunnels, by the id the UI gave them.
#[derive(Default)]
pub struct TunnelManager {
    tunnels: Mutex<HashMap<String, Arc<Tunnel>>>,
}

struct Tunnel {
    /// The ssh command line; starting the same id the same way keeps it running.
    args: Vec<String>,
    port: u16,
    stopped: AtomicBool,
    /// The loop gave up (Failed).
    done: AtomicBool,
    child: Mutex<Option<Child>>,
    sink: Mutex<StateSink>,
    state: Mutex<TunnelState>,
    nap: Mutex<()>,
    wake: Condvar,
}

impl Tunnel {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn set(&self, state: TunnelState) {
        if self.stopped() {
            return;
        }
        *self.state.lock().unwrap() = state.clone();
        let sink = self.sink.lock().unwrap().clone();
        sink(state);
    }

    /// Ends the ssh process and the loop around it.
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _guard = self.nap.lock().unwrap();
        self.wake.notify_all();
    }

    /// Sleeps for `ms` unless the tunnel is stopped first.
    fn nap(&self, ms: u64) {
        let guard = self.nap.lock().unwrap();
        let _ = self.wake.wait_timeout_while(guard, Duration::from_millis(ms), |_| !self.stopped());
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts tunnel `id`, or, when it's already running the same way, just
    /// sends its state to the new `sink` (the window was reloaded). Fails
    /// right away when the port here is taken.
    pub fn start(&self, id: &str, target: &SshTarget, env: &SshEnv, forward: Forward, sink: StateSink) -> Result<(), OpError> {
        forward.check()?;
        let args = tunnel_args(target, env, &forward);
        let port = forward.local_port();
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(t) = tunnels.get(id) {
            if t.args == args && !t.stopped() && !t.done.load(Ordering::SeqCst) {
                *t.sink.lock().unwrap() = sink.clone();
                let now = t.state.lock().unwrap().clone();
                sink(now);
                return Ok(());
            }
        }
        if let Some(old) = tunnels.remove(id) {
            old.stop();
        }
        if tunnels.values().any(|t| t.port == port && !t.done.load(Ordering::SeqCst)) {
            return Err(OpError::new(ErrorCode::Exists, format!("Another tunnel already uses port {port} on this computer.")));
        }
        if let Some(problem) = port_problem(port) {
            return Err(problem);
        }
        let tunnel = Arc::new(Tunnel {
            args,
            port,
            stopped: AtomicBool::new(false),
            done: AtomicBool::new(false),
            child: Mutex::new(None),
            sink: Mutex::new(sink),
            state: Mutex::new(TunnelState::Starting),
            nap: Mutex::new(()),
            wake: Condvar::new(),
        });
        tunnels.insert(id.to_string(), tunnel.clone());
        drop(tunnels);
        tunnel.set(TunnelState::Starting);
        let (target, env) = (target.clone(), env.clone());
        std::thread::Builder::new()
            .name(format!("tunnel-{id}"))
            .spawn(move || run(tunnel, target, env, forward))
            .map_err(|e| OpError::failed(format!("Couldn't start the tunnel: {e}")))?;
        Ok(())
    }

    pub fn stop(&self, id: &str) {
        let removed = self.tunnels.lock().unwrap().remove(id);
        if let Some(t) = removed {
            t.stop();
        }
    }

    /// Ends every tunnel's ssh process; called when the app quits.
    pub fn stop_all(&self) {
        let all: Vec<Arc<Tunnel>> = self.tunnels.lock().unwrap().drain().map(|(_, t)| t).collect();
        for t in all {
            t.stop();
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[derive(Default)]
struct Exit {
    /// ssh's last lines, without its debug chatter.
    lines: Vec<String>,
    /// When the tunnel came up, if it did.
    up_at: Option<Instant>,
}

fn run(t: Arc<Tunnel>, target: SshTarget, env: SshEnv, forward: Forward) {
    let mut failures = 0u32;
    let mut ever_up = false;
    while !t.stopped() {
        if failures > 0 {
            if let Some(problem) = port_problem(t.port) {
                t.set(TunnelState::Failed { message: problem.message });
                break;
            }
        }
        let exit = match once(&t, &target, &env, &forward) {
            Ok(exit) => exit,
            Err(e) => {
                t.set(TunnelState::Failed { message: e.message });
                break;
            }
        };
        if t.stopped() {
            break;
        }
        ever_up |= exit.up_at.is_some();
        match classify(&exit.lines, &forward, ever_up) {
            Err(message) => {
                t.set(TunnelState::Failed { message });
                break;
            }
            Ok(reason) => {
                // A tunnel that worked for a while starts the backoff over.
                if exit.up_at.is_some_and(|at| at.elapsed() > Duration::from_secs(10)) {
                    failures = 0;
                }
                failures += 1;
                let delay = backoff_ms(failures);
                t.set(TunnelState::Reconnecting { attempt: failures, retry_in_ms: delay, reason });
                t.nap(delay);
            }
        }
    }
    t.done.store(true, Ordering::SeqCst);
}

/// Runs ssh once, until it exits or the tunnel is stopped.
fn once(t: &Arc<Tunnel>, target: &SshTarget, env: &SshEnv, forward: &Forward) -> Result<Exit, OpError> {
    let mut child = tunnel_command(target, env, forward).spawn().map_err(spawn_error)?;
    let stderr = child.stderr.take();
    {
        // Checked under the lock, so Stop either finds this child or the
        // loop sees it was stopped.
        let mut slot = t.child.lock().unwrap();
        if t.stopped() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Exit::default());
        }
        *slot = Some(child);
    }
    let (tx, rx) = mpsc::channel();
    let (reader_t, reader_fwd) = (t.clone(), forward.clone());
    std::thread::spawn(move || {
        let _ = tx.send(read_stderr(stderr, &reader_t, &reader_fwd));
    });
    loop {
        {
            let mut slot = t.child.lock().unwrap();
            match slot.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                // Exited. None: Stop took it and ended it.
                Some(_) => {
                    if let Some(mut c) = slot.take() {
                        let _ = c.wait();
                    }
                    break;
                }
                None => break,
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default())
}

/// Follows what ssh says: "up" once it's listening, and warnings when a
/// connection through the tunnel goes nowhere.
fn read_stderr(stderr: Option<ChildStderr>, t: &Tunnel, forward: &Forward) -> Exit {
    let mut exit = Exit::default();
    let Some(stderr) = stderr else { return exit };
    let mut reader = BufReader::new(stderr);
    let mut buf = Vec::new();
    let mut failed_channels: HashSet<String> = HashSet::new();
    let mut warning: Option<String> = None;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        if exit.up_at.is_none() {
            if is_up(line) {
                exit.up_at = Some(Instant::now());
                t.set(TunnelState::On { warning: None });
            }
        } else {
            let next = match channel_event(line) {
                Some((channel, ChannelEvent::Failed(why))) => {
                    failed_channels.insert(channel);
                    open_failed_warning(&why, forward).or_else(|| warning.clone())
                }
                // A connection that went through clears the warning.
                Some((channel, ChannelEvent::Closed)) if !failed_channels.remove(&channel) => None,
                _ => warning.clone(),
            };
            if next != warning {
                warning = next;
                t.set(TunnelState::On { warning: warning.clone() });
            }
        }
        if !line.starts_with("debug") {
            exit.lines.push(line.to_string());
            if exit.lines.len() > 40 {
                exit.lines.remove(0);
            }
        }
    }
    exit
}

/// ssh got through login and is listening. (Its "Local forwarding
/// listening on" note comes before it actually binds the port, so it's no
/// sign; with ExitOnForwardFailure this one only comes once every forward is.)
fn is_up(line: &str) -> bool {
    line.contains("Entering interactive session")
}

enum ChannelEvent {
    Failed(String),
    Closed,
}

/// "channel 3: open failed: connect failed: Connection refused" and
/// "debug1: channel 3: free: direct-tcpip: ..." (the forwarded connection ended).
fn channel_event(line: &str) -> Option<(String, ChannelEvent)> {
    let rest = &line[line.find("channel ")? + "channel ".len()..];
    let (channel, rest) = rest.split_once(": ")?;
    if !channel.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(why) = rest.strip_prefix("open failed: ") {
        return Some((channel.to_string(), ChannelEvent::Failed(why.to_string())));
    }
    let closed = rest.starts_with("free: direct-tcpip") || rest.starts_with("free: dynamic-tcpip");
    closed.then(|| (channel.to_string(), ChannelEvent::Closed))
}

fn open_failed_warning(why: &str, forward: &Forward) -> Option<String> {
    if why.contains("administratively prohibited") {
        return Some("The server doesn't allow tunnels: its SSH settings turn them off (AllowTcpForwarding).".into());
    }
    match forward {
        Forward::Local { remote_host, remote_port, .. } if why.contains("connect failed") => {
            Some(format!("Nothing answers at {}:{remote_port} on the server right now.", or_server(remote_host)))
        }
        // Through a SOCKS proxy some sites failing is normal.
        _ => None,
    }
}

/// After ssh exits: `Ok(why it dropped)` to try again, or `Err(why it
/// can't work)` to stop.
fn classify(lines: &[String], forward: &Forward, ever_up: bool) -> Result<String, String> {
    let has = |s: &str| lines.iter().any(|l| l.contains(s));
    let port = forward.local_port();
    if has("Permission denied") && has("bind ") {
        return Err(format!("Port {port} on this computer needs administrator rights. Pick one above 1023."));
    }
    if has("Address already in use") || has("cannot listen to port") || has("Could not request local forwarding") {
        return Err(in_use(port).message);
    }
    if has("Permission denied") || has("Too many authentication failures") {
        return Err("The server didn't accept your login. Open a terminal to it once, then try again.".into());
    }
    if has("IDENTIFICATION HAS CHANGED") || has("Host key verification failed") || has("forwarding disabled due to host key") {
        return Err("The server's identity couldn't be confirmed, so the tunnel stays off. Open a terminal to it to check.".into());
    }
    let unknown_host = has("Name or service not known") || has("nodename nor servname") || has("No such host is known");
    if unknown_host && !ever_up {
        return Err("Couldn't find that server. Check its address for typos.".into());
    }
    let last = lines
        .iter()
        .rev()
        .find(|l| !l.starts_with("Transferred:") && !l.starts_with("Bytes per second") && !l.starts_with("Authenticated to"))
        .map(String::as_str)
        .unwrap_or("");
    Ok(drop_reason(last))
}

fn drop_reason(line: &str) -> String {
    let l = line.to_ascii_lowercase();
    let reason = if l.contains("closed by remote host") || l.contains("connection closed") {
        "The server closed the connection"
    } else if l.contains("timeout") || l.contains("timed out") || l.contains("not responding") {
        "The server stopped responding"
    } else if l.contains("connection refused") {
        "The server refused the connection"
    } else if l.contains("network is unreachable") || l.contains("no route to host") {
        "No network connection"
    } else if l.contains("temporary failure in name resolution") || l.contains("could not resolve") {
        "No internet connection"
    } else if l.contains("broken pipe") || l.contains("reset by peer") {
        "The connection dropped"
    } else {
        "Connection lost"
    };
    reason.to_string()
}

// ---------------------------------------------------------------- web apps on the server

/// Something listening on TCP on the server.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebApp {
    pub port: u16,
    /// Where a tunnel to it connects, as seen from the server: 127.0.0.1
    /// unless it listens on one specific other address.
    pub remote_host: String,
    /// The program listening ("node", "nginx"), when the server says.
    pub process: Option<String>,
    /// What the port or program is known as ("PostgreSQL", "Docker").
    pub known_as: Option<String>,
    /// The page's `<title>`.
    pub title: Option<String>,
    /// "http" or "https", when it answered like a website.
    pub scheme: Option<String>,
    /// The HTTP status of its front page.
    pub status: Option<u16>,
    /// Likely a website: it answered HTTP, or it's a usual web port or program.
    pub web: bool,
    /// Also listens on an address other computers can reach.
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAppScan {
    /// Likely websites first, then by port.
    pub apps: Vec<WebApp>,
    /// curl is on the server, so pages could be asked for their titles.
    pub curl: bool,
    /// Ran as root (or sudo without a password), so every program is named.
    pub as_root: bool,
}

/// Lists ports (the same way the check-up does), waits for the list of ports
/// to ask, then asks each one's front page at once, in parallel. Only the
/// server's own addresses are asked; a redirect is followed only within the
/// same app.
const SCAN: &str = r##"export LC_ALL=C
PATH="$PATH:/usr/local/sbin:/usr/sbin:/sbin"
has() { command -v "$1" >/dev/null 2>&1; }
S=""; R=0
if [ "$(id -u)" = 0 ]; then R=1
elif has sudo && sudo -n true 2>/dev/null; then R=1; S="sudo -n"; fi
echo @@root@@; echo "$R"
echo @@curl@@; has curl && echo yes
echo @@listen@@
if has ss; then echo "#ss"; $S ss -tulnp || ss -tuln
elif has netstat; then echo "#netstat"; $S netstat -tulnp || netstat -tuln
fi 2>/dev/null
echo @@ready@@
IFS= read -r T || exit 0
set -f
look() {
  curl -s -k -m 1 -H 'Accept: text/html' -D - "$1://$2:$3$4" 2>/dev/null | head -c 200000 | tr -d '\000' | awk '
    st == 0 && /^HTTP\// { code = $2; sub(/\r$/, "", code); st = 1; next }
    st == 1 && /^\r?$/ { st = 2; next }
    st == 1 { if (tolower(substr($0, 1, 9)) == "location:") { loc = substr($0, 10); gsub(/^[ \t]+|[ \t\r]+$/, "", loc) } next }
    st == 2 && /^HTTP\// { code = $2; sub(/\r$/, "", code); loc = ""; st = 1; next }
    st == 2 { st = 3 }
    st == 3 { b = b " " $0; if (length(b) > 100000) exit }
    END {
      if (code == "") exit
      t = ""; i = index(tolower(b), "<title")
      if (i) { r = substr(b, i); j = index(r, ">"); if (j) { r = substr(r, j + 1); k = index(tolower(r), "</title"); if (k) r = substr(r, 1, k - 1); t = substr(r, 1, 300) } }
      gsub(/\t/, " ", t); gsub(/\t/, " ", loc)
      printf "%s\t%s\t%s\n", code, loc, t
    }'
}
probe() {
  o=$(look "$1" "$2" "$3" /)
  case "$o" in 3[0-9][0-9]"	/"*) l=${o#*	}; l=${l%%	*}; o2=$(look "$1" "$2" "$3" "$l"); [ -n "$o2" ] && o=$o2 ;; esac
  printf '%s' "$o"
}
n=0
for x in $T; do
  p=${x%%,*}; h=${x#*,}
  (
    s=http; o=$(probe http "$h" "$p")
    case "$o" in ""|"400	"*) o2=$(probe https "$h" "$p"); [ -n "$o2" ] && { s=https; o=$o2; } ;; esac
    [ -n "$o" ] && printf '@@app %s %s %s\n' "$p" "$s" "$o"
  ) &
  n=$((n + 1)); [ $((n % 16)) = 0 ] && wait
done
wait
echo @@end@@
exit 0"##;

/// Connecting can take 10 s, and each page gets a second or two.
const SCAN_LIMIT: Duration = Duration::from_secs(30);

/// At most this many ports get asked for a page.
const MAX_PROBES: usize = 48;

/// Ports and programs that don't speak HTTP, and could log a scary warning
/// when asked for a page (Redis does), so they aren't asked.
const NOT_HTTP_PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 110, 111, 139, 143, 389, 445, 465, 587, 636, 993, 995, 1433, 1521, 3306, 5432, 5672, 5900, 6379, 9042, 9092, 9300,
    11211, 25565, 27017, 33060,
];
const NOT_HTTP_PROGRAMS: &[&str] = &[
    "sshd", "postgres", "mysqld", "mariadbd", "redis-server", "valkey-server", "mongod", "memcached", "master", "exim4", "exim", "dovecot",
    "smbd", "rpcbind", "systemd-resolve", "dnsmasq", "named", "unbound", "slapd",
];

/// Usual homes of websites and web apps, for servers without curl.
const WEB_PORTS: &[u16] = &[80, 443, 3000, 3001, 4000, 4200, 5000, 5173, 8000, 8008, 8080, 8081, 8088, 8443, 8888, 9000, 9090, 9443];
const WEB_PROGRAMS: &[&str] = &[
    "nginx", "apache2", "httpd", "caddy", "lighttpd", "traefik", "node", "deno", "bun", "gunicorn", "uvicorn", "php", "ruby", "puma", "grafana",
    "grafana-server", "code-server", "gitea", "forgejo", "prometheus", "portainer", "uwsgi", "jellyfin", "hugo",
];

pub fn scan(target: &Target, env: &SshEnv) -> Result<WebAppScan, OpError> {
    let Target::Ssh(t) = target else {
        return Err(OpError::new(ErrorCode::Unsupported, "Tunnels are for servers."));
    };
    let mut cmd = remote_command(t, env, SCAN);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(spawn_error)?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if tx.send(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string()).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + SCAN_LIMIT;
    let (mut head, mut answers) = (Vec::new(), Vec::new());
    let mut found: Option<Found> = None;
    let mut timed_out = false;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) if found.is_none() && line.trim() == "@@ready@@" => {
                let f = Found::read(&head);
                let ask = if f.curl { probe_list(f.ports.as_deref().unwrap_or_default()) } else { String::new() };
                if let Some(mut pipe) = stdin.take() {
                    let _ = writeln!(pipe, "{ask}");
                }
                found = Some(f);
            }
            Ok(line) if found.is_none() => head.push(line),
            Ok(line) => answers.push(line),
            Err(RecvTimeoutError::Timeout) => {
                timed_out = true;
                let _ = child.kill();
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    let Some(found) = found else {
        return Err(if timed_out {
            OpError::new(ErrorCode::Network, "The server took too long to answer.")
        } else {
            OpError::from_ssh(status.code(), &errors)
        });
    };
    found.into_scan(&answers)
}

/// What the first half of the scan printed.
struct Found {
    as_root: bool,
    curl: bool,
    /// None when neither ss nor netstat is there.
    ports: Option<Vec<Port>>,
}

/// One TCP port, from all the addresses it listens on.
#[derive(Debug, Clone, PartialEq)]
struct Port {
    port: u16,
    hosts: Vec<String>,
    process: Option<String>,
}

impl Found {
    fn read(lines: &[String]) -> Self {
        let mut sections: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut current = "";
        for line in lines {
            if let Some(name) = line.trim().strip_prefix("@@").and_then(|r| r.strip_suffix("@@")) {
                current = name;
                sections.entry(name).or_default();
            } else {
                sections.entry(current).or_default().push(line);
            }
        }
        let first = |name: &str| sections.get(name).and_then(|l| l.first()).map(|l| l.trim()).unwrap_or("");
        Self {
            as_root: first("root") == "1",
            curl: first("curl") == "yes",
            ports: sections.get("listen").and_then(|l| ports(l)),
        }
    }

    fn into_scan(self, answers: &[String]) -> Result<WebAppScan, OpError> {
        let ports = self.ports.ok_or_else(|| {
            OpError::new(ErrorCode::Unsupported, "Neither ss nor netstat is installed on the server, so SSHeesh can't see what's listening.")
        })?;
        let answers: HashMap<u16, Answer> = answers.iter().filter_map(|l| parse_answer(l)).collect();
        let mut apps: Vec<WebApp> = ports
            .into_iter()
            .map(|p| {
                let answer = answers.get(&p.port);
                let process = p.process.as_deref();
                let known_web = WEB_PORTS.contains(&p.port) || process.is_some_and(|n| WEB_PROGRAMS.contains(&n));
                WebApp {
                    port: p.port,
                    remote_host: forward_host(&p.hosts),
                    known_as: known_as(p.port, process),
                    title: answer.and_then(|a| a.title.clone()),
                    scheme: answer.map(|a| a.scheme.clone()),
                    status: answer.map(|a| a.status),
                    web: answer.is_some() || known_web,
                    public: p.hosts.iter().any(|h| !is_local(h)),
                    process: p.process,
                }
            })
            .collect();
        apps.sort_by_key(|a| (!a.web, a.port));
        Ok(WebAppScan { apps, curl: self.curl, as_root: self.as_root })
    }
}

/// TCP ports from `ss`/`netstat` output, without SSH's own.
fn ports(listing: &[&str]) -> Option<Vec<Port>> {
    let mut by_port: BTreeMap<u16, Port> = BTreeMap::new();
    for Listener { udp, host, port, process } in listeners(listing)? {
        if udp {
            continue;
        }
        let entry = by_port.entry(port).or_insert_with(|| Port { port, hosts: Vec::new(), process: None });
        if !entry.hosts.contains(&host) {
            entry.hosts.push(host);
        }
        if entry.process.is_none() {
            entry.process = process;
        }
    }
    let is_ssh = |p: &Port| p.port == 22 || p.process.as_deref().is_some_and(|n| n.starts_with("sshd"));
    Some(by_port.into_values().filter(|p| !is_ssh(p)).collect())
}

/// An address from ss or netstat without brackets, and without the
/// interface of an IPv4 address ("127.0.0.53%lo").
fn bare(host: &str) -> String {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    match h.split_once('%') {
        Some((ip, _)) if ip.parse::<Ipv4Addr>().is_ok() => ip.to_string(),
        _ => h.to_string(),
    }
}

/// Where a tunnel to a port connects, as seen from the server: 127.0.0.1
/// when it listens on every address or on 127.0.0.1, else the one address it
/// listens on (IPv4 first), such as ::1 or a private network's address.
fn forward_host(hosts: &[String]) -> String {
    let addrs: Vec<String> = hosts.iter().map(|h| bare(h)).collect();
    let reaches_loopback = |a: &String| {
        a == "*"
            || match a.parse::<IpAddr>() {
                Ok(IpAddr::V4(v4)) => v4.is_unspecified() || v4 == Ipv4Addr::LOCALHOST,
                Ok(IpAddr::V6(v6)) => v6.is_unspecified() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_unspecified() || v4 == Ipv4Addr::LOCALHOST),
                Err(_) => false,
            }
    };
    if addrs.iter().any(reaches_loopback) {
        return "127.0.0.1".into();
    }
    let unmapped = |a: &String| match a.parse::<Ipv6Addr>().ok().and_then(|v6| v6.to_ipv4_mapped()) {
        Some(v4) => v4.to_string(),
        None => a.clone(),
    };
    let addrs: Vec<String> = addrs.iter().map(unmapped).collect();
    addrs
        .iter()
        .find(|a| a.parse::<Ipv4Addr>().is_ok())
        .or_else(|| addrs.first())
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".into())
}

/// The host part of a URL for `forward_host`'s address.
fn url_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{}]", host.replace('%', "%25"))
    } else {
        host.to_string()
    }
}

/// "3000,127.0.0.1 5173,[::1]": the ports to ask for a page, and where.
fn probe_list(ports: &[Port]) -> String {
    ports
        .iter()
        .filter(|p| !NOT_HTTP_PORTS.contains(&p.port) && !p.process.as_deref().is_some_and(|n| NOT_HTTP_PROGRAMS.contains(&n)))
        .map(|p| format!("{},{}", p.port, url_host(&forward_host(&p.hosts))))
        .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b".,:[]%".contains(&b)))
        .take(MAX_PROBES)
        .collect::<Vec<_>>()
        .join(" ")
}

fn known_as(port: u16, process: Option<&str>) -> Option<String> {
    let by_program = process.and_then(|p| PROGRAMS.iter().find(|(n, ..)| *n == p)).map(|&(_, name, _)| name);
    let by_port = PORTS.iter().find(|(p, ..)| *p == port).map(|&(_, name, _)| name).filter(|n| *n != "web");
    by_program.or(by_port).map(str::to_string)
}

struct Answer {
    scheme: String,
    status: u16,
    title: Option<String>,
}

/// "@@app 3000 http 200<TAB><TAB>Grafana" (the middle field is a redirect's target).
fn parse_answer(line: &str) -> Option<(u16, Answer)> {
    let rest = line.strip_prefix("@@app ")?;
    let (port, rest) = rest.split_once(' ')?;
    let (scheme, rest) = rest.split_once(' ')?;
    let mut fields = rest.split('\t');
    let status = fields.next()?.trim().parse().ok()?;
    let title = fields.nth(1).and_then(clean_title);
    let scheme = if scheme == "https" { "https" } else { "http" }.to_string();
    Some((port.parse().ok()?, Answer { scheme, status, title }))
}

fn clean_title(raw: &str) -> Option<String> {
    let text = decode_entities(raw);
    let words: Vec<&str> = text.split_whitespace().collect();
    let title: String = words.join(" ").chars().filter(|c| !c.is_control()).take(120).collect();
    (!title.is_empty()).then_some(title)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.char_indices().take(12).find(|&(_, c)| c == ';').map(|(i, _)| i);
        match end.and_then(|e| entity(&rest[1..e]).map(|c| (e, c))) {
            Some((e, c)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(dec) = name.strip_prefix('#') {
        return dec.parse().ok().and_then(char::from_u32);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "middot" => '·',
        "bull" => '•',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SshTarget {
        SshTarget { destination: "app".into(), user: Some("deploy".into()), port: Some(2222), ..Default::default() }
    }

    fn local(local_port: u16, remote_host: &str, remote_port: u16) -> Forward {
        Forward::Local { local_port, remote_host: remote_host.into(), remote_port }
    }

    #[test]
    fn forwards_bind_this_computer_only() {
        assert_eq!(local(8080, "", 3000).args(), ["-L", "127.0.0.1:8080:127.0.0.1:3000"]);
        assert_eq!(local(8080, " 10.0.0.5 ", 80).args(), ["-L", "127.0.0.1:8080:10.0.0.5:80"]);
        assert_eq!(local(5173, "::1", 5173).args(), ["-L", "127.0.0.1:5173:[::1]:5173"]);
        assert_eq!(Forward::Socks { local_port: 1080 }.args(), ["-D", "127.0.0.1:1080"]);
    }

    #[test]
    fn forwards_are_checked() {
        assert!(local(0, "", 80).check().is_err());
        assert!(local(8080, "", 0).check().is_err());
        assert!(local(8080, "-oProxyCommand=sh", 80).check().is_err());
        assert!(local(8080, "db host", 80).check().is_err());
        assert!(local(8080, "db.internal", 5432).check().is_ok());
        assert!(local(8080, "[fe80::1%eth0]", 80).check().is_ok());
        assert!(Forward::Socks { local_port: 0 }.check().is_err());
    }

    #[test]
    fn forwards_come_from_the_ui_as_tagged_json() {
        let f: Forward = serde_json::from_str(r#"{"kind":"local","localPort":8080,"remoteHost":"127.0.0.1","remotePort":3000}"#).unwrap();
        assert_eq!(f, local(8080, "127.0.0.1", 3000));
        let s: Forward = serde_json::from_str(r#"{"kind":"socks","localPort":1080}"#).unwrap();
        assert_eq!(s, Forward::Socks { local_port: 1080 });
    }

    #[test]
    fn tunnel_ssh_signs_in_on_its_own_and_never_asks() {
        let env = SshEnv { control_dir: Some("/tmp/cm".into()), askpass: None };
        let a = tunnel_args(&target(), &env, &local(8080, "", 3000));
        let has = |pair: [&str; 2]| a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["-o", "ExitOnForwardFailure=yes"]));
        assert!(has(["-o", "ControlMaster=no"]));
        assert!(has(["-o", "ControlPath=none"]));
        assert!(has(["-o", "BatchMode=yes"]));
        assert!(has(["-L", "127.0.0.1:8080:127.0.0.1:3000"]));
        assert!(has(["-p", "2222"]));
        assert!(!a.iter().any(|x| x.starts_with("ControlMaster=auto") || x.starts_with("ControlPath=/")));
        assert_eq!(a[a.len() - 4..], ["-N", "-T", "--", "app"]);
    }

    #[test]
    fn a_saved_password_goes_through_askpass_instead_of_batch_mode() {
        let mut t = target();
        t.password = Some(crate::secrets::Secret::new("hunter2"));
        let env = SshEnv { control_dir: None, askpass: Some("/opt/ssheesh/ssheesh".into()) };
        let a = tunnel_args(&t, &env, &Forward::Socks { local_port: 1080 });
        assert!(!a.iter().any(|x| x == "BatchMode=yes"));
        assert!(a.iter().any(|x| x == "NumberOfPasswordPrompts=1"));
        assert!(!a.iter().any(|x| x.contains("hunter2")));
        let cmd = tunnel_command(&t, &env, &Forward::Socks { local_port: 1080 });
        assert!(cmd.get_envs().any(|(k, _)| k == crate::secrets::ASKPASS_FLAG));
    }

    #[test]
    fn local_ports_in_use_are_spotted_and_avoided() {
        let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = taken.local_addr().unwrap().port();
        assert!(!port_free(port));
        assert_eq!(port_problem(port).unwrap().message, format!("Port {port} on this computer is already in use."));
        let picked = pick_local_port(port, &[]).unwrap();
        assert_ne!(picked, port);
        assert!(port_free(picked));
        drop(taken);
        // Freed, it's picked again, unless a test running alongside took that port meanwhile.
        let again = pick_local_port(port, &[]).unwrap();
        assert!(again == port || !port_free(port), "picked {again} though {port} is free");
        assert_ne!(pick_local_port(port, &[port]).unwrap(), port);
    }

    #[test]
    fn a_port_taken_here_is_said_plainly_and_isnt_retried() {
        let lines = ["bind [127.0.0.1]:8080: Address already in use", "channel_setup_fwd_listener_tcpip: cannot listen to port: 8080", "Could not request local forwarding."]
            .map(String::from);
        assert_eq!(classify(&lines, &local(8080, "", 3000), true), Err("Port 8080 on this computer is already in use.".into()));
        let low = ["bind [127.0.0.1]:80: Permission denied", "Could not request local forwarding."].map(String::from);
        assert!(classify(&low, &local(80, "", 80), false).unwrap_err().contains("administrator"));
    }

    #[test]
    fn login_problems_stop_network_problems_retry() {
        let denied = ["root@203.0.113.9: Permission denied (publickey).".to_string()];
        assert!(classify(&denied, &local(8080, "", 80), false).unwrap_err().contains("didn't accept your login"));
        let changed = ["Port forwarding is disabled to avoid man-in-the-middle attacks.", "Error: forwarding disabled due to host key check failure"].map(String::from);
        assert!(classify(&changed, &local(8080, "", 80), false).unwrap_err().contains("identity"));
        let dropped = ["Authenticated to 203.0.113.9 ([203.0.113.9]:22) using \"publickey\".", "Timeout, server 203.0.113.9 not responding."].map(String::from);
        assert_eq!(classify(&dropped, &local(8080, "", 80), true), Ok("The server stopped responding".into()));
        let closed = ["Connection to 203.0.113.9 closed by remote host.", "Transferred: sent 3520, received 3440 bytes, in 3.9 seconds"].map(String::from);
        assert_eq!(classify(&closed, &local(8080, "", 80), true), Ok("The server closed the connection".into()));
        let unknown = ["ssh: Could not resolve hostname app: Name or service not known".to_string()];
        assert!(classify(&unknown, &local(8080, "", 80), false).is_err());
        assert!(classify(&unknown, &local(8080, "", 80), true).is_ok());
    }

    #[test]
    fn reads_what_ssh_says_about_connections() {
        assert!(is_up("debug1: Entering interactive session."));
        assert!(!is_up("debug1: Local forwarding listening on 127.0.0.1 port 8080."));
        assert!(!is_up("debug1: Authenticating to 203.0.113.9:22 as 'deploy'"));
        let Some((ch, ChannelEvent::Failed(why))) = channel_event("channel 3: open failed: connect failed: Connection refused") else { panic!() };
        assert_eq!(ch, "3");
        assert_eq!(
            open_failed_warning(&why, &local(8080, "", 3000)).unwrap(),
            "Nothing answers at 127.0.0.1:3000 on the server right now."
        );
        assert!(open_failed_warning("administratively prohibited: open failed", &Forward::Socks { local_port: 1080 }).unwrap().contains("AllowTcpForwarding"));
        assert!(open_failed_warning("connect failed: Name or service not known", &Forward::Socks { local_port: 1080 }).is_none());
        let closed = "debug1: channel 3: free: direct-tcpip: listening port 8080 for 127.0.0.1 port 3000, connect from 127.0.0.1 port 51000 to 127.0.0.1 port 8080, nchannels 2";
        assert!(matches!(channel_event(closed), Some((c, ChannelEvent::Closed)) if c == "3"));
        assert!(channel_event("debug1: channel 0: new port-listener [port listener] (inactive timeout: 0)").is_none());
    }

    #[test]
    fn states_serialize_for_the_ui() {
        let json = serde_json::to_string(&TunnelState::Reconnecting { attempt: 2, retry_in_ms: 2000, reason: "x".into() }).unwrap();
        assert_eq!(json, r#"{"kind":"reconnecting","attempt":2,"retryInMs":2000,"reason":"x"}"#);
        assert_eq!(serde_json::to_string(&TunnelState::On { warning: None }).unwrap(), r#"{"kind":"on","warning":null}"#);
        assert_eq!(serde_json::to_string(&TunnelState::Starting).unwrap(), r#"{"kind":"starting"}"#);
    }

    #[test]
    fn picks_where_a_tunnel_connects_on_the_server() {
        let hosts = |h: &[&str]| h.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(forward_host(&hosts(&["0.0.0.0", "[::]"])), "127.0.0.1");
        assert_eq!(forward_host(&hosts(&["*"])), "127.0.0.1");
        assert_eq!(forward_host(&hosts(&["::"])), "127.0.0.1");
        assert_eq!(forward_host(&hosts(&["[::1]", "127.0.0.1"])), "127.0.0.1");
        assert_eq!(forward_host(&hosts(&["[::1]"])), "::1");
        assert_eq!(forward_host(&hosts(&["10.0.0.5"])), "10.0.0.5");
        assert_eq!(forward_host(&hosts(&["[fd00::5]", "198.51.100.4"])), "198.51.100.4");
        assert_eq!(forward_host(&hosts(&["127.0.0.53%lo"])), "127.0.0.53");
        assert_eq!(forward_host(&hosts(&["[::ffff:127.0.0.1]"])), "127.0.0.1");
        assert_eq!(forward_host(&hosts(&["[::ffff:10.1.2.3]"])), "10.1.2.3");
    }

    const SS: &str = r#"@@root@@
1
@@curl@@
yes
@@listen@@
#ss
Netid State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process
udp   UNCONN 0      0      127.0.0.53%lo:53         0.0.0.0:*     users:(("systemd-resolve",pid=571,fd=13))
tcp   LISTEN 0      4096   127.0.0.53%lo:53         0.0.0.0:*     users:(("systemd-resolve",pid=571,fd=14))
tcp   LISTEN 0      128          0.0.0.0:22         0.0.0.0:*     users:(("sshd",pid=880,fd=3))
tcp   LISTEN 0      511        127.0.0.1:3000       0.0.0.0:*     users:(("grafana",pid=1201,fd=9))
tcp   LISTEN 0      244        127.0.0.1:5432       0.0.0.0:*     users:(("postgres",pid=990,fd=6))
tcp   LISTEN 0      511          0.0.0.0:80         0.0.0.0:*     users:(("nginx",pid=1100,fd=6))
tcp   LISTEN 0      511             [::]:80            [::]:*     users:(("nginx",pid=1100,fd=7))
tcp   LISTEN 0      511            [::1]:5173          [::]:*     users:(("node",pid=1302,fd=21))
tcp   LISTEN 0      4096        10.0.0.5:9100       0.0.0.0:*     users:(("node_exporter",pid=777,fd=3))
tcp   LISTEN 0      511                *:6379             *:*     users:(("redis-server",pid=660,fd=6))
tcp   LISTEN 0      128             [::]:22            [::]:*     users:(("sshd",pid=880,fd=4))"#;

    fn head(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn asks_only_ports_that_may_speak_http() {
        let found = Found::read(&head(SS));
        assert!(found.as_root && found.curl);
        assert_eq!(probe_list(found.ports.as_deref().unwrap()), "80,127.0.0.1 3000,127.0.0.1 5173,[::1] 9100,10.0.0.5");
    }

    #[test]
    fn web_apps_come_first_with_their_titles() {
        let answers = head(
            "@@app 3000 http 200\t\tGrafana\n@@app 80 http 200\t\tWelcome to  nginx!\n@@app 5173 http 200\t\t<b>Vite</b> &amp; React &#8211; dev\n@@app 9100 http 404\t\t\n@@end@@",
        );
        let scan = Found::read(&head(SS)).into_scan(&answers).unwrap();
        let ports: Vec<u16> = scan.apps.iter().map(|a| a.port).collect();
        assert_eq!(ports, [80, 3000, 5173, 9100, 53, 5432, 6379]);
        let by = |p: u16| scan.apps.iter().find(|a| a.port == p).unwrap();
        assert_eq!(by(80).title.as_deref(), Some("Welcome to nginx!"));
        assert!(by(80).public);
        assert_eq!(by(3000).title.as_deref(), Some("Grafana"));
        assert!(!by(3000).public);
        assert_eq!(by(5173).title.as_deref(), Some("<b>Vite</b> & React – dev"));
        assert_eq!(by(5173).remote_host, "::1");
        assert_eq!(by(9100).status, Some(404));
        assert_eq!(by(9100).remote_host, "10.0.0.5");
        assert!(by(9100).web && by(9100).title.is_none());
        assert_eq!(by(5432).known_as.as_deref(), Some("PostgreSQL"));
        assert!(!by(5432).web);
        assert_eq!(by(6379).known_as.as_deref(), Some("Redis"));
        assert!(by(6379).public);
        assert_eq!(by(53).remote_host, "127.0.0.53");
    }

    #[test]
    fn without_curl_usual_web_ports_still_count() {
        let text = SS.replace("@@curl@@\nyes", "@@curl@@");
        let found = Found::read(&head(&text));
        assert!(!found.curl);
        let scan = found.into_scan(&[]).unwrap();
        let web: Vec<u16> = scan.apps.iter().filter(|a| a.web).map(|a| a.port).collect();
        assert_eq!(web, [80, 3000, 5173]);
        assert!(scan.apps.iter().all(|a| a.title.is_none() && a.scheme.is_none()));
    }

    #[test]
    fn says_so_when_the_server_cant_list_ports() {
        let found = Found::read(&head("@@root@@\n0\n@@curl@@\nyes\n@@listen@@\n"));
        assert_eq!(found.into_scan(&[]).unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn reads_busybox_netstat_too() {
        let text = "@@root@@\n0\n@@curl@@\n@@listen@@\n#netstat\nProto Recv-Q Send-Q Local Address Foreign Address State PID/Program name\ntcp 0 0 0.0.0.0:8080 0.0.0.0:* LISTEN 2101/node\ntcp 0 0 :::8080 :::* LISTEN 2101/node\ntcp 0 0 127.0.0.1:8384 0.0.0.0:* LISTEN -\n";
        let scan = Found::read(&head(text)).into_scan(&[]).unwrap();
        assert_eq!(scan.apps.len(), 2);
        assert_eq!(scan.apps[0].process.as_deref(), Some("node"));
        assert!(scan.apps[0].web && scan.apps[0].public);
        assert_eq!(scan.apps[1].port, 8384);
        assert!(!scan.apps[1].web);
    }

    #[test]
    fn decodes_titles() {
        assert_eq!(clean_title("  Tom &amp; Jerry&#39;s &lt;3 &unknown; & co  ").as_deref(), Some("Tom & Jerry's <3 &unknown; & co"));
        assert_eq!(clean_title("Caf&#xe9;\u{1}").as_deref(), Some("Café"));
        assert_eq!(clean_title("   "), None);
        assert_eq!(clean_title(&"x".repeat(500)).unwrap().len(), 120);
        assert_eq!(decode_entities("a&b;é&amp;"), "a&b;é&");
    }
}
