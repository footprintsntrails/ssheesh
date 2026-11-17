//! The security check and recipes without a screen (0.10's `ServerTools.tsx`, `Recipes.tsx`
//! and `src/lib/recipes.ts`).
//!
//! - **The security check** ([`checkup`]) reads a server and says, finding by finding, what
//!   it found and why it matters, worst first. A finding may offer a fix: a command shown in
//!   full before anything runs, run only after you confirm, and on a Production server only
//!   after you confirm a second time. Looking changes nothing.
//! - **Recipes** ([`book`]) are step-by-step guides for common server jobs (log in with a
//!   key, turn on the firewall, ...). Every step says in plain words what it does and why,
//!   then offers its command to paste into a terminal, without pressing Return: you run every
//!   command yourself. Commands are single lines, aimed at Debian 12/13, Ubuntu 22.04/24.04 and
//!   Fedora/RHEL 9, and safe to run twice.
//! - **What the window asks its backend** ([`Ask`], [`Reply`]): the check itself, running a
//!   confirmed fix, and what a recipe needs to know (the server's kernel, how it's reached,
//!   the keys on this computer). sshee-core's types are mirrored here, so this crate needs no
//!   ssh.

pub mod book;
pub mod checkup;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use book::{recipe, recipe_for_check, RECIPE_IDS};

/// Debian and Ubuntu (apt), or Fedora, RHEL and its rebuilds (dnf).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Os {
    #[default]
    Debian,
    Rhel,
}

impl Os {
    pub const ALL: [Os; 2] = [Os::Debian, Os::Rhel];

    pub fn label(self) -> &'static str {
        match self {
            Os::Debian => "Debian / Ubuntu",
            Os::Rhel => "Fedora / RHEL",
        }
    }
}

/// Debian/Ubuntu or Fedora/RHEL, guessed from the kernel's release ("Linux 6.8.0-45-generic",
/// "5.14.0-427.el9.x86_64"); `None` when it doesn't say. Containers report their host's
/// kernel, so it's only a guess.
pub fn guess_os(kernel: &str) -> Option<Os> {
    let k = kernel.to_lowercase();
    let rhel = ["el", "fc", "amzn"].iter().any(|tag| {
        k.match_indices(&format!(".{tag}")).any(|(at, m)| k[at + m.len()..].chars().next().is_some_and(|c| c.is_ascii_digit()))
    });
    if rhel {
        return Some(Os::Rhel);
    }
    let debian = ["-generic", "-amd64", "-arm64", "-cloud", "-aws", "-azure", "-gcp", "-gke", "-oracle", "-kvm", "-lowlatency", "-virtual", "-raspi", "+deb", "+rpt", "ubuntu", "debian"];
    debian.iter().any(|d| k.contains(d)).then_some(Os::Debian)
}

/// Text that may depend on the server and what's filled in.
#[derive(Clone)]
pub struct Text(Arc<dyn Fn(&Ctx) -> String + Send + Sync>);

impl Text {
    pub fn by(f: impl Fn(&Ctx) -> String + Send + Sync + 'static) -> Text {
        Text(Arc::new(f))
    }

    /// One text for Debian and Ubuntu, another for Fedora and RHEL.
    pub fn os(debian: impl Into<Text>, rhel: impl Into<Text>) -> Text {
        let (d, r) = (debian.into(), rhel.into());
        Text::by(move |c| match c.os {
            Os::Debian => d.get(c),
            Os::Rhel => r.get(c),
        })
    }

    pub fn get(&self, c: &Ctx) -> String {
        (self.0)(c)
    }
}

impl From<&'static str> for Text {
    fn from(s: &'static str) -> Text {
        Text::by(move |_| s.to_owned())
    }
}

impl From<String> for Text {
    fn from(s: String) -> Text {
        Text::by(move |_| s.clone())
    }
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Text")
    }
}

/// What a field takes.
#[derive(Debug, Clone)]
pub enum FieldKind {
    Text {
        placeholder: &'static str,
        /// Whether the text is usable, and what to say when it isn't.
        valid: Option<(fn(&str) -> bool, &'static str)>,
    },
    /// One of a few (value, label).
    Choice(&'static [(&'static str, &'static str)]),
    /// A public key: picked from this computer, or pasted.
    Key,
}

/// A box a recipe asks you to fill in first.
#[derive(Debug, Clone)]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub help: &'static str,
    pub initial: Option<Text>,
}

/// One step of a recipe.
#[derive(Debug, Clone)]
pub struct Step {
    /// Plain text, unique within its recipe.
    pub title: &'static str,
    /// One or two sentences: what it does and why.
    pub explain: Text,
    pub command: Text,
    /// Run on your own computer, not on the server.
    pub local: bool,
    /// Only for this kind of system.
    pub only: Option<Os>,
    /// Said before the command: what could go wrong.
    pub warn: Option<Text>,
    /// Can cut you off from the server; a check that you can still log in follows.
    pub lockout: bool,
    /// Checks that the steps before it worked.
    pub check: bool,
    /// What you should see when it worked.
    pub expect: Option<Text>,
    /// What to do when it didn't: why, and a command.
    pub if_fails: Option<(Text, Text)>,
    /// Fields the command uses; it's hidden until they're filled in.
    pub needs: &'static [&'static str],
    /// Not needed when this computer already has a key.
    pub skip_if_key: bool,
}

/// What a recipe is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Secure,
    Setup,
}

impl Group {
    pub const ALL: [Group; 2] = [Group::Secure, Group::Setup];

    pub fn label(self) -> &'static str {
        match self {
            Group::Secure => "Lock it down",
            Group::Setup => "Set things up",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub summary: &'static str,
    /// Who it's for, in one line.
    pub who: &'static str,
    pub fields: Vec<Field>,
    pub steps: Vec<Step>,
}

impl Recipe {
    /// The steps for this kind of system.
    pub fn steps_for(&self, os: Os) -> Vec<&Step> {
        self.steps.iter().filter(|s| s.only.is_none_or(|o| o == os)).collect()
    }

    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// It picks a key from this computer.
    pub fn wants_key(&self) -> bool {
        self.fields.iter().any(|f| matches!(f.kind, FieldKind::Key))
    }
}

/// How the server is reached, as a fresh `ssh` typed on this computer reaches it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshHost {
    /// Its name in the list; for a server from the ssh config, the name ssh knows it by.
    pub name: String,
    pub hostname: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_file: Option<String>,
    /// From `~/.ssh/config`: ssh reads the rest from there.
    pub from_config: bool,
}

/// A key pair in `~/.ssh` on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalKey {
    /// "~/.ssh/id_ed25519"
    pub path: String,
    pub name: String,
    /// "ED25519", "RSA"…
    pub kind: String,
}

/// What a recipe knows about the server and this computer, from the backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The server's kernel release, to guess its system.
    pub kernel: Option<String>,
    pub host: Option<SshHost>,
    pub keys: Vec<LocalKey>,
    /// Commands for this computer are typed into PowerShell.
    pub windows: bool,
    /// ssh's options while Tor is on (`-o`, value, ...), so a check goes through Tor too.
    pub tor: Option<Vec<String>>,
}

/// An `ssh` typed on this computer.
#[derive(Debug, Clone, Default)]
pub struct SshOptions<'a> {
    /// Log in as this user instead of the usual one.
    pub user: Option<&'a str>,
    /// Keys only, never a password.
    pub key_only: bool,
    /// A key file to offer.
    pub identity: Option<&'a str>,
    /// A command to run instead of a shell.
    pub run: Option<&'a str>,
    /// Give `run` a terminal, so sudo can ask for a password.
    pub tty: bool,
}

/// What a step's text and command may depend on.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub os: Os,
    /// The port SSHeesh connects to.
    pub host_port: u16,
    /// The SSH port to use in commands: the one typed in, or `host_port`.
    pub ssh_port: u16,
    /// The fields that are filled in and usable, by id.
    pub values: BTreeMap<String, String>,
    pub facts: Facts,
}

impl Ctx {
    pub fn new(os: Os, facts: &Facts, values: BTreeMap<String, String>) -> Ctx {
        let host_port = facts.host.as_ref().and_then(|h| h.port).unwrap_or(22);
        let ssh_port = values.get("sshPort").filter(|p| is_port(p)).and_then(|p| p.trim().parse().ok()).unwrap_or(host_port);
        Ctx { os, host_port, ssh_port, values, facts: facts.clone() }
    }

    /// A filled-in field's value, or "".
    pub fn v(&self, id: &str) -> &str {
        self.values.get(id).map_or("", String::as_str)
    }

    /// An ssh command, run on your computer, that logs in to this server afresh.
    pub fn ssh(&self, o: SshOptions) -> String {
        ssh_command(self.facts.host.as_ref(), &o, self.facts.windows, self.facts.tor.as_deref())
    }
}

/// Quotes `s` for a shell where it's typed: POSIX shells or PowerShell. `None` for text with
/// control characters, which can't be typed safely (0.10's `typedQuote`).
pub fn typed_quote(s: &str, powershell: bool) -> Option<String> {
    if s.chars().any(|c| c.is_control()) {
        return None;
    }
    if powershell {
        let mut out = String::from("'");
        for c in s.chars() {
            if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        return Some(out);
    }
    if s.is_empty() {
        return Some("''".into());
    }
    let mut out = String::new();
    let mut run = String::new();
    for c in s.chars() {
        if c == '\'' || c == '\\' {
            if !run.is_empty() {
                out.push_str(&format!("'{run}'"));
                run.clear();
            }
            out.push('\\');
            out.push(c);
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        out.push_str(&format!("'{run}'"));
    }
    Some(out)
}

fn quote_arg(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "_~./:\\-".contains(c)) {
        s.to_owned()
    } else {
        format!("\"{}\"", s.replace('"', ""))
    }
}

/// An ssh command that logs in to `host` from your computer, the way SSHeesh does, but as a
/// fresh login: never through a connection that's already signed in (SSHeesh shares one on
/// macOS and Linux), so it really tests whether logging in works.
pub fn ssh_command(host: Option<&SshHost>, o: &SshOptions, windows: bool, tor: Option<&[String]>) -> String {
    let mut args = vec!["ssh".to_owned()];
    if o.tty {
        args.push("-t".into());
    }
    if !windows {
        args.push("-o ControlPath=none".into());
    }
    // While Tor is on, this check goes through it too, as SSHeesh's own ssh does.
    if let Some(tor) = tor {
        for pair in tor.chunks_exact(2) {
            if let Some(quoted) = typed_quote(&pair[1], windows) {
                args.push(format!("{} {quoted}", pair[0]));
            }
        }
    }
    if o.key_only {
        args.push("-o PreferredAuthentications=publickey".into());
    }
    match host {
        None => args.push(o.user.map_or("localhost".into(), |u| format!("{u}@localhost"))),
        Some(h) if !h.from_config => {
            if let Some(port) = h.port.filter(|p| *p != 22) {
                args.push(format!("-p {port}"));
            }
            if let Some(identity) = o.identity.or(h.identity_file.as_deref()) {
                args.push(format!("-i {}", quote_arg(identity)));
            }
            let user = o.user.or(h.user.as_deref());
            args.push(user.map_or(h.hostname.clone(), |u| format!("{u}@{}", h.hostname)));
        }
        Some(h) => {
            // An entry in ~/.ssh/config: ssh reads the rest from there.
            if let Some(identity) = o.identity {
                args.push(format!("-i {}", quote_arg(identity)));
            }
            if let Some(user) = o.user {
                args.push(format!("-l {user}"));
            }
            args.push(h.name.clone());
        }
    }
    if let Some(run) = o.run {
        args.push(run.to_owned());
    }
    args.join(" ")
}

/// A port number: 1 to 65535, no leading zero.
pub fn is_port(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && !s.starts_with('0') && s.len() <= 5 && s.chars().all(|c| c.is_ascii_digit()) && s.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n))
}

const KEY_TYPES: [&str; 7] = [
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// A public key as one authorized_keys line, or `None` when `text` isn't one. `label` (the
/// key's file name) replaces the key's own comment, so a key read from this computer carries
/// no user or computer name. What's left of the comment never needs quoting.
pub fn parse_public_key(text: &str, label: Option<&str>) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let mut words = line.split_whitespace();
    let (kind, blob) = (words.next()?, words.next()?);
    let comment: Vec<&str> = words.collect();
    let blob_ok = blob.len() >= 40 && {
        let body = blob.trim_end_matches('=');
        blob.len() - body.len() <= 3 && body.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
    };
    if !KEY_TYPES.contains(&kind) || !blob_ok {
        return None;
    }
    let tag: String = label.map_or_else(|| comment.join("-"), str::to_owned).chars().filter(|c| c.is_ascii_alphanumeric() || "._@-".contains(*c)).collect();
    Some(if tag.is_empty() { format!("{kind} {blob}") } else { format!("{kind} {blob} {tag}") })
}

/// Why a field's text isn't usable, or `None` when it is (or is still empty).
pub fn field_error(f: &Field, raw: &str) -> Option<&'static str> {
    let v = raw.trim();
    if v.is_empty() {
        return None;
    }
    match &f.kind {
        FieldKind::Key => parse_public_key(v, None).is_none().then_some("That isn't a public key. One starts with ssh-ed25519, ssh-rsa or ecdsa-sha2-."),
        FieldKind::Choice(choices) => (!choices.iter().any(|(value, _)| *value == v)).then_some("Pick one of the choices."),
        FieldKind::Text { valid: Some((ok, why)), .. } => (!ok(v)).then_some(*why),
        FieldKind::Text { valid: None, .. } => None,
    }
}

/// The fields that are filled in and usable. `typed` holds what was typed, by field id; a
/// field nobody touched takes its initial value. `keyFile`, the picked key's path, comes
/// along with the key.
pub fn field_values(recipe: &Recipe, typed: &BTreeMap<String, String>, base: &Ctx) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for f in &recipe.fields {
        let raw = typed.get(f.id).cloned().or_else(|| f.initial.as_ref().map(|t| t.get(base))).unwrap_or_default();
        let v = raw.trim();
        if v.is_empty() || field_error(f, v).is_some() {
            continue;
        }
        let value = match f.kind {
            FieldKind::Key => parse_public_key(v, None).unwrap_or_else(|| v.to_owned()),
            _ => v.to_owned(),
        };
        out.insert(f.id.to_owned(), value);
    }
    if out.contains_key("publicKey") {
        if let Some(file) = typed.get("keyFile").filter(|f| !f.is_empty()) {
            out.insert("keyFile".into(), file.clone());
        }
    }
    out
}

/// The fields a step still waits for.
pub fn missing<'a>(recipe: &'a Recipe, step: &Step, values: &BTreeMap<String, String>) -> Vec<&'a Field> {
    step.needs.iter().filter(|id| !values.contains_key(**id)).filter_map(|id| recipe.field(id)).collect()
}

/// "Fill in “Domain” and “Port your app listens on” above to see the command."
pub fn fill_in_first(missing: &[&Field]) -> String {
    let names: Vec<String> = missing.iter().map(|f| format!("“{}”", f.label)).collect();
    format!("Fill in {} above to see the command.", names.join(" and "))
}

/// "2 of 6 steps done. Tick a step off once it worked.", or "All done."
pub fn progress(done: usize, of: usize) -> String {
    if done >= of {
        "All done.".into()
    } else {
        format!("{done} of {of} steps done. Tick a step off once it worked.")
    }
}

/// How many of `steps` are ticked off in `done` (step titles).
pub fn done_in(steps: &[&Step], done: &[String]) -> usize {
    steps.iter().filter(|s| done.iter().any(|d| d == s.title)).count()
}

/// What the window asks the backend for. Each answer comes back later through `Remote`,
/// addressed to the server it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Run the security check on a server.
    Check { server: String },
    /// Run a fix you confirmed, without a terminal.
    Fix { server: String, check: String, command: String },
    /// What a recipe needs: the kernel, how the server is reached, this computer's keys.
    Facts { server: String },
    /// Read a public key from this computer (`path` without `.pub`).
    ReadKey { server: String, path: String },
}

/// How a fix that was run went (sshee-core's `RunOutcome`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done { output: String },
    Failed { output: String, code: Option<i32> },
    /// It has to run in a terminal, where you can answer it: what it will want, in plain words.
    NeedsYou { ask: String },
}

impl Outcome {
    /// The line over its output.
    pub fn headline(&self) -> String {
        match self {
            Outcome::Done { .. } => "Done".into(),
            Outcome::Failed { code: Some(code), .. } => format!("It stopped with an error (code {code})"),
            Outcome::Failed { code: None, .. } => "It stopped before the end".into(),
            Outcome::NeedsYou { .. } => "This one needs you in a terminal".into(),
        }
    }
}

/// The backend's answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Checked { server: String, result: Result<checkup::Report, String> },
    Fixed { server: String, check: String, outcome: Outcome },
    Facts { server: String, facts: Facts },
    /// The key line read (`None`: it couldn't be read as a public key).
    Key { server: String, path: String, line: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SshHost {
        SshHost { name: "web-7731".into(), hostname: "203.0.113.17".into(), port: Some(2222), user: Some("deploy".into()), identity_file: None, from_config: false }
    }

    #[test]
    fn the_system_is_guessed_from_the_kernel() {
        assert_eq!(guess_os("Linux 6.8.0-45-generic"), Some(Os::Debian));
        assert_eq!(guess_os("6.1.0-18-amd64"), Some(Os::Debian));
        assert_eq!(guess_os("5.14.0-427.el9.x86_64"), Some(Os::Rhel));
        assert_eq!(guess_os("6.9.4-200.fc40.x86_64"), Some(Os::Rhel));
        assert_eq!(guess_os("6.1.79-99.167.amzn2023.x86_64"), Some(Os::Rhel));
        assert_eq!(guess_os("6.1.0-amzn2023"), None);
        assert_eq!(guess_os("6.6.12"), None);
    }

    #[test]
    fn a_fresh_ssh_reaches_the_server_as_ssheesh_does() {
        let h = host();
        let o = SshOptions { key_only: true, run: Some("echo Key login works"), identity: Some("~/.ssh/id_ed25519"), ..SshOptions::default() };
        assert_eq!(
            ssh_command(Some(&h), &o, false, None),
            "ssh -o ControlPath=none -o PreferredAuthentications=publickey -p 2222 -i ~/.ssh/id_ed25519 deploy@203.0.113.17 echo Key login works"
        );
        // Windows shares no connection; a config entry is named as ssh knows it.
        let cfg = SshHost { from_config: true, ..h.clone() };
        assert_eq!(ssh_command(Some(&cfg), &SshOptions { user: Some("alex"), ..SshOptions::default() }, true, None), "ssh -l alex web-7731");
        // Through Tor, quoted for the shell it's typed into.
        let tor = ["-o".to_owned(), "ProxyCommand=/opt/ssheesh --tor-proxy %h %p".to_owned()];
        assert_eq!(ssh_command(None, &SshOptions::default(), false, Some(&tor)), "ssh -o ControlPath=none -o 'ProxyCommand=/opt/ssheesh --tor-proxy %h %p' localhost");
        assert_eq!(quote_arg("C:\\Users\\me\\key file"), "\"C:\\Users\\me\\key file\"");
    }

    #[test]
    fn quoting_for_a_typed_shell() {
        assert_eq!(typed_quote("it's", false).unwrap(), "'it'\\''s'");
        assert_eq!(typed_quote("", false).unwrap(), "''");
        assert_eq!(typed_quote("it's", true).unwrap(), "'it''s'");
        assert_eq!(typed_quote("a\nb", false), None);
    }

    #[test]
    fn public_keys_are_read_and_their_comment_replaced() {
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIGq1Yw5mN2Rk8xXc0p1wQ7mJ9z2eYb3LmN4oP5qR6sT7";
        assert_eq!(parse_public_key(&format!("ssh-ed25519 {blob} me@laptop-7731\n"), Some("id_ed25519")).unwrap(), format!("ssh-ed25519 {blob} id_ed25519"));
        assert_eq!(parse_public_key(&format!("# mine\nssh-ed25519 {blob} me@laptop 7731"), None).unwrap(), format!("ssh-ed25519 {blob} me@laptop-7731"));
        assert_eq!(parse_public_key(&format!("ssh-ed25519 {blob}"), None).unwrap(), format!("ssh-ed25519 {blob}"));
        assert!(parse_public_key("ssh-dss AAAA", None).is_none());
        assert!(parse_public_key("ssh-ed25519 short", None).is_none());
        assert!(parse_public_key("-----BEGIN OPENSSH PRIVATE KEY-----", None).is_none());
    }

    #[test]
    fn ports() {
        assert!(is_port("22") && is_port("65535") && is_port(" 2222 "));
        assert!(!is_port("0") && !is_port("022") && !is_port("65536") && !is_port("") && !is_port("2x"));
    }

    #[test]
    fn fields_fill_values_and_steps_wait_for_them() {
        let r = recipe("caddy-https").unwrap();
        let facts = Facts { host: Some(host()), ..Facts::default() };
        let base = Ctx::new(Os::Debian, &facts, BTreeMap::new());
        let mut typed = BTreeMap::new();
        typed.insert("domain".into(), "https://shop.example".into());
        let values = field_values(&r, &typed, &base);
        assert!(values.is_empty());
        assert_eq!(field_error(r.field("domain").unwrap(), "https://shop.example"), Some("A domain like example.com or app.example.com, without https:// or a path."));
        typed.insert("domain".into(), "shop.example.com".into());
        typed.insert("appPort".into(), "3000".into());
        let values = field_values(&r, &typed, &base);
        let ctx = Ctx::new(Os::Debian, &facts, values.clone());
        let send = r.steps.iter().find(|s| s.title == "Send your domain to the app").unwrap();
        assert!(missing(&r, send, &values).is_empty());
        assert!(send.command.get(&ctx).contains("'shop.example.com' '3000'"));
        let none = missing(&r, send, &BTreeMap::new());
        assert_eq!(fill_in_first(&none), "Fill in “Domain” and “Port your app listens on” above to see the command.");
        // The SSH port starts as the one SSHeesh connects to.
        let fw = recipe("firewall").unwrap();
        let v = field_values(&fw, &BTreeMap::new(), &base);
        assert_eq!(v.get("sshPort").map(String::as_str), Some("2222"));
        assert_eq!(Ctx::new(Os::Debian, &facts, v).ssh_port, 2222);
    }

    #[test]
    fn progress_reads_plainly() {
        assert_eq!(progress(2, 6), "2 of 6 steps done. Tick a step off once it worked.");
        assert_eq!(progress(6, 6), "All done.");
        assert_eq!(Outcome::Failed { output: String::new(), code: Some(100) }.headline(), "It stopped with an error (code 100)");
    }
}
