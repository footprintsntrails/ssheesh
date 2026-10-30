//! Everything SSHeesh does that isn't UI: finding hosts, running terminal
//! sessions that survive network drops, browsing files and reading metrics.
//!
//! All remote work goes through the system `ssh` binary, so the user's
//! `~/.ssh/config`, keys and agent just work.

pub mod containers;
pub mod error;
pub mod files;
pub mod hosts;
pub mod metrics;
pub mod session;
pub mod ssh;
mod text;

pub use error::{ErrorCode, OpError};
pub use hosts::{Host, HostSource, HostStore, NewHost};
pub use session::{SessionEvent, SessionManager, SessionSpec, SessionState};
pub use ssh::{SshEnv, SshTarget, Target};
