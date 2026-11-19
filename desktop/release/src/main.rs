//! SSHeesh's release steps that need care, as one small program (plan §6.1, Phase 7 item 5;
//! `reports/Обновления 1.0 — что добавить в выпуск.md`):
//!
//! - `sign`: minisign signatures by the release key (the one every 0.10 trusts), each with
//!   `timestamp:…\tfile:<name>\tversion:<version>` as its signed trusted comment. 0.10's updater
//!   (tauri-plugin-updater 2.14.1) checks that `version:` against `latest.json`'s; 1.x checks
//!   `file:` and `version:`. `tauri signer sign` can't write a version, so this does it. The key
//!   comes as the workflow's secrets give it: `TAURI_SIGNING_PRIVATE_KEY` (the key file, base64
//!   as `tauri signer generate` writes it) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
//! - `manifests`: `latest.json` (what moves 0.10.x to this release, in the form 0.10's updater
//!   reads) and `update-v1.json` (what 1.x reads, `desktop/app/src/update/release.rs`), from the
//!   release's files and their signatures in one folder.
//! - `check`: reads both back as 0.10 and 1.x would and refuses to let them out if any system's
//!   entry wouldn't install: the entry 0.10's updater would pick (its `get_urls`), the file it
//!   names, its signature and signed version, and the file's form (a Windows x64 program, an
//!   ELF for Linux, a `.app` with SSHeesh's bundle id in one top folder for the Mac).
//!
//! Nothing here touches the network, GitHub or a release: the workflow does that with what
//! this wrote.

mod check;
mod files;
mod sign;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage:
  ssheesh-release sign --version V [--minisig] FILE...
  ssheesh-release manifests --version V --repo OWNER/NAME --tag TAG --dir DIR
                            [--platforms windows,macos,macos-intel,linux] [--pulled FILE] [--notes FILE]
  ssheesh-release verify --version V [--key PUBLIC_KEY_BASE64] FILE...
  ssheesh-release check --version V --repo OWNER/NAME --tag TAG --dir DIR
                        [--platforms ...] [--key PUBLIC_KEY_BASE64]";

/// The release key's public half, as 0.10's `src-tauri/tauri.conf.json` has it.
pub const RELEASE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIyMzlCOEJDMURBOUQ3NDQKUldSRTE2a2R2TGc1c3Z6V3NjSDk1QlVTM01qZ2tRdyswMStJUlVteE9FRzBtSjloQVlQWUZ0WUwK";

/// The arguments, as `--name value` pairs and the rest.
#[derive(Debug, Default)]
struct Args {
    version: Option<String>,
    repo: Option<String>,
    tag: Option<String>,
    dir: Option<PathBuf>,
    platforms: Option<String>,
    pulled: Option<PathBuf>,
    notes: Option<PathBuf>,
    key: Option<String>,
    minisig: bool,
    files: Vec<PathBuf>,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--version" => a.version = Some(value()?),
            "--repo" => a.repo = Some(value()?),
            "--tag" => a.tag = Some(value()?),
            "--dir" => a.dir = Some(value()?.into()),
            "--platforms" => a.platforms = Some(value()?),
            "--pulled" => a.pulled = Some(value()?.into()),
            "--notes" => a.notes = Some(value()?.into()),
            "--key" => a.key = Some(value()?),
            "--minisig" => a.minisig = true,
            other if other.starts_with("--") => return Err(format!("unknown flag {other}")),
            file => a.files.push(file.into()),
        }
    }
    Ok(a)
}

fn need<T: Clone>(v: &Option<T>, name: &str) -> Result<T, String> {
    v.clone().ok_or_else(|| format!("--{name} is needed"))
}

fn run(argv: &[String]) -> Result<(), String> {
    let (command, rest) = argv.split_first().ok_or(USAGE)?;
    let a = parse(rest)?;
    match command.as_str() {
        "sign" => {
            let version = need(&a.version, "version")?;
            files::version_ok(&version)?;
            if a.files.is_empty() {
                return Err("nothing to sign".into());
            }
            let key = sign::SecretKey::from_env()?;
            for file in &a.files {
                let out = sign::sign_file(&key, file, &version, a.minisig)?;
                println!("signed {} -> {}", file.display(), out.display());
            }
            Ok(())
        }
        "manifests" => {
            let release = files::Release::of(&need(&a.version, "version")?, &need(&a.repo, "repo")?, &need(&a.tag, "tag")?, &need(&a.dir, "dir")?, a.platforms.as_deref())?;
            let notes = match &a.notes {
                Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?,
                None => format!("SSHeesh {}", release.version),
            };
            let pulled = match &a.pulled {
                Some(f) => files::pulled_of(&std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?)?,
                None => Vec::new(),
            };
            let latest = release.latest_json(&notes)?;
            let v1 = release.update_v1_json(&pulled)?;
            std::fs::write(release.dir.join("latest.json"), &latest).map_err(|e| e.to_string())?;
            std::fs::write(release.dir.join("update-v1.json"), &v1).map_err(|e| e.to_string())?;
            println!("wrote latest.json ({} systems of 0.10) and update-v1.json", release.platforms.len());
            Ok(())
        }
        "verify" => {
            let version = need(&a.version, "version")?;
            let key = a.key.clone().unwrap_or_else(|| RELEASE_PUBLIC_KEY.to_owned());
            for file in &a.files {
                let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
                let sig_at = format!("{}.sig", file.display());
                let sig = std::fs::read_to_string(&sig_at).map_err(|e| format!("{sig_at}: {e}"))?;
                let trusted = check::signed(&data, &sig, &key, &version).map_err(|e| format!("{}: {e}", file.display()))?;
                println!("ok: {} ({trusted})", file.display());
            }
            Ok(())
        }
        "check" => {
            let release = files::Release::of(&need(&a.version, "version")?, &need(&a.repo, "repo")?, &need(&a.tag, "tag")?, &need(&a.dir, "dir")?, a.platforms.as_deref())?;
            let key = a.key.unwrap_or_else(|| RELEASE_PUBLIC_KEY.to_owned());
            let report = check::all(&release, &key)?;
            for line in report {
                println!("ok: {line}");
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ssheesh-release: {e}");
            ExitCode::FAILURE
        }
    }
}
