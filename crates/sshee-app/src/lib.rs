//! SSHeesh's app state without screens, for 1.0 on Rune.
//!
//! `raw` is what 0.10 left on disk, as stored; `legacy` finds and reads it; `model` holds the
//! typed state with the same versions and migrations as 0.10's zustand stores; `import` turns
//! the raw stores into the model. Plan: `reports/План 1.0.md` sections 4 and 6 (local notes).

pub mod raw;
pub mod legacy;
pub mod model;
pub mod import;
