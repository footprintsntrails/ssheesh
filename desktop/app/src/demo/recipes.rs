//! The demo's security check and recipes: made-up findings for each made-up server, fixes that
//! "run" at once, and a made-up key. Nothing connects; the answers go through the window's
//! queue, as a real backend's do.

use sshee_app::model::Snippets;
use sshee_app::recipes::checkup::{Finding, Report, Verdict};
use sshee_app::recipes::{Ask, Facts, LocalKey, Outcome, Reply, SshHost};

/// A made-up public key: the shape of one, not a real one.
const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDemoDemoDemoDemoDemoDemoDemoDemoDemoDemo0001 dev@laptop-7731";

fn finding(id: &str, title: &str, verdict: Verdict, detail: &str, fix: Option<&str>, note: Option<&str>) -> Finding {
    Finding { id: id.into(), title: title.into(), verdict, detail: detail.into(), fix: fix.map(str::to_owned), fix_note: note.map(str::to_owned) }
}

/// What the check finds on a made-up server.
pub fn report(server: &str) -> Report {
    let shop = server == "host:demo-web" || server == "host:demo-db";
    let mut findings = vec![
        finding(
            "password-login",
            "Password login",
            if shop { Verdict::Bad } else { Verdict::Good },
            if shop {
                "Anyone can try passwords on this server, and bots do, all day. Logging in with a key only shuts them out."
            } else {
                "Only keys log in here: password guessing can't work."
            },
            shop.then_some("printf 'PasswordAuthentication no\\nKbdInteractiveAuthentication no\\n' | sudo tee /etc/ssh/sshd_config.d/00-ssheesh.conf >/dev/null && sudo sshd -t && sudo systemctl reload ssh"),
            shop.then_some("Turns password login off. Check first that your key logs you in, or you could lock yourself out."),
        ),
        finding("root-login", "Root login", Verdict::Good, "root can't log in over SSH.", None, None),
        finding(
            "firewall",
            "Firewall",
            if shop { Verdict::Bad } else { Verdict::Good },
            if shop { "No firewall is on: every port a program opens can be reached from the internet." } else { "ufw is on and lets in SSH and the web." },
            shop.then_some("sudo ufw allow 22/tcp && sudo ufw --force enable"),
            shop.then_some("Lets SSH in first, then turns ufw on."),
        ),
        finding("open-ports", "Open ports", Verdict::Warn, "Port 5432 (PostgreSQL) is open to the internet. A database usually shouldn't be.", None, None),
        finding(
            "auto-updates",
            "Automatic updates",
            Verdict::Warn,
            "Security updates aren't installed by themselves. Known holes stay open until someone updates by hand.",
            Some("sudo apt-get update && sudo apt-get install -y unattended-upgrades"),
            Some("Installs unattended-upgrades, which installs security updates every day."),
        ),
        finding("security-updates", "Security updates", Verdict::Good, "No security updates are waiting.", None, None),
        finding("brute-force", "Login attack protection", Verdict::Unknown, "Couldn't tell whether fail2ban, CrowdSec or sshguard runs: that needs root.", None, None),
        finding("reboot", "Pending restart", Verdict::Good, "No restart is waiting.", None, None),
    ];
    if !shop {
        findings.retain(|f| f.id != "open-ports");
    }
    Report { findings, as_root: !shop }
}

/// How a fresh `ssh` typed here would reach a made-up server.
fn host(server: &str) -> Option<SshHost> {
    let (name, address) = match server {
        "host:demo-web" => ("web-7731", "203.0.113.17"),
        "host:demo-staging" => ("web-staging", "203.0.113.18"),
        "host:demo-backups" => ("backups", "203.0.113.24"),
        "host:demo-db" => ("db-main", "203.0.113.31"),
        _ => return None,
    };
    let user = if server == "host:demo-db" { "postgres" } else { "deploy" };
    Some(SshHost { name: name.into(), hostname: address.into(), port: None, user: Some(user.into()), identity_file: None, from_config: false })
}

/// The answer to what the window asked.
pub fn answer(ask: Ask) -> Reply {
    match ask {
        Ask::Check { server } => {
            let result = if server == "local" { Err("Check-ups are for servers.".into()) } else { Ok(report(&server)) };
            Reply::Checked { server, result }
        }
        Ask::Fix { server, check, command } => {
            let outcome = if sshee_core::fixes::asks_before(&command).is_some() {
                Outcome::NeedsYou { ask: "It lists what it's about to install and asks whether to go ahead: type y and press Enter.".into() }
            } else {
                Outcome::Done { output: format!("Done on the made-up server: {}", command.split(" && ").last().unwrap_or("")) }
            };
            Reply::Fixed { server, check, outcome }
        }
        Ask::Facts { server } => {
            let kernel = match server.as_str() {
                "host:demo-db" => "Linux 5.14.0-427.el9.x86_64",
                _ => "Linux 6.8.0-45-generic",
            };
            let facts = Facts {
                kernel: Some(kernel.into()),
                host: host(&server),
                keys: vec![LocalKey { path: "~/.ssh/id_ed25519".into(), name: "id_ed25519".into(), kind: "ED25519".into() }],
                windows: false,
                tor: None,
            };
            Reply::Facts { server, facts }
        }
        Ask::ReadKey { server, path } => Reply::Key { server, path, line: Some(KEY.into()) },
    }
}

/// The demo's snippets: 0.10's starters and one kept for web-7731.
pub fn snippets() -> Snippets {
    let mut list = Snippets::default();
    sshee_app::snippets::save(&mut list, None, "Tail the shop's log", "tail -f /var/log/shop/worker.log", Some("host:demo-web"));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_answers_hold_together() {
        let r = report("host:demo-web");
        assert!(r.findings.iter().any(|f| f.verdict == Verdict::Bad && f.fix.is_some()));
        assert!(report("host:demo-staging").findings.iter().all(|f| f.verdict != Verdict::Bad));
        assert!(sshee_app::recipes::parse_public_key(KEY, None).is_some(), "the made-up key reads as a key");
        assert!(matches!(answer(Ask::Fix { server: "host:demo-web".into(), check: "x".into(), command: "sudo apt-get upgrade".into() }), Reply::Fixed { outcome: Outcome::NeedsYou { .. }, .. }));
        let Reply::Facts { facts, .. } = answer(Ask::Facts { server: "host:demo-db".into() }) else { panic!() };
        assert_eq!(sshee_app::recipes::guess_os(facts.kernel.as_deref().unwrap()), Some(sshee_app::recipes::Os::Rhel));
        assert_eq!(snippets().snippets.len(), Snippets::default().snippets.len() + 1);
    }
}
