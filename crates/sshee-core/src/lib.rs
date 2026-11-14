//! Everything SSHeesh does that isn't UI: finding hosts, running terminal
//! sessions that survive network drops, browsing files and reading metrics.
//!
//! All remote work goes through the system `ssh` binary, so the user's
//! `~/.ssh/config`, keys and agent just work.

pub mod agents;
pub mod app_identity;
pub mod checkup;
pub mod containers;
pub mod disk;
pub mod drop_folder;
pub mod error;
pub mod files;
pub mod fixes;
pub mod hostkeys;
pub mod hosts;
pub mod identity;
pub mod keys;
pub mod media;
pub mod metrics;
pub mod scrub;
pub mod secrets;
pub mod session;
pub mod shell_pool;
pub mod ssh;
mod text;
pub mod tor;
pub mod tunnels;
pub mod welcome;
pub mod xpipe;

pub use app_identity::AppIdentity;
pub use error::{ErrorCode, OpError};
pub use hosts::{Host, HostSource, HostStore, NewHost};
pub use session::{Delivery, SessionEvent, SessionManager, SessionSpec, SessionState};
pub use ssh::{SshEnv, SshTarget, Target};
