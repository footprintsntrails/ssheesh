//! Saved tunnels, per server: `sshee-tunnels`, version 1 (`src/lib/tunnels.ts`).
//!
//! Tunnels left on start again when the app opens. Temporary ones ("Open" on a web app) are
//! never saved.

use super::json::{string_enum, JsonMap};
use super::{Platform, Store};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTunnels {
    /// Each server's tunnels by host id, in the order they were added.
    #[serde(rename = "byHost")]
    pub by_host: IndexMap<String, Vec<SavedTunnel>>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Store for SavedTunnels {
    const KEY: &'static str = "sshee-tunnels";
    const VERSION: u64 = 1;
    const SHIPPED: &'static [u64] = &[1];

    fn defaults(_platform: Platform) -> Self {
        SavedTunnels::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTunnel {
    pub id: String,
    /// Optional; without one it's described by its ports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub forward: Forward,
    /// The switch: tunnels left on start again when the app opens.
    pub on: bool,
    /// How its web app opens in the browser, when it was kept from one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<Scheme>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// A port here that leads through the server: to a port there (`Local`), or out to
/// anywhere as a SOCKS proxy (`Socks`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Forward {
    pub kind: ForwardKind,
    /// The port on this computer.
    pub local_port: u16,
    /// For `Local`: the address as the server sees it; empty means the server itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_host: Option<String>,
    /// For `Local`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_port: Option<u16>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

string_enum! {
    pub enum ForwardKind { Local = "local", Socks = "socks", }
    default Local
}

string_enum! {
    pub enum Scheme { Http = "http", Https = "https", }
}
