//! The archives: one per playlist + base folder + format, in
//! `data/history/archives/pl_<Name>_<hash>/<format>/data.json` + `archive.txt`,
//! plus the flat `individual_archive_<format>.json` of the loose downloads.
//!
//! [`ArchiveStore`] is the facade; the work lives in [`io`] (disk), [`sync`]
//! (recording downloads, finding files again, numbering), [`query`] (reads for
//! the interface) and [`maintenance`] (startup reconciliation and deleting).
//!
//! Entries are never deleted: they only get dates stamped. The single source of
//! truth is the JSON; `archive.txt` is regenerated from it for yt-dlp.

pub mod io;
pub mod maintenance;
pub mod query;
pub mod sync;

use std::collections::{HashMap, HashSet};
use std::io::Result;
use std::sync::Arc;

use log::info;
use serde::Serialize;

pub use maintenance::DeleteResult;
pub use query::{EntryStatus, EntryView, LocalPlaylist, PlaylistFormatInfo, TrackFile};
pub use sync::TitleCollision;

use crate::Location;
use crate::clock::{self, Clock};
use crate::model::{ArchiveData, ArchiveEntry};
use crate::storage::Storage;
use crate::transfer;

/// Where the numbering mode about to be applied comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberModeSource {
    /// A really new local playlist: what the user asked for rules.
    Requested,
    /// This same format already exists: what it stored rules.
    SameFormat,
    /// The same playlist exists in another format: its mode is inherited.
    OtherFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberMode {
    pub number_files: bool,
    pub source: NumberModeSource,
}

#[derive(Clone)]
pub struct ArchiveStore {
    storage: Arc<dyn Storage>,
    archives: Location,
    clock: Clock,
}

impl ArchiveStore {
    // === CONSTRUCTION ===

    pub fn new(storage: Arc<dyn Storage>, archives: Location, clock: Clock) -> Self {
        Self { storage, archives, clock }
    }

    pub fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }

    pub fn archives(&self) -> &Location {
        &self.archives
    }

    pub fn load(&self, json: &Location) -> Result<ArchiveData> {
        io::load(self.storage(), json)
    }

    pub fn save(&self, json: &Location, data: &ArchiveData) -> Result<()> {
        io::save(self.storage(), json, data)
    }

    /// "Now" as the archives stamp it: what a copy or an import writes in the
    /// manifest goes by the same clock.
    pub fn now(&self) -> String {
        clock::stamp((self.clock)())
    }

    fn new_entry(&self) -> ArchiveEntry {
        ArchiveEntry { added_at: Some(self.now()), ..Default::default() }
    }

    fn txt_for(json: &Location) -> Result<Location> {
        io::txt_file_for(json).ok_or_else(|| std::io::Error::other(format!("no archive.txt for {json}")))
    }

    // === BEFORE AND AFTER A DOWNLOAD ===

    /// Before launching yt-dlp: reconciles the JSON with the disk, regenerates
    /// `archive.txt` and returns the last number used (`--autonumber-start`).
    pub fn prepare(&self, json: &Location, folder: &Location, format: &str) -> Result<i32> {
        let mut data = self.load(json)?;
        data.folder = Some(sync::stored(folder));
        sync::reconcile_entries(self.storage(), &mut data, folder, &crate::paths::format_extension(format), &self.now());
        data.last_synced_at = Some(self.now());
        self.save(json, &data)?;
        io::export_work_file(self.storage(), &Self::txt_for(json)?, &data)?;
        info!("Archive prepared: {} known ids.", data.entries.len());
        Ok(io::parse_int_safe(data.last_number.as_deref()))
    }

    /// After the download: records what yt-dlp reported (`captured`: id ->
    /// final file, from the `--print` hooks), then the general reconciliation
    /// (which also covers old-style files the hooks could not capture).
    /// Returns the title clashes worth a warning in Activity; nothing is
    /// renamed because of them.
    pub fn commit(
        &self,
        json: &Location,
        folder: &Location,
        format: &str,
        captured: &[(String, Location)],
    ) -> Result<Vec<TitleCollision>> {
        let mut data = self.load(json)?;
        data.folder = Some(sync::stored(folder));
        let now = self.now();
        for (id, file) in captured {
            sync::register_captured(self.storage(), &mut data, folder, id, file, &now);
        }
        sync::reconcile_entries(self.storage(), &mut data, folder, &crate::paths::format_extension(format), &now);
        data.last_synced_at = Some(now.clone());
        self.save(json, &data)?;
        // The manifest is refreshed here, with what has just been downloaded
        // already inside. It fails nothing: the download is fine whatever
        // happens to it.
        transfer::write_manifest_quietly(self.storage(), json, &now);
        info!("Archive updated after download: {} ids.", data.entries.len());
        Ok(sync::title_collisions(self.storage(), &data))
    }

    /// Persists ONE capture as soon as it happens: if the process dies, only
    /// the file that was halfway is lost, not the whole run.
    pub fn register_single(&self, json: &Location, folder: &Location, id: &str, file: &Location) -> Result<()> {
        let mut data = self.load(json)?;
        sync::register_captured(self.storage(), &mut data, folder, id, file, &self.now());
        self.save(json, &data)
    }

    // === WHAT NEEDS DOWNLOADING ===

    /// Of the ids in the playlist RIGHT NOW, the ones never seen or known but
    /// without a real file (and not unavailable). "New" and "missing" are one
    /// calculation: a normal download fetches whatever is missing, with no
    /// separate "recover" flow.
    pub fn ids_pending_download(&self, json: &Location, current: &[String]) -> Result<Vec<String>> {
        let data = self.load(json)?;
        Ok(current
            .iter()
            .filter(|id| match data.entries.get(*id) {
                None => true,
                Some(entry) => entry.unavailable_at.is_none() && !io::has_real_file(self.storage(), &data, entry),
            })
            .cloned()
            .collect())
    }

    /// The candidates worth checking one by one against YouTube: no title, no
    /// file and not yet unavailable. Once marked they stop being candidates.
    pub fn ids_needing_availability_check(&self, json: &Location, candidates: &[String]) -> Result<Vec<String>> {
        let data = self.load(json)?;
        Ok(candidates
            .iter()
            .filter(|id| {
                data.entries.get(*id).is_some_and(|e| {
                    e.unavailable_at.is_none()
                        && !io::has_real_file(self.storage(), &data, e)
                        && io::blank(e.title.as_deref())
                })
            })
            .cloned()
            .collect())
    }

    pub fn known_ids(&self, json: &Location) -> Result<Vec<String>> {
        Ok(self.load(json)?.entries.keys().cloned().collect())
    }

    // === SYNCING WITH THE PLAYLIST ===

    /// Registers the playlist items with no entry yet, born pending, so the
    /// Gestor shows them while they download (they used to appear all at once,
    /// already green, at the end — reported). Returns how many.
    pub fn register_new_playlist_items(&self, json: &Location, id_to_title: &[(String, String)]) -> Result<usize> {
        if id_to_title.is_empty() {
            return Ok(0);
        }
        let mut data = self.load(json)?;
        let added = self.add_missing_entries(&mut data, id_to_title);
        if added > 0 {
            // If the folder already has the file of any of these (another copy
            // of the app downloaded it over the same folder, say), it is linked
            // with what the manifest knows instead of being downloaded again.
            if let Some(folder) = data.folder.clone().filter(|f| !f.trim().is_empty()) {
                transfer::relink_from_manifest(self.storage(), &mut data, &Location::parse(&folder));
            }
            self.save(json, &data)?;
            info!("Registered {added} new playlist items as pending.");
        }
        Ok(added)
    }

    /// Marks `removed_from_playlist_at` against what the playlist holds now.
    /// Independent of the real download: stopping halfway must not stop
    /// removals being detected. Never deletes a file.
    pub fn sync_playlist_membership(&self, json: &Location, url: Option<&str>, current: &HashSet<String>) -> Result<()> {
        let mut data = self.load(json)?;
        if let Some(url) = url.filter(|u| !u.trim().is_empty()) {
            data.url = Some(url.to_string());
        }
        let now = self.now();
        sync::handle_playlist_removals(&mut data, current, &now);
        data.last_synced_at = Some(now.clone());
        self.save(json, &data)?;
        transfer::write_manifest_quietly(self.storage(), json, &now);
        info!("Playlist sync completed ({} ids seen).", current.len());
        Ok(())
    }

    /// Fills in titles of KNOWN entries still without a file (unavailable
    /// videos rarely get one of their own). Never creates entries, and never
    /// overwrites the title of a downloaded file (its name is more reliable).
    pub fn refresh_known_titles(&self, json: &Location, id_to_title: &HashMap<String, String>) -> Result<()> {
        if id_to_title.is_empty() {
            return Ok(());
        }
        let mut data = self.load(json)?;
        let mut changed = false;
        let storage = self.storage();
        let device_backed = data.is_device_backed();
        for (id, title) in id_to_title {
            if title.trim().is_empty() {
                continue;
            }
            let Some(entry) = data.entries.get(id) else { continue };
            let real = if device_backed {
                !io::blank(entry.device_file_name.as_deref()) && entry.deleted_at.is_none()
            } else {
                entry.file_path.as_deref().is_some_and(|p| storage.exists(&Location::parse(p)).unwrap_or(false))
            };
            if !real && entry.title.as_deref() != Some(title.as_str()) {
                data.entries[id].title = Some(title.clone());
                changed = true;
            }
        }
        if changed {
            self.save(json, &data)?;
        }
        Ok(())
    }

    /// The position of each KNOWN id in the source playlist, only to list an
    /// unnumbered playlist as YouTube does. Touches no file.
    pub fn refresh_playlist_order(&self, json: &Location, id_to_index: &HashMap<String, i32>) -> Result<()> {
        if id_to_index.is_empty() {
            return Ok(());
        }
        let mut data = self.load(json)?;
        let mut changed = false;
        for (id, &index) in id_to_index {
            if index <= 0 {
                continue; // "NA" in playlists with no index
            }
            if let Some(entry) = data.entries.get_mut(id)
                && entry.playlist_index != Some(index)
            {
                entry.playlist_index = Some(index);
                changed = true;
            }
        }
        if changed {
            self.save(json, &data)?;
        }
        Ok(())
    }

    /// "Crear Playlist": registers it WITHOUT downloading, one pending entry
    /// per item not known yet. Idempotent: known ids are untouched, and the
    /// numbering mode is only set when the archive is born (it lives in the
    /// real file names, so it never changes).
    pub fn register_playlist(
        &self,
        json: &Location,
        url: Option<&str>,
        folder: Option<&Location>,
        id_to_title: &[(String, String)],
        number_files_if_new: bool,
    ) -> Result<()> {
        // Checked BEFORE loading: a missing file loads as a fresh archive.
        let is_new = !self.storage.exists(json)?;
        let mut data = self.load(json)?;
        if let Some(url) = url.filter(|u| !u.trim().is_empty()) {
            data.url = Some(url.to_string());
        }
        if let Some(folder) = folder {
            data.folder = Some(sync::stored(folder));
        }
        if is_new {
            data.number_files = number_files_if_new;
        }
        self.add_missing_entries(&mut data, id_to_title);
        // Registering over a folder that already has the music: the entries are
        // born with no file, and the app only finds a file again by the "[id]"
        // in its name or by number, neither of which an unnumbered playlist
        // has, so "Sincronizar" downloaded everything again with the music
        // sitting right there (reported). The manifest only fills gaps.
        if let Some(folder) = folder {
            transfer::relink_from_manifest(self.storage(), &mut data, folder);
        }
        let now = self.now();
        data.last_synced_at = Some(now.clone());
        self.save(json, &data)?;
        io::export_work_file(self.storage(), &Self::txt_for(json)?, &data)?;
        // The playlist is born with its manifest inside: from minute one that
        // folder can be taken to another installation, even with no music yet.
        transfer::write_manifest_quietly(self.storage(), json, &now);
        info!("Playlist registered without downloading ({} known ids): {json}", id_to_title.len());
        Ok(())
    }

    fn add_missing_entries(&self, data: &mut ArchiveData, id_to_title: &[(String, String)]) -> usize {
        let mut added = 0;
        for (id, title) in id_to_title {
            if id.trim().is_empty() || data.entries.contains_key(id) {
                continue;
            }
            let mut entry = self.new_entry();
            if !title.trim().is_empty() {
                entry.title = Some(title.clone());
            }
            data.entries.insert(id.clone(), entry);
            added += 1;
        }
        added
    }

    /// Stamped when a run TRULY finishes (the Gestor sorts by it), unlike
    /// `last_synced_at`, stamped at the start.
    pub fn touch_last_activity(&self, json: &Location) -> Result<()> {
        let mut data = self.load(json)?;
        data.last_activity_at = Some(self.now());
        self.save(json, &data)
    }

    // === NUMBERING (per local playlist, immutable) ===

    /// Unreadable means unnumbered: numbering is never invented over an unknown state.
    pub fn is_numbered(&self, json: &Location) -> bool {
        self.load(json).is_ok_and(|d| d.number_files)
    }

    /// This format's own mode if it exists; else the mode of the same playlist
    /// in another format (one playlist must not end up numbered in MP3 and
    /// unnumbered in MP4 because of a checkbox); else what was asked for. The
    /// source is returned so the interface can say why the checkbox was ignored.
    pub fn resolve_number_mode(&self, json: &Location, requested: bool) -> NumberMode {
        if self.storage.exists(json).unwrap_or(false) {
            return NumberMode { number_files: self.is_numbered(json), source: NumberModeSource::SameFormat };
        }
        let root = io::parent_of(json).and_then(|fmt_dir| io::parent_of(&fmt_dir));
        for sibling in root.and_then(|r| self.storage.list(&r).ok()).unwrap_or_default() {
            if !sibling.is_dir {
                continue;
            }
            if let Ok(sibling_json) = self.storage.child(&sibling.location, io::DATA_JSON)
                && self.storage.exists(&sibling_json).unwrap_or(false)
            {
                return NumberMode { number_files: self.is_numbered(&sibling_json), source: NumberModeSource::OtherFormat };
            }
        }
        NumberMode { number_files: requested, source: NumberModeSource::Requested }
    }

    /// The number already stored for that id (from an earlier attempt).
    pub fn saved_number(&self, json: &Location, id: &str) -> Option<String> {
        self.load(json).ok()?.entries.get(id)?.number.clone()
    }

    /// None when the stored title is literally the id: that is the trace of an
    /// earlier failure, and reusing it would perpetuate it.
    pub fn saved_title(&self, json: &Location, id: &str) -> Option<String> {
        let title = self.load(json).ok()?.entries.get(id)?.title.clone()?;
        (!title.trim().is_empty() && title != id).then_some(title)
    }

    /// lastNumber + 1, for an id that never had a number of its own.
    pub fn next_free_number(&self, json: &Location) -> Option<String> {
        let data = self.load(json).ok()?;
        Some(format!("{:03}", io::parse_int_safe(data.last_number.as_deref()) + 1))
    }

    // === UNAVAILABLE AND FAILED ===

    /// Permanently unavailable (deleted, private, copyright...): it stops being
    /// proposed as pending. Not for network or client failures — see [`Self::mark_failed`].
    pub fn mark_unavailable(&self, json: &Location, id: &str, reason: &str) -> Result<()> {
        let mut data = self.load(json)?;
        let now = self.now();
        let entry = data.entries.entry(id.to_string()).or_insert_with(|| self.new_entry());
        entry.unavailable_at = Some(now);
        entry.unavailable_reason = Some(reason.to_string());
        self.save(json, &data)?;
        info!("Marked as unavailable on YouTube ({id}): {reason}");
        Ok(())
    }

    /// No client could download it in this run. Not permanent: the next
    /// update tries again on its own.
    pub fn mark_failed(&self, json: &Location, id: &str, title: Option<&str>, reason: &str) -> Result<()> {
        let mut data = self.load(json)?;
        let now = self.now();
        let entry = data.entries.entry(id.to_string()).or_insert_with(|| self.new_entry());
        if let Some(title) = title.filter(|t| !t.trim().is_empty())
            && io::blank(entry.title.as_deref())
        {
            entry.title = Some(title.to_string());
        }
        entry.last_failed_at = Some(now);
        entry.last_failed_reason = Some(reason.to_string());
        self.save(json, &data)?;
        info!("Marked as failed in this run ({id}): {reason}");
        Ok(())
    }

    pub fn is_unavailable(&self, json: &Location, id: &str) -> bool {
        self.load(json).ok().and_then(|d| d.entries.get(id).map(|e| e.unavailable_at.is_some())).unwrap_or(false)
    }

    // === READS AND MAINTENANCE ===

    pub fn list_local_playlists(&self) -> Vec<LocalPlaylist> {
        query::list_local_playlists(self.storage(), &self.archives)
    }

    pub fn list_entries(&self, json: &Location) -> Result<Vec<EntryView>> {
        Ok(query::list_entries(self.storage(), &self.load(json)?))
    }

    pub fn list_tracks(&self, json: &Location) -> Result<Vec<TrackFile>> {
        Ok(query::list_tracks(self.storage(), &self.load(json)?))
    }

    pub fn first_video_id(&self, json: &Location) -> Option<String> {
        query::first_video_id(&self.load(json).ok()?)
    }

    /// Once at startup. Returns the archives reconciled.
    pub fn reconcile_all(&self) -> Vec<Location> {
        maintenance::reconcile_all(self.storage(), &self.archives, &self.now())
    }

    pub fn delete_all_downloaded_files(&self, json: &Location) -> DeleteResult {
        maintenance::delete_all_downloaded_files(self.storage(), json)
    }

    /// Never deletes music files.
    pub fn forget_playlist_format(&self, json: &Location) {
        maintenance::forget_playlist_format(self.storage(), json)
    }
}

#[cfg(test)]
mod tests;
