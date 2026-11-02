//! The archives on disk: reading and writing `data.json`, dumping
//! `archive.txt` for yt-dlp, finding every archive, and the predicates the
//! rest shares ("is it really downloaded?").
//!
//! An archive always lives in the app's own folder (`data/history/archives`),
//! which is a plain path even on Android, so archive locations are taken
//! apart with `std::path`. The music they point at may be a URI.

use std::io;
use std::path::Path;

use crate::Location;
use crate::model::{ArchiveData, ArchiveEntry};
use crate::storage::Storage;

pub const DATA_JSON: &str = "data.json";
pub const ARCHIVE_TXT: &str = "archive.txt";
/// The playlist's passport, kept inside its music folder (see the transfer phase).
pub const MANIFEST_NAME: &str = "soundify-playlist.json";

/// A missing file is an empty archive (a playlist about to be registered).
pub fn load(storage: &dyn Storage, json: &Location) -> io::Result<ArchiveData> {
    if !storage.exists(json)? {
        return Ok(ArchiveData::default());
    }
    let bytes = storage.read(json)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{json}: {e}")))
}

pub fn save(storage: &dyn Storage, json: &Location, data: &ArchiveData) -> io::Result<()> {
    if let Some(parent) = parent_of(json) {
        storage.create_dir_all(&parent)?;
    }
    let bytes = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
    storage.write(json, &bytes)
}

/// The `archive.txt` yt-dlp reads with `--download-archive`.
///
/// Deliberately ONLY the ids yt-dlp must skip: the ones with a real file right
/// now, and the ones marked unavailable (so they are never retried). A missing
/// file NOT marked unavailable is left out on purpose: asked for by position
/// with `--playlist-items`, yt-dlp then downloads it instead of skipping it
/// thinking it is already there. The JSON is the single source of truth.
pub fn export_work_file(storage: &dyn Storage, txt: &Location, data: &ArchiveData) -> io::Result<()> {
    if let Some(parent) = parent_of(txt) {
        storage.create_dir_all(&parent)?;
    }
    let mut text = String::new();
    for (id, entry) in &data.entries {
        if has_real_file(storage, data, entry) || entry.unavailable_at.is_some() {
            text.push_str("youtube ");
            text.push_str(id);
            text.push_str(LINE_SEPARATOR);
        }
    }
    storage.write(txt, text.as_bytes())
}

const LINE_SEPARATOR: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// Every active archive: the loose `individual_*.json` plus one
/// `pl_NAME_hash/FORMAT/data.json` per playlist and format.
///
/// The old `playlist_NAME_archive_FORMAT.json` files left by the withdrawn
/// migration are deliberately NOT included: they stay on disk as a backup only.
pub fn list_active_archive_json_files(storage: &dyn Storage, archives: &Location) -> Vec<Location> {
    let Ok(children) = storage.list(archives) else { return Vec::new() };
    let mut result: Vec<Location> = children
        .iter()
        .filter(|e| !e.is_dir && e.name.starts_with("individual_") && e.name.ends_with(".json"))
        .map(|e| e.location.clone())
        .collect();
    for playlist in children.iter().filter(|e| e.is_dir && e.name.starts_with("pl_")) {
        for format in storage.list(&playlist.location).unwrap_or_default().into_iter().filter(|e| e.is_dir) {
            if let Ok(json) = storage.child(&format.location, DATA_JSON)
                && storage.exists(&json).unwrap_or(false)
            {
                result.push(json);
            }
        }
    }
    result
}

/// A playlist's archive (`pl_X_hash/mp3/data.json`) and not the flat one of the
/// loose downloads (`individual_archive_mp3.json`): only playlists have a music
/// folder of their own, the manifest among what depends on it.
pub fn is_playlist_archive(json: &Location) -> bool {
    json.as_path()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .is_some_and(|name| name.to_string_lossy().starts_with("pl_"))
}

/// "individual_archive_mp3.json" -> "mp3"; "pl_X_hash/mp3/data.json" -> "mp3".
pub fn format_of(json: &Location) -> Option<String> {
    let path = json.as_path()?;
    if is_playlist_archive(json) {
        return path.parent()?.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let index = name.rfind("_archive_")?;
    let format = &name[index + "_archive_".len()..];
    Some(format.strip_suffix(".json").unwrap_or(format).to_string())
}

pub fn txt_file_for(json: &Location) -> Option<Location> {
    let path = json.as_path()?;
    if is_playlist_archive(json) {
        return Some(Location::Path(path.with_file_name(ARCHIVE_TXT)));
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let txt = name.strip_suffix(".json").map(|base| format!("{base}.txt")).unwrap_or(name);
    Some(Location::Path(path.with_file_name(txt)))
}

pub(crate) fn parent_of(location: &Location) -> Option<Location> {
    location
        .as_path()?
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| Location::Path(p.to_path_buf()))
}

/// Is it really downloaded RIGHT NOW? The only criterion, used alike by the
/// pending list, "Ver archivos" and the Gestor so they always agree.
///
/// A device-backed playlist trusts its last known state (`deleted_at`): a
/// device round trip per entry on every read is not affordable.
pub fn has_real_file(storage: &dyn Storage, data: &ArchiveData, entry: &ArchiveEntry) -> bool {
    if data.is_device_backed() {
        return !blank(entry.device_file_name.as_deref()) && entry.deleted_at.is_none();
    }
    match entry.file_path.as_deref() {
        Some(path) if !path.trim().is_empty() => storage.exists(&Location::parse(path)).unwrap_or(false),
        _ => false,
    }
}

pub fn parse_int_safe(text: Option<&str>) -> i32 {
    text.and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

pub(crate) fn blank(text: Option<&str>) -> bool {
    text.is_none_or(|t| t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn path(text: &str) -> Location {
        Location::path(text)
    }

    #[test]
    fn a_pl_grandparent_folder_is_a_playlist_archive() {
        assert!(is_playlist_archive(&path("data/history/archives/pl_Foo_ab12cd/mp3/data.json")));
        assert!(!is_playlist_archive(&path("data/history/archives/individual_archive_mp3.json")));
        assert!(!is_playlist_archive(&Location::uri("content://x/pl_a/mp3/data.json")));
    }

    #[test]
    fn the_format_comes_from_the_folder_or_the_name() {
        assert_eq!(format_of(&path("data/history/archives/pl_Foo_ab12cd/mp3/data.json")).as_deref(), Some("mp3"));
        assert_eq!(format_of(&path("individual_archive_flac.json")).as_deref(), Some("flac"));
        assert_eq!(format_of(&path("something_else.json")), None);
    }

    #[test]
    fn the_txt_sits_next_to_its_json() {
        assert_eq!(
            txt_file_for(&path("data/history/archives/pl_Foo_ab12cd/mp3/data.json")),
            Some(path("data/history/archives/pl_Foo_ab12cd/mp3/archive.txt"))
        );
        assert_eq!(txt_file_for(&path("individual_archive_mp3.json")), Some(path("individual_archive_mp3.txt")));
    }

    #[test]
    fn parse_int_safe_never_fails() {
        assert_eq!(parse_int_safe(None), 0);
        assert_eq!(parse_int_safe(Some("   ")), 0);
        assert_eq!(parse_int_safe(Some("  012 ")), 12);
        assert_eq!(parse_int_safe(Some("abc")), 0);
    }

    #[test]
    fn a_missing_archive_loads_empty_and_saving_creates_its_folder() {
        let storage = MemoryStorage::new();
        let json = path("/archives/pl_A_123456/mp3/data.json");
        assert_eq!(load(&storage, &json).unwrap(), ArchiveData::default());
        let data = ArchiveData { number_files: true, ..Default::default() };
        save(&storage, &json, &data).unwrap();
        assert!(load(&storage, &json).unwrap().number_files);
    }

    #[test]
    fn yt_dlp_only_skips_what_is_really_on_disk_or_unavailable() {
        let storage = MemoryStorage::new();
        storage.create_dir_all(&path("/music")).unwrap();
        storage.write(&path("/music/a.mp3"), b"x").unwrap();
        let mut data = ArchiveData::default();
        let on_disk = ArchiveEntry { file_path: Some("/music/a.mp3".into()), ..Default::default() };
        let gone = ArchiveEntry { file_path: Some("/music/b.mp3".into()), ..Default::default() };
        let unavailable = ArchiveEntry { unavailable_at: Some("2026-01-01 10:00".into()), ..Default::default() };
        data.entries.insert("aaaaaaaaaaa".into(), on_disk);
        data.entries.insert("bbbbbbbbbbb".into(), gone);
        data.entries.insert("ccccccccccc".into(), unavailable);
        let txt = path("/archives/individual_archive_mp3.txt");
        export_work_file(&storage, &txt, &data).unwrap();
        let text = String::from_utf8(storage.read(&txt).unwrap()).unwrap();
        let ids: Vec<&str> = text.lines().collect();
        assert_eq!(ids, ["youtube aaaaaaaaaaa", "youtube ccccccccccc"]);
    }

    #[test]
    fn lists_individual_and_playlist_archives_but_not_the_old_scheme() {
        let storage = MemoryStorage::new();
        let archives = path("/archives");
        storage.create_dir_all(&path("/archives/pl_A_123456/mp3")).unwrap();
        storage.create_dir_all(&path("/archives/pl_A_123456/empty")).unwrap();
        storage.write(&path("/archives/pl_A_123456/mp3/data.json"), b"{}").unwrap();
        storage.write(&path("/archives/individual_archive_mp3.json"), b"{}").unwrap();
        storage.write(&path("/archives/playlist_A_archive_mp3.json"), b"{}").unwrap();
        let found = list_active_archive_json_files(&storage, &archives);
        assert_eq!(found, [path("/archives/individual_archive_mp3.json"), path("/archives/pl_A_123456/mp3/data.json")]);
    }
}
