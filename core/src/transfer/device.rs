//! The device-backed twin of the parent module (= `DevicePlaylistTransfer` in
//! the Java app): the manifest of a playlist that lives on a phone, copying a
//! playlist to and from a phone, and finding and importing one there.
//!
//! The manifest is the same [`PlaylistManifest`] local playlists use; only how
//! it is filled and read back differs (`device_file_name` instead of a path,
//! the device fields instead of `folder`).
//!
//! Every device call blocks and can take seconds (see [`PortableDevice`]), so
//! all of this runs off the UI thread. The local end is a real folder on disk:
//! a pushed or pulled file needs a path, which is why a local location that is
//! not one is refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use log::{debug, info, warn};

use super::{
    ExportResult, ImportResult, Result, SCHEMA, TransferError, count_downloaded, downloaded_items, is_file,
    pending_count, readable_size, remove_if_empty, undo_copies, write_manifest,
};
use crate::Location;
use crate::archive::io::{self, ARCHIVE_TXT, DATA_JSON, MANIFEST_NAME};
use crate::device::{self, DeviceEntry, DeviceTarget, PortableDevice, join_path, last_segment, parent_path};
use crate::model::{ArchiveData, ArchiveEntry, ManifestItem, PlaylistManifest};
use crate::paths;
use crate::storage::Storage;

/// Smaller than the local search's: every folder looked at here is a device
/// round trip (tens to hundreds of ms), not a local syscall. A cap generous for
/// a whole drive is reckless for a phone.
const MAX_SCAN_DEPTH: usize = 2;
const MAX_SCAN_DIRS: usize = 300;

/// How one import inside a batch went (see [`import_all`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceImportOutcome {
    pub location: DeviceTarget,
    pub result: Option<ImportResult>,
    pub error: Option<TransferError>,
    pub already_registered: bool,
}

// === THE MANIFEST ON THE DEVICE ===

/// Leaves (or refreshes) the manifest inside this device playlist's folder,
/// from the same moments the archive really changes. Never fails and
/// interrupts nothing, like the local one: a phone that does not answer right
/// now cannot bring down a sync that finished well. A local archive is none of
/// its business.
pub fn write_manifest_quietly(storage: &dyn Storage, phone: &dyn PortableDevice, json: &Location, scratch: &Path, now: &str) {
    let attempt = || -> Result<()> {
        let data = io::load(storage, json)?;
        let (Some(device_id), Some(device_path)) = (data.device_id.as_deref(), data.device_path.as_deref()) else {
            return Ok(());
        };
        if !data.is_device_backed() || data.title.is_none() {
            return Ok(());
        }
        let manifest = build_manifest(storage, json, &data, now);
        if !push_manifest(phone, device_id, device_path, &manifest, scratch)? {
            debug!("Could not push the manifest to {device_id}:{device_path}");
        }
        Ok(())
    };
    if let Err(e) = attempt() {
        debug!("Could not write the device manifest of {json}: {e}");
    }
}

/// The local `build_manifest`, reading the device fields instead.
pub fn build_manifest(storage: &dyn Storage, json: &Location, data: &ArchiveData, exported_at: &str) -> PlaylistManifest {
    let format = io::parent_of(json).map(|parent| parent.name()).unwrap_or_default();
    let folder_name = last_segment(data.device_path.as_deref().unwrap_or_default()).to_string();
    let items = data
        .entries
        .iter()
        .map(|(id, entry)| ManifestItem {
            id: id.clone(),
            number: entry.number.clone(),
            title: entry.title.clone(),
            // Same rule as local: the name only travels if the file really is
            // on the device; otherwise the item is pending at the destination.
            file_name: if io::has_real_file(storage, data, entry) { entry.device_file_name.clone() } else { None },
            unavailable_at: entry.unavailable_at.clone(),
            unavailable_reason: entry.unavailable_reason.clone(),
            removed_from_playlist_at: entry.removed_from_playlist_at.clone(),
        })
        .collect();
    PlaylistManifest {
        schema: SCHEMA,
        exported_at: exported_at.to_string(),
        title: data.title.clone().unwrap_or_default(),
        url: data.url.clone(),
        format,
        number_files: data.number_files,
        folder_name,
        items,
    }
}

fn push_manifest(
    phone: &dyn PortableDevice,
    device_id: &str,
    device_path: &str,
    manifest: &PlaylistManifest,
    scratch: &Path,
) -> Result<bool> {
    let temp = TempFolder::new(scratch, "manifest")?;
    // Named like the destination from the start: there is no rename step on
    // the device for it.
    let local = temp.path.join(MANIFEST_NAME);
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| TransferError::Io(e.to_string()))?;
    std::fs::write(&local, bytes)?;
    if !phone.mkdirs(device_id, device_path) {
        return Ok(false);
    }
    // The old one is deleted first rather than trusting the copy to overwrite
    // a file of the same name: every other push goes to a brand-new name, so
    // overwriting on MTP was never exercised. Not finding one is fine.
    phone.delete(device_id, &[join_path(device_path, MANIFEST_NAME)]);
    Ok(phone.push(device_id, device_path, &local, None))
}

// === COPYING A LOCAL PLAYLIST TO A DEVICE ===

/// Copies an already downloaded LOCAL playlist onto a device folder and leaves
/// the manifest there. What to copy is decided exactly as a local export
/// decides it; only writing the bytes changes.
///
/// `target.path` is the BASE the picker returned: the final "Title (format)"
/// folder is worked out here from the manifest, so a local folder whose name
/// does not follow that convention never leaks into the device path.
///
/// Cancelling deletes what THIS run pushed in one batch (coarser than the
/// local per-file undo, an accepted difference between the two transports) and
/// never writes the manifest. A push that fails aborts the whole copy, as a
/// failed local copy would.
#[allow(clippy::too_many_arguments)]
pub fn export_to_device(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    json: &Location,
    target: &DeviceTarget,
    progress: &mut dyn FnMut(usize, usize, &str),
    cancelled: &dyn Fn() -> bool,
    scratch: &Path,
    now: &str,
) -> Result<ExportResult> {
    // Asked fresh: the picker showing the phone a moment ago proves nothing.
    if !phone.is_available() || !phone.is_reachable(&target.device_id) {
        return Err(TransferError::DeviceUnreachable);
    }
    let manifest = super::build_manifest(storage, json, now)?;
    let source = super::source_folder(storage, json)?;
    let device_id = target.device_id.as_str();
    let folder = paths::device_playlist_sub_path(Some(&target.path), &manifest.title, &manifest.format);
    let to_copy = downloaded_items(&manifest);

    // One listing up front, not one round trip per file: what is already
    // there, by name and size.
    if !phone.mkdirs(device_id, &folder) {
        return Err(TransferError::DeviceUnreachable);
    }
    let existing = file_sizes(&phone.list(device_id, &folder));

    let (mut copied, mut skipped, mut missing) = (0, 0, 0);
    let mut stopped = false;
    let mut pushed_now = Vec::new();
    for (i, item) in to_copy.iter().enumerate() {
        if cancelled() {
            stopped = true;
            break;
        }
        let name = item.file_name.as_deref().unwrap_or_default();
        let from = storage.child(&source, name)?;
        if !storage.exists(&from)? {
            missing += 1;
        } else if already_on_device(storage, &from, name, &existing) {
            skipped += 1;
        } else {
            let local = local_path(&from)?;
            if !phone.push(device_id, &folder, local, Some(name)) {
                return Err(TransferError::CopyToDeviceFailed { file_name: name.to_string() });
            }
            copied += 1;
            pushed_now.push(join_path(&folder, name));
        }
        progress(i + 1, to_copy.len(), name);
    }

    if stopped {
        // One batch call deletes all of it or none of it.
        let undone = if !pushed_now.is_empty() && phone.delete(device_id, &pushed_now) { pushed_now.len() } else { 0 };
        // Cancelling leaves nothing half done, the folder included (a cancelled
        // copy left an empty folder behind — reported). Judged by what was
        // there BEFORE this run, not by listing it again: a listing right after
        // a delete can still show stale contents on MTP.
        if existing.is_empty() {
            phone.delete(device_id, std::slice::from_ref(&folder));
        }
        return Ok(ExportResult { copied: 0, skipped, missing, cancelled: true, undone });
    }

    if !push_manifest(phone, device_id, &folder, &manifest, scratch)? {
        debug!("Could not push the manifest to {device_id}:{folder}");
    }
    info!(
        "Playlist copied to device: {} ({}): {copied} copied, {skipped} already there, {missing} with no file, to {}:{folder}",
        manifest.title, manifest.format, target.device_name
    );
    Ok(ExportResult { copied, skipped, missing, cancelled: false, undone: 0 })
}

/// Same name + size as the local `already_there`, against a device listing.
pub fn already_on_device(storage: &dyn Storage, source: &Location, file_name: &str, device_sizes: &HashMap<String, u64>) -> bool {
    match (device_sizes.get(file_name), storage.size(source)) {
        (Some(on_device), Ok(local)) => *on_device == local,
        _ => false,
    }
}

// === COPYING A DEVICE PLAYLIST TO THIS COMPUTER ===

/// The reverse of [`export_to_device`]: a device playlist's music down onto a
/// local folder, with the manifest left there, so the result is exactly what
/// a local copy produces. `destination` is the BASE chosen; the playlist's own
/// folder is made inside. Cancelling rolls back what this run pulled, file by
/// file: it is real local disk.
pub fn export_from_device(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    json: &Location,
    destination: &Location,
    progress: &mut dyn FnMut(usize, usize, &str),
    cancelled: &dyn Fn() -> bool,
    now: &str,
) -> Result<ExportResult> {
    let data = io::load(storage, json)?;
    let (Some(device_id), Some(device_path)) = (data.device_id.clone(), data.device_path.clone()) else {
        return Err(TransferError::NoFolder);
    };
    if !phone.is_available() || !phone.is_reachable(&device_id) {
        return Err(TransferError::DeviceUnreachable);
    }

    let manifest = build_manifest(storage, json, &data, now);
    let target = storage.child(destination, &manifest.folder_name)?;
    let to_copy = downloaded_items(&manifest);
    // What is really on the device right now, by name and size, in one listing.
    let on_device = file_sizes(&phone.list(&device_id, &device_path));

    storage.create_dir_all(&target)?;
    // Unlike the way to the phone (MTP gives no free space), this end is local.
    let needed = bytes_to_pull(storage, &to_copy, &on_device, &target)?;
    if let Some(free) = storage.available_space(&target)?
        && needed > free
    {
        remove_if_empty(storage, &target);
        return Err(TransferError::NotEnoughSpace { needed: readable_size(needed), free: readable_size(free) });
    }

    let (mut copied, mut skipped, mut missing) = (0, 0, 0);
    let mut stopped = false;
    let mut copied_now = Vec::new();
    for (i, item) in to_copy.iter().enumerate() {
        if cancelled() {
            stopped = true;
            break;
        }
        let name = item.file_name.as_deref().unwrap_or_default();
        let to = storage.child(&target, name)?;
        match on_device.get(name) {
            // Gone from the device since the last sync: the same "missing" a
            // local file the archive remembers but that vanished counts as.
            None => missing += 1,
            Some(&size) if already_local(storage, &to, size) => skipped += 1,
            Some(_) => {
                if !phone.pull(&device_id, &join_path(&device_path, name), local_path(&to)?) {
                    return Err(TransferError::CopyFromDeviceFailed { file_name: name.to_string() });
                }
                copied += 1;
                copied_now.push(to);
            }
        }
        progress(i + 1, to_copy.len(), name);
    }

    if stopped {
        let undone = undo_copies(storage, &copied_now, &target);
        info!("Copy from device cancelled: rolled back {undone} of {} file(s) copied in this run.", copied_now.len());
        return Ok(ExportResult { copied: 0, skipped, missing, cancelled: true, undone });
    }

    write_manifest(storage, &target, &manifest)?;
    info!(
        "Playlist copied from device: {} ({}): {copied} copied, {skipped} already there, {missing} with no file, to {destination}",
        manifest.title, manifest.format
    );
    Ok(ExportResult { copied, skipped, missing, cancelled: false, undone: 0 })
}

/// Same name + size, against a size read from the device.
pub fn already_local(storage: &dyn Storage, target: &Location, expected_size: u64) -> bool {
    is_file(storage, target) && storage.size(target).is_ok_and(|size| size == expected_size)
}

/// The bytes that really have to be pulled: what is on the device and not yet
/// right at the destination. One already fetched listing, no round trips.
pub fn bytes_to_pull(
    storage: &dyn Storage,
    items: &[&ManifestItem],
    device_sizes: &HashMap<String, u64>,
    target: &Location,
) -> Result<u64> {
    let mut total = 0;
    for item in items {
        let name = item.file_name.as_deref().unwrap_or_default();
        // Not on the device right now: counted as missing while copying.
        let Some(&size) = device_sizes.get(name) else { continue };
        if !already_local(storage, &storage.child(target, name)?, size) {
            total += size;
        }
    }
    Ok(total)
}

// === FINDING PLAYLISTS ON A DEVICE ===

/// The importable playlist folders in sight from `root` inside a device: the
/// folder itself if it carries a manifest, or the ones that do down to two
/// levels below. A folder with a manifest is not followed inwards.
pub fn find_importable(phone: &dyn PortableDevice, device_id: &str, device_name: &str, root: &str) -> Vec<DeviceTarget> {
    if !phone.is_available() {
        return Vec::new();
    }
    let at = |path: &str| DeviceTarget { device_id: device_id.into(), device_name: device_name.into(), path: path.into() };
    let entries = phone.list(device_id, root);
    if has_manifest_entry(&entries) {
        return vec![at(root)];
    }
    let mut found = Vec::new();
    scan_for_manifests(phone, device_id, root, &entries, 1, &mut found, &mut 0);
    let mut found: Vec<DeviceTarget> = found.iter().map(|path| at(path)).collect();
    found.sort_by_key(|location| location.path.to_lowercase());
    found
}

fn scan_for_manifests(
    phone: &dyn PortableDevice,
    device_id: &str,
    path: &str,
    entries: &[DeviceEntry],
    depth: usize,
    found: &mut Vec<String>,
    visited: &mut usize,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    for entry in entries.iter().filter(|e| e.is_folder) {
        if *visited >= MAX_SCAN_DIRS {
            return;
        }
        *visited += 1;
        let child = join_path(path, &entry.name);
        let children = phone.list(device_id, &child);
        if has_manifest_entry(&children) {
            found.push(child);
            continue;
        }
        scan_for_manifests(phone, device_id, &child, &children, depth + 1, found, visited);
    }
}

fn has_manifest_entry(entries: &[DeviceEntry]) -> bool {
    entries.iter().any(|e| !e.is_folder && e.name == MANIFEST_NAME)
}

/// Is this location already registered here? A cheap check that never touches
/// the device: it reads "Title (format)" off the folder name (the convention
/// every device playlist folder is created with). Pulling every manifest just
/// to filter a list would be one more slow round trip per playlist BEFORE the
/// user confirmed anything; [`import_from`] makes the real check with the
/// manifest in hand. When it cannot tell, it says NO, like the local one.
pub fn is_already_registered(storage: &dyn Storage, archives: &Location, location: &DeviceTarget) -> bool {
    let Some((title, format)) = device::parse_folder_name(last_segment(&location.path)) else {
        return false;
    };
    archive_json(storage, archives, location, title, format).is_ok_and(|json| storage.exists(&json).unwrap_or(false))
}

pub fn not_yet_registered(storage: &dyn Storage, archives: &Location, locations: &[DeviceTarget]) -> Vec<DeviceTarget> {
    locations.iter().filter(|l| !is_already_registered(storage, archives, l)).cloned().collect()
}

// === IMPORTING ===

/// Imports several locations in one batch: one failing does not stop the rest.
/// `progress(done, total, folder name)` is said before each one, since pulling
/// a manifest can take seconds and a batch with nothing said in between left
/// no way to show "N of M" (reported).
pub fn import_all(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    archives: &Location,
    locations: &[DeviceTarget],
    progress: &mut dyn FnMut(usize, usize, &str),
    scratch: &Path,
    now: &str,
) -> Vec<DeviceImportOutcome> {
    let mut outcomes = Vec::new();
    for (i, location) in locations.iter().enumerate() {
        progress(i, locations.len(), last_segment(&location.path));
        let outcome = match import_from(storage, phone, archives, location, scratch, now) {
            Ok(result) => DeviceImportOutcome { location: location.clone(), result: Some(result), error: None, already_registered: false },
            Err(error) => {
                let already_registered = matches!(error, TransferError::AlreadyRegistered { .. });
                if !already_registered {
                    warn!("Could not import {}:{}: {error}", location.device_name, location.path);
                }
                DeviceImportOutcome { location: location.clone(), result: None, error: Some(error), already_registered }
            }
        };
        outcomes.push(outcome);
    }
    progress(locations.len(), locations.len(), "");
    outcomes
}

/// Registers a playlist found on a device. Not one music file is copied: it is
/// already on the phone, and that device + folder becomes the playlist's.
pub fn import_from(
    storage: &dyn Storage,
    phone: &dyn PortableDevice,
    archives: &Location,
    location: &DeviceTarget,
    scratch: &Path,
    now: &str,
) -> Result<ImportResult> {
    let manifest = pull_manifest(phone, location, scratch)?;
    let json = archive_json(storage, archives, location, &manifest.title, &manifest.format)?;
    if storage.exists(&json)? {
        return Err(TransferError::AlreadyRegistered { title: manifest.title, format: manifest.format });
    }
    let txt = io::txt_file_for(&json).ok_or_else(|| TransferError::Io(format!("no {ARCHIVE_TXT} for {json}")))?;

    let data = to_archive_data(&manifest, location, now);
    io::save(storage, &json, &data)?;
    io::export_work_file(storage, &txt, &data)?;

    let downloaded = count_downloaded(storage, &data);
    let total = manifest.items.len();
    info!(
        "Device playlist imported: {} ({}): {total} ids, {downloaded} with a file, from {}:{}",
        manifest.title, manifest.format, location.device_name, location.path
    );
    Ok(ImportResult {
        pending: pending_count(&manifest, downloaded),
        title: manifest.title,
        format: manifest.format,
        total,
        downloaded,
    })
}

fn pull_manifest(phone: &dyn PortableDevice, location: &DeviceTarget, scratch: &Path) -> Result<PlaylistManifest> {
    let temp = TempFolder::new(scratch, "import")?;
    let local = temp.path.join(MANIFEST_NAME);
    if !phone.pull(&location.device_id, &join_path(&location.path, MANIFEST_NAME), &local) || !local.is_file() {
        return Err(TransferError::NoManifest);
    }
    let manifest: PlaylistManifest =
        serde_json::from_slice(&std::fs::read(&local)?).map_err(|e| TransferError::Io(e.to_string()))?;
    if manifest.schema > SCHEMA {
        return Err(TransferError::NewerSchema { found: manifest.schema, supported: SCHEMA });
    }
    Ok(manifest)
}

/// The local `to_archive_data`, writing the device fields instead of paths.
pub fn to_archive_data(manifest: &PlaylistManifest, location: &DeviceTarget, now: &str) -> ArchiveData {
    let mut data = ArchiveData {
        url: manifest.url.clone(),
        title: Some(manifest.title.clone()),
        device_id: Some(location.device_id.clone()),
        device_name: Some(location.device_name.clone()),
        device_path: Some(location.path.clone()),
        number_files: manifest.number_files,
        last_synced_at: Some(now.to_string()),
        ..Default::default()
    };
    let mut last_number: Option<String> = None;
    for item in &manifest.items {
        if let Some(number) = &item.number
            && last_number.as_ref().is_none_or(|last| number > last)
        {
            last_number = Some(number.clone());
        }
        data.entries.insert(
            item.id.clone(),
            ArchiveEntry {
                added_at: Some(now.to_string()),
                number: item.number.clone(),
                title: item.title.clone(),
                unavailable_at: item.unavailable_at.clone(),
                unavailable_reason: item.unavailable_reason.clone(),
                removed_from_playlist_at: item.removed_from_playlist_at.clone(),
                device_file_name: item.file_name.clone(),
                ..Default::default()
            },
        );
    }
    data.last_number = last_number;
    data
}

// === HELPERS ===

/// The archive a device playlist folder would have: its hash goes by device +
/// BASE folder (the one holding "Title (format)"), as a new one is registered.
fn archive_json(storage: &dyn Storage, archives: &Location, location: &DeviceTarget, title: &str, format: &str) -> Result<Location> {
    let base = device::hash_key(&location.device_id, parent_path(&location.path));
    let dir = paths::playlist_archive_dir(storage, archives, title, &base, format)?;
    Ok(storage.child(&dir, DATA_JSON)?)
}

fn file_sizes(entries: &[DeviceEntry]) -> HashMap<String, u64> {
    entries.iter().filter(|e| !e.is_folder).map(|e| (e.name.clone(), e.size_bytes)).collect()
}

/// A device copies to and from real files: a location that is not a path
/// (an Android URI) has no business here.
fn local_path(location: &Location) -> Result<&Path> {
    location.as_path().ok_or_else(|| TransferError::Io(format!("{location} is not a local file")))
}

/// A folder of its own under the scratch folder, removed when dropped.
pub(crate) struct TempFolder {
    pub path: PathBuf,
}

impl TempFolder {
    pub fn new(scratch: &Path, prefix: &str) -> std::io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = scratch.join(format!("{prefix}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests;
