//! Whether what came down can be trusted: where it comes from, its SHA-256
//! and its Ed25519 signature (the Java `UpdateVerifier`).
//!
//! The hash alone is not enough: whoever tampers with the ZIP can recompute
//! its hash effortlessly; what they cannot do is sign it. The signature is
//! what protects the user if the GitHub account itself is ever stolen.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use log::{debug, info};
use ring::signature::{ED25519, UnparsedPublicKey};
use sha2::{Digest, Sha256};

use super::{REPOSITORY, UpdateError};

const TRUSTED_HOST: &str = "github.com";

/// The same key the Java app carries: an X.509 SubjectPublicKeyInfo in PEM.
/// Compiled in, so a build without it cannot exist.
const PUBLIC_KEY_PEM: &str = include_str!("../../keys/update-signing.pub");

/// The fixed DER prefix of an Ed25519 SubjectPublicKeyInfo; the raw key follows.
const ED25519_SPKI_PREFIX: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

// === ORIGIN ===

/// Is this URL a release download of the app's own GitHub repository?
/// Checked BEFORE requesting anything: the URLs come from a JSON arriving
/// from outside.
///
/// The host is compared whole (`github.com.malo.tld` merely starts with it),
/// a user part is refused (in `https://github.com@malo.tld/` the real host is
/// the second one) and only https counts. The path has to START with the
/// repository's downloads: anyone can host files under github.com.
///
/// GitHub then redirects the download to its file servers; that hop is not
/// checked here, which is why the hash and the signature exist.
pub fn is_trusted_source(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else { return false };
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    if parsed.host_str().is_none_or(|host| !host.eq_ignore_ascii_case(TRUSTED_HOST)) {
        return false;
    }
    // GitHub ignores the case of owner and repository names.
    let downloads = format!("/{REPOSITORY}/releases/download/").to_lowercase();
    parsed.path().to_lowercase().starts_with(&downloads)
}

// === CONTENT ===

/// Fails closed: a hash that does not match stops before looking at the
/// signature, and a hash without a signature is never enough.
pub(super) fn verify(file: &Path, expected_hash: &str, signature: &[u8], public_key: &[u8]) -> Result<(), UpdateError> {
    let actual = sha256_file(file)?;
    let expected = clean_hash(expected_hash).ok_or(UpdateError::BadHashFile)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(UpdateError::HashMismatch { expected, actual });
    }
    if !verify_signature(file, signature, public_key)? {
        return Err(UpdateError::SignatureInvalid);
    }
    info!("Update verified: hash and signature are correct ({}).", file.display());
    Ok(())
}

/// The SHA-256 of a file, in lower-case hex.
pub fn sha256_file(file: &Path) -> io::Result<String> {
    let mut digest = Sha256::new();
    let mut input = File::open(file)?;
    let mut buffer = [0u8; 8192];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// The hash inside a published `.sha256`, whatever made it: bare,
/// `sha256sum` (hash and file name) or `certutil` (a courtesy line first, and
/// in old versions the hash split by spaces). None if there is no SHA-256.
pub(super) fn clean_hash(raw: &str) -> Option<String> {
    for line in raw.trim().lines() {
        if let Some(token) = line.split_whitespace().find(|t| is_sha256(t)) {
            return Some(token.to_lowercase());
        }
        let joined: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if is_sha256(&joined) {
            return Some(joined.to_lowercase());
        }
    }
    None
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Is `signature` a valid Ed25519 signature of the file's bytes? A missing or
/// malformed signature (truncated, the wrong size) is simply "not valid", never
/// an error: the user sees the plain reason instead of a cryptographic one.
pub(super) fn verify_signature(file: &Path, signature: &[u8], public_key: &[u8]) -> io::Result<bool> {
    if signature.is_empty() {
        return Ok(false);
    }
    let contents = std::fs::read(file)?;
    let valid = UnparsedPublicKey::new(&ED25519, public_key).verify(&contents, signature).is_ok();
    if !valid {
        debug!("Signature rejected ({} bytes).", signature.len());
    }
    Ok(valid)
}

/// The raw 32-byte key out of the embedded PEM.
pub(super) fn public_key() -> Result<Vec<u8>, UpdateError> {
    let base64: String = PUBLIC_KEY_PEM
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .flat_map(|line| line.chars().filter(|c| !c.is_whitespace()))
        .collect();
    let der = STANDARD.decode(base64).map_err(|e| UpdateError::KeyUnreadable(e.to_string()))?;
    match der.strip_prefix(&ED25519_SPKI_PREFIX[..]) {
        Some(raw) if raw.len() == 32 => Ok(raw.to_vec()),
        _ => Err(UpdateError::KeyUnreadable("not an Ed25519 public key".into())),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::fs;
    use tempfile::TempDir;

    pub struct Keys(Ed25519KeyPair);

    impl Keys {
        pub fn new() -> Self {
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            Self(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
        }

        pub fn public(&self) -> Vec<u8> {
            self.0.public_key().as_ref().to_vec()
        }

        pub fn sign(&self, file: &Path) -> Vec<u8> {
            self.0.sign(&fs::read(file).unwrap()).as_ref().to_vec()
        }
    }

    fn zip(dir: &TempDir, contents: &str) -> std::path::PathBuf {
        let file = dir.path().join("Soundify-1.8-tauri.zip");
        fs::write(&file, contents).unwrap();
        file
    }

    // --- origin ---

    #[test]
    fn the_app_repository_releases_are_accepted() {
        assert!(is_trusted_source("https://github.com/DannyGT97/Soundify/releases/download/v1.8/Soundify-1.8-tauri.zip"));
        assert!(is_trusted_source("https://github.com/dannygt97/soundify/releases/download/v1.8/x.zip"), "case does not matter");
    }

    #[test]
    fn security_a_domain_that_merely_starts_with_github_does_not_get_through() {
        assert!(!is_trusted_source("https://github.com.malo.tld/DannyGT97/Soundify/releases/download/v1/x.zip"));
        assert!(!is_trusted_source("https://notgithub.com/DannyGT97/Soundify/releases/download/v1/x.zip"));
    }

    #[test]
    fn security_nor_a_host_hidden_behind_an_at_sign() {
        assert!(!is_trusted_source("https://github.com@malo.tld/DannyGT97/Soundify/releases/download/v1/x.zip"));
    }

    #[test]
    fn neither_without_https_nor_from_another_repository() {
        assert!(!is_trusted_source("http://github.com/DannyGT97/Soundify/releases/download/v1/x.zip"), "no https");
        assert!(!is_trusted_source("https://github.com/otro/Soundify/releases/download/v1/x.zip"), "another owner");
        assert!(!is_trusted_source("https://github.com/DannyGT97/SoundifyFake/releases/download/v1/x.zip"));
        assert!(!is_trusted_source("https://github.com/otro/repo/x.zip"));
    }

    #[test]
    fn security_only_release_downloads_count_not_any_file_in_the_repository() {
        assert!(!is_trusted_source("https://github.com/DannyGT97/Soundify/raw/main/x.zip"));
        assert!(!is_trusted_source("https://github.com/otro/repo/DannyGT97/Soundify/releases/download/v1/x.zip"));
    }

    #[test]
    fn what_is_not_a_url_is_rejected_without_blowing_up() {
        assert!(!is_trusted_source(""));
        assert!(!is_trusted_source("   "));
        assert!(!is_trusted_source("no es una url"));
        assert!(!is_trusted_source("file:///C:/x.zip"), "a local path is never a valid origin");
    }

    // --- hash ---

    #[test]
    fn sha256_of_a_known_content() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "abc").unwrap();
        assert_eq!(sha256_file(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_format_left_by_sha256sum_and_certutil_is_accepted() {
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(clean_hash(hash).as_deref(), Some(hash));
        assert_eq!(clean_hash(&format!("{hash}  Soundify-1.8.zip\n")).as_deref(), Some(hash), "sha256sum");
        assert_eq!(clean_hash(&hash.to_uppercase()).as_deref(), Some(hash), "in upper case");
        let certutil = format!("SHA256 hash of Soundify-1.8.zip:\n{hash}\nCertUtil: -hashfile ok");
        assert_eq!(clean_hash(&certutil).as_deref(), Some(hash), "certutil, with its courtesy line");
    }

    #[test]
    fn a_half_formed_hash_is_rejected_instead_of_compared() {
        assert_eq!(clean_hash(""), None);
        assert_eq!(clean_hash("ba7816bf"), None, "too short");
        assert_eq!(clean_hash("zzzz"), None, "not even hexadecimal");
    }

    // --- signature ---

    #[test]
    fn what_is_signed_with_the_right_key_is_accepted() {
        let dir = TempDir::new().unwrap();
        let keys = Keys::new();
        let file = zip(&dir, "contenido de la actualización");
        assert!(verify_signature(&file, &keys.sign(&file), &keys.public()).unwrap());
    }

    #[test]
    fn regression_a_tampered_file_does_not_validate() {
        let dir = TempDir::new().unwrap();
        let keys = Keys::new();
        let file = zip(&dir, "contenido de la actualización");
        let signature = keys.sign(&file);
        // Somebody swaps the ZIP for another and leaves the original signature.
        fs::write(&file, "contenido de la actualización con sorpresa").unwrap();
        assert!(!verify_signature(&file, &signature, &keys.public()).unwrap());
    }

    #[test]
    fn signed_with_another_key_does_not_pass_either() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        assert!(!verify_signature(&file, &Keys::new().sign(&file), &Keys::new().public()).unwrap());
    }

    #[test]
    fn without_a_signature_nothing_is_taken_as_good() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        assert!(!verify_signature(&file, &[], &Keys::new().public()).unwrap());
    }

    // --- fails closed ---

    #[test]
    fn with_the_right_hash_but_any_old_signature_it_still_does_not_install() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        let hash = sha256_file(&file).unwrap();
        assert_eq!(verify(&file, &hash, &[1, 2, 3], &Keys::new().public()), Err(UpdateError::SignatureInvalid));
    }

    #[test]
    fn regression_a_malformed_signature_is_rejected_not_thrown_at_you() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        let hash = sha256_file(&file).unwrap();
        for broken in [vec![1u8], vec![1, 2, 3], vec![0; 63], vec![0; 200]] {
            assert_eq!(
                verify(&file, &hash, &broken, &Keys::new().public()),
                Err(UpdateError::SignatureInvalid),
                "signature of {} bytes",
                broken.len()
            );
        }
    }

    #[test]
    fn the_app_carries_its_public_key() {
        // If this fails, no update can be verified (and therefore installed).
        let key = public_key().unwrap();
        assert_eq!(key.len(), 32);
        assert_eq!(STANDARD.encode(&key), "oDKOHRv8V9SrsATlOJXHYRq0DwBsp4fwXEg7bRsMvW4=");
    }

    #[test]
    fn a_hash_that_does_not_match_stops_before_the_signature() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        let zeros = "0".repeat(64);
        assert!(matches!(verify(&file, &zeros, &[1], &Keys::new().public()), Err(UpdateError::HashMismatch { .. })));
    }

    #[test]
    fn an_unreadable_hash_file_does_not_pass_either() {
        let dir = TempDir::new().unwrap();
        let file = zip(&dir, "actualización");
        assert_eq!(verify(&file, "esto no es un hash", &[1], &Keys::new().public()), Err(UpdateError::BadHashFile));
    }
}
