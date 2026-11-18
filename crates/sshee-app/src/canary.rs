//! 1.x updating itself: what the window says about it. Which build this is ([`Build`], shown in
//! Settings → About), where it looks for a newer one ([`Channel`]), and the one quiet line
//! across the top of the page when one is out ([`Notice`]): its words and its buttons.
//!
//! Two kinds of build look: test builds (the canary workflow's, one pre-release tagged
//! `canary`) and released 1.x builds (`update-v1.json` in the `channel-stable` or `channel-rc`
//! release, plan §6.7.1). The work itself (asking GitHub, checking the manifest's and the
//! download's signatures, putting the new program in place and going back if it doesn't
//! start) is the program's: `desktop/app/src/update.rs`. Nothing here reaches 0.10: it reads
//! `latest.json` from GitHub's "latest" release and nothing else.

use crate::model::UpdateMode;

/// Which build this is: set by the program when it starts (`desktop/app/build.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Build {
    /// The package's version: "1.0.0-canary", "1.0.1".
    pub version: String,
    /// The commit it was built from, short ("4edfea3"); empty when unknown.
    pub sha: String,
    /// The canary workflow's run that built it. `None`: not a test build.
    pub run: Option<u64>,
    /// Built by the release workflow (`SSHEESH_RELEASE`): it looks for newer releases. A build
    /// that is neither this nor a test build was made on someone's computer and never looks.
    pub release: bool,
}

impl Build {
    /// The version a manifest names this build by: "1.0.0-canary.124" for a test build, the
    /// package's own otherwise.
    pub fn full_version(&self) -> String {
        match self.run {
            Some(run) => format!("{}.{run}", self.version),
            None => self.version.clone(),
        }
    }

    /// What Settings shows: "1.0.0-canary.124 (4edfea3)", "1.0.1 (4edfea3)" for a release, or
    /// "1.0.0-canary (4edfea3, built here)" for a build made on someone's computer.
    pub fn label(&self) -> String {
        let version = self.full_version();
        match (self.updates_itself(), self.sha.is_empty()) {
            (true, false) => format!("{version} ({})", self.sha),
            (true, true) => version,
            (false, false) => format!("{version} ({}, built here)", self.sha),
            (false, true) => format!("{version} (built here)"),
        }
    }

    /// Whether this build looks for newer ones: a test build or a release, never one built here.
    pub fn updates_itself(&self) -> bool {
        self.run.is_some() || self.release
    }

    /// Whether this is a test build (the canary workflow's).
    pub fn is_test(&self) -> bool {
        self.run.is_some()
    }
}

/// Where a build looks for a newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Test builds: `canary.json` in the `canary` pre-release, signed with the canary key.
    Canary,
    /// Releases: `update-v1.json` in the `channel-stable` release, signed with the release key.
    Stable,
    /// Release candidates: `update-v1.json` in the `channel-rc` release, the same key.
    Rc,
}

/// What a look does with a newer build it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Downloads, checks and stages it, then offers "Restart to update" ("Install updates:
    /// Automatically").
    Install,
    /// Only says it's out, with a link to its page ("Install updates: Ask me", or a mode this
    /// version doesn't know).
    Tell,
}

impl Channel {
    /// Where `build` looks, and what it does with what it finds, under Settings → Updates:
    /// "Install updates" (`mode`) and "Check for test builds" (`test_builds`).
    ///
    /// - "Never check": nowhere (as 0.10's "manual", which never looks by itself).
    /// - A test build looks for test builds while "Check for test builds" is on; turned off,
    ///   it looks for releases instead, so it moves on to 1.0.0 once that's out.
    /// - A release looks for releases; one whose version is a pre-release ("1.0.0-rc.2") looks
    ///   among release candidates. "Check for test builds" never moves a release to the
    ///   canary: it's on by default, and a release mustn't install a test build by itself.
    /// - A build made on someone's computer never looks.
    pub fn choose(build: &Build, mode: &UpdateMode, test_builds: bool) -> Option<(Channel, Act)> {
        let act = match mode {
            UpdateMode::Manual => return None,
            UpdateMode::Auto => Act::Install,
            UpdateMode::Notify | UpdateMode::Other(_) => Act::Tell,
        };
        let channel = if build.is_test() {
            if test_builds {
                Channel::Canary
            } else {
                Channel::Stable
            }
        } else if build.release {
            if build.version.contains('-') {
                Channel::Rc
            } else {
                Channel::Stable
            }
        } else {
            return None;
        };
        Some((channel, act))
    }
}

/// Why a newer build can only be downloaded by hand.
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

/// Where every release is listed.
pub const RELEASES_PAGE: &str = "https://github.com/footprintsntrails/ssheesh/releases";

/// Whether `version` names a test build: "1.0.0-canary.125".
pub fn is_test_version(version: &str) -> bool {
    version.contains("-canary.")
}

/// The page `version` is downloaded from by hand: the canary pre-release for a test build, the
/// release's own page (tag `v<version>`) otherwise, and the list of releases for anything that
/// isn't plainly a version.
pub fn page_of(version: &str) -> String {
    if is_test_version(version) {
        RELEASE_PAGE.to_owned()
    } else if !version.is_empty() && version.len() <= 64 && version.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b)) && !version.contains("..") {
        format!("{RELEASES_PAGE}/tag/v{version}")
    } else {
        RELEASES_PAGE.to_owned()
    }
}

/// What the line across the top of the page says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Downloaded and checked, next to this copy: a restart puts it in place.
    Ready { version: String },
    /// This version was withdrawn (the manifest lists it as pulled), and `version`, older but
    /// good, is downloaded and checked to take its place.
    Replaces { version: String },
    /// Newer, and "Install updates" is "Ask me": only said, with a link to its page.
    Out { version: String },
    /// Newer, but only to download from its page.
    ByHand { version: String, why: ByHand },
    /// The last update didn't start, so this is the build from before it.
    RolledBack { version: String },
}

/// A button on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Puts the new build in place and starts it.
    Restart,
    /// Opens the version's page ([`Notice::page`]).
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
            Notice::Ready { version } | Notice::Replaces { version } | Notice::Out { version } | Notice::ByHand { version, .. } | Notice::RolledBack { version } => version,
        }
    }

    /// Whether it's about a test build (so "Check for test builds" off hides it).
    pub fn is_test(&self) -> bool {
        is_test_version(self.version())
    }

    /// Where "Download" goes.
    pub fn page(&self) -> String {
        page_of(self.version())
    }

    /// One sentence.
    pub fn text(&self) -> String {
        if self.is_test() {
            return match self {
                Notice::Ready { version } => format!("A new test build is ready ({version})."),
                Notice::Replaces { version } => format!("This test build was withdrawn. The one before it ({version}) is ready to replace it."),
                Notice::Out { version } => format!("A new test build is out ({version})."),
                Notice::ByHand { version, why } => format!("A new test build is out ({version}). {}", why.why()),
                Notice::RolledBack { version } => format!("The new test build ({version}) didn't start, so you're back on this one."),
            };
        }
        match self {
            Notice::Ready { version } => format!("SSHeesh {version} is ready."),
            Notice::Replaces { version } => format!("This version of SSHeesh was withdrawn. SSHeesh {version} is ready to replace it."),
            Notice::Out { version } => format!("SSHeesh {version} is out."),
            Notice::ByHand { version, why } => format!("SSHeesh {version} is out. {}", why.why()),
            Notice::RolledBack { version } => format!("SSHeesh {version} didn't start, so you're back on this version."),
        }
    }

    /// Its buttons, the one that does it first.
    pub fn buttons(&self) -> &'static [Button] {
        match self {
            Notice::Ready { .. } | Notice::Replaces { .. } => &[Button::Restart, Button::Later],
            Notice::Out { .. } | Notice::ByHand { .. } => &[Button::Download, Button::Later],
            Notice::RolledBack { .. } => &[Button::Ok],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(run: Option<u64>, sha: &str) -> Build {
        Build { version: "1.0.0-canary".into(), sha: sha.into(), run, release: false }
    }

    fn release(version: &str) -> Build {
        Build { version: version.into(), sha: "4edfea3".into(), run: None, release: true }
    }

    #[test]
    fn a_build_says_its_run_and_commit() {
        assert_eq!(build(Some(124), "4edfea3").label(), "1.0.0-canary.124 (4edfea3)");
        assert_eq!(build(Some(124), "4edfea3").full_version(), "1.0.0-canary.124");
        assert_eq!(build(None, "4edfea3").label(), "1.0.0-canary (4edfea3, built here)");
        assert_eq!(build(None, "").label(), "1.0.0-canary (built here)");
        assert_eq!(release("1.0.1").label(), "1.0.1 (4edfea3)");
        assert_eq!(release("1.0.1").full_version(), "1.0.1");
        assert!(build(Some(1), "").updates_itself() && !build(None, "4edfea3").updates_itself());
        assert!(release("1.0.1").updates_itself() && !release("1.0.1").is_test());
    }

    #[test]
    fn the_channel_follows_the_build_and_settings() {
        let (auto, ask, never) = (UpdateMode::Auto, UpdateMode::Notify, UpdateMode::Manual);
        let test = build(Some(124), "4edfea3");
        // A test build stays on test builds, unless "Check for test builds" is off.
        assert_eq!(Channel::choose(&test, &auto, true), Some((Channel::Canary, Act::Install)));
        assert_eq!(Channel::choose(&test, &auto, false), Some((Channel::Stable, Act::Install)));
        // A release never moves to test builds by itself; a release candidate has its own.
        assert_eq!(Channel::choose(&release("1.0.1"), &auto, true), Some((Channel::Stable, Act::Install)));
        assert_eq!(Channel::choose(&release("1.0.1"), &auto, false), Some((Channel::Stable, Act::Install)));
        assert_eq!(Channel::choose(&release("1.0.0-rc.2"), &auto, true), Some((Channel::Rc, Act::Install)));
        // "Ask me" only says; so does a mode this version doesn't know; "Never check" never looks.
        assert_eq!(Channel::choose(&release("1.0.1"), &ask, true), Some((Channel::Stable, Act::Tell)));
        assert_eq!(Channel::choose(&test, &ask, true), Some((Channel::Canary, Act::Tell)));
        assert_eq!(Channel::choose(&release("1.0.1"), &UpdateMode::Other("later".into()), true), Some((Channel::Stable, Act::Tell)));
        assert_eq!(Channel::choose(&release("1.0.1"), &never, true), None);
        assert_eq!(Channel::choose(&test, &never, false), None);
        // Built here: never.
        assert_eq!(Channel::choose(&build(None, "4edfea3"), &auto, true), None);
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
        assert!(back.is_test() && back.page() == RELEASE_PAGE);
    }

    #[test]
    fn a_release_is_named_and_linked_by_its_version() {
        let ready = Notice::Ready { version: "1.0.1".into() };
        assert_eq!(ready.text(), "SSHeesh 1.0.1 is ready.");
        assert!(!ready.is_test());
        let out = Notice::Out { version: "1.0.1".into() };
        assert_eq!(out.text(), "SSHeesh 1.0.1 is out.");
        assert_eq!(out.buttons(), [Button::Download, Button::Later]);
        assert_eq!(out.page(), "https://github.com/footprintsntrails/ssheesh/releases/tag/v1.0.1");
        let replaces = Notice::Replaces { version: "1.0.0".into() };
        assert!(replaces.text().contains("withdrawn") && replaces.buttons()[0] == Button::Restart);
        // Anything that isn't plainly a version links to the list, not to a made-up address.
        assert_eq!(page_of("1.0.1/../x"), RELEASES_PAGE);
        assert_eq!(page_of("1.0..1"), RELEASES_PAGE);
        assert_eq!(page_of(""), RELEASES_PAGE);
    }
}
