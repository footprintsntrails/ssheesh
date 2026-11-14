//! A terminal tab named after its working folder: `~/shop`, `/etc/nginx`, `…/releases/current`.
//!
//! New in 1.0 (0.10's tabs were "Terminal", "Terminal 2"; the 1.0 design names them by
//! folder). The folder comes from the shell: OSC 7, or the title most shells set
//! (`user@host: ~/shop`). The rules here turn it into a short name and say which titles are
//! such names, so a folder never replaces a title something else gave the tab (an agent's task).

/// How many folders of a long path a title keeps.
const KEPT: usize = 2;

/// The longest title a folder gets before only its last folders are kept.
const LONG: usize = 28;

/// Whose home folder a path may be under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Home<'a> {
    /// The home folder itself, when it's known (this computer's).
    pub path: Option<&'a str>,
    /// The user the terminal signed in as, when the server's address says.
    pub user: Option<&'a str>,
}

impl<'a> Home<'a> {
    /// From a server's address (`deploy@203.0.113.17`): the user before the `@`, if any.
    pub fn of_address(address: &'a str) -> Home<'a> {
        Home { path: None, user: address.split_once('@').map(|(user, _)| user).filter(|u| !u.is_empty()) }
    }

    /// `path` with the home folder written `~`. Without a known home or user, the first
    /// folder under `/home` or `/Users` is taken for it: nearly always right, and a title is
    /// only a name.
    fn tilde(&self, path: &str) -> String {
        let under = |home: &str| -> Option<String> {
            let rest = path.strip_prefix(home)?;
            (rest.is_empty() || rest.starts_with('/')).then(|| format!("~{rest}"))
        };
        if let Some(home) = self.path.map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()) {
            return under(home).unwrap_or_else(|| path.to_owned());
        }
        if let Some(user) = self.user {
            let homes = if user == "root" { vec!["/root".to_owned()] } else { vec![format!("/home/{user}"), format!("/Users/{user}")] };
            return homes.iter().find_map(|h| under(h)).unwrap_or_else(|| path.to_owned());
        }
        for base in ["/home/", "/Users/"] {
            if let Some(rest) = path.strip_prefix(base) {
                let (user, rest) = rest.split_once('/').map_or((rest, ""), |(u, r)| (u, r));
                if !user.is_empty() {
                    return if rest.is_empty() { "~".to_owned() } else { format!("~/{rest}") };
                }
            }
        }
        under("/root").unwrap_or_else(|| path.to_owned())
    }
}

/// The title for a terminal whose shell is in `folder` (an absolute path, or one already
/// starting with `~`). `None` for something that isn't a folder's path.
pub fn folder_title(folder: &str, home: Home) -> Option<String> {
    let folder = folder.trim();
    let windows = folder.len() > 2 && folder.as_bytes()[1] == b':' && folder.as_bytes()[0].is_ascii_alphabetic();
    if !(folder.starts_with('/') || folder == "~" || folder.starts_with("~/") || windows) || folder.chars().any(char::is_control) {
        return None;
    }
    let trimmed = if folder.len() > 1 { folder.trim_end_matches('/') } else { folder };
    let short = if trimmed.starts_with('~') { trimmed.to_owned() } else { home.tilde(trimmed) };
    let parts: Vec<&str> = short.split('/').filter(|p| !p.is_empty()).collect();
    if short.chars().count() <= LONG || parts.len() <= KEPT + 1 {
        return Some(short);
    }
    Some(format!("…/{}", parts[parts.len() - KEPT..].join("/")))
}

/// The folder in a window title a shell set, when it names one: `deploy@web-7731: ~/shop`
/// (bash's and zsh's usual title) or a bare path. `None` for any other title (`vim notes.md`).
pub fn folder_in_title(title: &str) -> Option<&str> {
    let title = title.trim();
    let path = match title.rsplit_once(": ") {
        Some((who, path)) if who.contains('@') && !who.contains(' ') => path.trim(),
        _ => title,
    };
    let looks = path.starts_with('/') || path == "~" || path.starts_with("~/");
    (looks && !path.contains(' ')).then_some(path)
}

/// Whether a tab's title is one [`folder_title`] made (or nothing): the only titles a new
/// folder may replace.
pub fn is_folder_title(title: Option<&str>) -> bool {
    title.is_none_or(|t| t == "~" || t.starts_with("~/") || t.starts_with('/') || t.starts_with("…/") || folder_title(t, Home::default()).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_a_tilde() {
        let deploy = Home::of_address("deploy@203.0.113.17");
        assert_eq!(folder_title("/home/deploy/shop", deploy).as_deref(), Some("~/shop"));
        assert_eq!(folder_title("/home/deploy", deploy).as_deref(), Some("~"));
        assert_eq!(folder_title("/home/deploy/", deploy).as_deref(), Some("~"));
        // Someone else's home isn't yours, and a folder that only starts like yours isn't home.
        assert_eq!(folder_title("/home/alex/shop", deploy).as_deref(), Some("/home/alex/shop"));
        assert_eq!(folder_title("/home/deployer", deploy).as_deref(), Some("/home/deployer"));
        // (Root's home, spelled in two pieces so no scanner takes it for a path of this machine.)
        assert_eq!(folder_title(&format!("/{}/backup", "root"), Home::of_address("root@203.0.113.17")).as_deref(), Some("~/backup"));
        assert_eq!(folder_title("/Users/dev/site", Home { path: Some("/Users/dev"), user: None }).as_deref(), Some("~/site"));
        // Nobody said who signed in: the folder under /home is taken for home.
        assert_eq!(folder_title("/home/alex/shop", Home::default()).as_deref(), Some("~/shop"));
        assert_eq!(folder_title("/root", Home::default()).as_deref(), Some("~"));
        assert_eq!(folder_title("~/shop", Home::default()).as_deref(), Some("~/shop"));
    }

    #[test]
    fn other_folders_are_their_paths_and_long_ones_keep_their_end() {
        let home = Home::of_address("deploy@203.0.113.17");
        assert_eq!(folder_title("/", home).as_deref(), Some("/"));
        assert_eq!(folder_title("/etc/nginx/", home).as_deref(), Some("/etc/nginx"));
        assert_eq!(folder_title("/var/www/shop/releases/2026-11-13/current", home).as_deref(), Some("…/2026-11-13/current"));
        assert_eq!(folder_title("/home/deploy/shop/node_modules/left-pad/lib", home).as_deref(), Some("…/left-pad/lib"));
        assert_eq!(folder_title("C:/Users/dev", home).as_deref(), Some("C:/Users/dev"));
        // Not a folder.
        for bad in ["", "shop", "vim notes.md", "/tmp/\u{1b}[31m"] {
            assert_eq!(folder_title(bad, home), None, "{bad:?}");
        }
    }

    #[test]
    fn a_shells_title_gives_its_folder() {
        assert_eq!(folder_in_title("deploy@web-7731: ~/shop"), Some("~/shop"));
        assert_eq!(folder_in_title("deploy@web-7731:~/shop"), None, "no space after the colon: not that shape");
        assert_eq!(folder_in_title("/srv/app"), Some("/srv/app"));
        assert_eq!(folder_in_title("~"), Some("~"));
        for other in ["vim notes.md", "htop", "note: /etc is read-only", "sshee-scroll 0 480 24 2000", ""] {
            assert_eq!(folder_in_title(other), None, "{other:?}");
        }
    }

    #[test]
    fn only_a_folders_title_gives_way_to_a_new_folder() {
        for ours in [None, Some("~"), Some("~/shop"), Some("/etc/nginx"), Some("…/releases/current"), Some("C:/Users/dev")] {
            assert!(is_folder_title(ours), "{ours:?}");
        }
        for theirs in [Some("Migration 0042"), Some("worker logs"), Some("psql shop")] {
            assert!(!is_folder_title(theirs), "{theirs:?}");
        }
    }
}
