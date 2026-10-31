//! Shells that stay signed in to a server, for quick one-off commands.
//!
//! Starting `ssh` for every command is cheap where connections are shared
//! (macOS, Linux), but Windows' OpenSSH can't share them, so each folder you
//! opened was a whole new login: a second or more. A pooled shell logs in
//! once; after that each command costs about one round trip.
//!
//! Each command runs in its own `sh -c` with no input, so it can't change
//! the shell or read what comes next. Its output streams back between two
//! marks that are new for every command, so binary output (`find -printf
//! '\0'`) is safe; then its exit code, and its errors from a file the shell
//! made once when it started. Past the command itself that's no extra
//! programs on the server: the wrapper used to start seven (`mktemp`, `wc`,
//! `cat`, `rm`), about 20 ms of every folder you opened.
//!
//! Polls that come back every few seconds (metrics, what each terminal
//! runs) go through `run_poll`: they take turns, and leave the last idle
//! shell to you, so opening a folder never waits for a new sign-in behind
//! them.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::OpError;
use crate::ssh::{remote_command, shell_quote, spawn_error, SshEnv, SshTarget};

/// Idle shells kept per server, and how long one may sit unused. Three
/// covers a poll, a folder you open and one listed ahead of time.
const KEEP_IDLE: usize = 3;
const IDLE_LIMIT: Duration = Duration::from_secs(120);
/// Shell start-up noise (an `echo` in .bashrc) skipped before a reply.
const MAX_NOISE: usize = 256 * 1024;
/// Sent to a new shell first: the file each command's errors go to.
const SETUP: &str = "e=$(mktemp) || exit 1; trap 'rm -f \"$e\"' EXIT\n";

struct Shell {
    child: Arc<Mutex<Child>>,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// What ssh itself printed: why it couldn't connect, if it couldn't.
    stderr: Arc<Mutex<Vec<u8>>>,
    used: Instant,
}

impl Shell {
    /// Wraps a freshly started `sh` and has it make its error file. One that
    /// can't is gone by the first command, which says so.
    fn start(child: Arc<Mutex<Child>>, mut stdin: ChildStdin, stdout: ChildStdout, stderr: Arc<Mutex<Vec<u8>>>) -> Shell {
        // The pipe holds it until ssh has signed in.
        let _ = stdin.write_all(SETUP.as_bytes());
        Shell { child, stdin, stdout: BufReader::with_capacity(64 * 1024, stdout), stderr, used: Instant::now() }
    }

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

/// An idle shell, if there's one to take. You get the one used last, the
/// likeliest to still be connected. A poll gets the one waiting longest, so
/// all of them keep in use, and never the last one: that stays for you.
fn take_idle(key: &str, poll: bool) -> Option<Shell> {
    let mut map = pool().lock().unwrap_or_else(|e| e.into_inner());
    let shells = map.get_mut(key)?;
    shells.retain(|s| s.used.elapsed() < IDLE_LIMIT && s.alive());
    match (poll, shells.len()) {
        (false, n) if n > 0 => shells.pop(),
        (true, n) if n > 1 => Some(shells.remove(0)),
        _ => None,
    }
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
    Ok(Shell::start(Arc::new(Mutex::new(child)), stdin, stdout, stderr))
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

/// The text sent to the shell for one command. What comes back:
///
/// ```text
/// <noise>\x1f{mark}S\n<stdout>\x1f{mark}E<code>\n<stderr>\x1f{mark}Z\n
/// ```
fn request(script: &str, mark: &str) -> String {
    format!(
        "printf '\\037%sS\\n' {mark}\nsh -c {} </dev/null 2>\"$e\"; c=$?\nprintf '\\037%sE%s\\n' {mark} \"$c\"\n[ -s \"$e\" ] && cat -- \"$e\"\nprintf '\\037%sZ\\n' {mark}\n",
        shell_quote(script)
    )
}

/// The three marks around a reply, each followed by the rest of its line.
/// \x1f hardly ever shows up in text, so looking for them is quick.
fn marks(mark: &str) -> [Vec<u8>; 3] {
    ["S", "E", "Z"].map(|end| format!("\x1f{mark}{end}").into_bytes())
}

/// The rest of a mark's line: nothing, or the exit code.
fn rest_of_line(r: &mut impl BufRead) -> Result<String, Broken> {
    let mut line = Vec::new();
    match r.read_until(b'\n', &mut line) {
        Ok(0) | Err(_) => Err(Broken::Lost),
        Ok(_) => Ok(String::from_utf8_lossy(&line).trim().to_string()),
    }
}

/// Where `needle` first starts in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    let mut from = 0;
    while let Some(i) = hay.get(from..)?.iter().position(|&b| b == first) {
        let at = from + i;
        let tail = hay.get(at + 1..at + needle.len())?;
        if tail == rest {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// Reads up to and past `mark`, and returns what came before it. Reads
/// nothing beyond the mark, which may well arrive split over two reads.
fn read_to_mark(r: &mut impl BufRead, mark: &[u8], limit: usize) -> Result<Vec<u8>, Broken> {
    let mut got = Vec::new();
    loop {
        let chunk = r.fill_buf().map_err(|_| Broken::Lost)?;
        if chunk.is_empty() {
            return Err(Broken::Lost);
        }
        let (had, len) = (got.len(), chunk.len());
        got.extend_from_slice(chunk);
        let from = had.saturating_sub(mark.len() - 1);
        if let Some(at) = find(&got[from..], mark).map(|i| from + i) {
            r.consume(at + mark.len() - had);
            got.truncate(at);
            return Ok(got);
        }
        r.consume(len);
        if got.len() > limit {
            return Err(Broken::Lost);
        }
    }
}

fn exchange(shell: &mut Shell, script: &str, mark: &str) -> Result<Reply, Broken> {
    shell.stdin.write_all(request(script, mark).as_bytes()).and_then(|_| shell.stdin.flush()).map_err(|_| Broken::Unsent)?;
    let [start, end, done] = marks(mark);
    let r = &mut shell.stdout;
    read_to_mark(r, &start, MAX_NOISE)?;
    rest_of_line(r)?;
    let out = read_to_mark(r, &end, usize::MAX)?;
    let code = rest_of_line(r)?.parse().map_err(|_| Broken::Lost)?;
    let err = read_to_mark(r, &done, usize::MAX)?;
    rest_of_line(r)?;
    Ok(Reply { code, out, err })
}

/// A mark no file can hold: new for every command, and random.
fn next_mark() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, Ordering::Relaxed));
    format!("__sshee_{}_{:016x}__", std::process::id(), h.finish())
}

/// Runs `script` with `sh` on the server through a signed-in shell and
/// returns its output. `read_only` commands are retried once on a fresh
/// shell when a reused one turns out to be dead, and stopped after `limit`;
/// others are retried only if they never reached the server, and may take
/// as long as they need.
pub fn run(t: &SshTarget, env: &SshEnv, script: &str, read_only: Option<Duration>) -> Result<Vec<u8>, OpError> {
    run_on(t, env, script, read_only, false)
}

type Lanes = Mutex<HashMap<String, Arc<Mutex<()>>>>;

/// Like `run` for a read-only command, for polls that come back every few
/// seconds. One server's polls take turns, and leave the last idle shell
/// for whatever you do: a poll that finds only that one starts another
/// shell, and the pool keeps both. Without this, metrics and the terminal
/// watch could hold every shell just when you open a folder, and on Windows
/// that folder waited a whole new sign-in.
pub fn run_poll(t: &SshTarget, env: &SshEnv, script: &str, limit: Duration) -> Result<Vec<u8>, OpError> {
    static LANES: OnceLock<Lanes> = OnceLock::new();
    let key = key(t, env);
    let lane = LANES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default().clone();
    let _turn = lane.lock().unwrap_or_else(|e| e.into_inner());
    run_on(t, env, script, Some(limit), true)
}

fn run_on(t: &SshTarget, env: &SshEnv, script: &str, read_only: Option<Duration>, poll: bool) -> Result<Vec<u8>, OpError> {
    let key = key(t, env);
    for attempt in 0..2 {
        let (mut shell, reused) = match take_idle(&key, poll) {
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
    fn finds_marks_split_across_reads() {
        let mark = b"\x1f__m__E";
        assert_eq!(find(b"ab\x1f__m__E0", mark), Some(2));
        assert_eq!(find(b"\x1f__m_\x1f__m__E", mark), Some(5));
        assert_eq!(find(b"\x1f__m__", mark), None);
        // A reader that hands out three bytes at a time.
        let data = b"noise\n\x1f__m__S\nbin\0ary\x1f__m__E0\r\nrest";
        let mut r = BufReader::with_capacity(3, &data[..]);
        assert_eq!(read_to_mark(&mut r, b"\x1f__m__S", 1024).ok().unwrap(), b"noise\n");
        assert_eq!(rest_of_line(&mut r).ok().unwrap(), "");
        assert_eq!(read_to_mark(&mut r, mark, usize::MAX).ok().unwrap(), b"bin\0ary");
        assert_eq!(rest_of_line(&mut r).ok().unwrap(), "0");
        let mut rest = String::new();
        r.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "rest");
        // The stream ending before the mark is a lost shell.
        let mut r = BufReader::new(&b"no mark here"[..]);
        assert!(read_to_mark(&mut r, mark, usize::MAX).is_err());
    }

    #[test]
    fn a_request_keeps_the_command_away_from_the_shell() {
        let r = request("cd /tmp && exit 5", "__m__");
        // Its own sh, no input, errors to the shell's file.
        assert!(r.contains("sh -c 'cd /tmp && exit 5' </dev/null 2>\"$e\"; c=$?"));
        assert!(r.starts_with("printf '\\037%sS\\n' __m__\n"));
        assert!(r.ends_with("printf '\\037%sZ\\n' __m__\n"));
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
        let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        let mut shell = Shell::start(Arc::new(Mutex::new(child)), stdin, stdout, Arc::new(Mutex::new(Vec::new())));
        let r = exchange(&mut shell, "printf 'a\\0b\\n'; echo oops >&2; exit 3", &next_mark()).ok().unwrap();
        assert_eq!((r.code, r.out.as_slice(), r.err.as_slice()), (3, &b"a\0b\n"[..], &b"oops\n"[..]));
        // The errors of one command don't stay for the next.
        let r = exchange(&mut shell, "printf 'no newline'", &next_mark()).ok().unwrap();
        assert_eq!((r.code, r.out.as_slice(), r.err.as_slice()), (0, &b"no newline"[..], &b""[..]));
        // The shell is still there, and the last command's `cd` and `exit` didn't touch it.
        let r = exchange(&mut shell, "cd / && exit 0", &next_mark()).ok().unwrap();
        assert_eq!(r.code, 0);
        let r = exchange(&mut shell, "pwd", &next_mark()).ok().unwrap();
        assert_eq!(String::from_utf8_lossy(&r.out).trim(), std::env::current_dir().unwrap().display().to_string());
        // A command can't eat the next request.
        let r = exchange(&mut shell, "cat; echo done", &next_mark()).ok().unwrap();
        assert_eq!(r.out, b"done\n");
        // Big output streams through whole.
        let r = exchange(&mut shell, "i=0; while [ $i -lt 20000 ]; do echo \"line $i\"; i=$((i+1)); done", &next_mark()).ok().unwrap();
        assert_eq!(r.out.iter().filter(|&&b| b == b'\n').count(), 20000);
        assert!(shell.alive());
    }

    #[test]
    fn polls_leave_the_last_shell_for_you() {
        let key = "test-polls";
        let shell = || {
            let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
            Shell::start(Arc::new(Mutex::new(child)), stdin, stdout, Arc::new(Mutex::new(Vec::new())))
        };
        give_back(key.into(), shell());
        assert!(take_idle(key, true).is_none());
        give_back(key.into(), shell());
        let first = take_idle(key, true).expect("a poll may take one of two");
        assert!(take_idle(key, true).is_none());
        assert!(take_idle(key, false).is_some());
        drop(first);
        assert!(take_idle(key, false).is_none());
    }
}
