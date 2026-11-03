//! The archive side of a playlist whose music lives on a phone (the device
//! parts of the Java `ArchiveStore`, `ArchiveSync` and `ArchiveMaintenance`):
//! reconciling it against a fresh listing of the device and deleting it from
//! there. Registering lives in the facade, next to its local twin.
//!
//! Every function that talks to the device blocks (see [`PortableDevice`]):
//! callers run them off the UI thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use log::{info, warn};

use super::io::{self, has_real_file};
use super::maintenance::DeleteResult;
use crate::Location;
use crate::device::PortableDevice;
use crate::model::ArchiveData;
use crate::storage::Storage;
use crate::transfer;

/// After a delete the device says failed, the fresh listings that get the
/// last word (see [`delete_all_from_device`]).
const DELETE_RECHECKS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

/// The device-backed `reconcile_entries`: no folder to scan, one fresh listing
/// of the device instead (name -> size). Only ever called with a listing that
/// really came from the device right now. Entries that never had a device file
/// (a pending download's placeholder) are left alone: there is nothing on the
/// device to compare them with.
pub fn reconcile_device_entries(data: &mut ArchiveData, device_listing: &HashMap<String, u64>, now: &str) {
    for entry in data.entries.values_mut() {
        let Some(file_name) = entry.device_file_name.as_deref().filter(|n| !n.trim().is_empty()) else { continue };
        if device_listing.contains_key(file_name) {
            entry.deleted_at = None;
        } else if entry.deleted_at.is_none() {
            entry.deleted_at = Some(now.to_string());
        }
    }
}

/// The device-backed equivalent of checking a local folder: if the phone is
/// not connected right now, nothing is touched — nothing is marked deleted just
/// because the cable is out. Returns whether the archive was reconciled.
pub fn reconcile_device_archive(storage: &dyn Storage, phone: &dyn PortableDevice, json: &Location, now: &str) -> bool {
    let mut data = match io::load(storage, json) {
        Ok(data) if data.is_device_backed() => data,
        Ok(_) => return false,
        Err(e) => {
            warn!("Failed to reconcile device archive {json}: {e}");
            return false;
        }
    };
    let (Some(device_id), Some(device_path)) = (data.device_id.clone(), data.device_path.clone()) else { return false };
    if !phone.is_available() {
        return false;
    }
    // is_reachable(), not list_devices(): the latter only says Windows shows a
    // device by this name, which a charge-only phone still does — but the
    // listing just below would then come back EMPTY (storage unreadable) and
    // be read as "every file is gone", stamping deleted_at on the whole
    // archive. This is the one device call that WRITES unattended at startup,
    // so getting "connected" wrong here is the most dangerous spot of all.
    if !phone.is_reachable(&device_id) {
        info!("Device not connected, reconciliation skipped: {}", data.device_name.as_deref().unwrap_or(&device_id));
        return false;
    }
    let listing: HashMap<String, u64> =
        phone.list(&device_id, &device_path).into_iter().filter(|e| !e.is_folder).map(|e| (e.name, e.size_bytes)).collect();
    reconcile_device_entries(&mut data, &listing, now);
    data.last_synced_at = Some(now.to_string());
    match io::save(storage, json, &data) {
        Ok(()) => true,
        Err(e) => {
            warn!("Failed to save reconciled device archive {json}: {e}");
            false
        }
    }
}

/// The startup pass for the device archives, which the local one leaves out:
/// run on a thread of its own by the caller, since a device that does not
/// answer must never delay the first thing the user sees. Each device call is
/// already bounded by the implementation, so one bad phone cannot hold up the
/// rest for long. Returns the archives reconciled.
pub fn reconcile_device_archives(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    archives: &Location,
    scratch: &Path,
    now: &str,
) -> Vec<Location> {
    if !phone.is_available() {
        return Vec::new();
    }
    let mut reconciled = Vec::new();
    for json in io::list_active_archive_json_files(storage, archives) {
        if !io::load(storage, &json).is_ok_and(|data| data.is_device_backed()) {
            continue;
        }
        if reconcile_device_archive(storage, phone, &json, now) {
            // The device playlists from before the manifest went there too.
            transfer::device::write_manifest_quietly(storage, phone, &json, scratch, now);
            reconciled.push(json);
        }
    }
    info!("Startup device archive reconciliation done: {} archives checked.", reconciled.len());
    reconciled
}

/// "Eliminar todo" for a device playlist: ONE delete of the whole folder —
/// whatever is in it goes, recorded or not, the same "everything means
/// everything" as the local one.
///
/// Pessimistic by default: if the device cannot even be asked, everything
/// recorded is reported as `left_over` rather than claiming a success that
/// did not happen (only what could NOT be deleted is shown in red).
pub fn delete_all_from_device(storage: &dyn Storage, phone: &dyn PortableDevice, data: &ArchiveData) -> DeleteResult {
    delete_all_with_rechecks(storage, phone, data, &DELETE_RECHECKS)
}

fn delete_all_with_rechecks(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    data: &ArchiveData,
    rechecks: &[Duration],
) -> DeleteResult {
    let recorded = data.entries.values().filter(|e| has_real_file(storage, data, e)).count();
    let pessimistic = DeleteResult { left_over: recorded, ..Default::default() };
    let (Some(device_id), Some(device_path)) = (data.device_id.as_deref(), data.device_path.as_deref()) else {
        return pessimistic;
    };
    if !phone.is_available() {
        warn!("Portable device transport not available on this OS, nothing deleted");
        return pessimistic;
    }
    let total = phone.list(device_id, device_path).iter().filter(|e| !e.is_folder).count();
    let mut ok = phone.delete(device_id, &[device_path.to_string()]);
    // The shell has been seen to report failure for a delete that had gone
    // through, so a fresh listing gets the last word — and not just one right
    // after: the shell's cache of the folder can still show what was there a
    // moment ago (reported again with a single check). A short, bounded retry
    // lets it settle before trusting it either way.
    if !ok {
        for delay in rechecks {
            std::thread::sleep(*delay);
            if phone.list(device_id, device_path).is_empty() {
                ok = true;
                break;
            }
        }
    }
    if !ok {
        warn!("Could not delete the device folder: {}:{device_path}", data.device_name.as_deref().unwrap_or(device_id));
        return pessimistic;
    }
    DeleteResult { deleted: recorded, unregistered: total.saturating_sub(recorded), left_over: 0 }
}

/// Where a device playlist's files wait locally before being pushed: one
/// folder per archive (`<staging>/pl_<Name>_<hash>/<format>`), stable across
/// runs, so a file left over by an interrupted push is found again the next
/// time this same playlist is synced. A loose-downloads device archive (a flat
/// `individual_device_archive_...json`) gets its own folder by file stem.
pub fn staging_folder(staging_root: &Path, json: &Location) -> Option<PathBuf> {
    let json = json.as_path()?;
    if io::is_playlist_archive(&Location::Path(json.to_path_buf())) {
        let format_dir = json.parent()?;
        let playlist_dir = format_dir.parent()?;
        Some(staging_root.join(playlist_dir.file_name()?).join(format_dir.file_name()?))
    } else {
        Some(staging_root.join(json.file_stem()?))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::device::fake::{FakeDevice, ID, NAME};
    use crate::model::ArchiveEntry;
    use crate::storage::LocalStorage;

    const NOW: &str = "2026-11-03 10:00";

    fn device_data(files: &[&str]) -> ArchiveData {
        let mut data = ArchiveData {
            title: Some("Cocina".into()),
            device_id: Some(ID.into()),
            device_name: Some(NAME.into()),
            device_path: Some("Music/Cocina (mp3)".into()),
            ..Default::default()
        };
        for name in files {
            data.entries
                .insert(name.to_string(), ArchiveEntry { device_file_name: Some(name.to_string()), ..Default::default() });
        }
        data.entries.insert("pending".into(), ArchiveEntry::default());
        data
    }

    fn saved(dir: &Path, data: &ArchiveData) -> Location {
        let folder = dir.join("archives").join("pl_Cocina_abc").join("mp3");
        fs::create_dir_all(&folder).unwrap();
        let json = Location::Path(folder.join("data.json"));
        io::save(&LocalStorage, &json, data).unwrap();
        json
    }

    #[test]
    fn reconciling_marks_what_left_the_device_and_clears_what_came_back() {
        let mut data = device_data(&["a.mp3", "b.mp3"]);
        data.entries.get_mut("b.mp3").unwrap().deleted_at = Some("2026-01-01 00:00".into());
        let listing = HashMap::from([("b.mp3".to_string(), 3)]);
        reconcile_device_entries(&mut data, &listing, NOW);
        assert_eq!(data.entries["a.mp3"].deleted_at.as_deref(), Some(NOW));
        assert_eq!(data.entries["b.mp3"].deleted_at, None);
        assert_eq!(data.entries["pending"].deleted_at, None);
    }

    #[test]
    fn reconciling_keeps_the_first_date_it_went_missing() {
        let mut data = device_data(&["a.mp3"]);
        data.entries.get_mut("a.mp3").unwrap().deleted_at = Some("2026-01-01 00:00".into());
        reconcile_device_entries(&mut data, &HashMap::new(), NOW);
        assert_eq!(data.entries["a.mp3"].deleted_at.as_deref(), Some("2026-01-01 00:00"));
    }

    #[test]
    fn regression_a_phone_that_does_not_answer_marks_nothing_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let json = saved(dir.path(), &device_data(&["a.mp3"]));
        let phone = FakeDevice::new();
        phone.set_reachable(false);
        assert!(!reconcile_device_archive(&LocalStorage, &phone, &json, NOW));
        assert_eq!(io::load(&LocalStorage, &json).unwrap().entries["a.mp3"].deleted_at, None);
    }

    #[test]
    fn a_connected_phone_is_reconciled_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let json = saved(dir.path(), &device_data(&["a.mp3", "b.mp3"]));
        let phone = FakeDevice::new();
        phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaa");
        assert!(reconcile_device_archive(&LocalStorage, &phone, &json, NOW));
        let data = io::load(&LocalStorage, &json).unwrap();
        assert_eq!(data.entries["a.mp3"].deleted_at, None);
        assert_eq!(data.entries["b.mp3"].deleted_at.as_deref(), Some(NOW));
        assert_eq!(data.last_synced_at.as_deref(), Some(NOW));
    }

    #[test]
    fn the_startup_pass_reconciles_device_archives_and_refreshes_their_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        saved(dir.path(), &device_data(&["a.mp3"]));
        let phone = FakeDevice::new();
        phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaa");
        let archives = Location::Path(dir.path().join("archives"));
        assert_eq!(reconcile_device_archives(&LocalStorage, &phone, &archives, &scratch, NOW).len(), 1);
        assert!(phone.file("Music/Cocina (mp3)/soundify-playlist.json").is_some());
    }

    #[test]
    fn deleting_everything_removes_the_folder_and_counts_the_unrecorded() {
        let phone = FakeDevice::new();
        phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaa");
        phone.put_file("Music/Cocina (mp3)/stray.mp3", b"x");
        let result = delete_all_with_rechecks(&LocalStorage, &phone, &device_data(&["a.mp3"]), &[]);
        assert_eq!(result, DeleteResult { deleted: 1, unregistered: 1, left_over: 0 });
        assert!(!phone.has_folder("Music/Cocina (mp3)"));
    }

    #[test]
    fn deleting_from_an_unreachable_phone_reports_everything_as_left_over() {
        let phone = FakeDevice::new();
        phone.set_reachable(false);
        let result = delete_all_with_rechecks(&LocalStorage, &phone, &device_data(&["a.mp3", "b.mp3"]), &[]);
        assert_eq!(result, DeleteResult { deleted: 0, unregistered: 0, left_over: 2 });
    }

    #[test]
    fn the_staging_folder_follows_the_archive_folder() {
        let root = Path::new("staging");
        let json = Location::Path(PathBuf::from("archives").join("pl_Cocina_abc").join("mp3").join("data.json"));
        assert_eq!(staging_folder(root, &json), Some(root.join("pl_Cocina_abc").join("mp3")));
        let loose = Location::Path(PathBuf::from("archives").join("individual_device_archive_abc_mp3.json"));
        assert_eq!(staging_folder(root, &loose), Some(root.join("individual_device_archive_abc_mp3")));
    }
}
