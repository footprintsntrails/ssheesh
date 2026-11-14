//! How much typing could have shown before the server answered: `sshee-typing-counts`,
//! version 0 (`src/lib/typingCounts.ts`).
//!
//! Numbers only, nothing of what was typed or where. 0.10 saves them only in batches of 200
//! keys or more, so the counts can't tell how long a password typed in between was.

use super::json::{Json, JsonMap};
use super::{Platform, Store};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypingCounts {
    /// By way of connecting (`tor`, `direct`), then by where the typing went (`line`,
    /// `agent`, `screen`).
    pub saved: IndexMap<String, IndexMap<String, EchoTally>>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Store for TypingCounts {
    const KEY: &'static str = "sshee-typing-counts";
    const VERSION: u64 = 0;
    const SHIPPED: &'static [u64] = &[0];

    fn defaults(_platform: Platform) -> Self {
        TypingCounts::default()
    }
}

/// One place's counts by name (`keys`, `typed`, `atOnce`, `wrong`, …) and `echoes`: how
/// many answers came within each step of 10, 20, 40 … 3000 ms, or later.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EchoTally(pub JsonMap);

impl EchoTally {
    /// A count, 0 when there's none.
    pub fn count(&self, name: &str) -> u64 {
        self.0.get(name).and_then(Json::as_u64).unwrap_or(0)
    }

    /// Answers per step of time, as saved.
    pub fn echoes(&self) -> Vec<u64> {
        match self.0.get("echoes") {
            Some(Json::Array(steps)) => steps.iter().map(|n| n.as_u64().unwrap_or(0)).collect(),
            _ => vec![],
        }
    }
}
