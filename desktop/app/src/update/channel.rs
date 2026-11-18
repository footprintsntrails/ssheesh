//! Where each channel's manifest is, and the key that signs it (plan §6.7.1).
//!
//! - **Canary** (test builds): `canary.json` in the `canary` pre-release; the archives are
//!   signed with the canary key, the manifest itself isn't.
//! - **Stable** (releases): `update-v1.json` and its signature `update-v1.json.minisig` in the
//!   `channel-stable` release, a release kept only for this, at an address that doesn't follow
//!   GitHub's "latest". So the stop switch for 0.10 (`latest.json` in the "latest" release)
//!   never reaches 1.x, and a 1.x installed by hand before 1.0.0 became "latest" finds its
//!   manifest from the first day.
//! - **Rc** (release candidates): the same in the `channel-rc` release.
//!
//! Releases are signed with the release key, the one 0.10 already trusts. Only its public
//! half is here, as `src-tauri/tauri.conf.json` keeps it (`plugins.updater.pubkey`); a test
//! below keeps the two the same.

use sshee_app::canary::Channel;

/// Kept in `update.rs`, where `.github/workflows/canary.yml` says to put it.
pub use super::CANARY_PUBLIC_KEY;

/// The release key's public half (minisign key B239B8BC1DA9D744), as 0.10's
/// `src-tauri/tauri.conf.json` has it: public, already in every 0.10 there is.
pub const RELEASE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIyMzlCOEJDMURBOUQ3NDQKUldSRTE2a2R2TGc1c3Z6V3NjSDk1QlVTM01qZ2tRdyswMStJUlVteE9FRzBtSjloQVlQWUZ0WUwK";

/// The name `update-v1.json` is signed under (its signature's `file:`).
pub const RELEASE_MANIFEST: &str = "update-v1.json";

/// Where a channel's manifest is and who signs what's in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub manifest: &'static str,
    /// The manifest's own signature; `None`: the manifest isn't signed (the canary's: only its
    /// archives are).
    pub signature: Option<&'static str>,
    /// The key the channel's signatures are checked with.
    pub key: &'static str,
}

impl Source {
    pub fn of(channel: Channel) -> Source {
        match channel {
            // Never `releases/latest`: that's 0.10's.
            Channel::Canary => Source { manifest: "https://github.com/footprintsntrails/ssheesh/releases/download/canary/canary.json", signature: None, key: CANARY_PUBLIC_KEY },
            Channel::Stable => Source {
                manifest: "https://github.com/footprintsntrails/ssheesh/releases/download/channel-stable/update-v1.json",
                signature: Some("https://github.com/footprintsntrails/ssheesh/releases/download/channel-stable/update-v1.json.minisig"),
                key: RELEASE_PUBLIC_KEY,
            },
            Channel::Rc => Source {
                manifest: "https://github.com/footprintsntrails/ssheesh/releases/download/channel-rc/update-v1.json",
                signature: Some("https://github.com/footprintsntrails/ssheesh/releases/download/channel-rc/update-v1.json.minisig"),
                key: RELEASE_PUBLIC_KEY,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::manifest::is_release_file;
    use crate::update::verify;

    #[test]
    fn the_release_key_is_the_one_0_10_trusts() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../../../src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(conf["plugins"]["updater"]["pubkey"].as_str(), Some(RELEASE_PUBLIC_KEY));
        assert!(verify::public_key(RELEASE_PUBLIC_KEY).is_some());
    }

    #[test]
    fn the_placeholder_canary_key_never_installs() {
        // Until the owner sets the canary key, a look only offers the download page.
        if CANARY_PUBLIC_KEY.is_empty() {
            assert!(verify::public_key(CANARY_PUBLIC_KEY).is_none());
        } else {
            assert!(verify::public_key(CANARY_PUBLIC_KEY).is_some(), "CANARY_PUBLIC_KEY isn't a minisign public key");
            assert_ne!(CANARY_PUBLIC_KEY, RELEASE_PUBLIC_KEY, "test builds must have a key of their own");
        }
    }

    #[test]
    fn each_channel_reads_this_repositorys_releases_and_never_latest() {
        for channel in [Channel::Canary, Channel::Stable, Channel::Rc] {
            let source = Source::of(channel);
            for url in std::iter::once(source.manifest).chain(source.signature) {
                assert!(is_release_file(url), "{url}");
                assert!(!url.contains("/latest/"), "{url}");
            }
        }
        // Releases sign their manifest with the release key; test builds don't use it.
        assert_eq!(Source::of(Channel::Stable).key, RELEASE_PUBLIC_KEY);
        assert_eq!(Source::of(Channel::Rc).key, RELEASE_PUBLIC_KEY);
        assert_eq!(Source::of(Channel::Canary).key, CANARY_PUBLIC_KEY);
        assert!(Source::of(Channel::Stable).manifest.ends_with("/channel-stable/update-v1.json"));
        assert!(Source::of(Channel::Rc).manifest.ends_with("/channel-rc/update-v1.json"));
        assert_eq!(Source::of(Channel::Canary).signature, None);
    }
}
