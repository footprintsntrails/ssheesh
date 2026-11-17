//! Which build this is, put into the program as it's built (`update::build`): the commit and,
//! for a test build, the canary workflow's run.
//!
//! - `SSHEESH_BUILD_SHA`: the commit. The canary workflow sets it; elsewhere `GITHUB_SHA`
//!   (any CI), then `git rev-parse` here, then nothing.
//! - `SSHEESH_CANARY_RUN`: the canary workflow's run number, set only on the step that builds
//!   what it publishes. A build without it (tests, a build on your computer) never updates
//!   itself.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").current_dir(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for var in ["SSHEESH_BUILD_SHA", "SSHEESH_CANARY_RUN", "GITHUB_SHA"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let given = std::env::var("SSHEESH_BUILD_SHA").ok().or_else(|| std::env::var("GITHUB_SHA").ok()).filter(|s| !s.trim().is_empty());
    let sha = match given {
        Some(sha) => Some(sha.trim().to_owned()),
        None => {
            // Built here: the commit checked out, and a new one builds this again.
            for path in ["HEAD", "packed-refs"].iter().filter_map(|p| git(&dir, &["rev-parse", "--git-path", p])) {
                let path = dir.join(path);
                if path.exists() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
            if let Some(head) = git(&dir, &["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&dir, &["rev-parse", "--git-path", &r])) {
                let path = dir.join(head);
                if path.exists() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
            git(&dir, &["rev-parse", "HEAD"])
        }
    };
    // Seven hex digits, as GitHub and `git log --oneline` show a commit; anything else is dropped.
    let sha: String = sha.unwrap_or_default().chars().take(7).collect();
    let sha = if sha.len() == 7 && sha.chars().all(|c| c.is_ascii_hexdigit()) { sha.to_ascii_lowercase() } else { String::new() };
    println!("cargo:rustc-env=SSHEESH_BUILD_SHA={sha}");
    let run = std::env::var("SSHEESH_CANARY_RUN").ok().and_then(|r| r.trim().parse::<u64>().ok()).filter(|r| *r > 0);
    println!("cargo:rustc-env=SSHEESH_CANARY_RUN={}", run.map(|r| r.to_string()).unwrap_or_default());
}
