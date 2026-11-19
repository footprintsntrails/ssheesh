//! minisign's signing, as `tauri signer sign` does it, plus the version in the trusted comment.
//!
//! The key file (`tauri signer generate`, minisign's format): `untrusted comment: …` and one
//! base64 line of `Ed`, the KDF (`Sc`: scrypt, or none), `B2`, the KDF's salt, opslimit and
//! memlimit, then the key's id, its 64 secret bytes (seed and public key) and a BLAKE2b-256
//! checksum, those three XORed with scrypt's output when the key has a password. Tauri keeps
//! the whole file base64 again in `TAURI_SIGNING_PRIVATE_KEY`.
//!
//! A signature (prehashed, `ED`): Ed25519 over BLAKE2b-512 of the file, then a global
//! signature over that signature and the trusted comment. Tauri's `.sig` is the signature
//! file's text base64; minisign's `.minisig` is the text itself.

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use blake2::digest::consts::U32;
use blake2::{Blake2b, Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};

/// A release key, unlocked.
pub struct SecretKey {
    pub id: [u8; 8],
    key: SigningKey,
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretKey({})", hex_id(&self.id))
    }
}

/// The key's id as minisign prints it (`B239B8BC1DA9D744`).
pub fn hex_id(id: &[u8; 8]) -> String {
    id.iter().rev().map(|b| format!("{b:02X}")).collect()
}

/// libsodium's `pickparams` for scrypt: log2(N), r, p from minisign's opslimit and memlimit.
fn scrypt_params(opslimit: u64, memlimit: u64) -> (u8, u32, u32) {
    let ops = opslimit.max(32768);
    let r: u64 = 8;
    let (n_log2, p);
    if ops < memlimit / 32 {
        p = 1;
        let max_n = ops / (r * 4);
        n_log2 = (1..63u8).find(|&n| (1u64 << n) > max_n / 2).unwrap_or(63);
    } else {
        let max_n = memlimit / (r * 128);
        n_log2 = (1..63u8).find(|&n| (1u64 << n) > max_n / 2).unwrap_or(63);
        let max_rp = ((ops / 4) / (1u64 << n_log2)).min(0x3fff_ffff);
        p = max_rp / r;
    }
    (n_log2, r as u32, p as u32)
}

/// The key file's text: as given, or base64 of it (as Tauri keeps it).
fn key_text(given: &str) -> Result<String, String> {
    let given = given.trim();
    if given.starts_with("untrusted comment:") {
        return Ok(given.to_owned());
    }
    let bytes = STANDARD.decode(given.replace(['\n', '\r'], "")).map_err(|_| "the key is neither minisign's key file nor base64 of it")?;
    String::from_utf8(bytes).map_err(|_| "the key isn't text".into())
}

impl SecretKey {
    /// From `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
    pub fn from_env() -> Result<SecretKey, String> {
        let key = std::env::var("TAURI_SIGNING_PRIVATE_KEY").map_err(|_| "TAURI_SIGNING_PRIVATE_KEY isn't set")?;
        let password = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
        SecretKey::decode(&key, &password)
    }

    pub fn decode(given: &str, password: &str) -> Result<SecretKey, String> {
        let text = key_text(given)?;
        let line = text.lines().nth(1).ok_or("the key file has no key line")?.trim();
        let raw = STANDARD.decode(line).map_err(|_| "the key line isn't base64")?;
        if raw.len() != 158 || &raw[0..2] != b"Ed" || &raw[4..6] != b"B2" {
            return Err("not a minisign Ed25519 secret key".into());
        }
        let mut secret: Vec<u8> = raw[54..158].to_vec();
        match &raw[2..4] {
            b"Sc" => {
                let salt = &raw[6..38];
                let ops = u64::from_le_bytes(raw[38..46].try_into().unwrap_or_default());
                let mem = u64::from_le_bytes(raw[46..54].try_into().unwrap_or_default());
                let (n_log2, r, p) = scrypt_params(ops, mem);
                let params = scrypt::Params::new(n_log2, r, p, scrypt::Params::RECOMMENDED_LEN).map_err(|e| format!("the key's scrypt settings: {e}"))?;
                let mut stream = [0u8; 104];
                scrypt::scrypt(password.as_bytes(), salt, &params, &mut stream).map_err(|e| e.to_string())?;
                for (s, k) in secret.iter_mut().zip(stream) {
                    *s ^= k;
                }
            }
            [0, 0] => {}
            _ => return Err("the key is locked with a KDF other than scrypt".into()),
        }
        let id: [u8; 8] = secret[0..8].try_into().unwrap_or_default();
        let sk = &secret[8..72];
        let mut sum = Blake2b::<U32>::new();
        sum.update(b"Ed");
        sum.update(id);
        sum.update(sk);
        if sum.finalize().as_slice() != &secret[72..104] {
            return Err("the key's password is wrong (its checksum doesn't match)".into());
        }
        let seed: [u8; 32] = sk[0..32].try_into().unwrap_or_default();
        let key = SigningKey::from_bytes(&seed);
        if key.verifying_key().as_bytes() != &sk[32..64] {
            return Err("the key's public half doesn't match its secret".into());
        }
        Ok(SecretKey { id, key })
    }

    /// The public key file's text, as minisign writes it (the tests check signatures with it).
    #[cfg(test)]
    pub fn public_text(&self) -> String {
        let mut raw = b"Ed".to_vec();
        raw.extend_from_slice(&self.id);
        raw.extend_from_slice(self.key.verifying_key().as_bytes());
        format!("untrusted comment: minisign public key: {}\n{}\n", hex_id(&self.id), STANDARD.encode(raw))
    }

    /// The signature file's text for `data` with `trusted` as its trusted comment.
    pub fn signature_text(&self, data: &[u8], trusted: &str) -> Result<String, String> {
        if trusted.contains(['\n', '\r']) {
            return Err("a trusted comment is one line".into());
        }
        let digest = Blake2b512::digest(data);
        let sig = self.key.sign(&digest);
        let mut boxed = b"ED".to_vec();
        boxed.extend_from_slice(&self.id);
        boxed.extend_from_slice(&sig.to_bytes());
        let mut global = sig.to_bytes().to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let global = self.key.sign(&global);
        Ok(format!("untrusted comment: signature from tauri secret key\n{}\ntrusted comment: {trusted}\n{}\n", STANDARD.encode(boxed), STANDARD.encode(global.to_bytes())))
    }
}

/// The trusted comment for `name` of `version`, signed at `timestamp`.
pub fn trusted_comment(timestamp: u64, name: &str, version: &str) -> String {
    format!("timestamp:{timestamp}\tfile:{name}\tversion:{version}")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// Signs `file` for `version`: `<file>.sig` (Tauri's, base64) or `<file>.minisig` (the text).
pub fn sign_file(key: &SecretKey, file: &Path, version: &str, minisig: bool) -> Result<PathBuf, String> {
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or("a file name that isn't text")?;
    if name.contains(['\t', '\n']) {
        return Err(format!("{name}: no tabs or line ends in a signed name"));
    }
    let text = key.signature_text(&data, &trusted_comment(now(), name, version))?;
    let (out, body) = if minisig { (format!("{}.minisig", file.display()), text) } else { (format!("{}.sig", file.display()), STANDARD.encode(text)) };
    std::fs::write(&out, body).map_err(|e| format!("{out}: {e}"))?;
    Ok(out.into())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use minisign_verify::{PublicKey, Signature};

    /// Makes a key file as minisign does, locked with `password` under small scrypt settings
    /// (fast for a test; the format is the one `tauri signer generate` writes).
    pub fn key_file(seed: u8, password: Option<&str>) -> String {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let id = [seed, 2, 3, 4, 5, 6, 7, 8];
        let mut secret = id.to_vec();
        secret.extend_from_slice(&signing.to_bytes());
        secret.extend_from_slice(signing.verifying_key().as_bytes());
        let mut sum = Blake2b::<U32>::new();
        sum.update(b"Ed");
        sum.update(id);
        sum.update(&secret[8..72]);
        secret.extend_from_slice(&sum.finalize());
        let salt = [7u8; 32];
        let (ops, mem) = (32768u64, 16 * 1024 * 1024u64);
        let mut raw = b"Ed".to_vec();
        raw.extend_from_slice(if password.is_some() { b"Sc" } else { &[0, 0] });
        raw.extend_from_slice(b"B2");
        raw.extend_from_slice(&salt);
        raw.extend_from_slice(&ops.to_le_bytes());
        raw.extend_from_slice(&mem.to_le_bytes());
        if let Some(password) = password {
            let (n, r, p) = scrypt_params(ops, mem);
            let mut stream = [0u8; 104];
            scrypt::scrypt(password.as_bytes(), &salt, &scrypt::Params::new(n, r, p, scrypt::Params::RECOMMENDED_LEN).unwrap(), &mut stream).unwrap();
            for (s, k) in secret.iter_mut().zip(stream) {
                *s ^= k;
            }
        }
        raw.extend_from_slice(&secret);
        STANDARD.encode(format!("untrusted comment: rsign encrypted secret key\n{}\n", STANDARD.encode(raw)))
    }

    #[test]
    fn libsodiums_scrypt_settings() {
        // rsign's (and so Tauri's) defaults: N = 2^15, r = 8, p = 1.
        assert_eq!(scrypt_params(1_048_576, 33_554_432), (15, 8, 1));
        // minisign's own defaults (opslimit 2^25, memlimit 2^30): N = 2^20, r = 8, p = 1.
        assert_eq!(scrypt_params(33_554_432, 1_073_741_824), (20, 8, 1));
        assert_eq!(scrypt_params(32768, 16 * 1024 * 1024), (10, 8, 1));
    }

    #[test]
    fn a_signature_checks_out_as_0_10_and_1x_read_it() {
        let file = key_file(9, Some("hunter2"));
        assert!(SecretKey::decode(&file, "wrong").unwrap_err().contains("password"));
        let key = SecretKey::decode(&file, "hunter2").unwrap();
        let public = PublicKey::decode(&key.public_text()).unwrap();
        let trusted = trusted_comment(1_794_688_787, "SSHeesh_1.0.0_x64-setup.exe", "1.0.0");
        let text = key.signature_text(b"the installer", &trusted).unwrap();
        let sig = Signature::decode(&text).unwrap();
        // `true`: legacy allowed, as tauri-plugin-updater calls it.
        public.verify(b"the installer", &sig, true).unwrap();
        assert_eq!(sig.trusted_comment(), trusted);
        assert!(public.verify(b"another file", &sig, true).is_err());
        // Unlocked keys read too.
        assert_eq!(SecretKey::decode(&key_file(9, None), "").unwrap().id, key.id);
        assert!(key.signature_text(b"x", "a\nb").is_err());
    }

    #[test]
    fn the_signature_files_are_tauris_and_minisigns() {
        let key = SecretKey::decode(&key_file(4, Some("pw")), "pw").unwrap();
        let dir = std::env::temp_dir().join(format!("ssheesh-release-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("update-v1.json");
        std::fs::write(&file, b"{}").unwrap();
        let sig = sign_file(&key, &file, "1.0.0", false).unwrap();
        let text = String::from_utf8(STANDARD.decode(std::fs::read_to_string(&sig).unwrap()).unwrap()).unwrap();
        assert!(text.contains("\tfile:update-v1.json\tversion:1.0.0\n"), "{text}");
        let minisig = sign_file(&key, &file, "1.0.0", true).unwrap();
        assert!(std::fs::read_to_string(&minisig).unwrap().starts_with("untrusted comment:"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
