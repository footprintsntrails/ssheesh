//! `update-v1.json`: what 1.x reads to update itself (plan §6.7.1, §6.8; `shipping.md` §6.2).
//! It lives in a release of its own per channel (`channel.rs`), next to its minisign signature
//! `update-v1.json.minisig`, both by the release key:
//!
//! ```json
//! { "schema": 1, "version": "1.0.1", "pub_date": "2026-12-01T10:00:00Z", "pulled": ["1.0.0"],
//!   "platforms": { "linux-x64": { "url": "https://github.com/…/releases/download/v1.0.1/ssheesh-1.0.1-linux-x64.tar.gz",
//!                                 "size": 31457280, "sha256": "…", "signature": "…" } } }
//! ```
//!
//! - The manifest's signature carries `file:update-v1.json` and `version:<version>` in its
//!   trusted comment: an older manifest can't be passed off as this one, nor any other file.
//! - Each archive's signature carries `file:<the file's name in its url>` and
//!   `version:<version>`.
//! - `pulled`: withdrawn versions. A copy on one of them is offered this version even when
//!   it's older (the only way down); nobody is offered a pulled version.
//! - Archives are packed as test builds are (`manifest::Platform`), and come only from this
//!   repository's releases (`manifest::is_release_file`).
//!
//! Versions compare by semver precedence: 1.0.0-canary.124 < 1.0.0-rc.1 < 1.0.0 < 1.0.1.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use semver::Version;
use serde_json::Value;
use sshee_app::canary::Channel;

use super::channel::RELEASE_MANIFEST;
use super::manifest::{asset, Asset, Platform};
use super::verify;

/// The schema this copy reads; a manifest that says another is refused.
const SCHEMA: u64 = 1;

/// What was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The version as the manifest writes it (and its signatures say it).
    pub text: String,
    pub pub_date: String,
    /// Withdrawn versions.
    pub pulled: Vec<Version>,
    pub platforms: BTreeMap<String, Asset>,
}

/// A version as manifests and builds write it: semver, without a leading "v".
pub fn version(text: &str) -> Option<Version> {
    Version::parse(text.trim()).ok().filter(|_| text.len() <= 64)
}

/// Whether `a` is a later version than `b`. Build metadata ("+abc") doesn't count; anything
/// that isn't a version is never later.
pub fn newer(a: &str, b: &str) -> bool {
    match (version(a), version(b)) {
        (Some(a), Some(b)) => a.cmp_precedence(&b) == Ordering::Greater,
        _ => false,
    }
}

impl Release {
    /// Reads a manifest. Doesn't check its signature: [`Release::verified`] does both.
    pub fn parse(bytes: &[u8]) -> Result<Release, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("update-v1.json doesn't read: {e}"))?;
        match v.get("schema") {
            None => {}
            Some(s) if s.as_u64() == Some(SCHEMA) => {}
            Some(other) => return Err(format!("update-v1.json is of schema {other}, this copy reads {SCHEMA}")),
        }
        let text = v.get("version").and_then(Value::as_str).ok_or("no version")?.to_owned();
        let version = version(&text).filter(|_| text == text.trim()).ok_or_else(|| format!("{text} isn't a version"))?;
        let pub_date = v.get("pub_date").and_then(Value::as_str).unwrap_or_default().to_owned();
        let pulled = match v.get("pulled") {
            None | Some(Value::Null) => Vec::new(),
            // A list that doesn't read is refused whole: a withdrawn version mustn't be missed.
            Some(Value::Array(list)) => list.iter().map(|p| p.as_str().and_then(self::version).ok_or_else(|| format!("pulled: {p} isn't a version"))).collect::<Result<_, _>>()?,
            Some(_) => return Err("pulled isn't a list".into()),
        };
        let mut platforms = BTreeMap::new();
        if let Some(all) = v.get("platforms").and_then(Value::as_object) {
            for (key, entry) in all {
                // One system's bad entry doesn't stop the others.
                if let Ok(a) = asset(entry) {
                    platforms.insert(key.clone(), a);
                }
            }
        }
        Ok(Release { version, text, pub_date, pulled, platforms })
    }

    /// Reads a manifest and checks its signature (`signature`, the `.minisig` file's text) by
    /// `key`, made for `update-v1.json` and the version the manifest names.
    pub fn verified(bytes: &[u8], signature: &str, key: &str) -> Result<Release, String> {
        let release = Release::parse(bytes)?;
        verify::signed(bytes, signature, key, &[format!("file:{RELEASE_MANIFEST}"), format!("version:{}", release.text)]).map_err(|e| format!("update-v1.json isn't the one signed: {e}"))?;
        Ok(release)
    }

    /// This system's archive.
    pub fn asset(&self, platform: Platform) -> Option<&Asset> {
        self.platforms.get(platform.key())
    }

    fn is_pulled(&self, v: &Version) -> bool {
        self.pulled.iter().any(|p| p.cmp_precedence(v) == Ordering::Equal)
    }
}

/// The newest manifest a channel's looks took here, kept in the updater's folder: an older
/// manifest, signed as it was, can't be passed off again (a replay) to offer a version since
/// withdrawn, or to take a copy down a way since closed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The highest version a manifest named.
    pub version: Option<Version>,
    /// The latest `pub_date` taken.
    pub pub_date: String,
}

impl Seen {
    /// Whether `release` may be taken after what was seen:
    /// - a later version;
    /// - the same version, published no earlier;
    /// - an earlier version only as the way down from the highest one, which it withdraws
    ///   (`pulled`), and published no earlier: an older manifest from before that version came
    ///   back can't withdraw it again.
    pub fn admits(&self, release: &Release) -> Result<(), String> {
        let Some(highest) = &self.version else { return Ok(()) };
        let not_before = || match (date(&release.pub_date), date(&self.pub_date)) {
            (Some(theirs), Some(seen)) => theirs >= seen,
            // Without a date to go by, the version alone decides.
            _ => true,
        };
        let ok = match release.version.cmp_precedence(highest) {
            Ordering::Greater => true,
            Ordering::Equal => not_before(),
            Ordering::Less => release.is_pulled(highest) && not_before(),
        };
        if ok {
            Ok(())
        } else {
            Err(format!("update-v1.json names {}, older than the {highest} it named before: not taken", release.text))
        }
    }

    /// Takes `release` in, once [`Seen::admits`] it. True when anything changed.
    pub fn note(&mut self, release: &Release) -> bool {
        let before = self.clone();
        if self.version.as_ref().is_none_or(|v| release.version.cmp_precedence(v) == Ordering::Greater) {
            self.version = Some(release.version.clone());
        }
        if let Some(theirs) = date(&release.pub_date) {
            if date(&self.pub_date).is_none_or(|seen| theirs > seen) {
                self.pub_date.clone_from(&release.pub_date);
            }
        }
        *self != before
    }

    pub fn read(bytes: &[u8]) -> Seen {
        let v: Value = serde_json::from_slice(bytes).unwrap_or_default();
        Seen { version: v["version"].as_str().and_then(version), pub_date: v["pub_date"].as_str().unwrap_or_default().to_owned() }
    }

    pub fn write(&self) -> Vec<u8> {
        serde_json::json!({ "version": self.version.as_ref().map(Version::to_string), "pub_date": self.pub_date }).to_string().into_bytes()
    }
}

/// A manifest's `pub_date` ("2026-12-01T10:00:00Z"), when it reads as one.
fn date(text: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(text.trim()).ok()
}

/// What a look found for this copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer<'a> {
    /// A later version.
    Newer(&'a Asset),
    /// This copy's version was withdrawn: the manifest's, older or not, takes its place.
    Replaces(&'a Asset),
}

impl<'a> Offer<'a> {
    pub fn asset(self) -> &'a Asset {
        match self {
            Offer::Newer(a) | Offer::Replaces(a) => a,
        }
    }
}

/// What `release` offers a copy of version `own` on `channel` for `platform`, leaving out
/// versions that failed to start here (`failed`).
///
/// - Never a pulled version, never one that failed here, never the same version.
/// - Never older, unless `own` itself is pulled ([`Offer::Replaces`]).
/// - On the stable channel never a pre-release ("1.0.1-rc.1"): only release candidates' own
///   channel carries those.
pub fn offer<'a>(release: &'a Release, own: &str, channel: Channel, platform: Platform, failed: &[String]) -> Option<Offer<'a>> {
    let own = version(own)?;
    let theirs = &release.version;
    if release.is_pulled(theirs) || failed.iter().any(|f| f == &release.text) || (channel == Channel::Stable && !theirs.pre.is_empty()) {
        return None;
    }
    let asset = release.asset(platform)?;
    match theirs.cmp_precedence(&own) {
        Ordering::Greater => Some(Offer::Newer(asset)),
        Ordering::Less if release.is_pulled(&own) => Some(Offer::Replaces(asset)),
        _ => None,
    }
}

/// The name an archive is signed under: its file's name in its url.
pub fn signed_name(asset: &Asset) -> &str {
    asset.url.rsplit('/').next().unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::update::manifest::DOWNLOADS;
    use crate::update::verify::tests::TestKey;

    const HASH: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    /// A manifest of `version` with a Linux archive, and whatever else `extra` adds.
    pub(crate) fn sample(version: &str, extra: &str) -> String {
        format!(
            r#"{{"schema":1,"version":"{version}","pub_date":"2026-12-01T10:00:00Z"{extra},"platforms":{{
              "linux-x64":{{"url":"{DOWNLOADS}v{version}/ssheesh-{version}-linux-x64.tar.gz","size":1234,"sha256":"{HASH}","signature":"c2ln"}},
              "windows-x64":{{"url":"https://example.invalid/ssheesh-{version}-windows-x64.zip","size":1234,"sha256":"{HASH}","signature":"c2ln"}}}}}}"#
        )
    }

    fn release(version: &str, extra: &str) -> Release {
        Release::parse(sample(version, extra).as_bytes()).unwrap()
    }

    #[test]
    fn the_manifest_reads() {
        let r = release("1.0.1", r#","pulled":["1.0.0"]"#);
        assert_eq!((r.text.as_str(), r.pub_date.as_str()), ("1.0.1", "2026-12-01T10:00:00Z"));
        assert_eq!(r.version, Version::new(1, 0, 1));
        assert_eq!(r.pulled, [Version::new(1, 0, 0)]);
        let linux = r.asset(Platform::LinuxX64).unwrap();
        assert_eq!((linux.size, linux.sha256.as_str()), (1234, HASH));
        assert_eq!(signed_name(linux), "ssheesh-1.0.1-linux-x64.tar.gz");
        // A download from anywhere but this repository's releases is left out.
        assert!(r.asset(Platform::WindowsX64).is_none());
        // No list of withdrawn versions: none are.
        assert!(release("1.0.1", "").pulled.is_empty());
    }

    #[test]
    fn what_isnt_a_manifest_is_refused() {
        for bad in [
            "".to_owned(),
            "{".to_owned(),
            r#"{"platforms":{}}"#.to_owned(),
            r#"{"version":"v1.0.1"}"#.to_owned(),
            r#"{"version":"1.0"}"#.to_owned(),
            r#"{"version":" 1.0.1"}"#.to_owned(),
            r#"{"version":1}"#.to_owned(),
            sample("1.0.1", r#","pulled":"1.0.0""#),
            sample("1.0.1", r#","pulled":["1.0.0","soon"]"#),
            sample("1.0.1", r#","pulled":[1]"#),
            sample("1.0.1", "").replace(r#""schema":1"#, r#""schema":2"#),
        ] {
            assert!(Release::parse(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn versions_compare_by_semver() {
        assert!(newer("1.0.1", "1.0.0"));
        assert!(newer("1.0.0", "1.0.0-canary.124"));
        assert!(newer("1.0.0", "1.0.0-rc.3"));
        assert!(newer("1.0.0-rc.1", "1.0.0-canary.124"));
        assert!(newer("1.0.0-canary.125", "1.0.0-canary.124"));
        assert!(newer("1.0.0-canary.124", "1.0.0-canary.99"), "numbers compare as numbers");
        assert!(newer("1.10.0", "1.9.9"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0+other", "1.0.0"), "build metadata doesn't make a version later");
        assert!(!newer("1.0.0-canary.124", "1.0.0"));
        assert!(!newer("0.10.14", "1.0.0"));
        assert!(!newer("2.0.0", "not a version") && !newer("garbage", "1.0.0"));
    }

    #[test]
    fn only_a_later_version_is_offered() {
        let r = release("1.0.1", "");
        let at = |own: &str| offer(&r, own, Channel::Stable, Platform::LinuxX64, &[]);
        assert!(matches!(at("1.0.0"), Some(Offer::Newer(_))));
        assert!(matches!(at("1.0.0-canary.124"), Some(Offer::Newer(_))), "a test build moves on to the release");
        assert!(matches!(at("1.0.1-rc.2"), Some(Offer::Newer(_))));
        // The same, a later one: nothing. Never down.
        assert_eq!(at("1.0.1"), None);
        assert_eq!(at("1.0.2"), None);
        assert_eq!(at("2.0.0-canary.1"), None);
        // A version that failed to start here, a system with no entry, a copy that isn't a version.
        assert_eq!(offer(&r, "1.0.0", Channel::Stable, Platform::LinuxX64, &["1.0.1".into()]), None);
        assert_eq!(offer(&r, "1.0.0", Channel::Stable, Platform::WindowsX64, &[]), None);
        assert_eq!(offer(&r, "1.0.0", Channel::Stable, Platform::MacArm64, &[]), None);
        assert_eq!(at("one"), None);
    }

    #[test]
    fn pre_releases_stay_on_their_channel() {
        let rc = release("1.0.1-rc.1", "");
        assert_eq!(offer(&rc, "1.0.0", Channel::Stable, Platform::LinuxX64, &[]), None);
        assert!(matches!(offer(&rc, "1.0.0", Channel::Rc, Platform::LinuxX64, &[]), Some(Offer::Newer(_))));
        assert!(matches!(offer(&rc, "1.0.1-rc.0", Channel::Rc, Platform::LinuxX64, &[]), Some(Offer::Newer(_))));
        // A release in the candidates' channel: newer than any of its candidates.
        let stable = release("1.0.1", "");
        assert!(matches!(offer(&stable, "1.0.1-rc.3", Channel::Rc, Platform::LinuxX64, &[]), Some(Offer::Newer(_))));
    }

    #[test]
    fn a_withdrawn_version_is_replaced_and_never_offered() {
        // 1.0.1 was withdrawn: its copies get 1.0.0 back, the only way down.
        let back = release("1.0.0", r#","pulled":["1.0.1"]"#);
        assert!(matches!(offer(&back, "1.0.1", Channel::Stable, Platform::LinuxX64, &[]), Some(Offer::Replaces(_))));
        // Anyone else stays where they are.
        assert_eq!(offer(&back, "1.0.2", Channel::Stable, Platform::LinuxX64, &[]), None);
        assert!(matches!(offer(&back, "0.9.0", Channel::Stable, Platform::LinuxX64, &[]), Some(Offer::Newer(_))));
        // A manifest naming a pulled version offers it to nobody.
        let pulled = release("1.0.2", r#","pulled":["1.0.2"]"#);
        assert_eq!(offer(&pulled, "1.0.0", Channel::Stable, Platform::LinuxX64, &[]), None);
        assert_eq!(offer(&pulled, "1.0.2", Channel::Stable, Platform::LinuxX64, &[]), None);
    }

    /// An older manifest, signed as it was, isn't taken again after a newer one: it can't
    /// offer a version since withdrawn, nor take copies down a way since closed.
    #[test]
    fn an_older_manifest_is_not_taken_again() {
        let dated = |version: &str, date: &str, pulled: &str| {
            let text = sample(version, &format!(r#","pulled":[{pulled}]"#)).replace("2026-12-01T10:00:00Z", date);
            Release::parse(text.as_bytes()).unwrap()
        };
        let mut seen = Seen::default();
        let first = dated("1.0.1", "2026-12-01T10:00:00Z", "");
        assert!(seen.admits(&first).is_ok(), "nothing seen yet");
        assert!(seen.note(&first));
        // 1.0.2 came out and withdrew 1.0.1; then 1.0.1's own manifest is played back.
        let next = dated("1.0.2", "2026-12-08T10:00:00Z", r#""1.0.1""#);
        assert!(seen.admits(&next).is_ok());
        seen.note(&next);
        assert!(seen.admits(&first).is_err());
        // The same manifest again: taken, and nothing changes.
        assert!(seen.admits(&next).is_ok() && !seen.note(&next));
        // 1.0.2 is withdrawn in turn: the way down to 1.0.1 is the newer manifest's to say.
        let down = dated("1.0.1", "2026-12-10T10:00:00Z", r#""1.0.2""#);
        assert!(seen.admits(&down).is_ok());
        seen.note(&down);
        assert_eq!((seen.version.as_ref().map(Version::to_string).as_deref(), seen.pub_date.as_str()), (Some("1.0.2"), "2026-12-10T10:00:00Z"));
        // 1.0.2 is put back: the manifest that withdrew it can't withdraw it again.
        let back = dated("1.0.2", "2026-12-12T10:00:00Z", "");
        assert!(seen.admits(&back).is_ok());
        seen.note(&back);
        assert!(seen.admits(&down).is_err());
        assert!(seen.admits(&next).is_err(), "nor an older 1.0.2");
        // Kept across starts.
        assert_eq!(Seen::read(&seen.write()), seen);
        assert_eq!(Seen::read(b"not json"), Seen::default());
    }

    #[test]
    fn the_manifest_is_taken_only_as_signed() {
        let (ours, theirs) = (TestKey::new(11), TestKey::new(12));
        let bytes = sample("1.0.1", "");
        let good = "timestamp:1794688787\tfile:update-v1.json\tversion:1.0.1";
        assert_eq!(Release::verified(bytes.as_bytes(), &ours.minisig(bytes.as_bytes(), good), &ours.public()).unwrap().text, "1.0.1");
        // As `tauri signer` writes a signature, too.
        assert!(Release::verified(bytes.as_bytes(), &ours.sign_comment(bytes.as_bytes(), good), &ours.public()).is_ok());
        // Another key (a wrong key for this copy): refused.
        assert!(Release::verified(bytes.as_bytes(), &theirs.minisig(bytes.as_bytes(), good), &ours.public()).is_err());
        // The release key's real public half doesn't take a signature made here.
        assert!(Release::verified(bytes.as_bytes(), &ours.minisig(bytes.as_bytes(), good), crate::update::channel::RELEASE_PUBLIC_KEY).is_err());
        // One byte changed after signing.
        let changed = bytes.replace("1234", "1235");
        assert!(Release::verified(changed.as_bytes(), &ours.minisig(bytes.as_bytes(), good), &ours.public()).is_err());
        // An older manifest's signature, or one for another file.
        let older = "timestamp:1794688787\tfile:update-v1.json\tversion:1.0.0";
        assert!(Release::verified(bytes.as_bytes(), &ours.minisig(bytes.as_bytes(), older), &ours.public()).is_err());
        let other = "timestamp:1794688787\tfile:latest.json\tversion:1.0.1";
        assert!(Release::verified(bytes.as_bytes(), &ours.minisig(bytes.as_bytes(), other), &ours.public()).is_err());
        // No signature, or garbage.
        assert!(Release::verified(bytes.as_bytes(), "", &ours.public()).is_err());
        assert!(Release::verified(bytes.as_bytes(), "not a signature", &ours.public()).is_err());
    }
}
