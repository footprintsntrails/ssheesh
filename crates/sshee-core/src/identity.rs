//! Who and where "This computer" is. Streamer mode hides these names in
//! terminal prompts, so the app needs to know them.

use std::process::{Command, Stdio};

use serde::Serialize;

use crate::ssh::hide_console;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalIdentity {
    pub user: String,
    pub hostname: String,
}

pub fn local() -> LocalIdentity {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let user = env("USER").or_else(|| env("USERNAME")).or_else(|| env("LOGNAME")).unwrap_or_default();
    let hostname = env("COMPUTERNAME").or_else(hostname_command).unwrap_or_default();
    LocalIdentity { user, hostname }
}

fn hostname_command() -> Option<String> {
    let mut cmd = Command::new("hostname");
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn finds_this_machines_name() {
        assert!(!local().hostname.is_empty());
    }
}
