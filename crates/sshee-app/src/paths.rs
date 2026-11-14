//! Paths on a machine, as Files shows and walks them: the folders of the address bar, the
//! folder above, a name joined to a folder. A port of 0.10's `pathCrumbs` (src/lib/paths.ts)
//! and `joinPath`, `parentPath` (src/lib/format.ts), `folderName` (src/lib/bookmarks.ts).
//!
//! A server's paths use `/`. This computer's on Windows arrive with `/` too (sshee-core lists
//! `C:/Users/...`), so a drive is the one other kind of root there is.

/// One folder of the address bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    /// What it says: "/", "~", "C:", or the folder's name.
    pub label: String,
    /// The folder a click on it opens.
    pub path: String,
}

impl Crumb {
    /// The start of a path: the top of the disk, the home folder, or a drive.
    pub fn is_root(&self) -> bool {
        self.label == "/" || self.label == "~" || self.label.ends_with(':')
    }
}

/// `C:` when `path` starts with a drive (`C:`, `C:/...`).
fn drive(path: &str) -> Option<&str> {
    let b = path.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/')).then(|| &path[..2])
}

/// "/srv/app/logs" → "/", "srv", "app", "logs", each with the folder it opens.
pub fn crumbs(path: &str) -> Vec<Crumb> {
    let mut out = Vec::new();
    let rest = if let Some(d) = drive(path) {
        out.push(Crumb { label: d.to_owned(), path: format!("{d}/") });
        &path[2..]
    } else if path == "~" || path.starts_with("~/") {
        out.push(Crumb { label: "~".into(), path: "~".into() });
        &path[1..]
    } else if path.starts_with('/') {
        out.push(Crumb { label: "/".into(), path: "/".into() });
        path
    } else {
        path
    };
    let mut at = out.first().map(|c| c.path.clone()).unwrap_or_default();
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        at = join(&at, part);
        out.push(Crumb { label: part.to_owned(), path: at.clone() });
    }
    out
}

/// `name` inside `dir`.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder above `path`; the top of the disk and a drive are their own.
pub fn parent(path: &str) -> String {
    if let Some(d) = drive(path) {
        let rest = path[2..].trim_end_matches('/');
        return match rest.rfind('/') {
            Some(i) if i > 0 => format!("{d}{}", &rest[..i]),
            _ => format!("{d}/"),
        };
    }
    let p = path.trim_end_matches('/');
    match p.rfind('/') {
        Some(i) if i > 0 => p[..i].to_owned(),
        Some(_) => "/".into(),
        // "~" and a bare name have nothing above them that can be told from here.
        None if p.is_empty() => "/".into(),
        None => p.to_owned(),
    }
}

/// Whether a folder has one above it to go to: every folder but the top of the disk or of a
/// drive. ("~" has: the server answers a listing with the home folder's full path.)
pub fn has_parent(dir: &str) -> bool {
    if dir == "/" || dir == "~" || dir.is_empty() {
        return false;
    }
    match drive(dir) {
        Some(_) => dir.len() > 3,
        None => true,
    }
}

/// The last part of a path: a file's or a folder's own name. "/" for the top of the disk.
pub fn name(path: &str) -> &str {
    if path == "/" {
        return "/";
    }
    let p = path.trim_end_matches('/');
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Whether `path` is `dir` or inside it.
pub fn within(path: &str, dir: &str) -> bool {
    path == dir || path.strip_prefix(dir).is_some_and(|rest| dir.ends_with('/') || rest.starts_with('/'))
}

/// A file's name on this computer, where a path may be split by `\` (Windows): for "Uploaded
/// notes.txt".
pub fn local_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(path: &str) -> Vec<(String, String)> {
        crumbs(path).into_iter().map(|c| (c.label, c.path)).collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn a_path_splits_into_the_folders_it_goes_through() {
        assert_eq!(labels("/srv/app/logs"), pairs(&[("/", "/"), ("srv", "/srv"), ("app", "/srv/app"), ("logs", "/srv/app/logs")]));
        assert_eq!(labels("/"), pairs(&[("/", "/")]));
        assert_eq!(labels("~"), pairs(&[("~", "~")]));
        assert_eq!(labels("~/shop/releases"), pairs(&[("~", "~"), ("shop", "~/shop"), ("releases", "~/shop/releases")]));
        assert_eq!(labels("C:/Users/dev"), pairs(&[("C:", "C:/"), ("Users", "C:/Users"), ("dev", "C:/Users/dev")]));
        assert_eq!(labels("C:"), pairs(&[("C:", "C:/")]));
        // Doubled and trailing slashes are no folders.
        assert_eq!(labels("/srv//app/"), pairs(&[("/", "/"), ("srv", "/srv"), ("app", "/srv/app")]));
        // A path that starts nowhere known keeps its parts.
        assert_eq!(labels("shop/releases"), pairs(&[("shop", "shop"), ("releases", "shop/releases")]));
        assert!(crumbs("/srv")[0].is_root() && !crumbs("/srv")[1].is_root());
        assert!(crumbs("C:/x")[0].is_root() && crumbs("~/x")[0].is_root());
    }

    #[test]
    fn joining_and_going_up() {
        assert_eq!(join("/srv", "app"), "/srv/app");
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("C:/", "Users"), "C:/Users");
        assert_eq!(parent("/srv/app/logs"), "/srv/app");
        assert_eq!(parent("/srv/"), "/");
        assert_eq!(parent("/srv"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(parent("C:/Users/dev"), "C:/Users");
        assert_eq!(parent("C:/Users"), "C:/");
        assert_eq!(parent("C:/"), "C:/");
        assert_eq!(parent("~/shop"), "~");
    }

    #[test]
    fn every_folder_but_the_top_has_one_above() {
        assert!(has_parent("/srv") && has_parent("/home/deploy/shop") && has_parent("C:/Users"));
        assert!(!has_parent("/") && !has_parent("C:/") && !has_parent("C:") && !has_parent("~"));
    }

    #[test]
    fn names_and_what_is_inside_what() {
        assert_eq!(name("/srv/app/logs"), "logs");
        assert_eq!(name("/srv/app/"), "app");
        assert_eq!(name("/"), "/");
        assert_eq!(name("notes.md"), "notes.md");
        assert!(within("/srv/app", "/srv/app") && within("/srv/app/logs", "/srv/app") && within("/etc", "/"));
        assert!(!within("/srv/application", "/srv/app") && !within("/srv", "/srv/app"));
        assert_eq!(local_name("C:\\Users\\dev\\notes.txt"), "notes.txt");
        assert_eq!(local_name("/home/dev/photos/"), "photos");
    }
}
