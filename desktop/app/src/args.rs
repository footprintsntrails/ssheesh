//! SSHeesh Preview's own flags. Rune takes its own (`--snap`, `--script`, `--size`, `--theme`,
//! `--help`, ...) out first; these are what's left.

use std::path::PathBuf;

pub const USAGE: &str = "\
usage: ssheesh-preview [options]
  --demo              start on made-up servers and tabs, with made-up terminal output:
                      nothing is read from this computer and nothing connects
  --sheet             show the component sheet (every control in its states) instead of the window
  --demo-leftovers    with --demo: offer two made-up copies an earlier run left unsent
  --demo-update       with --demo: say a made-up newer test build is ready
  --demo-welcome      with --demo: start with the welcome, as at a first start
  --demo-whats-new    with --demo: start with What's new, as after an update from 0.10.14
  --demo-card STATE   with --demo: the first terminal on backups shows a session card
                      (hostkey-new, hostkey-changed, disconnected, retrying, rejoining,
                      asleep, failed, ended, connecting: a slow sign-in); without it,
                      backups asks to trust its key
  --ssh-config FILE   the ssh config to list servers from and connect with (ssh's -F),
                      instead of ~/.ssh/config
  --tor-check         start this copy's Tor without a window and say each step
                      (first on its own; exits 0 once Tor is on)
Rune's own flags (--snap, --script, --tree, --theme, ...) are listed by --help.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub demo: bool,
    pub sheet: bool,
    pub demo_leftovers: bool,
    pub demo_update: bool,
    pub demo_welcome: bool,
    pub demo_whats_new: bool,
    pub ssh_config: Option<PathBuf>,
    pub demo_card: Option<String>,
}

impl Args {
    /// The flags of this start. `raw` is every argument after the program's name, as the
    /// system gave them; `extra` what's left once Rune took its own.
    ///
    /// A notification's link (Windows starts the program with it, `"exe" "%1"`) is a launch
    /// of its own, parsed strictly: the link is all it may carry. A link that comes with
    /// anything else (a crafted one whose quote splits it into more arguments, say) starts
    /// the plain window, every other argument ignored: never `--ssh-config` or a demo.
    pub fn of_launch(raw: &[String], extra: &[String]) -> Result<Args, String> {
        if raw.iter().any(|a| crate::desktop::is_link(a)) {
            return Ok(Args::default());
        }
        Args::parse(extra)
    }

    pub fn parse(argv: &[String]) -> Result<Args, String> {
        let mut args = Args::default();
        let mut it = argv.iter();
        while let Some(flag) = it.next() {
            let switch = match flag.as_str() {
                "--demo" => &mut args.demo,
                "--sheet" => &mut args.sheet,
                "--demo-leftovers" => &mut args.demo_leftovers,
                "--demo-update" => &mut args.demo_update,
                "--demo-welcome" => &mut args.demo_welcome,
                "--demo-whats-new" => &mut args.demo_whats_new,
                "--demo-card" => {
                    match it.next() {
                        Some(value) if sshee_ui::terminal::replay::status_named(value).is_some() && args.demo_card.is_none() => args.demo_card = Some(value.clone()),
                        _ => return Err(format!("{flag} needs a state, once")),
                    }
                    continue;
                }
                "--ssh-config" => {
                    if args.ssh_config.is_some() {
                        return Err(format!("{flag} is given twice"));
                    }
                    // A value never starts with "-": that's a flag missing its value.
                    match it.next() {
                        Some(value) if !value.is_empty() && !value.starts_with('-') => args.ssh_config = Some(value.into()),
                        _ => return Err(format!("{flag} needs a value")),
                    }
                    continue;
                }
                other => return Err(format!("unknown argument {other}")),
            };
            if std::mem::replace(switch, true) {
                return Err(format!("{flag} is given twice"));
            }
        }
        if (args.demo_welcome || args.demo_whats_new) && !args.demo {
            return Err("--demo-welcome and --demo-whats-new go with --demo".into());
        }
        if args.demo_card.is_some() && !args.demo {
            return Err("--demo-card goes with --demo".into());
        }
        if args.demo_leftovers && !args.demo {
            return Err("--demo-leftovers goes with --demo".into());
        }
        if args.demo_update && !args.demo {
            return Err("--demo-update goes with --demo".into());
        }
        if args.demo && args.ssh_config.is_some() {
            return Err("--demo connects to nothing: it takes no --ssh-config".into());
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args, String> {
        Args::parse(&argv.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn nothing_given_is_the_real_window() {
        assert_eq!(parse(&[]).unwrap(), Args::default());
        assert!(parse(&["--demo"]).unwrap().demo);
        assert!(parse(&["--demo", "--sheet"]).unwrap().sheet);
        assert!(parse(&["--demo", "--demo-leftovers"]).unwrap().demo_leftovers);
        assert!(parse(&["--demo-leftovers"]).is_err());
        assert!(parse(&["--demo", "--demo-update"]).unwrap().demo_update);
        assert!(parse(&["--demo-update"]).is_err());
        assert_eq!(parse(&["--ssh-config", "/tmp/p/ssh_config"]).unwrap().ssh_config, Some("/tmp/p/ssh_config".into()));
    }

    #[test]
    fn a_links_launch_takes_nothing_else() {
        let of = |raw: &[&str], extra: &[&str]| {
            let owned = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
            Args::of_launch(&owned(raw), &owned(extra))
        };
        // A toast's click starts the program with its link, for the window to take.
        let link = "ssheesh-preview://notification/3";
        assert_eq!(of(&[link], &[link]).unwrap(), Args::default());
        // A link split into more arguments: everything but the plain window is ignored.
        let split = [link, "--ssh-config", "/tmp/p/ssh_config", "--demo"];
        assert_eq!(of(&split, &split).unwrap(), Args::default());
        let before = ["--ssh-config", "/tmp/p/ssh_config", "SSHEESH-PREVIEW://notification/3"];
        assert_eq!(of(&before, &before).unwrap(), Args::default());
        // Without a link the flags are read as ever, and a link is no flag of theirs.
        assert!(of(&["--demo"], &["--demo"]).unwrap().demo);
        assert!(parse(&[link]).is_err());
    }

    #[test]
    fn mistakes_are_refused() {
        for argv in [
            &["--demo", "--demo"][..],
            &["--ssh-config"],
            &["--ssh-config", "--demo"],
            &["--ssh-config", "a", "--ssh-config", "b"],
            &["--demo", "--ssh-config", "a"],
            &["--dem"],
            &["web-7731"],
        ] {
            assert!(parse(argv).is_err(), "{argv:?}");
        }
    }
}
