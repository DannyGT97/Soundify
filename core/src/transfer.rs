//! Copying a local playlist to another folder and importing one, around the
//! manifest (`soundify-playlist.json`) that lives inside every playlist's music
//! folder (= `PlaylistTransfer` in the Java app).
//!
//! The manifest is the playlist's passport: it is written into the music folder
//! every time the archive changes (register, end of a download, end of a sync,
//! startup reconciliation), so ANY music folder can be imported as is. The
//! archive keeps being the source of truth: the manifest never overwrites what
//! the app already knows, it only fills gaps.

use std::path::{Component, Path, PathBuf};

use log::{debug, info, warn};
use serde::Serialize;

use crate::Location;
use crate::archive::io::{self, ARCHIVE_TXT, DATA_JSON, MANIFEST_NAME};
use crate::archive::sync;
use crate::model::{ArchiveData, ArchiveEntry, ManifestItem, PlaylistManifest};
use crate::paths;
use crate::storage::Storage;

/// The manifest format this version understands; a newer one is refused.
pub const SCHEMA: i32 = 1;

/// How far down [`find_importable`] looks. Two levels cover what people really
/// do — pointing at `Music` (with the playlists inside) or `Documents\Music`
/// (one folder in between) — without crawling the whole disk.
const MAX_SCAN_DEPTH: usize = 2;

/// Folders looked at in one search at most: pointing at a drive's root stops
/// there and returns what was found instead of leaving the app thinking.
const MAX_SCAN_DIRS: usize = 4000;

// === ERRORS AND RESULTS ===

/// Why a copy or an import could not go ahead. The interface shows it through
/// [`TransferError::key`] and [`TransferError::args`] (the Java app's i18n keys).
#[derive(Debug, Clone, PartialEq)]
pub enum TransferError {
    NoFolder,
    SameFolder { title: String },
    NotEnoughSpace { needed: String, free: String },
    NoManifest,
    InvalidManifest,
    NewerSchema { found: i32, supported: i32 },
    AlreadyRegistered { title: String, format: String },
    DriveRoot,
    FolderMissing { folder_name: String },
    DeviceUnreachable,
    CopyToDeviceFailed { file_name: String },
    CopyFromDeviceFailed { file_name: String },
    Io(String),
}

impl TransferError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::NoFolder => "transfer.noFolder",
            Self::SameFolder { .. } => "transfer.sameFolder",
            Self::NotEnoughSpace { .. } => "transfer.notEnoughSpace",
            Self::NoManifest => "transfer.noManifest",
            Self::InvalidManifest => "transfer.invalidManifest",
            Self::NewerSchema { .. } => "transfer.newerSchema",
            Self::AlreadyRegistered { .. } => "transfer.alreadyRegistered",
            Self::DriveRoot => "transfer.driveRoot",
            Self::FolderMissing { .. } => "transfer.folderMissing",
            Self::DeviceUnreachable => "transfer.deviceUnreachable",
            Self::CopyToDeviceFailed { .. } => "transfer.copyToDeviceFailed",
            Self::CopyFromDeviceFailed { .. } => "transfer.copyFromDeviceFailed",
            Self::Io(_) => "transfer.ioError",
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Self::NoFolder | Self::DriveRoot | Self::DeviceUnreachable => vec![],
            Self::SameFolder { title } => vec![title.clone()],
            Self::NotEnoughSpace { needed, free } => vec![needed.clone(), free.clone()],
            Self::NoManifest | Self::InvalidManifest => vec![MANIFEST_NAME.to_string()],
            Self::NewerSchema { found, supported } => vec![found.to_string(), supported.to_string()],
            Self::AlreadyRegistered { title, format } => vec![title.clone(), format.clone()],
            Self::FolderMissing { folder_name } => vec![folder_name.clone()],
            Self::CopyToDeviceFailed { file_name } | Self::CopyFromDeviceFailed { file_name } => vec![file_name.clone()],
            Self::Io(message) => vec![message.clone()],
        }
    }
}

/// English, for the log. The interface uses `key` + `args` instead.
impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFolder => write!(f, "this archive has no folder"),
            Self::SameFolder { title } => write!(f, "\"{title}\" already lives in that folder"),
            Self::NotEnoughSpace { needed, free } => write!(f, "not enough space: {needed} needed, {free} free"),
            Self::NoManifest => write!(f, "no {MANIFEST_NAME} in that folder"),
            Self::InvalidManifest => write!(f, "that {MANIFEST_NAME} is damaged or not one of Soundify's"),
            Self::NewerSchema { found, supported } => {
                write!(f, "manifest schema {found} is newer than {supported}")
            }
            Self::AlreadyRegistered { title, format } => write!(f, "\"{title}\" ({format}) is already registered"),
            Self::DriveRoot => write!(f, "cannot import from the root of a drive"),
            Self::FolderMissing { folder_name } => write!(f, "folder \"{folder_name}\" is missing"),
            Self::DeviceUnreachable => write!(f, "the device is not reachable"),
            Self::CopyToDeviceFailed { file_name } => write!(f, "could not copy \"{file_name}\" to the device"),
            Self::CopyFromDeviceFailed { file_name } => write!(f, "could not copy \"{file_name}\" from the device"),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<std::io::Error> for TransferError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TransferError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub copied: usize,
    pub skipped: usize,
    pub missing: usize,
    pub cancelled: bool,
    /// Files copied in this run and deleted again because it was cancelled.
    pub undone: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub title: String,
    pub format: String,
    pub total: usize,
    pub downloaded: usize,
    pub pending: usize,
}

/// How one import inside a batch went (see [`import_all`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOutcome {
    pub folder: Location,
    pub result: Option<ImportResult>,
    pub error: Option<TransferError>,
    pub already_registered: bool,
}

impl ImportOutcome {
    pub fn ok(&self) -> bool {
        self.result.is_some()
    }
}

/// Where the archive and its .txt would live if a folder were imported.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveTarget {
    pub format_folder: Location,
    pub json: Location,
    pub txt: Location,
}

// === THE MANIFEST INSIDE THE PLAYLIST FOLDER ===

/// Is this folder an importable playlist, that is, does it carry a manifest?
///
/// A file with that name is not enough: it has to be a manifest (see
/// [`parse_manifest`]). One from a newer version does count, so the user is told
/// to update instead of the folder being passed over in silence, and so does one
/// that could not be read just now: that says nothing about what it contains.
pub fn has_manifest(storage: &dyn Storage, folder: &Location) -> bool {
    if !is_dir(storage, folder) {
        return false;
    }
    let Ok(file) = storage.child(folder, MANIFEST_NAME) else { return false };
    if !is_file(storage, &file) {
        return false;
    }
    storage.read(&file).map_or(true, |bytes| !matches!(parse_manifest(&bytes), Err(TransferError::InvalidManifest)))
}

/// Reads a manifest and says whether it really is one: JSON, with a schema, a
/// title and a format. Without this, `{}` or a file of another app with the same
/// name was imported as a playlist with no title or format. Both apps have always
/// written those three, so no old manifest is left out; the folder name is not
/// asked for, since Java leaves it empty for an archive with no folder.
pub fn parse_manifest(bytes: &[u8]) -> Result<PlaylistManifest> {
    let manifest: PlaylistManifest = serde_json::from_slice(bytes).map_err(|e| {
        debug!("Not a manifest: {e}");
        TransferError::InvalidManifest
    })?;
    if manifest.schema > SCHEMA {
        return Err(TransferError::NewerSchema { found: manifest.schema, supported: SCHEMA });
    }
    if manifest.schema < 1 || manifest.title.trim().is_empty() || manifest.format.trim().is_empty() {
        return Err(TransferError::InvalidManifest);
    }
    Ok(manifest)
}

pub fn write_manifest(storage: &dyn Storage, folder: &Location, manifest: &PlaylistManifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| TransferError::Io(e.to_string()))?;
    storage.write(&storage.child(folder, MANIFEST_NAME)?, &bytes)?;
    Ok(())
}

pub fn read_manifest(storage: &dyn Storage, folder: &Location) -> Result<PlaylistManifest> {
    let file = storage.child(folder, MANIFEST_NAME)?;
    if !storage.exists(&file)? {
        return Err(TransferError::NoManifest);
    }
    parse_manifest(&storage.read(&file)?)
}

/// Leaves (or refreshes) the manifest inside this local playlist's folder.
///
/// It never fails and interrupts nothing: the archive is still the source of
/// truth, so failing to write it (a read-only folder, a USB unplugged mid
/// download) cannot bring down a download that already finished well. At
/// worst that folder is not importable until the next sync.
///
/// It skips what is none of its business: the archives of loose downloads
/// (no folder of their own) and folders not reachable right now.
pub fn write_manifest_quietly(storage: &dyn Storage, json: &Location, exported_at: &str) {
    if !io::is_playlist_archive(json) {
        return;
    }
    let attempt = || -> Result<()> {
        let folder = source_folder(storage, json)?;
        // Careful, it is not created: if the folder is gone (a drive unplugged,
        // the user moved it), writing the manifest would recreate it empty and
        // the Gestor would say the playlist is fine when what is there is an
        // orphan folder with not a single file.
        if !is_dir(storage, &folder) {
            return Ok(());
        }
        let manifest = keep_what_was_known(storage, build_manifest(storage, json, exported_at)?, &folder);
        write_manifest(storage, &folder, &manifest)
    };
    if let Err(e) = attempt() {
        debug!("Could not write the manifest of {json}: {e}");
    }
}

/// Rewriting the manifest cannot leave it knowing LESS than it already knew.
///
/// It is built from scratch out of the archive, and the archive can know less:
/// "Crear Playlist" over a folder that already has the music records the ids
/// with no file, and the manifest was rewritten with zero names. The music was
/// still there, but the paper saying which file each id is got lost — and that
/// folder, taken to another computer, downloaded everything again (reported).
///
/// So every id the new manifest leaves nameless gets the previous manifest's
/// name back, ONLY if that file still exists: otherwise it would resurrect a
/// deleted file. Never the other way round: what the archive knows rules.
pub fn keep_what_was_known(storage: &dyn Storage, fresh: PlaylistManifest, folder: &Location) -> PlaylistManifest {
    if !has_manifest(storage, folder) {
        return fresh;
    }
    let previous = match read_manifest(storage, folder) {
        Ok(previous) => previous,
        Err(e) => {
            // Unreadable or from a newer version: there is nothing to keep.
            debug!("Could not read the previous manifest of {folder}: {e}");
            return fresh;
        }
    };
    let known: std::collections::HashMap<&str, &str> = previous
        .items
        .iter()
        .filter_map(|item| item.file_name.as_deref().map(|name| (item.id.as_str(), name)))
        .collect();
    if known.is_empty() {
        return fresh;
    }

    let mut merged = fresh;
    let mut recovered = 0;
    for item in merged.items.iter_mut().filter(|item| item.file_name.is_none()) {
        if let Some(&name) = known.get(item.id.as_str())
            && storage.child(folder, name).is_ok_and(|file| is_file(storage, &file))
        {
            item.file_name = Some(name.to_string());
            recovered += 1;
        }
    }
    if recovered > 0 {
        info!("Manifest of {}: kept {recovered} file name(s) the archive does not know yet.", folder.name());
    }
    merged
}

/// Fills in, from the folder's manifest, the file of the entries that have
/// none. Returns how many were relinked.
///
/// The case: "Crear Playlist" over a folder that already has the music records
/// the ids with no file, and the app cannot match them on its own — it only
/// finds a file again by the `[id]` in its name or by number, and an
/// unnumbered playlist has neither. Matching by title is no good: yt-dlp
/// changes characters (`|` becomes `｜`) and two videos can share a title.
/// The manifest does not guess it, it says it.
///
/// This does not contradict "the archive rules": the entry has no file at all,
/// there is nothing to overwrite. The limits: only entries WITHOUT a file,
/// only if the file really exists, and only the id -> file link (plus the
/// number and title read from that name). No URL, no numbering mode, no new
/// entries.
pub fn relink_from_manifest(storage: &dyn Storage, data: &mut ArchiveData, folder: &Location) -> usize {
    if !has_manifest(storage, folder) {
        return 0;
    }
    let manifest = match read_manifest(storage, folder) {
        Ok(manifest) => manifest,
        Err(e) => {
            debug!("Could not read the manifest of {folder} to relink: {e}");
            return 0;
        }
    };

    let mut relinked = 0;
    for item in &manifest.items {
        let Some(name) = item.file_name.as_deref() else { continue };
        let Some(entry) = data.entries.get(&item.id) else { continue };
        if io::has_real_file(storage, data, entry) {
            continue;
        }
        let Ok(file) = storage.child(folder, name) else { continue };
        if !is_file(storage, &file) {
            continue;
        }
        let numbered = data.number_files;
        let entry = data.entries.get_mut(&item.id).expect("checked above");
        entry.file_path = Some(sync::stored(&file));
        // The real file name rules over the manifest's title: it is where the
        // title comes from in the rest of the app. The same parser the
        // reconciliation uses, which respects whether the playlist numbers.
        if let Some(parsed) = sync::parse_file_name(numbered, name, Some(&item.id))
            && !parsed.title.trim().is_empty()
        {
            entry.title = Some(parsed.title);
            if numbered && let Some(number) = parsed.number {
                entry.number = Some(number);
            }
        }
        // It had a file from the start: this is not a removal.
        entry.deleted_at = None;
        relinked += 1;
    }
    if relinked > 0 {
        info!("Relinked {relinked} file(s) of {} from its manifest: they were already downloaded.", folder.name());
    }
    relinked
}

// === COPYING TO ANOTHER FOLDER ===

/// The real folder where this archive's music lives.
pub fn source_folder(storage: &dyn Storage, json: &Location) -> Result<Location> {
    let data = io::load(storage, json)?;
    match data.folder.as_deref() {
        Some(folder) if !folder.trim().is_empty() => Ok(Location::parse(folder)),
        _ => Err(TransferError::NoFolder),
    }
}

/// A local playlist's manifest, built from its archive. The format comes from
/// the archive folder's name (`pl_Name_hash/mp3/data.json`), not guessed from
/// the files' extension: there are entries with no file at all.
pub fn build_manifest(storage: &dyn Storage, json: &Location, exported_at: &str) -> Result<PlaylistManifest> {
    let data = io::load(storage, json)?;
    let format = io::parent_of(json).map(|parent| parent.name()).unwrap_or_default();
    let folder_name = data.folder.as_deref().map(|f| Location::parse(f).name()).unwrap_or_default();
    let title = title_from_folder_name(&folder_name, &format);

    let items = data
        .entries
        .iter()
        .map(|(id, entry)| ManifestItem {
            id: id.clone(),
            number: entry.number.clone(),
            title: entry.title.clone(),
            // Only if the file really is there: otherwise it travels as
            // pending and is downloaded at the destination.
            file_name: file_name_if_real(storage, &data, entry),
            unavailable_at: entry.unavailable_at.clone(),
            unavailable_reason: entry.unavailable_reason.clone(),
            removed_from_playlist_at: entry.removed_from_playlist_at.clone(),
        })
        .collect();

    Ok(PlaylistManifest {
        schema: SCHEMA,
        exported_at: exported_at.to_string(),
        title,
        url: data.url.clone(),
        format,
        number_files: data.number_files,
        folder_name,
        items,
    })
}

fn file_name_if_real(storage: &dyn Storage, data: &ArchiveData, entry: &ArchiveEntry) -> Option<String> {
    if !io::has_real_file(storage, data, entry) {
        return None;
    }
    entry.file_path.as_deref().map(|path| Location::parse(path).name())
}

/// "Canciones (mp3)" + "mp3" -> "Canciones". The same as the Gestor does.
pub fn title_from_folder_name(folder_name: &str, format: &str) -> String {
    if folder_name.trim().is_empty() {
        return "playlist".into();
    }
    folder_name.strip_suffix(&format!(" ({format})")).unwrap_or(folder_name).to_string()
}

/// The ids that have a file: the only ones that have to be copied.
pub fn downloaded_items(manifest: &PlaylistManifest) -> Vec<&ManifestItem> {
    manifest.items.iter().filter(|item| item.file_name.is_some()).collect()
}

/// Already copied at the destination? Same name and same size: relaunching an
/// export that was cut short (a USB unplugged, a full disk) does not copy the
/// gigabytes that were already there all over again.
pub fn already_there(storage: &dyn Storage, source: &Location, target: &Location) -> bool {
    match (storage.size(source), storage.size(target)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Are the two the same folder? Compared after resolving `.` and `..` and
/// without telling case apart (how paths behave on Windows), so it works even
/// when the destination does not exist yet. Two URIs only match verbatim.
pub fn is_same_folder(a: &Location, b: &Location) -> bool {
    match (a, b) {
        (Location::Path(a), Location::Path(b)) => normalized(a) == normalized(b),
        _ => a == b,
    }
}

fn normalized(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result.to_string_lossy().to_lowercase()
}

/// The bytes that really have to be copied (what is there does not count),
/// checking `cancelled` between files: with hundreds of files this scan alone
/// took a while, and pressing Cancel during it did nothing (reported). None if
/// cancelled partway. Deliberately reports no progress: progress means "one
/// file really done" for every caller.
pub fn bytes_to_copy(
    storage: &dyn Storage,
    items: &[&ManifestItem],
    source: &Location,
    target: &Location,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<u64>> {
    let mut total = 0;
    for item in items {
        if cancelled() {
            return Ok(None);
        }
        let name = item.file_name.as_deref().unwrap_or_default();
        let from = storage.child(source, name)?;
        if storage.exists(&from)? && !already_there(storage, &from, &storage.child(target, name)?) {
            total += storage.size(&from)?;
        }
    }
    Ok(Some(total))
}

/// Copies the playlist into `destination` (its own folder inside it) and
/// leaves the manifest there. The space is checked BEFORE starting: being left
/// halfway with several GB copied and an error is far worse than not starting.
///
/// `progress(done, total, file name)` runs once per file really processed;
/// `cancelled` is checked between one file and the next.
pub fn export(
    storage: &dyn Storage,
    json: &Location,
    destination: &Location,
    progress: &mut dyn FnMut(usize, usize, &str),
    cancelled: &dyn Fn() -> bool,
    exported_at: &str,
) -> Result<ExportResult> {
    let manifest = build_manifest(storage, json, exported_at)?;
    let source = source_folder(storage, json)?;
    let target = storage.child(destination, &manifest.folder_name)?;

    // Copying a playlist over itself would make every copy have the same source
    // and destination file. It is checked against the folder about to be
    // CREATED, not the one chosen: choosing the base folder (the one holding
    // "Canciones (mp3)") passed the screen's filter and ended up right there
    // (reported).
    if is_same_folder(&source, &target) {
        return Err(TransferError::SameFolder { title: manifest.title });
    }
    storage.create_dir_all(&target)?;

    let to_copy = downloaded_items(&manifest);
    let Some(needed) = bytes_to_copy(storage, &to_copy, &source, &target, cancelled)? else {
        // Cancelled during the scan: nothing was copied yet, nothing to undo.
        remove_if_empty(storage, &target);
        return Ok(ExportResult { copied: 0, skipped: 0, missing: 0, cancelled: true, undone: 0 });
    };
    if let Some(free) = storage.available_space(&target)?
        && needed > free
    {
        return Err(TransferError::NotEnoughSpace { needed: readable_size(needed), free: readable_size(free) });
    }

    let (mut copied, mut skipped, mut missing) = (0, 0, 0);
    let mut stopped = false;
    // What THIS run copied, to undo it on cancel. What was already there is not
    // ours to delete.
    let mut copied_now = Vec::new();
    for (i, item) in to_copy.iter().enumerate() {
        if cancelled() {
            stopped = true;
            break;
        }
        let name = item.file_name.as_deref().unwrap_or_default();
        let from = storage.child(&source, name)?;
        let to = storage.child(&target, name)?;
        if !storage.exists(&from)? {
            missing += 1;
        } else if already_there(storage, &from, &to) {
            skipped += 1;
        } else {
            storage.copy(&from, &to)?;
            copied += 1;
            copied_now.push(to);
        }
        progress(i + 1, to_copy.len(), name);
    }

    // Cancelling is "I do not want this", not "pause": what this run copied is
    // undone and the manifest is NOT written, so the destination is left as it
    // was. Files from an earlier export stay.
    if stopped {
        let undone = undo_copies(storage, &copied_now, &target);
        info!("Export cancelled: rolled back {undone} of {} file(s) copied in this run.", copied_now.len());
        return Ok(ExportResult { copied: 0, skipped, missing, cancelled: true, undone });
    }

    // The manifest goes INSIDE the playlist's folder, not loose at the
    // destination: the destination is usually the user's own folder, and its
    // fixed name meant a SECOND export there overwrote the first one's manifest
    // and left it unable to be imported (reported).
    write_manifest(storage, &target, &manifest)?;
    info!("Playlist exported to {destination}: {copied} copied, {skipped} already there, {missing} with no file.");
    Ok(ExportResult { copied, skipped, missing, cancelled: false, undone: 0 })
}

/// Deletes what this run copied and, if that leaves the folder empty, the
/// folder too. Returns how many were really deleted (one in use by another
/// program may resist; no reason to fail anything).
pub(crate) fn undo_copies(storage: &dyn Storage, copied_now: &[Location], target: &Location) -> usize {
    let mut undone = 0;
    for file in copied_now {
        if !storage.exists(file).unwrap_or(false) || storage.remove_file(file).is_ok() {
            undone += 1;
        } else {
            warn!("Could not roll back the copy of {file}");
        }
    }
    remove_if_empty(storage, target);
    undone
}

pub(crate) fn remove_if_empty(storage: &dyn Storage, folder: &Location) {
    if storage.list(folder).is_ok_and(|children| children.is_empty())
        && let Err(e) = storage.remove_dir_all(folder)
    {
        warn!("Could not delete the empty folder {folder}: {e}");
    }
}

// === IMPORTING ===

/// The importable playlist folders in sight from `root`: the folder itself if
/// it carries a manifest, or else the ones that do down to two levels below.
/// Pointing at `Music` recovers its eight playlists in one go. A folder with a
/// manifest is not followed inwards: inside a playlist there is only music.
pub fn find_importable(storage: &dyn Storage, root: &Location) -> Vec<Location> {
    if !is_dir(storage, root) {
        return Vec::new();
    }
    if has_manifest(storage, root) {
        return vec![root.clone()];
    }
    let mut found = Vec::new();
    scan_for_manifests(storage, root, 1, &mut found, &mut 0);
    found.sort_by_key(|folder| folder.name().to_lowercase());
    found
}

fn scan_for_manifests(storage: &dyn Storage, folder: &Location, depth: usize, found: &mut Vec<Location>, visited: &mut usize) {
    if depth > MAX_SCAN_DEPTH || *visited >= MAX_SCAN_DIRS {
        return;
    }
    // No permissions, or it stopped being reachable.
    let Ok(children) = storage.list(folder) else { return };
    for child in children.into_iter().filter(|c| c.is_dir) {
        if *visited >= MAX_SCAN_DIRS {
            return;
        }
        *visited += 1;
        if has_manifest(storage, &child.location) {
            found.push(child.location);
            continue;
        }
        scan_for_manifests(storage, &child.location, depth + 1, found, visited);
    }
}

/// Imports several folders in one batch. One that fails does not stop the
/// rest: when recovering a whole folder, one being registered already is the
/// normal thing.
pub fn import_all(storage: &dyn Storage, archives: &Location, folders: &[Location], now: &str) -> Vec<ImportOutcome> {
    folders
        .iter()
        .map(|folder| match import_from(storage, archives, folder, now) {
            Ok(result) => ImportOutcome { folder: folder.clone(), result: Some(result), error: None, already_registered: false },
            Err(error) => {
                let already_registered = matches!(error, TransferError::AlreadyRegistered { .. });
                if !already_registered {
                    warn!("Could not import {folder}: {error}");
                }
                ImportOutcome { folder: folder.clone(), result: None, error: Some(error), already_registered }
            }
        })
        .collect()
}

/// Registers the playlist in this installation. It copies not one file: the
/// music is already where it has to be, and that folder becomes the playlist's.
///
/// If that playlist + folder + format is already registered, NOTHING is merged:
/// merging two records behind the user's back can give them a fright, and the
/// numbering mode is immutable — two that disagree cannot be reconciled.
pub fn import_from(storage: &dyn Storage, archives: &Location, folder: &Location, now: &str) -> Result<ImportResult> {
    let manifest = read_manifest(storage, folder)?;
    let target = resolve_target(storage, archives, folder, &manifest)?;
    if storage.exists(&target.json)? {
        return Err(TransferError::AlreadyRegistered { title: manifest.title, format: manifest.format });
    }

    let data = to_archive_data(storage, &manifest, &target.format_folder, now)?;
    // save() and export_work_file() create the archive's folder themselves,
    // which is what lets resolve_target() create nothing.
    io::save(storage, &target.json, &data)?;
    io::export_work_file(storage, &target.txt, &data)?;

    let downloaded = count_downloaded(storage, &data);
    let total = manifest.items.len();
    info!("Playlist imported: {} ({}): {total} ids, {downloaded} with a file.", manifest.title, manifest.format);
    Ok(ImportResult {
        pending: pending_count(&manifest, downloaded),
        title: manifest.title,
        format: manifest.format,
        total,
        downloaded,
    })
}

/// "This folder with a manifest" -> "this archive of this installation". One
/// place for [`import_from`] and [`is_already_registered`]: worked out twice,
/// the check could end up looking at another file than the one written.
/// It creates nothing: it is also called just to ask.
pub fn resolve_target(
    storage: &dyn Storage,
    archives: &Location,
    folder: &Location,
    manifest: &PlaylistManifest,
) -> Result<ArchiveTarget> {
    let format_folder = resolve_format_folder(storage, folder, manifest)?;
    let base = parent(&format_folder).ok_or(TransferError::DriveRoot)?;
    let dir = paths::playlist_archive_dir(storage, archives, &manifest.title, &base, &manifest.format)?;
    Ok(ArchiveTarget { json: storage.child(&dir, DATA_JSON)?, txt: storage.child(&dir, ARCHIVE_TXT)?, format_folder })
}

/// Where the music of a folder carrying a manifest is: normally that same
/// folder, but one level above (next to the playlist's folder) is accepted too,
/// so exports made before the manifest moved inside still work.
fn resolve_format_folder(storage: &dyn Storage, chosen: &Location, manifest: &PlaylistManifest) -> Result<Location> {
    let inside = storage.child(chosen, &manifest.folder_name)?;
    if is_dir(storage, &inside) {
        return Ok(inside);
    }
    if chosen.name() == manifest.folder_name {
        return Ok(chosen.clone());
    }
    Err(TransferError::FolderMissing { folder_name: manifest.folder_name.clone() })
}

/// Is this folder already registered here? Used to take it off the list
/// BEFORE proposing anything (they showed up as "will be registered" and then
/// were not — reported). When in doubt it says NO, so the import goes ahead and
/// the user sees the real reason. [`import_from`] keeps its own check.
pub fn is_already_registered(storage: &dyn Storage, archives: &Location, folder: &Location) -> bool {
    read_manifest(storage, folder)
        .and_then(|manifest| resolve_target(storage, archives, folder, &manifest))
        .is_ok_and(|target| storage.exists(&target.json).unwrap_or(false))
}

pub fn not_yet_registered(storage: &dyn Storage, archives: &Location, folders: &[Location]) -> Vec<Location> {
    folders.iter().filter(|f| !is_already_registered(storage, archives, f)).cloned().collect()
}

/// The archive of a manifest, with the paths rebuilt against `format_folder`.
/// Kept apart from [`import_from`]: the conversion, without deciding where it
/// is stored. `now` is the archive's stamp format, never ISO: mixing both
/// left one field in two formats, and 'T' sorts ahead of a space.
pub fn to_archive_data(
    storage: &dyn Storage,
    manifest: &PlaylistManifest,
    format_folder: &Location,
    now: &str,
) -> Result<ArchiveData> {
    let mut data = ArchiveData {
        url: manifest.url.clone(),
        folder: Some(sync::stored(format_folder)),
        number_files: manifest.number_files,
        last_synced_at: Some(now.to_string()),
        ..Default::default()
    };
    let mut last_number: Option<String> = None;
    for item in &manifest.items {
        // The path is rebuilt HERE: the manifest only carries the name.
        let file_path = match item.file_name.as_deref() {
            Some(name) => Some(sync::stored(&storage.child(format_folder, name)?)),
            None => None,
        };
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
                file_path,
                ..Default::default()
            },
        );
    }
    data.last_number = last_number;
    Ok(data)
}

/// How many entries have a real file on disk RIGHT NOW.
pub fn count_downloaded(storage: &dyn Storage, data: &ArchiveData) -> usize {
    data.entries.values().filter(|e| io::has_real_file(storage, data, e)).count()
}

/// Pending after importing: neither downloaded nor marked unavailable (those
/// are never retried).
pub(crate) fn pending_count(manifest: &PlaylistManifest, downloaded: usize) -> usize {
    let unavailable = manifest.items.iter().filter(|i| i.file_name.is_none() && i.unavailable_at.is_some()).count();
    manifest.items.len().saturating_sub(downloaded + unavailable)
}

pub fn readable_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{} KB", (b / KB).round())
    } else if b < KB * KB * KB {
        format!("{} MB", (b / (KB * KB)).round())
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

// === HELPERS ===

fn is_dir(storage: &dyn Storage, location: &Location) -> bool {
    storage.list(location).is_ok()
}

pub(crate) fn is_file(storage: &dyn Storage, location: &Location) -> bool {
    storage.exists(location).unwrap_or(false) && !is_dir(storage, location)
}

fn parent(location: &Location) -> Option<Location> {
    match location {
        Location::Path(_) => io::parent_of(location),
        Location::Uri(uri) => uri.rsplit_once('/').map(|(parent, _)| Location::Uri(parent.to_string())),
    }
}

pub mod device;

#[cfg(test)]
mod tests;
