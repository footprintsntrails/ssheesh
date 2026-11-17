//! Containers and the Stats page's programs on sshee-core: what a Containers tab asks
//! (`sshee_app::containers::Ask`) and a Stats tab asks (`sshee_app::stats::Ask`), done on a
//! machine and answered back to the window (`Remote::containers`, `Remote::stats`).
//!
//! Every request runs on a thread of its own, as Files' do, over the machine's shared
//! connection (a look never signs in by itself). sshee-app mirrors sshee-core's types; the
//! conversions are here, with the checks a terminal into a container needs before it runs.

use sshee_app::containers::{Action, Answer, Ask, Container, ContainerList, Want};
use sshee_app::model::{ContainerRef, ContainerRun, Engine};
use sshee_app::stats::Process;
use sshee_core::containers as core;
use sshee_core::error::{ErrorCode, OpError};
use sshee_core::ssh::{SshEnv, Target};
use sshee_ui::shell::Remote;

use crate::files::{failure, Prepare};

fn engine(e: core::Engine) -> Engine {
    match e {
        core::Engine::Docker => Engine::Docker,
        core::Engine::Podman => Engine::Podman,
    }
}

fn core_engine(e: &Engine) -> Option<core::Engine> {
    match e {
        Engine::Docker => Some(core::Engine::Docker),
        Engine::Podman => Some(core::Engine::Podman),
        Engine::Other(_) => None,
    }
}

pub fn list(l: core::ContainerList) -> ContainerList {
    ContainerList {
        engines: l.engines.into_iter().map(engine).collect(),
        containers: l
            .containers
            .into_iter()
            .map(|c| Container { engine: engine(c.engine), id: c.id, name: c.name, image: c.image, running: c.running, state: c.state, status: c.status, ports: c.ports, project: c.project })
            .collect(),
        problems: l.problems,
    }
}

fn action(a: Action) -> core::Action {
    match a {
        Action::Start => core::Action::Start,
        Action::Stop => core::Action::Stop,
        Action::Restart => core::Action::Restart,
    }
}

/// A container id as docker and podman print it: hex only, so nothing in it is read as an
/// option or by a shell (a saved tab is a file anyone could have edited).
fn is_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a terminal into a container runs: `docker exec -it <id> sh …` or `docker logs -f …`.
/// `None` for an engine this version doesn't know or an id that isn't one.
pub fn argv(c: &ContainerRef) -> Option<Vec<String>> {
    let engine = core_engine(&c.engine)?;
    if !is_id(&c.id) {
        return None;
    }
    let run = match c.run {
        ContainerRun::Logs => core::Run::Logs,
        _ => core::Run::Shell,
    };
    Some(core::argv(engine, &c.id, run))
}

/// Containers' and programs' side of the live backend.
pub struct Containers {
    env: SshEnv,
    remote: Option<Remote>,
}

/// A request's thread could not start: the window hears it as a failure.
fn no_thread() -> OpError {
    OpError::new(ErrorCode::Failed, "Couldn't start the work.")
}

/// The answer to `want` when it couldn't be done at all.
fn failed(want: &Want, e: OpError) -> Answer {
    match want {
        Want::List { ticket } => Answer::Listed { ticket: *ticket, result: Err(failure(e)) },
        Want::Act { id, name, action, .. } => Answer::Acted { id: id.clone(), name: name.clone(), action: *action, result: Err(failure(e)) },
    }
}

impl Containers {
    pub fn new(env: SshEnv) -> Containers {
        Containers { env, remote: None }
    }

    pub fn start(&mut self, remote: Remote) {
        self.remote = Some(remote);
    }

    /// A Containers tab's request, on a thread of its own.
    pub fn ask(&self, ask: Ask, target: Option<Target>, prepare: Prepare) {
        let Some(remote) = self.remote.clone() else { return };
        let Some(mut target) = target else {
            remote.containers(ask.reply(failed(&ask.want, OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore."))));
            return;
        };
        let env = self.env.clone();
        let (gone, asked) = (remote.clone(), ask.clone());
        let name = if matches!(ask.want, Want::List { .. }) { "containers-list" } else { "containers-act" };
        let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
            prepare(&mut target);
            let reply = match &ask.want {
                Want::List { ticket } => Answer::Listed { ticket: *ticket, result: core::list(&target, &env).map(list).map_err(failure) },
                Want::Act { engine, id, name, action: a } => {
                    let result = match core_engine(engine).filter(|_| is_id(id)) {
                        Some(e) => core::act(&target, &env, e, id, action(*a)).map_err(failure),
                        None => Err(failure(OpError::new(ErrorCode::Unsupported, "SSHeesh doesn't know this container's engine."))),
                    };
                    Answer::Acted { id: id.clone(), name: name.clone(), action: *a, result }
                }
            };
            remote.containers(ask.reply(reply));
        });
        if spawned.is_err() {
            gone.containers(asked.reply(failed(&asked.want, no_thread())));
        }
    }

    /// A Stats tab asks which programs run, on a thread of its own.
    pub fn programs(&self, ask: sshee_app::stats::Ask, target: Option<Target>, prepare: Prepare) {
        let Some(remote) = self.remote.clone() else { return };
        let Some(mut target) = target else {
            remote.stats(ask.reply(Err(failure(OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore.")))));
            return;
        };
        let env = self.env.clone();
        let (failed, asked) = (remote.clone(), ask.clone());
        let spawned = std::thread::Builder::new().name("stats-programs".into()).spawn(move || {
            prepare(&mut target);
            let result = sshee_core::metrics::processes(&target, &env)
                .map(|list| list.into_iter().map(|p| Process { pid: p.pid, cpu: p.cpu, memory: p.memory, command: p.command }).collect())
                .map_err(failure);
            remote.stats(ask.reply(result));
        });
        if spawned.is_err() {
            failed.stats(asked.reply(Err(failure(no_thread()))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::containers::{Board, Problem};
    use sshee_app::model::JsonMap;

    #[test]
    fn sshee_cores_list_reads_as_the_tabs() {
        let out = "@engine docker\n\
abc123def4567890\tshop-api-1\tregistry.example/shop/api:2.14\trunning\tUp 3 days (healthy)\t0.0.0.0:8080->8080/tcp\tshop\n\
@engine podman\n\
permission denied while trying to connect\n";
        let l = list(core::parse(out));
        assert_eq!(l.engines, [Engine::Docker, Engine::Podman]);
        assert_eq!((l.containers[0].id.as_str(), l.containers[0].project.as_deref()), ("abc123def456", Some("shop")));
        // sshee-core's sentence for a user who isn't allowed is one sshee-app finds the fix for.
        let mut b = Board::default();
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Ok(ContainerList { containers: vec![], ..l }));
        match b.problem() {
            Some(Problem::Page { command, .. }) => assert_eq!(command.as_deref(), Some("sudo usermod -aG podman $USER")),
            other => panic!("{other:?}"),
        }
        // And for one that isn't running.
        let stopped = core::parse("@engine docker\nCannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?\n");
        let mut b = Board::default();
        let Want::List { ticket } = b.list() else { unreachable!() };
        b.listed(ticket, Ok(list(stopped)));
        assert!(matches!(b.problem(), Some(Problem::Page { ref title, command: Some(ref c), .. }) if title == "Docker isn't running" && c == "sudo systemctl enable --now docker"));
    }

    #[test]
    fn a_terminal_into_a_container_runs_only_a_real_id() {
        let c = |id: &str, run: ContainerRun| ContainerRef { engine: Engine::Docker, id: id.into(), name: "shop-api-1".into(), run, extra: JsonMap::new() };
        assert_eq!(argv(&c("abc123def456", ContainerRun::Shell)).unwrap()[..4], ["docker", "exec", "-it", "abc123def456"]);
        assert_eq!(argv(&c("abc123def456", ContainerRun::Logs)).unwrap()[..3], ["docker", "logs", "-f"]);
        // An edited tabs file can't make it run something else.
        assert_eq!(argv(&c("--privileged", ContainerRun::Shell)), None);
        assert_eq!(argv(&c("abc;reboot", ContainerRun::Shell)), None);
        assert_eq!(argv(&c("", ContainerRun::Shell)), None);
        assert_eq!(argv(&ContainerRef { engine: Engine::Other("lxc".into()), ..c("abc", ContainerRun::Shell) }), None);
    }
}
