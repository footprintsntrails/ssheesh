//! Files' pinned and recent folders between runs: `bookmarks.json` in this copy's own folder,
//! in 0.10's own shape (`sshee-bookmarks`). At a start with no file yet, 0.10's are brought
//! over (`main.rs`, from the same read of 0.10's storage the settings come from). 0.10's own
//! store is never written.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use sshee_app::model::Bookmarks;

const FILE: &str = "bookmarks.json";

/// The bookmarks kept in `dir`, if there's a file and it reads. One that doesn't read is moved
/// aside to `bookmarks.damaged.json` (the next save would write over it), and the window starts
/// without bookmarks.
pub fn load(dir: &Path) -> Option<Bookmarks> {
    sshee_app::settings_store::read_json_or_set_aside(&dir.join(FILE))
}

/// Whether this copy keeps bookmarks of its own yet: a file that reads. One that doesn't is
/// moved aside, so 0.10's are brought over instead.
pub fn has(dir: &Path) -> bool {
    load(dir).is_some()
}

/// Writes the bookmarks to `dir`, whole or not at all, private to you: folder names say a lot.
pub fn save(dir: &Path, b: &Bookmarks) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(b).map_err(std::io::Error::other)?;
    sshee_app::settings_store::write_atomic(&dir.join(FILE), &json)
}

/// One thread that writes the file, in the order changes came; of a burst only the latest.
pub struct Writer {
    tx: Option<mpsc::Sender<Bookmarks>>,
}

impl Writer {
    pub fn new(dir: Option<PathBuf>) -> Writer {
        let Some(dir) = dir else { return Writer { tx: None } };
        let (tx, rx) = mpsc::channel::<Bookmarks>();
        let spawned = std::thread::Builder::new().name("bookmarks".into()).spawn(move || {
            while let Ok(mut b) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    b = newer;
                }
                if let Err(e) = save(&dir, &b) {
                    eprintln!("ssheesh-preview: the bookmarks weren't saved: {e}");
                }
            }
        });
        Writer { tx: spawned.ok().map(|_| tx) }
    }

    pub fn save(&self, b: &Bookmarks) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(b.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_are_kept_in_0_10s_shape_and_read_back() {
        let dir = std::env::temp_dir().join(format!("sshee-preview-bookmarks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).is_none() && !has(&dir));
        let mut b = Bookmarks::default();
        sshee_app::files::bookmarks::toggle_pin(&mut b, "host:web", "/srv/www");
        sshee_app::files::bookmarks::visit(&mut b, "host:web", "/etc");
        save(&dir, &b).unwrap();
        assert!(has(&dir));
        assert_eq!(load(&dir), Some(b));
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.contains("\"pins\"") && text.contains("\"recent\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_doesnt_read_is_moved_aside_not_written_over() {
        let dir = std::env::temp_dir().join(format!("sshee-preview-bookmarks-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        // Not "has": 0.10's bookmarks may come over.
        assert!(!has(&dir) && load(&dir).is_none());
        save(&dir, &Bookmarks::default()).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("bookmarks.damaged.json")).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
