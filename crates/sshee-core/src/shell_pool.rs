//! Shells that stay signed in to a server, for quick one-off commands.
//!
//! Starting `ssh` for every command is cheap where connections are shared
//! (macOS, Linux), but Windows' OpenSSH can't share them, so each folder you
//! opened was a whole new login: a second or more. A pooled shell logs in
//! once; after that each command costs about one round trip.
//!
//! Each command runs in its own `sh -c` with no input, so it can't change
//! the shell or read what comes next. Its output and errors go to temporary
//! files on the server, and come back after a header line that gives the
//! exit code and both sizes, so binary output (`find -printf '\0'`) is safe.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::OpError;
use crate::ssh::{remote_command, shell_quote, spawn_error, SshEnv, SshTarget};

/// Idle shells kept per server, and how long one may sit unused.
const KEEP_IDLE: usize = 2;
const IDLE_LIMIT: Duration = Duration::from_secs(120);
/// Lines of shell start-up noise (an `echo` in .bashrc) skipped before a reply.
const MAX_NOISE: usize = 1000;

struct Shell {
    child: Arc<Mutex<Child>>,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// What ssh itself printed: why it couldn't connect, if it couldn't.
    stderr: Arc<Mutex<Vec<u8>>>,
    used: Instant,
}

impl Shell {
    fn alive(&self) -> bool {
        self.child.lock().map(|mut c| matches!(c.try_wait(), Ok(None))).unwrap_or(false)
    }

    /// Ends the shell and says why it went away, the way `run_remote` would.
    fn failure(&mut self) -> OpError {
        let status = {
            let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match child.try_wait() {
                    Ok(Some(s)) => break Some(s),
                    Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                    _ => {
                        let _ = child.kill();
                        break child.wait().ok();
                    }
                }
            }
        };
        // Give the stderr reader a moment to catch ssh's last words.
        thread::sleep(Duration::from_millis(30));
        let stderr = String::from_utf8_lossy(&self.stderr.lock().unwrap_or_else(|e| e.into_inner())).to_string();
        match status.and_then(|s| s.code()) {
            Some(code) if code != 0 || !stderr.trim().is_empty() => OpError::from_ssh(Some(code), &stderr),
            _ => OpError::failed("Lost the connection to the server. Try again."),
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

type Pool = Mutex<HashMap<String, Vec<Shell>>>;

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        // Shells nobody used for a while sign out.
        thread::spawn(|| loop {
            thread::sleep(Duration::from_secs(30));
            let stale: Vec<Shell> = {
                let mut map = pool().lock().unwrap_or_else(|e| e.into_inner());
                let mut out = Vec::new();
                for shells in map.values_mut() {
                    let (keep, drop): (Vec<_>, Vec<_>) = shells.drain(..).partition(|s| s.used.elapsed() < IDLE_LIMIT && s.alive());
                    *shells = keep;
                    out.extend(drop);
                }
                map.retain(|_, v| !v.is_empty());
                out
            };
            drop(stale);
        });
        Mutex::new(HashMap::new())
    })
}

/// Shells are shared only between identical logins.
fn key(t: &SshTarget, env: &SshEnv) -> String {
    format!(
        "{}\0{:?}\0{:?}\0{:?}\0{:?}\0{:?}\0{}\0{:?}",
        t.destination,
        t.user,
        t.port,
        t.identity_file,
        t.extra_args,
        t.identity_agent,
        t.password.is_some(),
        env.control_dir
    )
}

fn take_idle(key: &str) -> Option<Shell> {
    let mut map = pool().lock().unwrap_or_else(|e| e.into_inner());
    let shells = map.get_mut(key)?;
    while let Some(s) = shells.pop() {
        if s.used.elapsed() < IDLE_LIMIT && s.alive() {
            return Some(s);
        }
    }
    None
}

fn give_back(key: String, mut shell: Shell) {
    shell.used = Instant::now();
    let mut map = pool().lock().unwrap_or_else(|e| e.into_inner());
    let shells = map.entry(key).or_default();
    if shells.len() < KEEP_IDLE {
        shells.push(shell);
    }
}

fn spawn(t: &SshTarget, env: &SshEnv) -> Result<Shell, OpError> {
    let mut cmd = remote_command(t, env, "exec sh");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(spawn_error)?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(OpError::failed("Couldn't talk to ssh."));
    };
    let stderr = Arc::new(Mutex::new(Vec::new()));
    if let Some(mut pipe) = child.stderr.take() {
        let buf = stderr.clone();
        thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
                if b.len() < 64 * 1024 {
                    b.extend_from_slice(&chunk[..n]);
                }
            }
        });
    }
    Ok(Shell { child: Arc::new(Mutex::new(child)), stdin, stdout: BufReader::new(stdout), stderr, used: Instant::now() })
}

/// What went wrong talking to a shell.
enum Broken {
    /// The command never reached the server, so running it again is safe.
    Unsent,
    /// It may have run.
    Lost,
}

struct Reply {
    code: i32,
    out: Vec<u8>,
    err: Vec<u8>,
}

/// The text sent to the shell for one command. `mark` starts the reply's header line.
fn request(script: &str, mark: &str) -> String {
    format!(
        "o=$(mktemp) && e=$(mktemp) || exit 1\nsh -c {} </dev/null >\"$o\" 2>\"$e\"; c=$?\nprintf '%s %s %s %s\\n' {mark} \"$c\" \"$(wc -c <\"$o\")\" \"$(wc -c <\"$e\")\"\ncat \"$o\" \"$e\"; rm -f \"$o\" \"$e\"\n",
        shell_quote(script)
    )
}

/// "MARK 0 120 0" → (code, stdout bytes, stderr bytes). `wc` may pad with spaces.
fn parse_header(line: &[u8], mark: &str) -> Option<(i32, usize, usize)> {
    let text = std::str::from_utf8(line).ok()?;
    let mut parts = text.split_whitespace();
    if parts.next()? != mark {
        return None;
    }
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn exchange(shell: &mut Shell, script: &str, mark: &str) -> Result<Reply, Broken> {
    shell.stdin.write_all(request(script, mark).as_bytes()).and_then(|_| shell.stdin.flush()).map_err(|_| Broken::Unsent)?;
    let mut line = Vec::new();
    let mut noise = 0;
    let (code, out_len, err_len) = loop {
        line.clear();
        match shell.stdout.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return Err(Broken::Lost),
            Ok(_) => {}
        }
        if let Some(h) = parse_header(&line, mark) {
            break h;
        }
        noise += 1;
        if noise > MAX_NOISE {
            return Err(Broken::Lost);
        }
    };
    let mut out = vec![0; out_len];
    let mut err = vec![0; err_len];
    shell.stdout.read_exact(&mut out).map_err(|_| Broken::Lost)?;
    shell.stdout.read_exact(&mut err).map_err(|_| Broken::Lost)?;
    Ok(Reply { code, out, err })
}

fn next_mark() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("__sshee_{}_{}_{nanos}__", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Runs `script` with `sh` on the server through a signed-in shell and
/// returns its output. `read_only` commands are retried once on a fresh
/// shell when a reused one turns out to be dead, and stopped after `limit`;
/// others are retried only if they never reached the server, and may take
/// as long as they need.
pub fn run(t: &SshTarget, env: &SshEnv, script: &str, read_only: Option<Duration>) -> Result<Vec<u8>, OpError> {
    let key = key(t, env);
    for attempt in 0..2 {
        let (mut shell, reused) = match take_idle(&key) {
            Some(s) => (s, true),
            None => (spawn(t, env)?, false),
        };
        let timed_out = Arc::new(AtomicBool::new(false));
        let done = read_only.map(|limit| {
            let (tx, rx) = mpsc::channel::<()>();
            let child = shell.child.clone();
            let flag = timed_out.clone();
            thread::spawn(move || {
                if rx.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
                    flag.store(true, Ordering::SeqCst);
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                }
            });
            tx
        });
        let result = exchange(&mut shell, script, &next_mark());
        drop(done);
        match result {
            Ok(r) => {
                give_back(key, shell);
                return if r.code == 0 { Ok(r.out) } else { Err(OpError::from_ssh(Some(r.code), &String::from_utf8_lossy(&r.err))) };
            }
            Err(broken) => {
                if timed_out.load(Ordering::SeqCst) {
                    return Err(OpError::failed("The server took too long to answer."));
                }
                let retry = attempt == 0 && reused && (read_only.is_some() || matches!(broken, Broken::Unsent));
                let err = shell.failure();
                if !retry {
                    return Err(err);
                }
            }
        }
    }
    Err(OpError::failed("Lost the connection to the server. Try again."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header() {
        assert_eq!(parse_header(b"__m__ 0 120 0\n", "__m__"), Some((0, 120, 0)));
        assert_eq!(parse_header(b"__m__ 3       12        7\n", "__m__"), Some((3, 12, 7)));
        assert_eq!(parse_header(b"hello from .bashrc\n", "__m__"), None);
        assert_eq!(parse_header(b"__other__ 0 1 1\n", "__m__"), None);
    }

    #[test]
    fn a_request_keeps_the_command_away_from_the_shell() {
        let r = request("cd /tmp && exit 5", "__m__");
        // Its own sh, no input, output to files.
        assert!(r.contains("sh -c 'cd /tmp && exit 5' </dev/null >\"$o\" 2>\"$e\""));
        assert!(r.contains("printf '%s %s %s %s\\n' __m__"));
        assert!(r.ends_with("rm -f \"$o\" \"$e\"\n"));
    }

    #[test]
    fn marks_are_unique() {
        assert_ne!(next_mark(), next_mark());
    }

    /// The whole round trip against a local `sh`, the same way a server's runs it.
    #[cfg(unix)]
    #[test]
    fn talks_to_a_real_shell() {
        let mut child = std::process::Command::new("sh").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut shell = Shell {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child: Arc::new(Mutex::new(child)),
            stderr: Arc::new(Mutex::new(Vec::new())),
            used: Instant::now(),
        };
        let r = exchange(&mut shell, "printf 'a\\0b\\n'; echo oops >&2; exit 3", &next_mark()).ok().unwrap();
        assert_eq!((r.code, r.out.as_slice(), r.err.as_slice()), (3, &b"a\0b\n"[..], &b"oops\n"[..]));
        // The shell is still there, and the last command's `cd` and `exit` didn't touch it.
        let r = exchange(&mut shell, "cd / && exit 0", &next_mark()).ok().unwrap();
        assert_eq!(r.code, 0);
        let r = exchange(&mut shell, "pwd", &next_mark()).ok().unwrap();
        assert_eq!(String::from_utf8_lossy(&r.out).trim(), std::env::current_dir().unwrap().display().to_string());
        // A command can't eat the next request.
        let r = exchange(&mut shell, "cat; echo done", &next_mark()).ok().unwrap();
        assert_eq!(r.out, b"done\n");
        assert!(shell.alive());
    }
}
