//! Folders seen lately, per machine. Going back, up, or into a folder seen a moment ago shows
//! its last listing at once while a fresh one loads. A port of 0.10's src/lib/dirCache.ts as
//! a plain struct: the window owns one, and the time is passed in.
//!
//! What 0.10's cache did with promises isn't here: a request on its way is told from a newer
//! one by its ticket ([`crate::files::Browser`]), and telling whoever shows a folder that it
//! changed is the screen's job, since it holds the tabs.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::files::{Clock, Listing};
use crate::paths::within;

/// A listing is shown without asking again for this long (0.10's `FRESH_MS`).
pub const FRESH_MS: u64 = 3_000;
/// The most folders kept; the ones seen longest ago go first.
pub const MAX: usize = 300;

/// A folder as it was last seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    /// Shared: a listing that didn't change stays the same allocation, so whoever shows it
    /// can tell by pointer that there's nothing to sort or draw again.
    pub listing: Arc<Listing>,
    /// When it arrived, in milliseconds on the window's clock.
    pub at: u64,
    /// When it was taken on this computer, for the list's dates.
    pub clock: Clock,
}

/// The cache: listings by machine and path, under the path asked for and the one it turned
/// out to be ("~" and "/home/deploy").
#[derive(Debug, Default)]
pub struct DirCache {
    /// Oldest first.
    listings: IndexMap<(String, String), Cached>,
}

impl DirCache {
    pub fn new() -> DirCache {
        DirCache::default()
    }

    /// The last listing of a folder, under the path asked or the one it resolved to.
    pub fn get(&self, server: &str, path: &str) -> Option<&Cached> {
        self.listings.get(&(server.to_owned(), path.to_owned()))
    }

    /// Whether a folder's listing is new enough to show without asking again.
    pub fn is_fresh(&self, server: &str, path: &str, now_ms: u64) -> bool {
        self.get(server, path).is_some_and(|c| now_ms.saturating_sub(c.at) < FRESH_MS)
    }

    /// Keeps a listing that just arrived for the folder `asked`, and returns the one to show:
    /// the listing already kept when nothing changed, so nothing that shows it has to sort or
    /// draw again.
    pub fn remember(&mut self, server: &str, asked: &str, listing: Listing, clock: Clock, now_ms: u64) -> Arc<Listing> {
        let old = self.get(server, &listing.path).map(|c| c.listing.clone());
        let kept = match old {
            Some(old) if *old == listing => old,
            _ => Arc::new(listing),
        };
        let resolved = kept.path.clone();
        for path in [asked, resolved.as_str()] {
            let key = (server.to_owned(), path.to_owned());
            // Taken out first: the newest is last, whatever place it had.
            self.listings.shift_remove(&key);
            self.listings.insert(key, Cached { listing: kept.clone(), at: now_ms, clock });
        }
        while self.listings.len() > MAX {
            self.listings.shift_remove_index(0);
        }
        kept
    }

    fn drop_where(&mut self, server: &str, matches: impl Fn(&str) -> bool) {
        self.listings.retain(|(s, path), c| s != server || !(matches(path) || matches(&c.listing.path)));
    }

    /// Forgets a folder and everything in it (deleted, renamed, moved).
    pub fn forget(&mut self, server: &str, path: &str) {
        self.drop_where(server, |p| within(p, path));
    }

    /// The app changed something in `dir`: its listing is no longer what's there. The folders
    /// inside it stay.
    pub fn changed(&mut self, server: &str, dir: &str) {
        self.drop_where(server, |p| p == dir);
    }

    pub fn len(&self) -> usize {
        self.listings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.listings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::{dir, file, listing};

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    fn home() -> Listing {
        listing("/home/deploy", vec![dir("shop"), file("README.md", 812, 1_700_000_000)])
    }

    #[test]
    fn a_listing_is_kept_under_the_path_asked_and_the_one_it_is() {
        let mut cache = DirCache::new();
        let shown = cache.remember(WEB, "~", home(), Clock { now: 5, utc_offset: 3_600 }, 1_000);
        let by_asked = cache.get(WEB, "~").unwrap();
        assert!(Arc::ptr_eq(&by_asked.listing, &shown));
        assert_eq!((by_asked.at, by_asked.clock), (1_000, Clock { now: 5, utc_offset: 3_600 }));
        assert!(Arc::ptr_eq(&cache.get(WEB, "/home/deploy").unwrap().listing, &shown));
        // Another machine's folder of the same name is another folder.
        assert!(cache.get(DB, "~").is_none());
        assert!(cache.get(WEB, "/home").is_none());
    }

    #[test]
    fn nothing_changed_keeps_the_old_listing_and_a_change_replaces_it() {
        let mut cache = DirCache::new();
        let first = cache.remember(WEB, "~", home(), Clock::default(), 1_000);
        let again = cache.remember(WEB, "/home/deploy", home(), Clock { now: 9, utc_offset: 0 }, 2_000);
        assert!(Arc::ptr_eq(&first, &again));
        // The time it was seen is the new one, under the name asked for this time.
        assert_eq!(cache.get(WEB, "~").unwrap().at, 1_000);
        assert_eq!(cache.get(WEB, "/home/deploy").unwrap().at, 2_000);
        assert_eq!(cache.get(WEB, "/home/deploy").unwrap().clock.now, 9);

        let mut grown = home();
        grown.entries.push(file("notes.txt", 3, 1_700_000_100));
        let third = cache.remember(WEB, "/home/deploy", grown.clone(), Clock::default(), 3_000);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(*third, grown);
        // One field of one entry is a change too.
        let mut touched = grown.clone();
        touched.entries[2].modified += 1;
        assert!(!Arc::ptr_eq(&third, &cache.remember(WEB, "/home/deploy", touched, Clock::default(), 3_500)));
    }

    #[test]
    fn fresh_for_three_seconds() {
        let mut cache = DirCache::new();
        cache.remember(WEB, "/srv", listing("/srv", vec![]), Clock::default(), 10_000);
        assert!(cache.is_fresh(WEB, "/srv", 10_000) && cache.is_fresh(WEB, "/srv", 12_999));
        assert!(!cache.is_fresh(WEB, "/srv", 13_000));
        assert!(!cache.is_fresh(WEB, "/etc", 10_000));
        // A clock that went back never makes a listing stale.
        assert!(cache.is_fresh(WEB, "/srv", 9_000));
    }

    #[test]
    fn forgetting_takes_the_folder_and_all_inside_it_and_changed_only_the_folder() {
        let mut cache = DirCache::new();
        for path in ["/srv", "/srv/app", "/srv/app/logs", "/srv/application", "/etc"] {
            cache.remember(WEB, path, listing(path, vec![]), Clock::default(), 0);
        }
        cache.remember(DB, "/srv/app", listing("/srv/app", vec![]), Clock::default(), 0);
        cache.changed(WEB, "/srv/app");
        assert!(cache.get(WEB, "/srv/app").is_none() && cache.get(WEB, "/srv/app/logs").is_some());
        cache.remember(WEB, "/srv/app", listing("/srv/app", vec![]), Clock::default(), 0);
        cache.forget(WEB, "/srv/app");
        assert!(cache.get(WEB, "/srv/app").is_none() && cache.get(WEB, "/srv/app/logs").is_none());
        assert!(cache.get(WEB, "/srv").is_some() && cache.get(WEB, "/srv/application").is_some() && cache.get(WEB, "/etc").is_some());
        assert!(cache.get(DB, "/srv/app").is_some());
    }

    #[test]
    fn a_folder_kept_under_another_name_goes_with_it() {
        let mut cache = DirCache::new();
        cache.remember(WEB, "~", home(), Clock::default(), 0);
        cache.changed(WEB, "/home/deploy");
        assert!(cache.get(WEB, "~").is_none() && cache.is_empty());
        cache.remember(WEB, "~/shop", listing("/home/deploy/shop", vec![]), Clock::default(), 0);
        cache.forget(WEB, "/home/deploy");
        assert!(cache.is_empty());
    }

    #[test]
    fn at_most_three_hundred_folders_the_oldest_go_first() {
        let mut cache = DirCache::new();
        for n in 0..MAX + 5 {
            let path = format!("/d/{n}");
            cache.remember(WEB, &path, listing(&path, vec![]), Clock::default(), n as u64);
        }
        assert_eq!(cache.len(), MAX);
        assert!(cache.get(WEB, "/d/4").is_none() && cache.get(WEB, "/d/5").is_some());
        // Seen again, a folder is the newest.
        cache.remember(WEB, "/d/5", listing("/d/5", vec![]), Clock::default(), 999);
        cache.remember(WEB, "/d/new", listing("/d/new", vec![]), Clock::default(), 999);
        assert!(cache.get(WEB, "/d/5").is_some() && cache.get(WEB, "/d/6").is_none());
    }
}
