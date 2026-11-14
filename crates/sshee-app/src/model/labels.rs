//! Per-server labels, icons and colors: `sshee-labels`, version 2 (`src/lib/labels.ts`).

use super::json::{string_enum, Json, JsonMap};
use super::{Platform, Store};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Labels {
    /// Production, Staging or Development, by host id.
    pub env: IndexMap<String, EnvLabel>,
    /// Servers from ~/.ssh/config hidden from the list, by host id.
    pub hidden: Vec<String>,
    /// Servers from ~/.ssh/config already said yes or no to.
    pub config_seen: Vec<String>,
    /// The looks picked by hand, by host id.
    pub looks: IndexMap<String, HostLook>,
    /// Icons guessed from what runs on each server; `looks` win.
    pub auto_looks: IndexMap<String, AutoLook>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Store for Labels {
    const KEY: &'static str = "sshee-labels";
    const VERSION: u64 = 2;
    const SHIPPED: &'static [u64] = &[1, 2];

    fn defaults(_platform: Platform) -> Self {
        Labels::default()
    }

    /// 2 added `autoLooks`. Before it, adding a server saved the default look (server icon,
    /// the old blue) as if picked; those go, so the server gets a guessed icon and a color
    /// of its own.
    fn migrate(mut s: JsonMap, from: u64, _platform: Platform) -> JsonMap {
        if from < 2 {
            if let Some(Json::Object(looks)) = s.get_mut("looks") {
                // 0.10 throws on a look without a text color and loses every label; here
                // such a look is kept, and left for the reader to judge.
                looks.retain(|_, look| {
                    let Some(look) = look.as_object() else { return true };
                    let server = look.get("icon").and_then(Json::as_str) == Some("server");
                    let blue = look.get("color").and_then(Json::as_str).is_some_and(|c| c.to_lowercase() == "#5b9dff");
                    !(server && blue)
                });
            }
        }
        s
    }
}

/// A server's icon and color as picked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostLook {
    /// An icon id (`server`, `globe`, `database`, …).
    pub icon: String,
    /// `#rrggbb`.
    pub color: String,
    /// A picture of your own instead of the icon: a small PNG data URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// An icon SSHeesh guessed for a server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoLook {
    pub icon: String,
    #[serde(flatten)]
    pub extra: JsonMap,
}

string_enum! {
    pub enum EnvLabel { Prod = "prod", Staging = "staging", Dev = "dev", }
}
