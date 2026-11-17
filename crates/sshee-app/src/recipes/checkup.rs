//! The security check's findings and the rules for running a fix (0.10's `CheckupDialog` and
//! `Command`), without a screen.
//!
//! - Findings are shown worst first (to fix, worth a look, couldn't tell, good); within a
//!   verdict, in the order the server's check gave them.
//! - A fix is a command shown in full. Nothing runs until you confirm it, and on a Production
//!   server you confirm twice: the second time says it's Production, by the server's label.
//!   [`Gate`] is that rule.
//! - A fix that needs you (sudo's password, a package manager's "go ahead?") opens in a
//!   terminal instead, typed and run there, with a plain word on what to answer.

/// What a check found (sshee-core's `checkup::Verdict`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    Bad,
    Warn,
    /// Couldn't tell, usually for want of root.
    Unknown,
    Good,
}

impl Verdict {
    /// Worst first.
    fn order(self) -> u8 {
        match self {
            Verdict::Bad => 0,
            Verdict::Warn => 1,
            Verdict::Unknown => 2,
            Verdict::Good => 3,
        }
    }

    /// What a screen reader says before the finding's name.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Bad => "To fix",
            Verdict::Warn => "Worth a look",
            Verdict::Unknown => "Couldn't tell",
            Verdict::Good => "Good",
        }
    }
}

/// One finding (sshee-core's `checkup::Check`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable: "password-login", "root-login", "open-ports", "firewall", ...
    pub id: String,
    pub title: String,
    pub verdict: Verdict,
    /// One or two plain sentences: what was found and why it matters.
    pub detail: String,
    /// A command that fixes it.
    pub fix: Option<String>,
    /// One sentence on what `fix` does, or why there's none.
    pub fix_note: Option<String>,
}

impl Finding {
    /// A fix is offered: there is one and it isn't fine already.
    pub fn offers_fix(&self) -> bool {
        self.fix.is_some() && self.verdict != Verdict::Good
    }

    /// The recipe that walks through fixing it, when it isn't fine.
    pub fn recipe(&self) -> Option<&'static str> {
        (self.verdict != Verdict::Good).then(|| super::recipe_for_check(&self.id)).flatten()
    }
}

/// A check's findings (sshee-core's `checkup::Checkup`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Some checks read files only root can; false when sudo wanted a password, so those may
    /// come back "couldn't tell".
    pub as_root: bool,
}

/// Said when the check couldn't be root.
pub const NEEDS_ROOT: &str = "Some of this needs root. Connect as root, or allow sudo without a password, to see everything.";

/// Said under the title while nothing has run.
pub const PROMISE: &str = "nothing changes until you run a fix";

/// Said while the check runs.
pub const CHECKING: &str = "Checking logins, open ports, the firewall and updates…";

impl Report {
    /// The findings worst first; within a verdict, as the server gave them.
    pub fn sorted(&self) -> Vec<&Finding> {
        let mut out: Vec<&Finding> = self.findings.iter().collect();
        out.sort_by_key(|f| f.verdict.order());
        out
    }

    /// "2 to fix · 1 worth a look · 5 good", each part with its verdict.
    pub fn summary(&self) -> Vec<(Verdict, String)> {
        let count = |v: Verdict| self.findings.iter().filter(|f| f.verdict == v).count();
        [
            (Verdict::Bad, "to fix"),
            (Verdict::Warn, "worth a look"),
            (Verdict::Good, "good"),
            (Verdict::Unknown, "couldn't tell"),
        ]
        .into_iter()
        .filter_map(|(v, words)| {
            let n = count(v);
            (n > 0).then(|| (v, format!("{n} {words}")))
        })
        .collect()
    }
}

/// Where running a fix stands, between the button and the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// "Run this on web-7731?" with the command in full.
    Confirm,
    /// "web-7731 is a Production server": asked after the first yes, on Production only.
    Production,
}

/// What a yes leads to: the next question, or `None` to run it now.
pub fn after_yes(gate: Gate, production: bool) -> Option<Gate> {
    match gate {
        Gate::Confirm if production => Some(Gate::Production),
        _ => None,
    }
}

/// The first question's title.
pub fn confirm_title(server: &str) -> String {
    format!("Run this on {server}?")
}

/// The second question's title and its line, on Production.
pub fn production_title(server: &str) -> String {
    format!("{server} is a Production server")
}

pub const PRODUCTION_LINE: &str = "This changes a server you labelled Production. Run it only if you meant to change it now.";

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: &str, verdict: Verdict, fix: Option<&str>) -> Finding {
        Finding { id: id.into(), title: id.into(), verdict, detail: String::new(), fix: fix.map(str::to_owned), fix_note: None }
    }

    #[test]
    fn findings_come_worst_first_and_are_summed_up() {
        let r = Report {
            findings: vec![
                finding("reboot", Verdict::Good, None),
                finding("firewall", Verdict::Bad, Some("sudo ufw enable")),
                finding("open-ports", Verdict::Warn, None),
                finding("brute-force", Verdict::Unknown, None),
                finding("password-login", Verdict::Bad, Some("printf …")),
            ],
            as_root: false,
        };
        let ids: Vec<&str> = r.sorted().iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["firewall", "password-login", "open-ports", "brute-force", "reboot"]);
        let words: Vec<String> = r.summary().into_iter().map(|(_, w)| w).collect();
        assert_eq!(words, ["2 to fix", "1 worth a look", "1 good", "1 couldn't tell"]);
        assert!(r.findings[1].offers_fix() && !r.findings[0].offers_fix());
        assert_eq!(r.findings[1].recipe(), Some("firewall"));
        assert_eq!(finding("firewall", Verdict::Good, None).recipe(), None);
        assert_eq!(r.findings[2].recipe(), None);
    }

    #[test]
    fn production_is_never_fixed_on_one_yes() {
        assert_eq!(after_yes(Gate::Confirm, false), None);
        assert_eq!(after_yes(Gate::Confirm, true), Some(Gate::Production));
        assert_eq!(after_yes(Gate::Production, true), None);
        assert_eq!(confirm_title("web-7731"), "Run this on web-7731?");
        assert_eq!(production_title("web-7731"), "web-7731 is a Production server");
    }
}
