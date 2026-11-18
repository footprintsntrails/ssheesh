//! Tunnels on sshee-core: each one its own `ssh -N` (`sshee_core::tunnels::TunnelManager`),
//! restarted with backoff when it drops, its state sent to the window. The saved list is this
//! copy's own `tunnels.json` (in `~/.sshee-preview`, 0.10's `sshee-tunnels` shape); the ones
//! left on start again when the program opens. 0.10's list is never written.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sshee_app::model::{ForwardKind, SavedTunnels};
use sshee_app::tunnels::{remote_host, Ask, Reply, State};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::tunnels::{Forward, TunnelManager, TunnelState};
use sshee_ui::shell::Remote;

use crate::edit::Reach;

const FILE: &str = "tunnels.json";

/// The running tunnels and where the list is kept.
pub struct Tunnels {
    manager: Arc<TunnelManager>,
    env: SshEnv,
    /// This copy's folder; `None`: nothing is kept.
    dir: Option<PathBuf>,
    remote: Option<Remote>,
    turns: Turns,
}

/// Which ask about each tunnel is the latest. A start reads the keychain on a thread first; a
/// Stop, or a newer start, that came meanwhile wins, and the older start then does nothing.
#[derive(Clone, Default)]
struct Turns(Arc<Mutex<HashMap<String, u64>>>);

impl Turns {
    /// A new ask about `id`: its turn, which ends every earlier one.
    fn take(&self, id: &str) -> u64 {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let turn = all.entry(id.to_owned()).or_default();
        *turn += 1;
        *turn
    }

    /// Runs `f` if `turn` is still the latest about `id`, with no other ask about it meanwhile
    /// (a Stop waits for the start it would otherwise miss). Whether it ran.
    fn if_latest(&self, id: &str, turn: u64, f: impl FnOnce()) -> bool {
        let all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let latest = all.get(id) == Some(&turn);
        if latest {
            f();
        }
        latest
    }

    /// Ends every earlier turn about `id` and runs `f` while no start can slip in.
    fn end(&self, id: &str, f: impl FnOnce()) {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *all.entry(id.to_owned()).or_default() += 1;
        f();
    }
}

impl Tunnels {
    pub fn new(env: SshEnv, dir: Option<PathBuf>) -> Tunnels {
        Tunnels { manager: Arc::new(TunnelManager::new()), env, dir, remote: None, turns: Turns::default() }
    }

    /// Reads the list kept last time and tells the window. What to ask next: a start for
    /// each one left on.
    pub fn start(&mut self, remote: Remote) -> Vec<Ask> {
        self.remote = Some(remote.clone());
        let saved = self.dir.as_deref().map(load).unwrap_or_default();
        remote.tunnels(Reply::Loaded(saved.clone()));
        let mut starts = Vec::new();
        for (host, list) in &saved.by_host {
            starts.extend(list.iter().filter(|t| t.on).map(|t| Ask::Start { id: t.id.clone(), host: host.clone(), forward: t.forward.clone() }));
        }
        starts
    }

    /// What the window asks. `reach`: the server's target and its saved password, for a start.
    pub fn ask(&self, ask: Ask, reach: Reach) {
        let Some(remote) = self.remote.clone() else { return };
        match ask {
            Ask::Start { id, forward, .. } => {
                let (target, prepare) = reach;
                let Some(forward) = core_forward(&forward) else {
                    return remote.tunnels(Reply::State { id, state: Some(State::Failed { message: "SSHeesh doesn't know this kind of tunnel.".into() }) });
                };
                let Some(mut target) = target else {
                    return remote.tunnels(Reply::State { id, state: Some(State::Failed { message: "This server isn't in the list anymore.".into() }) });
                };
                let (manager, env, turns) = (self.manager.clone(), self.env.clone(), self.turns.clone());
                let turn = turns.take(&id);
                // The saved password is read from the keychain, which may wait: on a thread.
                let spawned = std::thread::Builder::new().name("tunnel-start".into()).spawn({
                    let (remote, id) = (remote.clone(), id.clone());
                    move || {
                        prepare(&mut target);
                        let Target::Ssh(target) = target else {
                            return remote.tunnels(Reply::State { id, state: Some(State::Failed { message: "Tunnels lead to a server, not to this computer.".into() }) });
                        };
                        let sink_remote = remote.clone();
                        let sink_id = id.clone();
                        let sink = Arc::new(move |state: TunnelState| sink_remote.tunnels(Reply::State { id: sink_id.clone(), state: Some(state_of(state)) }));
                        // Stopped, removed or started again while the keychain answered: not
                        // this start's to do anymore.
                        let mut started = Ok(());
                        turns.if_latest(&id, turn, || started = manager.start(&id, &target, &env, forward, sink));
                        if let Err(e) = started {
                            remote.tunnels(Reply::State { id, state: Some(State::Failed { message: e.message }) });
                        }
                    }
                });
                if let Err(e) = spawned {
                    remote.tunnels(Reply::State { id, state: Some(State::Failed { message: format!("Couldn't start the tunnel: {e}") }) });
                }
            }
            Ask::Stop { id } => {
                self.turns.end(&id, || self.manager.stop(&id));
                remote.tunnels(Reply::State { id, state: None });
            }
            Ask::Save(list) => {
                if let Some(dir) = &self.dir {
                    if let Err(e) = save(dir, &list) {
                        eprintln!("ssheesh-preview: the tunnels weren't saved: {e}");
                    }
                }
            }
        }
    }

    /// For when Tor goes on or off, from another thread: every running tunnel starts again
    /// the new way.
    pub fn rerouter(&self) -> impl Fn() + Send + 'static {
        let manager = self.manager.clone();
        move || manager.reroute()
    }
}

/// The saved list's forward as sshee-core runs it. `None`: a kind this version doesn't know.
pub fn core_forward(f: &sshee_app::model::Forward) -> Option<Forward> {
    let remote_port = f.remote_port.unwrap_or_default();
    Some(match f.kind {
        ForwardKind::Local => Forward::Local { local_port: f.local_port, remote_host: remote_host(f).to_owned(), remote_port },
        ForwardKind::Socks => Forward::Socks { local_port: f.local_port },
        ForwardKind::Remote => Forward::Remote { remote_port, local_port: f.local_port },
        ForwardKind::Other(_) => return None,
    })
}

/// sshee-core's state as the window's.
pub fn state_of(s: TunnelState) -> State {
    match s {
        TunnelState::Starting => State::Starting,
        TunnelState::On { warning } => State::On { warning },
        TunnelState::Reconnecting { attempt, retry_in_ms, reason } => State::Reconnecting { attempt, retry_in_ms, reason },
        TunnelState::Failed { message } => State::Failed { message },
    }
}

/// The list kept in `dir`; empty when there's none or it doesn't read. One that doesn't read is
/// moved aside to a `.damaged` name first, so the next save can't write over it.
pub fn load(dir: &Path) -> SavedTunnels {
    sshee_app::settings_store::read_json_or_set_aside(&dir.join(FILE)).unwrap_or_default()
}

/// Writes the list to `dir`, whole or not at all.
pub fn save(dir: &Path, list: &SavedTunnels) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Which servers and ports: the user's own business.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        std::fs::rename(&tmp, dir.join(FILE))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::model::SavedTunnel;
    use sshee_app::tunnels::{local, reverse, socks};

    #[test]
    fn every_kind_reaches_ssh_as_it_was_saved() {
        assert_eq!(core_forward(&local(8080, "", 3000)), Some(Forward::Local { local_port: 8080, remote_host: "127.0.0.1".into(), remote_port: 3000 }));
        assert_eq!(core_forward(&local(5432, "db.internal", 5432)).unwrap().args(), ["-L", "127.0.0.1:5432:db.internal:5432"]);
        assert_eq!(core_forward(&socks(1080)), Some(Forward::Socks { local_port: 1080 }));
        assert_eq!(core_forward(&reverse(9000, 3000)).unwrap().args(), ["-R", "127.0.0.1:9000:127.0.0.1:3000"]);
        let unknown = sshee_app::model::Forward { kind: ForwardKind::Other("udp".into()), ..local(1, "", 1) };
        assert_eq!(core_forward(&unknown), None);
        assert_eq!(state_of(TunnelState::On { warning: None }), State::On { warning: None });
    }

    #[test]
    fn a_start_waiting_on_the_keychain_loses_to_a_later_stop_or_start() {
        let turns = Turns::default();
        // Start, then Stop before the keychain answers: the start does nothing.
        let first = turns.take("tun-1");
        turns.end("tun-1", || {});
        assert!(!turns.if_latest("tun-1", first, || panic!("a stopped tunnel started")));
        // Two quick starts: only the later one runs, whichever thread is first.
        let (a, b) = (turns.take("tun-1"), turns.take("tun-1"));
        assert!(turns.if_latest("tun-1", b, || {}));
        assert!(!turns.if_latest("tun-1", a, || panic!("an older start ran")));
        // Another tunnel's turns are its own.
        let other = turns.take("tun-2");
        assert!(turns.if_latest("tun-2", other, || {}));
    }

    #[test]
    fn the_list_is_kept_in_the_copys_folder_and_read_back() {
        let dir = std::env::temp_dir().join(format!("sshee-preview-tunnels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), SavedTunnels::default());
        let mut list = SavedTunnels::default();
        list.by_host.insert("demo-web".into(), vec![SavedTunnel { id: "tun-1".into(), name: Some("Grafana".into()), forward: local(3000, "", 3000), on: true, ..SavedTunnel::default() }]);
        save(&dir, &list).unwrap();
        assert_eq!(load(&dir), list);
        // 0.10's shape: byHost, and camelCase forwards.
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.contains("\"byHost\"") && text.contains("\"localPort\": 3000"), "{text}");
        // One that doesn't read is moved aside, not written over by the next save.
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        assert_eq!(load(&dir), SavedTunnels::default());
        save(&dir, &list).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(FILE.replace(".json", ".damaged.json"))).unwrap(), "{ not json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
