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

/// The user's usual folders that exist, under `home`.
pub fn places(home: &Path) -> Vec<BrowseRoot> {
    [("music", "Music"), ("downloads", "Downloads"), ("pictures", "Pictures"), ("desktop", "Desktop")]
        .into_iter()
        .filter_map(|(id, dir)| {
            let path = home.join(dir);
            path.is_dir().then(|| BrowseRoot { id: id.into(), kind: RootKind::Place, path: path.display().to_string(), total_bytes: 0, free_bytes: 0 })
        })
        .collect()
}

/// The user's home: where the places are looked for.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| p.is_dir())
}

/// The disks that answer right now, with their space. On Windows one per letter;
/// elsewhere just the root.
pub fn drives() -> Vec<BrowseRoot> {
    let candidates: Vec<String> = if cfg!(windows) { ('A'..='Z').map(|letter| format!("{letter}:\\")).collect() } else { vec!["/".to_string()] };
    candidates
        .into_iter()
        .filter(|root| Path::new(root).exists())
        .map(|root| {
            let total_bytes = fs4::total_space(&root).unwrap_or(0);
            let free_bytes = fs4::available_space(&root).unwrap_or(0);
            BrowseRoot { id: format!("drive:{root}"), kind: RootKind::Drive, path: root, total_bytes, free_bytes }
        })
        .collect()
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
    fn the_real_drives_report_space() {
        let drives = drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().all(|d| d.kind == RootKind::Drive && !d.path.is_empty()));
    }
}
