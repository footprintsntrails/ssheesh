//! SSHeesh's terminal without a screen, for 1.0 on Rune.
//!
//! Nothing here depends on a UI toolkit: the screen layer asks for the rows that changed and
//! draws them. Design: `research_notes/v1-plan/terminal.md` section 4 (local notes).

// The grid engine and what feeds it.
pub mod engine;
pub mod model;
pub mod osc;
pub mod input;
pub mod select;
pub mod gate;
pub mod scroll;

// What the user sees in the rows, and what the app learns from them.
pub mod present;
pub mod secrets;
pub mod privacy;
pub mod highlight;
pub mod links;
pub mod find;
pub mod away;
pub mod attention;
pub mod question;
pub mod echo;
