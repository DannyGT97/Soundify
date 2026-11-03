//! Swaps the app for the new version (the Java `UpdateInstaller`).
//!
//! **Windows will not let you overwrite an executable that is running**, so
//! the replacement has to be done by another process. Here that "other
//! process" is **the new version itself**, running from the download folder:
//!
//! 1. The app downloads and verifies the ZIP (see [`super::download`])
//! 2. It unpacks it into `updates/staged/`
//! 3. It launches `updates/staged/<exe> --apply-update <folder>` and closes
//! 4. The new copy runs from ANOTHER folder, so the old folder's files are
//!    free once the old app is gone. It waits for that, does the swap and
//!    relaunches the app in its usual place
//! 5. The new version's first start cleans up what is left over
//!
//! No separate executable and no scripts: the swapping code lives in this very
//! repository and the NEW version runs it, so a failure here is fixed forwards.
//!
//! **What is never touched:** `data/` (settings, archives, logs) and `updates/`.
//!
//! **The way back:** a folder being replaced is moved aside to `.old` before
//! copying the new one and is not deleted until the new version has started
//! properly once. If the copying is cut short, what was moved aside is restored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::{error, info, warn};

use super::UpdateError;
use crate::tools::zip_extract::extract_matching;

pub const APPLY_FLAG: &str = "--apply-update";

/// The suffix of what is moved aside while the swap lasts.
pub const OLD_SUFFIX: &str = ".old";

/// What an update may NOT touch however much it comes in the ZIP: these are
/// the user's things, not the app's.
const PROTECTED: [&str; 2] = ["data", "updates"];

// === THE "CURRENT APP" SIDE ===

/// Unpacks the already verified ZIP into `staged` (emptied first) and returns
/// the folder holding `exe_name`.
pub fn stage(verified_zip: &Path, staged: &Path, exe_name: &str) -> Result<PathBuf, UpdateError> {
    delete_tree(staged);
    fs::create_dir_all(staged)?;

    let files = extract_matching(verified_zip, |name| safe_target(staged, name))?;
    if files == 0 {
        return Err(UpdateError::EmptyZip);
    }
    let Some(root) = find_app_root(staged, exe_name) else {
        delete_tree(staged);
        return Err(UpdateError::NoAppInZip(exe_name.to_string()));
    };
    info!("Update staged at {} ({files} files).", root.display());
    Ok(root)
}

// === THE "NEW VERSION" SIDE ===

/// Was it started to install an update rather than to open the app? The
/// destination folder, or None for a normal start (a flag with no value
/// behind it included: there is no folder to touch).
pub fn app_dir_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2).find(|pair| pair[0] == APPLY_FLAG).map(|pair| PathBuf::from(&pair[1]))
}

/// Waits until `exe` can be opened for writing, which is exactly what the swap
/// needs: Windows refuses it while the old app is running (and Linux answers
/// "text file busy"). No process API involved. False if it never frees up.
pub fn wait_until_writable(exe: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if !exe.exists() || fs::OpenOptions::new().write(true).open(exe).is_ok() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Copies `source`'s contents over `app_dir`, respecting what belongs to the
/// user and leaving a way back. Kept apart from the waits and the processes
/// so it can be tested.
///
/// - `data/` and `updates/` are not touched.
/// - Folders are replaced WHOLE, not merged: leaving the old jumbled with the
///   new leaves the pieces of two versions inside.
/// - What is replaced is moved aside to `.old` and restored if something fails.
pub fn swap_into(source: &Path, app_dir: &Path) -> io::Result<()> {
    let mut moved_aside = Vec::new();
    let result = swap_entries(source, app_dir, &mut moved_aside);
    if let Err(e) = &result {
        error!("Swap failed, rolling back: {e}");
        restore(&moved_aside);
    }
    result
}

fn swap_entries(source: &Path, app_dir: &Path, moved_aside: &mut Vec<PathBuf>) -> io::Result<()> {
    fs::create_dir_all(app_dir)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if PROTECTED.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        let target = app_dir.join(&name);
        if entry.file_type()?.is_dir() {
            if target.exists() {
                let aside = aside_of(&target);
                delete_tree(&aside);
                fs::rename(&target, &aside)?;
                moved_aside.push(target.clone());
            }
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Cleans up what an update leaves behind: the folders moved aside and the
/// download copy. Called at startup, once the new version has proven it
/// works, which is exactly when the way back stops being needed.
pub fn clean_up_after_update(app_dir: &Path, staged: &Path) {
    match fs::read_dir(app_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().ends_with(OLD_SUFFIX) {
                    info!("Cleaning leftover from the previous update: {}", entry.path().display());
                    delete_tree(&entry.path());
                }
            }
        }
        Err(e) => warn!("Could not clean up update leftovers: {e}"),
    }
    delete_tree(staged);
}

// === PURE LOGIC ===

/// Where to write a ZIP entry, or None if it tries to escape `base`.
///
/// A ZIP arrives from outside, and its entries can be called
/// `../../Windows/System32/something.dll`. The name is resolved lexically
/// (a backslash counts as a separator: some Windows tools write them) and
/// discarded if it leaves the folder.
pub fn safe_target(base: &Path, entry_name: &str) -> Option<PathBuf> {
    if entry_name.trim().is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for piece in entry_name.split(['/', '\\']) {
        match piece {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    warn!("Entry rejected for trying to escape the folder: {entry_name}");
                    return None;
                }
            }
            // "C:" or a root: an absolute path is never inside the folder.
            _ if piece.contains(':') => return None,
            _ => parts.push(piece),
        }
    }
    if entry_name.starts_with(['/', '\\']) || parts.is_empty() {
        return None;
    }
    Some(parts.iter().fold(base.to_path_buf(), |path, part| path.join(part)))
}

/// The folder with the app inside the unpacked ZIP. Both ways of compressing
/// are accepted: with a wrapping folder inside, or with its contents at the root.
pub fn find_app_root(staged: &Path, exe_name: &str) -> Option<PathBuf> {
    if staged.join(exe_name).is_file() {
        return Some(staged.to_path_buf());
    }
    fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|dir| dir.is_dir() && dir.join(exe_name).is_file())
}

// === PRIVATE ===

fn aside_of(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(OLD_SUFFIX);
    target.with_file_name(name)
}

/// Puts back what was moved aside, when the installation has gone wrong.
fn restore(moved_aside: &[PathBuf]) {
    for target in moved_aside {
        delete_tree(target);
        let aside = aside_of(target);
        if aside.exists()
            && let Err(e) = fs::rename(&aside, target)
        {
            error!("Could not restore {}: the installation may be half-done ({e})", target.display());
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn delete_tree(path: &Path) {
    let result = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    if let Err(e) = result
        && e.kind() != io::ErrorKind::NotFound
    {
        warn!("Could not fully delete {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Component;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    const EXE: &str = "soundify.exe";

    /// The resolved path is made of plain names only: no ".." survived.
    fn is_plain(path: &Path, base: &Path) -> bool {
        path.strip_prefix(base).is_ok_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))))
    }

    fn file(dir: &Path, name: &str, contents: &str) {
        let target = dir.join(name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, contents).unwrap();
    }

    fn read(path: PathBuf) -> String {
        fs::read_to_string(path).unwrap()
    }

    // --- a ZIP cannot write wherever it likes ---

    #[test]
    fn security_an_entry_with_dot_dot_is_discarded() {
        let base = Path::new("staged");
        assert_eq!(safe_target(base, "../fuera.dll"), None);
        assert_eq!(safe_target(base, "../../Windows/System32/algo.dll"), None);
        assert_eq!(safe_target(base, "app/../../fuera.dll"), None);
        // Windows ZIPs use a backslash with some generators.
        assert_eq!(safe_target(base, "..\\fuera.dll"), None);
        assert_eq!(safe_target(base, "/etc/passwd"), None);
        assert_eq!(safe_target(base, "C:\\Windows\\x.dll"), None);
    }

    #[test]
    fn what_belongs_inside_is_extracted() {
        let base = Path::new("staged");
        assert_eq!(safe_target(base, EXE), Some(base.join(EXE)));
        assert_eq!(safe_target(base, "app/x.dll"), Some(base.join("app").join("x.dll")));
        // A "." in the middle is innocent: what counts is where it ends up.
        assert_eq!(safe_target(base, "app/./x.dll"), Some(base.join("app").join("x.dll")));
        assert!(is_plain(&safe_target(base, "a/b/../c").unwrap(), base));
    }

    #[test]
    fn an_empty_name_is_not_an_entry() {
        let base = Path::new("staged");
        assert_eq!(safe_target(base, ""), None);
        assert_eq!(safe_target(base, "   "), None);
    }

    // --- where the app lives inside the ZIP ---

    #[test]
    fn both_with_and_without_a_wrapping_folder_are_accepted() {
        let dir = TempDir::new().unwrap();
        let plain = dir.path().join("plain");
        file(&plain, EXE, "exe");
        assert_eq!(find_app_root(&plain, EXE), Some(plain.clone()));

        let wrapped = dir.path().join("wrapped");
        file(&wrapped, &format!("Soundify/{EXE}"), "exe");
        assert_eq!(find_app_root(&wrapped, EXE), Some(wrapped.join("Soundify")));
    }

    #[test]
    fn a_zip_with_no_executable_is_not_an_update() {
        let dir = TempDir::new().unwrap();
        file(dir.path(), "leeme.txt", "hola");
        fs::create_dir_all(dir.path().join("app")).unwrap();
        assert_eq!(find_app_root(dir.path(), EXE), None);
    }

    #[test]
    fn staging_unpacks_and_finds_the_app() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("u.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        for (name, body) in [(format!("Soundify/{EXE}"), "exe nuevo"), ("../fuera.txt".into(), "no")] {
            writer.start_file(name, SimpleFileOptions::default()).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let staged = dir.path().join("updates").join("staged");
        file(&staged, "leftover.txt", "of a previous attempt");

        let root = stage(&zip, &staged, EXE).unwrap();

        assert_eq!(read(root.join(EXE)), "exe nuevo");
        assert!(!staged.join("leftover.txt").exists(), "staged is emptied first");
        assert!(!dir.path().join("updates").join("fuera.txt").exists());
    }

    #[test]
    fn a_zip_without_the_app_is_refused_and_cleaned() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("u.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        writer.start_file("leeme.txt", SimpleFileOptions::default()).unwrap();
        writer.write_all(b"hola").unwrap();
        writer.finish().unwrap();
        let staged = dir.path().join("staged");

        assert_eq!(stage(&zip, &staged, EXE), Err(UpdateError::NoAppInZip(EXE.into())));
        assert!(!staged.exists());
    }

    // --- launch arguments ---

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_install_launch_is_recognised() {
        assert_eq!(app_dir_from_args(&args(&["soundify.exe", APPLY_FLAG, "C:\\Soundify"])), Some(PathBuf::from("C:\\Soundify")));
    }

    #[test]
    fn a_normal_launch_is_not_mistaken_for_an_install_one() {
        assert_eq!(app_dir_from_args(&args(&[])), None);
        assert_eq!(app_dir_from_args(&args(&["soundify.exe", "--otra-cosa", "x"])), None);
        assert_eq!(app_dir_from_args(&args(&["soundify.exe", APPLY_FLAG])), None);
    }

    #[test]
    fn a_free_executable_needs_no_wait() {
        let dir = TempDir::new().unwrap();
        file(dir.path(), EXE, "exe");
        assert!(wait_until_writable(&dir.path().join(EXE), Duration::ZERO));
        assert!(wait_until_writable(&dir.path().join("missing.exe"), Duration::ZERO));
    }

    // --- the swap ---

    fn old_installation(dir: &TempDir) -> PathBuf {
        let app = dir.path().join("Soundify");
        file(&app, EXE, "exe viejo");
        file(&app, "resources/old-only.txt", "recurso viejo");
        file(&app, "data/config/settings.json", r#"{"theme":"DARK"}"#);
        file(&app, "data/history/archives/pl_x/mp3/data.json", r#"{"entries":{}}"#);
        file(&app, "updates/Soundify-0.2.0-tauri.zip", "el zip descargado");
        app
    }

    fn new_version(dir: &TempDir) -> PathBuf {
        let fresh = dir.path().join("staged").join("Soundify");
        file(&fresh, EXE, "exe nuevo");
        file(&fresh, "resources/new.txt", "recurso nuevo");
        file(&fresh, "data/config/settings.json", "{}");
        fresh
    }

    #[test]
    fn the_user_data_is_not_touched() {
        let dir = TempDir::new().unwrap();
        let app = old_installation(&dir);

        swap_into(&new_version(&dir), &app).unwrap();

        assert_eq!(read(app.join("data/config/settings.json")), r#"{"theme":"DARK"}"#);
        assert!(app.join("data/history/archives/pl_x/mp3/data.json").exists(), "the archives are the most valuable thing");
        assert!(app.join("updates/Soundify-0.2.0-tauri.zip").exists(), "nor updates/: it is where the install comes from");
    }

    #[test]
    fn the_executable_and_the_app_folders_are_replaced() {
        let dir = TempDir::new().unwrap();
        let app = old_installation(&dir);

        swap_into(&new_version(&dir), &app).unwrap();

        assert_eq!(read(app.join(EXE)), "exe nuevo");
        assert_eq!(read(app.join("resources/new.txt")), "recurso nuevo");
    }

    #[test]
    fn folders_are_replaced_whole_the_two_versions_are_not_mixed() {
        let dir = TempDir::new().unwrap();
        let app = old_installation(&dir);

        swap_into(&new_version(&dir), &app).unwrap();

        assert!(!app.join("resources/old-only.txt").exists());
    }

    #[test]
    fn the_old_one_is_moved_aside_not_deleted() {
        let dir = TempDir::new().unwrap();
        let app = old_installation(&dir);

        swap_into(&new_version(&dir), &app).unwrap();

        assert_eq!(read(app.join(format!("resources{OLD_SUFFIX}/old-only.txt"))), "recurso viejo");
    }

    #[test]
    fn the_next_startup_cleans_up() {
        let dir = TempDir::new().unwrap();
        let app = old_installation(&dir);
        swap_into(&new_version(&dir), &app).unwrap();
        let staged = app.join("updates").join("staged");
        file(&staged, EXE, "copia de la descarga");

        clean_up_after_update(&app, &staged);

        assert!(!app.join(format!("resources{OLD_SUFFIX}")).exists());
        assert!(!staged.exists());
        assert!(app.join("resources/new.txt").exists(), "cleaning up does not take the installed version with it");
        assert!(app.join("updates/Soundify-0.2.0-tauri.zip").exists());
    }

    #[test]
    fn a_completely_fresh_install_works_too() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("destino");

        swap_into(&new_version(&dir), &target).unwrap();

        assert_eq!(read(target.join(EXE)), "exe nuevo");
        assert!(!target.join("data").exists(), "not even a fresh data/ comes from the ZIP");
    }
}
