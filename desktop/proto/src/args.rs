//! The prototype's own flags. Rune takes its own (`--snap`, `--script`, `--size`, `--help`, ...)
//! out first; these are what's left.

use std::path::PathBuf;

use sshee_ui::terminal::metrics::CellMode;

pub const USAGE: &str = "\
usage: ssheesh-proto [--local | --ssh DESTINATION [--ssh-config FILE] | --replay FIXTURE] [options]
  --local             a shell on this computer (the default)
  --ssh DESTINATION   a shell on a server over ssh: a Host from the ssh config, or user@host
  --ssh-config FILE   the ssh config file ssh reads (passed as -F)
  --replay FIXTURE    play a fixture's output on Rune's clock instead of a session (CI)
  --replay-out FILE   with --replay: record what the terminal writes, resizes and copies
                      (JSON lines)
  --secret-seed N     with --replay: which made-up secrets fill {{SECRET_A}} and {{SECRET_B}}
  --name LABEL        the session's name in the title row
  --cell MODE         how cells snap to pixels: floor (0.10's, the default), round or exact
  --metrics FILE      write measurements to FILE (JSON lines; numbers only)
  --metrics-frames    with --metrics: a record for every frame too
  --hud               show the measurements in the title row
Rune's own flags (--snap, --script, --tree, ...) are listed by --help.";

/// Where the pane's output comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Session {
    Local,
    Ssh { destination: String, config: Option<PathBuf> },
    Replay { fixture: PathBuf, out: Option<PathBuf>, seed: u64 },
}

impl Session {
    /// What the meter calls it.
    pub fn mode(&self) -> &'static str {
        match self {
            Session::Local => "local",
            Session::Ssh { .. } => "ssh",
            Session::Replay { .. } => "replay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub session: Session,
    pub name: Option<String>,
    pub cell: Option<CellMode>,
    pub metrics: Option<PathBuf>,
    pub metrics_frames: bool,
    pub hud: bool,
}

impl Args {
    pub fn parse(argv: &[String]) -> Result<Args, String> {
        let (mut local, mut ssh, mut config, mut name, mut cell) = (false, None, None, None, None);
        let (mut replay, mut replay_out, mut seed, mut metrics) = (None, None, None, None);
        let (mut metrics_frames, mut hud) = (false, false);
        let mut it = argv.iter();
        while let Some(flag) = it.next() {
            let switch = match flag.as_str() {
                "--local" => Some(&mut local),
                "--metrics-frames" => Some(&mut metrics_frames),
                "--hud" => Some(&mut hud),
                _ => None,
            };
            if let Some(on) = switch {
                if std::mem::replace(on, true) {
                    return Err(format!("{flag} is given twice"));
                }
                continue;
            }
            let slot = match flag.as_str() {
                "--ssh" => &mut ssh,
                "--ssh-config" => &mut config,
                "--name" => &mut name,
                "--cell" => &mut cell,
                "--replay" => &mut replay,
                "--replay-out" => &mut replay_out,
                "--secret-seed" => &mut seed,
                "--metrics" => &mut metrics,
                other => return Err(format!("unknown argument {other}")),
            };
            if slot.is_some() {
                return Err(format!("{flag} is given twice"));
            }
            // A value never starts with "-": that's a flag missing its value (and keeps a
            // destination from reading as an ssh option).
            match it.next() {
                Some(value) if !value.is_empty() && !value.starts_with('-') => *slot = Some(value.clone()),
                _ => return Err(format!("{flag} needs a value")),
            }
        }
        if replay.is_none() && (replay_out.is_some() || seed.is_some()) {
            return Err("--replay-out and --secret-seed need --replay".into());
        }
        if metrics.is_none() && metrics_frames {
            return Err("--metrics-frames needs --metrics".into());
        }
        let session = match (local, ssh, replay) {
            (true, Some(_), _) | (true, _, Some(_)) | (_, Some(_), Some(_)) => {
                return Err("--local, --ssh and --replay are different sessions: pick one".into())
            }
            (_, Some(destination), None) => Session::Ssh { destination, config: config.take().map(PathBuf::from) },
            (_, None, Some(fixture)) => {
                let seed = match seed {
                    None => 0,
                    Some(s) => s.parse().map_err(|_| format!("--secret-seed takes a number, not {s}"))?,
                };
                Session::Replay { fixture: fixture.into(), out: replay_out.map(PathBuf::from), seed }
            }
            (_, None, None) => Session::Local,
        };
        if config.is_some() && !matches!(session, Session::Ssh { .. }) {
            return Err("--ssh-config needs --ssh".into());
        }
        let cell = match cell {
            None => None,
            Some(mode) => Some(CellMode::parse(&mode).ok_or_else(|| format!("--cell takes floor, round or exact, not {mode}"))?),
        };
        Ok(Args { session, name, cell, metrics: metrics.map(PathBuf::from), metrics_frames, hud })
    }

    /// What the title row calls the session: `--name`, else "This computer", the destination
    /// as typed, or "Replay".
    pub fn label(&self) -> String {
        match (&self.name, &self.session) {
            (Some(name), _) => name.clone(),
            (None, Session::Local) => "This computer".into(),
            (None, Session::Ssh { destination, .. }) => destination.clone(),
            (None, Session::Replay { .. }) => "Replay".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args, String> {
        Args::parse(&argv.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_local_shell_is_the_default() {
        let args = parse(&[]).unwrap();
        assert_eq!(args, Args { session: Session::Local, name: None, cell: None, metrics: None, metrics_frames: false, hud: false });
        assert_eq!(parse(&["--cell", "round"]).unwrap().cell, Some(CellMode::Round));
        assert_eq!(args.label(), "This computer");
        assert_eq!(parse(&["--local"]).unwrap().session, Session::Local);
    }

    #[test]
    fn an_ssh_session_is_named_as_typed() {
        let args = parse(&["--ssh", "web-7731", "--ssh-config", "/tmp/p/ssh_config"]).unwrap();
        assert_eq!(args.session, Session::Ssh { destination: "web-7731".into(), config: Some("/tmp/p/ssh_config".into()) });
        assert_eq!(args.label(), "web-7731");
        assert_eq!(parse(&["--name", "Build box", "--ssh", "deploy@203.0.113.7"]).unwrap().label(), "Build box");
    }

    #[test]
    fn a_replay_and_the_meter() {
        let args = parse(&["--replay", "f.json", "--replay-out", "w.jsonl", "--secret-seed", "2", "--metrics", "m.jsonl", "--metrics-frames", "--hud"]).unwrap();
        assert_eq!(args.session, Session::Replay { fixture: "f.json".into(), out: Some("w.jsonl".into()), seed: 2 });
        assert_eq!(args.session.mode(), "replay");
        assert_eq!(args.label(), "Replay");
        assert_eq!(args.metrics, Some("m.jsonl".into()));
        assert!(args.metrics_frames && args.hud);
    }

    #[test]
    fn mistakes_are_refused() {
        for argv in [
            &["--ssh"][..],
            &["--ssh", "--name", "x"],
            &["--ssh", "-oProxyCommand=x"],
            &["--name", ""],
            &["--local", "--ssh", "web-7731"],
            &["--local", "--replay", "f.json"],
            &["--ssh", "web-7731", "--replay", "f.json"],
            &["--ssh-config", "/tmp/p/ssh_config"],
            &["--replay", "f.json", "--ssh-config", "/tmp/p/ssh_config"],
            &["--replay-out", "w.jsonl"],
            &["--replay", "f.json", "--secret-seed", "two"],
            &["--metrics-frames"],
            &["--hud", "--hud"],
            &["--ssh", "a", "--ssh", "b"],
            &["--local", "--local"],
            &["--loca"],
            &["web-7731"],
            &["--cell", "half"],
        ] {
            assert!(parse(argv).is_err(), "{argv:?}");
        }
    }
}
