//! A phone in memory, for the tests of everything written against
//! [`PortableDevice`]. One device, "usb#1", whose storage is a tree of folders
//! and files held as bytes.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Mutex;

use super::{DeviceEntry, DeviceInfo, PortableDevice, join_path, parent_path};

pub const ID: &str = "usb#1";
pub const NAME: &str = "Pixel";

#[derive(Default)]
struct Tree {
    folders: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
}

pub struct FakeDevice {
    tree: Mutex<Tree>,
    reachable: Mutex<bool>,
    /// File names whose push or pull fails.
    failing: Mutex<HashSet<String>>,
    /// Every push, as "folder/name", in order.
    pushed: Mutex<Vec<String>>,
}

impl Default for FakeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDevice {
    pub fn new() -> Self {
        Self {
            tree: Mutex::default(),
            reachable: Mutex::new(true),
            failing: Mutex::default(),
            pushed: Mutex::default(),
        }
    }

    pub fn set_reachable(&self, reachable: bool) {
        *self.reachable.lock().unwrap() = reachable;
    }

    pub fn fail_on(&self, file_name: &str) {
        self.failing.lock().unwrap().insert(file_name.to_string());
    }

    pub fn put_file(&self, path: &str, bytes: &[u8]) {
        let mut tree = self.tree.lock().unwrap();
        add_folders(&mut tree, parent_path(path));
        tree.files.insert(path.to_string(), bytes.to_vec());
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.tree.lock().unwrap().files.get(path).cloned()
    }

    pub fn has_folder(&self, path: &str) -> bool {
        self.tree.lock().unwrap().folders.contains(path)
    }

    /// The file names right inside `folder`.
    pub fn names_in(&self, folder: &str) -> Vec<String> {
        self.list(ID, folder).into_iter().filter(|e| !e.is_folder).map(|e| e.name).collect()
    }

    pub fn pushed(&self) -> Vec<String> {
        self.pushed.lock().unwrap().clone()
    }

    fn up(&self, device_id: &str) -> bool {
        device_id == ID && *self.reachable.lock().unwrap()
    }

    fn fails(&self, name: &str) -> bool {
        self.failing.lock().unwrap().contains(name)
    }
}

fn add_folders(tree: &mut Tree, path: &str) {
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        current = join_path(&current, segment);
        tree.folders.insert(current.clone());
    }
}

impl PortableDevice for FakeDevice {
    fn is_available(&self) -> bool {
        true
    }

    fn list_devices(&self) -> Vec<DeviceInfo> {
        vec![DeviceInfo { id: ID.into(), name: NAME.into() }]
    }

    fn is_reachable(&self, device_id: &str) -> bool {
        self.up(device_id)
    }

    fn list(&self, device_id: &str, path: &str) -> Vec<DeviceEntry> {
        if !self.up(device_id) {
            return Vec::new();
        }
        let tree = self.tree.lock().unwrap();
        if !path.is_empty() && !tree.folders.contains(path) {
            return Vec::new();
        }
        let folders = tree.folders.iter().filter(|f| parent_path(f) == path && f.as_str() != path).map(|f| DeviceEntry {
            name: super::last_segment(f).to_string(),
            size_bytes: 0,
            is_folder: true,
        });
        let files = tree.files.iter().filter(|(f, _)| parent_path(f) == path).map(|(f, bytes)| DeviceEntry {
            name: super::last_segment(f).to_string(),
            size_bytes: bytes.len() as u64,
            is_folder: false,
        });
        folders.chain(files).collect()
    }

    fn mkdirs(&self, device_id: &str, path: &str) -> bool {
        if !self.up(device_id) {
            return false;
        }
        add_folders(&mut self.tree.lock().unwrap(), path);
        true
    }

    fn push(&self, device_id: &str, path: &str, local: &Path, dest_name: Option<&str>) -> bool {
        let name = dest_name.map(str::to_string).unwrap_or_else(|| {
            local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        });
        if !self.up(device_id) || self.fails(&name) || !(path.is_empty() || self.has_folder(path)) {
            return false;
        }
        let Ok(bytes) = std::fs::read(local) else { return false };
        let target = join_path(path, &name);
        self.tree.lock().unwrap().files.insert(target.clone(), bytes);
        self.pushed.lock().unwrap().push(target);
        true
    }

    fn delete(&self, device_id: &str, paths: &[String]) -> bool {
        if !self.up(device_id) {
            return false;
        }
        let mut tree = self.tree.lock().unwrap();
        for path in paths {
            tree.files.remove(path);
            let inside = format!("{path}/");
            tree.files.retain(|f, _| !f.starts_with(&inside));
            tree.folders.retain(|f| f != path && !f.starts_with(&inside));
        }
        true
    }

    fn pull(&self, device_id: &str, path: &str, local: &Path) -> bool {
        if !self.up(device_id) || self.fails(super::last_segment(path)) {
            return false;
        }
        let Some(bytes) = self.file(path) else { return false };
        std::fs::write(local, bytes).is_ok()
    }

    fn open_in_explorer(&self, device_id: &str, path: &str) -> bool {
        self.up(device_id) && (path.is_empty() || self.has_folder(path))
    }
}
