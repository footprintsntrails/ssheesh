//! The server list: hosts from ~/.ssh/config plus ones added in the app.

use crate::ssh::SshTarget;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostSource {
    SshConfig,
    Saved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub source: HostSource,
}

impl Host {
    pub fn target(&self) -> SshTarget {
        match self.source {
            // Let ssh resolve everything from the config entry itself.
            HostSource::SshConfig => SshTarget { destination: self.name.clone(), ..Default::default() },
            HostSource::Saved => SshTarget {
                destination: self.hostname.clone(),
                user: self.user.clone(),
                port: self.port,
                identity_file: self.identity_file.clone(),
                extra_args: Vec::new(),
            },
        }
    }
}

/// What the "Add server" form sends.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewHost {
    pub name: Option<String>,
    /// `user@host:port`, `host:port` or just `host`.
    pub address: String,
    pub identity_file: Option<String>,
}

pub struct HostStore {
    file: PathBuf,
    ssh_config: Option<PathBuf>,
    cache: Vec<Host>,
}

impl HostStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            file: data_dir.join("hosts.json"),
            ssh_config: dirs::home_dir().map(|h| h.join(".ssh").join("config")),
            cache: Vec::new(),
        }
    }

    pub fn with_ssh_config(mut self, path: Option<PathBuf>) -> Self {
        self.ssh_config = path;
        self
    }

    pub fn list(&mut self) -> Vec<Host> {
        let mut hosts = self.ssh_config.as_deref().map(parse_ssh_config_file).unwrap_or_default();
        let saved = self.load_saved();
        hosts.retain(|h| !saved.iter().any(|s| s.name == h.name));
        hosts.extend(saved);
        self.cache = hosts.clone();
        hosts
    }

    pub fn get(&mut self, id: &str) -> Option<Host> {
        if let Some(h) = self.cache.iter().find(|h| h.id == id) {
            return Some(h.clone());
        }
        self.list().into_iter().find(|h| h.id == id)
    }

    pub fn add(&mut self, input: NewHost) -> Result<Host, String> {
        let (user, hostname, port) = parse_address(&input.address)?;
        let name = input
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| hostname.clone());
        let mut saved = self.load_saved();
        let host = Host {
            id: format!("saved:{}", unique_id()),
            name,
            hostname,
            user,
            port,
            identity_file: input.identity_file.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            source: HostSource::Saved,
        };
        saved.push(host.clone());
        self.write_saved(&saved)?;
        Ok(host)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let mut saved = self.load_saved();
        saved.retain(|h| h.id != id);
        self.write_saved(&saved)
    }

    fn load_saved(&self) -> Vec<Host> {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write_saved(&self, hosts: &[Host]) -> Result<(), String> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(hosts).map_err(|e| e.to_string())?;
        std::fs::write(&self.file, json).map_err(|e| format!("Couldn't save the server list: {e}"))
    }
}

fn unique_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", n)
}

pub fn parse_address(input: &str) -> Result<(Option<String>, String, Option<u16>), String> {
    let s = input.trim().trim_start_matches("ssh://");
    if s.is_empty() {
        return Err("Enter a server address, like root@203.0.113.10".into());
    }
    let (user, rest) = match s.rsplit_once('@') {
        Some((u, r)) => (Some(u.to_string()).filter(|u| !u.is_empty()), r),
        None => (None, s),
    };
    // [v6::addr]:port or host:port. A bare IPv6 address has several colons.
    let (host, port) = if let Some(r) = rest.strip_prefix('[') {
        let (h, tail) = r.split_once(']').ok_or("Missing ] in the address")?;
        (h.to_string(), tail.strip_prefix(':'))
    } else if rest.matches(':').count() == 1 {
        let (h, p) = rest.split_once(':').unwrap_or((rest, ""));
        (h.to_string(), Some(p))
    } else {
        (rest.to_string(), None)
    };
    let port = match port {
        Some(p) if !p.is_empty() => Some(p.parse::<u16>().map_err(|_| format!("\"{p}\" isn't a valid port"))?),
        _ => None,
    };
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err("That doesn't look like a server address".into());
    }
    Ok((user, host, port))
}

pub fn parse_ssh_config_file(path: &Path) -> Vec<Host> {
    let mut out = Vec::new();
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    parse_into(path, &base, &mut out, 0);
    let mut seen = std::collections::HashSet::new();
    out.retain(|h| seen.insert(h.name.clone()));
    out
}

fn parse_into(path: &Path, base: &Path, out: &mut Vec<Host>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    parse_config_text(&text, base, out, depth);
}

fn parse_config_text(text: &str, base: &Path, out: &mut Vec<Host>, depth: u8) {
    // Indexes into `out` that the current `Host` block applies to.
    let mut current: Vec<usize> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = split_kv(line);
        let value = value.trim_matches('"');
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for alias in value.split_whitespace() {
                    if alias.contains(['*', '?', '!']) {
                        continue;
                    }
                    out.push(Host {
                        id: format!("cfg:{alias}"),
                        name: alias.to_string(),
                        hostname: alias.to_string(),
                        user: None,
                        port: None,
                        identity_file: None,
                        source: HostSource::SshConfig,
                    });
                    current.push(out.len() - 1);
                }
            }
            "match" => current.clear(),
            "include" => {
                for pattern in value.split_whitespace() {
                    for file in expand_include(pattern, base) {
                        parse_into(&file, base, out, depth + 1);
                    }
                }
            }
            k => {
                for &i in &current {
                    let h = &mut out[i];
                    // First value wins, like ssh itself.
                    match k {
                        "hostname" if h.hostname == h.name => h.hostname = value.to_string(),
                        "user" if h.user.is_none() => h.user = Some(value.to_string()),
                        "port" if h.port.is_none() => h.port = value.parse().ok(),
                        "identityfile" if h.identity_file.is_none() => h.identity_file = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }
}

fn split_kv(line: &str) -> (&str, &str) {
    let idx = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
    let key = &line[..idx];
    let value = line[idx..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    (key, value.trim())
}

fn expand_include(pattern: &str, base: &Path) -> Vec<PathBuf> {
    let expanded = crate::ssh::expand_tilde(pattern);
    let path = if Path::new(&expanded).is_absolute() { PathBuf::from(expanded) } else { base.join(expanded) };
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else { return vec![] };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| wildcard_match(&name, &e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_and_skips_patterns() {
        let cfg = "Host *\n  ServerAliveInterval 30\n\nHost prod api\n  HostName 203.0.113.10\n  User deploy\n  Port 2222\n\nHost pi\n  HostName=raspberrypi.local\n";
        let mut out = Vec::new();
        parse_config_text(cfg, Path::new("/tmp"), &mut out, 0);
        let names: Vec<_> = out.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["prod", "api", "pi"]);
        assert_eq!(out[0].hostname, "203.0.113.10");
        assert_eq!(out[1].user.as_deref(), Some("deploy"));
        assert_eq!(out[0].port, Some(2222));
        assert_eq!(out[2].hostname, "raspberrypi.local");
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(parse_address("root@1.2.3.4:2222").unwrap(), (Some("root".into()), "1.2.3.4".into(), Some(2222)));
        assert_eq!(parse_address("example.com").unwrap(), (None, "example.com".into(), None));
        assert_eq!(parse_address("me@[2001:db8::1]:22").unwrap(), (Some("me".into()), "2001:db8::1".into(), Some(22)));
        assert_eq!(parse_address("2001:db8::1").unwrap(), (None, "2001:db8::1".into(), None));
        assert!(parse_address("host:abc").is_err());
    }

    #[test]
    fn wildcard() {
        assert!(wildcard_match("*.conf", "a.conf"));
        assert!(!wildcard_match("*.conf", "a.txt"));
    }
}
