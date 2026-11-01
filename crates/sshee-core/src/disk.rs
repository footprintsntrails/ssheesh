//! What's using a server's disk: how full each disk is, the biggest folders
//! (one level at a time), Docker's images, containers, volumes and build
//! cache, the systemd journal and package caches, plus safe cleanups, each
//! explained. Nothing is deleted here; cleanups are commands you run.

use crate::agents::run_with_timeout;
use crate::error::{ErrorCode, OpError};
use crate::ssh::{run_remote, shell_quote, SshEnv, SshTarget, Target};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub mount: String,
    /// Bytes.
    pub size: u64,
    pub used: u64,
}

/// One entry inside a folder, for the "biggest folders" list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Absolute path.
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub is_dir: bool,
}

/// `docker system df`, in bytes.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerUsage {
    pub images: u64,
    pub images_reclaimable: u64,
    pub containers: u64,
    pub containers_reclaimable: u64,
    pub volumes: u64,
    pub volumes_reclaimable: u64,
    pub build_cache: u64,
    pub build_cache_reclaimable: u64,
    /// Images no container uses, when Docker could list them one by one.
    pub unused_images: Option<u64>,
}

/// One strength of a cleanup, like how old temporary files must be.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupChoice {
    /// "Older than a week".
    pub label: String,
    pub command: String,
    pub bytes: Option<u64>,
}

/// A safe cleanup you can run, with what it frees when that's known.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleanup {
    /// Stable id: "docker-images", "docker-build-cache", "docker-containers",
    /// "journal", "apt-cache", "dnf-cache", "old-logs"…
    pub id: String,
    /// Short name, like "Unused Docker images".
    pub title: String,
    /// One plain sentence on what it removes and why that's safe.
    pub detail: String,
    /// The command to run (may start with sudo).
    pub command: String,
    /// Roughly how much it frees, when known.
    pub bytes: Option<u64>,
    /// Strengths to pick from; `command` and `bytes` above are the one picked first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<CleanupChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskReport {
    pub volumes: Vec<Volume>,
    /// None when Docker isn't installed or isn't reachable.
    pub docker: Option<DockerUsage>,
    /// systemd journal size in bytes; None without journald.
    pub journal: Option<u64>,
    /// Only cleanups that would free something worth it.
    pub cleanups: Vec<Cleanup>,
    /// False when sudo needed a password, so some sizes may be missing or low.
    pub as_root: bool,
}

const MIB: u64 = 1 << 20;
/// Smaller cleanups aren't worth anyone's attention.
const WORTH: u64 = 50 * MIB;
/// The journal cleanup trims it to this, and is only offered above `JOURNAL_BIG`.
const JOURNAL_KEEP: u64 = 200 * MIB;
const JOURNAL_BIG: u64 = 300 * MIB;
const TOP: usize = 30;
/// `docker system df` can be slow on a busy host; past this, the rest still counts.
const REPORT_LIMIT: Duration = Duration::from_secs(30);
/// A few seconds above the server-side `timeout 55` in `BIGGEST_SCRIPT`, so
/// du is stopped there (and prints what it has) rather than left running.
const BIGGEST_LIMIT: Duration = Duration::from_secs(60);

/// Everything in one round trip, each source in its own `@@name@@` section.
/// Never prompts: sudo only with -n. Docker goes last because it's the one
/// that can be slow, and running out of time then loses only Docker.
const REPORT_SCRIPT: &str = r#"export LC_ALL=C
S=""
if [ "$(id -u)" = 0 ]; then W=root
elif sudo -n true >/dev/null 2>&1; then W=sudo; S="sudo -n"
else W=user; fi
echo @@who@@; echo "$W"
echo @@df@@; df -Pk 2>/dev/null
echo @@mounts@@; cat /proc/mounts 2>/dev/null
echo @@journal@@; command -v journalctl >/dev/null 2>&1 && $S journalctl --disk-usage 2>/dev/null
echo @@apt@@; command -v apt-get >/dev/null 2>&1 && [ -d /var/cache/apt/archives ] && $S du -s -k /var/cache/apt/archives 2>/dev/null
echo @@dnf@@; command -v dnf >/dev/null 2>&1 && for d in /var/cache/dnf /var/cache/libdnf5; do [ -d "$d" ] && $S du -s -k "$d" 2>/dev/null; done
echo @@tmp@@; $S find /tmp /var/tmp -xdev -type f ! -path '*/systemd-private-*' ! -name '.X*-lock' -printf '%T@ %A@ %s\n' 2>/dev/null | awk -v now="$(date +%s)" '{t=($1>$2)?$1:$2; a=(now-t)/86400; if (a>1) d1+=$3; if (a>7) d7+=$3; if (a>30) d30+=$3} END {print d1+0, d7+0, d30+0}'
echo @@oldlogs@@; [ -d /var/log ] && $S find /var/log -type f \( -name '*.gz' -o -name '*.[0-9]' -o -name '*.old' \) -exec du -k {} + 2>/dev/null | awk '{s+=$1} END {print s+0}'
echo @@docker@@
if command -v docker >/dev/null 2>&1; then
  if docker system df --format '{{json .}}' 2>/dev/null; then echo @@docker-via@@; echo direct; D="docker"
  elif [ -n "$S" ] && sudo -n docker system df --format '{{json .}}' 2>/dev/null; then echo @@docker-via@@; echo sudo; D="sudo -n docker"
  fi
  [ -n "$D" ] && { echo @@docker-images@@; $D system df -v --format '{{json .Images}}' 2>/dev/null; }
fi
exit 0"#;

/// Follows `P=<folder>`. Runs the walk as root when sudo allows, so
/// root-only folders count. `timeout` stops du on the server too (when the
/// server's `timeout` takes plain seconds), and GNU du prints each entry as
/// it finishes, so a slow disk still gives the entries done by then.
/// `cd` first, so a folder that's a symlink is walked, not just the link.
const BIGGEST_SCRIPT: &str = r#"export LC_ALL=C
case "$P" in ""|"~") P=$HOME ;; "~/"*) P="$HOME/${P#"~/"}" ;; esac
S=""
[ "$(id -u)" = 0 ] || { sudo -n true >/dev/null 2>&1 && S="sudo -n"; }
T=""
timeout 1 true >/dev/null 2>&1 && T="timeout 55"
D="-d 1"
du -k -d 0 /dev/null >/dev/null 2>&1 || D="--max-depth=1"
$S sh -c 'export LC_ALL=C
cd -- "$1" 2>/dev/null || { echo @@missing@@; exit 0; }
echo @@base@@; pwd
echo @@dirs@@
for f in * .[!.]* ..?*; do [ -d "$f" ] && [ ! -L "$f" ] && printf "%s\n" "$f"; done
echo @@du@@
exec $2 du -a -x -k $3 . 2>/dev/null' sh "$P" "$T" "$D"
exit 0"#;

/// The quick overview: disks, Docker, journal and cleanups. Doesn't walk folders.
pub fn report(target: &Target, env: &SshEnv) -> Result<DiskReport, OpError> {
    let t = server(target)?;
    let out = run_with_timeout(t, env, REPORT_SCRIPT, REPORT_LIMIT)?;
    let text = String::from_utf8_lossy(&out);
    let s = sections(&text);
    if !s.contains_key("who") {
        return Err(no_answer(t, env));
    }
    Ok(parse_report(&s))
}

/// The biggest entries directly inside `path`, largest first, staying on
/// that disk. Folders you can't read count as what could be read.
pub fn biggest(target: &Target, env: &SshEnv, path: &str) -> Result<Vec<Usage>, OpError> {
    let t = server(target)?;
    let script = format!("P={}\n{BIGGEST_SCRIPT}", shell_quote(path));
    let out = run_with_timeout(t, env, &script, BIGGEST_LIMIT)?;
    let text = String::from_utf8_lossy(&out);
    let s = sections(&text);
    if s.contains_key("missing") {
        return Err(OpError::new(ErrorCode::NotFound, "This folder doesn't exist or you can't open it."));
    }
    if !s.contains_key("base") {
        return Err(no_answer(t, env));
    }
    Ok(parse_biggest(&s))
}

fn server(target: &Target) -> Result<&SshTarget, OpError> {
    match target {
        Target::Ssh(t) => Ok(t),
        Target::Local => Err(OpError::new(ErrorCode::Unsupported, "This is for servers.")),
    }
}

/// The timed run hides why ssh failed; a plain run tells sign-in and
/// network problems apart so the UI can offer the right fix.
fn no_answer(t: &SshTarget, env: &SshEnv) -> OpError {
    match run_remote(t, env, "true", None) {
        Err(e) => e,
        Ok(_) => OpError::failed("The server took too long to answer. Try again in a moment."),
    }
}

type Sections<'a> = HashMap<&'a str, Vec<&'a str>>;

/// Splits script output at `@@name@@` lines. A section that printed nothing
/// is still present, so "ran but found nothing" differs from "never ran".
fn sections(text: &str) -> Sections<'_> {
    let mut map: Sections = HashMap::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(name) = line.trim_end().strip_prefix("@@").and_then(|l| l.strip_suffix("@@")) {
            map.entry(name).or_default();
            current = Some(name);
        } else if let Some(name) = current {
            map.entry(name).or_default().push(line);
        }
    }
    map
}

fn lines<'a, 'b>(s: &'b Sections<'a>, name: &str) -> &'b [&'a str] {
    s.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn parse_report(s: &Sections) -> DiskReport {
    let who = lines(s, "who").first().map(|w| w.trim()).unwrap_or("user");
    let root = who == "root";
    let docker_via = lines(s, "docker-via").first().map(|v| v.trim());
    let docker = docker_via.map(|_| {
        let mut d = parse_docker(lines(s, "docker"));
        if let Some((count, bytes)) = unused_images(lines(s, "docker-images")) {
            d.unused_images = Some(count);
            d.images_reclaimable = bytes;
        }
        d
    });
    let journal = lines(s, "journal").iter().find_map(|l| journal_bytes(l));
    let found = Found {
        sudo: if root { "" } else { "sudo " },
        docker: if docker_via == Some("sudo") { "sudo docker" } else { "docker" },
        apt: kb_total(lines(s, "apt")),
        dnf: kb_total(lines(s, "dnf")),
        old_logs: kb_total(lines(s, "oldlogs")),
        temp: temp_by_age(lines(s, "tmp")),
    };
    DiskReport {
        volumes: parse_volumes(lines(s, "df"), lines(s, "mounts")),
        cleanups: found.cleanups(docker.as_ref(), journal),
        docker,
        journal,
        as_root: root || who == "sudo",
    }
}

/// What the cleanups need besides Docker and the journal.
struct Found {
    /// "sudo " unless already root.
    sudo: &'static str,
    /// How this user reaches Docker.
    docker: &'static str,
    apt: Option<u64>,
    dnf: Option<u64>,
    old_logs: Option<u64>,
    /// Bytes in /tmp and /var/tmp unused for over a day, a week and a month.
    temp: Option<[u64; 3]>,
}

impl Found {
    /// Never volume pruning: volumes hold data people want to keep.
    fn cleanups(&self, docker: Option<&DockerUsage>, journal: Option<u64>) -> Vec<Cleanup> {
        let (sudo, dk) = (self.sudo, self.docker);
        let mut out = Vec::new();
        let mut add = |id: &str, title: &str, detail: &str, command: String, bytes: u64| {
            if bytes >= WORTH {
                out.push(Cleanup {
                    id: id.into(),
                    title: title.into(),
                    detail: detail.into(),
                    command,
                    bytes: Some(bytes),
                    choices: Vec::new(),
                });
            }
        };
        if let Some(d) = docker {
            let images = match d.unused_images {
                Some(1) => "Removes the one image no container uses; it downloads again when needed.".to_string(),
                Some(n) => format!("Removes the {n} images no container uses; they download again when needed."),
                None => "Removes images no container uses; they download again when needed.".to_string(),
            };
            add("docker-images", "Unused Docker images", &images, format!("{dk} image prune -a"), d.images_reclaimable);
            // -a, because the reclaimable figure counts all unused cache,
            // not only the dangling part a plain prune removes.
            add(
                "docker-build-cache",
                "Docker build cache",
                "Removes files Docker kept to speed up image builds; the next build just takes a little longer.",
                format!("{dk} builder prune -a"),
                d.build_cache_reclaimable,
            );
            add(
                "docker-containers",
                "Stopped containers",
                "Removes containers that aren't running; their images and volumes stay, so they can be created again.",
                format!("{dk} container prune"),
                d.containers_reclaimable,
            );
        }
        if let Some(j) = journal.filter(|&j| j > JOURNAL_BIG) {
            add(
                "journal",
                "Old system logs",
                "Deletes the oldest system log entries until about 200 MB is left; recent ones stay.",
                format!("{sudo}journalctl --vacuum-size=200M"),
                j - JOURNAL_KEEP,
            );
        }
        if let Some(b) = self.apt {
            add(
                "apt-cache",
                "Downloaded packages",
                "Removes installer files apt kept after installing software; what's installed isn't touched.",
                format!("{sudo}apt-get clean"),
                b,
            );
        }
        // `clean all`, because the measured folder is mostly package lists,
        // which `clean packages` leaves; dnf downloads fresh ones when needed.
        if let Some(b) = self.dnf {
            add(
                "dnf-cache",
                "Downloaded packages",
                "Removes packages and package lists dnf downloaded earlier; what's installed isn't touched.",
                format!("{sudo}dnf clean all"),
                b,
            );
        }
        if let Some(b) = self.old_logs {
            add(
                "old-logs",
                "Old rotated logs",
                "Deletes older copies of log files that were already set aside; the current logs stay.",
                format!(r"{sudo}find /var/log -type f \( -name '*.gz' -o -name '*.[0-9]' -o -name '*.old' \) -delete"),
                b,
            );
        }
        // Only files nobody has opened or changed for that long (whichever is
        // later counts), never sockets like tmux's, a systemd service's private
        // folder, or a display's lock file. Unlike the others it's offered
        // however little there is, so the command is always at hand, as long
        // as anything is over a day old. A week is picked first, when a week
        // frees something.
        if let Some(by_age) = self.temp.filter(|t| t[0] > 0) {
            let choices: Vec<CleanupChoice> = [("Older than a day", 1440), ("Older than a week", 10_080), ("Older than a month", 43_200)]
                .iter()
                .zip(by_age)
                .map(|((label, minutes), bytes)| CleanupChoice {
                    label: label.to_string(),
                    command: format!(
                        "{sudo}find /tmp /var/tmp -xdev -type f ! -path '*/systemd-private-*' ! -name '.X*-lock' -mmin +{minutes} -amin +{minutes} -delete"
                    ),
                    bytes: Some(bytes),
                })
                .collect();
            let first = if by_age[1] > 0 { 1 } else { 0 };
            out.push(Cleanup {
                id: "temp".into(),
                title: "Temporary files".into(),
                detail: "Deletes files in /tmp and /var/tmp that nothing has opened or changed for as long as you pick. Programs leave these behind and often forget them.".into(),
                command: choices[first].command.clone(),
                bytes: choices[first].bytes,
                choices,
            });
        }
        out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        out
    }
}

/// "d1 d7 d30": bytes of temporary files unused for over a day, a week, a month.
fn temp_by_age(rows: &[&str]) -> Option<[u64; 3]> {
    let n: Vec<u64> = rows.first()?.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
    match n.as_slice() {
        [a, b, c] => Some([*a, *b, *c]),
        _ => None,
    }
}

/// Sums the first column of `du -k` style lines (or a bare number), in bytes.
/// None when nothing was printed, like a cache folder that isn't there.
fn kb_total(rows: &[&str]) -> Option<u64> {
    let kb: Vec<u64> = rows.iter().filter_map(|l| l.split_whitespace().next()?.parse().ok()).collect();
    (!kb.is_empty()).then(|| kb.iter().sum::<u64>() * 1024)
}

/// `df -Pk`, keeping real disks. File system types come from /proc/mounts
/// when there is one; elsewhere the device name gives tmpfs and friends away.
fn parse_volumes(df: &[&str], mounts: &[&str]) -> Vec<Volume> {
    let types: HashMap<String, &str> = mounts
        .iter()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            // Later mounts hide earlier ones at the same place.
            (f.len() >= 3).then(|| (unescape_mount(f[1]), f[2]))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut volumes = Vec::new();
    let mut root = None;
    for line in df {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let (Ok(size), Ok(used)) = (f[1].parse::<u64>(), f[2].parse::<u64>()) else { continue };
        let v = Volume { mount: f[5..].join(" "), size: size * 1024, used: used * 1024 };
        if v.mount == "/" {
            root = Some(v.clone());
        }
        if size == 0 || !is_real_disk(f[0], types.get(&v.mount).copied(), &v.mount) {
            continue;
        }
        // A bind mount shows the same disk again under another folder.
        if !seen.insert((f[0], size, used)) {
            continue;
        }
        volumes.push(v);
    }
    // Inside a container the root is an overlay; it's still the disk you have.
    if volumes.is_empty() {
        volumes.extend(root);
    }
    volumes.sort_by(|a, b| (b.mount == "/").cmp(&(a.mount == "/")).then(b.size.cmp(&a.size)));
    volumes
}

fn is_real_disk(source: &str, fs: Option<&str>, mount: &str) -> bool {
    const SKIP_FS: [&str; 27] = [
        "tmpfs", "devtmpfs", "overlay", "squashfs", "efivarfs", "proc", "sysfs", "cgroup", "cgroup2", "devpts",
        "mqueue", "debugfs", "tracefs", "securityfs", "pstore", "bpf", "configfs", "fusectl", "hugetlbfs", "autofs",
        "binfmt_misc", "nsfs", "ramfs", "rpc_pipefs", "fuse.lxcfs", "fuse.snapfuse", "nullfs",
    ];
    const SKIP_SOURCE: [&str; 14] = [
        "tmpfs", "devtmpfs", "udev", "overlay", "none", "shm", "run", "proc", "sysfs", "cgroup", "squashfs",
        "efivarfs", "devfs", "map",
    ];
    // Docker's and Podman's own mounts are overlays and shm, caught above; a
    // disk of its own at /var/lib/docker is worth seeing.
    const SKIP_UNDER: [&str; 7] = ["/proc", "/sys", "/dev", "/run", "/snap", "/boot/efi", "/var/lib/kubelet"];
    mount.starts_with('/')
        && !fs.is_some_and(|fs| SKIP_FS.contains(&fs))
        && !SKIP_SOURCE.contains(&source)
        && !SKIP_UNDER.iter().any(|p| mount == *p || mount.strip_prefix(p).is_some_and(|r| r.starts_with('/')))
}

/// /proc/mounts writes a space in a folder name as `\040`.
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let code = b.get(i + 1..i + 4).and_then(|o| u8::from_str_radix(std::str::from_utf8(o).ok()?, 8).ok());
        match code {
            Some(c) if b[i] == b'\\' => {
                out.push(c);
                i += 4;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One row of `docker system df --format '{{json .}}'`.
#[derive(Deserialize)]
struct DfRow {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Size", default)]
    size: String,
    /// "1.1GB (89%)", or just "0B".
    #[serde(rename = "Reclaimable", default)]
    reclaimable: String,
}

fn parse_docker(rows: &[&str]) -> DockerUsage {
    let mut d = DockerUsage::default();
    for row in rows.iter().filter_map(|l| serde_json::from_str::<DfRow>(l.trim()).ok()) {
        let (size, free) = match row.kind.as_str() {
            "Images" => (&mut d.images, &mut d.images_reclaimable),
            "Containers" => (&mut d.containers, &mut d.containers_reclaimable),
            "Local Volumes" => (&mut d.volumes, &mut d.volumes_reclaimable),
            "Build Cache" => (&mut d.build_cache, &mut d.build_cache_reclaimable),
            _ => continue,
        };
        *size = size_bytes(&row.size, 1000).unwrap_or(0);
        *free = row.reclaimable.split('(').next().and_then(|r| size_bytes(r, 1000)).unwrap_or(0);
    }
    d
}

/// One image in `docker system df -v --format '{{json .Images}}'`.
#[derive(Deserialize)]
struct ImageRow {
    /// How many containers use it: a number, "0", or "N/A".
    #[serde(rename = "Containers", default)]
    containers: serde_json::Value,
    /// Its layers no other image has.
    #[serde(rename = "UniqueSize", default)]
    unique: String,
}

/// How many images no container uses, and what removing them surely frees.
///
/// Docker's own "reclaimable" figure counts layers those images share with
/// images in use, which `image prune -a` keeps, so it can promise gigabytes
/// that never come free (and keeps promising them after the prune). The
/// layers only an unused image has do come free, so their sum is a floor:
/// layers shared between unused images alone come on top.
fn unused_images(rows: &[&str]) -> Option<(u64, u64)> {
    let images: Vec<ImageRow> = serde_json::from_str(rows.first()?.trim()).ok()?;
    let unused = images.iter().filter(|i| match &i.containers {
        serde_json::Value::Number(n) => n.as_u64() == Some(0),
        serde_json::Value::String(s) => s.trim() == "0",
        _ => false,
    });
    let (count, bytes) = unused.fold((0, 0), |(c, b), i| (c + 1, b + size_bytes(&i.unique, 1000).unwrap_or(0)));
    Some((count, bytes))
}

/// "Archived and active journals take up 1.2G in the file system." Older
/// systemd says "Journals take up 1.2G on disk."
fn journal_bytes(line: &str) -> Option<u64> {
    let at = line.find("take up ")? + "take up ".len();
    size_bytes(line[at..].split_whitespace().next()?, 1024)
}

/// A size like "1.234GB", "12.3kB", "1.2G" or "0B": the unit's first letter
/// is the power of `base`. Docker counts in 1000s and systemd in 1024s; an
/// explicit "MiB" is always 1024s.
fn size_bytes(s: &str, base: u64) -> Option<u64> {
    let s = s.trim();
    let at = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let n: f64 = s[..at].parse().ok()?;
    let unit = s[at..].trim();
    let power = match unit.chars().next().map(|c| c.to_ascii_uppercase()) {
        None | Some('B') => 0,
        Some('K') => 1,
        Some('M') => 2,
        Some('G') => 3,
        Some('T') => 4,
        Some('P') => 5,
        Some('E') => 6,
        _ => return None,
    };
    let base = if unit.contains("iB") { 1024.0 } else { base as f64 };
    Some((n * base.powi(power)).round() as u64)
}

/// `du -a -k -d 1 .` run inside the folder: "./name" lines, then "." for the
/// folder itself, which is left out. Cut short, it's the entries finished.
fn parse_biggest(s: &Sections) -> Vec<Usage> {
    let base = lines(s, "base").first().map(|b| b.trim_end_matches('/')).unwrap_or("");
    let dirs: HashSet<&str> = lines(s, "dirs").iter().copied().collect();
    let mut out: Vec<Usage> = lines(s, "du")
        .iter()
        .filter_map(|l| {
            let (kb, p) = l.split_once('\t')?;
            let name = p.strip_prefix("./")?;
            // Deeper lines only show up where du can't limit the depth.
            if name.is_empty() || name.contains('/') {
                return None;
            }
            Some(Usage {
                path: format!("{base}/{name}"),
                name: name.to_string(),
                bytes: kb.trim().parse::<u64>().ok()? * 1024,
                is_dir: dirs.contains(name),
            })
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    out.truncate(TOP);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GNU_DF: &str = "Filesystem     1024-blocks      Used Available Capacity Mounted on
tmpfs              1631264      1772   1629492       1% /run
/dev/sda1        101445540  62341212  39087944      62% /
tmpfs              8156304         0   8156304       0% /dev/shm
tmpfs                 5120         0      5120       0% /run/lock
/dev/sda15          106858      6186    100673       6% /boot/efi
/dev/sdb1        515928320 412742656  76955904      85% /mnt/data
/dev/sdc1         10485760   1048576   9437184      10% /mnt/my disk
/dev/loop0           65536     65536         0     100% /snap/core20/2318
/dev/sda1        101445540  62341212  39087944      62% /srv/www
overlay          101445540  62341212  39087944      62% /var/lib/docker/overlay2/0f3a9c/merged
shm                  65536         0     65536       0% /var/lib/docker/containers/abc/mounts/shm
/dev/sdd1         20971520   2097152  18874368      10% /var/lib/docker
tmpfs              1631260         4   1631256       1% /run/user/1000
";

    const MOUNTS: &str = "sysfs /sys sysfs rw,nosuid 0 0
proc /proc proc rw 0 0
/dev/sda1 / ext4 rw,relatime 0 0
/dev/sda15 /boot/efi vfat rw 0 0
/dev/sdb1 /mnt/data xfs rw 0 0
/dev/sdc1 /mnt/my\\040disk ext4 rw 0 0
/dev/loop0 /snap/core20/2318 squashfs ro 0 0
/dev/sda1 /srv/www ext4 rw 0 0
/dev/sdd1 /var/lib/docker ext4 rw 0 0
overlay /var/lib/docker/overlay2/0f3a9c/merged overlay rw 0 0
shm /var/lib/docker/containers/abc/mounts/shm tmpfs rw 0 0
";

    const DOCKER_ROWS: &str = r#"{"Active":"3","Reclaimable":"1.1GB (89%)","Size":"1.234GB","TotalCount":"7","Type":"Images"}
{"Active":"3","Reclaimable":"86.5MB (97%)","Size":"88.9MB","TotalCount":"5","Type":"Containers"}
{"Active":"2","Reclaimable":"512MB (40%)","Size":"1.28GB","TotalCount":"4","Type":"Local Volumes"}
{"Active":"0","Reclaimable":"2.04GB","Size":"2.04GB","TotalCount":"31","Type":"Build Cache"}
"#;

    fn report_of(text: &str) -> DiskReport {
        let s = sections(text);
        assert!(s.contains_key("who"));
        parse_report(&s)
    }

    fn ids(r: &DiskReport) -> Vec<&str> {
        r.cleanups.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn keeps_real_disks_from_gnu_df() {
        let df: Vec<&str> = GNU_DF.lines().collect();
        let mounts: Vec<&str> = MOUNTS.lines().collect();
        let v = parse_volumes(&df, &mounts);
        let names: Vec<&str> = v.iter().map(|v| v.mount.as_str()).collect();
        assert_eq!(names, ["/", "/mnt/data", "/var/lib/docker", "/mnt/my disk"]);
        assert_eq!(v[0].size, 101445540 * 1024);
        assert_eq!(v[0].used, 62341212 * 1024);
    }

    #[test]
    fn keeps_real_disks_from_busybox_df() {
        let df = "Filesystem           1024-blocks    Used Available Capacity Mounted on
/dev/vda3             19475088   2147324  16315476  12% /
devtmpfs                 10240         0     10240   0% /dev
shm                     500456         0    500456   0% /dev/shm
/dev/vda1               523248    295120    228128  56% /boot
tmpfs                   200184       132    200052   0% /run
cgroup_root              10240         0     10240   0% /sys/fs/cgroup
";
        let df: Vec<&str> = df.lines().collect();
        let v = parse_volumes(&df, &[]);
        let names: Vec<&str> = v.iter().map(|v| v.mount.as_str()).collect();
        assert_eq!(names, ["/", "/boot"]);
        assert_eq!(v[1].used, 295120 * 1024);
    }

    #[test]
    fn container_root_still_shows() {
        let df = ["Filesystem 1024-blocks Used Available Capacity Mounted on", "overlay 1000 400 600 40% /"];
        let mounts = ["overlay / overlay rw 0 0"];
        let v = parse_volumes(&df, &mounts);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].mount, "/");
    }

    #[test]
    fn reads_docker_sizes() {
        let rows: Vec<&str> = DOCKER_ROWS.lines().collect();
        let d = parse_docker(&rows);
        assert_eq!(d.images, 1_234_000_000);
        assert_eq!(d.images_reclaimable, 1_100_000_000);
        assert_eq!(d.containers, 88_900_000);
        assert_eq!(d.containers_reclaimable, 86_500_000);
        assert_eq!(d.volumes, 1_280_000_000);
        assert_eq!(d.volumes_reclaimable, 512_000_000);
        assert_eq!(d.build_cache, 2_040_000_000);
        assert_eq!(d.build_cache_reclaimable, 2_040_000_000);
        assert_eq!(parse_docker(&["not json", r#"{"Type":"Images","Size":"0B","Reclaimable":"0B"}"#]).images, 0);
    }

    #[test]
    fn temporary_files_by_age() {
        assert_eq!(temp_by_age(&["734003200 314572800 0"]), Some([734_003_200, 314_572_800, 0]));
        assert_eq!(temp_by_age(&["1 2"]), None);
        assert_eq!(temp_by_age(&[]), None);
        let found = |temp| Found { sudo: "sudo ", docker: "docker", apt: None, dnf: None, old_logs: None, temp };
        let cleanups = found(Some([700 * MIB, 300 * MIB, 0])).cleanups(None, None);
        let t = cleanups.iter().find(|c| c.id == "temp").expect("offered");
        // A week is picked first; each choice has its own age and size.
        assert_eq!(t.bytes, Some(300 * MIB));
        assert_eq!(t.choices.len(), 3);
        assert!(t.command.contains("-mmin +10080 -amin +10080 -delete"));
        assert!(t.choices[0].command.starts_with("sudo find /tmp /var/tmp -xdev -type f ! -path '*/systemd-private-*'"));
        assert_eq!(t.choices[2].bytes, Some(0));
        // However little there is, it's offered, from a day when nothing is a week old.
        let small = found(Some([10 * MIB, 0, 0])).cleanups(None, None);
        let t = small.iter().find(|c| c.id == "temp").expect("offered when small");
        assert_eq!(t.bytes, Some(10 * MIB));
        assert!(t.command.contains("-mmin +1440 -amin +1440 -delete"));
        // Nothing over a day old, or no count at all: nothing to offer.
        assert!(found(Some([0, 0, 0])).cleanups(None, None).is_empty());
        assert!(found(None).cleanups(None, None).is_empty());
    }

    #[test]
    fn counts_only_what_an_image_prune_frees() {
        let rows = r#"[{"Containers":"2","ID":"a","SharedSize":"80MB","Size":"1.2GB","UniqueSize":"1.12GB"},{"Containers":"0","ID":"b","SharedSize":"1.1GB","Size":"1.3GB","UniqueSize":"200MB"},{"Containers":0,"ID":"c","SharedSize":"0B","Size":"50MB","UniqueSize":"50MB"},{"Containers":"N/A","ID":"d","UniqueSize":"9GB"}]"#;
        assert_eq!(unused_images(&[rows]), Some((2, 250_000_000)));
        // After a prune: nothing unused, so nothing to offer.
        assert_eq!(unused_images(&[r#"[{"Containers":"1","UniqueSize":"1GB"}]"#]), Some((0, 0)));
        assert_eq!(unused_images(&[]), None);
        assert_eq!(unused_images(&["not json"]), None);
    }

    #[test]
    fn reads_sizes() {
        assert_eq!(size_bytes("12.3kB", 1000), Some(12_300));
        assert_eq!(size_bytes("0B", 1000), Some(0));
        assert_eq!(size_bytes("1.5TB", 1000), Some(1_500_000_000_000));
        assert_eq!(size_bytes("2MiB", 1000), Some(2 * MIB));
        assert_eq!(size_bytes("1.5G", 1024), Some(3 << 29));
        assert_eq!(size_bytes("512B", 1024), Some(512));
        assert_eq!(size_bytes("GB", 1000), None);
        assert_eq!(size_bytes("1.2X", 1000), None);
    }

    #[test]
    fn reads_journal_size() {
        assert_eq!(journal_bytes("Archived and active journals take up 1.5G in the file system."), Some(3 << 29));
        assert_eq!(journal_bytes("Journals take up 152.0M on disk."), Some(152 * MIB));
        assert_eq!(journal_bytes("No journal files were found."), None);
    }

    #[test]
    fn root_with_docker() {
        let text = format!(
            "@@who@@\nroot\n@@df@@\n{GNU_DF}@@mounts@@\n{MOUNTS}@@journal@@\nArchived and active journals take up 1.5G in the file system.\n\
             @@apt@@\n184280\t/var/cache/apt/archives\n@@dnf@@\n@@oldlogs@@\n245760\n@@docker@@\n{DOCKER_ROWS}@@docker-via@@\ndirect\n"
        );
        let r = report_of(&text);
        assert!(r.as_root);
        assert_eq!(r.volumes.len(), 4);
        assert_eq!(r.journal, Some(3 << 29));
        assert_eq!(r.docker.as_ref().unwrap().images, 1_234_000_000);
        assert_eq!(
            ids(&r),
            ["docker-build-cache", "journal", "docker-images", "old-logs", "apt-cache", "docker-containers"]
        );
        let journal = &r.cleanups[1];
        assert_eq!(journal.command, "journalctl --vacuum-size=200M");
        assert_eq!(journal.bytes, Some((3 << 29) - 200 * MIB));
        assert_eq!(r.cleanups[2].command, "docker image prune -a");
        assert_eq!(r.cleanups[3].bytes, Some(240 * MIB));
        assert!(r.cleanups[3].command.starts_with("find /var/log -type f"));
        assert_eq!(r.cleanups[4].command, "apt-get clean");
        assert!(r.cleanups.iter().all(|c| !c.command.contains("volume") && !c.command.starts_with("sudo")));
    }

    #[test]
    fn user_without_sudo() {
        // Not in the docker group and no sudo: Docker is out of reach, and the
        // journal is only this user's own.
        let text = "@@who@@\nuser\n@@df@@\n/dev/sda1 1000000 500000 500000 50% /\n@@mounts@@\n\
                    @@journal@@\nArchived and active journals take up 8.0M in the file system.\n\
                    @@apt@@\n102400\t/var/cache/apt/archives\n@@dnf@@\n@@oldlogs@@\n0\n@@docker@@\n";
        let r = report_of(text);
        assert!(!r.as_root);
        assert!(r.docker.is_none());
        assert_eq!(r.journal, Some(8 * MIB));
        assert_eq!(ids(&r), ["apt-cache"]);
        assert_eq!(r.cleanups[0].command, "sudo apt-get clean");
        assert_eq!(r.cleanups[0].bytes, Some(100 * MIB));
    }

    #[test]
    fn docker_through_sudo() {
        let text = format!(
            "@@who@@\nsudo\n@@df@@\n@@mounts@@\n@@journal@@\n@@apt@@\n@@dnf@@\n120000\t/var/cache/dnf\n10000\t/var/cache/libdnf5\n\
             @@oldlogs@@\n1024\n@@docker@@\n{DOCKER_ROWS}@@docker-via@@\nsudo\n"
        );
        let r = report_of(&text);
        assert!(r.as_root);
        assert_eq!(r.journal, None);
        let images = r.cleanups.iter().find(|c| c.id == "docker-images").unwrap();
        assert_eq!(images.command, "sudo docker image prune -a");
        let dnf = r.cleanups.iter().find(|c| c.id == "dnf-cache").unwrap();
        assert_eq!(dnf.command, "sudo dnf clean all");
        assert_eq!(dnf.bytes, Some(130000 * 1024));
        // 1 MB of old logs isn't worth offering.
        assert!(!ids(&r).contains(&"old-logs"));
    }

    #[test]
    fn small_things_are_not_offered() {
        let text = "@@who@@\nroot\n@@journal@@\nArchived and active journals take up 280.0M in the file system.\n\
                    @@docker@@\n{\"Type\":\"Images\",\"Size\":\"40MB\",\"Reclaimable\":\"40MB (100%)\"}\n@@docker-via@@\ndirect\n";
        let r = report_of(text);
        assert!(r.cleanups.is_empty());
        assert!(r.docker.is_some());
    }

    #[test]
    fn nothing_printed_means_no_answer() {
        assert!(!sections("").contains_key("who"));
        assert!(!sections("Welcome to Ubuntu\n").contains_key("base"));
    }

    #[test]
    fn biggest_from_gnu_du() {
        let text = "@@base@@\n/\n@@dirs@@\nroot\nusr\nvar\netc\nlost+found\nopt\n.cache\n@@du@@\n\
                    16\t./lost+found\n4\t./opt\n0\t./bin\n2102440\t./var\n44602960\t./root\n5020164\t./usr\n\
                    6736\t./etc\n12\t./.cache\n2097152\t./swap.img\n51732364\t.\n";
        let u = parse_biggest(&sections(text));
        let names: Vec<&str> = u.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["root", "usr", "var", "swap.img", "etc", "lost+found", ".cache", "opt", "bin"]);
        assert_eq!(u[0].path, "/root");
        assert_eq!(u[0].bytes, 44602960 * 1024);
        assert!(u[0].is_dir);
        assert!(!u[3].is_dir, "a file");
        assert!(!u[8].is_dir, "a symlink to a folder isn't walked");
    }

    #[test]
    fn biggest_cut_short_and_capped() {
        // Timed out: no "." line and only some entries. Also more than TOP.
        let mut text = String::from("@@base@@\n/var/\n@@dirs@@\nlog\n@@du@@\n");
        for i in 0..40 {
            text.push_str(&format!("{}\t./f{i}\n", 100 + i));
        }
        text.push_str("900\t./log\n5\t./log/nested\n");
        let u = parse_biggest(&sections(&text));
        assert_eq!(u.len(), TOP);
        assert_eq!(u[0].path, "/var/log");
        assert!(u[0].is_dir);
        assert_eq!(u[1].name, "f39");
    }

    #[test]
    fn biggest_of_missing_folder() {
        let s = sections("@@missing@@\n");
        assert!(s.contains_key("missing"));
        assert!(parse_biggest(&s).is_empty());
    }

    #[test]
    fn only_for_servers() {
        let env = SshEnv::default();
        assert_eq!(report(&Target::Local, &env).unwrap_err().code, ErrorCode::Unsupported);
        assert_eq!(biggest(&Target::Local, &env, "/").unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn unescapes_mount_names() {
        assert_eq!(unescape_mount("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape_mount("/plain\\"), "/plain\\");
    }
}
