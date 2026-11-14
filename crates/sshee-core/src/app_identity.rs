//! Which copy of SSHeesh this process is: where the copy keeps its own files
//! and what it calls itself in the keychain.
//!
//! A Preview can run next to the released app. Each copy keeps its shared
//! connections (`cm`), the host keys it's checking (`hostkeys`) and Tor
//! (`tor`) in a folder of its own, and its saved passwords under a keychain
//! name of its own, so neither touches the other's. The copy every version
//! before 1.0 was is `standard`: `~/.sshee` and the app's id, which used to
//! be written into the code.
//!
//! Two processes of the same copy share its folder: 1.0 starting while 0.10
//! is on its way out during the update. They're the same person's settings,
//! so that's as it should be; what one process mustn't take from the other,
//! Tor's door, is its own (see `tor::door_file`).
//!
//! The identity is set once, first thing in `main` (`set`), before the
//! executable checks whether ssh started it as a helper: the Tor helper
//! finds the app's door through it (see `tor::proxy_main`). So it has to come
//! from something every role of the executable sees the same way, such as
//! how the program was built, and not from a choice made later on screen.
//! Nothing set means `standard`, so an app that never sets one behaves as
//! every version before.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The folder in your home folder that the standard copy keeps its files in.
pub const STANDARD_FOLDER: &str = ".sshee";
/// The keychain name the standard copy files saved passwords under: the
/// app's id. Every version so far saved them here, so changing it would lose
/// them.
pub const STANDARD_KEYCHAIN_SERVICE: &str = "io.github.footprintsntrails.ssheesh";
/// The folder a Preview copy keeps its files in, beside the standard one.
pub const PREVIEW_FOLDER: &str = ".sshee-preview";
/// The keychain name a Preview copy files saved passwords under.
pub const PREVIEW_KEYCHAIN_SERVICE: &str = "io.github.footprintsntrails.ssheesh.preview";

/// The passport of a running copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    /// The copy's own folder. None when there's no home folder to put it in:
    /// then Tor doesn't start and host keys can't be checked, as before.
    dir: Option<PathBuf>,
    /// Where ssh's shared connections live. Never None: without a home
    /// folder they go under /tmp, as they always did.
    control_dir: PathBuf,
    keychain_service: String,
}

impl AppIdentity {
    /// The copy every version before 1.0 was: `~/.sshee` and the app's id.
    pub fn standard() -> Self {
        Self::in_home(STANDARD_FOLDER, STANDARD_KEYCHAIN_SERVICE)
    }

    /// A Preview copy that runs beside the standard one without sharing
    /// its files or saved passwords.
    pub fn preview() -> Self {
        Self::in_home(PREVIEW_FOLDER, PREVIEW_KEYCHAIN_SERVICE)
    }

    /// A copy with `folder` (one name, like ".sshee") in your home folder.
    pub fn in_home(folder: &str, keychain_service: &str) -> Self {
        debug_assert!(!folder.is_empty() && !folder.contains(['/', '\\']) && folder != "..", "one folder name: {folder}");
        let home = dirs::home_dir();
        // Kept short on purpose: unix socket paths are limited to ~104 bytes.
        let control_dir = home.clone().unwrap_or_else(|| PathBuf::from("/tmp")).join(folder).join("cm");
        Self { dir: home.map(|h| h.join(folder)), control_dir, keychain_service: keychain_service.into() }
    }

    /// A copy whose folder is `dir` itself: a portable copy, or a test.
    pub fn in_dir(dir: PathBuf, keychain_service: &str) -> Self {
        Self { control_dir: dir.join("cm"), dir: Some(dir), keychain_service: keychain_service.into() }
    }

    /// The copy's own folder, if there's one.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The name the keychain (Keychain, Credential Manager, Secret Service)
    /// files this copy's saved passwords under.
    pub fn keychain_service(&self) -> &str {
        &self.keychain_service
    }

    /// Where ssh's shared connections live (see `SshEnv::control_dir`).
    pub fn control_dir(&self) -> PathBuf {
        self.control_dir.clone()
    }

    /// Where a server's keys are fetched to while you look at them.
    pub fn hostkeys_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("hostkeys"))
    }

    /// Where Tor keeps what it learned, whether it was left on, its log and
    /// the doors of this copy's processes (see `tor`).
    pub fn tor_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("tor"))
    }
}

impl Default for AppIdentity {
    fn default() -> Self {
        Self::standard()
    }
}

static CURRENT: OnceLock<AppIdentity> = OnceLock::new();

/// Makes `identity` this process's, for the rest of its life. Call it first
/// thing in `main`: only the first call counts, and only before anything
/// asked for `current`. When it comes too late, the identity is handed back.
pub fn set(identity: AppIdentity) -> Result<(), AppIdentity> {
    CURRENT.set(identity)
}

/// This process's identity: the one `set`, or else the standard one.
pub fn current() -> &'static AppIdentity {
    CURRENT.get_or_init(AppIdentity::standard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_copy_is_where_every_version_kept_its_files() {
        let id = AppIdentity::standard();
        assert_eq!(id.keychain_service(), "io.github.footprintsntrails.ssheesh");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(id.dir(), Some(home.join(".sshee").as_path()));
            assert_eq!(id.control_dir(), home.join(".sshee").join("cm"));
            assert_eq!(id.hostkeys_dir(), Some(home.join(".sshee").join("hostkeys")));
            assert_eq!(id.tor_dir(), Some(home.join(".sshee").join("tor")));
        }
        // Nothing set in tests, so the process is the standard copy.
        assert_eq!(current(), &id);
    }

    #[test]
    fn two_copies_share_no_folder_and_no_keychain_name() {
        let base = std::env::temp_dir().join(format!("sshee-identity-{}", std::process::id()));
        let a = AppIdentity::in_dir(base.join("a"), STANDARD_KEYCHAIN_SERVICE);
        let b = AppIdentity::in_dir(base.join("b"), PREVIEW_KEYCHAIN_SERVICE);
        let folders = |id: &AppIdentity| vec![id.control_dir(), id.hostkeys_dir().unwrap(), id.tor_dir().unwrap()];
        for mine in folders(&a) {
            for theirs in folders(&b) {
                assert!(!mine.starts_with(&theirs) && !theirs.starts_with(&mine), "{mine:?} and {theirs:?}");
            }
        }
        assert_ne!(a.keychain_service(), b.keychain_service());
        let (standard, preview) = (AppIdentity::standard(), AppIdentity::preview());
        assert_ne!(standard.control_dir(), preview.control_dir());
        assert_ne!(standard.tor_dir(), preview.tor_dir());
        assert_ne!(standard.keychain_service(), preview.keychain_service());
    }
}
