//! Files and folders, asked through a trait.
//!
//! The Java app's rule "downloaded = the file EXISTS" stays, but the question
//! goes to [`Storage`], never to `std::fs` directly: on Android the music
//! folder is a SAF tree and only the platform can answer for it.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use crate::Location;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub location: Location,
    pub is_dir: bool,
}

pub trait Storage: Send + Sync {
    /// `name` inside `dir`. Not always a string join: a SAF child has its own URI.
    fn child(&self, dir: &Location, name: &str) -> io::Result<Location>;
    fn exists(&self, location: &Location) -> io::Result<bool>;
    /// The direct children of `dir`, sorted by name.
    fn list(&self, dir: &Location) -> io::Result<Vec<DirEntry>>;
    fn read(&self, location: &Location) -> io::Result<Vec<u8>>;
    /// Writes a whole file. The folder must already exist: it is never created
    /// on the way. Writing the manifest into a playlist folder that is gone
    /// must fail, not leave an empty new folder behind that makes the Gestor
    /// believe the playlist is still there (a real bug in the Java app).
    fn write(&self, location: &Location, bytes: &[u8]) -> io::Result<()>;
    fn create_dir_all(&self, dir: &Location) -> io::Result<()>;
    fn remove_file(&self, location: &Location) -> io::Result<()>;
    /// The folder and everything inside it ("Eliminar todo" deletes the whole folder).
    fn remove_dir_all(&self, dir: &Location) -> io::Result<()>;
    fn rename(&self, from: &Location, to: &Location) -> io::Result<()>;

    /// The file's size in bytes ("already copied" = same name and same size).
    fn size(&self, location: &Location) -> io::Result<u64> {
        Ok(self.read(location)?.len() as u64)
    }

    /// Copies one file over whatever is at `to`. Like [`Storage::write`], the
    /// target folder must already exist.
    fn copy(&self, from: &Location, to: &Location) -> io::Result<()> {
        let bytes = self.read(from)?;
        self.write(to, &bytes)
    }

    /// Free bytes where `dir` lives, or None if this storage cannot tell (then
    /// nothing is refused for lack of space).
    fn available_space(&self, _dir: &Location) -> io::Result<Option<u64>> {
        Ok(None)
    }
}

// === LOCAL DISK ===

/// The desktop storage: plain paths on the local file system.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalStorage;

impl LocalStorage {
    fn path(location: &Location) -> io::Result<&Path> {
        location.as_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, format!("not a local path: {location}"))
        })
    }
}

impl Storage for LocalStorage {
    fn child(&self, dir: &Location, name: &str) -> io::Result<Location> {
        Ok(Location::Path(Self::path(dir)?.join(name)))
    }

    fn exists(&self, location: &Location) -> io::Result<bool> {
        Self::path(location)?.try_exists()
    }

    fn list(&self, dir: &Location) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(Self::path(dir)?)? {
            let entry = entry?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                location: Location::Path(entry.path()),
                is_dir: entry.file_type()?.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read(&self, location: &Location) -> io::Result<Vec<u8>> {
        std::fs::read(Self::path(location)?)
    }

    fn write(&self, location: &Location, bytes: &[u8]) -> io::Result<()> {
        // std::fs::write would fail anyway without the folder; checked first so
        // the error says which folder, not just "not found".
        let path = Self::path(location)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && !parent.is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("folder does not exist: {}", parent.display()),
            ));
        }
        std::fs::write(path, bytes)
    }

    fn create_dir_all(&self, dir: &Location) -> io::Result<()> {
        std::fs::create_dir_all(Self::path(dir)?)
    }

    fn remove_file(&self, location: &Location) -> io::Result<()> {
        std::fs::remove_file(Self::path(location)?)
    }

    fn remove_dir_all(&self, dir: &Location) -> io::Result<()> {
        std::fs::remove_dir_all(Self::path(dir)?)
    }

    fn rename(&self, from: &Location, to: &Location) -> io::Result<()> {
        std::fs::rename(Self::path(from)?, Self::path(to)?)
    }

    fn size(&self, location: &Location) -> io::Result<u64> {
        Ok(std::fs::metadata(Self::path(location)?)?.len())
    }

    /// Streamed by the OS: a video of several GB never passes through memory.
    fn copy(&self, from: &Location, to: &Location) -> io::Result<()> {
        std::fs::copy(Self::path(from)?, Self::path(to)?).map(|_| ())
    }

    fn available_space(&self, dir: &Location) -> io::Result<Option<u64>> {
        fs4::available_space(Self::path(dir)?).map(Some)
    }
}

// === IN MEMORY ===

/// A storage that lives in memory, for tests. It takes paths and URIs alike,
/// so the logic can be tested against the Android shape without a device.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    /// `None` is a folder, `Some` a file with its bytes.
    nodes: Mutex<BTreeMap<Location, Option<Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    fn parent(location: &Location) -> Option<Location> {
        match location {
            Location::Path(path) => path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| Location::Path(p.to_path_buf())),
            Location::Uri(uri) => uri.rsplit_once('/').map(|(parent, _)| Location::Uri(parent.to_string())),
        }
    }

    fn is_inside(location: &Location, dir: &Location) -> bool {
        let mut current = Self::parent(location);
        while let Some(parent) = current {
            if &parent == dir {
                return true;
            }
            current = Self::parent(&parent);
        }
        false
    }

    fn not_found(location: &Location) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, format!("not found: {location}"))
    }

    fn name(location: &Location) -> String {
        match location {
            Location::Path(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            Location::Uri(uri) => uri.rsplit_once('/').map(|(_, name)| name.to_string()).unwrap_or_default(),
        }
    }
}

impl Storage for MemoryStorage {
    fn child(&self, dir: &Location, name: &str) -> io::Result<Location> {
        Ok(match dir {
            Location::Path(path) => Location::Path(path.join(name)),
            Location::Uri(uri) => Location::Uri(format!("{uri}/{name}")),
        })
    }

    fn exists(&self, location: &Location) -> io::Result<bool> {
        Ok(self.nodes.lock().unwrap().contains_key(location))
    }

    fn list(&self, dir: &Location) -> io::Result<Vec<DirEntry>> {
        let nodes = self.nodes.lock().unwrap();
        match nodes.get(dir) {
            Some(None) => {}
            Some(Some(_)) => return Err(io::Error::new(io::ErrorKind::NotADirectory, dir.to_string())),
            None => return Err(Self::not_found(dir)),
        }
        let mut entries: Vec<DirEntry> = nodes
            .iter()
            .filter(|(location, _)| Self::parent(location).as_ref() == Some(dir))
            .map(|(location, node)| DirEntry {
                name: Self::name(location),
                location: location.clone(),
                is_dir: node.is_none(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read(&self, location: &Location) -> io::Result<Vec<u8>> {
        match self.nodes.lock().unwrap().get(location) {
            Some(Some(bytes)) => Ok(bytes.clone()),
            Some(None) => Err(io::Error::new(io::ErrorKind::IsADirectory, location.to_string())),
            None => Err(Self::not_found(location)),
        }
    }

    fn write(&self, location: &Location, bytes: &[u8]) -> io::Result<()> {
        let mut nodes = self.nodes.lock().unwrap();
        if let Some(parent) = Self::parent(location)
            && !matches!(nodes.get(&parent), Some(None))
        {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("folder does not exist: {parent}")));
        }
        nodes.insert(location.clone(), Some(bytes.to_vec()));
        Ok(())
    }

    fn create_dir_all(&self, dir: &Location) -> io::Result<()> {
        let mut nodes = self.nodes.lock().unwrap();
        let mut current = Some(dir.clone());
        while let Some(location) = current {
            if let Some(Some(_)) = nodes.get(&location) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("a file is in the way: {location}")));
            }
            current = Self::parent(&location);
            nodes.insert(location, None);
        }
        Ok(())
    }

    fn remove_file(&self, location: &Location) -> io::Result<()> {
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get(location) {
            Some(Some(_)) => {
                nodes.remove(location);
                Ok(())
            }
            Some(None) => Err(io::Error::new(io::ErrorKind::IsADirectory, location.to_string())),
            None => Err(Self::not_found(location)),
        }
    }

    fn remove_dir_all(&self, dir: &Location) -> io::Result<()> {
        let mut nodes = self.nodes.lock().unwrap();
        if !nodes.contains_key(dir) {
            return Err(Self::not_found(dir));
        }
        nodes.retain(|location, _| location != dir && !Self::is_inside(location, dir));
        Ok(())
    }

    fn rename(&self, from: &Location, to: &Location) -> io::Result<()> {
        let mut nodes = self.nodes.lock().unwrap();
        let moved: Vec<Location> = nodes
            .keys()
            .filter(|location| *location == from || Self::is_inside(location, from))
            .cloned()
            .collect();
        if moved.is_empty() {
            return Err(Self::not_found(from));
        }
        for old in moved {
            let node = nodes.remove(&old).unwrap_or_default();
            let new = rebase(&old, from, to);
            nodes.insert(new, node);
        }
        Ok(())
    }
}

/// `location` moved from under `from` to under `to`.
fn rebase(location: &Location, from: &Location, to: &Location) -> Location {
    match (location, from, to) {
        (Location::Path(path), Location::Path(old), Location::Path(new)) => {
            Location::Path(new.join(path.strip_prefix(old).unwrap_or(path)))
        }
        (Location::Uri(uri), Location::Uri(old), Location::Uri(new)) => {
            Location::Uri(format!("{new}{}", uri.strip_prefix(old.as_str()).unwrap_or(uri)))
        }
        _ => to.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same checks against every implementation: they must behave alike.
    fn contract(storage: &dyn Storage, root: Location) {
        storage.create_dir_all(&root).unwrap();
        let folder = storage.child(&root, "Chill").unwrap();
        let file = storage.child(&folder, "001 - Echoes.mp3").unwrap();

        // Never creates the folder on the way.
        assert_eq!(storage.write(&file, b"x").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(!storage.exists(&folder).unwrap());

        storage.create_dir_all(&folder).unwrap();
        storage.write(&file, b"audio").unwrap();
        assert!(storage.exists(&file).unwrap());
        assert_eq!(storage.read(&file).unwrap(), b"audio");

        let second = storage.child(&folder, "002 - Gravity.mp3").unwrap();
        storage.write(&second, b"more").unwrap();
        let names: Vec<String> = storage.list(&folder).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["001 - Echoes.mp3", "002 - Gravity.mp3"]);
        assert_eq!(storage.size(&file).unwrap(), 5);

        // Copying overwrites, and never creates the target folder either.
        storage.copy(&file, &second).unwrap();
        assert_eq!(storage.read(&second).unwrap(), b"audio");
        let elsewhere = storage.child(&root, "Missing").unwrap();
        assert!(storage.copy(&file, &storage.child(&elsewhere, "x.mp3").unwrap()).is_err());

        let renamed = storage.child(&root, "Chill (copy)").unwrap();
        storage.rename(&folder, &renamed).unwrap();
        assert!(!storage.exists(&folder).unwrap());
        let moved = storage.child(&renamed, "001 - Echoes.mp3").unwrap();
        assert_eq!(storage.read(&moved).unwrap(), b"audio");

        storage.remove_file(&moved).unwrap();
        assert!(!storage.exists(&moved).unwrap());

        storage.remove_dir_all(&renamed).unwrap();
        assert!(!storage.exists(&renamed).unwrap());
        assert!(storage.list(&root).unwrap().is_empty());
    }

    #[test]
    fn local_storage_follows_the_contract() {
        let dir = tempfile::tempdir().unwrap();
        contract(&LocalStorage, Location::path(dir.path().join("music")));
    }

    #[test]
    fn memory_storage_follows_the_contract_with_paths() {
        contract(&MemoryStorage::new(), Location::path("/music"));
    }

    #[test]
    fn memory_storage_follows_the_contract_with_uris() {
        contract(&MemoryStorage::new(), Location::uri("content://tree/primary%3AMusic"));
    }

    #[test]
    fn local_storage_refuses_a_uri_instead_of_guessing() {
        let error = LocalStorage.exists(&Location::uri("content://tree/x")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
