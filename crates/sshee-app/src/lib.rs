//! SSHeesh's app state without screens, for 1.0 on Rune.
//!
//! `raw` is what 0.10 left on disk, as stored; `legacy` finds and reads it; `model` holds the
//! typed state with the same versions and migrations as 0.10's zustand stores; `import` turns
//! the raw stores into the model; `workbench` is what the 1.0 window shows and does (servers,
//! tabs, the tab you're on), without a screen. Plan: `reports/План 1.0.md` sections 4 and 6 (local notes).
//!
//! The rules of single screens: `home` (the main menu and a server's card), `palette` (the
//! command palette: what it lists, how typing narrows it, which row Return picks), `servers`
//! (the "Add a server" form, the imports, and what the window asks its backend to do with the
//! list), `labels` (a server's icon, colour, label and name).

pub mod raw;
pub mod legacy;
pub mod model;
pub mod import;
pub mod workbench;
pub mod home;
pub mod labels;
pub mod palette;
pub mod servers;
pub mod agents;
