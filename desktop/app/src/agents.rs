//! Which program is in front in each terminal on the servers you're connected to, so a tab
//! can be named after the agent working in it ("Terminal 2 (Claude Code)").
//!
//! A port of 0.10's `startForegroundWatch` (`src/lib/agents.ts`): one question per server
//! every few seconds (`sshee_core::agents::foreground`, one `tmux list-panes`), on a thread of
//! its own. A server is first asked once which agents it has installed
//! (`sshee_core::agents::detect`); one with none isn't asked again: there'd be nothing to
//! name. Only servers with a connected terminal are asked (the window says which), so a look
//! reuses that connection and never signs in or asks about a host key on its own.
//!
//! Not here yet: 0.10 stops asking while its window is hidden.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use sshee_core::agents;
use sshee_core::ssh::{SshEnv, Target};
use sshee_ui::shell::Remote;

/// How often each server is asked (0.10's `WATCH_MS`).
const EVERY: Duration = Duration::from_secs(5);
/// How soon after the servers changed the first question goes.
const FIRST: Duration = Duration::from_millis(1500);

/// A server to ask: its key in the workbench and how to reach it.
pub type Asked = (String, Target);

/// The thread that asks. Dropping it ends the thread.
pub struct Watcher {
    tx: mpsc::Sender<Vec<Asked>>,
}

/// tmux's answer as the window takes it: (session key, program).
fn running(list: Vec<agents::Foreground>) -> Vec<(String, String)> {
    list.into_iter().map(|f| (f.session_key, f.command)).collect()
}

impl Watcher {
    pub fn start(env: SshEnv, remote: Remote) -> Watcher {
        let (tx, rx) = mpsc::channel::<Vec<Asked>>();
        let spawned = std::thread::Builder::new().name("agents".into()).spawn(move || {
            let mut servers: Vec<Asked> = Vec::new();
            // Whether a server has any agent installed, once it could be asked.
            let mut has_agents: HashMap<String, bool> = HashMap::new();
            let mut wait = FIRST;
            loop {
                match rx.recv_timeout(if servers.is_empty() { Duration::from_secs(3600) } else { wait }) {
                    Ok(next) => {
                        servers = next;
                        wait = FIRST;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                for (key, target) in &servers {
                    let known = match has_agents.get(key) {
                        Some(known) => *known,
                        // A look that failed is tried again next round.
                        None => match agents::detect(target, &env) {
                            Ok(found) => {
                                has_agents.insert(key.clone(), !found.is_empty());
                                !found.is_empty()
                            }
                            Err(_) => false,
                        },
                    };
                    if !known {
                        continue;
                    }
                    if let Ok(list) = agents::foreground(target, &env) {
                        remote.set_foreground(key.clone(), running(list));
                    }
                }
                wait = EVERY;
            }
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: the agents' thread didn't start: {e}");
        }
        Watcher { tx }
    }

    /// The servers with a connected terminal, each time they change.
    pub fn watch(&self, servers: Vec<Asked>) {
        let _ = self.tx.send(servers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmuxs_answer_is_handed_on_by_session_key() {
        let list = vec![agents::Foreground { session_key: "k1".into(), command: "claude".into() }, agents::Foreground { session_key: "k2".into(), command: "zsh".into() }];
        assert_eq!(running(list), [("k1".to_owned(), "claude".to_owned()), ("k2".to_owned(), "zsh".to_owned())]);
    }
}
