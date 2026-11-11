//! What a task needs to itself while it runs, so that two of them never touch
//! the same thing: the archive (`data.json`, numbering, `archive.txt`), the
//! music folder (files, cleanup of leftovers) and the phone's single queue.
//!
//! The planner in [`crate::engine::tasks`] starts a waiting task only if none
//! of its keys is held by a running one. While the app runs one task at a time
//! this changes nothing, but it is what makes more than one safe later.
//!
//! The keys are compared by name, never by touching the disk: two spellings of
//! the same folder (`C:\Music\` and `c:/music`) are the same key, and when in
//! doubt two keys are treated as the same (a wrong "same" only makes a task
//! wait; a wrong "different" corrupts an archive).

use crate::archive::ArchiveStore;
use crate::location::Location;

/// One thing a task holds for as long as it runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceKey {
    /// An archive's `data.json`, by its normalized location.
    Archive(String),
    /// A music folder, by its normalized location.
    Folder(String),
    /// The phone: one copy at a time, whatever the playlist.
    Device,
}

impl ResourceKey {
    pub fn archive(json: &str) -> Self {
        ResourceKey::Archive(normalize(json))
    }

    pub fn folder(folder: &str) -> Self {
        ResourceKey::Folder(normalize(folder))
    }
}

/// Same separators, no trailing slash, no case: how two spellings of a place meet.
fn normalize(place: &str) -> String {
    let mut text = place.trim().replace('\\', "/").to_lowercase();
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    text
}

/// True when the two sets share a key.
pub fn conflict(held: &[ResourceKey], wanted: &[ResourceKey]) -> bool {
    wanted.iter().any(|key| held.contains(key))
}

/// A loose download: the folder it goes into.
pub fn for_single(folder: &str) -> Vec<ResourceKey> {
    vec![ResourceKey::folder(folder)]
}

/// A single that ends on a phone.
pub fn for_device() -> Vec<ResourceKey> {
    vec![ResourceKey::Device]
}

/// A sync of one archive: the archive and the folder it downloads into, or the
/// phone when the playlist lives there. An archive that cannot be read holds
/// only itself (the run will report it).
pub fn for_update(archives: &ArchiveStore, json: &Location) -> Vec<ResourceKey> {
    let mut keys = vec![ResourceKey::archive(&json.to_string())];
    match archives.load(json) {
        Ok(data) if data.is_device_backed() => keys.push(ResourceKey::Device),
        Ok(data) => {
            if let Some(folder) = data.folder.as_deref().filter(|f| !f.trim().is_empty()) {
                keys.push(ResourceKey::folder(folder));
            }
        }
        Err(_) => {}
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock;
    use crate::model::ArchiveData;
    use crate::storage::{MemoryStorage, Storage};
    use std::sync::Arc;

    #[test]
    fn two_spellings_of_a_place_are_one_key() {
        assert_eq!(ResourceKey::folder("C:\\Music\\Rock\\"), ResourceKey::folder("c:/music/rock"));
        assert_ne!(ResourceKey::folder("/music/a"), ResourceKey::folder("/music/b"));
        assert_ne!(ResourceKey::folder("/music/a"), ResourceKey::archive("/music/a"), "a folder is not an archive");
    }

    #[test]
    fn sets_conflict_when_they_share_any_key() {
        let a = vec![ResourceKey::archive("/a/data.json"), ResourceKey::folder("/music/a")];
        let b = vec![ResourceKey::folder("/music/b"), ResourceKey::folder("/MUSIC/A/")];
        let c = vec![ResourceKey::archive("/c/data.json")];
        assert!(conflict(&a, &b));
        assert!(!conflict(&a, &c));
        assert!(!conflict(&a, &[]));
        assert!(conflict(&for_device(), &for_device()), "one phone, one copy at a time");
    }

    #[test]
    fn a_sync_holds_its_archive_and_its_folder_and_a_phone_playlist_holds_the_phone() {
        let storage = Arc::new(MemoryStorage::new());
        let archives = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-02 10:00"));
        storage.create_dir_all(&Location::path("/archives/pl_a/mp3")).unwrap();
        storage.create_dir_all(&Location::path("/archives/pl_p/mp3")).unwrap();
        let json = Location::path("/archives/pl_a/mp3/data.json");
        let phone_json = Location::path("/archives/pl_p/mp3/data.json");

        let local = ArchiveData { folder: Some("/music/A".to_string()), ..ArchiveData::default() };
        archives.save(&json, &local).unwrap();
        assert_eq!(for_update(&archives, &json), vec![ResourceKey::archive(&json.to_string()), ResourceKey::folder("/music/a")]);

        let phone = ArchiveData { device_id: Some("usb#1".to_string()), ..ArchiveData::default() };
        archives.save(&phone_json, &phone).unwrap();
        assert_eq!(for_update(&archives, &phone_json), vec![ResourceKey::archive(&phone_json.to_string()), ResourceKey::Device]);

        let missing = Location::path("/archives/none/data.json");
        assert_eq!(for_update(&archives, &missing), vec![ResourceKey::archive(&missing.to_string())], "an unreadable archive holds only itself");
    }
}
