//! Everything SSHeesh does that isn't UI: finding hosts, running terminal
//! sessions that survive network drops, browsing files and reading metrics.
//!
//! All remote work goes through the system `ssh` binary, so the user's
//! `~/.ssh/config`, keys and agent just work.

pub mod agents;
pub mod checkup;
pub mod containers;
pub mod disk;
pub mod error;
pub mod files;
pub mod hostkeys;
pub mod hosts;
pub mod identity;
pub mod keys;
pub mod metrics;
pub mod secrets;
pub mod session;
pub mod shell_pool;
pub mod ssh;
mod text;
pub mod tunnels;
pub mod welcome;

pub use error::{ErrorCode, OpError};
pub use hosts::{Host, HostSource, HostStore, NewHost};
pub use session::{SessionEvent, SessionManager, SessionSpec, SessionState};
pub use ssh::{SshEnv, SshTarget, Target};
