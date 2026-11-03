//! The library as the front draws it (`src/api/types.ts`): one playlist per
//! playlist + base folder, one block per format, one track per entry.
//!
//! Only a translation of [`crate::archive`]: the states come from
//! [`crate::archive::query::status_of`], so the Gestor, the player and
//! "Ver archivos" keep saying the same thing.

use chrono::NaiveDate;
use serde::Serialize;

use crate::archive::{ArchiveStore, EntryStatus, PlaylistFormatInfo};
use crate::clock::parse_day;
use crate::cover_art;
use crate::device::DeviceTarget;
use crate::storage::Storage;
use crate::ytdlp::unavailable;

/// Every format yt-dlp can produce, as the Java app's enums listed them. Phase
/// 4 filters them against what the installed yt-dlp really admits.
pub const AUDIO_FORMATS: [&str; 8] = ["MP3", "OPUS", "M4A", "FLAC", "AAC", "ALAC", "VORBIS", "WAV"];
pub const VIDEO_FORMATS: [&str; 6] = ["MP4", "MKV", "AVI", "FLV", "MOV", "WEBM"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub number: i32,
    pub title: String,
    pub added_days_ago: i64,
    /// Not stored in the archive: the player learns it from the file itself.
    pub seconds: i32,
    pub status: &'static str,
    /// Why it is unavailable, as an i18n key ([`crate::ytdlp::unavailable::reason_key`]).
    pub reason: Option<&'static str>,
    /// The file to play, when there is one.
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatBlock {
    /// The archive's `data.json`: unique, and what every later command takes.
    pub key: String,
    pub playlist_id: String,
    pub format: String,
    pub video: bool,
    pub numbered: bool,
    /// Every update syncs with the source playlist: the Java per-format switch is gone.
    pub sync: bool,
    pub last_sync: String,
    /// The video whose thumbnail is the default cover (the first in the
    /// playlist's order, without asking YouTube).
    pub first_video_id: Option<String>,
    /// Whether its folder (or phone) can be reached now: «Eliminar todo» is
    /// only offered then. A phone is optimistically reachable, "last known state".
    pub accessible: bool,
    /// Set when its files live on a phone: copying, opening and deleting go
    /// through the device instead of the disk.
    pub device: Option<DeviceTarget>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub folder: String,
    /// The picture the user picked as its cover, if any (otherwise its first
    /// format's first video's thumbnail is shown).
    pub cover: Option<String>,
    pub formats: Vec<FormatBlock>,
}

pub fn is_video_format(format: &str) -> bool {
    VIDEO_FORMATS.iter().any(|f| f.eq_ignore_ascii_case(format))
}

pub fn library(store: &ArchiveStore, today: NaiveDate) -> Vec<Playlist> {
    store
        .list_local_playlists()
        .into_iter()
        .map(|playlist| {
            let id = playlist.root_dir.to_string();
            let formats = playlist
                .formats
                .iter()
                .map(|info| FormatBlock {
                    key: info.json.to_string(),
                    playlist_id: id.clone(),
                    format: info.format.to_uppercase(),
                    video: is_video_format(&info.format),
                    numbered: info.number_files,
                    sync: true,
                    last_sync: info.last_synced_at.clone().unwrap_or_default(),
                    first_video_id: store.first_video_id(&info.json),
                    accessible: info.folder_accessible,
                    device: device_of(info),
                    tracks: tracks(store, &info.json, today),
                })
                .collect();
            let folder = playlist
                .formats
                .iter()
                .find_map(|f| f.folder.as_ref().map(ToString::to_string))
                .or(playlist.base_folder.clone())
                .unwrap_or_default();
            let cover = cover_art::custom_cover(store.storage(), &playlist.root_dir).map(|c| c.to_string());
            Playlist { id, title: playlist.title, folder, cover, formats }
        })
        .collect()
}

fn device_of(info: &PlaylistFormatInfo) -> Option<DeviceTarget> {
    Some(DeviceTarget {
        device_id: info.device_id.clone()?,
        device_name: info.device_name.clone().unwrap_or_default(),
        path: info.device_path.clone()?,
    })
}

fn tracks(store: &ArchiveStore, json: &crate::Location, today: NaiveDate) -> Vec<Track> {
    let Ok(data) = store.load(json) else { return Vec::new() };
    let storage = store.storage();
    store
        .list_entries(json)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .filter_map(|(index, view)| {
            let has_file = view.file_path.as_deref().is_some_and(|p| exists(storage, p));
            let reason = match &view.status {
                EntryStatus::Unavailable { reason } => Some(unavailable::reason_key(reason.as_deref())),
                _ => None,
            };
            let status = match view.status {
                EntryStatus::Ok => "ok",
                EntryStatus::Pending { .. } => "pending",
                EntryStatus::Failed { .. } => "failed",
                EntryStatus::Unavailable { .. } => "unavailable",
                // Out of the playlist: only worth showing while its file is still there.
                EntryStatus::Removed { .. } if has_file => "ok",
                EntryStatus::Removed { .. } => return None,
            };
            let added = data.entries.get(&view.id).and_then(|e| parse_day(e.added_at.as_deref()));
            Some(Track {
                number: view.number.as_deref().and_then(|n| n.trim().parse().ok()).unwrap_or(index as i32 + 1),
                title: view.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| view.id.clone()),
                added_days_ago: added.map(|day| (today - day).num_days().max(0)).unwrap_or(0),
                seconds: 0,
                status,
                reason,
                file: view.file_path.filter(|_| has_file),
                id: view.id,
            })
        })
        .collect()
}

fn exists(storage: &dyn Storage, stored: &str) -> bool {
    !stored.trim().is_empty() && storage.exists(&crate::Location::parse(stored)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::Location;
    use crate::clock;
    use crate::storage::MemoryStorage;

    #[test]
    fn translates_states_and_skips_what_left_the_playlist_without_a_file() {
        let storage = Arc::new(MemoryStorage::new());
        storage.create_dir_all(&Location::path("/music/P (mp3)")).unwrap();
        let store = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-02 10:00"));
        let json = Location::path("/archives/pl_P_000000/mp3/data.json");
        let folder = Location::path("/music/P (mp3)");
        let items: Vec<(String, String)> =
            [("aaa", "A"), ("bbb", "B"), ("ccc", "C")].iter().map(|(i, t)| (i.to_string(), t.to_string())).collect();
        store.register_playlist(&json, None, Some(&folder), &items, true).unwrap();
        storage.write(&Location::path("/music/P (mp3)/001 - A.mp3"), b"x").unwrap();
        store.commit(&json, &folder, "mp3", &[("aaa".into(), Location::path("/music/P (mp3)/001 - A.mp3"))]).unwrap();
        store.sync_playlist_membership(&json, None, &["aaa".to_string(), "bbb".to_string()].into()).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        let library = library(&store, today);
        assert_eq!(library.len(), 1);
        let block = &library[0].formats[0];
        assert_eq!(block.format, "MP3");
        assert!(!block.video && block.numbered);
        let states: Vec<(&str, &str)> = block.tracks.iter().map(|t| (t.id.as_str(), t.status)).collect();
        // Java's order: numbered by number, so what has none yet comes first.
        assert_eq!(states, [("bbb", "pending"), ("aaa", "ok")]);
        assert_eq!(block.tracks[1].added_days_ago, 3);
        assert_eq!(block.tracks[1].number, 1);
        assert_eq!(block.tracks[1].file.as_deref(), Some("/music/P (mp3)/001 - A.mp3"));
        assert_eq!(block.tracks[0].file, None);
        assert!(block.tracks.iter().all(|t| t.reason.is_none()));
        assert_eq!(library[0].cover, None);
    }

    #[test]
    fn a_playlist_carries_its_picked_cover_and_each_block_its_first_video() {
        let storage = Arc::new(MemoryStorage::new());
        let store = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-02 10:00"));
        let json = Location::path("/archives/pl_P_000000/mp3/data.json");
        let items: Vec<(String, String)> =
            [("aaaaaaaaaaa", "A"), ("bbbbbbbbbbb", "B")].iter().map(|(i, t)| (i.to_string(), t.to_string())).collect();
        store.register_playlist(&json, None, Some(&Location::path("/music/P (mp3)")), &items, false).unwrap();
        let order = [("aaaaaaaaaaa".to_string(), 2), ("bbbbbbbbbbb".to_string(), 1)].into();
        store.refresh_playlist_order(&json, &order).unwrap();
        storage.write(&Location::path("/archives/pl_P_000000/cover.jpg"), &[0xFF, 0xD8, 0xFF]).unwrap();

        let playlist = &library(&store, NaiveDate::from_ymd_opt(2026, 11, 2).unwrap())[0];
        let cover = playlist.cover.as_deref().map(Location::parse);
        assert_eq!(cover, Some(Location::path("/archives/pl_P_000000/cover.jpg")));
        assert_eq!(playlist.formats[0].first_video_id.as_deref(), Some("bbbbbbbbbbb"));
    }

    #[test]
    fn an_unavailable_track_carries_its_reason_as_an_i18n_key() {
        let storage = Arc::new(MemoryStorage::new());
        let store = ArchiveStore::new(storage, Location::path("/archives"), clock::fixed("2026-11-02 10:00"));
        let json = Location::path("/archives/pl_P_000000/mp3/data.json");
        let items = vec![("aaa".to_string(), "A".to_string())];
        store.register_playlist(&json, None, Some(&Location::path("/music/P (mp3)")), &items, true).unwrap();
        store.mark_unavailable(&json, "aaa", "ERROR: [youtube] aaa: Private video. Sign in if you've been granted access").unwrap();

        let track = &library(&store, NaiveDate::from_ymd_opt(2026, 11, 2).unwrap())[0].formats[0].tracks[0];
        assert_eq!(track.status, "unavailable");
        assert_eq!(track.reason, Some("unavailable.privateVideo"));
    }

    #[test]
    fn video_formats_are_told_apart() {
        assert!(is_video_format("mp4") && is_video_format("WEBM"));
        assert!(!is_video_format("mp3") && !is_video_format("m4a"));
    }
}
