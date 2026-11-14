//! SSHeesh Preview's own flags. Rune takes its own (`--snap`, `--script`, `--size`, `--theme`,
//! `--help`, ...) out first; these are what's left.

use std::path::PathBuf;

pub const USAGE: &str = "\
usage: ssheesh-preview [options]
  --demo              start on made-up servers and tabs, with made-up terminal output:
                      nothing is read from this computer and nothing connects
  --sheet             show the component sheet (every control in its states) instead of the window
  --ssh-config FILE   the ssh config to list servers from and connect with (ssh's -F),
                      instead of ~/.ssh/config
Rune's own flags (--snap, --script, --tree, --theme, ...) are listed by --help.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub demo: bool,
    pub sheet: bool,
    pub ssh_config: Option<PathBuf>,
}

impl Args {
    pub fn parse(argv: &[String]) -> Result<Args, String> {
        let mut args = Args::default();
        let mut it = argv.iter();
        while let Some(flag) = it.next() {
            let switch = match flag.as_str() {
                "--demo" => &mut args.demo,
                "--sheet" => &mut args.sheet,
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
        assert_eq!(parse(&["--ssh-config", "/tmp/p/ssh_config"]).unwrap().ssh_config, Some("/tmp/p/ssh_config".into()));
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
