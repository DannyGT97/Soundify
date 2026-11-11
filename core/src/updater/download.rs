//! Downloading an update and verifying it before anything else can touch it
//! (the Java `UpdateDownloader`). The package is the portable ZIP or, on
//! Android, the APK: the checks are the same.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use log::{info, warn};

use super::verify::{self, is_trusted_source};
use super::{Update, UpdateError};
use crate::http::Http;

/// Downloads the update's package (ZIP or APK) into `updates_dir` and returns it only once its
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
    for url in [&update.hash_url, &update.signature_url, &update.package_url] {
        if !is_trusted_source(url) {
            return Err(UpdateError::UntrustedSource(url.clone()));
        }
    }

    let expected_hash = http.get_string(&update.hash_url, &[])?;
    let signature = decode_signature(&http.get_string(&update.signature_url, &[])?)?;

    let suffix = update.package.suffix();
    let zip = updates_dir.join(format!("Soundify-{}{suffix}", safe_name(&update.version)));
    std::fs::create_dir_all(updates_dir)?;
    http.to_file(&update.package_url, &zip, on_progress)?;

    if let Err(e) = verify::verify(&zip, &expected_hash, &signature, public_key) {
        if let Err(remove) = std::fs::remove_file(&zip) {
            warn!("Could not delete the rejected download {}: {remove}", zip.display());
        }
        return Err(e);
    }
    info!("Update {} downloaded and verified at {}", update.version, zip.display());
    prune_old_zips(updates_dir, &zip, suffix, KEPT_ZIPS);
    Ok(zip)
}

/// How many downloaded ZIPs stay in `updates/` (the new one included): someone
/// updating release after release would otherwise pile up ~80 MB for each.
pub const KEPT_ZIPS: usize = 2;

/// Deletes the oldest `Soundify-*<suffix>` of `updates_dir` until only `keep`
/// are left. `current` (the one just downloaded) is never deleted. Only files
/// with that exact name pattern are touched: `staged/` and anything else stay.
fn prune_old_zips(updates_dir: &Path, current: &Path, suffix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(updates_dir) else { return };
    let mut zips: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_ours = name.starts_with("Soundify-") && name.ends_with(suffix);
            let meta = entry.metadata().ok()?;
            (is_ours && meta.is_file()).then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), entry.path()))
        })
        .collect();
    // Newest first; the file name breaks ties so the result does not depend on the disk's order.
    zips.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut kept = 0;
    for (_, path) in zips {
        if path == current || kept < keep {
            kept += 1;
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => info!("Deleted the old update {}", path.display()),
            Err(e) => warn!("Could not delete the old update {}: {e}", path.display()),
        }
    }
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
    use crate::updater::Package;
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
            package: Package::Zip,
            package_url: file.clone(),
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

    /// A file in `dir` whose modification time is `age_secs` seconds ago.
    fn aged(dir: &Path, name: &str, age_secs: u64) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"zip").unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
        fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        path
    }

    #[test]
    fn only_the_newest_zips_are_kept() {
        let dir = TempDir::new().unwrap();
        let updates = dir.path().join("updates");
        fs::create_dir_all(updates.join("staged")).unwrap();
        for (version, age) in [("v1.0.0", 4000), ("v1.1.0", 3000), ("v1.1.2", 2000), ("v1.1.3", 1000)] {
            aged(&updates, &format!("Soundify-{version}-tauri.zip"), age);
        }
        let notes = aged(&updates, "notas.txt", 9000);
        let keys = Keys::new();
        let http = published(b"la app nueva", &keys, &dir);

        download_with_key(&http, &update(), &updates, &mut |_| {}, &keys.public()).unwrap();

        let mut left: Vec<String> = fs::read_dir(&updates).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        // The new one and the newest of the old ones; the rest of the files are not ours to delete.
        assert_eq!(left, ["Soundify-v0.2.0-tauri.zip", "Soundify-v1.1.3-tauri.zip", "notas.txt", "staged"]);
        assert!(notes.exists());
    }

    #[test]
    fn a_rejected_download_does_not_delete_the_old_zips() {
        let dir = TempDir::new().unwrap();
        let updates = dir.path().join("updates");
        fs::create_dir_all(&updates).unwrap();
        let old = aged(&updates, "Soundify-v1.0.0-tauri.zip", 1000);
        let keys = Keys::new();
        let mut http = published(b"la app nueva", &keys, &dir);
        http.zip = b"otra cosa".to_vec();

        assert!(download_with_key(&http, &update(), &updates, &mut |_| {}, &keys.public()).is_err());

        assert!(old.exists());
    }

    #[test]
    fn an_apk_update_is_downloaded_as_an_apk_and_prunes_only_apks() {
        let dir = TempDir::new().unwrap();
        let updates = dir.path().join("updates");
        fs::create_dir_all(&updates).unwrap();
        for (version, age) in [("v1.0.0", 3000), ("v1.1.0", 2000), ("v1.1.3", 1000)] {
            aged(&updates, &format!("Soundify-{version}-tauri.apk"), age);
        }
        let zip = aged(&updates, "Soundify-v1.0.0-tauri.zip", 9000);
        let keys = Keys::new();
        let http = published(b"la app nueva", &keys, &dir);
        let mut apk = update();
        apk.package = Package::Apk;

        let path = download_with_key(&http, &apk, &updates, &mut |_| {}, &keys.public()).unwrap();

        assert_eq!(path, updates.join("Soundify-v0.2.0-tauri.apk"));
        let mut left: Vec<String> = fs::read_dir(&updates).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["Soundify-v0.2.0-tauri.apk", "Soundify-v1.0.0-tauri.zip", "Soundify-v1.1.3-tauri.apk"]);
        assert!(zip.exists(), "a ZIP is not an APK");
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
        foreign.package_url = "https://malo.tld/Soundify-0.2.0-tauri.zip".into();

        let result = download_with_key(&http, &foreign, dir.path(), &mut |_| {}, &Keys::new().public());

        assert_eq!(result, Err(UpdateError::UntrustedSource(foreign.package_url.clone())));
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
