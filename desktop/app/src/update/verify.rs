//! Whether a download is the one that was published and signed: its size, its SHA-256, and its
//! minisign signature by the channel's key, made for the name (and, for releases, the version)
//! its signed trusted comment carries.
//!
//! Two keys, never mixed (`channel.rs`): test builds are signed with the canary key of their
//! own, so a test build's signature can't make 0.10 or a release install anything; releases
//! (`update-v1.json` and what it names) with the release key 0.10 already trusts, whose public
//! half is in `src-tauri/tauri.conf.json`.
//!
//! A signature is read as `tauri signer` writes it (base64 of minisign's lines, the `.sig`
//! file) or as minisign itself does (the lines as they are, a `.minisig` file).

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use super::manifest::Asset;

/// Why an archive isn't installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not as many bytes as published.
    Size { want: u64, got: u64 },
    /// Not the bytes published.
    Hash,
    /// This copy has no key for the channel (the canary placeholder), so nothing can be checked.
    NoKey,
    /// Published without a signature.
    Unsigned,
    /// The signature doesn't read, isn't by the channel's key, or isn't of these bytes.
    Signature(String),
    /// Signed for another build, another version or another system: what was looked for.
    Name { want: String },
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Size { want, got } => write!(f, "{got} bytes came where {want} were published"),
            Refused::Hash => f.write_str("the download's SHA-256 isn't the one published"),
            Refused::NoKey => f.write_str("this copy has no key to check signatures with"),
            Refused::Unsigned => f.write_str("the build was published without a signature"),
            Refused::Signature(why) => write!(f, "the signature doesn't check out: {why}"),
            Refused::Name { want } => write!(f, "the signature isn't for {want}"),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A key or a signature as Tauri keeps them: base64 of minisign's lines.
fn unwrap_box(b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    String::from_utf8(bytes).ok()
}

/// A public key as `tauri signer generate` prints it (and as `tauri.conf.json` keeps it).
/// `None` for the placeholder or anything that isn't a minisign key.
pub fn public_key(b64: &str) -> Option<PublicKey> {
    PublicKey::decode(&unwrap_box(b64)?).ok()
}

/// A signature's lines, from a `.sig` (base64) or a `.minisig` (the lines themselves).
fn signature_text(text: &str) -> Option<String> {
    let text = text.trim();
    if text.starts_with("untrusted comment:") {
        Some(format!("{text}\n"))
    } else {
        unwrap_box(text)
    }
}

/// Checks `signature` over `bytes`: by `key`, and with every one of `parts` among the fields
/// of its signed trusted comment ("timestamp:…\tfile:<name>\tversion:<v>").
pub fn signed(bytes: &[u8], signature: &str, key: &str, parts: &[String]) -> Result<(), Refused> {
    let key = public_key(key).ok_or(Refused::NoKey)?;
    let signature = signature_text(signature).ok_or_else(|| Refused::Signature("it isn't a signature".into()))?;
    let signature = Signature::decode(&signature).map_err(|e| Refused::Signature(e.to_string()))?;
    // Minisign's own (pre-hashed) signatures only, as `tauri signer` and minisign make.
    key.verify(bytes, &signature, false).map_err(|e| Refused::Signature(e.to_string()))?;
    // The trusted comment is signed too, with the signature (the line above checked it).
    let fields: Vec<&str> = signature.trusted_comment().split('\t').collect();
    if let Some(missing) = parts.iter().find(|want| !fields.contains(&want.as_str())) {
        let want = missing.strip_prefix("file:").map(str::to_owned).unwrap_or_else(|| missing.replacen(':', " ", 1));
        return Err(Refused::Name { want });
    }
    Ok(())
}

/// Checks `bytes` against what was published for this system (`asset`): its size, its hash,
/// then its signature by `key`, with each of `parts` in its trusted comment.
pub fn check_with(bytes: &[u8], asset: &Asset, key: &str, parts: &[String]) -> Result<(), Refused> {
    let got = bytes.len() as u64;
    if got != asset.size {
        return Err(Refused::Size { want: asset.size, got });
    }
    if sha256_hex(bytes) != asset.sha256 {
        return Err(Refused::Hash);
    }
    if public_key(key).is_none() {
        return Err(Refused::NoKey);
    }
    let signature = asset.signature.as_deref().ok_or(Refused::Unsigned)?;
    signed(bytes, signature, key, parts)
}

/// A test build's archive: signed by `key` for `signed_name` (`Platform::signed_name`, which
/// carries the version). The looks build the same check through `update::found`.
#[cfg(test)]
pub fn check(bytes: &[u8], asset: &Asset, signed_name: &str, key: &str) -> Result<(), Refused> {
    check_with(bytes, asset, key, &[format!("file:{signed_name}")])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use blake2::Blake2b512;
    use ed25519_dalek::{Signer, SigningKey};

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// A minisign key made here, and signatures made with it as minisign makes them.
    pub(crate) struct TestKey {
        key: SigningKey,
        id: [u8; 8],
    }

    impl TestKey {
        pub(crate) fn new(seed: u8) -> TestKey {
            // A key of its own for every test that asks: from the seed and this moment.
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            let mut secret = [seed; 32];
            secret[..4].copy_from_slice(&nanos.to_le_bytes());
            let mut id = [0u8; 8];
            id.copy_from_slice(&Blake2b512::digest(secret)[..8]);
            TestKey { key: SigningKey::from_bytes(&secret), id }
        }

        /// The public key as `tauri signer generate` prints it.
        pub(crate) fn public(&self) -> String {
            let mut raw = b"Ed".to_vec();
            raw.extend_from_slice(&self.id);
            raw.extend_from_slice(self.key.verifying_key().as_bytes());
            b64(format!("untrusted comment: minisign public key: test\n{}\n", b64(&raw)).as_bytes())
        }

        /// A `.sig` file's text for `data` signed under the name `file`.
        pub(crate) fn sign(&self, data: &[u8], file: &str) -> String {
            b64(self.minisig(data, &format!("timestamp:1794688787\tfile:{file}")).as_bytes())
        }

        /// A `.sig` file's text for `data` with this trusted comment.
        pub(crate) fn sign_comment(&self, data: &[u8], trusted: &str) -> String {
            b64(self.minisig(data, trusted).as_bytes())
        }

        /// A `.minisig` file's lines for `data` with this trusted comment, as minisign writes it.
        pub(crate) fn minisig(&self, data: &[u8], trusted: &str) -> String {
            let signature = self.key.sign(&Blake2b512::digest(data));
            let mut line = b"ED".to_vec();
            line.extend_from_slice(&self.id);
            line.extend_from_slice(&signature.to_bytes());
            let mut global = signature.to_bytes().to_vec();
            global.extend_from_slice(trusted.as_bytes());
            let global = self.key.sign(&global);
            format!("untrusted comment: signature from tauri secret key\n{}\ntrusted comment: {trusted}\n{}\n", b64(&line), b64(&global.to_bytes()))
        }
    }

    pub(crate) fn asset_of(data: &[u8], signature: Option<String>) -> Asset {
        Asset { url: "https://github.com/footprintsntrails/ssheesh/releases/download/canary/a.tar.gz".into(), size: data.len() as u64, sha256: sha256_hex(data), signature }
    }

    const NAME: &str = "ssheesh-1.0.0-canary.42-linux-x64.tar.gz";

    #[test]
    fn a_signature_by_the_canary_key_passes() {
        let key = TestKey::new(7);
        let data = b"made-up archive bytes";
        assert_eq!(check(data, &asset_of(data, Some(key.sign(data, NAME))), NAME, &key.public()), Ok(()));
    }

    #[test]
    fn a_signature_tauri_signer_made_passes() {
        // Made once with `tauri signer generate` (a throwaway key, password "testpass") and
        // `tauri signer sign` on a file of these bytes named NAME, as the workflow does.
        const PUBLIC: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ5NkRDRjQ5REIyQkRFMTUKUldRVjNpdmJTYzl0U1NHRFRXTzc5SE5sM2loSUFlN0xBMXJ6YjdNcW9oUFhlY0JRbkI4cmltWloK";
        const SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRVjNpdmJTYzl0U2VLVXNRY3MrN0RlU011dUJUVWVVUC9COHpJeWpaY0FvOXFrKzMxd1ozY3Izd3pnaVJ2M2tkR1NpUERCdll5WW4wbURSM1Fmd3Yzam85dUFhWTZSWFE0PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0Njg4Nzg3CWZpbGU6c3NoZWVzaC0xLjAuMC1jYW5hcnkuNDItbGludXgteDY0LnRhci5negovTjZGV1NJTEFkV3NkRzBnQ29hS1Z4WE9uRlhOMHBNY2xOVlJlVExVaVJmZVR5OUlqMVdrMFRlc1BtaDdxK1NLQ21DdER4NS93UVBRamY5ZnA2NG5DUT09Cg==";
        let data = b"made-up archive bytes for a test\n";
        assert_eq!(check(data, &asset_of(data, Some(SIG.into())), NAME, PUBLIC), Ok(()));
        // The same signature under the next build's name doesn't.
        assert!(matches!(check(data, &asset_of(data, Some(SIG.into())), "ssheesh-1.0.0-canary.43-linux-x64.tar.gz", PUBLIC), Err(Refused::Name { .. })));
    }

    #[test]
    fn a_hash_or_size_that_differs_is_refused_before_the_signature() {
        let key = TestKey::new(8);
        let data = b"made-up archive bytes";
        let mut asset = asset_of(data, Some(key.sign(data, NAME)));
        asset.sha256 = sha256_hex(b"other bytes");
        assert_eq!(check(data, &asset, NAME, &key.public()), Err(Refused::Hash));
        let longer = b"made-up archive bytes!";
        assert_eq!(check(longer, &asset_of(data, Some(key.sign(data, NAME))), NAME, &key.public()), Err(Refused::Size { want: 21, got: 22 }));
    }

    #[test]
    fn what_the_canary_key_didnt_sign_is_refused() {
        let (ours, theirs) = (TestKey::new(1), TestKey::new(2));
        let data = b"made-up archive bytes";
        // Another key's signature over the right bytes.
        assert!(matches!(check(data, &asset_of(data, Some(theirs.sign(data, NAME))), NAME, &ours.public()), Err(Refused::Signature(_))));
        // The right key over other bytes, published with those bytes' hash.
        let other = b"made-up archive bytez";
        assert!(matches!(check(other, &asset_of(other, Some(ours.sign(data, NAME))), NAME, &ours.public()), Err(Refused::Signature(_))));
        // Signed for another system's archive.
        let mac = "ssheesh-1.0.0-canary.42-macos-arm64.tar.gz";
        assert_eq!(check(data, &asset_of(data, Some(ours.sign(data, mac))), NAME, &ours.public()), Err(Refused::Name { want: NAME.into() }));
        // Nothing signed, garbage, and the placeholder key.
        assert_eq!(check(data, &asset_of(data, None), NAME, &ours.public()), Err(Refused::Unsigned));
        assert!(matches!(check(data, &asset_of(data, Some("not base64!".into())), NAME, &ours.public()), Err(Refused::Signature(_))));
        assert_eq!(check(data, &asset_of(data, Some(ours.sign(data, NAME))), NAME, ""), Err(Refused::NoKey));
        assert_eq!(check(data, &asset_of(data, Some(ours.sign(data, NAME))), NAME, "none yet"), Err(Refused::NoKey));
    }

    #[test]
    fn a_release_signature_names_its_file_and_its_version() {
        let (ours, theirs) = (TestKey::new(4), TestKey::new(5));
        let data = b"made-up release archive";
        let parts = ["file:ssheesh-1.0.1-linux-x64.tar.gz".to_owned(), "version:1.0.1".to_owned()];
        let comment = "timestamp:1794688787\tfile:ssheesh-1.0.1-linux-x64.tar.gz\tversion:1.0.1";
        // As `tauri signer` keeps it (base64) and as minisign writes it (the lines).
        assert_eq!(signed(data, &ours.sign_comment(data, comment), &ours.public(), &parts), Ok(()));
        assert_eq!(signed(data, &ours.minisig(data, comment), &ours.public(), &parts), Ok(()));
        // Another key: refused, however right its comment.
        assert!(matches!(signed(data, &theirs.sign_comment(data, comment), &ours.public(), &parts), Err(Refused::Signature(_))));
        // The right key, but signed for another version, or with no version at all.
        let older = "timestamp:1794688787\tfile:ssheesh-1.0.1-linux-x64.tar.gz\tversion:1.0.0";
        assert_eq!(signed(data, &ours.sign_comment(data, older), &ours.public(), &parts), Err(Refused::Name { want: "version 1.0.1".into() }));
        assert_eq!(signed(data, &ours.sign(data, "ssheesh-1.0.1-linux-x64.tar.gz"), &ours.public(), &parts), Err(Refused::Name { want: "version 1.0.1".into() }));
        // A field only starting like the one wanted isn't it.
        let longer = "timestamp:1794688787\tfile:ssheesh-1.0.1-linux-x64.tar.gz\tversion:1.0.10";
        assert!(matches!(signed(data, &ours.sign_comment(data, longer), &ours.public(), &parts), Err(Refused::Name { .. })));
    }
}
