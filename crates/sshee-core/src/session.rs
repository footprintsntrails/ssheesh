//! Terminal sessions that reconnect by themselves.
//!
//! Each pane gets a session running in a pseudo-terminal. For servers the
//! session runs `ssh`; when ssh dies because the network dropped (exit code
//! 255), we wait a moment and start it again. On servers with tmux the remote
//! shell lives inside a named tmux session, so the new connection lands back
//! in the same shell with everything still running.

use crate::ssh::{durable_command, password_env, session_name, ssh_program, terminal_args, SshEnv, Target};
use crate::text::{last_meaningful_line, Marker, MarkerScanner, Utf8Stream};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    Output { data: String },
    Status { state: SessionState },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionState {
    Connecting { attempt: u32 },
    /// `durable` is true when the shell lives in tmux on the server, `tor`
    /// when the connection goes through Tor.
    Connected { durable: bool, tor: bool },
    Reconnecting { attempt: u32, retry_in_ms: u64, reason: String },
    /// The connection dropped and automatic reconnecting is off: waiting
    /// for the user to press Reconnect.
    Disconnected { reason: String },
    /// Let go of after sitting unused; the shell keeps running in tmux on
    /// the server. Waiting for the user to come back and reconnect.
    Asleep,
    Ended { code: Option<u32> },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub target: Target,
    /// Stable per pane, so reconnecting (or reopening the app) finds the
    /// same tmux session on the server.
    pub key: String,
    pub env: SshEnv,
    /// Run this instead of a login shell (e.g. a shell inside a container).
    pub run: Option<Vec<String>>,
    /// What a new session shows first (a login shell only).
    pub welcome: crate::welcome::Welcome,
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// What a session's program gets of this process's environment besides what it inherits:
/// variables set back that the app took out of its own environment when it started, and the
/// app's own that a child mustn't have. The 1.0 app removes Rune's trace and log switches from
/// itself before Rune can read them; a shell in its terminal should still have a developer's
/// own `OTEL_*`, and must not have `RUNE_READY_DIR` (a Rune program run there would write its
/// ready note where a launcher waits for the app's). Empty unless set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChildEnv {
    pub set: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    pub remove: Vec<std::ffi::OsString>,
}

static CHILD_ENV: std::sync::OnceLock<ChildEnv> = std::sync::OnceLock::new();

/// Sets [`ChildEnv`] for every session started from now on. Once, at start, before the first
/// session; a second call is refused and gets its value back.
pub fn set_child_env(env: ChildEnv) -> Result<(), ChildEnv> {
    CHILD_ENV.set(env)
}

fn apply_child_env(cmd: &mut CommandBuilder, env: &ChildEnv) {
    for (name, value) in &env.set {
        cmd.env(name, value);
    }
    for name in &env.remove {
        cmd.env_remove(name);
    }
}

/// How a session hands its output to its sink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Delivery {
    /// Held until it stops coming for 2 ms (at most 16 ms or 64 KiB), and
    /// handed on from a thread of its own, one event at a time: for a web
    /// view, where every hand-over is a message.
    #[default]
    Batched,
    /// Each read as it comes, on the reader thread (named `session-read`),
    /// in order: for a sink that parses output into a terminal model of its
    /// own and wakes the window. The sink must not block, or the program's
    /// output stalls behind it.
    ///
    /// Statuses: a server's `Connected` comes from the reader thread, just
    /// before the output that follows ssh's marker; this computer's comes
    /// from the session's thread before its reader starts. The others come
    /// from the session's thread, and one that ends a connection comes after
    /// that connection's last output, as the session waits up to 2 s for its
    /// reader to finish. Past that wait (a program left in the background
    /// still holds the terminal open) the old reader's output can still be
    /// coming while the session goes on, so the sink can be called from two
    /// threads at once and must lock what it shares.
    Direct,
}

pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<Shared>>>,
    /// Reconnect by itself after a drop, or wait for the user.
    auto_reconnect: AtomicBool,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self { sessions: Mutex::new(HashMap::new()), auto_reconnect: AtomicBool::new(true) }
    }
}

struct Shared {
    /// Keys and pastes for the terminal, written by a thread of its own: a
    /// program that doesn't read its input can't freeze the app.
    writer: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    size: Mutex<PtySize>,
    closed: AtomicBool,
    auto_reconnect: AtomicBool,
    /// Put to sleep: its connection was let go of on purpose (see `sleep`).
    asleep: AtomicBool,
    /// A server's terminal, not this computer's.
    ssh: AtomicBool,
    /// Its shell lives in tmux on the server (as last connected).
    durable: AtomicBool,
    /// This try has connected (not still signing in).
    connected: AtomicBool,
    /// Its connection was ended to reconnect another way (see `reroute`).
    rerouting: AtomicBool,
    wake: Mutex<bool>,
    wake_cv: Condvar,
}

impl Shared {
    fn kill_child(&self) {
        if let Some(k) = self.killer.lock().unwrap().as_mut() {
            let _ = k.kill();
        }
    }

    fn wake(&self) {
        *self.wake.lock().unwrap() = true;
        self.wake_cv.notify_all();
    }

    /// Waits until someone presses Reconnect, turns automatic reconnecting
    /// on, or closes the pane.
    fn wait_for_wake(&self) {
        let guard = self.wake.lock().unwrap();
        let mut guard = self.wake_cv.wait_while(guard, |woken| !*woken).unwrap();
        *guard = false;
    }

    /// Sleeps for `ms` unless someone presses "Reconnect now" or closes the pane.
    fn sleep_unless_woken(&self, ms: u64) {
        let guard = self.wake.lock().unwrap();
        let (mut guard, _) = self
            .wake_cv
            .wait_timeout_while(guard, Duration::from_millis(ms), |woken| !*woken)
            .unwrap();
        *guard = false;
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens pane `id`'s session at `cols` x `rows`. `prepare` works out what
    /// to run on the session's own thread, as it may wait on the keychain;
    /// the session is there at once, so keys, resizes and Close sent
    /// meanwhile find it. An `Err` from it ends the session as Failed.
    /// Output reaches `sink` in batches ([`Delivery::Batched`]).
    pub fn open(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
        prepare: impl FnOnce() -> Result<SessionSpec, String> + Send + 'static,
        sink: EventSink,
    ) {
        self.open_with(id, cols, rows, prepare, sink, Delivery::Batched);
    }

    /// [`open`](Self::open), with output handed to `sink` as `delivery` says.
    pub fn open_with(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
        prepare: impl FnOnce() -> Result<SessionSpec, String> + Send + 'static,
        sink: EventSink,
        delivery: Delivery,
    ) {
        self.close(id);
        let sink = match delivery {
            Delivery::Batched => batched(sink),
            Delivery::Direct => sink,
        };
        let shared = Arc::new(Shared {
            writer: Mutex::new(None),
            master: Mutex::new(None),
            killer: Mutex::new(None),
            size: Mutex::new(PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }),
            closed: AtomicBool::new(false),
            auto_reconnect: AtomicBool::new(self.auto_reconnect.load(Ordering::SeqCst)),
            asleep: AtomicBool::new(false),
            ssh: AtomicBool::new(false),
            durable: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            rerouting: AtomicBool::new(false),
            wake: Mutex::new(false),
            wake_cv: Condvar::new(),
        });
        self.sessions.lock().unwrap().insert(id.to_string(), shared.clone());
        std::thread::Builder::new()
            .name(format!("session-{id}"))
            .spawn(move || match prepare() {
                Ok(spec) => run_loop(shared, spec, sink),
                Err(message) => sink(SessionEvent::Status { state: SessionState::Failed { message } }),
            })
            .expect("spawn session thread");
    }

    pub fn write(&self, id: &str, data: &[u8]) {
        if let Some(s) = self.get(id) {
            if let Some(w) = s.writer.lock().unwrap().as_ref() {
                let _ = w.send(data.to_vec());
            }
        }
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) {
        if let Some(s) = self.get(id) {
            let size = PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 };
            *s.size.lock().unwrap() = size;
            if let Some(m) = s.master.lock().unwrap().as_ref() {
                let _ = m.resize(size);
            }
        }
    }

    /// Applies to open sessions too. Turning it on resumes sessions that are
    /// waiting for a manual reconnect.
    pub fn set_auto_reconnect(&self, enabled: bool) {
        self.auto_reconnect.store(enabled, Ordering::SeqCst);
        for s in self.sessions.lock().unwrap().values() {
            s.auto_reconnect.store(enabled, Ordering::SeqCst);
            if enabled {
                s.wake();
            }
        }
    }

    pub fn reconnect_now(&self, id: &str) {
        if let Some(s) = self.get(id) {
            s.asleep.store(false, Ordering::SeqCst);
            s.wake();
        }
    }

    /// Lets go of the connection of a session nobody has used for a while.
    /// Its shell lives on in tmux on the server, and the session waits,
    /// Asleep, until `reconnect_now`. One with no connection up is left as
    /// it is.
    pub fn sleep(&self, id: &str) {
        if let Some(s) = self.get(id) {
            if let Some(k) = s.killer.lock().unwrap().as_mut() {
                s.asleep.store(true, Ordering::SeqCst);
                let _ = k.kill();
            }
        }
    }

    /// Moves the server terminals onto Tor or off it, after it was turned
    /// on or off. Each one in tmux, or still signing in, reconnects at once,
    /// and one in tmux picks up where it was. One without tmux would lose
    /// what its shell is doing, so it stays as it is until it reconnects by
    /// itself. Returns how many stayed.
    pub fn reroute(&self) -> usize {
        let mut stayed = 0;
        for s in self.sessions.lock().unwrap().values() {
            if !s.ssh.load(Ordering::SeqCst) {
                continue;
            }
            // No ssh running right now: its next try already goes the new way.
            let mut killer = s.killer.lock().unwrap();
            let Some(k) = killer.as_mut() else { continue };
            if s.durable.load(Ordering::SeqCst) || !s.connected.load(Ordering::SeqCst) {
                s.rerouting.store(true, Ordering::SeqCst);
                let _ = k.kill();
            } else {
                stayed += 1;
            }
        }
        stayed
    }

    pub fn close(&self, id: &str) {
        let removed = self.sessions.lock().unwrap().remove(id);
        if let Some(s) = removed {
            s.closed.store(true, Ordering::SeqCst);
            s.kill_child();
            s.wake();
        }
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Shared>> {
        self.sessions.lock().unwrap().get(id).cloned()
    }
}

struct Exit {
    code: u32,
    tail: String,
    ready: bool,
}

fn run_loop(shared: Arc<Shared>, spec: SessionSpec, sink: EventSink) {
    let is_ssh = matches!(spec.target, Target::Ssh(_));
    shared.ssh.store(is_ssh, Ordering::SeqCst);
    // Notes whether it's connected and the shell is in tmux, for `reroute`.
    let sink: EventSink = {
        let (inner, shared) = (sink, shared.clone());
        Arc::new(move |e: SessionEvent| {
            if let SessionEvent::Status { state: SessionState::Connected { durable, .. } } = &e {
                shared.durable.store(*durable, Ordering::SeqCst);
                shared.connected.store(true, Ordering::SeqCst);
            }
            inner(e)
        })
    };
    let mut attempt = 0u32;
    let mut failures = 0u32;
    let mut ever_ready = false;
    loop {
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        attempt += 1;
        shared.connected.store(false, Ordering::SeqCst);
        shared.durable.store(false, Ordering::SeqCst);
        sink(SessionEvent::Status { state: SessionState::Connecting { attempt } });
        // A server Tor can't take shouldn't be tried over and over.
        if let Target::Ssh(t) = &spec.target {
            if let Some(message) = crate::tor::refusal(t) {
                sink(SessionEvent::Status { state: SessionState::Failed { message } });
                return;
            }
        }
        let started = Instant::now();
        let exit = match run_once(&shared, &spec, &sink) {
            Ok(exit) => exit,
            Err(message) => {
                sink(SessionEvent::Status { state: SessionState::Failed { message } });
                return;
            }
        };
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        if shared.rerouting.swap(false, Ordering::SeqCst) {
            failures = 0;
            continue;
        }
        if shared.asleep.load(Ordering::SeqCst) {
            sink(SessionEvent::Status { state: SessionState::Asleep });
            // Only Reconnect wakes it: not turning automatic reconnecting on.
            while shared.asleep.load(Ordering::SeqCst) && !shared.closed.load(Ordering::SeqCst) {
                shared.wait_for_wake();
            }
            failures = 0;
            continue;
        }
        ever_ready |= exit.ready;
        if is_ssh && exit.code == 255 {
            let line = last_meaningful_line(&exit.tail);
            // Once connected, the last line may be the server's own output (a
            // log saying "Permission denied", say), not ssh's: never fatal then.
            let fatal = if exit.ready { None } else { fatal_reason(&line, ever_ready) };
            if let Some(message) = fatal {
                sink(SessionEvent::Status { state: SessionState::Failed { message } });
                return;
            }
            // A session that worked for a while starts the backoff from scratch.
            if exit.ready && started.elapsed() > Duration::from_secs(10) {
                failures = 0;
            }
            failures += 1;
            // ssh only sees a connection Tor couldn't make close; Tor knows why.
            let tor_why = match &spec.target {
                Target::Ssh(t) if !exit.ready => crate::tor::recent_failure_for(t),
                _ => None,
            };
            let reason = tor_why.unwrap_or_else(|| friendly_reason(&line));
            if shared.auto_reconnect.load(Ordering::SeqCst) {
                let delay = backoff_ms(failures);
                sink(SessionEvent::Status { state: SessionState::Reconnecting { attempt: failures, retry_in_ms: delay, reason } });
                shared.sleep_unless_woken(delay);
            } else {
                sink(SessionEvent::Status { state: SessionState::Disconnected { reason } });
                shared.wait_for_wake();
            }
            continue;
        }
        sink(SessionEvent::Status { state: SessionState::Ended { code: Some(exit.code) } });
        return;
    }
}

/// Output is held until it stops coming for this long, so a screen drawn in
/// a few reads (tmux after a scroll, a program redrawing) reaches the window
/// whole and never shows half drawn. A key's echo waits just this.
const BATCH_QUIET: Duration = Duration::from_millis(2);
/// Output that keeps coming still goes at least this often (about a frame),
/// so `cat` of a big file is a few hundred messages to the window instead of
/// tens of thousands.
const BATCH_WINDOW: Duration = Duration::from_millis(16);
/// Held-back output past this size goes without waiting.
const BATCH_MAX: usize = 64 * 1024;

/// Output held back to go to the screen in one piece.
#[derive(Default)]
struct Batch {
    pending: String,
    /// When the first and the latest output held back came.
    first: Option<Instant>,
    latest: Option<Instant>,
}

impl Batch {
    /// Output that arrived at `now`: what to send right away, if anything.
    fn add(&mut self, data: String, now: Instant) -> Option<String> {
        self.pending.push_str(&data);
        self.first.get_or_insert(now);
        self.latest = Some(now);
        if self.pending.len() >= BATCH_MAX {
            return self.take();
        }
        None
    }

    /// When the output held back is due, if there is any.
    fn due(&self) -> Option<Instant> {
        Some((self.latest? + BATCH_QUIET).min(self.first? + BATCH_WINDOW))
    }

    /// Everything held back, to send now.
    fn take(&mut self) -> Option<String> {
        self.first = None;
        self.latest = None;
        if self.pending.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.pending))
    }
}

/// A sink that hands output on in batches (see `Batch`), from a thread of
/// its own. A status always comes after the output that was sent before it.
fn batched(sink: EventSink) -> EventSink {
    let (tx, rx) = mpsc::channel::<SessionEvent>();
    let out = sink.clone();
    let spawned = std::thread::Builder::new().name("session-output".into()).spawn(move || {
        let mut batch = Batch::default();
        let send = |data: Option<String>| {
            if let Some(data) = data {
                out(SessionEvent::Output { data });
            }
        };
        loop {
            let next = match batch.due() {
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
            };
            match next {
                Ok(SessionEvent::Output { data }) => send(batch.add(data, Instant::now())),
                Ok(status) => {
                    send(batch.take());
                    out(status);
                }
                Err(RecvTimeoutError::Timeout) => send(batch.take()),
                // The session is over and its reader done.
                Err(RecvTimeoutError::Disconnected) => return send(batch.take()),
            }
        }
    });
    match spawned {
        Ok(_) => Arc::new(move |event| {
            let _ = tx.send(event);
        }),
        Err(_) => sink,
    }
}

pub fn backoff_ms(failures: u32) -> u64 {
    const STEPS: [u64; 5] = [1000, 2000, 4000, 8000, 15000];
    STEPS[(failures.saturating_sub(1) as usize).min(STEPS.len() - 1)]
}

fn run_once(shared: &Arc<Shared>, spec: &SessionSpec, sink: &EventSink) -> Result<Exit, String> {
    // Asked before ssh starts, as its ProxyCommand is chosen then too.
    let tor = matches!(&spec.target, Target::Ssh(t) if crate::tor::carries(t));
    let size = *shared.size.lock().unwrap();
    let pair = native_pty_system().openpty(size).map_err(|e| format!("Couldn't open a terminal: {e}"))?;
    let mut child = pair.slave.spawn_command(build_command(spec)).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("No such file") || msg.contains("not found") {
            "The ssh program isn't installed on this computer.".to_string()
        } else {
            format!("Couldn't start the session: {msg}")
        }
    })?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (input, keys) = mpsc::channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("session-write".into())
        .spawn(move || {
            // Ends when the session drops its sender, or the terminal stops taking input.
            for data in keys {
                if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        })
        .expect("spawn session writer");
    *shared.writer.lock().unwrap() = Some(input);
    *shared.master.lock().unwrap() = Some(pair.master);
    *shared.killer.lock().unwrap() = Some(child.clone_killer());
    if shared.closed.load(Ordering::SeqCst) {
        shared.kill_child();
    }

    let is_ssh = matches!(spec.target, Target::Ssh(_));
    if !is_ssh {
        sink(SessionEvent::Status { state: SessionState::Connected { durable: false, tor: false } });
    }

    let (tx, rx) = mpsc::channel();
    let reader_sink = sink.clone();
    // Named, so the reader's time shows as its own in profilers and meters
    // (and `Delivery::Direct` sinks run on it).
    std::thread::Builder::new()
        .name("session-read".into())
        .spawn(move || {
            let _ = tx.send(read_loop(reader, reader_sink, tor));
        })
        .expect("spawn session reader");

    let status = child.wait().map_err(|e| e.to_string())?;
    // Dropping the pty master lets the reader see EOF (needed on Windows).
    *shared.writer.lock().unwrap() = None;
    // Closed outside the lock: on Windows that can take a while, and the
    // window's own thread takes this lock for a resize.
    let master = shared.master.lock().unwrap().take();
    drop(master);
    *shared.killer.lock().unwrap() = None;
    let (tail, ready) = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    Ok(Exit { code: status.exit_code(), tail, ready: ready || !is_ssh })
}

fn read_loop(mut reader: Box<dyn Read + Send>, sink: EventSink, tor: bool) -> (String, bool) {
    let mut utf8 = Utf8Stream::default();
    let mut markers = MarkerScanner::default();
    let mut tail = String::new();
    let mut ready = false;
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let text = utf8.push(&buf[..n]);
                let (clean, found) = markers.scan(&text);
                for m in found {
                    ready = true;
                    sink(SessionEvent::Status { state: SessionState::Connected { durable: m == Marker::Durable, tor } });
                }
                if !clean.is_empty() {
                    tail.push_str(&clean);
                    if tail.len() > 2048 {
                        let cut = tail.len() - 1024;
                        let cut = (cut..tail.len()).find(|&i| tail.is_char_boundary(i)).unwrap_or(tail.len());
                        tail.drain(..cut);
                    }
                    // tail keeps ssh's notes for working out why a session ended;
                    // the screen doesn't need them.
                    let shown = crate::text::strip_ssh_mux_noise(&clean).into_owned();
                    if !shown.is_empty() {
                        sink(SessionEvent::Output { data: shown });
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (tail, ready)
}

fn build_command(spec: &SessionSpec) -> CommandBuilder {
    let mut cmd = match &spec.target {
        Target::Local if spec.run.is_some() => {
            let argv = spec.run.as_ref().expect("checked");
            let mut c = CommandBuilder::new(&argv[0]);
            c.args(&argv[1..]);
            c
        }
        Target::Local => {
            // PowerShell instead of cmd.exe on Windows; the login shell elsewhere.
            let mut c = if cfg!(windows) {
                let mut c = CommandBuilder::new("powershell.exe");
                c.arg("-NoLogo");
                c
            } else {
                CommandBuilder::new_default_prog()
            };
            if let Some(home) = dirs::home_dir() {
                c.cwd(home);
            }
            c
        }
        Target::Ssh(t) => {
            let mut c = CommandBuilder::new(ssh_program());
            c.args(terminal_args(t, &spec.env));
            c.args(["-tt", "--"]);
            c.arg(&t.destination);
            c.arg(durable_command(&spec.key, spec.run.as_deref(), &spec.welcome));
            for (k, v) in password_env(t, &spec.env).unwrap_or_default() {
                c.env(k, v);
            }
            c
        }
    };
    if let Some(env) = CHILD_ENV.get() {
        apply_child_env(&mut cmd, env);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Apps started from the Dock/Start menu often have no locale, which
    // makes the remote side fall back to ASCII.
    if std::env::var_os("LANG").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    cmd
}

/// Errors where retrying can't help, so we stop and say why.
fn fatal_reason(line: &str, ever_connected: bool) -> Option<String> {
    if line.contains("Permission denied") || line.contains("Too many authentication failures") {
        return Some("The server didn't accept your login. Check the user name and key, then retry.".into());
    }
    if line.contains("IDENTIFICATION HAS CHANGED") {
        return Some("The server's identity changed since you last connected.".into());
    }
    if line.contains("Host key verification failed") {
        return Some("The server's identity couldn't be checked, so SSHeesh didn't connect.".into());
    }
    let unknown_host = line.contains("Name or service not known")
        || line.contains("nodename nor servname")
        || line.contains("No such host is known");
    if unknown_host && !ever_connected {
        return Some("Couldn't find that server. Check the address for typos.".into());
    }
    None
}

fn friendly_reason(line: &str) -> String {
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

/// Name of the tmux session on the server for a pane, for asking it things
/// like the current directory.
pub fn tmux_session_for(key: &str) -> String {
    session_name(key)
}

/// Where a pane's tmux session is scrolled to (see `ssh::SCROLL_TITLE`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TmuxScroll {
    /// Scrolled back (tmux's copy mode) rather than following the output.
    pub in_mode: bool,
    /// Lines scrolled up from the bottom.
    pub position: u32,
    /// Lines of history above the screen.
    pub history: u32,
    pub height: u32,
    /// The most history tmux keeps.
    pub limit: u32,
}

/// Reads "sshee-scroll 1 100 480 24 2000". Out of copy mode tmux leaves the
/// position empty, so there's one number fewer.
pub fn parse_scroll(text: &str) -> Option<TmuxScroll> {
    let mut words = text.split_whitespace();
    if words.next()? != "sshee-scroll" {
        return None;
    }
    let n: Vec<u32> = words.map(|w| w.parse().ok()).collect::<Option<_>>()?;
    let (in_mode, position, rest) = match n.as_slice() {
        [mode, pos, rest @ ..] if rest.len() == 3 => (*mode == 1, *pos, rest),
        [mode, rest @ ..] if rest.len() == 3 => (*mode == 1, 0, rest),
        _ => return None,
    };
    Some(TmuxScroll { in_mode, position, history: rest[0], height: rest[1], limit: rest[2] })
}

/// Scrolls a pane's tmux session so `position` lines of history sit below
/// the screen (0 goes back to following the output), then says where it
/// ended up. Without a position it only reports. tmux doesn't report a
/// scroll made this way through the title, hence the answer.
pub fn tmux_scroll(target: &Target, env: &SshEnv, key: &str, position: Option<u32>) -> Result<TmuxScroll, crate::OpError> {
    let Target::Ssh(t) = target else {
        return Err(crate::OpError::new(crate::ErrorCode::Unsupported, "Not a server session."));
    };
    // Session names only hold letters, digits, "-" and "_", so no quoting.
    let s = session_name(key);
    let go = match position {
        None => String::new(),
        Some(0) => format!("tmux if-shell -F -t {s} '#{{pane_in_mode}}' 'send-keys -X -t {s} cancel' 2>/dev/null; "),
        Some(n) => format!("tmux copy-mode -e -t {s} \\; send-keys -X -t {s} goto-line {n} 2>/dev/null; "),
    };
    let script = format!("{go}tmux display-message -p -t {s} '{}'", crate::ssh::SCROLL_TITLE);
    let out = crate::ssh::run_remote_read(t, env, &script)?;
    parse_scroll(&String::from_utf8_lossy(&out)).ok_or_else(|| crate::OpError::failed("Couldn't tell where the terminal is scrolled to."))
}

/// Ends a pane's tmux session on the server. Used when the user closes the
/// pane on purpose; quitting the app leaves sessions running so they can be
/// picked up again next time.
pub fn end_remote(target: &Target, env: &SshEnv, key: &str) -> Result<(), crate::OpError> {
    if let Target::Ssh(t) = target {
        let script = format!("tmux kill-session -t {} 2>/dev/null; exit 0", crate::ssh::shell_quote(&session_name(key)));
        crate::ssh::run_remote(t, env, &script, None)?;
    }
    Ok(())
}

/// The last `lines` lines of a durable session's scrollback, joined where
/// tmux wrapped them. Used to show what happened while you were away.
pub fn history(target: &Target, env: &SshEnv, key: &str, lines: u32) -> Result<String, crate::OpError> {
    let Target::Ssh(t) = target else { return Ok(String::new()) };
    let script = format!(
        "tmux capture-pane -p -J -S -{} -t {} 2>/dev/null; exit 0",
        lines.clamp(10, 20000),
        crate::ssh::shell_quote(&session_name(key))
    );
    let out = crate::ssh::run_remote_read(t, env, &script)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The rows of a durable session as tmux keeps them, its history first, for
/// searching. Unlike `history` they aren't joined, so a row's place from the
/// bottom is where tmux can scroll to.
pub fn rows(target: &Target, env: &SshEnv, key: &str) -> Result<String, crate::OpError> {
    let Target::Ssh(t) = target else { return Ok(String::new()) };
    let script = format!("tmux capture-pane -p -S -{FIND_ROWS} -t {} 2>/dev/null; exit 0", crate::ssh::shell_quote(&session_name(key)));
    let out = crate::ssh::run_remote_read(t, env, &script)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The most history searched in a terminal.
const FIND_ROWS: u32 = 100_000;

// ---------------------------------------------------------------- terminals left running on a server

/// One of SSHeesh's terminals still running in tmux on a server. A pane
/// opened with its `session_key` as `SessionSpec::key` attaches to it again,
/// with whatever runs in it. That works for a container's shell too: tmux
/// attaches to the session as it is, and `SessionSpec::run` only counts if
/// the session ended in the meantime.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSession {
    pub session_key: String,
    /// When it started, in Unix seconds.
    pub created: i64,
    /// When something last happened in it (output, typing), in Unix seconds.
    /// 0 from a tmux too old to say.
    pub last_active: i64,
    /// Terminals attached to it right now, here or elsewhere: 0 when none.
    pub attached: u32,
    /// The program in front: "zsh", "claude", "vim", "docker" for a
    /// container's shell.
    pub command: String,
    /// The folder it's in.
    pub cwd: String,
}

/// What `list_remote` found on a server.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessions {
    /// The server has tmux. Without it a terminal ends with its connection,
    /// so there's never anything to find.
    pub tmux: bool,
    /// The last used first.
    pub sessions: Vec<RemoteSession>,
}

/// SSHeesh's terminals still running on a server, to open them again when
/// the app lost track of them: 1.0 couldn't read the layout an earlier
/// version left, or tabs were closed while their server was out of reach.
/// Only sessions SSHeesh named are listed, the rest of your tmux sessions
/// aren't. "This computer" has none: its terminals don't run in tmux.
pub fn list_remote(target: &Target, env: &SshEnv) -> Result<RemoteSessions, crate::OpError> {
    let Target::Ssh(t) = target else { return Ok(RemoteSessions::default()) };
    let out = crate::ssh::run_remote_read(t, env, LIST_SCRIPT)?;
    Ok(parse_remote(&String::from_utf8_lossy(&out)))
}

/// What `LIST_SCRIPT` says on a server without tmux.
const NO_TMUX: &str = "sshee-no-tmux";

/// Lists every tmux session, one per line: its name, when it started and
/// was last active, terminals attached, then the program and folder of its
/// pane in front. The folder comes last, so a tab in it can't shift the
/// rest. No tmux running yet is no sessions.
const LIST_SCRIPT: &str = "command -v tmux >/dev/null 2>&1 || { echo sshee-no-tmux; exit 0; }; tmux list-sessions -F '#{session_name}\t#{session_created}\t#{session_activity}\t#{session_attached}\t#{pane_current_command}\t#{pane_current_path}' 2>/dev/null; exit 0";

fn parse_remote(out: &str) -> RemoteSessions {
    if out.lines().any(|l| l.trim() == NO_TMUX) {
        return RemoteSessions { tmux: false, sessions: Vec::new() };
    }
    let mut sessions: Vec<RemoteSession> = out
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.trim_end_matches('\r').splitn(6, '\t').collect();
            let [name, created, active, attached, command, cwd] = f.as_slice() else { return None };
            // Only a name SSHeesh could have given, so that a pane opened
            // with the key lands in this very session.
            let key = name.strip_prefix("sshee-").filter(|k| !k.is_empty() && session_name(k) == *name)?;
            Some(RemoteSession {
                session_key: key.to_string(),
                created: created.parse().unwrap_or(0),
                last_active: active.parse().unwrap_or(0),
                attached: attached.parse().unwrap_or(0),
                command: command.to_string(),
                cwd: cwd.to_string(),
            })
        })
        .collect();
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active).then_with(|| a.session_key.cmp(&b.session_key)));
    RemoteSessions { tmux: true, sessions }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_childs_environment_gets_back_what_the_app_took_and_not_its_own() {
        let mut cmd = CommandBuilder::new("sh");
        cmd.env("SSHEE_TEST_KEPT", "/tmp/p/ready");
        let env = ChildEnv { set: vec![("SSHEE_TEST_OTEL".into(), "http://203.0.113.9:4318".into())], remove: vec!["SSHEE_TEST_KEPT".into()] };
        apply_child_env(&mut cmd, &env);
        assert_eq!(cmd.get_env("SSHEE_TEST_OTEL"), Some(std::ffi::OsStr::new("http://203.0.113.9:4318")));
        assert_eq!(cmd.get_env("SSHEE_TEST_KEPT"), None);
        // Nothing set: the environment as inherited.
        let mut plain = CommandBuilder::new("sh");
        plain.env("SSHEE_TEST_KEPT", "x");
        apply_child_env(&mut plain, &ChildEnv::default());
        assert_eq!(plain.get_env("SSHEE_TEST_KEPT"), Some(std::ffi::OsStr::new("x")));
    }

    #[test]
    fn finds_only_the_terminals_sshee_left() {
        let out = "sshee-k1\t1700000000\t1700000500\t0\tclaude\t/srv/app\n\
                   work\t1700000000\t1700009999\t1\tvim\t/home/me\n\
                   sshee-a_b-2\t1700000100\t1700000900\t1\tzsh\t/home/me/with\ttab\r\n\
                   sshee-\t1\t2\t0\tsh\t/\n\
                   sshee-bad.name\t1\t2\t0\tsh\t/\n\
                   sshee-old\t1700000000\t\t0\tbash\t/root\n\
                   sshee-cut\t1700000000\n";
        let found = parse_remote(out);
        assert!(found.tmux);
        let keys: Vec<&str> = found.sessions.iter().map(|s| s.session_key.as_str()).collect();
        // The last used first; one from a tmux that can't say when, last.
        assert_eq!(keys, ["a_b-2", "k1", "old"]);
        assert_eq!(
            found.sessions[1],
            RemoteSession { session_key: "k1".into(), created: 1_700_000_000, last_active: 1_700_000_500, attached: 0, command: "claude".into(), cwd: "/srv/app".into() }
        );
        assert_eq!(found.sessions[0].cwd, "/home/me/with\ttab");
        assert_eq!(found.sessions[0].attached, 1);
        assert_eq!(parse_remote(""), RemoteSessions { tmux: true, sessions: Vec::new() });
        assert_eq!(parse_remote("sshee-no-tmux\n"), RemoteSessions { tmux: false, sessions: Vec::new() });
        assert_eq!(list_remote(&Target::Local, &SshEnv::default()).unwrap(), RemoteSessions::default());
    }

    /// The listing against a real tmux, with sessions as SSHeesh names them.
    #[cfg(unix)]
    #[test]
    fn lists_the_sessions_a_real_tmux_has() {
        use std::process::Command;
        if !Command::new("tmux").arg("-V").output().is_ok_and(|o| o.status.success()) {
            return;
        }
        // Short: a tmux socket's path has a limit.
        let dir = std::path::PathBuf::from(format!("/tmp/sshee-ls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmux = |args: &[&str]| Command::new("tmux").env("TMUX_TMPDIR", &dir).env_remove("TMUX").args(args).output().unwrap();
        let list = || {
            let out = Command::new("sh").arg("-c").arg(LIST_SCRIPT).env("TMUX_TMPDIR", &dir).env_remove("TMUX").output().unwrap();
            parse_remote(&String::from_utf8_lossy(&out.stdout))
        };
        // No tmux running yet: nothing, but tmux is there.
        assert_eq!(list(), RemoteSessions { tmux: true, sessions: Vec::new() });
        let key = "q7x2m9k1ab";
        assert!(tmux(&["new-session", "-d", "-s", &session_name(key), "-c", "/", "sleep 30"]).status.success());
        assert!(tmux(&["new-session", "-d", "-s", "mine", "sleep 30"]).status.success());
        // Right after it starts, the pane's program can still be the forked tmux
        // or the shell about to exec `sleep`: give it a moment to settle.
        let settled = Instant::now() + Duration::from_secs(3);
        let found = loop {
            let found = list();
            let running = found.sessions.first().is_some_and(|s| s.command == "sleep");
            if running || Instant::now() > settled {
                break found;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        tmux(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found.tmux);
        assert_eq!(found.sessions.len(), 1, "{found:?}");
        let s = &found.sessions[0];
        assert_eq!(s.session_key, key);
        assert_eq!((s.command.as_str(), s.cwd.as_str(), s.attached), ("sleep", "/", 0));
        assert!(s.created > 1_600_000_000 && s.last_active >= s.created, "{s:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_server_without_tmux_says_so() {
        let empty = std::env::temp_dir().join(format!("sshee-no-tmux-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(LIST_SCRIPT).env("PATH", &empty).output().unwrap();
        assert!(out.status.success());
        assert!(!parse_remote(&String::from_utf8_lossy(&out.stdout)).tmux);
        let _ = std::fs::remove_dir(&empty);
    }

    #[test]
    fn reads_the_scroll_report() {
        assert_eq!(parse_scroll("sshee-scroll 1 100 480 24 2000\n"), Some(TmuxScroll { in_mode: true, position: 100, history: 480, height: 24, limit: 2000 }));
        assert_eq!(parse_scroll("sshee-scroll 0  480 24 2000"), Some(TmuxScroll { in_mode: false, position: 0, history: 480, height: 24, limit: 2000 }));
        assert_eq!(parse_scroll("vim ~/notes"), None);
        assert_eq!(parse_scroll("sshee-scroll 1 x 480 24 2000"), None);
    }

    #[test]
    fn output_goes_once_it_stops_coming_and_a_stream_in_batches() {
        let t0 = Instant::now();
        let us = |n: u64| t0 + Duration::from_micros(n);
        let mut b = Batch::default();
        // A screen redrawn in three reads goes as one, once they stop.
        assert_eq!(b.add("top ".into(), t0), None);
        assert_eq!(b.add("middle ".into(), us(300)), None);
        assert_eq!(b.add("bottom".into(), us(600)), None);
        assert_eq!(b.due(), Some(us(600) + BATCH_QUIET));
        assert_eq!(b.take().as_deref(), Some("top middle bottom"));
        assert_eq!((b.take(), b.due()), (None, None));
        // Output that keeps coming goes a window after it started.
        for n in 0..40 {
            assert_eq!(b.add("x".into(), us(10_000 + n * 1000)), None);
        }
        assert_eq!(b.due(), Some(us(10_000) + BATCH_WINDOW));
    }

    #[test]
    fn a_full_batch_goes_without_waiting() {
        let t0 = Instant::now();
        let mut b = Batch::default();
        assert_eq!(b.add("y".repeat(BATCH_MAX - 1), t0), None);
        let full = b.add("z".into(), t0).expect("sent once full");
        assert_eq!(full.len(), BATCH_MAX);
        assert_eq!(b.due(), None);
    }

    #[test]
    fn batched_output_keeps_its_order_and_comes_before_a_status() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink = batched(Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }));
        let mut want = String::new();
        for i in 0..500 {
            let chunk = format!("{i} ");
            want.push_str(&chunk);
            sink(SessionEvent::Output { data: chunk });
        }
        sink(SessionEvent::Status { state: SessionState::Ended { code: Some(0) } });
        sink(SessionEvent::Output { data: "after".into() });
        drop(sink);
        let (mut shown, mut messages) = (String::new(), 0);
        let mut events = rx.iter();
        for e in events.by_ref() {
            match e {
                SessionEvent::Output { data } => {
                    shown.push_str(&data);
                    messages += 1;
                }
                SessionEvent::Status { state } => {
                    assert_eq!(state, SessionState::Ended { code: Some(0) });
                    break;
                }
            }
        }
        assert_eq!(shown, want);
        assert!(messages < 500, "{messages} messages for 500 chunks");
        let rest: Vec<_> = events.collect();
        assert!(matches!(rest.as_slice(), [SessionEvent::Output { data }] if data == "after"));
    }

    #[cfg(unix)]
    #[test]
    fn a_session_put_to_sleep_waits_for_reconnect() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let m = SessionManager::new();
        let spec = || Ok(SessionSpec { target: Target::Local, key: "p".into(), env: SshEnv::default(), run: Some(vec!["sleep".into(), "30".into()]), welcome: Default::default() });
        m.open("p", 80, 24, spec, Arc::new(move |e| {
            if let SessionEvent::Status { state } = e {
                let _ = tx.lock().unwrap().send(state);
            }
        }));
        let next = || rx.recv_timeout(Duration::from_secs(5)).expect("a status");
        assert_eq!(next(), SessionState::Connecting { attempt: 1 });
        assert_eq!(next(), SessionState::Connected { durable: false, tor: false });
        m.sleep("p");
        assert_eq!(next(), SessionState::Asleep);
        // Automatic reconnecting is for drops; a session put to sleep waits for you.
        m.set_auto_reconnect(true);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        m.reconnect_now("p");
        assert_eq!(next(), SessionState::Connecting { attempt: 2 });
        assert_eq!(next(), SessionState::Connected { durable: false, tor: false });
        m.close("p");
    }

    /// Every event a sink got: what, on which thread, when.
    #[cfg(unix)]
    type Seen = Arc<Mutex<Vec<(SessionEvent, Option<String>, Instant)>>>;

    /// A sink that keeps everything it's given, and a way to wait for the end.
    #[cfg(unix)]
    fn recorder() -> (EventSink, Seen, mpsc::Receiver<()>) {
        let seen: Seen = Arc::default();
        let (done_tx, done) = mpsc::channel();
        let done_tx = Mutex::new(done_tx);
        let keep = seen.clone();
        let sink: EventSink = Arc::new(move |e: SessionEvent| {
            let end = matches!(&e, SessionEvent::Status { state: SessionState::Ended { .. } | SessionState::Failed { .. } });
            keep.lock().unwrap().push((e, std::thread::current().name().map(str::to_owned), Instant::now()));
            if end {
                let _ = done_tx.lock().unwrap().send(());
            }
        });
        (sink, seen, done)
    }

    #[cfg(unix)]
    fn local(argv: &[&str]) -> impl FnOnce() -> Result<SessionSpec, String> + Send + 'static {
        let run = argv.iter().map(|a| a.to_string()).collect();
        move || Ok(SessionSpec { target: Target::Local, key: "d".into(), env: SshEnv::default(), run: Some(run), welcome: Default::default() })
    }

    /// The output a program printed, without the line end portable-pty's writer can add after
    /// it: dropping the writer (the session does once the program exits) sends "\n" and EOF
    /// to the terminal, and while the other end is still open the terminal echoes "\r\n".
    #[cfg(unix)]
    fn printed(text: String) -> String {
        text.strip_suffix("\r\n").map(str::to_owned).unwrap_or(text)
    }

    #[cfg(unix)]
    fn outputs(seen: &Seen) -> Vec<(String, Option<String>, Instant)> {
        let seen = seen.lock().unwrap();
        seen.iter()
            .filter_map(|(e, thread, at)| match e {
                SessionEvent::Output { data } => Some((data.clone(), thread.clone(), *at)),
                SessionEvent::Status { .. } => None,
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn direct_output_arrives_per_read_on_the_reader_thread() {
        let (sink, seen, done) = recorder();
        let m = SessionManager::new();
        m.open_with("d", 80, 24, local(&["sh", "-c", "printf one; sleep 0.3; printf two"]), sink, Delivery::Direct);
        done.recv_timeout(Duration::from_secs(10)).expect("the session ends");
        let out = outputs(&seen);
        let text: String = out.iter().map(|(d, _, _)| d.as_str()).collect();
        assert_eq!(printed(text), "onetwo");
        assert!(out.iter().all(|(_, thread, _)| thread.as_deref() == Some("session-read")), "{out:?}");
        // Two reads, handed on as they came: nothing held "one" back for "two".
        let at = |what: &str| out.iter().find(|(d, _, _)| d.contains(what)).map(|(_, _, at)| *at).unwrap();
        assert!(at("two").duration_since(at("one")) >= Duration::from_millis(200), "{out:?}");
        let states: Vec<SessionState> = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(e, _, _)| match e {
                SessionEvent::Status { state } => Some(state.clone()),
                SessionEvent::Output { .. } => None,
            })
            .collect();
        assert_eq!(
            states,
            [SessionState::Connecting { attempt: 1 }, SessionState::Connected { durable: false, tor: false }, SessionState::Ended { code: Some(0) }]
        );
    }

    #[cfg(unix)]
    #[test]
    fn direct_end_comes_after_all_output() {
        const SIZE: usize = 1024 * 1024;
        let (sink, seen, done) = recorder();
        let m = SessionManager::new();
        let script = format!("head -c {SIZE} /dev/zero | tr '\\0' x");
        m.open_with("d", 80, 24, local(&["sh", "-c", &script]), sink, Delivery::Direct);
        done.recv_timeout(Duration::from_secs(20)).expect("the session ends");
        let seen = seen.lock().unwrap();
        let text: String = seen
            .iter()
            .filter_map(|(e, _, _)| match e {
                SessionEvent::Output { data } => Some(data.as_str()),
                SessionEvent::Status { .. } => None,
            })
            .collect();
        let text = printed(text);
        assert_eq!(text.len(), SIZE);
        assert!(text.bytes().all(|b| b == b'x'));
        let (last, _, _) = seen.last().unwrap();
        assert!(matches!(last, SessionEvent::Status { state: SessionState::Ended { code: Some(0) } }), "{last:?}");
    }

    /// `open` keeps 0.10's batching: output comes from the batching thread.
    #[cfg(unix)]
    #[test]
    fn open_still_batches() {
        let (sink, seen, done) = recorder();
        let m = SessionManager::new();
        m.open("d", 80, 24, local(&["sh", "-c", "printf one; printf two"]), sink);
        done.recv_timeout(Duration::from_secs(10)).expect("the session ends");
        let out = outputs(&seen);
        let text: String = out.iter().map(|(d, _, _)| d.as_str()).collect();
        assert_eq!(printed(text), "onetwo");
        assert!(out.iter().all(|(_, thread, _)| thread.as_deref() == Some("session-output")), "{out:?}");
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_ms(1), 1000);
        assert_eq!(backoff_ms(3), 4000);
        assert_eq!(backoff_ms(99), 15000);
    }

    #[test]
    fn auth_errors_are_fatal_network_errors_are_not() {
        assert!(fatal_reason("root@x: Permission denied (publickey).", false).is_some());
        assert!(fatal_reason("Connection to x closed by remote host.", true).is_none());
        assert!(fatal_reason("ssh: Could not resolve hostname x: Name or service not known", true).is_none());
        assert!(fatal_reason("ssh: Could not resolve hostname x: Name or service not known", false).is_some());
    }

    #[test]
    fn events_serialize_for_the_ui() {
        let e = SessionEvent::Status {
            state: SessionState::Reconnecting { attempt: 2, retry_in_ms: 2000, reason: "x".into() },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"type":"status","state":{"kind":"reconnecting","attempt":2,"retryInMs":2000,"reason":"x"}}"#);
        let asleep = serde_json::to_string(&SessionEvent::Status { state: SessionState::Asleep }).unwrap();
        assert_eq!(asleep, r#"{"type":"status","state":{"kind":"asleep"}}"#);
    }
}
