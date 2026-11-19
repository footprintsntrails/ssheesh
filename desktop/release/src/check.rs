//! The check before anything is published (plan §6.1): `latest.json` and `update-v1.json`
//! read back the way 0.10 and 1.x read them, each entry's file checked as it would be
//! installed. A mistake in `latest.json` would break the update of every 0.10 at once, and a
//! file that installs but doesn't start would replace 0.10 with nothing: so this refuses
//! rather than warns.

use std::io::Read as _;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use serde_json::Value;

use crate::files::{sha256_hex, version_ok, Release, System};

/// SSHeesh's bundle id on the Mac: the same as 0.10's, or the Keychain asks about every saved
/// password and notifications lose their permission.
const BUNDLE_ID: &str = "io.github.footprintsntrails.ssheesh";

fn unbox(b64: &str) -> Result<String, String> {
    let bytes = STANDARD.decode(b64.trim()).map_err(|_| "not base64")?;
    String::from_utf8(bytes).map_err(|_| "not text".into())
}

/// A signature as 0.10's updater checks it (`verify_signature`): by `key`, over `data`, with a
/// `version:` in its trusted comment equal to `version`. Returns the trusted comment.
pub fn signed(data: &[u8], signature_b64: &str, key_b64: &str, version: &str) -> Result<String, String> {
    let key = PublicKey::decode(&unbox(key_b64)?).map_err(|e| format!("the public key: {e}"))?;
    let text = if signature_b64.trim_start().starts_with("untrusted comment:") { signature_b64.to_owned() } else { unbox(signature_b64)? };
    let sig = Signature::decode(&text).map_err(|e| format!("the signature: {e}"))?;
    key.verify(data, &sig, true).map_err(|e| format!("the signature doesn't check out: {e}"))?;
    let trusted = sig.trusted_comment().to_owned();
    let signed_version = trusted.split('\t').find_map(|f| f.strip_prefix("version:")).ok_or("the signature carries no version")?;
    let (a, b) = (version_ok(signed_version)?, version_ok(version)?);
    if a != b {
        return Err(format!("signed for {signed_version}, announced as {version}"));
    }
    Ok(trusted)
}

/// The entry 0.10's updater picks on `os`-`arch` installed as `installer`, as its `get_urls`
/// does: `{os}-{arch}-{installer}`, then `{os}-{arch}`; both `url` and `signature` must be
/// there. The portable copy asks for its own key outright (`target`).
fn pick<'a>(platforms: &'a serde_json::Map<String, Value>, os: &str, arch: &str, installer: Option<&str>, target: Option<&str>) -> Result<(&'a str, &'a str, String), String> {
    let keys: Vec<String> = match target {
        Some(t) => vec![t.to_owned()],
        None => installer.map(|i| format!("{os}-{arch}-{i}")).into_iter().chain([format!("{os}-{arch}")]).collect(),
    };
    for key in &keys {
        if let Some(entry) = platforms.get(key) {
            if let (Some(url), Some(sig)) = (entry.get("url").and_then(Value::as_str), entry.get("signature").and_then(Value::as_str)) {
                return Ok((url, sig, key.clone()));
            }
        }
    }
    Err(format!("none of {keys:?} has a url and a signature"))
}

/// The file a release's url names, from the release's folder.
fn file_of(release: &Release, url: &str) -> Result<(String, Vec<u8>), String> {
    let prefix = release.url("");
    let name = url.strip_prefix(&prefix).filter(|n| !n.is_empty() && !n.contains(['/', '?', '#'])).ok_or_else(|| format!("{url} isn't a file of {}", prefix))?;
    let bytes = std::fs::read(release.path(name)).map_err(|e| format!("{name}: {e}"))?;
    Ok((name.to_owned(), bytes))
}

/// A Windows x64 program (PE32+, AMD64), with a manifest asking for no administrator.
fn windows_program(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return Err("not a Windows program".into());
    }
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap_or_default()) as usize;
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0") || bytes.get(pe + 4..pe + 6) != Some(&0x8664u16.to_le_bytes()[..]) {
        return Err("not an x64 Windows program".into());
    }
    if !contains(bytes, b"asInvoker") {
        return Err("no manifest with requestedExecutionLevel asInvoker".into());
    }
    Ok(())
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// An x86-64 ELF program.
fn linux_program(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 20 || &bytes[0..4] != b"\x7fELF" || bytes[4] != 2 || u16::from_le_bytes([bytes[18], bytes[19]]) != 62 {
        return Err("not an x86-64 ELF program".into());
    }
    Ok(())
}

/// The `.app.tar.gz` as 0.10's updater unpacks it: one top folder dropped, the rest becomes
/// the app. That top folder must be the only one; inside, `Contents/Info.plist` with SSHeesh's
/// bundle id and `ssheesh` as its executable, and `Contents/MacOS/ssheesh` executable.
fn mac_app(bytes: &[u8]) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let (mut tops, mut plist, mut exe_mode) = (std::collections::BTreeSet::new(), None, None);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| format!("the archive doesn't read: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let mut parts = path.components();
        let Some(top) = parts.next() else { continue };
        tops.insert(top.as_os_str().to_string_lossy().into_owned());
        let inner: std::path::PathBuf = parts.collect();
        if inner == std::path::Path::new("Contents/Info.plist") {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
            plist = Some(text);
        } else if inner == std::path::Path::new("Contents/MacOS/ssheesh") {
            exe_mode = Some(entry.header().mode().unwrap_or(0));
        }
    }
    if tops.len() != 1 || !tops.iter().all(|t| t.ends_with(".app")) {
        return Err(format!("not one .app folder at the top: {tops:?}"));
    }
    let plist = plist.ok_or("no Contents/Info.plist")?;
    let value_of = |key: &str| plist.split(&format!("<key>{key}</key>")).nth(1).and_then(|r| r.split("<string>").nth(1)).and_then(|r| r.split("</string>").next()).map(str::trim).map(str::to_owned);
    if value_of("CFBundleIdentifier").as_deref() != Some(BUNDLE_ID) {
        return Err(format!("its bundle id isn't {BUNDLE_ID}"));
    }
    if value_of("CFBundleExecutable").as_deref() != Some("ssheesh") {
        return Err("its executable isn't `ssheesh`".into());
    }
    match exe_mode {
        Some(mode) if mode & 0o111 != 0 => Ok(()),
        Some(_) => Err("Contents/MacOS/ssheesh isn't executable".into()),
        None => Err("no Contents/MacOS/ssheesh".into()),
    }
}

/// Checks both manifests in the release's folder. What each check found, for the log.
pub fn all(release: &Release, key: &str) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    said.extend(latest(release, key)?);
    said.extend(update_v1(release, key)?);
    Ok(said)
}

/// A kind of 0.10: who it is, its system and architecture as its updater names them, the
/// installer it was installed by, and the key it asks for outright (the portable copy).
type Kind = (&'static str, &'static str, &'static str, Option<&'static str>, Option<&'static str>);

/// `latest.json`, as each kind of 0.10 reads it.
pub fn latest(release: &Release, key: &str) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(release.path("latest.json")).map_err(|e| format!("latest.json: {e}"))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("latest.json doesn't read: {e}"))?;
    let version = v.get("version").and_then(Value::as_str).ok_or("latest.json has no version")?;
    let announced = version_ok(version.trim_start_matches('v'))?;
    if announced != version_ok(&release.version)? {
        return Err(format!("latest.json announces {version}, the release is {}", release.version));
    }
    if !v.get("notes").is_none_or(Value::is_string) {
        return Err("latest.json's notes aren't text".into());
    }
    match v.get("pub_date") {
        None => {}
        Some(Value::String(d)) if rfc3339_ok(d) => {}
        Some(other) => return Err(format!("latest.json's pub_date {other} isn't RFC 3339")),
    }
    let platforms = v.get("platforms").and_then(Value::as_object).ok_or("latest.json has no platforms")?;
    let mut said = Vec::new();
    let mut kinds: Vec<Kind> = Vec::new();
    for s in &release.platforms {
        match s {
            System::Windows => kinds.extend([("0.10 installed on Windows", "windows", "x86_64", Some("nsis"), None), ("0.10 portable on Windows", "windows", "x86_64", None, Some("windows-x86_64-portable"))]),
            System::Mac => kinds.push(("0.10 on an Apple silicon Mac", "darwin", "aarch64", Some("app"), None)),
            System::MacIntel => kinds.push(("0.10 on an Intel Mac", "darwin", "x86_64", Some("app"), None)),
            System::Linux => kinds.push(("0.10's AppImage on Linux", "linux", "x86_64", Some("appimage"), None)),
        }
    }
    for (who, os, arch, installer, target) in kinds {
        let (url, sig, key_used) = pick(platforms, os, arch, installer, target).map_err(|e| format!("{who}: {e}"))?;
        let (name, data) = file_of(release, url).map_err(|e| format!("{who}: {e}"))?;
        signed(&data, sig, key, version).map_err(|e| format!("{who}, {name}: {e}"))?;
        match os {
            "windows" => windows_program(&data),
            "linux" => linux_program(&data),
            _ => mac_app(&data),
        }
        .map_err(|e| format!("{who}, {name}: {e}"))?;
        said.push(format!("{who} takes {name} ({key_used}), signed for {version}"));
    }
    // Nothing for systems not moved: they stay quietly on 0.10.
    let moved: Vec<&str> = release.latest_entries().into_iter().map(|(k, _)| k).collect();
    if let Some(extra) = platforms.keys().find(|k| !moved.contains(&k.as_str())) {
        return Err(format!("latest.json has {extra}, a system this release doesn't move"));
    }
    Ok(said)
}

/// `2026-11-19T10:00:00Z` and the like.
fn rfc3339_ok(d: &str) -> bool {
    let b = d.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    b.len() >= 20 && digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10) && b[10] == b'T' && digits(11..13) && b[13] == b':' && digits(14..16) && b[16] == b':' && digits(17..19) && (d.ends_with('Z') || d[19..].contains(['+', '-']))
}

/// `update-v1.json` and its `.minisig`, as 1.x reads them (`desktop/app/src/update/release.rs`).
pub fn update_v1(release: &Release, key: &str) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(release.path("update-v1.json")).map_err(|e| format!("update-v1.json: {e}"))?;
    let minisig = std::fs::read_to_string(release.path("update-v1.json.minisig")).map_err(|e| format!("update-v1.json.minisig: {e}"))?;
    let trusted = signed(&bytes, &minisig, key, &release.version).map_err(|e| format!("update-v1.json: {e}"))?;
    if !trusted.split('\t').any(|f| f == "file:update-v1.json") {
        return Err("update-v1.json's signature isn't made for update-v1.json".into());
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("update-v1.json doesn't read: {e}"))?;
    if v.get("schema").and_then(Value::as_u64) != Some(1) || v.get("version").and_then(Value::as_str) != Some(release.version.as_str()) {
        return Err("update-v1.json isn't schema 1 of this version".into());
    }
    let platforms = v.get("platforms").and_then(Value::as_object).ok_or("update-v1.json has no platforms")?;
    let mut said = Vec::new();
    for (key_name, want) in release.v1_entries() {
        let entry = platforms.get(key_name).ok_or_else(|| format!("update-v1.json has no {key_name}"))?;
        let url = entry.get("url").and_then(Value::as_str).ok_or("no url")?;
        let (name, data) = file_of(release, url)?;
        if name != want {
            return Err(format!("{key_name} names {name}, not {want}"));
        }
        if entry.get("size").and_then(Value::as_u64) != Some(data.len() as u64) || entry.get("sha256").and_then(Value::as_str) != Some(sha256_hex(&data).as_str()) {
            return Err(format!("{key_name}: the size or SHA-256 isn't {name}'s"));
        }
        let sig = entry.get("signature").and_then(Value::as_str).ok_or("no signature")?;
        let trusted = signed(&data, sig, key, &release.version).map_err(|e| format!("{key_name}, {name}: {e}"))?;
        if !trusted.split('\t').any(|f| f == format!("file:{name}")) {
            return Err(format!("{key_name}: {name}'s signature is made for another file"));
        }
        said.push(format!("1.x on {key_name} takes {name}"));
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::{self, SecretKey};
    use std::path::Path;

    fn pe() -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        b.extend_from_slice(b"<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"false\"/>");
        b
    }

    fn elf() -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[18..20].copy_from_slice(&62u16.to_le_bytes());
        b
    }

    fn app_tar(id: &str, mode: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            let plist = format!("<plist><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleExecutable</key><string>ssheesh</string></dict></plist>");
            for (path, data, m) in [("SSHeesh.app/Contents/Info.plist", plist.as_bytes(), 0o644), ("SSHeesh.app/Contents/MacOS/ssheesh", b"macho".as_slice(), mode)] {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(m);
                h.set_cksum();
                t.append_data(&mut h, path, data).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        }
        out
    }

    fn zip_like() -> Vec<u8> {
        b"PK\x03\x04 a made-up zip".to_vec()
    }

    /// A whole release in a folder, signed with a throwaway key: then `manifests` and `check`
    /// as the workflow runs them.
    fn release_in(dir: &Path, key: &SecretKey) -> Release {
        let r = Release::of("1.0.0", "footprintsntrails/ssheesh", "v1.0.0", dir, None).unwrap();
        let n = &r.names;
        let files: Vec<(&str, Vec<u8>)> = vec![
            (&n.installer, pe()),
            (&n.portable, pe()),
            (&n.windows_zip, zip_like()),
            (&n.mac_app, app_tar(BUNDLE_ID, 0o755)),
            (&n.linux_program, elf()),
            (&n.linux_tar, b"\x1f\x8b made-up tar.gz".to_vec()),
        ];
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
            sign::sign_file(key, &dir.join(name), "1.0.0", false).unwrap();
        }
        let latest = r.latest_json("SSHeesh 1.0.0").unwrap();
        std::fs::write(dir.join("latest.json"), latest).unwrap();
        std::fs::write(dir.join("update-v1.json"), r.update_v1_json(&["0.9.0".into()]).unwrap()).unwrap();
        sign::sign_file(key, &dir.join("update-v1.json"), "1.0.0", true).unwrap();
        r
    }

    fn public(key: &SecretKey) -> String {
        STANDARD.encode(key.public_text())
    }

    #[test]
    fn a_release_made_as_the_workflow_makes_it_passes() {
        let dir = tempdir("pass");
        let key = SecretKey::decode(&sign::tests::key_file(5, Some("pw")), "pw").unwrap();
        let r = release_in(&dir, &key);
        let said = all(&r, &public(&key)).unwrap();
        assert!(said.iter().any(|l| l.contains("0.10 portable on Windows takes SSHeesh.exe")), "{said:?}");
        assert!(said.iter().any(|l| l.contains("0.10 installed on Windows takes SSHeesh_1.0.0_x64-setup.exe (windows-x86_64-nsis)")), "{said:?}");
        // The real release key didn't sign these.
        assert!(all(&r, crate::RELEASE_PUBLIC_KEY).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_would_break_an_update_is_refused() {
        let dir = tempdir("refuse");
        let key = SecretKey::decode(&sign::tests::key_file(6, None), "").unwrap();
        let r = release_in(&dir, &key);
        let k = public(&key);
        // A file changed after it was signed.
        std::fs::write(dir.join(&r.names.linux_program), [elf(), b"x".to_vec()].concat()).unwrap();
        assert!(latest(&r, &k).unwrap_err().contains("AppImage"));
        std::fs::write(dir.join(&r.names.linux_program), elf()).unwrap();
        sign::sign_file(&key, &dir.join(&r.names.linux_program), "1.0.0", false).unwrap();
        assert!(latest(&r, &k).is_ok());
        // Signed for another version.
        sign::sign_file(&key, &dir.join(&r.names.portable), "1.0.1", false).unwrap();
        std::fs::write(dir.join("latest.json"), r.latest_json("n").unwrap()).unwrap();
        assert!(latest(&r, &k).unwrap_err().contains("signed for 1.0.1"));
        sign::sign_file(&key, &dir.join(&r.names.portable), "1.0.0", false).unwrap();
        std::fs::write(dir.join("latest.json"), r.latest_json("n").unwrap()).unwrap();
        // A Mac app under another bundle id.
        std::fs::write(dir.join(&r.names.mac_app), app_tar("com.example.other", 0o755)).unwrap();
        sign::sign_file(&key, &dir.join(&r.names.mac_app), "1.0.0", false).unwrap();
        std::fs::write(dir.join("latest.json"), r.latest_json("n").unwrap()).unwrap();
        assert!(latest(&r, &k).unwrap_err().contains("bundle id"));
        std::fs::write(dir.join(&r.names.mac_app), app_tar(BUNDLE_ID, 0o644)).unwrap();
        sign::sign_file(&key, &dir.join(&r.names.mac_app), "1.0.0", false).unwrap();
        std::fs::write(dir.join("latest.json"), r.latest_json("n").unwrap()).unwrap();
        assert!(latest(&r, &k).unwrap_err().contains("executable"));
        // A pub_date that isn't RFC 3339 breaks every 0.10's update: refused.
        let mut v: Value = serde_json::from_str(&r.latest_json("n").unwrap_or_default()).unwrap_or_default();
        v["pub_date"] = "19 Nov 2026".into();
        std::fs::write(dir.join("latest.json"), v.to_string()).unwrap();
        assert!(latest(&r, &k).unwrap_err().contains("pub_date"));
        // update-v1.json changed after signing.
        let mut v1: Value = serde_json::from_slice(&std::fs::read(dir.join("update-v1.json")).unwrap()).unwrap();
        v1["pulled"] = serde_json::json!([]);
        std::fs::write(dir.join("update-v1.json"), v1.to_string()).unwrap();
        assert!(update_v1(&r, &k).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_release_for_some_systems_leaves_the_others_on_0_10() {
        let dir = tempdir("some");
        let key = SecretKey::decode(&sign::tests::key_file(8, None), "").unwrap();
        release_in(&dir, &key);
        let r = Release::of("1.0.0", "footprintsntrails/ssheesh", "v1.0.0", &dir, Some("windows")).unwrap();
        std::fs::write(dir.join("latest.json"), r.latest_json("n").unwrap()).unwrap();
        let said = latest(&r, &public(&key)).unwrap();
        assert_eq!(said.len(), 2);
        let all_systems = Release::of("1.0.0", "footprintsntrails/ssheesh", "v1.0.0", &dir, None).unwrap();
        assert!(latest(&all_systems, &public(&key)).is_err());
        let v: Value = serde_json::from_slice(&std::fs::read(dir.join("latest.json")).unwrap()).unwrap();
        assert!(v["platforms"].get("linux-x86_64").is_none() && v["platforms"].get("darwin-aarch64").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn programs_are_told_apart() {
        assert!(windows_program(&pe()).is_ok() && windows_program(&elf()).is_err());
        let mut no_manifest = pe();
        no_manifest.truncate(0x200);
        assert!(windows_program(&no_manifest).is_err());
        assert!(linux_program(&elf()).is_ok() && linux_program(&pe()).is_err());
        assert!(rfc3339_ok("2026-11-19T10:00:00Z") && rfc3339_ok("2026-11-19T10:00:00.5+03:00") && !rfc3339_ok("2026-11-19"));
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ssheesh-release-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
