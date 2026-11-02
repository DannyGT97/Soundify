//! Keeping an archive in step with its music folder: registering what was
//! downloaded, finding files again, marking what went missing or left the
//! playlist, and the numbering. Never deletes an entry: it only stamps dates.

use std::collections::HashSet;

use indexmap::IndexMap;
use log::{info, warn};

use super::io::{blank, has_real_file, parse_int_safe};
use crate::Location;
use crate::model::{ArchiveData, ArchiveEntry};
use crate::paths::{self, NumberTitle};
use crate::storage::Storage;

/// Two entries with the same title in an unnumbered playlist, one of them
/// without its file: both would be saved under the same name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TitleCollision {
    pub count: usize,
    pub title: String,
}

/// How an archive stores a location. Folders reach the core already absolute
/// (the Java app stored `getAbsolutePath()`), so nothing is resolved here.
pub(crate) fn stored(location: &Location) -> String {
    location.to_string()
}

/// A file yt-dlp just finished (its id came through the `--print` hooks).
pub fn register_captured(
    storage: &dyn Storage,
    data: &mut ArchiveData,
    folder: &Location,
    id: &str,
    file: &Location,
    now: &str,
) {
    if id.trim().is_empty() {
        return;
    }
    let numbered = data.number_files;
    let entry = data.entries.entry(id.to_string()).or_insert_with(|| ArchiveEntry {
        added_at: Some(now.to_string()),
        ..Default::default()
    });
    let previous_number = entry.number.clone();
    entry.file_path = Some(stored(file));
    entry.deleted_at = None;
    entry.removed_from_playlist_at = None;
    entry.unavailable_at = None;
    entry.unavailable_reason = None;
    entry.last_failed_at = None;
    entry.last_failed_reason = None;

    if let Some(parsed) = parse_file_name(numbered, &file.name(), Some(id)) {
        let mut assigned = parsed.number;
        // Downloaded again (deleted by hand, say): it gets its own number
        // back instead of the next free one, so the folder keeps its order.
        if let (Some(previous), Some(new)) = (&previous_number, &assigned)
            && previous != new
            && let Some(renamed) = rename_to_number(storage, folder, file, previous)
        {
            entry.file_path = Some(stored(&renamed));
            assigned = Some(previous.clone());
        }
        if assigned.is_some() {
            entry.number = assigned;
        }
        // regression: a file named after its id must not overwrite a real title.
        let parsed_is_just_the_id = parsed.title == id;
        let has_real_title = entry.title.as_deref().is_some_and(|t| !t.trim().is_empty() && t != id);
        if !(parsed_is_just_the_id && has_real_title) {
            entry.title = Some(parsed.title);
        }
    }
    let number = entry.number.clone();
    bump_last_number(data, number.as_deref());
}

/// Renames "012 - Title.mp3" to "<number> - Title.mp3". An orphan already
/// holding that name (an earlier attempt at the same item) is deleted first.
/// None when the number cannot be reused: the new one stays.
fn rename_to_number(storage: &dyn Storage, folder: &Location, file: &Location, number: &str) -> Option<Location> {
    let name = file.name();
    let dot = name.rfind('.')?;
    let (base, extension) = name.split_at(dot);
    let title_only = strip_leading_number(base);
    let target = storage.child(folder, &format!("{number} - {title_only}{extension}")).ok()?;
    if &target == file {
        return Some(target);
    }
    if storage.exists(&target).unwrap_or(false) {
        if storage.remove_file(&target).is_err() {
            warn!("Could not reuse number {number} for \"{name}\": \"{target}\" already exists and could not be deleted.");
            return None;
        }
        info!("Freed \"{target}\" (an orphan from an earlier attempt) to reuse its number {number}.");
    }
    match storage.rename(file, &target) {
        Ok(()) => Some(target),
        Err(e) => {
            warn!("Could not rename \"{name}\" to reuse its original number {number}: {e}");
            None
        }
    }
}

fn strip_leading_number(base: &str) -> &str {
    let digits = base.bytes().take_while(u8::is_ascii_digit).count();
    if digits >= 3 { base[digits..].strip_prefix(" - ").unwrap_or(base) } else { base }
}

/// Only an unnumbered playlist can collide: the number tells them apart otherwise.
pub fn title_collisions(storage: &dyn Storage, data: &ArchiveData) -> Vec<TitleCollision> {
    if data.number_files {
        return Vec::new();
    }
    let mut by_title: IndexMap<String, Vec<&ArchiveEntry>> = IndexMap::new();
    for entry in data.entries.values() {
        if entry.unavailable_at.is_some() || entry.removed_from_playlist_at.is_some() {
            continue;
        }
        let Some(title) = entry.title.as_deref().filter(|t| !t.trim().is_empty()) else { continue };
        by_title.entry(title.to_lowercase()).or_default().push(entry);
    }
    by_title
        .values()
        .filter(|group| group.len() >= 2 && group.iter().any(|e| !has_real_file(storage, data, e)))
        .map(|group| TitleCollision { count: group.len(), title: group[0].title.clone().unwrap_or_default() })
        .collect()
}

pub(crate) fn parse_file_name(numbered: bool, file_name: &str, id: Option<&str>) -> Option<NumberTitle> {
    if numbered { paths::parse_number_and_title(file_name, id) } else { paths::parse_title_only(file_name, id) }
}

/// Looks again at the music folder: finds files that moved name, stamps
/// `deleted_at` on what vanished (or clears it on what came back), adopts
/// old-style `[id]` files and recomputes the last number.
pub fn reconcile_entries(storage: &dyn Storage, data: &mut ArchiveData, folder: &Location, extension: &str, now: &str) {
    let files = files_with_extension(storage, folder, extension);
    let legacy_on_disk: IndexMap<String, (Location, String)> = files
        .iter()
        .filter_map(|(location, name)| {
            paths::parse_indexed_file_name(name).map(|p| (p.id, (location.clone(), name.clone())))
        })
        .collect();
    let by_number_on_disk = if data.number_files { by_number(&files) } else { IndexMap::new() };
    let numbered = data.number_files;

    for (id, entry) in data.entries.iter_mut() {
        let had_file_path = !blank(entry.file_path.as_deref());
        let mut exists =
            had_file_path && storage.exists(&Location::parse(entry.file_path.as_deref().unwrap_or_default())).unwrap_or(false);
        if !exists {
            let matched = legacy_on_disk
                .get(id)
                .or_else(|| entry.number.as_ref().and_then(|n| by_number_on_disk.get(n)));
            if let Some((location, name)) = matched {
                exists = true;
                entry.file_path = Some(stored(location));
                if let Some(parsed) = parse_file_name(numbered, name, Some(id))
                    && !parsed.title.trim().is_empty()
                {
                    entry.title = Some(parsed.title);
                }
            }
        }
        if exists {
            entry.deleted_at = None;
        } else if had_file_path {
            if entry.deleted_at.is_none() {
                entry.deleted_at = Some(now.to_string());
            }
        } else {
            // Never had a file: "deleted" would be a lie.
            entry.deleted_at = None;
        }
    }
    for (id, (location, name)) in legacy_on_disk {
        data.entries.entry(id).or_insert_with(|| new_legacy_entry(&location, &name, now));
    }
    let numbered_on_disk = by_number(&files);
    recompute_last_number(storage, data, numbered_on_disk.keys());
}

/// A device-backed playlist checked against a fresh listing of the device.
pub fn reconcile_device_entries(data: &mut ArchiveData, device_listing: &HashSet<String>, now: &str) {
    for entry in data.entries.values_mut() {
        let Some(name) = entry.device_file_name.as_deref().filter(|n| !n.trim().is_empty()) else { continue };
        if device_listing.contains(name) {
            entry.deleted_at = None;
        } else if entry.deleted_at.is_none() {
            entry.deleted_at = Some(now.to_string());
        }
    }
}

/// What the playlist on YouTube holds now (`seen`) against what the archive knows.
pub fn handle_playlist_removals(data: &mut ArchiveData, seen: &HashSet<String>, now: &str) {
    for (id, entry) in data.entries.iter_mut() {
        if entry.deleted_at.is_some() {
            continue; // no longer on disk: does not apply
        }
        if seen.contains(id) {
            entry.removed_from_playlist_at = None;
        } else if entry.removed_from_playlist_at.is_none() {
            entry.removed_from_playlist_at = Some(now.to_string());
        }
    }
}

fn new_legacy_entry(location: &Location, name: &str, now: &str) -> ArchiveEntry {
    let parsed = paths::parse_indexed_file_name(name);
    ArchiveEntry {
        number: parsed.as_ref().and_then(|p| p.number.clone()),
        title: Some(parsed.map(|p| p.title).unwrap_or_else(|| name.to_string())),
        file_path: Some(stored(location)),
        added_at: Some(now.to_string()),
        ..Default::default()
    }
}

/// The files of the folder with this extension (any case), with their names.
pub(crate) fn files_with_extension(storage: &dyn Storage, folder: &Location, extension: &str) -> Vec<(Location, String)> {
    let suffix = format!(".{}", extension.to_lowercase());
    storage
        .list(folder)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && e.name.to_lowercase().ends_with(&suffix))
        .map(|e| (e.location, e.name))
        .collect()
}

fn by_number(files: &[(Location, String)]) -> IndexMap<String, (Location, String)> {
    files
        .iter()
        .filter_map(|(location, name)| {
            paths::parse_number_and_title(name, None)?.number.map(|n| (n, (location.clone(), name.clone())))
        })
        .collect()
}

pub(crate) fn bump_last_number(data: &mut ArchiveData, number: Option<&str>) {
    let Some(number) = number else { return };
    if parse_int_safe(Some(number)) > parse_int_safe(data.last_number.as_deref()) {
        data.last_number = Some(number.to_string());
    }
}

/// The highest number among real files: deleting everything starts again at 001.
fn recompute_last_number<'a>(storage: &dyn Storage, data: &mut ArchiveData, on_disk: impl Iterator<Item = &'a String>) {
    let from_entries = data
        .entries
        .values()
        .filter(|e| has_real_file(storage, data, e))
        .map(|e| parse_int_safe(e.number.as_deref()));
    let max = from_entries.chain(on_disk.map(|n| parse_int_safe(Some(n)))).max().unwrap_or(0);
    data.last_number = (max > 0).then(|| format!("{max:03}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn entry(title: &str, file_path: Option<&str>) -> ArchiveEntry {
        ArchiveEntry { title: Some(title.into()), file_path: file_path.map(Into::into), ..Default::default() }
    }

    fn storage_with(files: &[&str]) -> MemoryStorage {
        let storage = MemoryStorage::new();
        storage.create_dir_all(&Location::path("/music")).unwrap();
        for file in files {
            storage.write(&Location::path(format!("/music/{file}")), b"x").unwrap();
        }
        storage
    }

    #[test]
    fn warns_when_a_title_clash_has_a_missing_file() {
        let storage = storage_with(&["Song.mp3"]);
        let mut data = ArchiveData::default();
        data.entries.insert("id1".into(), entry("Canción", Some("/music/Song.mp3")));
        data.entries.insert("id2".into(), entry("canción", Some("/music/no existe.mp3")));
        assert_eq!(title_collisions(&storage, &data), [TitleCollision { count: 2, title: "Canción".into() }]);
    }

    #[test]
    fn no_warning_when_both_files_already_exist() {
        let storage = storage_with(&["A.mp3", "B.mp3"]);
        let mut data = ArchiveData::default();
        data.entries.insert("id1".into(), entry("Canción", Some("/music/A.mp3")));
        data.entries.insert("id2".into(), entry("Canción", Some("/music/B.mp3")));
        assert!(title_collisions(&storage, &data).is_empty());
    }

    #[test]
    fn a_numbered_playlist_never_warns() {
        let mut data = ArchiveData { number_files: true, ..Default::default() };
        data.entries.insert("id1".into(), entry("Canción", None));
        data.entries.insert("id2".into(), entry("canción", None));
        assert!(title_collisions(&MemoryStorage::new(), &data).is_empty());
    }

    #[test]
    fn an_entry_removed_from_the_playlist_is_left_out_of_the_group() {
        let mut removed = entry("Canción", None);
        removed.removed_from_playlist_at = Some("2026-01-01 00:00".into());
        let mut data = ArchiveData::default();
        data.entries.insert("id1".into(), entry("Canción", None));
        data.entries.insert("id2".into(), removed);
        assert!(title_collisions(&MemoryStorage::new(), &data).is_empty());
    }

    #[test]
    fn strips_only_a_real_order_number() {
        assert_eq!(strip_leading_number("012 - Title"), "Title");
        assert_eq!(strip_leading_number("12 - Title"), "12 - Title");
        assert_eq!(strip_leading_number("2024"), "2024");
    }

    #[test]
    fn device_entries_follow_the_device_listing() {
        let mut data = ArchiveData { device_id: Some("usb#1".into()), ..Default::default() };
        let on_device = ArchiveEntry { device_file_name: Some("a.mp3".into()), deleted_at: Some("old".into()), ..Default::default() };
        let gone = ArchiveEntry { device_file_name: Some("b.mp3".into()), ..Default::default() };
        let never = ArchiveEntry::default();
        data.entries.insert("a".into(), on_device);
        data.entries.insert("b".into(), gone);
        data.entries.insert("c".into(), never);
        let listing: HashSet<String> = ["a.mp3".to_string()].into();
        reconcile_device_entries(&mut data, &listing, "2026-11-02 10:00");
        assert_eq!(data.entries["a"].deleted_at, None);
        assert_eq!(data.entries["b"].deleted_at.as_deref(), Some("2026-11-02 10:00"));
        assert_eq!(data.entries["c"].deleted_at, None);
    }
}
