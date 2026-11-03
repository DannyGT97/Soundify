//! Downloading an update and verifying it before anything else can touch it
//! (the Java `UpdateDownloader`).

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use log::{info, warn};

use super::verify::{self, is_trusted_source};
use super::{Update, UpdateError};
use crate::http::Http;

/// Downloads the update's ZIP into `updates_dir` and returns it only once its
/// hash and signature check out. What does not pass is deleted: somebody
/// could open it by hand assuming the app had validated it.
pub fn download(
    http: &dyn Http,
    update: &Update,
    updates_dir: &Path,
    on_progress: &mut dyn FnMut(f64),
) -> Result<PathBuf, UpdateError> {
    download_with_key(http, update, updates_dir, on_progress, &verify::public_key()?)
}

fn download_with_key(
    http: &dyn Http,
    update: &Update,
    updates_dir: &Path,
    on_progress: &mut dyn FnMut(f64),
    public_key: &[u8],
) -> Result<PathBuf, UpdateError> {
    // Belt and braces: these URLs were already filtered when reading the
    // release, but nothing stops another caller from building an `Update`.
    for url in [&update.hash_url, &update.signature_url, &update.zip_url] {
        if !is_trusted_source(url) {
            return Err(UpdateError::UntrustedSource(url.clone()));
        }
    }

    let expected_hash = http.get_string(&update.hash_url, &[])?;
    let signature = decode_signature(&http.get_string(&update.signature_url, &[])?)?;

    let zip = updates_dir.join(format!("Soundify-{}-tauri.zip", safe_name(&update.version)));
    std::fs::create_dir_all(updates_dir)?;
    http.to_file(&update.zip_url, &zip, on_progress)?;

    if let Err(e) = verify::verify(&zip, &expected_hash, &signature, public_key) {
        if let Err(remove) = std::fs::remove_file(&zip) {
            warn!("Could not delete the rejected download {}: {remove}", zip.display());
        }
        return Err(e);
    }
    info!("Update {} downloaded and verified at {}", update.version, zip.display());
    Ok(zip)
}

/// The published `.sig`: base64 (what the publishing script writes), or the
/// raw bytes if it is not base64.
fn decode_signature(raw: &str) -> Result<Vec<u8>, UpdateError> {
    if raw.trim().is_empty() {
        return Err(UpdateError::SignatureEmpty);
    }
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    // ISO-8859-1, as Java read it: each char is one byte.
    Ok(STANDARD.decode(cleaned).unwrap_or_else(|_| raw.chars().map(|c| c as u8).collect()))
}

/// The version as part of a file name. It comes from the release JSON: a
/// "../.." there would write outside `updates/`, so only what can be a
/// version is kept.
fn safe_name(version: &str) -> String {
    let cleaned: String =
        version.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).collect();
    if cleaned.is_empty() { "nueva".to_string() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::verify::tests::Keys;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::{fs, io};
    use tempfile::TempDir;

    const BASE: &str = "https://github.com/DannyGT97/Soundify/releases/download/v0.2.0/";

    /// Serves the three pieces; the ZIP is written to wherever it is asked.
    struct Published {
        texts: HashMap<String, String>,
        zip: Vec<u8>,
        requested: Mutex<Vec<String>>,
    }

    impl Http for Published {
        fn get_string(&self, url: &str, _: &[(&str, &str)]) -> io::Result<String> {
            self.requested.lock().unwrap().push(url.to_string());
            self.texts.get(url).cloned().ok_or_else(|| io::Error::other("404"))
        }

        fn to_file(&self, url: &str, target: &Path, on_progress: &mut dyn FnMut(f64)) -> io::Result<()> {
            self.requested.lock().unwrap().push(url.to_string());
            fs::write(target, &self.zip)?;
            on_progress(1.0);
            Ok(())
        }
    }

    fn update() -> Update {
        let file = format!("{BASE}Soundify-0.2.0-tauri.zip");
        Update {
            version: "v0.2.0".into(),
            notes: String::new(),
            zip_url: file.clone(),
            hash_url: format!("{file}.sha256"),
            signature_url: format!("{file}.sig"),
        }
    }

    /// What a real publication looks like: the hash and the signature of `zip`.
    fn published(zip: &[u8], keys: &Keys, scratch: &TempDir) -> Published {
        let original = scratch.path().join("original.zip");
        fs::write(&original, zip).unwrap();
        let update = update();
        let mut texts = HashMap::new();
        texts.insert(update.hash_url.clone(), format!("{}  Soundify-0.2.0-tauri.zip\n", verify::sha256_file(&original).unwrap()));
        texts.insert(update.signature_url.clone(), STANDARD.encode(keys.sign(&original)));
        Published { texts, zip: zip.to_vec(), requested: Mutex::default() }
    }

    #[test]
    fn a_genuine_download_is_kept() {
        let dir = TempDir::new().unwrap();
        let keys = Keys::new();
        let http = published(b"la app nueva", &keys, &dir);

        let zip = download_with_key(&http, &update(), &dir.path().join("updates"), &mut |_| {}, &keys.public()).unwrap();

        assert_eq!(zip, dir.path().join("updates").join("Soundify-v0.2.0-tauri.zip"));
        assert_eq!(fs::read(zip).unwrap(), b"la app nueva");
    }

    #[test]
    fn security_a_zip_swapped_on_the_way_is_rejected_and_deleted() {
        let dir = TempDir::new().unwrap();
        let keys = Keys::new();
        let mut http = published(b"la app nueva", &keys, &dir);
        http.zip = b"otra cosa".to_vec();

        let result = download_with_key(&http, &update(), &dir.path().join("updates"), &mut |_| {}, &keys.public());

        assert!(matches!(result, Err(UpdateError::HashMismatch { .. })));
        assert!(!dir.path().join("updates").join("Soundify-v0.2.0-tauri.zip").exists(), "nothing unverified is left");
    }

    #[test]
    fn security_signed_by_someone_else_is_rejected() {
        let dir = TempDir::new().unwrap();
        let http = published(b"la app nueva", &Keys::new(), &dir);

        let result = download_with_key(&http, &update(), &dir.path().join("updates"), &mut |_| {}, &Keys::new().public());

        assert_eq!(result, Err(UpdateError::SignatureInvalid));
    }

    #[test]
    fn security_a_foreign_url_is_refused_before_requesting_anything() {
        let dir = TempDir::new().unwrap();
        let http = published(b"x", &Keys::new(), &dir);
        let mut foreign = update();
        foreign.zip_url = "https://malo.tld/Soundify-0.2.0-tauri.zip".into();

        let result = download_with_key(&http, &foreign, dir.path(), &mut |_| {}, &Keys::new().public());

        assert_eq!(result, Err(UpdateError::UntrustedSource(foreign.zip_url.clone())));
        assert!(http.requested.lock().unwrap().is_empty());
    }

    #[test]
    fn an_empty_signature_is_said_as_such() {
        assert_eq!(decode_signature("  \n"), Err(UpdateError::SignatureEmpty));
        assert_eq!(decode_signature("AQID\n").unwrap(), vec![1, 2, 3]);
        assert_eq!(decode_signature("no base64!").unwrap(), b"no base64!".to_vec());
    }

    #[test]
    fn security_the_version_cannot_write_outside_updates() {
        assert_eq!(safe_name("v1.8.0"), "v1.8.0");
        assert_eq!(safe_name("../../x"), "....x");
        assert_eq!(safe_name("/\\:"), "nueva");
        assert_eq!(safe_name(""), "nueva");
    }
}
