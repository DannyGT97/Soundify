//! Reviewing and cleaning archives: the startup reconciliation and the
//! Gestor's "Eliminar todo" and "Olvidar". Nothing here happens on its own
//! except the startup reconciliation, which only updates what still exists
//! and deletes nothing.
//!
//! Note carried over from the Java app: clearing `unavailable_at` was only ever
//! done by the withdrawn "Archive maintenance" panel, and the pending list
//! always skips entries that have it. A video marked unavailable is not asked
//! for again even if it reappears on YouTube.

use log::{info, warn};
use serde::Serialize;

use super::io::{self, ARCHIVE_TXT, MANIFEST_NAME, has_real_file};
use super::sync;
use crate::Location;
use crate::storage::Storage;
use crate::transfer;

/// "Eliminar todo": `deleted` = recorded files deleted; `unregistered` = files
/// in the folder the app had not recorded, deleted all the same; `left_over` =
/// what could NOT be deleted, the only thing that deserves a warning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted: usize,
    pub unregistered: usize,
    pub left_over: usize,
}

impl DeleteResult {
    /// Everything that has ceased to exist: what the user is told.
    pub fn total(&self) -> usize {
        self.deleted + self.unregistered
    }
}

/// Walks every archive and updates which ids still have a file. A folder that
/// is not reachable right now (an unplugged drive) is skipped entirely, so
/// nothing is marked deleted by mistake. Device-backed archives are left for
/// their own reconciliation. Returns the archives that were reconciled, so the
/// caller can refresh their manifests.
pub fn reconcile_all(storage: &dyn Storage, archives: &Location, now: &str) -> Vec<Location> {
    let mut reconciled = Vec::new();
    for json in io::list_active_archive_json_files(storage, archives) {
        let mut data = match io::load(storage, &json) {
            Ok(data) => data,
            Err(e) => {
                warn!("Failed to reconcile {json}: {e}");
                continue;
            }
        };
        if data.is_device_backed() {
            continue;
        }
        let Some(folder) = data.folder.as_deref().filter(|f| !f.trim().is_empty()).map(Location::parse) else {
            info!("Archive has no registered folder, skipped until next use: {json}");
            continue;
        };
        if !storage.exists(&folder).unwrap_or(false) {
            info!("Folder not reachable, reconciliation skipped: {folder}");
            continue;
        }
        let Some(extension) = io::format_of(&json) else { continue };
        sync::reconcile_entries(storage, &mut data, &folder, &extension, now);
        data.last_synced_at = Some(now.to_string());
        match io::save(storage, &json, &data) {
            Ok(()) => {
                // The playlists from before the manifest became permanent were
                // left without one: here they get it, once and without any
                // noise. ALWAYS, not only when missing: the archive has just been
                // reconciled against the disk, so it is never more up to date.
                transfer::write_manifest_quietly(storage, &json, now);
                reconciled.push(json);
            }
            Err(e) => warn!("Failed to save reconciled archive {json}: {e}"),
        }
    }
    info!("Startup archive reconciliation done: {} archives checked.", reconciled.len());
    reconciled
}

/// Empties the playlist's whole folder and takes it away, INCLUDING files the
/// app had not recorded: leaving them forced the user to delete them by hand
/// right after asking for "everything", and the next download numbered above
/// them ("it starts at 011" — reported). The dialog says so before calling this.
/// The record itself goes with [`forget_playlist_format`], called right after.
pub fn delete_all_downloaded_files(storage: &dyn Storage, json: &Location) -> DeleteResult {
    let mut result = DeleteResult::default();
    let data = match io::load(storage, json) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to delete downloaded files of {json}: {e}");
            return result;
        }
    };
    if data.is_device_backed() {
        // Device playlists go through archive::device, which can reach the
        // phone; here nothing is claimed as deleted.
        result.left_over = data.entries.values().filter(|e| has_real_file(storage, &data, e)).count();
        return result;
    }
    for entry in data.entries.values() {
        let Some(path) = entry.file_path.as_deref().filter(|p| !p.trim().is_empty()) else { continue };
        let file = Location::parse(path);
        if storage.exists(&file).unwrap_or(false) {
            match storage.remove_file(&file) {
                Ok(()) => result.deleted += 1,
                Err(e) => {
                    warn!("Could not delete {file}: {e}");
                    result.left_over += 1;
                }
            }
        }
    }
    let Some(folder) = data.folder.as_deref().filter(|f| !f.trim().is_empty()).map(Location::parse) else {
        return result;
    };
    // Written by the app: deleted with the rest but NOT counted as a file it
    // had not recorded, which would be lying about our own file.
    if let Ok(manifest) = storage.child(&folder, MANIFEST_NAME)
        && storage.exists(&manifest).unwrap_or(false)
        && let Err(e) = storage.remove_file(&manifest)
    {
        warn!("Could not delete manifest {manifest}: {e}");
    }
    for child in storage.list(&folder).unwrap_or_default() {
        delete_recursively(storage, &child.location, child.is_dir, &mut result);
    }
    // Without this, an empty folder was left to delete by hand (reported).
    if storage.exists(&folder).unwrap_or(false)
        && let Err(e) = storage.remove_dir_all(&folder)
    {
        warn!("Could not delete folder {folder}: {e}");
    }
    result
}

/// Folders are not counted: what the user sees are their files.
fn delete_recursively(storage: &dyn Storage, location: &Location, is_dir: bool, result: &mut DeleteResult) {
    if is_dir {
        for child in storage.list(location).unwrap_or_default() {
            delete_recursively(storage, &child.location, child.is_dir, result);
        }
        if let Err(e) = storage.remove_dir_all(location) {
            warn!("Could not delete folder {location}: {e}");
        }
        return;
    }
    match storage.remove_file(location) {
        Ok(()) => result.unregistered += 1,
        Err(e) => {
            warn!("Could not delete {location}: {e}");
            result.left_over += 1;
        }
    }
}

/// "Olvidar": deletes THAT format's archive (json + txt), never the music. If
/// it was the playlist's last format, the empty `pl_` folder goes too.
pub fn forget_playlist_format(storage: &dyn Storage, json: &Location) {
    // Defensive: never risk the archives root by being called with a loose
    // individual archive.
    if !io::is_playlist_archive(json) {
        warn!("forget_playlist_format() called with a non-playlist archive, ignored: {json}");
        return;
    }
    let Some(fmt_dir) = io::parent_of(json) else { return };
    for file in [Some(json.clone()), storage.child(&fmt_dir, ARCHIVE_TXT).ok()].into_iter().flatten() {
        if storage.exists(&file).unwrap_or(false)
            && let Err(e) = storage.remove_file(&file)
        {
            warn!("Could not delete {file}: {e}");
        }
    }
    if storage.list(&fmt_dir).is_ok_and(|rest| rest.is_empty()) {
        let _ = storage.remove_dir_all(&fmt_dir);
        if let Some(pl_dir) = io::parent_of(&fmt_dir)
            && storage.list(&pl_dir).is_ok_and(|rest| rest.is_empty())
        {
            let _ = storage.remove_dir_all(&pl_dir);
        }
    }
    info!("Local playlist forgotten: {fmt_dir}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchiveData, ArchiveEntry};
    use crate::storage::MemoryStorage;

    fn path(text: &str) -> Location {
        Location::path(text)
    }

    #[test]
    fn delete_everything_takes_unrecorded_files_and_the_folder_too() {
        let storage = MemoryStorage::new();
        storage.create_dir_all(&path("/music/P (mp3)/sub")).unwrap();
        for file in ["/music/P (mp3)/001 - a.mp3", "/music/P (mp3)/stray.mp3", "/music/P (mp3)/sub/x.txt"] {
            storage.write(&path(file), b"x").unwrap();
        }
        storage.write(&path(&format!("/music/P (mp3)/{MANIFEST_NAME}")), b"{}").unwrap();
        let mut data = ArchiveData { folder: Some("/music/P (mp3)".into()), ..Default::default() };
        data.entries.insert("a".into(), ArchiveEntry { file_path: Some("/music/P (mp3)/001 - a.mp3".into()), ..Default::default() });
        let json = path("/archives/pl_P_000000/mp3/data.json");
        io::save(&storage, &json, &data).unwrap();

        let result = delete_all_downloaded_files(&storage, &json);
        assert_eq!(result, DeleteResult { deleted: 1, unregistered: 2, left_over: 0 });
        assert_eq!(result.total(), 3);
        assert!(!storage.exists(&path("/music/P (mp3)")).unwrap());
    }

    #[test]
    fn forgetting_the_last_format_removes_the_playlist_folder() {
        let storage = MemoryStorage::new();
        let json = path("/archives/pl_P_000000/mp3/data.json");
        io::save(&storage, &json, &ArchiveData::default()).unwrap();
        storage.write(&path("/archives/pl_P_000000/mp3/archive.txt"), b"").unwrap();
        let other = path("/archives/pl_P_000000/flac/data.json");
        io::save(&storage, &other, &ArchiveData::default()).unwrap();

        forget_playlist_format(&storage, &json);
        assert!(!storage.exists(&path("/archives/pl_P_000000/mp3")).unwrap());
        assert!(storage.exists(&other).unwrap());
        forget_playlist_format(&storage, &other);
        assert!(!storage.exists(&path("/archives/pl_P_000000")).unwrap());
    }

    #[test]
    fn forgetting_never_touches_an_individual_archive() {
        let storage = MemoryStorage::new();
        let json = path("/archives/individual_archive_mp3.json");
        io::save(&storage, &json, &ArchiveData::default()).unwrap();
        forget_playlist_format(&storage, &json);
        assert!(storage.exists(&json).unwrap());
    }

    #[test]
    fn an_unreachable_folder_is_skipped_at_startup() {
        let storage = MemoryStorage::new();
        let mut data = ArchiveData { folder: Some("/unplugged/P (mp3)".into()), ..Default::default() };
        data.entries.insert("a".into(), ArchiveEntry { file_path: Some("/unplugged/P (mp3)/a.mp3".into()), ..Default::default() });
        let json = path("/archives/pl_P_000000/mp3/data.json");
        io::save(&storage, &json, &data).unwrap();
        assert!(reconcile_all(&storage, &path("/archives"), "2026-11-02 10:00").is_empty());
        assert_eq!(io::load(&storage, &json).unwrap().entries["a"].deleted_at, None);
    }
}
