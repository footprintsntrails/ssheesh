//! The pane's session through sshee-core: a shell on this computer, or on a server over ssh
//! (in tmux there when the server has it). Output is parsed into the pane's model on the
//! session's reader thread (`Delivery::Direct`), which wakes the window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sshee_core::session::{Delivery, EventSink, SessionEvent, SessionManager, SessionSpec, SessionState};
use sshee_core::ssh::{SshEnv, SshTarget, Target};
use sshee_core::welcome::Welcome;
use sshee_term::engine::Size;
use sshee_term::model::Model;
use sshee_ui::terminal::{LinkStatus, StatusSink, TermLink};

use crate::args::Session;

/// The one pane's id with the session manager.
const PANE: &str = "pane";

/// The pane's session key: stable, so a reconnect (or the next run) finds the same tmux
/// session on the server.
const SESSION_KEY: &str = "proto-pane-1";

/// History the terminal keeps itself: a little with tmux (tmux keeps the rest on the server),
/// more without.
const HISTORY_TMUX: usize = 1_000;
const HISTORY_OWN: usize = 10_000;

pub struct LiveLink {
    manager: Arc<SessionManager>,
    session: Session,
    env: SshEnv,
    /// Output so far, in bytes (counted on the reader thread, for the meter).
    bytes: Arc<AtomicU64>,
}

impl LiveLink {
    pub fn new(session: Session) -> Self {
        let env = SshEnv::with_control_dir(sshee_core::app_identity::current().control_dir());
        LiveLink { manager: Arc::new(SessionManager::new()), session, env, bytes: Arc::new(AtomicU64::new(0)) }
    }

    fn target(&self) -> Target {
        match &self.session {
            Session::Ssh { destination, config } => Target::Ssh(SshTarget {
                destination: destination.clone(),
                extra_args: config.iter().flat_map(|c: &PathBuf| ["-F".to_string(), c.display().to_string()]).collect(),
                ..SshTarget::default()
            }),
            // A replay never reaches a live link.
            Session::Local | Session::Replay { .. } => Target::Local,
        }
    }
}

/// What the title row says of a session's state. sshee-core's failure texts name no host.
pub fn link_status(state: &SessionState) -> LinkStatus {
    match state {
        SessionState::Connecting { .. } => LinkStatus::Connecting,
        SessionState::Connected { durable, .. } => LinkStatus::Connected { durable: *durable },
        SessionState::Reconnecting { attempt, .. } => LinkStatus::Waiting { note: format!("reconnecting (try {attempt})") },
        SessionState::Disconnected { .. } => LinkStatus::Waiting { note: "disconnected · a key reconnects".into() },
        SessionState::Asleep => LinkStatus::Waiting { note: "asleep · a key wakes it".into() },
        SessionState::Ended { code } => LinkStatus::Gone { note: code.map_or("ended".into(), |c| format!("ended ({c})")) },
        SessionState::Failed { message } => LinkStatus::Gone { note: message.clone() },
    }
}

impl TermLink for LiveLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        let spec = SessionSpec {
            target: self.target(),
            key: SESSION_KEY.into(),
            env: self.env.clone(),
            run: None,
            welcome: Welcome { mode: "ssheesh".into(), parts: vec!["logo".into(), "system".into(), "usage".into()], note: String::new() },
        };
        // On the reader thread (output, a server's Connected) or the session's (the rest): the
        // model locks what it shares.
        let bytes = self.bytes.clone();
        let sink: EventSink = Arc::new(move |event| match event {
            SessionEvent::Output { data } => {
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                model.feed(data.as_bytes());
            }
            SessionEvent::Status { state } => {
                match &state {
                    SessionState::Connected { durable, .. } => model.connected(*durable, if *durable { HISTORY_TMUX } else { HISTORY_OWN }),
                    SessionState::Connecting { .. } => {}
                    _ => model.disconnected(),
                }
                status(link_status(&state));
            }
        });
        self.manager.open_with(PANE, size.cols, size.rows, move || Ok(spec), sink, Delivery::Direct);
    }

    fn write(&self, bytes: &[u8]) {
        self.manager.write(PANE, bytes);
    }

    fn resize(&self, size: Size) {
        self.manager.resize(PANE, size.cols, size.rows);
    }

    fn reconnect(&self) {
        self.manager.reconnect_now(PANE);
    }

    fn close(&self) {
        self.manager.close_all();
    }

    fn bytes_received(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_for_the_title_row() {
        assert_eq!(link_status(&SessionState::Connected { durable: true, tor: false }), LinkStatus::Connected { durable: true });
        assert!(link_status(&SessionState::Asleep).wants_reconnect());
        assert!(!link_status(&SessionState::Ended { code: Some(0) }).wants_reconnect());
        assert_eq!(link_status(&SessionState::Failed { message: "refused".into() }), LinkStatus::Gone { note: "refused".into() });
    }
}
