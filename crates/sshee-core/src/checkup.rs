//! A read-only security check-up of a server: how you can log in, what's
//! open to the internet, and whether it keeps itself patched and guarded.
//! Nothing is changed; each finding says why it matters and offers a
//! command you can run yourself.

use crate::agents::run_with_timeout;
use crate::error::{ErrorCode, OpError};
use crate::ssh::{run_remote, SshEnv, Target};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Good,
    Warn,
    Bad,
    /// Couldn't tell, usually for want of root.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// Stable id: "password-login", "root-login", "open-ports", "firewall",
    /// "docker-ports", "auto-updates", "brute-force", "reboot".
    pub id: String,
    /// Short name, like "Password login".
    pub title: String,
    pub verdict: Verdict,
    /// One or two plain sentences: what was found and why it matters.
    pub detail: String,
    /// A command that fixes it, for you to run; never run by SSHeesh.
    pub fix: Option<String>,
    /// One sentence on what `fix` does.
    pub fix_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkup {
    pub checks: Vec<Check>,
    /// Some checks read files only root can; false when sudo needed a
    /// password, so those checks may come back Unknown.
    pub as_root: bool,
}

/// Everything in one round trip. Any tool may be missing, root only ever
/// comes from `sudo -n` so nothing prompts, and slow commands get their own
/// limit where `timeout` exists. Docker goes last: it's the likeliest to hang.
/// `iptables -S` runs only for the legacy backend: `nft` already shows the
/// rest, and some iptables-nft versions create empty tables just by listing.
/// Security updates come from the package lists already on the server: apt
/// only simulates, without its lock or binary caches, and dnf reads its cache.
/// Nothing about the server's packages is sent anywhere.
const SCRIPT: &str = r##"export LC_ALL=C
PATH="$PATH:/usr/local/sbin:/usr/sbin:/sbin"
has() { command -v "$1" >/dev/null 2>&1; }
T=""; timeout 5 true 2>/dev/null && T="timeout 8"
S=""; R=0
if [ "$(id -u)" = 0 ]; then R=1
elif has sudo && $T sudo -n true 2>/dev/null; then R=1; S="sudo -n"; fi
asroot() { [ "$R" = 1 ] && $T $S "$@"; }
echo @@root@@; echo "$R"
echo @@os@@; cat /etc/os-release 2>/dev/null || cat /usr/lib/os-release 2>/dev/null
echo @@tools@@
for c in systemctl rc-service sshd ss netstat ufw firewall-cmd nft iptables docker apt-get dnf dnf5 pgrep; do has "$c" && echo "$c"; done
[ -e /var/log/auth.log ] && echo auth.log
echo @@conn@@; echo "$SSH_CONNECTION"
echo @@sshv@@; ssh -V 2>&1 | head -n 1
echo @@sshd-t@@
asroot sshd -T 2>/dev/null | grep -i -E '^(port|passwordauthentication|kbdinteractiveauthentication|challengeresponseauthentication|usepam|authenticationmethods|permitrootlogin) '
echo @@sshd-files@@
dump() { for f in "$@"; do [ -e "$f" ] || continue; if [ -r "$f" ]; then echo "@file $f"; cat "$f"; echo; else echo "@unreadable $f"; fi; done; }
incs() { for f in "$@"; do [ -r "$f" ] && sed -n -e 's/[[:space:]]#.*//' -e 's/^[[:space:]]*[Ii][Nn][Cc][Ll][Uu][Dd][Ee][[:space:]=][[:space:]]*//p' "$f" | tr -d '"'; done; }
cd /etc/ssh 2>/dev/null; c=/etc/ssh/sshd_config; seen=" $c "; dump "$c"
for d in 1 2 3; do
  n=""
  for p in $(incs $c); do
    case $p in /*) ;; *) p="/etc/ssh/$p" ;; esac
    for f in $p; do case "$seen" in *" $f "*) ;; *) seen="$seen$f "; n="$n $f"; dump "$f" ;; esac; done
  done
  c=$n; [ -n "$c" ] || break
done
echo @@listen@@
if has ss; then echo "#ss"; asroot ss -tulnp || ss -tuln
elif has netstat; then echo "#netstat"; asroot netstat -tulnp || netstat -tuln
fi 2>/dev/null
echo @@ufw@@
if has ufw; then echo "#installed"; asroot ufw status 2>/dev/null | head -n 1; fi
grep -E '^[[:space:]]*ENABLED[[:space:]]*=' /etc/ufw/ufw.conf 2>/dev/null
echo @@firewalld@@
if has firewall-cmd; then echo "#installed"; if has systemctl; then systemctl is-active firewalld 2>/dev/null; else $T firewall-cmd --state 2>/dev/null; fi; fi
echo @@nft@@
has nft && asroot nft list ruleset 2>/dev/null | awk '/^[[:space:]]*chain /{h=$0; k=0; next} /hook input/{k=1; print h} k{print} k && /^[[:space:]]*}/{k=0}'
echo @@iptables@@
if has iptables && ! { has nft && iptables -V 2>/dev/null | grep -q nf_tables; }; then asroot iptables -S INPUT; asroot ip6tables -S INPUT; fi 2>/dev/null
echo @@guards@@
for s in fail2ban:fail2ban-server crowdsec:crowdsec sshguard:sshguard; do
  u=${s%%:*}; p=${s#*:}
  if has systemctl && [ "$(systemctl is-active "$u" 2>/dev/null)" = active ]; then echo "$u"
  elif has pgrep && pgrep -x "$p" >/dev/null 2>&1; then echo "$u"; fi
done
echo @@apt@@
if has apt-config; then
  dpkg-query -W -f='${Status}\n' unattended-upgrades 2>/dev/null | sed 's/^/pkg /'
  apt-config dump 2>/dev/null | grep -E '^APT::Periodic::(Enable|Unattended-Upgrade) '
  has systemctl && echo "timer $(systemctl is-enabled apt-daily-upgrade.timer 2>/dev/null)"
fi
echo @@dnf@@
if has dnf && has systemctl; then
  for u in dnf-automatic.timer dnf-automatic-install.timer dnf5-automatic.timer; do echo "timer $u $(systemctl is-enabled "$u" 2>/dev/null)"; done
  grep -E '^[[:space:]]*apply_updates[[:space:]]*=' /etc/dnf/automatic.conf 2>/dev/null
fi
echo @@secupd@@
L=${T:+timeout 25}
if has apt-get; then
  echo "#apt"; echo "now $(date +%s)"
  stat -c %Y /var/lib/apt/periodic/update-success-stamp /var/lib/apt/lists/*Release /var/lib/apt/lists/*Packages* 2>/dev/null | sort -n | tail -n 1 | sed 's/^/lists /'
  o=$($L apt-get -s -o Debug::NoLocking=1 -o Dir::Cache::pkgcache= -o Dir::Cache::srcpkgcache= dist-upgrade 2>&1); echo "sim $?"
  printf '%s\n' "$o" | grep -E '^(Inst |E: )' | grep -i -E 'security|^E: '
  if has debsecan && grep -q '^ID=debian' /etc/os-release 2>/dev/null; then
    $T debsecan --suite "$(sed -n 's/^VERSION_CODENAME=//p' /etc/os-release | tr -d '"')" --only-fixed 2>/dev/null | grep '^CVE-' | head -n 300 | sed 's/^/vuln /'
  fi
elif has dnf; then
  echo "#dnf"; echo "now $(date +%s)"
  stat -c %Y /var/cache/dnf/*/repodata/repomd.xml /var/cache/libdnf5/*/repodata/repomd.xml 2>/dev/null | sort -n | tail -n 1 | sed 's/^/lists /'
  o=$($L $S dnf -C -q updateinfo list --security 2>&1); echo "list $?"; printf '%s\n' "$o" | head -n 500 | sed 's/^/adv /'
  $L $S dnf -C -q updateinfo list --security --with-cve 2>/dev/null | grep -o 'CVE-[0-9]*-[0-9]*' | head -n 100 | sed 's/^/cve /'
elif has pacman; then
  if has arch-audit; then echo "#arch"; o=$($T arch-audit -u 2>&1); echo "audit $?"; printf '%s\n' "$o" | head -n 300 | sed 's/^/aa /'; else echo "#pacman"; fi
fi
echo @@reboot@@
{ [ -f /var/run/reboot-required ] || [ -f /run/reboot-required ]; } && echo required
if has dnf; then o=$($T dnf -C -q needs-restarting -r 2>&1); echo "restarting $?"; printf '%s\n' "$o" | head -n 2 | sed 's/^/say /'; fi
echo "kernel $(uname -r)"
ls /lib/modules 2>/dev/null | sed 's/^/module /'
echo @@docker@@
if has docker; then
  echo "#installed"
  f='{{.Names}} {{.Ports}}'
  if o=$($T docker ps --format "$f" 2>/dev/null); then echo "#ok"; printf '%s\n' "$o"
  elif [ -n "$S" ] && o=$($T $S docker ps --format "$f" 2>/dev/null); then echo "#ok"; printf '%s\n' "$o"
  else echo "#error"; $T docker ps 2>&1 >/dev/null | head -n 2; fi
fi
echo @@end@@
exit 0"##;

/// Long enough for an apt simulation and a slow `docker ps`; a run cut short
/// still reports what it got.
const LIMIT: Duration = Duration::from_secs(45);

pub fn run(target: &Target, env: &SshEnv) -> Result<Checkup, OpError> {
    let Target::Ssh(t) = target else {
        return Err(OpError::new(ErrorCode::Unsupported, "Check-ups are for servers."));
    };
    let out = run_with_timeout(t, env, SCRIPT, LIMIT)?;
    let text = String::from_utf8_lossy(&out);
    if Sections::new(&text).get("root").is_none() {
        // The script never started; a bare command says why (sign-in, network…).
        run_remote(t, env, "true", None)?;
        return Err(OpError::failed("The server didn't answer the check-up."));
    }
    let mut checkup = parse(&text);
    if t.password.is_some() {
        // SSHeesh itself logs in here with a password: turning password login
        // off (or root's) would lock it out. Keys first, then the fix.
        for c in &mut checkup.checks {
            if (c.id == "password-login" || c.id == "root-login") && c.fix.as_deref().is_some_and(|f| f.contains("PasswordAuthentication no") || f.contains("prohibit-password")) {
                c.fix = None;
                c.fix_note = Some("SSHeesh logs in to this server with a password, so turning that off would lock it out too. Set up a key first: Recipes → Log in with a key.".into());
            }
        }
    }
    Ok(checkup)
}

fn parse(text: &str) -> Checkup {
    let f = Facts::new(text);
    let checks = [
        ("password-login", "Password login", password_login(&f)),
        ("root-login", "Root login", root_login(&f)),
        ("open-ports", "Open ports", open_ports(&f)),
        ("firewall", "Firewall", firewall(&f)),
        ("docker-ports", "Docker ports", docker_ports(&f)),
        ("auto-updates", "Automatic updates", auto_updates(&f)),
        ("security-updates", "Security updates", security_updates(&f)),
        ("brute-force", "Login attack protection", brute_force(&f)),
        ("reboot", "Pending restart", reboot(&f)),
    ];
    let checks = checks
        .into_iter()
        .map(|(id, title, x)| Check { id: id.into(), title: title.into(), verdict: x.verdict, detail: x.detail, fix: x.fix, fix_note: x.fix_note })
        .collect();
    Checkup { checks, as_root: f.as_root }
}

const UNFINISHED: &str = "The check-up ran out of time before getting to this.";
const SSHD_HIDDEN: &str = "Some of the SSH server's settings can only be read as root, and sudo here wants a password.";

struct Finding {
    verdict: Verdict,
    detail: String,
    fix: Option<String>,
    fix_note: Option<String>,
}

impl Finding {
    fn new(verdict: Verdict, detail: impl Into<String>) -> Self {
        Self { verdict, detail: detail.into(), fix: None, fix_note: None }
    }

    fn fix(mut self, command: impl Into<String>, note: impl Into<String>) -> Self {
        self.fix = Some(command.into());
        self.fix_note = Some(note.into());
        self
    }

    /// Advice for when no single command fits.
    fn note(mut self, note: impl Into<String>) -> Self {
        self.fix_note = Some(note.into());
        self
    }
}

fn good(detail: impl Into<String>) -> Finding {
    Finding::new(Verdict::Good, detail)
}

fn warn(detail: impl Into<String>) -> Finding {
    Finding::new(Verdict::Warn, detail)
}

fn bad(detail: impl Into<String>) -> Finding {
    Finding::new(Verdict::Bad, detail)
}

fn unknown(detail: impl Into<String>) -> Finding {
    Finding::new(Verdict::Unknown, detail)
}

/// The script's output, split at its `@@name@@` lines.
struct Sections<'a>(HashMap<&'a str, Vec<&'a str>>);

impl<'a> Sections<'a> {
    fn new(text: &'a str) -> Self {
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut current = None;
        for line in text.lines().map(|l| l.trim_end_matches('\r')) {
            if let Some(name) = line.strip_prefix("@@").and_then(|l| l.strip_suffix("@@")).filter(|n| !n.is_empty()) {
                map.entry(name).or_default();
                current = Some(name);
            } else if let Some(name) = current {
                map.entry(name).or_default().push(line);
            }
        }
        Self(map)
    }

    /// None when the run was cut short before this section.
    fn get(&self, name: &str) -> Option<&[&'a str]> {
        self.0.get(name).map(Vec::as_slice)
    }

    fn lines(&self, name: &str) -> &[&'a str] {
        self.get(name).unwrap_or_default()
    }

    fn has(&self, name: &str, line: &str) -> bool {
        self.lines(name).iter().any(|l| l.trim() == line)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Debian,
    Fedora,
    /// RHEL and its rebuilds: Rocky, Alma, CentOS, Amazon Linux…
    Rhel,
    /// Arch and its spins: Manjaro, EndeavourOS…
    Arch,
    Other,
}

struct Facts<'a> {
    s: Sections<'a>,
    as_root: bool,
    family: Family,
    os_id: String,
    tools: HashSet<&'a str>,
    files: Vec<ConfigFile<'a>>,
    /// None when no OpenSSH server config was found.
    sshd: Option<Sshd>,
    /// None when neither ss nor netstat is there.
    listeners: Option<Vec<Listener>>,
}

impl<'a> Facts<'a> {
    fn new(text: &'a str) -> Self {
        let s = Sections::new(text);
        let (family, os_id) = family(s.lines("os"));
        let tools = s.lines("tools").iter().map(|l| l.trim()).collect();
        let files = config_files(s.lines("sshd-files"));
        let sshd = Sshd::from_test(s.lines("sshd-t")).or_else(|| Sshd::from_files(&files));
        let listeners = listeners(s.lines("listen"));
        Self { as_root: s.has("root", "1"), family, os_id, tools, files, sshd, listeners, s }
    }

    fn password_allowed(&self) -> Option<bool> {
        self.sshd.as_ref()?.password_allowed()
    }

    /// Before 8.7 the keyword was ChallengeResponseAuthentication, and older
    /// servers ignore the new name.
    fn old_openssh(&self) -> bool {
        self.s.lines("sshd-t").iter().any(|l| l.starts_with("challengeresponseauthentication"))
            || self.s.lines("sshv").first().and_then(|l| openssh_version(l)).is_some_and(|v| v < (8, 7))
    }

    /// Ports SSH answers on, so a new firewall rule never shuts you out.
    fn ssh_ports(&self) -> Vec<u16> {
        let mut ports = Vec::new();
        let conn: Vec<&str> = self.s.lines("conn").first().map(|l| l.split_whitespace().collect()).unwrap_or_default();
        if let &[_, _, server, port] = conn.as_slice() {
            if !off_internet(server) {
                ports.extend(port.parse::<u16>());
            }
        }
        if let Some(sshd) = &self.sshd {
            ports.extend(&sshd.ports);
        }
        let from_sshd = |l: &&Listener| !l.udp && !off_internet(&l.host) && l.process.as_deref() == Some("sshd");
        ports.extend(self.listeners.iter().flatten().filter(from_sshd).map(|l| l.port));
        if ports.is_empty() {
            ports.push(22);
        }
        ports.sort_unstable();
        ports.dedup();
        ports
    }

    fn web_ports(&self) -> Vec<u16> {
        let mut ports: Vec<u16> =
            self.listeners.iter().flatten().filter(|l| !l.udp && !off_internet(&l.host) && matches!(l.port, 80 | 443)).map(|l| l.port).collect();
        ports.sort_unstable();
        ports.dedup();
        ports
    }

    fn reload_sshd(&self) -> &'static str {
        if !self.tools.contains("systemctl") && self.tools.contains("rc-service") {
            "sudo rc-service sshd reload"
        } else {
            "(sudo systemctl reload ssh || sudo systemctl reload sshd)"
        }
    }

    /// Sets sshd keywords: a drop-in file when the main config reads one
    /// first, otherwise lines put at the very top of sshd_config, where
    /// they win too. `sshd -t` stops a broken config from being loaded.
    fn sshd_fix(&self, drop_in: &str, lines: &[&str]) -> String {
        let path = format!("/etc/ssh/sshd_config.d/{drop_in}");
        let keys: Vec<String> = lines.iter().filter_map(|l| directive(l)).map(|(k, _)| k).collect();
        let write = if drop_in_works(&self.files, &path, &keys) {
            let body: String = lines.iter().map(|l| format!("{l}\\n")).collect();
            format!("printf '{body}' | sudo tee {path} >/dev/null")
        } else {
            let edits: String = lines.iter().map(|l| format!(" -e '1i {l}'")).collect();
            format!("sudo sed -i{edits} {MAIN_CONFIG}")
        };
        format!("{write} && sudo sshd -t && {}", self.reload_sshd())
    }
}

fn family(os_release: &[&str]) -> (Family, String) {
    let field = |key: &str| {
        os_release
            .iter()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches(['"', '\'']).to_ascii_lowercase())
            .unwrap_or_default()
    };
    let id = field("ID");
    let like = field("ID_LIKE");
    let is = |names: &[&str]| names.contains(&id.as_str()) || like.split_whitespace().any(|w| names.contains(&w));
    let family = if is(&["debian", "ubuntu"]) {
        Family::Debian
    } else if id == "fedora" {
        Family::Fedora
    } else if is(&["rhel", "centos", "fedora", "rocky", "almalinux", "ol", "amzn"]) {
        Family::Rhel
    } else if is(&["arch"]) {
        Family::Arch
    } else {
        Family::Other
    };
    (family, id)
}

fn openssh_version(line: &str) -> Option<(u32, u32)> {
    let mut parts = line.split("OpenSSH_").nth(1)?.split(|c: char| !c.is_ascii_digit());
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

// ---- sshd settings

const MAIN_CONFIG: &str = "/etc/ssh/sshd_config";

/// A config file as the script dumped it; `lines` is None when only root can read it.
struct ConfigFile<'a> {
    path: &'a str,
    lines: Option<Vec<&'a str>>,
}

fn config_files<'a>(dump: &[&'a str]) -> Vec<ConfigFile<'a>> {
    let mut files: Vec<ConfigFile> = Vec::new();
    for line in dump {
        if let Some(path) = line.strip_prefix("@file ") {
            files.push(ConfigFile { path, lines: Some(Vec::new()) });
        } else if let Some(path) = line.strip_prefix("@unreadable ") {
            files.push(ConfigFile { path, lines: None });
        } else if let Some(ConfigFile { lines: Some(lines), .. }) = files.last_mut() {
            lines.push(line);
        }
    }
    files
}

/// sshd's global settings, as far as they could be read.
#[derive(Debug, Default)]
struct Sshd {
    /// First value per lowercase keyword; None when a file only root can
    /// read came earlier and may have set it first.
    values: HashMap<String, Option<String>>,
    /// Every Port line counts, not just the first.
    ports: Vec<u16>,
    /// A file only root can read was pulled in, so later values may be shadowed.
    hidden: bool,
}

enum Setting<'a> {
    Set(&'a str),
    Default,
    Hidden,
}

impl Sshd {
    /// From `sshd -T`, which has already done all the resolving.
    fn from_test(lines: &[&str]) -> Option<Self> {
        let mut s = Self::default();
        for (key, value) in lines.iter().filter_map(|l| l.trim().split_once(' ')) {
            let key = key.to_ascii_lowercase();
            if key == "port" {
                s.ports.extend(value.trim().parse::<u16>());
            } else {
                s.values.entry(key).or_insert_with(|| Some(value.trim().to_string()));
            }
        }
        (!s.values.is_empty()).then_some(s)
    }

    fn from_files(files: &[ConfigFile]) -> Option<Self> {
        files.iter().any(|f| f.path == MAIN_CONFIG).then(|| {
            let mut s = Self::default();
            s.walk(files, MAIN_CONFIG, false, 0);
            s
        })
    }

    /// Reads a file the way sshd does: the first value wins, Include pulls
    /// files in where it stands, and what follows a Match isn't global.
    fn walk(&mut self, files: &[ConfigFile], path: &str, mut in_match: bool, depth: u8) {
        let Some(file) = files.iter().find(|f| f.path == path) else {
            return;
        };
        let Some(lines) = &file.lines else {
            self.hidden |= !in_match;
            return;
        };
        for (key, args) in lines.iter().filter_map(|l| directive(l)) {
            match key.as_str() {
                "match" => in_match = true,
                "include" => {
                    if depth < 8 {
                        for path in included(files, args) {
                            self.walk(files, path, in_match, depth + 1);
                        }
                    }
                }
                _ if in_match => {}
                "port" => self.ports.extend(args.parse::<u16>()),
                _ => {
                    let hidden = self.hidden;
                    self.values.entry(key).or_insert_with(|| (!hidden).then(|| args.to_string()));
                }
            }
        }
    }

    fn setting(&self, key: &str) -> Setting<'_> {
        match self.values.get(key) {
            Some(Some(v)) => Setting::Set(v),
            Some(None) => Setting::Hidden,
            None if self.hidden => Setting::Hidden,
            None => Setting::Default,
        }
    }

    fn flag(&self, key: &str, default: bool) -> Option<bool> {
        match self.setting(key) {
            Setting::Set(v) => Some(first_word(v) == "yes"),
            Setting::Default => Some(default),
            Setting::Hidden => None,
        }
    }

    /// None when it hinges on a file only root can read.
    fn password_allowed(&self) -> Option<bool> {
        let password = self.flag("passwordauthentication", true);
        let kbd = or(self.flag("kbdinteractiveauthentication", true), self.flag("challengeresponseauthentication", false));
        // Keyboard-interactive only ever asks for a password through PAM on Linux.
        let kbd = and(kbd, self.flag("usepam", false));
        let methods = match self.setting("authenticationmethods") {
            Setting::Set(v) => Some(methods_allow_password(v)),
            Setting::Default => Some(true),
            Setting::Hidden => None,
        };
        and(or(password, kbd), methods)
    }
}

/// AuthenticationMethods like "publickey,password" ask for a key as well,
/// so a guessed password alone gets nobody in.
fn methods_allow_password(value: &str) -> bool {
    value == "any"
        || value
            .split_whitespace()
            .any(|list| list.split(',').all(|m| m == "password" || m.starts_with("keyboard-interactive")))
}

fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or_default().to_ascii_lowercase()
}

/// Splits a config line into its lowercase keyword and arguments.
fn directive(line: &str) -> Option<(String, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
    let (key, rest) = line.split_at(end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    let args = rest.split(" #").next().unwrap_or_default().split("\t#").next().unwrap_or_default().trim();
    let key = key.to_ascii_lowercase();
    // The old name for the same switch since OpenSSH 8.7.
    let key = if key == "challengeresponseauthentication" { "kbdinteractiveauthentication".to_string() } else { key };
    Some((key, args))
}

/// Include patterns as absolute paths; relative ones are under /etc/ssh.
fn include_patterns(args: &str) -> impl Iterator<Item = String> + '_ {
    args.split_whitespace().map(|p| p.trim_matches('"')).map(|p| if p.starts_with('/') { p.to_string() } else { format!("/etc/ssh/{p}") })
}

/// The dumped files an Include pulls in, in the order sshd reads them.
fn included<'a>(files: &[ConfigFile<'a>], args: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    for pattern in include_patterns(args) {
        let mut hits: Vec<&str> = files.iter().map(|f| f.path).filter(|p| glob_match(pattern.as_bytes(), p.as_bytes())).collect();
        hits.sort_unstable();
        hits.dedup();
        out.extend(hits);
    }
    out
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => glob_match(rest, text) || (text.first().is_some_and(|&c| c != b'/') && glob_match(pattern, &text[1..])),
        Some((&p, rest)) => text.first().is_some_and(|&c| c == p || (p == b'?' && c != b'/')) && glob_match(rest, &text[1..]),
    }
}

/// A drop-in only counts if sshd_config includes it before setting any of
/// `keys` itself, since the first value wins.
fn drop_in_works(files: &[ConfigFile], drop_in: &str, keys: &[String]) -> bool {
    let Some(lines) = files.iter().find(|f| f.path == MAIN_CONFIG).and_then(|f| f.lines.as_ref()) else {
        return false;
    };
    for (key, args) in lines.iter().filter_map(|l| directive(l)) {
        if key == "match" || keys.contains(&key) {
            return false;
        }
        if key == "include" && include_patterns(args).any(|p| glob_match(p.as_bytes(), drop_in.as_bytes())) {
            return true;
        }
    }
    false
}

fn password_login(f: &Facts) -> Finding {
    let Some(sshd) = &f.sshd else {
        return if f.s.get("sshd-files").is_none() {
            unknown(UNFINISHED)
        } else {
            unknown("Couldn't find the SSH server's settings, so this couldn't be checked.")
        };
    };
    match sshd.password_allowed() {
        Some(false) => good("Logging in needs an SSH key, so bots guessing passwords get nowhere."),
        None => unknown(SSHD_HIDDEN),
        Some(true) => {
            let kbd = if f.old_openssh() { "ChallengeResponseAuthentication no" } else { "KbdInteractiveAuthentication no" };
            bad("People can log in with a password, so bots can keep guessing until one works. With key-only login, guessing gets them nowhere.").fix(
                f.sshd_fix("00-ssheesh.conf", &["PasswordAuthentication no", kbd]),
                "Turns off password login. Check first that you can log in with your key, or you could lock yourself out.",
            )
        }
    }
}

fn root_login(f: &Facts) -> Finding {
    let Some(sshd) = &f.sshd else {
        return if f.s.get("sshd-files").is_none() { unknown(UNFINISHED) } else { unknown("Couldn't find the SSH server's settings.") };
    };
    let value = match sshd.setting("permitrootlogin") {
        Setting::Hidden => return unknown(SSHD_HIDDEN),
        Setting::Set(v) => first_word(v),
        Setting::Default => "prohibit-password".into(),
    };
    let fix = |finding: Finding| {
        finding.fix(
            f.sshd_fix("00-ssheesh-root.conf", &["PermitRootLogin prohibit-password"]),
            "Lets root log in with a key only. If you log in as root with a password today, add your key first so you don't lock yourself out.",
        )
    };
    match value.as_str() {
        "yes" if sshd.password_allowed() == Some(false) => fix(warn(
            "Root can log in, though only with a key while password login stays off. Limiting root to keys keeps it that way if passwords are ever turned back on.",
        )),
        "yes" => fix(bad(
            "Root, the account that can do anything, can log in with a password. Bots try root first, and one right guess gives them the whole server.",
        )),
        "prohibit-password" | "without-password" => good("Root can log in only with a key, so guessing passwords for it gets nowhere."),
        "forced-commands-only" => good("Root can log in only with a key, and only to run set commands."),
        "no" => good("Root can't log in over SSH."),
        other => unknown(format!("Root login is set to \"{other}\", which SSHeesh doesn't recognize.")),
    }
}

// ---- open ports

#[derive(Debug)]
pub(crate) struct Listener {
    pub(crate) udp: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) process: Option<String>,
}

pub(crate) fn listeners(lines: &[&str]) -> Option<Vec<Listener>> {
    let ss = lines.iter().map(|l| l.trim()).find(|l| *l == "#ss" || *l == "#netstat")? == "#ss";
    // ss: Netid State Recv-Q Send-Q Local Peer Process
    // netstat: Proto Recv-Q Send-Q Local Foreign [State] [PID/Program]
    let local_at = if ss { 4 } else { 3 };
    let parse = |line: &&str| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let proto = fields.first()?;
        let udp = proto.starts_with("udp");
        if !udp && !proto.starts_with("tcp") {
            return None;
        }
        let (host, port) = fields.get(local_at)?.rsplit_once(':')?;
        let process = if ss {
            line.split_once("((\"").and_then(|(_, rest)| rest.split_once('"')).map(|(name, _)| name.to_string())
        } else {
            fields[local_at + 1..].iter().find_map(|f| {
                let (pid, name) = f.split_once('/')?;
                (pid.bytes().all(|b| b.is_ascii_digit()) && !name.is_empty()).then(|| name.to_string())
            })
        };
        Some(Listener { udp, host: host.to_string(), port: port.parse().ok()?, process })
    };
    Some(lines.iter().filter_map(parse).collect())
}

/// Loopback and link-local addresses can't be reached from the internet.
pub(crate) fn is_local(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.split('%').next().unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback() || ip.is_link_local(),
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unicast_link_local() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
        Err(_) => host == "localhost",
    }
}

/// Not reachable from the internet: this machine only, or a private network
/// (10/8, 172.16/12, 192.168/16, Tailscale's 100.64/10, fc00::/7). A database
/// on the Docker bridge or a LAN address isn't "open to the internet".
fn off_internet(host: &str) -> bool {
    if is_local(host) {
        return true;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.split('%').next().unwrap_or(host);
    let v4 = |ip: std::net::Ipv4Addr| ip.is_private() || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]));
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => v4(ip),
        Ok(IpAddr::V6(ip)) => (ip.segments()[0] & 0xfe00) == 0xfc00 || ip.to_ipv4_mapped().is_some_and(v4),
        Err(_) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Risk {
    Fine,
    /// Should only face this server or a private network.
    Private,
    /// Anyone who reaches it owns the server.
    TakeOver,
}

/// Ports people know by name, and what it means when they face the internet.
pub(crate) const PORTS: &[(u16, &str, Risk)] = &[
    (21, "FTP", Risk::Fine),
    (22, "SSH", Risk::Fine),
    (23, "Telnet", Risk::Private),
    (25, "mail", Risk::Fine),
    (53, "DNS", Risk::Fine),
    (80, "web", Risk::Fine),
    (110, "mail", Risk::Fine),
    (111, "rpcbind", Risk::Fine),
    (143, "mail", Risk::Fine),
    (443, "web", Risk::Fine),
    (445, "Windows file sharing", Risk::Private),
    (465, "mail", Risk::Fine),
    (587, "mail", Risk::Fine),
    (993, "mail", Risk::Fine),
    (995, "mail", Risk::Fine),
    (1194, "OpenVPN", Risk::Fine),
    (1433, "SQL Server", Risk::Private),
    (1521, "Oracle", Risk::Private),
    (2375, "Docker API", Risk::TakeOver),
    (2376, "Docker API", Risk::Private),
    (2379, "etcd", Risk::Private),
    (3306, "MySQL", Risk::Private),
    (5432, "PostgreSQL", Risk::Private),
    (5672, "RabbitMQ", Risk::Private),
    (5900, "VNC", Risk::Private),
    (5984, "CouchDB", Risk::Private),
    (6379, "Redis", Risk::Private),
    (8080, "web", Risk::Fine),
    (8086, "InfluxDB", Risk::Private),
    (8443, "web", Risk::Fine),
    (9042, "Cassandra", Risk::Private),
    (9092, "Kafka", Risk::Private),
    (9200, "Elasticsearch", Risk::Private),
    (9300, "Elasticsearch", Risk::Private),
    (11211, "Memcached", Risk::Private),
    (25565, "Minecraft", Risk::Fine),
    (27017, "MongoDB", Risk::Private),
    (33060, "MySQL", Risk::Private),
    (51820, "WireGuard", Risk::Fine),
];

/// Programs that should stay private on whatever port they use.
pub(crate) const PROGRAMS: &[(&str, &str, Risk)] = &[
    ("postgres", "PostgreSQL", Risk::Private),
    ("mysqld", "MySQL", Risk::Private),
    ("mariadbd", "MariaDB", Risk::Private),
    ("redis-server", "Redis", Risk::Private),
    ("mongod", "MongoDB", Risk::Private),
    ("memcached", "Memcached", Risk::Private),
    ("docker-proxy", "Docker", Risk::Fine),
];

#[derive(Debug)]
struct OpenPort {
    port: u16,
    tcp: bool,
    udp: bool,
    name: Option<String>,
    risk: Risk,
    docker: bool,
}

impl OpenPort {
    /// "5432 (PostgreSQL)", "51820/udp (WireGuard)", "8003".
    fn label(&self) -> String {
        let udp = if self.udp && !self.tcp { "/udp" } else { "" };
        match &self.name {
            Some(name) => format!("{}{udp} ({name})", self.port),
            None => format!("{}{udp}", self.port),
        }
    }

    /// "PostgreSQL (5432)".
    fn named(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.port),
            None => format!("Port {}", self.port),
        }
    }
}

fn public_ports(list: &[Listener]) -> Vec<OpenPort> {
    let mut ports: BTreeMap<u16, OpenPort> = BTreeMap::new();
    // DHCP clients listen on 68 and 546 to get the server its address.
    for l in list.iter().filter(|l| !off_internet(&l.host) && !(l.udp && matches!(l.port, 68 | 546))) {
        let known = PORTS.iter().find(|(p, ..)| *p == l.port).map(|&(_, name, risk)| (name, risk));
        let program = l.process.as_deref().map(|p| PROGRAMS.iter().find(|(n, ..)| *n == p).map_or((p, Risk::Fine), |&(_, name, risk)| (name, risk)));
        let entry = ports.entry(l.port).or_insert(OpenPort { port: l.port, tcp: false, udp: false, name: None, risk: Risk::Fine, docker: false });
        entry.tcp |= !l.udp;
        entry.udp |= l.udp;
        entry.docker |= l.process.as_deref() == Some("docker-proxy");
        entry.risk = entry.risk.max(known.map_or(Risk::Fine, |k| k.1)).max(program.map_or(Risk::Fine, |p| p.1));
        if entry.name.is_none() {
            entry.name = known.or(program).map(|(name, _)| name.to_string());
        }
    }
    ports.into_values().collect()
}

/// "a", "a and b", "a, b and c".
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A list that stays readable on a server with dozens of ports.
fn port_list(ports: &[&OpenPort]) -> String {
    const SHOWN: usize = 10;
    let mut labels: Vec<String> = ports.iter().take(SHOWN).map(|p| p.label()).collect();
    if ports.len() > SHOWN {
        labels.push(format!("{} more", ports.len() - SHOWN));
    }
    join_and(&labels)
}

fn open_ports(f: &Facts) -> Finding {
    let Some(list) = &f.listeners else {
        return if f.s.get("listen").is_none() {
            unknown(UNFINISHED)
        } else {
            unknown("Neither ss nor netstat is installed, so open ports couldn't be listed.")
        };
    };
    let open = public_ports(list);
    if open.is_empty() {
        return good("Nothing listens on a public address, so no program here can be reached from the internet.");
    }
    let (risky, others): (Vec<&OpenPort>, Vec<&OpenPort>) = open.iter().partition(|p| p.risk > Risk::Fine);
    let Some(first) = risky.first() else {
        return good(format!("Open to the internet: {}. None of them is a database or anything else that should stay private.", port_list(&others)));
    };
    let also = if others.is_empty() { String::new() } else { format!(" Also open: {}.", port_list(&others)) };
    if let Some(api) = risky.iter().find(|p| p.risk == Risk::TakeOver) {
        return bad(format!("The Docker API ({}) is open to the internet with no password, so anyone can take over this server.{also}", api.port))
            .note("Make Docker listen on its local socket only: remove the tcp:// address from its settings (daemon.json or its service file).");
    }
    let names: Vec<String> = risky.iter().map(|p| p.named()).collect();
    let (is, it, they) = if risky.len() == 1 { ("is", "it", "It") } else { ("are", "them", "They") };
    let p = first.port;
    let note = if first.docker {
        format!("In docker-compose.yml, publish it as \"127.0.0.1:{p}:{p}\" so only this server can reach it.")
    } else {
        format!("Make it listen on 127.0.0.1 only, or, if it runs in Docker, publish it as \"127.0.0.1:{p}:{p}\".")
    };
    bad(format!(
        "{} {is} open to the internet, where bots scan for {it} all day. {they} should only be reachable from this server or a private network.{also}",
        join_and(&names)
    ))
    .note(note)
}

// ---- firewall

/// An input chain that drops by default or ends in a catch-all drop.
fn nft_blocks(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        if l.contains("hook input") {
            return l.contains("policy drop");
        }
        let mut words = l.split_whitespace().peekable();
        loop {
            match words.peek() {
                Some(&"counter") => {
                    words.next();
                }
                Some(&"packets") | Some(&"bytes") => {
                    words.next();
                    words.next();
                }
                _ => break,
            }
        }
        matches!(words.next(), Some("drop" | "reject"))
    })
}

fn iptables_blocks(lines: &[&str]) -> bool {
    lines.iter().map(|l| l.trim()).any(|l| l == "-P INPUT DROP" || l == "-A INPUT -j DROP" || l.starts_with("-A INPUT -j REJECT"))
}

fn firewall(f: &Facts) -> Finding {
    let (Some(ufw), Some(firewalld), Some(nft), Some(iptables)) = (f.s.get("ufw"), f.s.get("firewalld"), f.s.get("nft"), f.s.get("iptables")) else {
        return unknown(UNFINISHED);
    };
    let ufw_installed = ufw.iter().any(|l| l.trim() == "#installed");
    let ufw_conf_on = ufw.iter().any(|l| l.replace([' ', '\t'], "").eq_ignore_ascii_case("ENABLED=yes"));
    let ufw_on = match ufw.iter().find_map(|l| l.trim().strip_prefix("Status:")) {
        Some(status) => status.trim() == "active",
        // Without root `ufw status` says nothing, but its config can be read.
        None => ufw_installed && ufw_conf_on,
    };
    if ufw_on {
        return good("ufw is on and blocks incoming connections you haven't allowed.");
    }
    if firewalld.iter().any(|l| matches!(l.trim(), "active" | "running")) {
        return good("firewalld is on and blocks incoming connections you haven't allowed.");
    }
    if nft_blocks(nft) || iptables_blocks(iptables) {
        return good("Firewall rules are in place that block incoming connections you haven't allowed.");
    }
    if !f.as_root {
        return unknown("Seeing the firewall rules needs root, and sudo here wants a password.");
    }
    let found = warn("No firewall is on, so any program listening on a public address can be reached from the internet. A firewall in your hosting provider's control panel would also do the job, but SSHeesh can't see that one.");
    let ssh = f.ssh_ports();
    let web = f.web_ports();
    let what = if web.is_empty() { "SSH" } else { "SSH and web traffic" };
    if ufw_installed || f.family == Family::Debian {
        let mut steps: Vec<String> = Vec::new();
        if !ufw_installed {
            steps.push("sudo apt-get install -y ufw".into());
        }
        steps.extend(ssh.iter().chain(&web).map(|p| format!("sudo ufw allow {p}/tcp")));
        steps.push("sudo ufw --force enable".into());
        return found.fix(
            steps.join(" && "),
            format!("Lets {what} in first so you aren't cut off, then blocks every other incoming connection; open more later with sudo ufw allow PORT/tcp."),
        );
    }
    if f.tools.contains("firewall-cmd") {
        // firewalld's default zone already lets SSH on port 22 in.
        let mut steps: Vec<String> = ssh.iter().filter(|&&p| p != 22).map(|p| format!("sudo firewall-offline-cmd --add-port={p}/tcp")).collect();
        steps.extend(web.iter().map(|p| format!("sudo firewall-offline-cmd --add-service={}", if *p == 80 { "http" } else { "https" })));
        steps.push("sudo systemctl enable --now firewalld".into());
        return found.fix(steps.join(" && "), format!("Lets {what} in first so you aren't cut off, then turns on firewalld, which blocks every other incoming connection."));
    }
    found
}

// ---- Docker

/// Host and container side of each mapping Docker opens on a public address.
fn published(ports: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for entry in ports.split(',') {
        let Some((outside, inside)) = entry.trim().split_once("->") else {
            continue;
        };
        let Some((host, port)) = outside.rsplit_once(':') else {
            continue;
        };
        let pair = (port.to_string(), inside.trim_end_matches("/tcp").to_string());
        if !off_internet(host) && !out.contains(&pair) {
            out.push(pair);
        }
    }
    out
}

fn docker_ports(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("docker") else {
        return unknown(UNFINISHED);
    };
    let lines: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if !lines.contains(&"#installed") {
        return good("Docker isn't installed.");
    }
    if let Some(at) = lines.iter().position(|l| *l == "#error") {
        let error = lines[at + 1..].join(" ").to_ascii_lowercase();
        return if error.contains("cannot connect") || error.contains("daemon running") {
            good("Docker is installed but not running.")
        } else if error.contains("permission denied") {
            unknown("Docker is installed, but listing its containers needs root or the docker group, and sudo here wants a password.")
        } else {
            unknown("Docker didn't answer, so its containers couldn't be checked.")
        };
    }
    let Some(at) = lines.iter().position(|l| *l == "#ok") else {
        return unknown(UNFINISHED);
    };
    let open: Vec<(&str, Vec<(String, String)>)> =
        lines[at + 1..].iter().map(|l| l.split_once(' ').unwrap_or((l, ""))).map(|(name, ports)| (name, published(ports))).filter(|(_, p)| !p.is_empty()).collect();
    if open.is_empty() {
        return good("No container is open to the internet.");
    }
    // A website's own ports are meant to be public.
    let private: Vec<(&str, Vec<(String, String)>)> = open
        .into_iter()
        .map(|(name, ports)| (name, ports.into_iter().filter(|(host, _)| host != "80" && host != "443").collect::<Vec<_>>()))
        .filter(|(_, p)| !p.is_empty())
        .collect();
    let Some((_, first)) = private.first() else {
        return good("Containers only open web ports (80 and 443) to the internet, which a website needs.");
    };
    let names: Vec<String> = private
        .iter()
        .map(|(name, ports)| format!("{name} ({})", ports.iter().map(|(host, _)| host.as_str()).collect::<Vec<_>>().join(", ")))
        .collect();
    let (what, is, them) = if names.len() == 1 { ("The container", "is", "it") } else { ("The containers", "are", "them") };
    let (host, inside) = &first[0];
    warn(format!(
        "{what} {} {is} open to the internet. Docker opens these ports itself, going around ufw and other firewalls, so turning on a firewall won't close {them}.",
        join_and(&names)
    ))
    .note(format!("In docker-compose.yml, write the port as \"127.0.0.1:{host}:{inside}\" so only this server can reach it, then run docker compose up -d."))
}

// ---- updates, guards, restarts

fn auto_updates(f: &Facts) -> Finding {
    match f.family {
        Family::Debian => apt_updates(f),
        Family::Fedora | Family::Rhel => dnf_updates(f),
        _ if f.s.get("os").is_none() => unknown(UNFINISHED),
        _ => unknown("SSHeesh can check automatic updates on Debian, Ubuntu, Fedora and other Red Hat family systems only."),
    }
}

const NO_UPDATES: &str = "Updates don't install by themselves, so known security holes stay open until someone updates by hand.";

fn apt_updates(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("apt") else {
        return unknown(UNFINISHED);
    };
    let periodic = |key: &str| {
        lines.iter().find_map(|l| {
            let value = l.trim().strip_prefix("APT::Periodic::")?.strip_prefix(key)?.strip_prefix(' ')?;
            Some(value.trim().trim_end_matches(';').trim_matches('"').to_string())
        })
    };
    let installed = lines.iter().any(|l| l.trim() == "pkg install ok installed");
    let scheduled = periodic("Unattended-Upgrade").is_some_and(|v| !v.is_empty() && v != "0");
    let enabled = periodic("Enable").is_none_or(|v| v != "0");
    let timer = lines.iter().find_map(|l| l.trim().strip_prefix("timer")).map(str::trim);
    let timer_off = matches!(timer, Some("disabled" | "masked"));
    if installed && scheduled && enabled && !timer_off {
        return good("Security updates install by themselves every day.");
    }
    let mut fix = "echo 'unattended-upgrades unattended-upgrades/enable_auto_updates boolean true' | sudo debconf-set-selections && sudo apt-get install -y unattended-upgrades && sudo dpkg-reconfigure -f noninteractive unattended-upgrades".to_string();
    if timer_off {
        fix.push_str(" && sudo systemctl enable --now apt-daily.timer apt-daily-upgrade.timer");
    }
    warn(NO_UPDATES).fix(fix, "Installs unattended-upgrades and turns it on, so security updates install every day.")
}

fn dnf_updates(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("dnf") else {
        return unknown(UNFINISHED);
    };
    if !f.tools.contains("dnf") {
        return unknown("This system has no dnf, so automatic updates couldn't be checked.");
    }
    let enabled = |unit: &str| {
        lines.iter().any(|l| {
            let mut w = l.split_whitespace();
            w.next() == Some("timer") && w.next() == Some(unit) && w.next().is_some_and(|s| s.starts_with("enabled"))
        })
    };
    let apply = lines
        .iter()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "apply_updates")
        .is_some_and(|(_, v)| matches!(v.trim().to_ascii_lowercase().as_str(), "yes" | "true" | "1" | "on"));
    let timer = enabled("dnf-automatic.timer") || enabled("dnf5-automatic.timer");
    if enabled("dnf-automatic-install.timer") || (timer && apply) {
        return good("Updates install by themselves every day.");
    }
    let found = if timer { warn("Updates are downloaded by themselves but never installed, so known security holes stay open.") } else { warn(NO_UPDATES) };
    let fix = if f.tools.contains("dnf5") {
        "sudo dnf install -y dnf5-plugin-automatic && printf '[commands]\\napply_updates = yes\\n' | sudo tee /etc/dnf/automatic.conf >/dev/null && sudo systemctl enable --now dnf5-automatic.timer"
    } else {
        "sudo dnf install -y dnf-automatic && sudo systemctl enable --now dnf-automatic-install.timer"
    };
    found.fix(fix, "Installs dnf-automatic and turns it on, so updates install every day.")
}

/// Holes in these let someone in or take over: logins, TLS, root, the
/// kernel, and whatever faces the internet.
const CRITICAL: &[&str] = &[
    "openssh", "openssl", "libssl", "sudo", "linux-image", "kernel", "libc6", "glibc", "systemd", "curl", "libcurl", "nginx", "apache2", "httpd", "docker",
    "containerd",
];

/// Package lists older than this miss recent fixes.
const STALE_DAYS: u64 = 7;

fn is_kernel(name: &str) -> bool {
    name.starts_with("kernel")
        || matches!(name, "linux" | "linux-lts" | "linux-zen" | "linux-hardened")
        || ["linux-image", "linux-modules", "linux-headers", "linux-generic", "linux-virtual", "linux-signed"].iter().any(|p| name.starts_with(p))
}

fn is_critical(name: &str) -> bool {
    is_kernel(name) || CRITICAL.iter().any(|p| name.starts_with(p))
}

/// Security fixes that are out but not installed yet.
#[derive(Debug, Default)]
struct Pending {
    packages: Vec<String>,
    cves: Vec<String>,
    /// The distribution itself rates one of them critical or important.
    severe: bool,
}

impl Pending {
    fn add_package(&mut self, name: &str) {
        if !self.packages.iter().any(|p| p == name) {
            self.packages.push(name.to_string());
        }
    }

    fn add_cve(&mut self, id: &str) {
        let id = id.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if id.starts_with("CVE-") && !self.cves.iter().any(|c| c == id) {
            self.cves.push(id.to_string());
        }
    }

    /// "12 security fixes are waiting, including openssh-server, openssl
    /// and the kernel. They fix CVE-…"
    fn describe(&self) -> String {
        let critical = self.packages.iter().filter(|n| is_critical(n));
        let mut names: Vec<String> = Vec::new();
        for name in critical.chain(self.packages.iter().filter(|n| !is_critical(n))) {
            let shown = if is_kernel(name) { "the kernel".to_string() } else { name.clone() };
            if !names.contains(&shown) {
                names.push(shown);
            }
        }
        let count = self.packages.len();
        let mut text = if count == 1 {
            format!("A security fix is waiting for {}.", names[0])
        } else if names.len() == count && count <= 6 {
            format!("{count} security fixes are waiting: {}.", join_and(&names))
        } else {
            names.truncate(6);
            format!("{count} security fixes are waiting, including {}.", join_and(&names))
        };
        if !self.cves.is_empty() {
            let mut ids: Vec<String> = self.cves.iter().take(5).cloned().collect();
            if self.cves.len() > 5 {
                ids.push(format!("{} more", self.cves.len() - 5));
            }
            let they = if count == 1 { "It fixes" } else { "They fix" };
            text.push_str(&format!(" {they} {}.", join_and(&ids)));
        }
        text
    }
}

/// Days since the package lists were refreshed, when the script could tell.
fn lists_age(lines: &[&str]) -> Option<u64> {
    let stamp = |key: &str| lines.iter().find_map(|l| l.trim().strip_prefix(key)?.trim().parse::<u64>().ok());
    Some(stamp("now ")?.saturating_sub(stamp("lists ")?) / 86_400)
}

fn days_ago(days: u64) -> String {
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        d => format!("{d} days ago"),
    }
}

fn pending_fixes(p: &Pending, age: Option<u64>, fix: &str) -> Finding {
    let stale = age.filter(|&d| d > STALE_DAYS);
    if p.packages.is_empty() {
        return match (age, stale) {
            (_, Some(d)) => warn(format!(
                "No security fixes show up, but the package lists were last refreshed {}, so newer fixes can't be seen.",
                days_ago(d)
            ))
            .fix(fix, "Refreshes the package lists and installs whatever fixes turn up."),
            (Some(d), None) => good(format!("No security fixes are waiting, going by package lists refreshed {}.", days_ago(d))),
            (None, None) => good("No security fixes are waiting."),
        };
    }
    let mut detail = p.describe();
    if let Some(d) = stale {
        detail.push_str(&format!(" The package lists were last refreshed {}, so the real number may be higher.", days_ago(d)));
    }
    let note = if p.packages.iter().any(|n| is_kernel(n)) {
        "Installs the fixes; the kernel one only takes effect after a restart."
    } else {
        "Installs the fixes."
    };
    let verdict = if p.severe || p.packages.iter().any(|n| is_critical(n)) { Verdict::Bad } else { Verdict::Warn };
    Finding::new(verdict, detail).fix(fix, note)
}

fn security_updates(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("secupd") else {
        return unknown(UNFINISHED);
    };
    match lines.iter().map(|l| l.trim()).find(|l| l.starts_with('#')) {
        Some("#apt") => apt_security(lines),
        Some("#dnf") => dnf_security(f, lines),
        Some("#arch") => arch_security(lines),
        Some("#pacman") => unknown("Arch doesn't mark which updates fix security holes. With arch-audit installed (sudo pacman -S arch-audit), SSHeesh can count them."),
        _ => unknown("This system doesn't mark which updates fix security holes, so they can't be counted."),
    }
}

fn value<'a>(lines: &[&'a str], key: &str) -> Option<&'a str> {
    lines.iter().find_map(|l| l.trim().strip_prefix(key)).map(str::trim)
}

/// The package an `apt-get -s` line installs from a security archive, like
/// "Inst openssl [3.0.13-0ubuntu3.4] (3.0.13-0ubuntu3.5 Ubuntu:24.04/noble-security [amd64])".
fn apt_security_install(line: &str) -> Option<&str> {
    let (name, rest) = line.trim().strip_prefix("Inst ")?.split_once(' ')?;
    let origin = &rest[rest.find('(')?..];
    origin.to_ascii_lowercase().contains("security").then(|| name.split(':').next().unwrap_or(name))
}

fn apt_security(lines: &[&str]) -> Finding {
    const FIX: &str = "sudo apt-get update && sudo apt-get upgrade --with-new-pkgs";
    match value(lines, "sim ") {
        None => return unknown(UNFINISHED),
        Some("0") => {}
        Some("124") => return unknown("apt took too long to work out the updates."),
        Some(_) => {
            let said = value(lines, "E: ").map(|e| format!(" It said: {e}")).unwrap_or_default();
            return unknown(format!("apt couldn't work out the updates, so pending fixes couldn't be counted.{said}"));
        }
    }
    let Some(age) = lists_age(lines) else {
        return unknown("There are no package lists yet, so pending fixes can't be counted.").fix(FIX, "Downloads the package lists and installs any fixes.");
    };
    let mut p = Pending::default();
    for name in lines.iter().filter_map(|l| apt_security_install(l)) {
        p.add_package(name);
    }
    // debsecan lines: "CVE-2024-6387 openssh-server (fixed, remotely exploitable, high urgency)".
    let vulns: Vec<(&str, &str)> = lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix("vuln "))
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?, w.next()?))
        })
        .collect();
    for (cve, pkg) in vulns {
        if p.packages.iter().any(|n| n == pkg) {
            p.add_cve(cve);
        }
    }
    pending_fixes(&p, Some(age), FIX)
}

/// "openssl-libs-1:3.1.4-3.fc39.x86_64" → "openssl-libs".
fn rpm_name(nevra: &str) -> Option<&str> {
    let (nevr, arch) = nevra.rsplit_once('.')?;
    if !["x86_64", "noarch", "aarch64", "i686", "ppc64le", "s390x", "armv7hl"].contains(&arch) {
        return None;
    }
    let mut parts = nevr.rsplitn(3, '-');
    parts.next()?;
    parts.next()?;
    parts.next()
}

fn dnf_security(f: &Facts, lines: &[&str]) -> Finding {
    let advisories: Vec<&str> = lines.iter().filter_map(|l| l.trim().strip_prefix("adv ")).collect();
    match value(lines, "list ") {
        None => return unknown(UNFINISHED),
        Some("0") => {}
        Some("124") => return unknown("dnf took too long to list security updates."),
        Some(_) if !f.as_root => {
            return unknown("Checking needs dnf's saved package lists, which only root can use here, and sudo here wants a password.");
        }
        Some(_) if advisories.join(" ").to_ascii_lowercase().contains("cache") => {
            return unknown("dnf hasn't saved any package lists yet, and this check doesn't go online. Running sudo dnf makecache once fixes that.");
        }
        Some(_) => return unknown("dnf couldn't list security updates."),
    }
    let mut p = Pending::default();
    // "RLSA-2024:4312 Important/Sec. openssh-8.7p1-38.el9_4.1.x86_64", or dnf5's columns.
    for words in advisories.iter().map(|l| l.split_whitespace().collect::<Vec<_>>()) {
        let Some(name) = words.iter().find_map(|w| rpm_name(w)) else {
            continue;
        };
        p.add_package(name);
        p.severe |= words.iter().any(|w| matches!(w.split('/').next(), Some("Critical" | "Important")));
    }
    for cve in lines.iter().filter_map(|l| l.trim().strip_prefix("cve ")) {
        p.add_cve(cve);
    }
    pending_fixes(&p, lists_age(lines), "sudo dnf upgrade --security")
}

fn arch_security(lines: &[&str]) -> Finding {
    match value(lines, "audit ") {
        None => return unknown(UNFINISHED),
        Some("0") => {}
        Some(_) => return unknown("arch-audit couldn't fetch Arch's list of security issues."),
    }
    let mut p = Pending::default();
    // "Package openssl is affected by CVE-2023-0464, CVE-2023-0465. High risk! Update to 3.0.8-2!"
    for words in lines.iter().filter_map(|l| l.trim().strip_prefix("aa ")).map(|l| l.split_whitespace().collect::<Vec<_>>()) {
        let &[first, name, ..] = words.as_slice() else {
            continue;
        };
        if first != "Package" {
            continue;
        }
        p.add_package(name);
        for w in &words {
            p.add_cve(w);
        }
        p.severe |= words.windows(2).any(|w| w[1].starts_with("risk") && matches!(w[0], "Critical" | "High"));
    }
    pending_fixes(&p, None, "sudo pacman -Syu")
}

fn brute_force(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("guards") else {
        return unknown(UNFINISHED);
    };
    let running = lines.iter().find_map(|l| match l.trim() {
        "fail2ban" => Some("fail2ban"),
        "crowdsec" => Some("CrowdSec"),
        "sshguard" => Some("SSHGuard"),
        _ => None,
    });
    if let Some(name) = running {
        return good(format!("{name} is running, so an address that keeps failing to log in gets blocked."));
    }
    let found = match f.password_allowed() {
        Some(false) => return good("Nothing blocks repeated login attempts, and that's fine: password login is off and bots can't guess a key."),
        Some(true) => warn("Nothing stops bots from trying password after password."),
        None => warn("Nothing blocks addresses that keep failing to log in, which matters if password login is on."),
    };
    let enable_sshd_jail = "printf '[sshd]\\nenabled = true\\n' | sudo tee /etc/fail2ban/jail.d/ssheesh.local >/dev/null && sudo systemctl enable --now fail2ban";
    let fix = match f.family {
        Family::Debian if f.tools.contains("auth.log") => "sudo apt-get install -y fail2ban".to_string(),
        // Without /var/log/auth.log, fail2ban has to read failed logins from the journal.
        Family::Debian => "sudo apt-get install -y fail2ban python3-systemd && printf '[sshd]\\nbackend = systemd\\n' | sudo tee /etc/fail2ban/jail.d/ssheesh.local >/dev/null && sudo systemctl restart fail2ban".to_string(),
        Family::Fedora => format!("sudo dnf install -y fail2ban && {enable_sshd_jail}"),
        // fail2ban comes from EPEL on the rebuilds; real RHEL needs a subscription step first.
        Family::Rhel if f.os_id != "rhel" => format!("sudo dnf install -y epel-release && sudo dnf install -y fail2ban && {enable_sshd_jail}"),
        _ => return found,
    };
    found.fix(fix, "Installs fail2ban, which blocks an address for a while after a few failed logins.")
}

fn reboot(f: &Facts) -> Finding {
    let Some(lines) = f.s.get("reboot") else {
        return unknown(UNFINISHED);
    };
    let field = |prefix: &'static str| lines.iter().filter_map(move |l| l.trim().strip_prefix(prefix)).map(str::trim);
    let flagged = field("required").next().is_some();
    let said = field("say ").collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let needs_restarting = field("restarting ").next() == Some("1") && said.contains("reboot is required");
    // Arch and Alpine delete the old kernel's modules on upgrade, so a
    // missing folder for the running kernel means a newer one awaits.
    let modules: Vec<&str> = field("module ").collect();
    let kernel = field("kernel ").next();
    let new_kernel = matches!(f.family, Family::Arch | Family::Other) && !modules.is_empty() && kernel.is_some_and(|k| !modules.contains(&k));
    if flagged || needs_restarting || new_kernel {
        warn("Installed updates are waiting for a restart; until then the old versions keep running.")
            .fix("sudo reboot", "Restarts the server, which drops connections for a minute or so; SSHeesh reconnects by itself.")
    } else {
        good("No restart is waiting.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_not_open_to_the_internet() {
        for private in ["10.0.0.5", "172.17.0.1", "192.168.1.20", "100.101.102.103", "[fd00::1]", "127.0.0.1", "::1"] {
            assert!(off_internet(private), "{private}");
        }
        for public in ["0.0.0.0", "*", "203.0.113.7", "[2001:db8::1]", "::", "100.128.0.1"] {
            assert!(!off_internet(public), "{public}");
        }
    }

    const UBUNTU_UFW: &str = r#"Welcome to Ubuntu
@@root@@
1
@@os@@
PRETTY_NAME="Ubuntu 24.04.1 LTS"
NAME="Ubuntu"
VERSION_ID="24.04"
ID=ubuntu
ID_LIKE=debian
@@tools@@
systemctl
sshd
ss
ufw
nft
iptables
apt-get
pgrep
auth.log
@@conn@@
203.0.113.7 51234 198.51.100.10 22
@@sshv@@
OpenSSH_9.6p1 Ubuntu-3ubuntu13.5, OpenSSL 3.0.13 30 Jan 2024
@@sshd-t@@
port 22
usepam yes
permitrootlogin without-password
passwordauthentication no
kbdinteractiveauthentication no
authenticationmethods any
@@sshd-files@@
@file /etc/ssh/sshd_config
Include /etc/ssh/sshd_config.d/*.conf
KbdInteractiveAuthentication no
UsePAM yes
Subsystem sftp /usr/lib/openssh/sftp-server

@file /etc/ssh/sshd_config.d/50-cloud-init.conf
PasswordAuthentication no

@@listen@@
#ss
Netid State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process
udp   UNCONN 0      0      127.0.0.53%lo:53         0.0.0.0:*     users:(("systemd-resolve",pid=612,fd=13))
udp   UNCONN 0      0      10.0.0.5%eth0:68         0.0.0.0:*     users:(("systemd-network",pid=590,fd=22))
tcp   LISTEN 0      4096         0.0.0.0:22         0.0.0.0:*     users:(("sshd",pid=1374,fd=3),("systemd",pid=1,fd=251))
tcp   LISTEN 0      511          0.0.0.0:80         0.0.0.0:*     users:(("nginx",pid=901,fd=6))
tcp   LISTEN 0      511          0.0.0.0:443        0.0.0.0:*     users:(("nginx",pid=901,fd=7))
tcp   LISTEN 0      244        127.0.0.1:5432       0.0.0.0:*     users:(("postgres",pid=822,fd=5))
tcp   LISTEN 0      4096            [::]:22            [::]:*     users:(("sshd",pid=1374,fd=4),("systemd",pid=1,fd=252))
tcp   LISTEN 0      511             [::]:80            [::]:*     users:(("nginx",pid=901,fd=8))
tcp   LISTEN 0      244            [::1]:5432          [::]:*     users:(("postgres",pid=822,fd=6))
@@ufw@@
#installed
Status: active
ENABLED=yes
@@firewalld@@
@@nft@@
	chain INPUT {
		type filter hook input priority filter; policy drop;
		counter packets 1840 bytes 113120 jump ufw-before-logging-input
	}
@@iptables@@
-P INPUT DROP
-A INPUT -j ufw-before-logging-input
@@guards@@
@@apt@@
pkg install ok installed
APT::Periodic::Unattended-Upgrade "1";
timer enabled
@@dnf@@
@@secupd@@
#apt
now 1793000000
lists 1792990000
sim 0
@@reboot@@
kernel 6.8.0-45-generic
module 6.8.0-41-generic
module 6.8.0-45-generic
@@docker@@
@@end@@
"#;

    const DEBIAN_OPEN: &str = r#"@@root@@
1
@@os@@
PRETTY_NAME="Debian GNU/Linux 12 (bookworm)"
ID=debian
@@tools@@
systemctl
sshd
ss
ufw
nft
iptables
docker
apt-get
pgrep
@@conn@@
203.0.113.7 50022 198.51.100.20 22
@@sshv@@
OpenSSH_9.2p1 Debian-2+deb12u3, OpenSSL 3.0.14 4 Jun 2024
@@sshd-t@@
port 22
usepam yes
permitrootlogin yes
passwordauthentication yes
kbdinteractiveauthentication no
authenticationmethods any
@@sshd-files@@
@file /etc/ssh/sshd_config
Include /etc/ssh/sshd_config.d/*.conf
PermitRootLogin yes
KbdInteractiveAuthentication no
UsePAM yes

@@listen@@
#ss
Netid State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process
tcp   LISTEN 0      4096         0.0.0.0:5432      0.0.0.0:*    users:(("docker-proxy",pid=2201,fd=4))
tcp   LISTEN 0      128          0.0.0.0:22        0.0.0.0:*    users:(("sshd",pid=600,fd=3))
tcp   LISTEN 0      4096         0.0.0.0:80        0.0.0.0:*    users:(("docker-proxy",pid=2240,fd=4))
tcp   LISTEN 0      4096            [::]:5432         [::]:*    users:(("docker-proxy",pid=2207,fd=4))
tcp   LISTEN 0      128             [::]:22           [::]:*    users:(("sshd",pid=600,fd=4))
tcp   LISTEN 0      511                *:3000            *:*    users:(("node",pid=3100,fd=20))
@@ufw@@
#installed
Status: inactive
ENABLED=no
@@firewalld@@
@@nft@@
@@iptables@@
-P INPUT ACCEPT
-P INPUT ACCEPT
@@guards@@
@@apt@@
timer enabled
@@dnf@@
@@secupd@@
#apt
now 1793000000
lists 1792900000
sim 0
Inst openssh-client [1:9.2p1-2+deb12u3] (1:9.2p1-2+deb12u4 Debian-Security:12/stable-security [amd64])
Inst openssh-server [1:9.2p1-2+deb12u3] (1:9.2p1-2+deb12u4 Debian-Security:12/stable-security [amd64]) []
Inst tzdata [2024a-0+deb12u1] (2024b-0+deb12u1 Debian:12.8/stable [all])
Inst libssl3 [3.0.14-1~deb12u2] (3.0.15-1~deb12u1 Debian-Security:12/stable-security [amd64])
Inst linux-image-6.1.0-27-amd64 (6.1.115-1 Debian-Security:12/stable-security [amd64])
vuln CVE-2024-6387 openssh-server (fixed, remotely exploitable, high urgency)
vuln CVE-2024-5535 libssl3 (fixed, low urgency)
vuln CVE-2023-1111 libfoo (fixed)
@@reboot@@
required
kernel 6.1.0-25-amd64
module 6.1.0-25-amd64
module 6.1.0-26-amd64
@@docker@@
#installed
#ok
db 0.0.0.0:5432->5432/tcp, :::5432->5432/tcp
web 0.0.0.0:80->80/tcp, :::80->80/tcp
cache 127.0.0.1:6379->6379/tcp
worker
@@end@@
"#;

    const UBUNTU_USER: &str = r#"@@root@@
0
@@os@@
ID=ubuntu
ID_LIKE=debian
@@tools@@
systemctl
sshd
ss
ufw
nft
iptables
docker
apt-get
pgrep
auth.log
@@conn@@
203.0.113.7 50022 198.51.100.30 22
@@sshv@@
OpenSSH_9.6p1 Ubuntu-3ubuntu13.5, OpenSSL 3.0.13 30 Jan 2024
@@sshd-t@@
@@sshd-files@@
@file /etc/ssh/sshd_config
Include /etc/ssh/sshd_config.d/*.conf
KbdInteractiveAuthentication no
UsePAM yes

@unreadable /etc/ssh/sshd_config.d/50-cloud-init.conf
@file /etc/ssh/sshd_config.d/60-cloudimg-settings.conf
PasswordAuthentication no

@@listen@@
#ss
Netid State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process
tcp   LISTEN 0      4096         0.0.0.0:22        0.0.0.0:*
tcp   LISTEN 0      4096            [::]:22           [::]:*
tcp   LISTEN 0      511        127.0.0.1:3000      0.0.0.0:*
@@ufw@@
#installed
ENABLED=no
@@firewalld@@
@@nft@@
@@iptables@@
@@guards@@
@@apt@@
pkg install ok installed
APT::Periodic::Unattended-Upgrade "1";
timer enabled
@@dnf@@
@@secupd@@
#apt
now 1793000000
lists 1792950000
sim 0
Inst libxml2 [2.9.14+dfsg-1.3ubuntu3] (2.9.14+dfsg-1.3ubuntu3.1 Ubuntu:24.04/noble-updates, Ubuntu:24.04/noble-security [amd64])
Inst vim-tiny [2:9.1.0016-1ubuntu7.2] (2:9.1.0016-1ubuntu7.3 Ubuntu:24.04/noble-updates, Ubuntu:24.04/noble-security [amd64])
Inst python3-urllib3 [2.0.7-1] (2.0.7-1ubuntu0.1 UbuntuESMApps:24.04/noble-apps-security [all])
@@reboot@@
kernel 6.8.0-45-generic
module 6.8.0-45-generic
@@docker@@
#installed
#error
permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock: Get "http://%2Fvar%2Frun%2Fdocker.sock/v1.47/containers/json": dial unix /var/run/docker.sock: connect: permission denied
@@end@@
"#;

    fn find<'a>(c: &'a Checkup, id: &str) -> &'a Check {
        c.checks.iter().find(|x| x.id == id).unwrap()
    }

    fn verdicts(c: &Checkup) -> Vec<(&str, Verdict)> {
        c.checks.iter().map(|x| (x.id.as_str(), x.verdict)).collect()
    }

    #[test]
    fn every_check_is_there_in_order() {
        let ids: Vec<String> = parse("").checks.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["password-login", "root-login", "open-ports", "firewall", "docker-ports", "auto-updates", "security-updates", "brute-force", "reboot"]);
    }

    #[test]
    fn a_tidy_ubuntu_box_is_all_good() {
        let c = parse(UBUNTU_UFW);
        assert!(c.as_root);
        for check in &c.checks {
            assert_eq!(check.verdict, Verdict::Good, "{}: {}", check.id, check.detail);
            assert!(check.fix.is_none());
        }
        assert_eq!(find(&c, "open-ports").detail, "Open to the internet: 22 (SSH), 80 (web) and 443 (web). None of them is a database or anything else that should stay private.");
        assert!(find(&c, "brute-force").detail.contains("can't guess a key"));
        assert!(find(&c, "firewall").detail.starts_with("ufw is on"));
    }

    #[test]
    fn password_login_and_a_public_database() {
        let c = parse(DEBIAN_OPEN);
        assert_eq!(
            verdicts(&c),
            [
                ("password-login", Verdict::Bad),
                ("root-login", Verdict::Bad),
                ("open-ports", Verdict::Bad),
                ("firewall", Verdict::Warn),
                ("docker-ports", Verdict::Warn),
                ("auto-updates", Verdict::Warn),
                ("security-updates", Verdict::Bad),
                ("brute-force", Verdict::Warn),
                ("reboot", Verdict::Warn),
            ]
        );
        let pw = find(&c, "password-login");
        assert_eq!(
            pw.fix.as_deref(),
            Some("printf 'PasswordAuthentication no\\nKbdInteractiveAuthentication no\\n' | sudo tee /etc/ssh/sshd_config.d/00-ssheesh.conf >/dev/null && sudo sshd -t && (sudo systemctl reload ssh || sudo systemctl reload sshd)")
        );
        assert!(pw.fix_note.as_deref().unwrap().contains("lock yourself out"));
        // PermitRootLogin sits in sshd_config after the Include, so a drop-in still wins.
        assert!(find(&c, "root-login").fix.as_deref().unwrap().contains("sshd_config.d/00-ssheesh-root.conf"));

        let ports = find(&c, "open-ports");
        assert!(ports.detail.starts_with("PostgreSQL (5432) is open to the internet"), "{}", ports.detail);
        assert!(ports.detail.contains("Also open: 22 (SSH), 80 (web) and 3000 (node)."), "{}", ports.detail);
        assert!(ports.fix.is_none());
        assert!(ports.fix_note.as_deref().unwrap().contains("127.0.0.1:5432:5432"));

        let fw = find(&c, "firewall");
        assert_eq!(fw.fix.as_deref(), Some("sudo ufw allow 22/tcp && sudo ufw allow 80/tcp && sudo ufw --force enable"));
        assert!(fw.fix_note.as_deref().unwrap().contains("SSH and web traffic"));

        let docker = find(&c, "docker-ports");
        assert!(docker.detail.starts_with("The container db (5432) is open"), "{}", docker.detail);
        assert!(docker.fix.is_none());
        assert!(docker.fix_note.as_deref().unwrap().contains("\"127.0.0.1:5432:5432\""));

        assert!(find(&c, "auto-updates").fix.as_deref().unwrap().contains("apt-get install -y unattended-upgrades"));
        // Debian 12 has no auth.log, so fail2ban must read the journal.
        assert!(find(&c, "brute-force").fix.as_deref().unwrap().contains("backend = systemd"));
        assert_eq!(find(&c, "reboot").fix.as_deref(), Some("sudo reboot"));
    }

    #[test]
    fn without_root_some_checks_are_unknown() {
        let c = parse(UBUNTU_USER);
        assert!(!c.as_root);
        assert_eq!(
            verdicts(&c),
            [
                ("password-login", Verdict::Unknown),
                ("root-login", Verdict::Unknown),
                ("open-ports", Verdict::Good),
                ("firewall", Verdict::Unknown),
                ("docker-ports", Verdict::Unknown),
                ("auto-updates", Verdict::Good),
                ("security-updates", Verdict::Warn),
                ("brute-force", Verdict::Warn),
                ("reboot", Verdict::Good),
            ]
        );
        assert!(find(&c, "password-login").detail.contains("sudo here wants a password"));
        assert!(find(&c, "firewall").detail.contains("needs root"));
        assert!(find(&c, "docker-ports").detail.contains("docker group"));
        assert_eq!(find(&c, "brute-force").fix.as_deref(), Some("sudo apt-get install -y fail2ban"));
        assert_eq!(find(&c, "open-ports").detail, "Open to the internet: 22 (SSH). None of them is a database or anything else that should stay private.");
    }

    #[test]
    fn a_run_cut_short_reports_what_it_reached() {
        let cut = UBUNTU_USER.split("@@ufw@@").next().unwrap();
        let c = parse(cut);
        assert_eq!(find(&c, "open-ports").verdict, Verdict::Good);
        for id in ["firewall", "docker-ports", "security-updates", "brute-force", "reboot"] {
            assert_eq!(find(&c, id).detail, UNFINISHED, "{id}");
        }
    }

    #[test]
    fn reads_sshd_config_like_sshd() {
        let dump = [
            "@file /etc/ssh/sshd_config",
            "# PasswordAuthentication yes",
            "Port 2222",
            "Include sshd_config.d/*.conf",
            "PasswordAuthentication yes",
            "PermitRootLogin=no # comment",
            "Match User deploy",
            "  PermitRootLogin yes",
            "  Include /etc/ssh/extra.conf",
            "@file /etc/ssh/sshd_config.d/10-b.conf",
            "Port 22",
            "UsePAM yes",
            "@file /etc/ssh/sshd_config.d/05-a.conf",
            "passwordauthentication no",
            "@unreadable /etc/ssh/extra.conf",
        ];
        let files = config_files(&dump);
        let s = Sshd::from_files(&files).unwrap();
        assert_eq!(first_word(s.values["passwordauthentication"].as_deref().unwrap()), "no");
        assert_eq!(s.values["permitrootlogin"].as_deref(), Some("no"));
        assert_eq!(s.ports, [2222, 22]);
        // The unreadable file only matters inside Match, so nothing is hidden.
        assert!(!s.hidden);
        // Keyboard-interactive is on by default and asks for a password through PAM.
        assert_eq!(s.password_allowed(), Some(true));
    }

    #[test]
    fn an_unreadable_include_hides_what_follows() {
        let files = config_files(&["@file /etc/ssh/sshd_config", "PasswordAuthentication no", "Include /etc/ssh/sshd_config.d/*.conf", "PermitRootLogin no", "@unreadable /etc/ssh/sshd_config.d/50-cloud-init.conf"]);
        let s = Sshd::from_files(&files).unwrap();
        assert_eq!(s.flag("passwordauthentication", true), Some(false));
        assert!(matches!(s.setting("permitrootlogin"), Setting::Hidden));
        // The hidden file could still turn keyboard-interactive on.
        assert_eq!(s.password_allowed(), None);
    }

    #[test]
    fn password_login_needs_pam_for_keyboard_interactive() {
        let test = |lines: &[&str]| Sshd::from_test(lines).unwrap().password_allowed();
        assert_eq!(test(&["passwordauthentication no", "kbdinteractiveauthentication yes", "usepam yes"]), Some(true));
        assert_eq!(test(&["passwordauthentication no", "kbdinteractiveauthentication yes", "usepam no"]), Some(false));
        assert_eq!(test(&["passwordauthentication yes", "usepam yes", "authenticationmethods publickey,password"]), Some(false));
        assert_eq!(test(&["passwordauthentication yes", "usepam yes", "authenticationmethods publickey password"]), Some(true));
        // Older servers print the old keyword alongside.
        assert_eq!(test(&["passwordauthentication no", "kbdinteractiveauthentication no", "challengeresponseauthentication yes", "usepam yes"]), Some(true));
    }

    #[test]
    fn falls_back_to_editing_sshd_config_when_a_drop_in_would_lose() {
        let text = DEBIAN_OPEN.replace("Include /etc/ssh/sshd_config.d/*.conf\nPermitRootLogin yes", "PermitRootLogin yes\nInclude /etc/ssh/sshd_config.d/*.conf");
        let c = parse(&text);
        assert_eq!(
            find(&c, "root-login").fix.as_deref(),
            Some("sudo sed -i -e '1i PermitRootLogin prohibit-password' /etc/ssh/sshd_config && sudo sshd -t && (sudo systemctl reload ssh || sudo systemctl reload sshd)")
        );
        assert!(find(&c, "password-login").fix.as_deref().unwrap().starts_with("printf"));
        let none = DEBIAN_OPEN.replace("Include /etc/ssh/sshd_config.d/*.conf\n", "");
        assert!(find(&parse(&none), "password-login").fix.as_deref().unwrap().starts_with("sudo sed -i -e '1i PasswordAuthentication no' -e '1i KbdInteractiveAuthentication no'"));
        let old = DEBIAN_OPEN.replace("OpenSSH_9.2p1", "OpenSSH_7.4p1");
        assert!(find(&parse(&old), "password-login").fix.as_deref().unwrap().contains("ChallengeResponseAuthentication no"));
    }

    #[test]
    fn root_with_a_key_only_is_a_warning() {
        let text = DEBIAN_OPEN.replace("passwordauthentication yes", "passwordauthentication no");
        let root = parse(&text);
        assert_eq!(find(&root, "root-login").verdict, Verdict::Warn);
        assert_eq!(find(&root, "password-login").verdict, Verdict::Good);
    }

    #[test]
    fn reads_busybox_netstat() {
        let lines = [
            "#netstat",
            "Active Internet connections (only servers)",
            "Proto Recv-Q Send-Q Local Address           Foreign Address         State       PID/Program name",
            "tcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN      2052/sshd",
            "tcp        0      0 0.0.0.0:6379            0.0.0.0:*               LISTEN      2101/redis-server",
            "tcp        0      0 :::22                   :::*                    LISTEN      2052/sshd",
            "udp        0      0 0.0.0.0:68              0.0.0.0:*                           1999/udhcpc",
            "udp        0      0 0.0.0.0:51820           0.0.0.0:*                           -",
        ];
        let open = public_ports(&listeners(&lines).unwrap());
        let labels: Vec<String> = open.iter().map(OpenPort::label).collect();
        assert_eq!(labels, ["22 (SSH)", "6379 (Redis)", "51820/udp (WireGuard)"]);
        assert_eq!(open[1].risk, Risk::Private);
    }

    #[test]
    fn spots_risky_programs_on_odd_ports_and_the_docker_api() {
        let lines = ["#ss", "tcp LISTEN 0 4096 0.0.0.0:15432 0.0.0.0:* users:((\"postgres\",pid=1,fd=5))", "tcp LISTEN 0 4096 *:2375 *:* users:((\"dockerd\",pid=2,fd=3))"];
        let open = public_ports(&listeners(&lines).unwrap());
        assert_eq!(open[0].named(), "Docker API (2375)");
        assert_eq!(open[0].risk, Risk::TakeOver);
        assert_eq!(open[1].named(), "PostgreSQL (15432)");
        assert_eq!(open[1].risk, Risk::Private);
        let text = UBUNTU_UFW.replace("tcp   LISTEN 0      244        127.0.0.1:5432", "tcp   LISTEN 0      244        0.0.0.0:2375");
        let c = parse(&text);
        let ports = find(&c, "open-ports");
        assert_eq!(ports.verdict, Verdict::Bad);
        assert!(ports.detail.starts_with("The Docker API (2375)"), "{}", ports.detail);
    }

    #[test]
    fn tells_public_addresses_from_local_ones() {
        for local in ["127.0.0.1", "127.0.0.53%lo", "[::1]", "::1", "[::ffff:127.0.0.1]", "[fe80::1%eth0]", "169.254.1.2", "localhost"] {
            assert!(is_local(local), "{local}");
        }
        for public in ["0.0.0.0", "*", "[::]", "::", "", "10.0.0.5", "203.0.113.9"] {
            assert!(!is_local(public), "{public}");
        }
    }

    #[test]
    fn reads_docker_port_mappings() {
        assert_eq!(published("0.0.0.0:8080->80/tcp, :::8080->80/tcp, 127.0.0.1:9000->9000/tcp, 5000/tcp"), [("8080".to_string(), "80".to_string())]);
        assert_eq!(published("[::]:53->53/udp"), [("53".to_string(), "53/udp".to_string())]);
        assert!(published("").is_empty());
    }

    #[test]
    fn docker_web_ports_alone_are_fine() {
        let text = DEBIAN_OPEN.replace("db 0.0.0.0:5432->5432/tcp, :::5432->5432/tcp\n", "");
        assert_eq!(find(&parse(&text), "docker-ports").verdict, Verdict::Good);
        let stopped = DEBIAN_OPEN.replace("#ok", "#error\nCannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?");
        assert_eq!(find(&parse(&stopped), "docker-ports").detail, "Docker is installed but not running.");
        let none = DEBIAN_OPEN.replace("#installed\n#ok", "");
        assert_eq!(find(&parse(&none), "docker-ports").detail, "Docker isn't installed.");
    }

    #[test]
    fn recognizes_firewall_rules() {
        assert!(nft_blocks(&["\tchain filter_INPUT {", "\t\ttype filter hook input priority filter + 10; policy accept;", "\t\tjump filter_INPUT_ZONES", "\t\treject with icmpx admin-prohibited", "\t}"]));
        assert!(nft_blocks(&["\t\tcounter packets 12 bytes 720 drop"]));
        assert!(!nft_blocks(&["\t\ttype filter hook input priority filter; policy accept;", "\t\tiifname { \"he-ipv6\" } drop"]));
        assert!(iptables_blocks(&["-P INPUT ACCEPT", "-A INPUT -j REJECT --reject-with icmp-host-prohibited"]));
        assert!(!iptables_blocks(&["-P INPUT ACCEPT", "-A INPUT -p tcp --dport 25 -j DROP"]));
    }

    #[test]
    fn fedora_that_only_downloads_updates() {
        let text = "@@root@@\n1\n@@os@@\nID=fedora\n@@tools@@\nsystemctl\ndnf\ndnf5\n@@dnf@@\ntimer dnf-automatic.timer \ntimer dnf-automatic-install.timer \ntimer dnf5-automatic.timer enabled\napply_updates = no\n@@reboot@@\nrestarting 1\nsay Reboot is required to fully utilize these updates.\n";
        let c = parse(text);
        let updates = find(&c, "auto-updates");
        assert_eq!(updates.verdict, Verdict::Warn);
        assert!(updates.detail.contains("never installed"));
        assert!(updates.fix.as_deref().unwrap().contains("dnf5-automatic.timer"));
        assert_eq!(find(&c, "reboot").verdict, Verdict::Warn);
        let on = text.replace("apply_updates = no", "apply_updates = yes");
        assert_eq!(find(&parse(&on), "auto-updates").verdict, Verdict::Good);
    }

    #[test]
    fn spots_a_new_kernel_on_arch() {
        let text = "@@os@@\nID=arch\n@@reboot@@\nkernel 6.10.5-arch1-1\nmodule 6.10.6-arch1-1\n";
        assert_eq!(find(&parse(text), "reboot").verdict, Verdict::Warn);
        let same = text.replace("6.10.6", "6.10.5");
        assert_eq!(find(&parse(&same), "reboot").verdict, Verdict::Good);
        assert_eq!(find(&parse(text), "auto-updates").verdict, Verdict::Unknown);
    }

    #[test]
    fn counts_security_fixes_from_an_apt_simulation() {
        let debian = parse(DEBIAN_OPEN);
        let sec = find(&debian, "security-updates");
        assert_eq!(sec.detail, "4 security fixes are waiting: openssh-client, openssh-server, libssl3 and the kernel. They fix CVE-2024-6387 and CVE-2024-5535.");
        assert_eq!(sec.fix.as_deref(), Some("sudo apt-get update && sudo apt-get upgrade --with-new-pkgs"));
        assert!(sec.fix_note.as_deref().unwrap().contains("restart"));

        let ubuntu = parse(UBUNTU_USER);
        let sec = find(&ubuntu, "security-updates");
        assert_eq!(sec.verdict, Verdict::Warn);
        assert_eq!(sec.detail, "3 security fixes are waiting: libxml2, vim-tiny and python3-urllib3.");
        assert_eq!(sec.fix_note.as_deref(), Some("Installs the fixes."));

        let tidy = parse(UBUNTU_UFW);
        assert_eq!(find(&tidy, "security-updates").detail, "No security fixes are waiting, going by package lists refreshed today.");
    }

    #[test]
    fn stale_or_missing_package_lists_are_called_out() {
        let old = parse(&DEBIAN_OPEN.replace("lists 1792900000", "lists 1789544000"));
        let sec = find(&old, "security-updates");
        assert_eq!(sec.verdict, Verdict::Bad);
        assert!(sec.detail.ends_with(" The package lists were last refreshed 40 days ago, so the real number may be higher."), "{}", sec.detail);
        assert!(sec.fix.as_deref().unwrap().starts_with("sudo apt-get update && "));

        let quiet = parse(&UBUNTU_UFW.replace("lists 1792990000", "lists 1791000000"));
        let sec = find(&quiet, "security-updates");
        assert_eq!(sec.verdict, Verdict::Warn);
        assert!(sec.detail.contains("refreshed 23 days ago, so newer fixes can't be seen"), "{}", sec.detail);

        let none = parse(&UBUNTU_UFW.replace("lists 1792990000\n", ""));
        let sec = find(&none, "security-updates");
        assert_eq!(sec.verdict, Verdict::Unknown);
        assert!(sec.fix.is_some());

        let broken = parse(&UBUNTU_UFW.replace("sim 0", "sim 100\nE: Unmet dependencies. Try 'apt --fix-broken install' with no packages (or specify a solution)."));
        let sec = find(&broken, "security-updates");
        assert_eq!(sec.verdict, Verdict::Unknown);
        assert!(sec.detail.contains("It said: Unmet dependencies."), "{}", sec.detail);
    }

    const ROCKY: &str = r#"@@root@@
1
@@os@@
ID="rocky"
ID_LIKE="rhel centos fedora"
@@tools@@
systemctl
dnf
@@secupd@@
#dnf
now 1793000000
lists 1792990000
list 0
adv RLSA-2024:4312 Important/Sec. openssh-8.7p1-38.el9_4.1.x86_64
adv RLSA-2024:4312 Important/Sec. openssh-server-8.7p1-38.el9_4.1.x86_64
adv RLSA-2024:5101 Moderate/Sec.  python3-setuptools-53.0.0-12.el9_4.1.noarch
cve CVE-2024-6387
cve CVE-2024-6387
cve CVE-2024-6345
"#;

    #[test]
    fn reads_dnf_security_advisories() {
        let c = parse(ROCKY);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Bad);
        assert_eq!(sec.detail, "3 security fixes are waiting: openssh, openssh-server and python3-setuptools. They fix CVE-2024-6387 and CVE-2024-6345.");
        assert_eq!(sec.fix.as_deref(), Some("sudo dnf upgrade --security"));

        let moderate = "@@os@@\nID=fedora\n@@tools@@\ndnf\n@@secupd@@\n#dnf\nlist 0\nadv FEDORA-2024-1a2b3c Moderate/Sec. python3-setuptools-69.0.3-4.fc40.noarch\n";
        let c = parse(moderate);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Warn);
        assert_eq!(sec.detail, "A security fix is waiting for python3-setuptools.");

        // dnf5 prints columns, and the severity alone makes it serious.
        let dnf5 = "@@os@@\nID=fedora\n@@secupd@@\n#dnf\nlist 0\nadv Name               Type     Severity  Package                         Issued\nadv FEDORA-2024-9f8e  security Important libxml2-2.12.8-1.fc41.x86_64 2024-07-01 10:00:00\n";
        let c = parse(dnf5);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Bad);
        assert_eq!(sec.detail, "A security fix is waiting for libxml2.");

        let user = ROCKY.replace("@@root@@\n1", "@@root@@\n0").replace("list 0", "list 1\nadv Error: Cache-only enabled but no cache for 'baseos'");
        let c = parse(&user);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Unknown);
        assert!(sec.detail.contains("only root can use"), "{}", sec.detail);
        let empty = ROCKY.replace("list 0", "list 1\nadv Error: Cache-only enabled but no cache for 'baseos'");
        assert!(find(&parse(&empty), "security-updates").detail.contains("dnf makecache"));
    }

    #[test]
    fn reads_arch_audit_and_gives_up_elsewhere() {
        let arch = "@@os@@\nID=arch\n@@secupd@@\n#arch\naudit 0\naa Package openssl is affected by CVE-2023-0464, CVE-2023-0465. Medium risk! Update to 3.0.8-2!\naa Package libxml2 is affected by CVE-2024-1111. Low risk! Update to 2.12.5-1!\n";
        let c = parse(arch);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Bad);
        assert_eq!(sec.detail, "2 security fixes are waiting: openssl and libxml2. They fix CVE-2023-0464, CVE-2023-0465 and CVE-2024-1111.");
        assert_eq!(sec.fix.as_deref(), Some("sudo pacman -Syu"));
        let low = arch.replace("aa Package openssl is affected by CVE-2023-0464, CVE-2023-0465. Medium risk! Update to 3.0.8-2!\n", "");
        assert_eq!(find(&parse(&low), "security-updates").verdict, Verdict::Warn);
        let high = low.replace("Low risk", "High risk");
        assert_eq!(find(&parse(&high), "security-updates").verdict, Verdict::Bad);

        let bare = "@@os@@\nID=arch\n@@secupd@@\n#pacman\n";
        assert!(find(&parse(bare), "security-updates").detail.contains("arch-audit"));
        let alpine = "@@os@@\nID=alpine\n@@secupd@@\n";
        let c = parse(alpine);
        let sec = find(&c, "security-updates");
        assert_eq!(sec.verdict, Verdict::Unknown);
        assert!(sec.detail.contains("doesn't mark"));
    }

    #[test]
    fn matches_include_globs() {
        assert!(glob_match(b"/etc/ssh/sshd_config.d/*.conf", b"/etc/ssh/sshd_config.d/00-ssheesh.conf"));
        assert!(!glob_match(b"/etc/ssh/sshd_config.d/*.conf", b"/etc/ssh/sshd_config.d/x/y.conf"));
        assert!(glob_match(b"/etc/ssh/a?.conf", b"/etc/ssh/ab.conf"));
        assert!(!glob_match(b"/etc/ssh/*.conf", b"/etc/ssh/sshd_config"));
    }
}
