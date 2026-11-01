//! Terminal sessions that reconnect by themselves.
//!
//! Each pane gets a session running in a pseudo-terminal. For servers the
//! session runs `ssh`; when ssh dies because the network dropped (exit code
//! 255), we wait a moment and start it again. On servers with tmux the remote
//! shell lives inside a named tmux session, so the new connection lands back
//! in the same shell with everything still running.

use crate::ssh::{common_args, durable_command, password_env, session_name, ssh_program, SshEnv, Target};
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
    /// `durable` is true when the shell lives in tmux on the server.
    Connected { durable: bool },
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
    pub fn open(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
        prepare: impl FnOnce() -> Result<SessionSpec, String> + Send + 'static,
        sink: EventSink,
    ) {
        self.close(id);
        let sink = batched(sink);
        let shared = Arc::new(Shared {
            writer: Mutex::new(None),
            master: Mutex::new(None),
            killer: Mutex::new(None),
            size: Mutex::new(PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }),
            closed: AtomicBool::new(false),
            auto_reconnect: AtomicBool::new(self.auto_reconnect.load(Ordering::SeqCst)),
            asleep: AtomicBool::new(false),
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
    let mut attempt = 0u32;
    let mut failures = 0u32;
    let mut ever_ready = false;
    loop {
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        attempt += 1;
        sink(SessionEvent::Status { state: SessionState::Connecting { attempt } });
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
            if shared.auto_reconnect.load(Ordering::SeqCst) {
                let delay = backoff_ms(failures);
                sink(SessionEvent::Status {
                    state: SessionState::Reconnecting { attempt: failures, retry_in_ms: delay, reason: friendly_reason(&line) },
                });
                shared.sleep_unless_woken(delay);
            } else {
                sink(SessionEvent::Status { state: SessionState::Disconnected { reason: friendly_reason(&line) } });
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
    std::thread::spawn(move || {
        // Ends when the session drops its sender, or the terminal stops taking input.
        for data in keys {
            if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    *shared.writer.lock().unwrap() = Some(input);
    *shared.master.lock().unwrap() = Some(pair.master);
    *shared.killer.lock().unwrap() = Some(child.clone_killer());
    if shared.closed.load(Ordering::SeqCst) {
        shared.kill_child();
    }

    let is_ssh = matches!(spec.target, Target::Ssh(_));
    if !is_ssh {
        sink(SessionEvent::Status { state: SessionState::Connected { durable: false } });
    }

    let (tx, rx) = mpsc::channel();
    let reader_sink = sink.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read_loop(reader, reader_sink));
    });

    let status = child.wait().map_err(|e| e.to_string())?;
    // Dropping the pty master lets the reader see EOF (needed on Windows).
    *shared.writer.lock().unwrap() = None;
    *shared.master.lock().unwrap() = None;
    *shared.killer.lock().unwrap() = None;
    let (tail, ready) = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    Ok(Exit { code: status.exit_code(), tail, ready: ready || !is_ssh })
}

fn read_loop(mut reader: Box<dyn Read + Send>, sink: EventSink) -> (String, bool) {
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
                    sink(SessionEvent::Status { state: SessionState::Connected { durable: m == Marker::Durable } });
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
            c.args(common_args(t, &spec.env));
            c.args(["-tt", "--"]);
            c.arg(&t.destination);
            c.arg(durable_command(&spec.key, spec.run.as_deref(), &spec.welcome));
            for (k, v) in password_env(t, &spec.env).unwrap_or_default() {
                c.env(k, v);
            }
            c
        }
    };
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(next(), SessionState::Connected { durable: false });
        m.sleep("p");
        assert_eq!(next(), SessionState::Asleep);
        // Automatic reconnecting is for drops; a session put to sleep waits for you.
        m.set_auto_reconnect(true);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        m.reconnect_now("p");
        assert_eq!(next(), SessionState::Connecting { attempt: 2 });
        assert_eq!(next(), SessionState::Connected { durable: false });
        m.close("p");
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
