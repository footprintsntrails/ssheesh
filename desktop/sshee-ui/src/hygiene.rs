//! What Rune must not find in the environment.
//!
//! Rune turns on tracing and logs from environment variables, in release builds too:
//! `RUNE_PERF` or `OTEL_EXPORTER_OTLP_ENDPOINT` export spans with element paths and input timing,
//! `OTEL_SERVICE_NAME` names them, `RUNE_A11Y_LOG` logs accessibility text (in a terminal, what
//! is on the screen), `RUNE_FRAME_LOG` writes frame logs to a path. Whoever can set a variable
//! for the app (a launcher, a shell profile) could read a terminal through them. Rune reads
//! about 65 such names and adds more, so every `RUNE_*` and `OTEL_*` goes except the two the
//! app needs: an allowlist, not a blocklist.

use std::ffi::OsString;

/// The variables that stay. `RUNE_READY_DIR`: where to write the note a launcher waits for
/// (pid, bundle id, milliseconds; Rune's platform/ready_signal.rs). `RUNE_GPU_BACKEND`: the
/// renderer switch, for trying wgpu on macOS 11 and 12.
pub const KEPT: [&str; 2] = ["RUNE_READY_DIR", "RUNE_GPU_BACKEND"];

/// Whether the variable `name` is removed. Case is ignored, as Windows looks names up.
pub fn removed(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    (name.starts_with("RUNE_") || name.starts_with("OTEL_")) && !KEPT.contains(&name.as_str())
}

/// What [`scrub_env`] took from the process, for the programs it starts: they get the
/// environment it was started with, less the two Rune switches meant for it alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scrubbed {
    /// The variables removed, with their values: a shell in the terminal gets them back (a
    /// developer's own `OTEL_*` or `RUNE_*`, from a launcher or a parent shell).
    pub removed: Vec<(OsString, OsString)>,
    /// The kept ones that are set: Rune's, not a child's. A Rune program run in the terminal
    /// mustn't write its ready note into the folder a launcher watches for SSHeesh's.
    pub kept: Vec<OsString>,
}

/// Removes them from this process's environment. Call it first thing in `main`, before any
/// thread exists: changing the environment is sound only while nothing else can read it.
/// Nothing in Rune reads them before `main` (it has no constructors that run earlier).
///
/// The kept ones stay in this process for good: Rune reads `RUNE_READY_DIR` once its first
/// window is ready, when other threads run and `remove_var` would race their reads. Children
/// are spared them through what this returns (sshee-core's `session::set_child_env`).
pub fn scrub_env() -> Scrubbed {
    let scrubbed = sort_out(std::env::vars_os());
    for (name, _) in &scrubbed.removed {
        std::env::remove_var(name);
    }
    scrubbed
}

/// Which of `vars` go and which of the kept ones are set.
fn sort_out(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Scrubbed {
    let mut scrubbed = Scrubbed::default();
    for (name, value) in vars {
        let text = name.to_string_lossy();
        if removed(&text) {
            scrubbed.removed.push((name, value));
        } else if KEPT.contains(&text.to_ascii_uppercase().as_str()) {
            scrubbed.kept.push(name);
        }
    }
    scrubbed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rune_and_telemetry_variables_go_except_the_two_kept() {
        for name in ["RUNE_PERF", "RUNE_A11Y_LOG", "RUNE_FRAME_LOG", "RUNE_SOMETHING_NEW", "OTEL_EXPORTER_OTLP_ENDPOINT", "OTEL_SERVICE_NAME", "rune_perf", "Otel_Service_Name"] {
            assert!(removed(name), "{name}");
        }
        for name in ["RUNE_READY_DIR", "RUNE_GPU_BACKEND", "rune_ready_dir", "HOME", "PATH", "RUNES", "RUNE", "WGPU_ADAPTER_NAME", "MY_RUNE_PERF", "DISPLAY"] {
            assert!(!removed(name), "{name}");
        }
    }

    #[test]
    fn what_goes_is_kept_for_the_children_and_the_kept_switches_are_named() {
        let var = |n: &str, v: &str| (OsString::from(n), OsString::from(v));
        let s = sort_out([var("HOME", "/tmp/p/home"), var("OTEL_SERVICE_NAME", "mine"), var("RUNE_READY_DIR", "/tmp/p/ready"), var("RUNE_PERF", "1")]);
        assert_eq!(s.removed, [var("OTEL_SERVICE_NAME", "mine"), var("RUNE_PERF", "1")]);
        assert_eq!(s.kept, [OsString::from("RUNE_READY_DIR")]);
    }
}
