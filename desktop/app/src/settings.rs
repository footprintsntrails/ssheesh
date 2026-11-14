//! This copy's settings: `settings.json` in its own folder (`~/.sshee-preview`), and what its
//! first start begins from. The store and its rules are sshee-app's (`settings_store`); here
//! is only where the file is and what was found of 0.10 on this computer.
//!
//! 0.10's own storage is read (from a copy, by `sshee_app::legacy`) and never written.

use std::path::Path;

use sshee_app::import::Imported;
use sshee_app::legacy::Found;
use sshee_app::settings_store::{OpenOptions, Seed, SettingsStore};
use sshee_core::AppIdentity;

/// Whether Tor was left on in the copy whose folder is `tor_dir` (sshee-core keeps a file
/// named `on` there while it is).
fn tor_left_on(tor_dir: Option<&Path>) -> bool {
    tor_dir.is_some_and(|dir| dir.join("on").exists())
}

/// Whether Tor was left on in 0.10 (`~/.sshee/tor/on`): the import then protects typing
/// rhythm too when it is in doubt.
pub fn tor_left_on_in_0_10() -> bool {
    tor_left_on(AppIdentity::standard().tor_dir().as_deref())
}

/// This copy's settings. Reads one small file; with no folder to keep one in (no home), the
/// settings live in memory for this run.
pub fn open() -> SettingsStore {
    let identity = sshee_core::app_identity::current();
    let options = OpenOptions { tor_on: tor_left_on(identity.tor_dir().as_deref()), ..OpenOptions::default() };
    match identity.dir() {
        Some(dir) => SettingsStore::open(dir, options),
        None => SettingsStore::in_memory(sshee_app::model::Settings::default()),
    }
}

/// What the first start begins from, given what looking for 0.10 turned up: its settings as
/// the import read them (doubts included); a new install's defaults when 0.10 never ran here
/// (`ran_here` false and nothing found); and a doubt when it did but nothing could be had.
pub fn seed(found: &Found, imported: Option<&Imported>, ran_here: bool) -> Seed {
    match imported {
        Some(imported) => Seed::from(imported),
        None if !ran_here && found.problems.is_empty() => Seed::Fresh,
        None => Seed::Doubt("the settings of SSHeesh 0.10 weren't found".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::import::{import_found, ImportOptions};
    use sshee_app::legacy::Places;

    /// A folder of this test's own under the build's temporary folder.
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-preview-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Looking for 0.10 where there is nothing.
    fn nothing_found(name: &str) -> Found {
        let empty = folder(name);
        let places = Places { scratch: Some(empty.join("scratch")), ..Places::linux(&empty.join("data"), &empty.join("config")) };
        sshee_app::legacy::find(&places)
    }

    #[test]
    fn a_computer_0_10_never_ran_on_starts_as_a_new_install() {
        let found = nothing_found("fresh");
        assert!(found.best().is_none());
        assert_eq!(seed(&found, None, false), Seed::Fresh);
    }

    #[test]
    fn traces_of_0_10_without_its_settings_are_a_doubt() {
        let found = nothing_found("traces");
        // Its server list is there, its settings aren't.
        assert!(matches!(seed(&found, None, true), Seed::Doubt(_)));
        // And what the import made of an empty storage is in doubt by its own rules.
        let imported = import_found(&found, &ImportOptions::default());
        assert!(imported.report.privacy.is_some());
        let Seed::Imported { settings, privacy } = seed(&found, Some(&imported), true) else { panic!() };
        assert!(settings.streamer_mode && settings.hide_secrets && privacy.is_some());
    }

    #[test]
    fn tor_left_on_is_a_file_in_the_copys_tor_folder() {
        let dir = folder("tor");
        assert!(!tor_left_on(Some(&dir)) && !tor_left_on(None));
        std::fs::write(dir.join("on"), "").unwrap();
        assert!(tor_left_on(Some(&dir)));
    }
}
