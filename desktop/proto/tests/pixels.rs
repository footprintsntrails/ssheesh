//! "Never drawn", proved with pixels (prototype-design.md §9, proof 1): the same fixture drawn
//! with two different made-up secrets gives the same pixels in every frame, while the secrets
//! come in pieces (cut after their first characters, inside a JSON string, across a wrap) and
//! after the start of one scrolled off the top. If any pixel depended on a secret's characters
//! the frames would differ. A control fixture that prints a secret's characters with nothing
//! to say they are one shows the comparison does see them.
//!
//! Needs a Vulkan driver, as tests/replay.rs.

use std::path::{Path, PathBuf};
use std::process::Command;

const LVP: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";

fn vulkan() -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("VK_ICD_FILENAMES").is_some() {
        return true;
    }
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().any(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some()))
}

/// Every 2nd frame of `fixture` drawn with secrets from `seed`, as PNG bytes, in order.
fn frames(name: &str, fixture: &Path, seed: u64) -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pixels").join(format!("{name}-{seed}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ssheesh-proto"));
    cmd.arg("--replay").arg(fixture).args(["--secret-seed", &seed.to_string(), "--name", "web-7731"]);
    cmd.arg("--out").arg(&dir).args(["--frames", "54", "--every", "2", "--scale", "1", "--size", "960x700"]);
    if cfg!(target_os = "linux") && std::env::var_os("VK_ICD_FILENAMES").is_none() && Path::new(LVP).exists() {
        cmd.env("VK_ICD_FILENAMES", LVP);
    }
    let result = cmd.output().expect("the prototype runs");
    assert!(result.status.success(), "{name}: {}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
    files.sort();
    files.into_iter().map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap())).collect()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

#[test]
fn secrets_in_pieces_draw_the_same_pixels_whatever_they_are() {
    if !vulkan() {
        eprintln!("skipped, no Vulkan driver here");
        return;
    }
    let one = frames("pieces", &fixture("secret_pieces.json"), 1);
    let two = frames("pieces", &fixture("secret_pieces.json"), 2);
    assert_eq!(one.len(), 27, "frames written");
    assert_eq!(one.len(), two.len());
    for ((name, a), (_, b)) in one.iter().zip(&two) {
        assert!(a == b, "{name} differs between the two secrets");
    }
    // The fixture does draw something new along the way (the comparison isn't of empty frames).
    assert!(one.windows(2).any(|w| w[0].1 != w[1].1));
}

#[test]
fn the_comparison_sees_characters_that_differ() {
    if !vulkan() {
        eprintln!("skipped, no Vulkan driver here");
        return;
    }
    // A secret's body printed with nothing to say it's one: it is drawn, and the frames differ.
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pixels");
    std::fs::create_dir_all(&dir).unwrap();
    let control = dir.join("control.json");
    std::fs::write(&control, r#"{ "events": [ { "t": 0.1, "out": "web-7731:~$ echo {{SECRET_A:10:30}}\r\n" } ] }"#).unwrap();
    let one = frames("control", &control, 1);
    let two = frames("control", &control, 2);
    assert!(one.iter().zip(&two).any(|(a, b)| a.1 != b.1), "a shown secret must change pixels");
}
