//! A release's files, their names, and the two manifests written from them.
//!
//! | File | Who takes it |
//! |---|---|
//! | `SSHeesh_<v>_x64-setup.exe` | 0.10 installed on Windows (`windows-x86_64-nsis`, `windows-x86_64`): its updater runs it with `/P /UPDATE /R /ARGS`; people downloading it |
//! | `SSHeesh.exe` | 0.10 portable on Windows (`windows-x86_64-portable`): it swaps it in for itself; people wanting one file |
//! | `ssheesh-<v>-windows-x64.zip` (`ssheesh.exe`) | 1.x on Windows (`update-v1.json`, `windows-x64`) |
//! | `SSHeesh_<v>_universal.app.tar.gz` (`SSHeesh.app/`) | 0.10 on a Mac (`darwin-*-app`, `darwin-*`), 1.x on a Mac (`macos-arm64`, `macos-x64`) |
//! | `SSHeesh_<v>_universal.dmg` | people downloading for a Mac |
//! | `ssheesh-<v>-linux-x64` (the program) | 0.10's AppImage on Linux (`linux-x86_64-appimage`, `linux-x86_64`): its updater writes any ELF over the AppImage |
//! | `ssheesh-<v>-linux-x64.tar.gz` (`ssheesh`) | 1.x on Linux (`linux-x64`); people downloading |
//!
//! Each has its signature beside it as `<file>.sig` (`sign`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// A version as releases are tagged: semver, no leading "v", no build metadata.
pub fn version_ok(v: &str) -> Result<semver::Version, String> {
    let parsed = semver::Version::parse(v).map_err(|e| format!("{v} isn't a version: {e}"))?;
    if !parsed.build.is_empty() || v.len() > 64 {
        return Err(format!("{v}: no build metadata in a release's version"));
    }
    Ok(parsed)
}

/// The systems a release moves 0.10 users of (`--platforms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum System {
    Windows,
    /// Apple silicon.
    Mac,
    MacIntel,
    Linux,
}

impl System {
    pub fn all() -> Vec<System> {
        vec![System::Windows, System::Mac, System::MacIntel, System::Linux]
    }

    pub fn parse_list(list: &str) -> Result<Vec<System>, String> {
        let mut out = Vec::new();
        for word in list.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            let s = match word {
                "windows" => System::Windows,
                "macos" => System::Mac,
                "macos-intel" => System::MacIntel,
                "linux" => System::Linux,
                "none" => continue,
                other => return Err(format!("unknown system {other}: windows, macos, macos-intel, linux or none")),
            };
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out.sort();
        Ok(out)
    }
}

/// The file names of version `v`.
pub struct Names {
    pub installer: String,
    pub portable: String,
    pub windows_zip: String,
    pub mac_app: String,
    pub linux_program: String,
    pub linux_tar: String,
}

impl Names {
    pub fn of(v: &str) -> Names {
        Names {
            installer: format!("SSHeesh_{v}_x64-setup.exe"),
            portable: "SSHeesh.exe".into(),
            windows_zip: format!("ssheesh-{v}-windows-x64.zip"),
            mac_app: format!("SSHeesh_{v}_universal.app.tar.gz"),
            linux_program: format!("ssheesh-{v}-linux-x64"),
            linux_tar: format!("ssheesh-{v}-linux-x64.tar.gz"),
        }
    }
}

/// A release being made: its files in `dir`.
pub struct Release {
    pub version: String,
    pub repo: String,
    pub tag: String,
    pub dir: PathBuf,
    /// The systems whose 0.10 users `latest.json` moves.
    pub platforms: Vec<System>,
    pub names: Names,
}

impl Release {
    pub fn of(version: &str, repo: &str, tag: &str, dir: &Path, platforms: Option<&str>) -> Result<Release, String> {
        version_ok(version)?;
        let plain = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        match repo.split_once('/') {
            Some((owner, name)) if plain(owner) && plain(name) => {}
            _ => return Err(format!("{repo} isn't OWNER/NAME")),
        }
        if !plain(tag) {
            return Err(format!("{tag} isn't a plain tag"));
        }
        let platforms = match platforms {
            Some(list) => System::parse_list(list)?,
            None => System::all(),
        };
        Ok(Release { version: version.into(), repo: repo.into(), tag: tag.into(), dir: dir.into(), platforms, names: Names::of(version) })
    }

    /// A file's address: an ordinary download link, never GitHub's API (whose rate limit the
    /// shared exits of Tor often use up, plan §6.1).
    pub fn url(&self, file: &str) -> String {
        format!("https://github.com/{}/releases/download/{}/{file}", self.repo, self.tag)
    }

    pub fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    /// `<file>.sig`'s text, as `latest.json` and `update-v1.json` carry it.
    pub fn signature(&self, file: &str) -> Result<String, String> {
        let at = self.dir.join(format!("{file}.sig"));
        let text = std::fs::read_to_string(&at).map_err(|e| format!("{}: {e}", at.display()))?;
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err(format!("{} is empty", at.display()));
        }
        Ok(text)
    }

    /// The `latest.json` entries for 0.10: `(key, file)`, in the order written.
    pub fn latest_entries(&self) -> Vec<(&'static str, &str)> {
        let n = &self.names;
        let mut out = Vec::new();
        for s in &self.platforms {
            match s {
                System::Windows => out.extend([("windows-x86_64-nsis", n.installer.as_str()), ("windows-x86_64", n.installer.as_str()), ("windows-x86_64-portable", n.portable.as_str())]),
                System::Mac => out.extend([("darwin-aarch64-app", n.mac_app.as_str()), ("darwin-aarch64", n.mac_app.as_str())]),
                System::MacIntel => out.extend([("darwin-x86_64-app", n.mac_app.as_str()), ("darwin-x86_64", n.mac_app.as_str())]),
                System::Linux => out.extend([("linux-x86_64-appimage", n.linux_program.as_str()), ("linux-x86_64", n.linux_program.as_str())]),
            }
        }
        out
    }

    /// `latest.json` as Tauri's updater reads it: `version`, `notes` (text), `pub_date`
    /// (RFC 3339; a wrong one breaks the whole file for every 0.10), and per entry its
    /// `signature` and `url`.
    pub fn latest_json(&self, notes: &str) -> Result<String, String> {
        let mut platforms = serde_json::Map::new();
        for (key, file) in self.latest_entries() {
            platforms.insert(key.into(), json!({ "signature": self.signature(file)?, "url": self.url(file) }));
        }
        let v = json!({ "version": self.version, "notes": notes, "pub_date": now_rfc3339(), "platforms": platforms });
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
    }

    /// `update-v1.json`'s entries: `(key, file)`.
    pub fn v1_entries(&self) -> Vec<(&'static str, &str)> {
        let n = &self.names;
        vec![("linux-x64", n.linux_tar.as_str()), ("macos-arm64", n.mac_app.as_str()), ("macos-x64", n.mac_app.as_str()), ("windows-x64", n.windows_zip.as_str())]
    }

    /// `update-v1.json` (`desktop/app/src/update/release.rs`): every system 1.x runs on, with
    /// each file's size and SHA-256; `pulled` carried over from the channel's last one.
    pub fn update_v1_json(&self, pulled: &[String]) -> Result<String, String> {
        let mut platforms = serde_json::Map::new();
        for (key, file) in self.v1_entries() {
            let bytes = std::fs::read(self.path(file)).map_err(|e| format!("{file}: {e}"))?;
            platforms.insert(key.into(), json!({ "url": self.url(file), "size": bytes.len(), "sha256": sha256_hex(&bytes), "signature": self.signature(file)? }));
        }
        let v = json!({ "schema": 1, "version": self.version, "pub_date": now_rfc3339(), "pulled": pulled, "platforms": platforms });
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
    }
}

/// The withdrawn versions of an `update-v1.json` (the channel's last one), checked to read.
pub fn pulled_of(bytes: &[u8]) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("the last update-v1.json doesn't read: {e}"))?;
    match v.get("pulled") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(list)) => list.iter().map(|p| p.as_str().filter(|s| version_ok(s).is_ok()).map(str::to_owned).ok_or_else(|| format!("pulled: {p} isn't a version"))).collect(),
        Some(_) => Err("pulled isn't a list".into()),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Now as `2026-11-19T10:00:00Z`.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
    rfc3339(secs)
}

/// Seconds since 1970 as RFC 3339 in UTC (days to a civil date: Howard Hinnant's algorithm).
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_794_688_787), "2026-11-14T20:39:47Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn systems_and_versions() {
        assert_eq!(System::parse_list("linux, windows,macos").unwrap(), [System::Windows, System::Mac, System::Linux]);
        assert_eq!(System::parse_list("none").unwrap(), []);
        assert!(System::parse_list("bsd").is_err());
        assert!(version_ok("1.0.0").is_ok() && version_ok("1.0.1-rc.1").is_ok());
        assert!(version_ok("v1.0.0").is_err() && version_ok("1.0.0+abc").is_err());
        assert_eq!(pulled_of(br#"{"pulled":["1.0.0"]}"#).unwrap(), ["1.0.0"]);
        assert!(pulled_of(br#"{"pulled":["x"]}"#).is_err());
        assert!(Release::of("1.0.0", "a/b/c", "v1.0.0", Path::new("."), None).is_err());
        assert!(Release::of("1.0.0", "o/n", "v1.0.0/../x", Path::new("."), None).is_err());
    }

    #[test]
    fn each_kind_of_0_10_finds_its_entry() {
        let r = Release::of("1.0.0", "footprintsntrails/ssheesh", "v1.0.0", Path::new("."), None).unwrap();
        let keys: Vec<_> = r.latest_entries().into_iter().map(|(k, _)| k).collect();
        for want in ["windows-x86_64-nsis", "windows-x86_64", "windows-x86_64-portable", "darwin-aarch64-app", "darwin-x86_64-app", "linux-x86_64-appimage"] {
            assert!(keys.contains(&want), "{want}");
        }
        assert_eq!(r.url("SSHeesh.exe"), "https://github.com/footprintsntrails/ssheesh/releases/download/v1.0.0/SSHeesh.exe");
        // Only the systems asked for.
        let r = Release::of("1.0.0", "o/n", "v1.0.0", Path::new("."), Some("windows")).unwrap();
        assert!(r.latest_entries().iter().all(|(k, _)| k.starts_with("windows-")));
    }
}
