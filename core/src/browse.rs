//! What the file explorer needs from the computer: the drives, the usual
//! places (Music, Downloads…), a folder's contents and «Nueva carpeta».
//!
//! Listing goes through [`Storage`], so it is tested with `MemoryStorage`; only
//! [`drives`] and [`places`] look at the real disk. A phone's folders do not
//! come through here: they are the `PortableDevice`'s.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::Location;
use crate::storage::Storage;
use crate::transfer;

/// Names Windows puts in every drive's root: nobody wants them in a list.
const HIDDEN_NAMES: [&str; 2] = ["$RECYCLE.BIN", "System Volume Information"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootKind {
    /// A whole disk: it carries its space.
    Drive,
    /// A user's folder (Music, Downloads…).
    Place,
}

/// A way in: a drive or one of the user's usual folders. The front names it
/// (`id` for a place, the letter in `path` for a drive) so its text is translated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRoot {
    pub id: String,
    pub kind: RootKind,
    pub path: String,
    /// 0 when unknown (a place does not say).
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Can be read but not written (a phone's SD card: Android gives apps no write access by path).
    pub read_only: bool,
}

/// What a folder's own manifest says it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePlaylist {
    pub title: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEntry {
    pub name: String,
    pub is_folder: bool,
    /// 0 for a folder.
    pub size_bytes: u64,
    /// Local time, `2026-11-17T09:30:00`; absent when the system does not say.
    pub modified: Option<String>,
    /// Set on a folder that carries a Soundify manifest.
    pub playlist: Option<BrowsePlaylist>,
}

/// A playlist found by [`find_playlists`]: where its folder is and what its manifest says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistHit {
    pub path: String,
    pub title: String,
    pub format: String,
    /// How deep inside the folder searched it was (1 = directly in it).
    pub depth: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindResult {
    pub hits: Vec<PlaylistHit>,
    /// The search stopped at its limits: there may be more.
    pub truncated: bool,
}

/// How far down and how much [`find_playlists`] looks before it stops.
pub const FIND_MAX_DEPTH: usize = 4;
pub const FIND_MAX_FOLDERS: usize = 5000;
pub const FIND_MAX_HITS: usize = 200;

#[derive(Debug, PartialEq, Eq)]
pub enum BrowseError {
    InvalidName,
    Exists,
    Io(String),
}

impl BrowseError {
    /// The i18n key the front shows.
    pub fn key(&self) -> &'static str {
        match self {
            BrowseError::InvalidName => "explorer.newFolder.invalid",
            BrowseError::Exists => "explorer.newFolder.exists",
            BrowseError::Io(_) => "explorer.newFolder.failed",
        }
    }
}

/// A name that is one folder, not a path, and that Windows would accept.
pub fn valid_folder_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        && !name.ends_with('.')
}

/// The folder's contents, folders first is the front's business. Entries the
/// system hides or refuses to describe are left out rather than failing the list.
pub fn list(storage: &dyn Storage, folder: &Location) -> io::Result<Vec<BrowseEntry>> {
    let mut entries = Vec::new();
    for child in storage.list(folder)? {
        if child.name.starts_with('.') || HIDDEN_NAMES.contains(&child.name.as_str()) {
            continue;
        }
        let modified = storage.modified(&child.location).ok().flatten().map(|time| time.format("%Y-%m-%dT%H:%M:%S").to_string());
        if child.is_dir {
            let playlist = transfer::read_manifest(storage, &child.location).ok().map(|m| BrowsePlaylist { title: m.title, format: m.format });
            entries.push(BrowseEntry { name: child.name, is_folder: true, size_bytes: 0, modified, playlist });
        } else {
            let size_bytes = storage.size(&child.location).unwrap_or(0);
            entries.push(BrowseEntry { name: child.name, is_folder: false, size_bytes, modified, playlist: None });
        }
    }
    Ok(entries)
}

/// The Soundify playlists inside `root`, up to [`FIND_MAX_DEPTH`] levels down. A playlist's own folder is not searched
/// (its subfolders are not playlists); hidden folders and the ones that cannot be read are skipped.
pub fn find_playlists(storage: &dyn Storage, root: &Location) -> FindResult {
    let mut result = FindResult::default();
    let mut visited = 0usize;
    let mut pending: Vec<(Location, usize)> = vec![(root.clone(), 0)];
    while let Some((folder, depth)) = pending.pop() {
        let Ok(children) = storage.list(&folder) else { continue };
        for child in children.into_iter().filter(|c| c.is_dir && !c.name.starts_with('.') && !HIDDEN_NAMES.contains(&c.name.as_str())) {
            visited += 1;
            if visited > FIND_MAX_FOLDERS || result.hits.len() >= FIND_MAX_HITS {
                result.truncated = true;
                result.hits.sort_by(|a, b| a.path.cmp(&b.path));
                return result;
            }
            if let Ok(manifest) = transfer::read_manifest(storage, &child.location) {
                result.hits.push(PlaylistHit { path: child.location.to_string(), title: manifest.title, format: manifest.format, depth: depth + 1 });
            } else if depth + 1 < FIND_MAX_DEPTH {
                pending.push((child.location, depth + 1));
            }
        }
    }
    result.hits.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// Creates `name` inside `parent` and says where it is. It never replaces: a
/// folder (or file) with that name already there is an error.
pub fn make_folder(storage: &dyn Storage, parent: &Location, name: &str) -> Result<Location, BrowseError> {
    if !valid_folder_name(name) {
        return Err(BrowseError::InvalidName);
    }
    let made = storage.child(parent, name.trim()).map_err(|e| BrowseError::Io(e.to_string()))?;
    if storage.exists(&made).unwrap_or(false) {
        return Err(BrowseError::Exists);
    }
    storage.create_dir_all(&made).map_err(|e| BrowseError::Io(e.to_string()))?;
    Ok(made)
}

/// The user's usual folders that exist, under `home` (a phone calls Downloads «Download»).
pub fn places(home: &Path) -> Vec<BrowseRoot> {
    let mut found: Vec<BrowseRoot> = Vec::new();
    for (id, dir) in [("music", "Music"), ("downloads", "Downloads"), ("downloads", "Download"), ("pictures", "Pictures"), ("camera", "DCIM"), ("desktop", "Desktop")] {
        let path = home.join(dir);
        if path.is_dir() && !found.iter().any(|root| root.id == id) {
            found.push(BrowseRoot { id: id.into(), kind: RootKind::Place, path: path.display().to_string(), total_bytes: 0, free_bytes: 0, read_only: false });
        }
    }
    found
}

/// The user's home: where the places are looked for.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| p.is_dir())
}

fn drive(id: String, path: String, read_only: bool) -> BrowseRoot {
    let total_bytes = fs4::total_space(&path).unwrap_or(0);
    let free_bytes = fs4::available_space(&path).unwrap_or(0);
    BrowseRoot { id, kind: RootKind::Drive, path, total_bytes, free_bytes, read_only }
}

/// A phone's storages under `storage_dir` (`/storage`): the internal one, `emulated/0`, and each SD card or
/// USB stick, a folder named like `3A1F-0C22`. The cards are read-only: apps write there only through the
/// system's own chooser, not by path.
pub fn android_storages(storage_dir: &Path) -> Vec<BrowseRoot> {
    let mut roots = Vec::new();
    let internal = storage_dir.join("emulated").join("0");
    if internal.is_dir() {
        roots.push(drive("internal".into(), internal.display().to_string(), false));
    }
    let mut cards: Vec<PathBuf> = std::fs::read_dir(storage_dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    cards.sort();
    for card in cards {
        let name = card.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name == "emulated" || name == "self" || !card.is_dir() {
            continue;
        }
        roots.push(drive(format!("sd:{name}"), card.display().to_string(), true));
    }
    roots
}

/// The disks that answer right now, with their space. On Windows one per letter, on Android the phone's
/// storages, elsewhere just the root.
pub fn drives() -> Vec<BrowseRoot> {
    if cfg!(target_os = "android") {
        return android_storages(Path::new("/storage"));
    }
    let candidates: Vec<String> = if cfg!(windows) { ('A'..='Z').map(|letter| format!("{letter}:\\")).collect() } else { vec!["/".to_string()] };
    candidates.into_iter().filter(|root| Path::new(root).exists()).map(|root| drive(format!("drive:{root}"), root, false)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn memory() -> (MemoryStorage, Location) {
        let storage = MemoryStorage::new();
        let root = Location::path("/music");
        storage.create_dir_all(&root).unwrap();
        (storage, root)
    }

    #[test]
    fn lists_folders_and_files_with_sizes() {
        let (storage, root) = memory();
        storage.create_dir_all(&storage.child(&root, "Rock").unwrap()).unwrap();
        storage.write(&storage.child(&root, "a.mp3").unwrap(), &[0; 10]).unwrap();
        let entries = list(&storage, &root).unwrap();
        let rock = entries.iter().find(|e| e.name == "Rock").unwrap();
        assert!(rock.is_folder && rock.playlist.is_none());
        let song = entries.iter().find(|e| e.name == "a.mp3").unwrap();
        assert!(!song.is_folder);
        assert_eq!(song.size_bytes, 10);
    }

    #[test]
    fn hides_what_nobody_wants_to_see() {
        let (storage, root) = memory();
        for name in [".git", "$RECYCLE.BIN", "System Volume Information", "Visible"] {
            storage.create_dir_all(&storage.child(&root, name).unwrap()).unwrap();
        }
        let names: Vec<_> = list(&storage, &root).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Visible"]);
    }

    #[test]
    fn recognises_a_soundify_playlist_by_its_manifest() {
        let (storage, root) = memory();
        let folder = storage.child(&root, "Lo-fi (mp3)").unwrap();
        storage.create_dir_all(&folder).unwrap();
        let manifest = crate::model::PlaylistManifest { schema: 1, title: "Lo-fi".into(), format: "mp3".into(), ..Default::default() };
        transfer::write_manifest(&storage, &folder, &manifest).unwrap();
        // A folder with a file of that name that is no manifest is not one.
        let fake = storage.child(&root, "Otra").unwrap();
        storage.create_dir_all(&fake).unwrap();
        storage.write(&storage.child(&fake, crate::archive::io::MANIFEST_NAME).unwrap(), b"{}").unwrap();
        let entries = list(&storage, &root).unwrap();
        let found = entries.iter().find(|e| e.name == "Lo-fi (mp3)").unwrap();
        assert_eq!(found.playlist, Some(BrowsePlaylist { title: "Lo-fi".into(), format: "mp3".into() }));
        assert!(entries.iter().find(|e| e.name == "Otra").unwrap().playlist.is_none());
    }

    fn playlist_in(storage: &MemoryStorage, folder: &Location, title: &str) {
        storage.create_dir_all(folder).unwrap();
        let manifest = crate::model::PlaylistManifest { schema: 1, title: title.into(), format: "mp3".into(), ..Default::default() };
        transfer::write_manifest(storage, folder, &manifest).unwrap();
    }

    #[test]
    fn finds_playlists_down_the_subfolders_but_not_inside_a_playlist() {
        let (storage, root) = memory();
        playlist_in(&storage, &Location::path("/music/Rock (mp3)"), "Rock");
        playlist_in(&storage, &Location::path("/music/Copias/2025/Pop (mp3)"), "Pop");
        // Inside a playlist, what is below is no other playlist.
        playlist_in(&storage, &Location::path("/music/Rock (mp3)/extra"), "Extra");
        storage.create_dir_all(&Location::path("/music/.oculta")).unwrap();
        playlist_in(&storage, &Location::path("/music/.oculta/Secreta (mp3)"), "Secreta");
        let found = find_playlists(&storage, &root);
        let titles: Vec<_> = found.hits.iter().map(|h| (h.title.as_str(), h.depth)).collect();
        assert_eq!(titles, [("Pop", 3), ("Rock", 1)]);
        assert!(!found.truncated);
    }

    #[test]
    fn does_not_search_deeper_than_its_limit() {
        let (storage, root) = memory();
        playlist_in(&storage, &Location::path("/music/a/b/c/d/Honda (mp3)"), "Honda");
        playlist_in(&storage, &Location::path("/music/a/b/c/Justa (mp3)"), "Justa");
        let titles: Vec<_> = find_playlists(&storage, &root).hits.into_iter().map(|h| h.title).collect();
        assert_eq!(titles, ["Justa"]);
    }

    #[test]
    fn finding_in_a_missing_folder_finds_nothing() {
        let (storage, _) = memory();
        assert_eq!(find_playlists(&storage, &Location::path("/nada")), FindResult::default());
    }

    #[test]
    fn listing_a_missing_folder_fails() {
        let (storage, _) = memory();
        assert!(list(&storage, &Location::path("/nada")).is_err());
    }

    #[test]
    fn folder_names_are_checked() {
        for good in ["Música", "Lo-fi (mp3)", "  con espacios  ", "a.b"] {
            assert!(valid_folder_name(good), "{good}");
        }
        for bad in ["", "  ", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a|b", "punto."] {
            assert!(!valid_folder_name(bad), "{bad}");
        }
    }

    #[test]
    fn makes_a_folder_but_never_over_another() {
        let (storage, root) = memory();
        let made = make_folder(&storage, &root, " Nueva ").unwrap();
        assert_eq!(made, Location::path("/music/Nueva"));
        assert!(storage.list(&made).unwrap().is_empty());
        assert_eq!(make_folder(&storage, &root, "Nueva"), Err(BrowseError::Exists));
        assert_eq!(make_folder(&storage, &root, "a/b"), Err(BrowseError::InvalidName));
        assert_eq!(BrowseError::Exists.key(), "explorer.newFolder.exists");
    }

    #[test]
    fn places_are_only_the_folders_that_exist() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("Music")).unwrap();
        std::fs::create_dir(home.path().join("Downloads")).unwrap();
        let ids: Vec<_> = places(home.path()).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["music", "downloads"]);
    }

    #[test]
    fn a_phones_storages_are_the_internal_one_and_the_cards_which_only_read() {
        let storage = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(storage.path().join("emulated").join("0")).unwrap();
        std::fs::create_dir(storage.path().join("3A1F-0C22")).unwrap();
        std::fs::create_dir(storage.path().join("self")).unwrap();
        std::fs::write(storage.path().join("stray.txt"), b"x").unwrap();
        let roots = android_storages(storage.path());
        let summary: Vec<_> = roots.iter().map(|r| (r.id.as_str(), r.read_only)).collect();
        assert_eq!(summary, [("internal", false), ("sd:3A1F-0C22", true)]);
    }

    #[test]
    fn a_phone_without_a_card_has_only_the_internal_storage() {
        let storage = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(storage.path().join("emulated").join("0")).unwrap();
        assert_eq!(android_storages(storage.path()).len(), 1);
        assert!(android_storages(&storage.path().join("missing")).is_empty());
    }

    #[test]
    fn a_phones_download_folder_counts_as_downloads() {
        let home = tempfile::tempdir().unwrap();
        for dir in ["Music", "Download", "DCIM"] {
            std::fs::create_dir(home.path().join(dir)).unwrap();
        }
        let ids: Vec<_> = places(home.path()).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["music", "downloads", "camera"]);
    }

    #[test]
    fn the_real_drives_report_space() {
        let drives = drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().all(|d| d.kind == RootKind::Drive && !d.path.is_empty()));
    }
}
