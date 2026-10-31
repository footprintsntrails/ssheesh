//! Docker and Podman containers on a machine: list them, start/stop them,
//! and build the commands that open a shell or follow logs inside one.

use crate::error::OpError;
use crate::ssh::{run_local, run_remote, run_remote_read, shell_quote, SshEnv, Target};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Docker,
    Podman,
}

impl Engine {
    pub fn bin(self) -> &'static str {
        match self {
            Engine::Docker => "docker",
            Engine::Podman => "podman",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub engine: Engine,
    pub id: String,
    pub name: String,
    pub image: String,
    pub running: bool,
    /// running, exited, paused, restarting…
    pub state: String,
    /// Human text from the engine, like "Up 3 days (healthy)".
    pub status: String,
    pub ports: String,
    /// Docker Compose project, when the container belongs to one.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerList {
    /// Engines installed on the machine.
    pub engines: Vec<Engine>,
    pub containers: Vec<Container>,
    /// Engines that are installed but couldn't be used, and why.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Start,
    Stop,
    Restart,
}

/// What a terminal opened on a container runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Run {
    Shell,
    Logs,
}

const FORMAT: &str = r#"{{.ID}}	{{.Names}}	{{.Image}}	{{.State}}	{{.Status}}	{{.Ports}}	{{.Label "com.docker.compose.project"}}"#;

fn script() -> String {
    format!(
        "for e in docker podman; do command -v $e >/dev/null 2>&1 || continue; echo \"@engine $e\"; $e ps -a --no-trunc --format {} 2>&1; done; exit 0",
        shell_quote(FORMAT)
    )
}

pub fn list(target: &Target, env: &SshEnv) -> Result<ContainerList, OpError> {
    let out = match target {
        Target::Ssh(t) => run_remote_read(t, env, &script())?,
        Target::Local => run_local(&script())?,
    };
    Ok(parse(&String::from_utf8_lossy(&out)))
}

pub fn parse(text: &str) -> ContainerList {
    let mut list = ContainerList::default();
    let mut engine: Option<Engine> = None;
    let mut reported = false;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("@engine ") {
            engine = match name.trim() {
                "docker" => Some(Engine::Docker),
                "podman" => Some(Engine::Podman),
                _ => None,
            };
            if let Some(e) = engine {
                list.engines.push(e);
            }
            reported = false;
            continue;
        }
        let Some(e) = engine else { continue };
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 6 {
            let id: String = f[0].chars().take(12).collect();
            let state = f[3].trim().to_lowercase();
            list.containers.push(Container {
                engine: e,
                id,
                name: f[1].split(',').next().unwrap_or(f[1]).trim().to_string(),
                image: f[2].trim().to_string(),
                running: state == "running",
                state,
                status: f[4].trim().to_string(),
                ports: f[5].trim().to_string(),
                project: f.get(6).map(|p| p.trim().to_string()).filter(|p| !p.is_empty() && !p.starts_with("<no value>")),
            });
        } else if !line.trim().is_empty() && !reported {
            reported = true;
            list.problems.push(friendly_problem(e, line.trim()));
        }
    }
    list.containers.sort_by(|a, b| {
        (a.project.is_none(), a.project.clone().unwrap_or_default(), !a.running, a.name.to_lowercase())
            .cmp(&(b.project.is_none(), b.project.clone().unwrap_or_default(), !b.running, b.name.to_lowercase()))
    });
    list
}

fn friendly_problem(engine: Engine, line: &str) -> String {
    let name = match engine {
        Engine::Docker => "Docker",
        Engine::Podman => "Podman",
    };
    let l = line.to_ascii_lowercase();
    if l.contains("permission denied") {
        return format!(
            "{name} is installed, but this user can't use it. Add the user to the {} group (sudo usermod -aG {} $USER), then sign in again.",
            engine.bin(),
            engine.bin()
        );
    }
    if l.contains("cannot connect") || l.contains("is the docker daemon running") {
        return format!("{name} is installed but not running.");
    }
    format!("{name}: {}", line.chars().take(160).collect::<String>())
}

pub fn act(target: &Target, env: &SshEnv, engine: Engine, id: &str, action: Action) -> Result<(), OpError> {
    let verb = match action {
        Action::Start => "start",
        Action::Stop => "stop",
        Action::Restart => "restart",
    };
    let script = format!("{} {} {}", engine.bin(), verb, shell_quote(id));
    match target {
        Target::Ssh(t) => run_remote(t, env, &script, None)?,
        Target::Local => run_local(&script)?,
    };
    Ok(())
}

/// The command a terminal runs to get into a container.
pub fn argv(engine: Engine, id: &str, run: Run) -> Vec<String> {
    let bin = engine.bin().to_string();
    match run {
        Run::Shell => vec![
            bin,
            "exec".into(),
            "-it".into(),
            id.to_string(),
            "sh".into(),
            "-c".into(),
            "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi".into(),
        ],
        Run::Logs => vec![bin, "logs".into(), "-f".into(), "--tail".into(), "200".into(), id.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_and_problems() {
        let out = "@engine docker\n\
abc123def4567890\tapp-api-1\tacme/api:2.14\trunning\tUp 3 days (healthy)\t0.0.0.0:8080->8080/tcp\tapp\n\
def456\told-worker\tworker:1\texited\tExited (0) 2 days ago\t\t\n\
@engine podman\n\
permission denied while trying to connect\n";
        let l = parse(out);
        assert_eq!(l.engines, vec![Engine::Docker, Engine::Podman]);
        assert_eq!(l.containers.len(), 2);
        assert_eq!(l.containers[0].name, "app-api-1");
        assert_eq!(l.containers[0].id, "abc123def456");
        assert!(l.containers[0].running);
        assert_eq!(l.containers[0].project.as_deref(), Some("app"));
        assert!(!l.containers[1].running);
        assert_eq!(l.problems.len(), 1);
        assert!(l.problems[0].contains("usermod"));
    }

    #[test]
    fn shell_argv_prefers_bash() {
        let a = argv(Engine::Docker, "web", Run::Shell);
        assert_eq!(&a[..4], ["docker", "exec", "-it", "web"]);
    }
}
