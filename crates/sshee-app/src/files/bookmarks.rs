//! Pinned and recently visited folders, per machine: the rules of 0.10's src/lib/bookmarks.ts
//! on [`crate::model::Bookmarks`] (the store 0.10 keeps as `sshee-bookmarks`), and the places
//! every machine has (the Places list of `PlacesPopover.tsx`).

use crate::model::Bookmarks;

/// How many recent folders are kept per machine.
pub const MAX_RECENT: usize = 6;

/// A machine's pinned folders, in the order they were added.
pub fn pins<'a>(b: &'a Bookmarks, server: &str) -> &'a [String] {
    b.pins.get(server).map_or(&[], Vec::as_slice)
}

/// A machine's recently opened folders, newest first.
pub fn recent<'a>(b: &'a Bookmarks, server: &str) -> &'a [String] {
    b.recent.get(server).map_or(&[], Vec::as_slice)
}

pub fn is_pinned(b: &Bookmarks, server: &str, path: &str) -> bool {
    pins(b, server).iter().any(|p| p == path)
}

/// Pins a folder, or takes its pin away. True when it's pinned now.
pub fn toggle_pin(b: &mut Bookmarks, server: &str, path: &str) -> bool {
    let list = b.pins.entry(server.to_owned()).or_default();
    match list.iter().position(|p| p == path) {
        Some(at) => {
            list.remove(at);
            false
        }
        None => {
            list.push(path.to_owned());
            true
        }
    }
}

/// Takes a pin away ("Remove bookmark" in the list of bookmarks).
pub fn remove_pin(b: &mut Bookmarks, server: &str, path: &str) {
    if let Some(list) = b.pins.get_mut(server) {
        list.retain(|p| p != path);
    }
}

/// A folder was opened: it's the newest of the recent ones, of which six are kept. True when
/// anything changed (so the store is only written then).
pub fn visit(b: &mut Bookmarks, server: &str, path: &str) -> bool {
    let list = b.recent.entry(server.to_owned()).or_default();
    if list.first().is_some_and(|p| p == path) {
        return false;
    }
    list.retain(|p| p != path);
    list.insert(0, path.to_owned());
    list.truncate(MAX_RECENT);
    true
}

/// "Remove from recent".
pub fn forget_recent(b: &mut Bookmarks, server: &str, path: &str) {
    if let Some(list) = b.recent.get_mut(server) {
        list.retain(|p| p != path);
    }
}

/// What 0.10 kept, brought into what this copy keeps: its pins after this copy's own (each
/// once), and its recent folders after this copy's, six at most. True when anything changed.
pub fn merge(into: &mut Bookmarks, from: &Bookmarks) -> bool {
    let before = into.clone();
    for (server, pins) in &from.pins {
        let list = into.pins.entry(server.clone()).or_default();
        for p in pins {
            if !list.contains(p) {
                list.push(p.clone());
            }
        }
    }
    for (server, recent) in &from.recent {
        let list = into.recent.entry(server.clone()).or_default();
        for p in recent {
            if !list.contains(p) {
                list.push(p.clone());
            }
        }
        list.truncate(MAX_RECENT);
    }
    *into != before
}

/// The recent folders the list of places shows: not those pinned, not the one you're in, five
/// at most.
pub fn recent_shown(b: &Bookmarks, server: &str, dir: &str) -> Vec<String> {
    let pins = pins(b, server);
    recent(b, server).iter().filter(|p| !pins.contains(p) && *p != dir).take(5).cloned().collect()
}

/// The id that hides the recent folders in the list of places (Settings' `hiddenPlaces`).
pub const RECENT_ID: &str = "recent";

/// A place, or the recent folders ([`RECENT_ID`]), in or out of the list of places: what
/// Settings keeps as `hiddenPlaces`.
pub fn toggle_hidden(hidden: &mut Vec<String>, id: &str) {
    match hidden.iter().position(|h| h == id) {
        Some(at) => drop(hidden.remove(at)),
        None => hidden.push(id.to_owned()),
    }
}

/// A folder worth one click on any machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Fixed, for keys and for hiding a place in Settings: "home", "root", or the path.
    pub id: &'static str,
    pub label: &'static str,
    pub path: String,
    /// The tooltip.
    pub title: String,
}

/// Well-known Linux folders, offered on servers.
pub const LINUX_PLACES: [(&str, &str); 5] = [("/var/log", "Logs"), ("/etc", "Config"), ("/var/www", "Web"), ("/opt", "Apps"), ("/tmp", "Temp")];

/// The Places list: Home, Disk, and on a server the well-known Linux folders. `home` is the
/// home folder's full path when it's known (a "~" listing told it); else Home goes to "~".
pub fn places(on_server: bool, home: Option<&str>) -> Vec<Place> {
    let mut list = vec![
        Place { id: "home", label: "Home", path: home.unwrap_or("~").to_owned(), title: home.map_or("Your home folder".to_owned(), |h| format!("Your home folder: {h}")) },
        // "Root" would read like the root user's folder; this is the top of the disk.
        Place { id: "root", label: "Disk", path: "/".into(), title: "/ — the top of the whole disk".into() },
    ];
    if on_server {
        list.extend(LINUX_PLACES.iter().map(|(path, label)| Place { id: path, label, path: (*path).to_owned(), title: (*path).to_owned() }));
    }
    list
}

/// The places the list shows: [`places`] without those hidden.
pub fn shown_places(on_server: bool, home: Option<&str>, hidden: &[String]) -> Vec<Place> {
    places(on_server, home).into_iter().filter(|p| !hidden.iter().any(|h| h == p.id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_0_10_kept_comes_after_what_this_copy_has() {
        let mut here = Bookmarks::default();
        toggle_pin(&mut here, WEB, "/srv/app");
        visit(&mut here, WEB, "/srv/app");
        let mut old = Bookmarks::default();
        toggle_pin(&mut old, WEB, "/etc");
        toggle_pin(&mut old, WEB, "/srv/app");
        toggle_pin(&mut old, "local", "/home/dev");
        for n in 1..=7 {
            visit(&mut old, WEB, &format!("/d/{n}"));
        }
        assert!(merge(&mut here, &old));
        assert_eq!(pins(&here, WEB), ["/srv/app", "/etc"]);
        assert_eq!(pins(&here, "local"), ["/home/dev"]);
        assert_eq!(recent(&here, WEB), ["/srv/app", "/d/7", "/d/6", "/d/5", "/d/4", "/d/3"]);
        // Twice changes nothing.
        assert!(!merge(&mut here, &old));
    }

    #[test]
    fn places_can_be_hidden_and_recent_folders_skip_pins_and_where_you_are() {
        let mut hidden = Vec::new();
        toggle_hidden(&mut hidden, "/var/log");
        toggle_hidden(&mut hidden, RECENT_ID);
        assert_eq!(hidden, ["/var/log", "recent"]);
        let shown = shown_places(true, None, &hidden);
        assert!(!shown.iter().any(|p| p.id == "/var/log") && shown.len() == 6);
        toggle_hidden(&mut hidden, "/var/log");
        assert_eq!(hidden, ["recent"]);
        let mut b = Bookmarks::default();
        for p in ["/a", "/b", "/c", "/d", "/e", "/f"] {
            visit(&mut b, WEB, p);
        }
        toggle_pin(&mut b, WEB, "/e");
        assert_eq!(recent_shown(&b, WEB, "/f"), ["/d", "/c", "/b", "/a"]);
        assert_eq!(recent_shown(&b, WEB, "/x").len(), 5);
    }

    const WEB: &str = "host:web";

    #[test]
    fn pins_toggle_and_keep_the_order_they_were_added() {
        let mut b = Bookmarks::default();
        assert!(pins(&b, WEB).is_empty());
        assert!(toggle_pin(&mut b, WEB, "/srv/app"));
        assert!(toggle_pin(&mut b, WEB, "/etc"));
        assert!(toggle_pin(&mut b, "local", "/home/dev"));
        assert_eq!(pins(&b, WEB), ["/srv/app", "/etc"]);
        assert!(is_pinned(&b, WEB, "/etc") && !is_pinned(&b, "local", "/etc"));
        assert!(!toggle_pin(&mut b, WEB, "/srv/app"));
        assert_eq!(pins(&b, WEB), ["/etc"]);
        remove_pin(&mut b, WEB, "/etc");
        remove_pin(&mut b, "host:none", "/etc");
        assert!(pins(&b, WEB).is_empty());
        assert_eq!(pins(&b, "local"), ["/home/dev"]);
    }

    #[test]
    fn six_recent_folders_newest_first_each_once() {
        let mut b = Bookmarks::default();
        for n in 1..=8 {
            assert!(visit(&mut b, WEB, &format!("/d/{n}")));
        }
        assert_eq!(recent(&b, WEB), ["/d/8", "/d/7", "/d/6", "/d/5", "/d/4", "/d/3"]);
        // The newest again changes nothing; an older one moves to the front.
        assert!(!visit(&mut b, WEB, "/d/8"));
        assert!(visit(&mut b, WEB, "/d/5"));
        assert_eq!(recent(&b, WEB), ["/d/5", "/d/8", "/d/7", "/d/6", "/d/4", "/d/3"]);
        forget_recent(&mut b, WEB, "/d/7");
        assert_eq!(recent(&b, WEB).len(), 5);
        assert!(recent(&b, "local").is_empty());
    }

    #[test]
    fn places_of_a_server_and_of_this_computer() {
        let server = places(true, Some("/home/deploy"));
        assert_eq!(server.iter().map(|p| p.label).collect::<Vec<_>>(), ["Home", "Disk", "Logs", "Config", "Web", "Apps", "Temp"]);
        assert_eq!((server[0].path.as_str(), server[0].title.as_str()), ("/home/deploy", "Your home folder: /home/deploy"));
        assert_eq!((server[2].id, server[2].path.as_str()), ("/var/log", "/var/log"));
        let here = places(false, None);
        assert_eq!(here.iter().map(|p| (p.id, p.path.as_str())).collect::<Vec<_>>(), [("home", "~"), ("root", "/")]);
        assert_eq!(here[0].title, "Your home folder");
    }
}
