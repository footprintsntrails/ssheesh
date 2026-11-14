//! Commands kept to run again: `sshee-snippets`, version 1 (`src/lib/snippets.ts`).
//!
//! A snippet's command can hold blanks (`{{name}}`, `{{name:fallback}}`) and, being typed by
//! people, secrets: never print one in a report.

use super::json::JsonMap;
use super::{Platform, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snippets {
    pub snippets: Vec<Snippet>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Default for Snippets {
    /// The five starters 0.10 shows before any snippet is saved.
    fn default() -> Self {
        let starter = |id: &str, name: &str, command: &str| Snippet { id: id.into(), name: name.into(), command: command.into(), extra: JsonMap::new() };
        Snippets {
            snippets: vec![
                starter("s-disk", "How full the disks are", "df -h -x tmpfs -x devtmpfs"),
                starter("s-logs", "Follow a container's log", "docker logs -f --tail {{lines:100}} {{container}}"),
                starter("s-restart", "Restart a service", "sudo systemctl restart {{service}} && systemctl status --no-pager {{service}}"),
                starter("s-ports", "What's listening on which port", "sudo ss -ltnp"),
                starter("s-big", "Biggest folders here", "du -xh --max-depth=1 {{folder:.}} 2>/dev/null | sort -rh | head -n 15"),
            ],
            extra: JsonMap::new(),
        }
    }
}

impl Store for Snippets {
    const KEY: &'static str = "sshee-snippets";
    const VERSION: u64 = 1;
    const SHIPPED: &'static [u64] = &[1];

    fn defaults(_platform: Platform) -> Self {
        Snippets::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(flatten)]
    pub extra: JsonMap,
}
