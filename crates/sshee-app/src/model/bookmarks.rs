//! Folder bookmarks and recently visited folders, per machine: `sshee-bookmarks`, version 1
//! (`src/lib/bookmarks.ts`).

use super::json::JsonMap;
use super::{Platform, Store};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bookmarks {
    /// Pinned folders by target key, in the order they were added.
    pub pins: IndexMap<String, Vec<String>>,
    /// Recently opened folders by target key, newest first (6 at most).
    pub recent: IndexMap<String, Vec<String>>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Store for Bookmarks {
    const KEY: &'static str = "sshee-bookmarks";
    const VERSION: u64 = 1;
    const SHIPPED: &'static [u64] = &[1];

    fn defaults(_platform: Platform) -> Self {
        Bookmarks::default()
    }
}
