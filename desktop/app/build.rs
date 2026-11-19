//! Which build this is, put into the program as it's built (`update::build`): the commit and,
//! for a test build, the canary workflow's run.
//!
//! - `SSHEESH_BUILD_SHA`: the commit. The canary workflow sets it; elsewhere `GITHUB_SHA`
//!   (any CI), then `git rev-parse` here, then nothing.
//! - `SSHEESH_CANARY_RUN`: the canary workflow's run number, set only on the step that builds
//!   what it publishes.
//! - `SSHEESH_RELEASE=1`: built by the release workflow, on the step that builds what it
//!   publishes: the build looks for newer releases (`update-v1.json`). Ignored when
//!   `SSHEESH_CANARY_RUN` is set.
//!
//! A build with neither (tests, a build on your computer) never updates itself.
//!
//! On Windows it also puts into the program what a clean Windows needs of it (plan, Phase 7
//! item 2): its icon, its version, and a manifest asking for no administrator
//! (`windows/ssheesh.manifest`), since the same file is the installer. A release build fails
//! without them; elsewhere (a cross-check without a resource compiler) it only warns.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").current_dir(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for var in ["SSHEESH_BUILD_SHA", "SSHEESH_CANARY_RUN", "SSHEESH_RELEASE", "GITHUB_SHA"] {
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
    let release = run.is_none() && std::env::var("SSHEESH_RELEASE").is_ok_and(|v| v.trim() == "1");
    println!("cargo:rustc-env=SSHEESH_RELEASE={}", if release { "1" } else { "" });
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources(&dir, release);
    }
}

/// The icon, the version and the manifest, as a Windows resource script compiled into the
/// program (`embed-resource`: rc.exe with MSVC, windres with MinGW).
fn windows_resources(dir: &Path, release: bool) {
    for file in ["windows/ssheesh.manifest", "windows/ssheesh.ico"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let numbers: Vec<u16> = version.split(['.', '-']).take(3).map(|n| n.parse().unwrap_or(0)).collect();
    let (major, minor, patch) = (numbers.first().copied().unwrap_or(0), numbers.get(1).copied().unwrap_or(0), numbers.get(2).copied().unwrap_or(0));
    let quoted = |p: PathBuf| p.display().to_string().replace('\\', "\\\\");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "SSHeesh contributors"
      VALUE "FileDescription", "SSHeesh"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "ssheesh"
      VALUE "LegalCopyright", "© 2026 footprintsntrails. CC BY-NC-SA 4.0"
      VALUE "OriginalFilename", "ssheesh.exe"
      VALUE "ProductName", "SSHeesh"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = quoted(dir.join("windows").join("ssheesh.ico")),
        manifest = quoted(dir.join("windows").join("ssheesh.manifest")),
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("ssheesh.rc");
    if let Err(e) = std::fs::write(&out, rc) {
        panic!("the Windows resource script couldn't be written: {e}");
    }
    if let Err(e) = embed_resource::compile(&out, embed_resource::NONE).manifest_required() {
        if release {
            panic!("the release's Windows resources (icon, version, manifest) didn't compile: {e}");
        }
        println!("cargo:warning=no Windows resources in this build (icon, version, manifest): {e}");
    }
}
