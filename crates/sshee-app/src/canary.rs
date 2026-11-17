//! Test builds (the canary) updating themselves: what the window says about it. Which build
//! this is ([`Build`], shown in Settings → About) and the one quiet line across the top of the
//! page when a newer test build is ready ([`Notice`]): its words and its buttons.
//!
//! The work itself (asking GitHub, checking the download's hash and signature, putting the new
//! program in place and going back if it doesn't start) is the program's:
//! `desktop/app/src/update.rs`. Nothing here reaches 0.10: its updates are `v*` releases and
//! `latest.json`; test builds have one pre-release of their own, tagged `canary`.

/// Which build this is: set by the program when it starts (`desktop/app/build.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Build {
    /// The package's version: "1.0.0-canary".
    pub version: String,
    /// The commit it was built from, short ("4edfea3"); empty when unknown.
    pub sha: String,
    /// The canary workflow's run that built it. `None`: built on this computer, and it never
    /// updates itself.
    pub run: Option<u64>,
}

impl Build {
    /// The version a canary manifest names this build by: "1.0.0-canary.124", or the package's
    /// own for a build made here.
    pub fn full_version(&self) -> String {
        match self.run {
            Some(run) => format!("{}.{run}", self.version),
            None => self.version.clone(),
        }
    }

    /// What Settings shows: "1.0.0-canary.124 (4edfea3)", or "1.0.0-canary (4edfea3, built
    /// here)" for a build that didn't come from the canary workflow.
    pub fn label(&self) -> String {
        let version = self.full_version();
        match (self.run, self.sha.is_empty()) {
            (Some(_), false) => format!("{version} ({})", self.sha),
            (Some(_), true) => version,
            (None, false) => format!("{version} ({}, built here)", self.sha),
            (None, true) => format!("{version} (built here)"),
        }
    }

    /// Whether this build looks for newer test builds: only one the canary workflow made.
    pub fn updates_itself(&self) -> bool {
        self.run.is_some()
    }
}

/// Why a newer test build can only be downloaded by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ByHand {
    /// The build wasn't signed (the repository has no canary key yet).
    Unsigned,
    /// This copy has no key to check a signature with (it was built before there was one).
    NoKey,
    /// The folder this copy runs from can't be written.
    Folder,
    /// The download didn't match what was published, or its signature didn't check out.
    Mismatch,
    /// Putting it in place failed: the reason, one plain line.
    Failed(String),
}

impl ByHand {
    fn why(&self) -> String {
        match self {
            ByHand::Unsigned => "It isn't signed, so it isn't installed by itself.".into(),
            ByHand::NoKey => "This copy can't check its signature, so it isn't installed by itself.".into(),
            ByHand::Folder => "The folder this copy runs from can't be written.".into(),
            ByHand::Mismatch => "The download didn't match what was published, so it wasn't installed.".into(),
            ByHand::Failed(why) => format!("It didn't install: {why}"),
        }
    }
}

/// The page where test builds are downloaded by hand.
pub const RELEASE_PAGE: &str = "https://github.com/footprintsntrails/ssheesh/releases/tag/canary";

/// What the line across the top of the page says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Downloaded and checked, next to this copy: a restart puts it in place.
    Ready { version: String },
    /// Newer, but only to download from the release page.
    ByHand { version: String, why: ByHand },
    /// The last update didn't start, so this is the build from before it.
    RolledBack { version: String },
}

/// A button on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Puts the new build in place and starts it.
    Restart,
    /// Opens [`RELEASE_PAGE`].
    Download,
    /// Hides the line until a newer build.
    Later,
    /// Hides the line.
    Ok,
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::Restart => "Restart to update",
            Button::Download => "Download",
            Button::Later => "Later",
            Button::Ok => "OK",
        }
    }

    /// A fixed word for the element's key.
    pub fn key(self) -> &'static str {
        match self {
            Button::Restart => "canary-restart",
            Button::Download => "canary-download",
            Button::Later => "canary-later",
            Button::Ok => "canary-ok",
        }
    }
}

impl Notice {
    /// The version it is about.
    pub fn version(&self) -> &str {
        match self {
            Notice::Ready { version } | Notice::ByHand { version, .. } | Notice::RolledBack { version } => version,
        }
    }

    /// One sentence.
    pub fn text(&self) -> String {
        match self {
            Notice::Ready { version } => format!("A new test build is ready ({version})."),
            Notice::ByHand { version, why } => format!("A new test build is out ({version}). {}", why.why()),
            Notice::RolledBack { version } => format!("The new test build ({version}) didn't start, so you're back on this one."),
        }
    }

    /// Its buttons, the one that does it first.
    pub fn buttons(&self) -> &'static [Button] {
        match self {
            Notice::Ready { .. } => &[Button::Restart, Button::Later],
            Notice::ByHand { .. } => &[Button::Download, Button::Later],
            Notice::RolledBack { .. } => &[Button::Ok],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(run: Option<u64>, sha: &str) -> Build {
        Build { version: "1.0.0-canary".into(), sha: sha.into(), run }
    }

    #[test]
    fn a_build_says_its_run_and_commit() {
        assert_eq!(build(Some(124), "4edfea3").label(), "1.0.0-canary.124 (4edfea3)");
        assert_eq!(build(Some(124), "4edfea3").full_version(), "1.0.0-canary.124");
        assert_eq!(build(None, "4edfea3").label(), "1.0.0-canary (4edfea3, built here)");
        assert_eq!(build(None, "").label(), "1.0.0-canary (built here)");
        assert!(build(Some(1), "").updates_itself() && !build(None, "4edfea3").updates_itself());
    }

    #[test]
    fn each_notice_has_its_words_and_buttons() {
        let ready = Notice::Ready { version: "1.0.0-canary.125".into() };
        assert_eq!(ready.text(), "A new test build is ready (1.0.0-canary.125).");
        assert_eq!(ready.buttons().iter().map(|b| b.label()).collect::<Vec<_>>(), ["Restart to update", "Later"]);
        let unsigned = Notice::ByHand { version: "1.0.0-canary.125".into(), why: ByHand::Unsigned };
        assert!(unsigned.text().contains("isn't signed"));
        assert_eq!(unsigned.buttons(), [Button::Download, Button::Later]);
        let back = Notice::RolledBack { version: "1.0.0-canary.125".into() };
        assert_eq!(back.buttons(), [Button::Ok]);
        assert_eq!(back.version(), "1.0.0-canary.125");
    }
}
