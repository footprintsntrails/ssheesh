//! The recipes themselves, word for word 0.10's (`RECIPES` in src/lib/recipes.ts).
//!
//! Commands are single lines (a newline would run them on paste), safe to run twice, and
//! write sshd settings to files of their own whose names match the security check's fixes.

use super::{Ctx, Field, FieldKind, Group, Os, Recipe, SshOptions, Step, Text};

/// Every recipe's id, in the order the list shows them.
pub const RECIPE_IDS: [&str; 8] = ["ssh-keys", "firewall", "fail2ban", "auto-updates", "sudo-user", "docker", "swap", "caddy-https"];

/// The recipe that fixes a security check finding, by the check's id.
pub fn recipe_for_check(check: &str) -> Option<&'static str> {
    match check {
        "password-login" => Some("ssh-keys"),
        "root-login" => Some("sudo-user"),
        "firewall" => Some("firewall"),
        "brute-force" => Some("fail2ban"),
        "auto-updates" => Some("auto-updates"),
        _ => None,
    }
}

/// A recipe by its id.
pub fn recipe(id: &str) -> Option<Recipe> {
    all().into_iter().find(|r| r.id == id)
}

/// A step with only what every step has; the rest is set field by field.
fn step(title: &'static str, explain: impl Into<Text>, command: impl Into<Text>) -> Step {
    Step {
        title,
        explain: explain.into(),
        command: command.into(),
        local: false,
        only: None,
        warn: None,
        lockout: false,
        check: false,
        expect: None,
        if_fails: None,
        needs: &[],
        skip_if_key: false,
    }
}

impl Step {
    fn local(mut self) -> Step {
        self.local = true;
        self
    }
    fn only(mut self, os: Os) -> Step {
        self.only = Some(os);
        self
    }
    fn warn(mut self, warn: impl Into<Text>) -> Step {
        self.warn = Some(warn.into());
        self
    }
    fn lockout(mut self) -> Step {
        self.lockout = true;
        self
    }
    fn check(mut self) -> Step {
        self.check = true;
        self
    }
    fn expect(mut self, expect: impl Into<Text>) -> Step {
        self.expect = Some(expect.into());
        self
    }
    fn if_fails(mut self, explain: impl Into<Text>, command: impl Into<Text>) -> Step {
        self.if_fails = Some((explain.into(), command.into()));
        self
    }
    fn needs(mut self, needs: &'static [&'static str]) -> Step {
        self.needs = needs;
        self
    }
    fn skip_if_key(mut self) -> Step {
        self.skip_if_key = true;
        self
    }
}

const SSHD_DIR: &str = "/etc/ssh/sshd_config.d";

fn reload_ssh(os: Os) -> &'static str {
    match os {
        Os::Debian => "sudo systemctl reload ssh",
        Os::Rhel => "sudo systemctl reload sshd",
    }
}

/// Writes sshd settings to a file of their own. sshd keeps the first value it reads and reads
/// sshd_config.d in name order, so "00-" beats cloud-init's 50-cloud-init.conf.
fn sshd_set(file: &'static str, lines: &'static [&'static str]) -> Text {
    Text::by(move |c| {
        let body: String = lines.iter().map(|l| format!("{l}\\n")).collect();
        format!("printf '{body}' | sudo tee {SSHD_DIR}/{file} >/dev/null && sudo sshd -t && {}", reload_ssh(c.os))
    })
}

fn sshd_undo(file: &'static str) -> Text {
    Text::by(move |c| format!("sudo rm -f {SSHD_DIR}/{file} && {}", reload_ssh(c.os)))
}

fn is_port(s: &str) -> bool {
    super::is_port(s)
}

/// A user name a recipe may create: lowercase, starting with a letter, not one the system uses.
fn is_new_user(s: &str) -> bool {
    const TAKEN: [&str; 10] = ["root", "daemon", "bin", "sys", "nobody", "admin", "sudo", "wheel", "docker", "caddy"];
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && s.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && !TAKEN.contains(&s)
}

/// A domain like example.com: labels of letters, digits and inner hyphens, the last one
/// starting with a letter.
fn is_domain(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    let labels: Vec<&str> = s.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !l.starts_with('-') && !l.ends_with('-')
    };
    s.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(label_ok)
        && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.starts_with(|c: char| c.is_ascii_alphabetic()))
}

const PORT_HELP: &str = "A number from 1 to 65535.";

fn ssh_port_field() -> Field {
    Field {
        id: "sshPort",
        label: "SSH port",
        kind: FieldKind::Text { placeholder: "22", valid: Some((is_port, PORT_HELP)) },
        help: "SSHeesh filled in the port it connects to. The first step shows the server's own; change this if they differ.",
        initial: Some(Text::by(|c| c.host_port.to_string())),
    }
}

fn see_ssh_port() -> Step {
    step(
        "See which port SSH uses",
        Text::by(|c| {
            format!(
                "SSHeesh reaches this server on port {}. This shows the port the SSH server itself listens on. They're nearly always the same; if not, put the one shown here in the SSH port box above.",
                c.host_port
            )
        }),
        "sudo sshd -T | grep -i '^port '",
    )
    .expect(Text::by(|c| format!("port {}", c.ssh_port)))
}

fn key_login() -> Text {
    Text::by(|c: &Ctx| {
        let file = c.values.get("keyFile").map(String::as_str);
        c.ssh(SshOptions { key_only: true, identity: file, run: Some("echo Key login works"), ..SshOptions::default() })
    })
}

/// Adds the key to ~/.ssh/authorized_keys unless it's there, like ssh-copy-id.
fn add_key() -> Text {
    Text::by(|c| {
        let key = c.v("publicKey");
        let blob = key.split(' ').nth(1).unwrap_or("");
        let add = format!(
            "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && {{ grep -qF '{blob}' ~/.ssh/authorized_keys || echo '{key}' >> ~/.ssh/authorized_keys; }}"
        );
        // SELinux only lets sshd read files labelled as SSH files.
        if c.os == Os::Rhel {
            format!("{add} && restorecon -R ~/.ssh")
        } else {
            add
        }
    })
}

/// While firewalld is off, firewall-offline-cmd edits the rules it starts with.
fn firewalld_allow_ssh(c: &Ctx) -> String {
    let rule = if c.ssh_port == 22 { "--add-service=ssh".to_owned() } else { format!("--add-port={}/tcp", c.ssh_port) };
    format!("if systemctl is-active --quiet firewalld; then sudo firewall-cmd --permanent {rule} && sudo firewall-cmd --reload; else sudo firewall-offline-cmd {rule}; fi")
}

fn web_in() -> Text {
    Text::os("sudo ufw allow 80/tcp && sudo ufw allow 443/tcp", "sudo firewall-cmd --permanent --add-service=http --add-service=https && sudo firewall-cmd --reload")
}

const DOCKER_PORTS: &str = "sudo docker ps --format '{{.Names}}: {{.Ports}}'";
const IF_DNF5: &str = "if command -v dnf5 >/dev/null 2>&1; then";

/// Every recipe, in order.
pub fn all() -> Vec<Recipe> {
    vec![ssh_keys(), firewall(), fail2ban(), auto_updates(), sudo_user(), docker(), swap(), caddy()]
}

fn ssh_keys() -> Recipe {
    Recipe {
        id: "ssh-keys",
        group: Group::Secure,
        title: "Log in with a key instead of a password",
        summary: "Set up an SSH key, then turn password login off so bots can't guess their way in.",
        who: "Anyone who still types a password to log in to this server.",
        fields: vec![Field { id: "publicKey", label: "Your key", kind: FieldKind::Key, help: "The key on this computer that should log in to the server.", initial: None }],
        steps: vec![
            step(
                "Make a key on your computer",
                "A key is a pair of files. The private half stays on your computer, like a house key; the public half (the .pub file) goes on servers, like a lock only that key opens. Press Enter to keep the usual place. A passphrase is optional: it locks the key with a password of its own.",
                "ssh-keygen -t ed25519",
            )
            .local()
            .skip_if_key()
            .expect("\"Your public key has been saved in\", then a path ending in .pub. If it asks to overwrite, answer n: you already have a key."),
            step(
                "Put your public key on the server",
                "This adds the public half of your key to ~/.ssh/authorized_keys, the list of keys allowed to log in as you. Run it as the user you log in as. Running it twice is fine: a key that's already there isn't added again.",
                add_key(),
            )
            .needs(&["publicKey"]),
            step(
                "Check that the key works",
                "In a new terminal on your computer, this logs in to the server afresh with your key only, never a password. If your key has a passphrase, it asks for that, which is fine.",
                key_login(),
            )
            .local()
            .check()
            .expect("Key login works")
            .if_fails(
                "\"Permission denied (publickey)\" means the server didn't take the key. SSH ignores keys when other users could change your home folder or ~/.ssh, so this tightens both on the server. Then check again, and don't go on until it works.",
                "chmod go-w ~ && chmod 700 ~/.ssh && chmod 600 ~/.ssh/authorized_keys",
            ),
            step(
                "Turn off password login",
                "This puts two settings in a file of their own, /etc/ssh/sshd_config.d/00-ssheesh.conf. Many cloud servers have a 50-cloud-init.conf next to it that turns password login back on. SSH keeps the first value it reads and reads these files in name order, so 00 wins. sshd -t checks for mistakes before SSH reloads.",
                sshd_set("00-ssheesh.conf", &["PasswordAuthentication no", "KbdInteractiveAuthentication no"]),
            )
            .lockout()
            .warn("Only do this once the check above works, or you could lock yourself out. Keep this terminal open until the last check passes: it stays logged in, so you can undo the change from here."),
            step(
                "Check that password login is off",
                "This asks SSH which settings it really uses after reading every file. Keyboard-interactive is the other way a password can be typed in.",
                "sudo sshd -T | grep -iE '^(passwordauthentication|kbdinteractiveauthentication) '",
            )
            .check()
            .expect("passwordauthentication no and kbdinteractiveauthentication no")
            .if_fails(
                "If either still says yes, a file that's read earlier sets it. This lists every file that mentions them, so you can change the setting there.",
                "sudo grep -riE '^\\s*(PasswordAuthentication|KbdInteractiveAuthentication)' /etc/ssh/sshd_config /etc/ssh/sshd_config.d",
            ),
            step(
                "Check you can still log in",
                "In a new terminal on your computer, log in once more with your key. Close the old terminal only after this works. SSHeesh tries your usual keys first, so it keeps signing in; if you picked a key with another name, add the server to SSHeesh again with that key.",
                key_login(),
            )
            .local()
            .check()
            .expect("Key login works")
            .if_fails("In the terminal you kept open, this turns password login back on. Then go back to the first check.", sshd_undo("00-ssheesh.conf")),
        ],
    }
}

fn firewall() -> Recipe {
    Recipe {
        id: "firewall",
        group: Group::Secure,
        title: "Turn on the firewall",
        summary: "Block every incoming connection you didn't ask for, without cutting yourself off.",
        who: "Any server on the internet that has no firewall turned on yet.",
        fields: vec![ssh_port_field()],
        steps: vec![
            see_ssh_port(),
            step(
                "Install the firewall",
                Text::os(
                    "ufw, the \"uncomplicated firewall\", is the usual one on Debian and Ubuntu. Ubuntu comes with it, so there this changes nothing. A firewall in your hosting provider's control panel is separate and can run alongside.",
                    "firewalld is the usual firewall on Fedora and RHEL, and is often installed already. A firewall in your hosting provider's control panel is separate and can run alongside.",
                ),
                Text::os("sudo apt-get install -y ufw", "sudo dnf install -y firewalld"),
            ),
            step(
                "Let SSH in first",
                Text::os(
                    "A rule says what may come in. Adding the rule for SSH before the firewall is on means turning it on can't cut you off.",
                    "A rule says what may come in. firewalld lets SSH in on port 22 out of the box; this makes sure the rule for your SSH port is there before it's on.",
                ),
                Text::os(Text::by(|c| format!("sudo ufw allow {}/tcp", c.ssh_port)), Text::by(firewalld_allow_ssh)),
            )
            .needs(&["sshPort"]),
            step(
                "Turn the firewall on",
                Text::os(
                    "ufw turns on now and after every restart. It warns that this may disrupt SSH connections and asks whether to go on: SSH is allowed, so type y and press Enter. From now on only what you allowed can come in; the server can still reach out.",
                    "firewalld starts now and after every restart. From now on only what you allowed can come in; the server can still reach out.",
                ),
                Text::os("sudo ufw enable", "sudo systemctl enable --now firewalld"),
            )
            .lockout()
            .warn("If the SSH rule is wrong, this blocks new logins. Keep this terminal open: the connection you're using stays up, so you can undo it from here."),
            step(
                "Check you can still log in",
                "In a new terminal on your computer, log in to the server afresh. Close the old terminal only after this works.",
                Text::by(|c| c.ssh(SshOptions { run: Some("echo Still in"), ..SshOptions::default() })),
            )
            .local()
            .check()
            .expect("Still in")
            .if_fails(
                "In the terminal you kept open, this turns the firewall off again. Then check the SSH port and try once more.",
                Text::os("sudo ufw disable", "sudo systemctl disable --now firewalld"),
            ),
            step("See what's allowed in", "This lists the firewall's rules, so you can see what's open.", Text::os("sudo ufw status verbose", "sudo firewall-cmd --list-all"))
                .check()
                .expect(Text::os(
                    Text::by(|c| format!("Status: active, and {}/tcp with ALLOW IN", c.ssh_port)),
                    Text::by(|c| if c.ssh_port == 22 { "ssh on the services line".to_owned() } else { format!("{}/tcp on the ports line", c.ssh_port) }),
                )),
            step(
                "Let web traffic in, if this is a web server",
                "Websites use port 80 (http) and 443 (https). Skip this if the server doesn't serve a website. Open other ports only if the whole internet should reach them; a database usually shouldn't.",
                web_in(),
            ),
            step(
                "Know that Docker goes around it",
                "Ports that Docker publishes (docker run -p, or ports: in docker-compose.yml) skip the firewall and stay open to everyone. This lists them: 0.0.0.0:5432->5432/tcp is open to the whole internet, 127.0.0.1:5432->5432/tcp only to this server. To make one private, publish it as 127.0.0.1:5432:5432. Skip this if you don't use Docker.",
                DOCKER_PORTS,
            ),
        ],
    }
}

fn fail2ban() -> Recipe {
    Recipe {
        id: "fail2ban",
        group: Group::Secure,
        title: "Stop password guessing with fail2ban",
        summary: "Block an address for an hour after it fails to log in a few times.",
        who: "Servers that still allow password login, or whose logs fill up with failed logins.",
        fields: vec![ssh_port_field()],
        steps: vec![
            see_ssh_port(),
            step(
                "Install fail2ban",
                Text::os(
                    "fail2ban watches for failed logins and blocks an address that keeps failing. python3-systemd lets it read them from the system journal, where newer Debian keeps them.",
                    "fail2ban watches for failed logins and blocks an address that keeps failing. Fedora has it built in. Rocky Linux, AlmaLinux and CentOS Stream get it from EPEL, a widely used extra package collection, so this adds EPEL first. On Red Hat's own RHEL, set up EPEL with Red Hat's guide instead.",
                ),
                Text::os(
                    "sudo apt-get update && sudo apt-get install -y fail2ban python3-systemd",
                    "{ grep -q '^ID=fedora' /etc/os-release || sudo dnf install -y epel-release; } && sudo dnf install -y fail2ban python3-systemd",
                ),
            ),
            step(
                "Tell it to guard SSH",
                Text::os(
                    "This writes the rules to a file of their own: an address that fails 5 logins within 10 minutes is blocked for an hour. That includes you, if you mistype your password too often.",
                    "This writes the rules to a file of their own: an address that fails 5 logins within 10 minutes is blocked for an hour. That includes you, if you mistype your password too often. fail2ban blocks through firewalld here, so turn the firewall on first.",
                ),
                Text::by(|c| {
                    format!(
                        "printf '[sshd]\\nenabled = true\\nport = {}\\nbackend = systemd\\nmaxretry = 5\\nfindtime = 10m\\nbantime = 1h\\n' | sudo tee /etc/fail2ban/jail.d/ssheesh.local >/dev/null",
                        c.ssh_port
                    )
                }),
            )
            .needs(&["sshPort"]),
            step("Start it", "This starts fail2ban with the new rules, now and after every restart.", "sudo systemctl enable fail2ban && sudo systemctl restart fail2ban"),
            step(
                "Check it's watching",
                "If it can't reach the socket, fail2ban is still starting: wait a few seconds and try again. To unblock an address later, for example 203.0.113.7, run sudo fail2ban-client set sshd unbanip 203.0.113.7.",
                "sudo fail2ban-client status sshd",
            )
            .check()
            .expect("Status for the jail: sshd, with counts of failed logins and blocked addresses")
            .if_fails("fail2ban's own log says why it didn't start.", "sudo journalctl -u fail2ban -n 30 --no-pager"),
        ],
    }
}

fn auto_updates() -> Recipe {
    Recipe {
        id: "auto-updates",
        group: Group::Secure,
        title: "Install security updates automatically",
        summary: "Let the server patch known security holes every day, without you.",
        who: "Nearly every server, unless someone updates it by hand every week.",
        fields: vec![],
        steps: vec![
            step(
                "Install unattended-upgrades",
                "unattended-upgrades installs updates with nobody logged in. Out of the box it only installs security updates, which rarely break anything. Ubuntu usually has it already.",
                "sudo apt-get update && sudo apt-get install -y unattended-upgrades",
            )
            .only(Os::Debian),
            step(
                "Run it every day",
                "This tells apt to refresh its package lists and run unattended-upgrades once a day. It's the same file Debian's own setup writes.",
                "printf 'APT::Periodic::Update-Package-Lists \"1\";\\nAPT::Periodic::Unattended-Upgrade \"1\";\\n' | sudo tee /etc/apt/apt.conf.d/20auto-upgrades >/dev/null",
            )
            .only(Os::Debian),
            step(
                "Check the setting",
                "This shows the two settings apt actually uses, after reading all its files.",
                "apt-config dump | grep -E '^APT::Periodic::(Update-Package-Lists|Unattended-Upgrade) '",
            )
            .only(Os::Debian)
            .check()
            .expect("Both lines end in \"1\";"),
            step(
                "Install dnf-automatic",
                "dnf-automatic installs updates with nobody logged in. Newer Fedora calls it dnf5-plugin-automatic, so this picks the right name.",
                format!("{IF_DNF5} sudo dnf install -y dnf5-plugin-automatic; else sudo dnf install -y dnf-automatic; fi"),
            )
            .only(Os::Rhel),
            step(
                "Have it install security updates",
                "Out of the box it only downloads updates. This makes it install them, limited to security updates, which rarely break anything.",
                format!("{IF_DNF5} printf '[commands]\\nupgrade_type = security\\napply_updates = yes\\n' | sudo tee /etc/dnf/automatic.conf >/dev/null; else sudo sed -i -e 's/^upgrade_type *=.*/upgrade_type = security/' -e 's/^apply_updates *=.*/apply_updates = yes/' /etc/dnf/automatic.conf; fi"),
            )
            .only(Os::Rhel),
            step(
                "Run it every day",
                "A timer runs it once a day, starting now and after every restart.",
                format!("{IF_DNF5} sudo systemctl enable --now dnf5-automatic.timer; else sudo systemctl enable --now dnf-automatic.timer; fi"),
            )
            .only(Os::Rhel),
            step(
                "Check the timer",
                "This shows when updates run next. Some updates, like a new kernel, only take effect after a restart; SSHeesh's security check tells you when one is waiting.",
                Text::os("systemctl list-timers apt-daily-upgrade.timer", "systemctl list-timers 'dnf*automatic*'"),
            )
            .check()
            .expect("A timer with a time in the NEXT column"),
        ],
    }
}

fn sudo_user() -> Recipe {
    let copy_key = |c: &Ctx| {
        let u = c.v("newUser");
        let base = format!("sudo install -d -m 700 -o {u} -g {u} /home/{u}/.ssh && sudo install -m 600 -o {u} -g {u} ~/.ssh/authorized_keys /home/{u}/.ssh/authorized_keys");
        if c.os == Os::Rhel {
            format!("{base} && sudo restorecon -R /home/{u}/.ssh")
        } else {
            base
        }
    };
    Recipe {
        id: "sudo-user",
        group: Group::Secure,
        title: "Use your own user instead of root",
        summary: "Make a user that runs admin commands with sudo, then stop root from logging in over SSH.",
        who: "Anyone who logs in to this server as root.",
        fields: vec![Field {
            id: "newUser",
            label: "Name for the new user",
            kind: FieldKind::Text {
                placeholder: "alex",
                valid: Some((is_new_user, "Lowercase letters, digits, - and _, starting with a letter, and not a name the system already uses.")),
            },
            help: "Lowercase, like alex or sam. You'll log in with it from now on.",
            initial: None,
        }],
        steps: vec![
            step(
                "Make sure sudo is there",
                "sudo lets a normal user run one command as root, after typing their own password. Most servers have it; a minimal Debian doesn't. If it's missing, you must be root, so this installs it without sudo.",
                Text::os("command -v sudo >/dev/null || apt-get install -y sudo", "command -v sudo >/dev/null || dnf install -y sudo"),
            ),
            step(
                "Create the user",
                Text::os(
                    "It asks for the new user's password twice: pick a long one, you'll type it for sudo. It then asks for a full name and such; press Enter to skip those.",
                    "It then asks for the new user's password twice: pick a long one, you'll type it for sudo.",
                ),
                Text::os(Text::by(|c| format!("sudo adduser {}", c.v("newUser"))), Text::by(|c| format!("sudo useradd -m {0} && sudo passwd {0}", c.v("newUser")))),
            )
            .needs(&["newUser"]),
            step(
                "Let it use sudo",
                Text::os("Members of the sudo group may run commands as root after typing their own password.", "Members of the wheel group may run commands as root after typing their own password."),
                Text::os(Text::by(|c| format!("sudo usermod -aG sudo {}", c.v("newUser"))), Text::by(|c| format!("sudo usermod -aG wheel {}", c.v("newUser")))),
            )
            .needs(&["newUser"]),
            step(
                "Give it your key",
                "This copies the keys that log you in now to the new user, so the same key works for both. If you log in with a password, skip this: the new user logs in with the password you just set.",
                Text::by(copy_key),
            )
            .needs(&["newUser"]),
            step(
                "Check the new user can log in and use sudo",
                "In a new terminal on your computer, this logs in as the new user and runs one command with sudo, which asks for the new user's password.",
                Text::by(|c| c.ssh(SshOptions { user: Some(c.v("newUser")), tty: true, run: Some("sudo whoami"), ..SshOptions::default() })),
            )
            .local()
            .check()
            .needs(&["newUser"])
            .expect("root")
            .if_fails(
                "This shows the new user's groups on the server: sudo (or wheel on Fedora and RHEL) should be among them. Don't go on until the check works.",
                Text::by(|c| format!("id {}", c.v("newUser"))),
            ),
            step(
                "Stop root logging in over SSH",
                "From now on you log in as the new user and put sudo in front of admin commands. This writes PermitRootLogin no to a file of its own, checks it with sshd -t and reloads SSH. If SSHeesh logs in to this server as root, add the server again with the new user's name.",
                sshd_set("00-ssheesh-root.conf", &["PermitRootLogin no"]),
            )
            .lockout()
            .warn("Only do this once the check above works. Keep this terminal open until the last check passes, so you can undo it from here."),
            step("Check root is turned away", "This asks SSH which setting it really uses after reading every file.", "sudo sshd -T | grep -i '^permitrootlogin '")
                .check()
                .expect("permitrootlogin no"),
            step(
                "Check you can still get in",
                "In a new terminal on your computer, log in as the new user once more. Close the old terminal only after this works.",
                Text::by(|c| c.ssh(SshOptions { user: Some(c.v("newUser")), run: Some("echo Still in"), ..SshOptions::default() })),
            )
            .local()
            .check()
            .needs(&["newUser"])
            .expect("Still in")
            .if_fails("In the terminal you kept open, this lets root log in again.", sshd_undo("00-ssheesh-root.conf")),
        ],
    }
}

fn docker() -> Recipe {
    Recipe {
        id: "docker",
        group: Group::Setup,
        title: "Install Docker",
        summary: "Install Docker from Docker's own package source, so you get current versions and their updates.",
        who: "Anyone who wants to run apps in containers on this server.",
        fields: vec![],
        steps: vec![
            step(
                "Remove other Docker packages",
                "Debian and Ubuntu have Docker packages of their own that clash with Docker's. This removes the ones that are installed; if apt asks, answer Y. Images and volumes in /var/lib/docker stay.",
                "sudo apt-get remove $(dpkg --get-selections docker.io docker-compose docker-compose-v2 docker-doc podman-docker containerd runc 2>/dev/null | cut -f1)",
            )
            .only(Os::Debian),
            step(
                "Add Docker's signing key",
                "apt checks every package against its source's signing key. This downloads Docker's key, for Debian or Ubuntu as the server is.",
                "sudo apt-get update && sudo apt-get install -y ca-certificates curl && sudo install -m 0755 -d /etc/apt/keyrings && sudo curl -fsSL \"https://download.docker.com/linux/$(. /etc/os-release && echo \"$ID\")/gpg\" -o /etc/apt/keyrings/docker.asc && sudo chmod a+r /etc/apt/keyrings/docker.asc",
            )
            .only(Os::Debian),
            step(
                "Add Docker's package source",
                "This tells apt where Docker's packages are, matched to your system's release.",
                "printf 'Types: deb\\nURIs: https://download.docker.com/linux/%s\\nSuites: %s\\nComponents: stable\\nSigned-By: /etc/apt/keyrings/docker.asc\\n' \"$(. /etc/os-release && echo \"$ID\")\" \"$(. /etc/os-release && echo \"${UBUNTU_CODENAME:-$VERSION_CODENAME}\")\" | sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null",
            )
            .only(Os::Debian),
            step(
                "Add Docker's package source",
                "This downloads the file that tells dnf where Docker's packages are: Docker's Fedora or RHEL source, or the CentOS one, which Rocky Linux and AlmaLinux use.",
                "sudo curl -fsSL -o /etc/yum.repos.d/docker-ce.repo \"https://download.docker.com/linux/$(. /etc/os-release; case \"$ID\" in fedora|rhel) echo \"$ID\" ;; *) echo centos ;; esac)/docker-ce.repo\"",
            )
            .only(Os::Rhel),
            step(
                "Install Docker",
                Text::os(
                    "Installs Docker, with Compose for docker-compose.yml files and Buildx for building images, and starts it now and after every restart.",
                    "Installs Docker, with Compose for docker-compose.yml files and Buildx for building images, and starts it now and after every restart. If dnf says it clashes with podman-docker, remove that first with sudo dnf remove podman-docker.",
                ),
                Text::os(
                    "sudo apt-get update && sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin && sudo systemctl enable --now docker",
                    "sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin && sudo systemctl enable --now docker",
                ),
            ),
            step("Check that Docker works", "This downloads a tiny test image and runs it once.", "sudo docker run --rm hello-world").check().expect("Hello from Docker!"),
            step(
                "Use docker without sudo",
                "This puts you in the docker group, so you can type docker instead of sudo docker. Skip it if you log in as root.",
                "sudo usermod -aG docker \"$USER\"",
            )
            .warn("Anyone in the docker group can take full control of the server, the same as root. Only add users you'd trust with root."),
            step(
                "Pick up the new group",
                "A new group only reaches new logins, and SSHeesh keeps your terminal running between connections. newgrp starts a shell in this terminal that has the group already; type exit to leave it.",
                "newgrp docker",
            ),
            step("Check docker works without sudo", "The same test as before, this time without sudo.", "docker run --rm hello-world")
                .check()
                .expect("Hello from Docker!")
                .if_fails("\"permission denied\" means this shell doesn't have the group yet. This lists your groups: run newgrp docker if docker isn't among them.", "id -nG"),
            step(
                "Keep containers' ports private",
                "Ports Docker publishes skip the firewall and are open to the whole internet. In docker-compose.yml, write a port as \"127.0.0.1:8080:80\" so only this server reaches it, and put Caddy in front for the web (see the HTTPS recipe). This lists what's published now: 0.0.0.0 means open to everyone.",
                DOCKER_PORTS,
            ),
        ],
    }
}

fn swap() -> Recipe {
    Recipe {
        id: "swap",
        group: Group::Setup,
        title: "Add swap",
        summary: "Give the server spare memory on disk, so a busy moment slows apps down instead of crashing them.",
        who: "Small servers, with 2 GB of memory or less, that have no swap yet.",
        fields: vec![Field {
            id: "swapSize",
            label: "Swap size",
            kind: FieldKind::Choice(&[("1", "1 GB"), ("2", "2 GB"), ("4", "4 GB"), ("8", "8 GB")]),
            help: "As much as the server's memory is plenty; 2 GB suits most small servers.",
            initial: Some("2".into()),
        }],
        steps: vec![
            step(
                "See memory, swap and free space",
                "free shows memory (Mem) and swap (Swap); a swap file takes disk space, so check / has room for it. Fedora often has swap already, as zram (compressed memory), and may not need more.",
                "free -h && swapon --show && df -h /",
            )
            .expect("Swap: 0B means there's no swap yet"),
            step(
                "Create the swap file",
                "This makes /swapfile, readable only by root, and sets it up as swap. Filling it can take a minute. btrfs, the file system Fedora uses, needs its own command for swap files, so this picks the right one. If /swapfile exists, it leaves it alone.",
                Text::by(|c| {
                    let gb: u64 = c.v("swapSize").parse().unwrap_or(2);
                    format!(
                        "if [ -e /swapfile ]; then echo \"/swapfile already exists\"; elif [ \"$(stat -f -c %T /)\" = btrfs ]; then sudo btrfs filesystem mkswapfile --size {gb}G /swapfile; else sudo dd if=/dev/zero of=/swapfile bs=1M count={} status=progress && sudo chmod 600 /swapfile && sudo mkswap /swapfile; fi",
                        gb * 1024
                    )
                }),
            )
            .needs(&["swapSize"]),
            step("Turn it on", "The server starts using it right away. If it says the device is busy, it's on already.", "sudo swapon /swapfile"),
            step(
                "Keep it on after a restart",
                "This adds /swapfile to /etc/fstab, the list of disks and files set up at startup, unless it's there already.",
                "grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap defaults 0 0' | sudo tee -a /etc/fstab >/dev/null && sudo systemctl daemon-reload",
            ),
            step("Check the swap", "The same look as at the start.", "swapon --show && free -h")
                .check()
                .expect(Text::by(|c| format!("/swapfile with a size of {}G, and swap in free's Swap line", if c.v("swapSize").is_empty() { "2" } else { c.v("swapSize") }))),
        ],
    }
}

fn caddy() -> Recipe {
    Recipe {
        id: "caddy-https",
        group: Group::Setup,
        title: "Put a web app behind HTTPS with Caddy",
        summary: "Serve an app that runs on this server at your own domain, with a free HTTPS certificate that renews itself.",
        who: "Anyone with an app on a local port and a domain name to point at this server.",
        fields: vec![
            Field {
                id: "domain",
                label: "Domain",
                kind: FieldKind::Text { placeholder: "example.com", valid: Some((is_domain, "A domain like example.com or app.example.com, without https:// or a path.")) },
                help: "The name people type to reach your app, like example.com or app.example.com.",
                initial: None,
            },
            Field {
                id: "appPort",
                label: "Port your app listens on",
                kind: FieldKind::Text { placeholder: "3000", valid: Some((is_port, PORT_HELP)) },
                help: "Often 3000 for Node apps, 8000 for Python, 8080 for Java.",
                initial: None,
            },
        ],
        steps: vec![
            step(
                "Point your domain at this server",
                "Where you bought the domain, or at your DNS provider, add an A record for it with this server's public IP address (and an AAAA record if the server has IPv6). This shows where the domain points now. Changes can take a few minutes to show.",
                Text::by(|c| format!("getent ahosts {} | awk '{{print $1}}' | sort -u", c.v("domain"))),
            )
            .needs(&["domain"])
            .expect("This server's public address. Nothing at all means the domain doesn't point anywhere yet."),
            step("Let web traffic in", "Caddy needs ports 80 and 443 open, both to get a certificate and to serve your app. Skip this if no firewall is on.", web_in()),
            step(
                "Add Caddy's package source",
                "Caddy is a web server that gets and renews HTTPS certificates by itself. This adds the Caddy project's own package source and signing key, as Caddy's install guide does.",
                "sudo apt-get update && sudo apt-get install -y curl gpg && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null && sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list",
            )
            .only(Os::Debian),
            step(
                "Install Caddy",
                Text::os(
                    "This installs Caddy and starts it, now and after every restart. It runs as its own user, caddy, not as root.",
                    "Caddy is a web server that gets and renews HTTPS certificates by itself. It comes from COPR, Fedora's community package service, where the Caddy team publishes it, as Caddy's install guide says. This installs it and starts it, now and after every restart.",
                ),
                Text::os(
                    "sudo apt-get update && sudo apt-get install -y caddy",
                    "sudo dnf install -y 'dnf-command(copr)' && sudo dnf copr enable -y @caddy/caddy && sudo dnf install -y caddy && sudo systemctl enable --now caddy",
                ),
            ),
            step(
                "Send your domain to the app",
                "Caddy's settings file gets three lines: requests for your domain go to your app on this server. Your app only needs to listen on localhost; Caddy faces the internet and handles HTTPS.",
                Text::by(|c| {
                    format!(
                        "[ -e /etc/caddy/Caddyfile.orig ] || sudo cp /etc/caddy/Caddyfile /etc/caddy/Caddyfile.orig; printf '%s {{\\n\\treverse_proxy localhost:%s\\n}}\\n' '{}' '{}' | sudo tee /etc/caddy/Caddyfile >/dev/null",
                        c.v("domain"),
                        c.v("appPort")
                    )
                }),
            )
            .needs(&["domain", "appPort"])
            .warn("This replaces /etc/caddy/Caddyfile, keeping the first one as Caddyfile.orig. If Caddy already serves other sites here, add these lines to the file instead."),
            step(
                "Load the new settings",
                "Caddy checks the new settings first and keeps the old ones running if something's wrong. The first time, it gets a certificate for your domain, which takes a few seconds.",
                "sudo systemctl reload caddy",
            ),
            step(
                "Check the site",
                "This asks your domain for the page over HTTPS and shows the first line of the answer.",
                Text::by(|c| format!("curl -sSI https://{} | head -n 1", c.v("domain"))),
            )
            .check()
            .needs(&["domain"])
            .expect("HTTP/2 200, or another code your app sends. 502 means Caddy works but nothing answers on your app's port.")
            .if_fails(
                "Caddy's log says why, most often that the domain doesn't point here yet or ports 80 and 443 are closed.",
                "sudo journalctl -u caddy -n 30 --no-pager",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Ctx, Facts, Os};
    use super::*;
    use std::collections::BTreeMap;

    fn ctx(os: Os, values: &[(&str, &str)]) -> Ctx {
        let values: BTreeMap<String, String> = values.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        Ctx::new(os, &Facts::default(), values)
    }

    #[test]
    fn every_recipe_is_listed_with_unique_step_titles_per_system() {
        let ids: Vec<&str> = all().iter().map(|r| r.id).collect();
        assert_eq!(ids, RECIPE_IDS);
        for r in all() {
            for os in Os::ALL {
                let steps = r.steps_for(os);
                assert!(!steps.is_empty(), "{} has no steps for {os:?}", r.id);
                let mut titles: Vec<&str> = steps.iter().map(|s| s.title).collect();
                titles.sort_unstable();
                titles.dedup();
                assert_eq!(titles.len(), steps.len(), "{} repeats a step title", r.id);
                // Every command is one line: a newline would run it on paste.
                let c = ctx(os, &[("sshPort", "22"), ("newUser", "alex"), ("domain", "shop.example.com"), ("appPort", "3000"), ("swapSize", "2"), ("publicKey", "ssh-ed25519 AAAA x")]);
                for s in steps {
                    assert!(!s.command.get(&c).contains('\n'), "{}: {}", r.id, s.title);
                    for n in s.needs {
                        assert!(r.field(n).is_some(), "{} needs a field it hasn't: {n}", r.id);
                    }
                }
            }
        }
        for check in ["password-login", "root-login", "firewall", "brute-force", "auto-updates"] {
            assert!(recipe(recipe_for_check(check).unwrap()).is_some());
        }
        assert_eq!(recipe_for_check("reboot"), None);
    }

    #[test]
    fn commands_follow_the_system_and_the_port() {
        let fw = recipe("firewall").unwrap();
        let allow = fw.steps.iter().find(|s| s.title == "Let SSH in first").unwrap();
        assert_eq!(allow.command.get(&ctx(Os::Debian, &[("sshPort", "2222")])), "sudo ufw allow 2222/tcp");
        assert!(allow.command.get(&ctx(Os::Rhel, &[])).contains("--add-service=ssh"));
        let keys = recipe("ssh-keys").unwrap();
        let off = keys.steps.iter().find(|s| s.title == "Turn off password login").unwrap();
        assert_eq!(
            off.command.get(&ctx(Os::Rhel, &[])),
            "printf 'PasswordAuthentication no\\nKbdInteractiveAuthentication no\\n' | sudo tee /etc/ssh/sshd_config.d/00-ssheesh.conf >/dev/null && sudo sshd -t && sudo systemctl reload sshd"
        );
        assert!(off.lockout && off.warn.is_some());
        let add = keys.steps.iter().find(|s| s.title == "Put your public key on the server").unwrap();
        let line = add.command.get(&ctx(Os::Debian, &[("publicKey", "ssh-ed25519 AAAAblob id_ed25519")]));
        assert!(line.contains("grep -qF 'AAAAblob'") && line.contains("echo 'ssh-ed25519 AAAAblob id_ed25519' >>"));
        let swap = recipe("swap").unwrap();
        assert!(swap.steps[1].command.get(&ctx(Os::Debian, &[("swapSize", "4")])).contains("count=4096"));
        assert_eq!(swap.steps_for(Os::Rhel).len(), 5);
        assert_eq!(recipe("auto-updates").unwrap().steps_for(Os::Debian).len(), 4);
    }

    #[test]
    fn fields_check_what_is_typed() {
        assert!(is_new_user("alex") && is_new_user("sam_2"));
        assert!(!is_new_user("root") && !is_new_user("Alex") && !is_new_user("2x") && !is_new_user(""));
        assert!(is_domain("example.com") && is_domain("app.shop.example.co"));
        assert!(!is_domain("https://example.com") && !is_domain("localhost") && !is_domain("-a.com") && !is_domain("a.1com"));
    }
}
