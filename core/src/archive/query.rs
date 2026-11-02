//! The reads feeding the interface: the Gestor's library of local playlists,
//! the file-by-file listing of "Ver archivos" and the player's tracks. Writes
//! nothing.
//!
//! "Downloaded" is always [`has_real_file`], the same criterion the pending
//! calculation uses, so the three views cannot contradict each other (which is
//! exactly what used to happen in the Java app).

use chrono::NaiveDate;
use log::warn;
use serde::Serialize;

use super::io::{self, DATA_JSON, has_real_file, parse_int_safe};
use crate::Location;
use crate::clock::parse_day;
use crate::model::{ArchiveData, ArchiveEntry};
use crate::storage::Storage;

/// One format of a local playlist, as the Gestor shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistFormatInfo {
    pub format: String,
    pub json: Location,
    pub folder: Option<Location>,
    /// Device-backed rows are optimistically reachable: "last known state".
    pub folder_accessible: bool,
    pub url: Option<String>,
    pub file_count: usize,
    pub downloaded_count: usize,
    pub pending_count: usize,
    pub unavailable_count: usize,
    pub failed_count: usize,
    pub last_synced_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub number_files: bool,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub device_path: Option<String>,
}

/// One playlist + base folder, with its formats grouped inside.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaylist {
    pub title: String,
    pub root_dir: Location,
    pub base_folder: Option<String>,
    pub formats: Vec<PlaylistFormatInfo>,
}

/// Where an entry stands. Checked in this order everywhere, so the Gestor and
/// "Ver archivos" can never label the same thing two ways (they once did: one
/// said "pending" where the other said "failed").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EntryStatus {
    Unavailable { reason: Option<String> },
    /// No longer in the source playlist: never downloaded again, so NOT pending.
    Removed { at: String },
    Failed { reason: Option<String>, at: String },
    /// `deleted_at` set: it was there once. None: never downloaded yet.
    Pending { deleted_at: Option<String> },
    Ok,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub id: String,
    pub number: Option<String>,
    pub title: Option<String>,
    pub status: EntryStatus,
    pub file_path: Option<String>,
}

/// One playable file of a playlist.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackFile {
    pub id: String,
    pub number: Option<String>,
    pub title: Option<String>,
    pub file: Location,
    pub added_on: Option<NaiveDate>,
}

/// Every known local playlist, from what each archive already holds: never
/// asks YouTube (that only happens on "Actualizar").
pub fn list_local_playlists(storage: &dyn Storage, archives: &Location) -> Vec<LocalPlaylist> {
    let Ok(children) = storage.list(archives) else { return Vec::new() };
    let mut result = Vec::new();
    for pl_dir in children.into_iter().filter(|e| e.is_dir && e.name.starts_with("pl_")) {
        let mut formats = Vec::new();
        let mut title = None;
        let mut base_folder = None;
        for fmt_dir in storage.list(&pl_dir.location).unwrap_or_default().into_iter().filter(|e| e.is_dir) {
            let Ok(json) = storage.child(&fmt_dir.location, DATA_JSON) else { continue };
            if !storage.exists(&json).unwrap_or(false) {
                continue;
            }
            let data = match io::load(storage, &json) {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to read local playlist archive {json}: {e}");
                    continue;
                }
            };
            let folder = data.folder.as_deref().filter(|f| !f.trim().is_empty()).map(Location::parse);
            let accessible =
                data.is_device_backed() || folder.as_ref().is_some_and(|f| storage.exists(f).unwrap_or(false));
            let counts = count(storage, &data);

            // A local playlist's title comes from its folder name; a
            // device-backed one has no folder and carries its own.
            if title.is_none() {
                if data.is_device_backed() {
                    title = data.title.clone();
                } else if let Some(folder) = &folder {
                    let suffix = format!(" ({})", fmt_dir.name);
                    let name = folder.name();
                    title = Some(name.strip_suffix(&suffix).unwrap_or(&name).to_string());
                }
            }
            if base_folder.is_none() {
                if data.is_device_backed() {
                    base_folder = Some(format!("📱 {}", data.device_name.clone().unwrap_or_default()));
                } else if let Some(parent) = folder.as_ref().and_then(parent_text) {
                    base_folder = Some(parent);
                }
            }
            formats.push(PlaylistFormatInfo {
                format: fmt_dir.name,
                json,
                folder,
                folder_accessible: accessible,
                url: data.url.clone(),
                file_count: data.entries.len(),
                downloaded_count: counts.downloaded,
                pending_count: counts.pending,
                unavailable_count: counts.unavailable,
                failed_count: counts.failed,
                last_synced_at: data.last_synced_at.clone(),
                last_activity_at: data.last_activity_at.clone(),
                number_files: data.number_files,
                device_id: data.device_id.clone(),
                device_name: data.device_name.clone(),
                device_path: data.device_path.clone(),
            });
        }
        if !formats.is_empty() {
            result.push(LocalPlaylist {
                title: title.unwrap_or_else(|| pl_dir.name.clone()),
                root_dir: pl_dir.location,
                base_folder,
                formats,
            });
        }
    }
    result
}

#[derive(Default)]
struct Counts {
    downloaded: usize,
    pending: usize,
    unavailable: usize,
    failed: usize,
}

fn count(storage: &dyn Storage, data: &ArchiveData) -> Counts {
    let mut counts = Counts::default();
    for entry in data.entries.values() {
        // REALLY downloaded: cannot be worked out by subtracting, because what
        // left the playlist falls into no group and may or may not have a file.
        let real = has_real_file(storage, data, entry);
        if real {
            counts.downloaded += 1;
        }
        match status_of(storage, data, entry) {
            EntryStatus::Unavailable { .. } => counts.unavailable += 1,
            EntryStatus::Failed { .. } => counts.failed += 1,
            EntryStatus::Pending { .. } => counts.pending += 1,
            EntryStatus::Removed { .. } | EntryStatus::Ok => {}
        }
    }
    counts
}

/// "OK" demands a real file RIGHT NOW, not just "no negative flag": an entry
/// created only to hold its title used to show as OK by elimination.
pub fn status_of(storage: &dyn Storage, data: &ArchiveData, entry: &ArchiveEntry) -> EntryStatus {
    if entry.unavailable_at.is_some() {
        EntryStatus::Unavailable { reason: entry.unavailable_reason.clone() }
    } else if let Some(at) = &entry.removed_from_playlist_at {
        // Checked BEFORE the file: these usually have none, and that does NOT
        // make them pending (they came up as pending forever — reported).
        EntryStatus::Removed { at: at.clone() }
    } else if let Some(at) = &entry.last_failed_at {
        EntryStatus::Failed { reason: entry.last_failed_reason.clone(), at: at.clone() }
    } else if !has_real_file(storage, data, entry) {
        EntryStatus::Pending { deleted_at: entry.deleted_at.clone() }
    } else {
        EntryStatus::Ok
    }
}

/// One row per entry for "Ver archivos", in display order.
pub fn list_entries(storage: &dyn Storage, data: &ArchiveData) -> Vec<EntryView> {
    entries_in_display_order(data)
        .into_iter()
        .map(|(id, entry)| EntryView {
            id: id.clone(),
            number: entry.number.clone(),
            title: entry.title.clone(),
            status: status_of(storage, data, entry),
            file_path: entry.file_path.clone(),
        })
        .collect()
}

/// What can be played right now: the files on disk, in "Ver archivos" order.
/// A device-backed playlist has no local files at all.
pub fn list_tracks(storage: &dyn Storage, data: &ArchiveData) -> Vec<TrackFile> {
    if data.is_device_backed() {
        return Vec::new();
    }
    entries_in_display_order(data)
        .into_iter()
        .filter(|(_, e)| e.unavailable_at.is_none() && e.removed_from_playlist_at.is_none())
        .filter(|(_, e)| has_real_file(storage, data, e))
        .map(|(id, entry)| TrackFile {
            id: id.clone(),
            number: entry.number.clone(),
            title: entry.title.clone(),
            file: Location::parse(entry.file_path.as_deref().unwrap_or_default()),
            added_on: parse_day(entry.added_at.as_deref()),
        })
        .collect()
}

/// The video at the top of the source playlist (its default cover), from the
/// stored positions; without any, the first one registered.
pub fn first_video_id(data: &ArchiveData) -> Option<String> {
    data.entries
        .iter()
        .filter(|(_, e)| e.unavailable_at.is_none() && e.removed_from_playlist_at.is_none())
        .min_by_key(|(_, e)| e.playlist_index.unwrap_or(i32::MAX))
        .map(|(id, _)| id.clone())
}

/// Numbered: the number rules, as in the folder. Unnumbered: the order of the
/// source playlist (`playlist_index`); without one, at the end in archive
/// order (the sort is stable). Whatever was added later used to pile up at the
/// end even when it was at the top on YouTube (reported).
fn entries_in_display_order(data: &ArchiveData) -> Vec<(&String, &ArchiveEntry)> {
    let mut ordered: Vec<_> = data.entries.iter().collect();
    if data.number_files {
        ordered.sort_by(|(a_id, a), (b_id, b)| {
            parse_int_safe(a.number.as_deref()).cmp(&parse_int_safe(b.number.as_deref())).then_with(|| a_id.cmp(b_id))
        });
    } else {
        ordered.sort_by_key(|(_, e)| e.playlist_index.unwrap_or(i32::MAX));
    }
    ordered
}

fn parent_text(folder: &Location) -> Option<String> {
    folder.as_path()?.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn data_with(entries: &[(&str, ArchiveEntry)]) -> ArchiveData {
        let mut data = ArchiveData::default();
        for (id, entry) in entries {
            data.entries.insert((*id).into(), entry.clone());
        }
        data
    }

    fn indexed(index: Option<i32>) -> ArchiveEntry {
        ArchiveEntry { playlist_index: index, ..Default::default() }
    }

    #[test]
    fn parse_day_reads_only_its_own_format() {
        assert_eq!(parse_day(Some("2026-10-19 08:30")), NaiveDate::from_ymd_opt(2026, 10, 19));
        assert_eq!(parse_day(Some("19/10/2026")), None);
        assert_eq!(parse_day(Some("   ")), None);
    }

    #[test]
    fn the_first_video_is_the_one_on_top_of_the_playlist() {
        let data = data_with(&[("b", indexed(Some(2))), ("a", indexed(Some(1))), ("c", indexed(None))]);
        assert_eq!(first_video_id(&data).as_deref(), Some("a"));
    }

    #[test]
    fn without_positions_the_first_registered_is_the_cover() {
        let data = data_with(&[("x", indexed(None)), ("y", indexed(None))]);
        assert_eq!(first_video_id(&data).as_deref(), Some("x"));
    }

    #[test]
    fn what_left_the_playlist_is_never_the_cover() {
        let removed = ArchiveEntry { removed_from_playlist_at: Some("2026-01-01 00:00".into()), playlist_index: Some(1), ..Default::default() };
        let data = data_with(&[("gone", removed), ("kept", indexed(Some(5)))]);
        assert_eq!(first_video_id(&data).as_deref(), Some("kept"));
        assert_eq!(first_video_id(&ArchiveData::default()), None);
    }

    #[test]
    fn regression_removed_is_not_pending_even_without_its_file() {
        let removed = ArchiveEntry { removed_from_playlist_at: Some("2026-01-01 00:00".into()), ..Default::default() };
        let data = data_with(&[("a", removed)]);
        let storage = MemoryStorage::new();
        assert_eq!(status_of(&storage, &data, &data.entries["a"]), EntryStatus::Removed { at: "2026-01-01 00:00".into() });
        assert_eq!(count(&storage, &data).pending, 0);
    }

    #[test]
    fn regression_ok_needs_a_real_file() {
        let titled_only = ArchiveEntry { title: Some("Song".into()), ..Default::default() };
        let data = data_with(&[("a", titled_only)]);
        assert_eq!(status_of(&MemoryStorage::new(), &data, &data.entries["a"]), EntryStatus::Pending { deleted_at: None });
    }

    #[test]
    fn unnumbered_lists_follow_the_playlist_and_unindexed_go_last() {
        let data = data_with(&[("late", indexed(None)), ("second", indexed(Some(2))), ("first", indexed(Some(1)))]);
        let ids: Vec<String> = list_entries(&MemoryStorage::new(), &data).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["first", "second", "late"]);
    }

    #[test]
    fn numbered_lists_follow_the_number() {
        let mut data = data_with(&[
            ("b", ArchiveEntry { number: Some("010".into()), playlist_index: Some(1), ..Default::default() }),
            ("a", ArchiveEntry { number: Some("002".into()), playlist_index: Some(2), ..Default::default() }),
        ]);
        data.number_files = true;
        let ids: Vec<String> = list_entries(&MemoryStorage::new(), &data).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn the_gestor_names_a_playlist_after_its_folder() {
        let storage = MemoryStorage::new();
        let archives = Location::path("/archives");
        let json = Location::path("/archives/pl_Chill_abcdef/mp3/data.json");
        storage.create_dir_all(&Location::path("/music/Chill (mp3)")).unwrap();
        storage.write(&Location::path("/music/Chill (mp3)/a.mp3"), b"x").unwrap();
        let data = data_with(&[
            ("a", ArchiveEntry { file_path: Some("/music/Chill (mp3)/a.mp3".into()), ..Default::default() }),
            ("b", ArchiveEntry::default()),
        ]);
        let data = ArchiveData { folder: Some("/music/Chill (mp3)".into()), ..data };
        io::save(&storage, &json, &data).unwrap();

        let playlists = list_local_playlists(&storage, &archives);
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].title, "Chill");
        let mp3 = &playlists[0].formats[0];
        assert!(mp3.folder_accessible);
        assert_eq!((mp3.file_count, mp3.downloaded_count, mp3.pending_count), (2, 1, 1));
    }
}
