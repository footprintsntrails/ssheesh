//! Measures, on real Tor, what could speed up file transfers: compressing a tar
//! stream on the server, and moving a transfer off a route that's weak.
//!
//! It needs a server of your own whose key may only run the fixed words below,
//! on a port Tor exits allow (8443, say). The server's address comes from the
//! environment and is never written to the results. Run several at once for more
//! numbers, each with a folder of its own: each then has entry relays of its own.
//!
//!   BENCH_HOST=203.0.113.7 BENCH_KEY=<dir with key, known_hosts> BENCH_DATA=<dir with the sets, here too> \
//!   cargo run -p sshee-core --release --example tor_transfer_bench -- <folder> <results.jsonl> \
//!       compress <routes> ["<set>:<how>,<how>;..."]
//!       pilot <trials>
//!       routes <trials> stay,move [<first trial>]
//!
//! `routes` alternates staying on the route picked with moving by the rule in RULE_SKIP,
//! RULE_WINDOW (seconds), RULE_RATE (bytes a second) and RULE_MOVES. Fix the rule from a
//! `pilot` first: the same runs can't both choose a rule and prove it.
//!
//! Words the server's forced command must know: `tar-<set>-<plain|gzip1|zstd1|zstd3|auto>`,
//! `file-from-<byte>` (a 200 MB file from that byte on), `up-<plain|gzip1|zstd1>`.
//!
//! ssh connects to a door here, which carries it over Tor with the isolation
//! token in force: the same way the app's helper does, minus one small process.

use arti_client::config::TorClientConfigBuilder;
use arti_client::{IsolationToken, StreamPrefs, TorClient, TorClientConfig};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tor_rtcompat::PreferredRuntime;

const CONNECT_WAIT: Duration = Duration::from_secs(30);
/// What the app's route choice does (see `tor::pick`).
const CANDIDATES: usize = 6;
const TIMINGS: u32 = 3;
const TIMING_GAP: Duration = Duration::from_millis(120);
const ENOUGH: usize = 3;
const PICK_TIME: Duration = Duration::from_millis(1800);

const FILE: u64 = 200_000_000;
/// A transfer not done by now was under 0.5 MB/s, which is all a slow one
/// needs to show.
const FILE_CAP: Duration = Duration::from_secs(400);
const AGAIN: u64 = 30_000_000;
const AGAIN_CAP: Duration = Duration::from_secs(90);

type Client = Arc<TorClient<PreferredRuntime>>;

struct Bench {
    rt: tokio::runtime::Runtime,
    client: Client,
    host: String,
    port: u16,
    door: u16,
    token: Arc<Mutex<IsolationToken>>,
    dir: PathBuf,
    key: String,
    known: String,
    out: Mutex<std::fs::File>,
    tag: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, out, what, rest @ ..] = &args[..] else { panic!("usage: tor_transfer_bench <dir> <out> <compress|routes> ...") };
    let host = std::env::var("BENCH_HOST").expect("BENCH_HOST");
    let port: u16 = std::env::var("BENCH_PORT").map_or(8443, |p| p.parse().unwrap());
    let client_dir = std::env::var("BENCH_KEY").expect("BENCH_KEY");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bench = start(&dir, host, port, &client_dir, out);
    match what.as_str() {
        "compress" => compress(&bench, rest[0].parse().unwrap(), rest.get(1).map_or(ALL_SETS, String::as_str)),
        "routes" => routes(&bench, rest[0].parse().unwrap(), &rest[1], rest.get(2).map(String::as_str)),
        "pilot" => pilot(&bench, rest[0].parse().unwrap()),
        other => panic!("unknown experiment {other}"),
    }
}

fn start(dir: &Path, host: String, port: u16, client_dir: &str, out: &str) -> Bench {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let client = rt.block_on(async {
        let mut b = TorClientConfigBuilder::from_directories(dir.join("state"), dir.join("cache"));
        {
            let ready = b.preemptive_circuits();
            let ports = ready.initial_predicted_ports();
            ports.clear();
            ports.push(22);
            ports.push(port);
            ready.min_exit_circs_for_port(4);
        }
        b.circuit_timing().max_dirtiness(Duration::from_secs(30 * 60));
        let config: TorClientConfig = b.build().unwrap();
        TorClient::create_bootstrapped(config).await.unwrap()
    });
    let token = Arc::new(Mutex::new(IsolationToken::new()));
    let listener = rt.block_on(TcpListener::bind(("127.0.0.1", 0))).unwrap();
    let door = listener.local_addr().unwrap().port();
    rt.spawn(run_door(listener, client.clone(), host.clone(), port, token.clone()));
    let out = std::fs::OpenOptions::new().create(true).append(true).open(out).unwrap();
    let tag = dir.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    Bench { rt, client, host, port, door, token, dir: dir.to_path_buf(), key: format!("{client_dir}/key"), known: format!("{client_dir}/known_hosts"), out: Mutex::new(out), tag }
}

async fn run_door(listener: TcpListener, client: Client, host: String, port: u16, token: Arc<Mutex<IsolationToken>>) {
    loop {
        let Ok((mut conn, _)) = listener.accept().await else { continue };
        let _ = conn.set_nodelay(true);
        let (client, host, token) = (client.clone(), host.clone(), *token.lock().unwrap());
        tokio::spawn(async move {
            let mut prefs = StreamPrefs::new();
            prefs.set_isolation(token);
            if let Ok(Ok(mut stream)) = timeout(CONNECT_WAIT, client.connect_with_prefs((host.as_str(), port), &prefs)).await {
                let _ = tokio::io::copy_bidirectional(&mut conn, &mut stream).await;
            }
        });
    }
}

impl Bench {
    fn use_route(&self, token: IsolationToken) {
        *self.token.lock().unwrap() = token;
    }

    /// As the app picks a server's route. None if none could be had.
    fn pick(&self) -> Option<(IsolationToken, u128)> {
        let (client, host, port) = (self.client.clone(), self.host.clone(), self.port);
        self.rt.block_on(async move {
            let mut candidates = tokio::task::JoinSet::new();
            for _ in 0..CANDIDATES {
                let (client, host) = (client.clone(), host.clone());
                candidates.spawn(async move {
                    let token = IsolationToken::new();
                    let mut timed = 0;
                    for round in 0..=TIMINGS {
                        if round > 1 {
                            tokio::time::sleep(TIMING_GAP).await;
                        }
                        let mut prefs = StreamPrefs::new();
                        prefs.set_isolation(token);
                        let started = Instant::now();
                        match timeout(CONNECT_WAIT, client.connect_with_prefs((host.as_str(), port), &prefs)).await {
                            Ok(Ok(_)) if round > 0 => timed += started.elapsed().as_millis(),
                            Ok(Ok(_)) => {}
                            _ => return None,
                        }
                    }
                    Some((token, timed / u128::from(TIMINGS)))
                });
            }
            let started = Instant::now();
            let mut timed: Vec<(IsolationToken, u128)> = Vec::new();
            loop {
                let wait = if timed.len() >= ENOUGH { PICK_TIME.saturating_sub(started.elapsed()) } else { CONNECT_WAIT * 2 };
                match timeout(wait, candidates.join_next()).await {
                    Ok(Some(Ok(Some(t)))) => timed.push(t),
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            }
            timed.into_iter().min_by_key(|t| t.1)
        })
    }

    fn ssh(&self, word: &str) -> Command {
        let mut c = Command::new("ssh");
        c.args(["-F", "/dev/null", "-i", &self.key, "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes"]);
        c.args(["-o", "StrictHostKeyChecking=yes", "-o", &format!("UserKnownHostsFile={}", self.known), "-o", "HostKeyAlias=bench"]);
        c.args(["-o", "ControlMaster=no", "-o", "ControlPath=none", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=6"]);
        c.args(["-o", "ConnectTimeout=60", "-o", "GSSAPIAuthentication=no", "-o", "Compression=no", "-o", "ObscureKeystrokeTiming=no"]);
        c.args(["-T", "-l", "root", "-p", &self.door.to_string(), "--", "127.0.0.1", word]);
        c.stderr(Stdio::piped());
        c
    }

    fn note(&self, line: String) {
        let mut out = self.out.lock().unwrap();
        writeln!(out, "{line}").unwrap();
        out.flush().unwrap();
    }
}

/// One go at one ssh, and how it went.
#[derive(Default, Clone)]
struct Run {
    bytes: u64,
    secs: f64,
    first_byte: Option<f64>,
    /// Bytes so far at each 250 ms.
    series: Vec<u64>,
    finished: bool,
    why: String,
}

impl Run {
    fn json(&self) -> String {
        format!(
            r#"{{"bytes":{},"secs":{:.3},"first_byte":{},"finished":{},"why":"{}","series":{:?}}}"#,
            self.bytes,
            self.secs,
            self.first_byte.map_or("null".into(), |f| format!("{f:.3}")),
            self.finished,
            self.why,
            self.series
        )
    }
}

/// When to give up on a go.
trait Watch {
    fn give_up(&self, secs: f64, first_byte: Option<f64>, series: &[u64]) -> Option<&'static str>;
}

struct CapOnly(Duration);

impl Watch for CapOnly {
    fn give_up(&self, secs: f64, _: Option<f64>, _: &[u64]) -> Option<&'static str> {
        (secs >= self.0.as_secs_f64()).then_some("cap")
    }
}

/// Reads ssh's output into `sink` (what would take it apart at the other end),
/// sampling how far it got, until it's all there or `watch` gives up.
fn download(mut ssh: Command, sink: Option<Child>, watch: &dyn Watch) -> Run {
    let started = Instant::now();
    ssh.stdin(Stdio::null()).stdout(Stdio::piped());
    let mut ssh = ssh.spawn().expect("ssh");
    let mut stdout = ssh.stdout.take().unwrap();
    let mut stderr = ssh.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut e = String::new();
        let _ = stderr.read_to_string(&mut e);
        e
    });
    let moved = Arc::new(AtomicU64::new(0));
    let first = Arc::new(Mutex::new(None));
    let mut sink = sink;
    let mut input = sink.as_mut().map(|s| s.stdin.take().unwrap());
    let copier = {
        let (moved, first) = (moved.clone(), first.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 128 * 1024];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Some(i) = input.as_mut() {
                            if i.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                        let mut f = first.lock().unwrap();
                        if f.is_none() {
                            *f = Some(started.elapsed().as_secs_f64());
                        }
                        drop(f);
                        moved.fetch_add(n as u64, Ordering::Relaxed);
                    }
                }
            }
            drop(input);
        })
    };
    let mut run = Run::default();
    let mut next = Duration::from_millis(250);
    loop {
        if copier.is_finished() {
            break;
        }
        if started.elapsed() >= next {
            run.series.push(moved.load(Ordering::Relaxed));
            next += Duration::from_millis(250);
            if let Some(why) = watch.give_up(started.elapsed().as_secs_f64(), *first.lock().unwrap(), &run.series) {
                run.why = why.into();
                let _ = ssh.kill();
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = copier.join();
    let status = ssh.wait().expect("ssh wait");
    let said = errors.join().unwrap_or_default();
    if let Some(mut s) = sink {
        let unpacked = s.wait().map(|st| st.success()).unwrap_or(false);
        if run.why.is_empty() && !unpacked {
            run.why = "unpack".into();
        }
    }
    run.bytes = moved.load(Ordering::Relaxed);
    run.secs = started.elapsed().as_secs_f64();
    run.first_byte = *first.lock().unwrap();
    run.finished = status.success() && run.why.is_empty();
    if run.why.is_empty() && !status.success() {
        run.why = format!("ssh {:?}: {}", status.code(), said.lines().last().unwrap_or("").replace('"', "'"));
    }
    run
}

fn sink_for(how: &str, into: &Path) -> Child {
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).unwrap();
    if how == "auto" {
        // A letter says how it's packed, as the app reads it.
        let script = r#"k=$(dd bs=1 count=1 2>/dev/null); case $k in z) exec tar --zstd -xf - -C "$0";; g) exec tar -xzf - -C "$0";; *) exec tar -xf - -C "$0";; esac"#;
        return Command::new("sh").args(["-c", script]).arg(into).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("sh");
    }
    let mut tar = Command::new("tar");
    match how {
        "plain" => {
            tar.args(["-xf", "-"]);
        }
        "gzip1" => {
            tar.args(["-xzf", "-"]);
        }
        _ => {
            tar.args(["--zstd", "-xf", "-"]);
        }
    }
    tar.arg("-C").arg(into).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("tar")
}

const ALL_SETS: &str = "text:plain,gzip1,zstd1,zstd3;media:plain,gzip1,zstd1;random:plain,gzip1,zstd1;mixed:plain,gzip1,zstd1";

/// Each route takes every set each way, in turn, twice, so that what a route
/// does over time falls on all of them alike.
fn compress(b: &Bench, routes: usize, spec: &str) {
    let sets: Vec<(&str, Vec<&str>)> = spec
        .split(';')
        .map(|one| {
            let (set, hows) = one.split_once(':').expect("<set>:<how>,<how>;...");
            (set, hows.split(',').collect())
        })
        .collect();
    let into = b.dir.join("unpack");
    let rounds: usize = std::env::var("BENCH_ROUNDS").map_or(2, |r| r.parse().unwrap());
    let uploads = std::env::var("BENCH_NO_UPLOADS").is_err();
    for route in 0..routes {
        b.use_route(IsolationToken::new());
        let begun = Instant::now();
        let mut weak = false;
        for round in 0..rounds {
            for (set, hows) in &sets {
                for i in 0..hows.len() {
                    // A different one goes first each time.
                    let how = hows[(i + round + route) % hows.len()];
                    // With BENCH_SINK=null what arrives is only counted: is a difference on the wire, or in unpacking?
                    let sink = std::env::var("BENCH_SINK").is_err().then(|| sink_for(how, &into));
                    let run = download(b.ssh(&format!("tar-{set}-{how}")), sink, &CapOnly(Duration::from_secs(200)));
                    b.note(format!(r#"{{"exp":"compress","w":"{}","route":{route},"round":{round},"set":"{set}","how":"{how}","run":{}}}"#, b.tag, run.json()));
                    // A route this slow says all it will in a quarter of an hour.
                    if (run.bytes < 1_000_000 && !run.finished) || begun.elapsed() > Duration::from_secs(15 * 60) {
                        weak = true;
                        break;
                    }
                }
                if weak {
                    break;
                }
            }
            if weak {
                break;
            }
        }
        // The other way: the same stream sent to the server.
        if !weak && uploads {
            for round in 0..2 {
                for how in ["plain", "gzip1", "zstd1"] {
                    let run = upload(b, "text", how);
                    b.note(format!(r#"{{"exp":"upload","w":"{}","route":{route},"round":{round},"set":"text","how":"{how}","run":{}}}"#, b.tag, run.json()));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&into);
    }
}

fn upload(b: &Bench, set: &str, how: &str) -> Run {
    let started = Instant::now();
    let data = std::env::var("BENCH_DATA").expect("BENCH_DATA");
    let pack = format!(
        "tar -cf - -C {data} {set}{}",
        match how {
            "plain" => "",
            "gzip1" => " | gzip -1",
            _ => " | zstd -1 -T1 -q -c",
        }
    );
    let mut src = Command::new("sh").args(["-c", &pack]).stdout(Stdio::piped()).spawn().expect("pack");
    let stream = src.stdout.take().unwrap();
    let mut ssh = b.ssh(&format!("up-{how}"));
    ssh.stdin(stream).stdout(Stdio::null());
    let mut ssh = ssh.spawn().expect("ssh");
    let mut run = Run::default();
    loop {
        if let Some(status) = ssh.try_wait().unwrap() {
            run.finished = status.success();
            break;
        }
        if started.elapsed() > Duration::from_secs(200) {
            let _ = ssh.kill();
            let _ = ssh.wait();
            run.why = "cap".into();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = src.kill();
    let _ = src.wait();
    run.secs = started.elapsed().as_secs_f64();
    run
}

/// What the rule looks at, once a route is carrying.
#[derive(Clone, Copy)]
struct Rule {
    skip: f64,
    window: f64,
    min_rate: f64,
    moves: u32,
}

fn rule_from_env() -> Rule {
    let get = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    Rule { skip: get("RULE_SKIP", 2.0), window: get("RULE_WINDOW", 8.0), min_rate: get("RULE_RATE", 500_000.0), moves: get("RULE_MOVES", 2.0) as u32 }
}

struct RuleWatch {
    rule: Rule,
    cap: f64,
    /// Whether the rule still applies to this go.
    armed: bool,
}

impl Watch for RuleWatch {
    fn give_up(&self, secs: f64, first_byte: Option<f64>, series: &[u64]) -> Option<&'static str> {
        if secs >= self.cap {
            return Some("cap");
        }
        if !self.armed {
            return None;
        }
        let Some(first) = first_byte else {
            // Connected or not, nothing's come for as long as a weak route gets.
            return (secs >= 20.0 + self.rule.skip + self.rule.window).then_some("no-first-byte");
        };
        let (from, to) = (first + self.rule.skip, first + self.rule.skip + self.rule.window);
        if secs < to {
            return None;
        }
        let at = |t: f64| series.get(((t / 0.25) as usize).saturating_sub(1)).copied().unwrap_or(0);
        let rate = at(to).saturating_sub(at(from)) as f64 / self.rule.window;
        (rate < self.rule.min_rate).then_some("weak")
    }
}

/// One 200 MB file, on one route or by the rule. Each go goes on from where the
/// last stopped, as a file's transfer does.
fn file_trial(b: &Bench, rule: Option<Rule>, trial: usize, arm: &str) -> Option<IsolationToken> {
    let picked = b.pick();
    let (token, pick_ms) = picked?;
    b.use_route(token);
    let (mut token, mut have, mut moves) = (token, 0u64, 0u32);
    let started = Instant::now();
    let mut goes = Vec::new();
    let mut finished = false;
    loop {
        let left = FILE_CAP.as_secs_f64() - started.elapsed().as_secs_f64();
        if left <= 0.0 {
            break;
        }
        let armed = rule.is_some_and(|r| moves < r.moves);
        let watch: Box<dyn Watch> = match rule {
            Some(r) => Box::new(RuleWatch { rule: r, cap: left, armed }),
            None => Box::new(CapOnly(Duration::from_secs_f64(left))),
        };
        let run = download(b.ssh(&format!("file-from-{have}")), None, watch.as_ref());
        have += run.bytes;
        goes.push(run.clone());
        if have >= FILE && run.finished {
            finished = true;
            break;
        }
        if run.why == "cap" {
            break;
        }
        // Dropped or weak: another route, if the rule allows. A dropped one
        // on one route goes on on that route, as the app does today.
        if armed && (run.why == "weak" || run.why == "no-first-byte") {
            moves += 1;
            token = IsolationToken::new();
            b.use_route(token);
        } else if goes.len() > 8 {
            break;
        } else {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let secs = started.elapsed().as_secs_f64();
    let goes: Vec<String> = goes.iter().map(Run::json).collect();
    b.note(format!(
        r#"{{"exp":"routes","w":"{}","arm":"{arm}","trial":{trial},"pick_ms":{pick_ms},"bytes":{have},"secs":{secs:.3},"finished":{finished},"moves":{moves},"goes":[{}]}}"#,
        b.tag,
        goes.join(",")
    ));
    Some(token)
}

/// 30 MB more on a route that just carried a transfer, or did a while ago: is
/// a route worth remembering?
fn again(b: &Bench, token: IsolationToken, arm: &str, trial: usize, after_secs: f64, kind: &str) {
    b.use_route(token);
    let run = download(b.ssh("file-from-0"), None, &TakeSome(AGAIN, AGAIN_CAP));
    b.note(format!(r#"{{"exp":"again","w":"{}","arm":"{arm}","trial":{trial},"kind":"{kind}","after":{after_secs:.0},"run":{}}}"#, b.tag, run.json()));
}

struct TakeSome(u64, Duration);

impl Watch for TakeSome {
    fn give_up(&self, secs: f64, _: Option<f64>, series: &[u64]) -> Option<&'static str> {
        if series.last().is_some_and(|b| *b >= self.0) {
            return Some("enough");
        }
        (secs >= self.1.as_secs_f64()).then_some("cap")
    }
}

/// Alternates the two ways of carrying the file, so that the time of day falls
/// on them alike.
fn routes(b: &Bench, trials: usize, arms: &str, first: Option<&str>) {
    let rule = rule_from_env();
    let arms: Vec<&str> = arms.split(',').collect();
    let start = first.map_or(0, |f| f.parse().unwrap());
    let mut remembered: Vec<(Instant, IsolationToken, String, usize)> = Vec::new();
    for trial in start..start + trials {
        let arm = arms[trial % arms.len()];
        let token = file_trial(b, (arm == "move").then_some(rule), trial, arm);
        if let Some(token) = token {
            again(b, token, arm, trial, 0.0, "soon");
            remembered.push((Instant::now(), token, arm.to_string(), trial));
        }
        // A route from a while ago, within how long the app keeps one.
        if remembered.first().is_some_and(|r| r.0.elapsed() >= Duration::from_secs(10 * 60)) {
            let (at, token, arm, trial) = remembered.remove(0);
            again(b, token, &arm, trial, at.elapsed().as_secs_f64(), "later");
        }
    }
}

/// The route's speed over the whole file, nothing else: where a rule's numbers
/// come from, before any rule is tried.
fn pilot(b: &Bench, trials: usize) {
    for trial in 0..trials {
        let _ = file_trial(b, None, trial, "pilot");
    }
}
