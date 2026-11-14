//! The tabs between runs: `tabs.json` in this copy's own folder (`~/.sshee-preview`). Each
//! terminal's session key is in it, so after a restart a terminal on a server finds its shell
//! in tmux again. 0.10's own stores are never written.

use std::io::Write;
use std::path::Path;

use sshee_app::workbench::SavedTabs;

const FILE: &str = "tabs.json";

/// The tabs saved in `dir`, if there are any and they read. A file that doesn't read is left
/// where it is and the window starts without tabs.
pub fn load(dir: &Path) -> Option<SavedTabs> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes the tabs to `dir`, whole or not at all: to a file beside it first, then over the
/// old one.
pub fn save(dir: &Path, saved: &SavedTabs) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Names of servers and of what runs on them: the user's own business.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        std::fs::rename(&tmp, dir.join(FILE))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::workbench::{Server, TabKind, Workbench};

    /// A folder of this test's own under the build's temporary folder.
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-preview-tabs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn tabs_are_saved_and_read_back() {
        let dir = folder("round");
        assert_eq!(load(&dir), None);
        let mut bench = Workbench::new(vec![Server::host("web", "web-7731", "deploy@203.0.113.17")]);
        bench.go_to("host:web");
        bench.open(TabKind::Files, "host:web");
        save(&dir, &bench.saved()).unwrap();
        assert_eq!(load(&dir), Some(bench.saved()));
        // Saving again replaces it, and leaves no file beside it.
        bench.new_terminal();
        save(&dir, &bench.saved()).unwrap();
        assert_eq!(load(&dir).unwrap().tabs.len(), 3);
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from(FILE)]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_does_not_read_starts_without_tabs_and_is_left_alone() {
        let dir = folder("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        assert_eq!(load(&dir), None);
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
