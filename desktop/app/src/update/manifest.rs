//! `canary.json`: what the canary workflow published last. One per pre-release, next to the
//! archives:
//!
//! ```json
//! { "version": "1.0.0-canary.124", "sha": "4edfea3", "pub_date": "2026-11-14T10:00:00Z",
//!   "platforms": { "linux-x64": { "url": "https://github.com/…/ssheesh-1.0.0-canary-linux-x64.tar.gz",
//!                                 "size": 31457280, "sha256": "…", "signature": "…" } } }
//! ```
//!
//! `signature` is the archive's minisign signature as `tauri signer sign` writes it (the
//! `.sig` file: base64 of the signature box), empty when the workflow had no key. It signs the
//! archive under the name [`Platform::signed_name`] gives, which carries the version: a
//! signature can't be moved onto a manifest that names another build.

use std::collections::BTreeMap;

use serde_json::Value;

/// The only place archives are downloaded from (GitHub sends the download on to its own
/// storage, `http::allowed_host`).
pub const DOWNLOADS: &str = "https://github.com/footprintsntrails/ssheesh/releases/download/";

/// The biggest archive taken: a test build is tens of megabytes.
pub const MOST_BYTES: u64 = 400 * 1024 * 1024;

/// A system the canary workflow builds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    LinuxX64,
    MacArm64,
    WindowsX64,
}

/// How an archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    TarGz,
    Zip,
}

impl Platform {
    /// The one this program was built for; `None` where the workflow doesn't build.
    pub fn current() -> Option<Platform> {
        Platform::of(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn of(os: &str, arch: &str) -> Option<Platform> {
        match (os, arch) {
            ("linux", "x86_64") => Some(Platform::LinuxX64),
            ("macos", "aarch64") => Some(Platform::MacArm64),
            ("windows", "x86_64") => Some(Platform::WindowsX64),
            _ => None,
        }
    }

    /// Its key in `platforms`, and the end of its archive's name.
    pub fn key(self) -> &'static str {
        match self {
            Platform::LinuxX64 => "linux-x64",
            Platform::MacArm64 => "macos-arm64",
            Platform::WindowsX64 => "windows-x64",
        }
    }

    pub fn packing(self) -> Packing {
        match self {
            Platform::WindowsX64 => Packing::Zip,
            _ => Packing::TarGz,
        }
    }

    fn extension(self) -> &'static str {
        match self.packing() {
            Packing::Zip => "zip",
            Packing::TarGz => "tar.gz",
        }
    }

    /// The program's name inside the archive.
    pub fn program(self) -> &'static str {
        match self {
            Platform::WindowsX64 => "ssheesh-preview.exe",
            _ => "ssheesh-preview",
        }
    }

    /// The name the archive of `version` was signed under: "ssheesh-1.0.0-canary.124-linux-x64.tar.gz".
    /// The signature's trusted comment says it, so it's checked with the signature.
    pub fn signed_name(self, version: &str) -> String {
        format!("ssheesh-{version}-{}.{}", self.key(), self.extension())
    }
}

/// One system's archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
    /// The `.sig` file's text; `None` when the build wasn't signed.
    pub signature: Option<String>,
}

/// What was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// "1.0.0-canary.124".
    pub version: String,
    /// The run number in `version`: what builds are compared by.
    pub run: u64,
    pub sha: String,
    pub pub_date: String,
    pub platforms: BTreeMap<String, Asset>,
}

/// The run number in a canary version: "1.0.0-canary.124" → 124.
pub fn run_of(version: &str) -> Option<u64> {
    let (base, run) = version.rsplit_once("-canary.")?;
    let base_ok = !base.is_empty() && base.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let digits_ok = !run.is_empty() && run.len() <= 12 && run.bytes().all(|b| b.is_ascii_digit());
    (base_ok && digits_ok).then(|| run.parse().ok()).flatten().filter(|r| *r > 0)
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key).and_then(Value::as_str).ok_or_else(|| format!("no {key}"))
}

fn asset(v: &Value) -> Result<Asset, String> {
    let url = text(v, "url")?;
    if !url.starts_with(DOWNLOADS) || url.len() > 512 || url.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) {
        return Err("a download from somewhere else".into());
    }
    let size = v.get("size").and_then(Value::as_u64).filter(|s| *s > 0 && *s <= MOST_BYTES).ok_or("no size, or too big")?;
    let sha256 = text(v, "sha256")?.to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("a sha256 that isn't one".into());
    }
    let signature = match v.get("signature") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) if s.len() <= 4096 => Some(s.trim().to_owned()),
        Some(_) => return Err("a signature that isn't one".into()),
    };
    Ok(Asset { url: url.to_owned(), size, sha256, signature })
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("canary.json doesn't read: {e}"))?;
        let version = text(&v, "version")?.to_owned();
        let run = run_of(&version).ok_or_else(|| format!("{version} isn't a test build's version"))?;
        let sha = v.get("sha").and_then(Value::as_str).unwrap_or_default().to_owned();
        let pub_date = v.get("pub_date").and_then(Value::as_str).unwrap_or_default().to_owned();
        let mut platforms = BTreeMap::new();
        if let Some(all) = v.get("platforms").and_then(Value::as_object) {
            for (key, entry) in all {
                // One system's bad entry doesn't stop the others.
                if let Ok(a) = asset(entry) {
                    platforms.insert(key.clone(), a);
                }
            }
        }
        Ok(Manifest { version, run, sha, pub_date, platforms })
    }

    /// This system's archive.
    pub fn asset(&self, platform: Platform) -> Option<&Asset> {
        self.platforms.get(platform.key())
    }
}

/// What a check found for a build of run `ours`: `Some` when the manifest names a newer build
/// for `platform` that didn't fail to start here before (`failed`).
pub fn offer<'a>(manifest: &'a Manifest, ours: u64, platform: Platform, failed: &[u64]) -> Option<&'a Asset> {
    (manifest.run > ours && !failed.contains(&manifest.run)).then(|| manifest.asset(platform)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn sample(version: &str, url: &str) -> String {
        format!(
            r#"{{"version":"{version}","sha":"4edfea3","pub_date":"2026-11-14T10:00:00Z","platforms":{{
              "linux-x64":{{"url":"{url}","size":1234,"sha256":"{HASH}","signature":"c2ln"}},
              "macos-arm64":{{"url":"{DOWNLOADS}canary/ssheesh-1.0.0-canary-macos-arm64.tar.gz","size":1234,"sha256":"{}","signature":""}},
              "windows-x64":{{"url":"{DOWNLOADS}canary/ssheesh-1.0.0-canary-windows-x64.zip","size":0,"sha256":"{HASH}"}}}}}}"#,
            HASH.to_uppercase()
        )
    }

    #[test]
    fn the_manifest_reads() {
        let m = Manifest::parse(sample("1.0.0-canary.124", &format!("{DOWNLOADS}canary/ssheesh-1.0.0-canary-linux-x64.tar.gz")).as_bytes()).unwrap();
        assert_eq!((m.run, m.sha.as_str(), m.pub_date.as_str()), (124, "4edfea3", "2026-11-14T10:00:00Z"));
        let linux = m.asset(Platform::LinuxX64).unwrap();
        assert_eq!((linux.size, linux.sha256.as_str(), linux.signature.as_deref()), (1234, HASH, Some("c2ln")));
        // Unsigned: an empty signature is none. Its hash is read in lowercase.
        let mac = m.asset(Platform::MacArm64).unwrap();
        assert_eq!((mac.signature.as_ref(), mac.sha256.as_str()), (None, HASH));
        // A size of nothing is a broken entry: left out, the others stay.
        assert!(m.asset(Platform::WindowsX64).is_none());
    }

    #[test]
    fn what_isnt_a_test_builds_manifest_is_refused() {
        for bad in ["", "{", r#"{"sha":"x"}"#, r#"{"version":"0.10.14"}"#, r#"{"version":"1.0.0-canary"}"#, r#"{"version":"1.0.0-canary.x"}"#, r#"{"version":"1.0.0-canary.0"}"#] {
            assert!(Manifest::parse(bad.as_bytes()).is_err(), "{bad}");
        }
        // A download from anywhere but this repository's releases is left out.
        let elsewhere = Manifest::parse(sample("1.0.0-canary.124", "https://example.invalid/ssheesh.tar.gz").as_bytes()).unwrap();
        assert!(elsewhere.asset(Platform::LinuxX64).is_none());
        let plain = Manifest::parse(sample("1.0.0-canary.124", "http://github.com/footprintsntrails/ssheesh/releases/download/canary/a").as_bytes()).unwrap();
        assert!(plain.asset(Platform::LinuxX64).is_none());
    }

    #[test]
    fn versions_compare_by_their_run() {
        assert_eq!(run_of("1.0.0-canary.124"), Some(124));
        assert_eq!(run_of("1.0.1-canary.7"), Some(7));
        for bad in ["1.0.0-canary.", "1.0.0-canary.-1", "1.0.0-canary.1.2", "-canary.5", "1..0-canary.5", "1.0.0-canary.99999999999999"] {
            assert_eq!(run_of(bad), None, "{bad}");
        }
        let m = Manifest::parse(sample("1.0.0-canary.124", &format!("{DOWNLOADS}canary/ssheesh-1.0.0-canary-linux-x64.tar.gz")).as_bytes()).unwrap();
        assert!(offer(&m, 123, Platform::LinuxX64, &[]).is_some());
        // The same build, an older one, one that failed to start here, a system with no entry.
        assert!(offer(&m, 124, Platform::LinuxX64, &[]).is_none());
        assert!(offer(&m, 130, Platform::LinuxX64, &[]).is_none());
        assert!(offer(&m, 123, Platform::LinuxX64, &[124]).is_none());
        assert!(offer(&m, 123, Platform::WindowsX64, &[]).is_none());
    }

    #[test]
    fn each_system_has_its_archive() {
        assert_eq!(Platform::of("linux", "x86_64"), Some(Platform::LinuxX64));
        assert_eq!(Platform::of("macos", "aarch64"), Some(Platform::MacArm64));
        assert_eq!(Platform::of("windows", "x86_64"), Some(Platform::WindowsX64));
        assert_eq!(Platform::of("macos", "x86_64"), None);
        assert_eq!(Platform::LinuxX64.signed_name("1.0.0-canary.124"), "ssheesh-1.0.0-canary.124-linux-x64.tar.gz");
        assert_eq!(Platform::WindowsX64.signed_name("1.0.0-canary.124"), "ssheesh-1.0.0-canary.124-windows-x64.zip");
        assert_eq!(Platform::WindowsX64.program(), "ssheesh-preview.exe");
    }
}
