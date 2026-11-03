//! A phone plugged in by USB, seen by Windows as an MTP/WPD portable device
//! (= the Java `utils/device` package). The transport itself lives behind
//! [`PortableDevice`]: the desktop crate has the Windows one, and anything else
//! gets [`NoDevices`]. What is done WITH a device (copying, importing, syncing
//! a playlist) is in `transfer::device` and `engine::download`, written against
//! the trait so it is tested with [`fake::FakeDevice`].
//!
//! A path inside a device is never a `Location`: it is a string of segments
//! joined by "/", "" for the storage root, and it is only understood by the
//! device that answers it.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::Location;

/// A device Windows sees connected right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

/// One entry inside a device folder. Name + size is deliberately the whole
/// comparison surface for deciding what to push: it is exactly what the
/// device answers cheaply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub name: String,
    /// 0 for a folder.
    pub size_bytes: u64,
    pub is_folder: bool,
}

/// What the device+folder picker returns: a spot inside a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTarget {
    pub device_id: String,
    pub device_name: String,
    /// "/"-separated, "" for the root.
    pub path: String,
}

/// Talks to a portable device.
///
/// **Every method blocks**, and a call can legitimately hang for a long time:
/// Windows can show its own "the device stopped responding" dialog and keep
/// the calling thread waiting until it is dismissed (~54 s measured). The
/// implementation is the one that bounds each call and returns the "nothing
/// happened" answer when it runs out; callers never run these on the UI thread.
/// None of the methods fails loudly: an unreachable device or path answers
/// empty or `false`.
pub trait PortableDevice: Send + Sync {
    /// False on any OS where this transport does not apply.
    fn is_available(&self) -> bool;

    /// Every portable device Windows currently sees connected.
    fn list_devices(&self) -> Vec<DeviceInfo>;

    /// Is the device's storage actually reachable right now? A phone plugged in
    /// "charge only" still shows up in [`Self::list_devices`] (Windows makes the
    /// device node from the USB descriptor alone), but its storage cannot be
    /// walked into until the phone grants file access. Every "is it connected"
    /// check outside the picker means this (the Gestor kept showing a
    /// charge-only phone as reachable — reported).
    fn is_reachable(&self, device_id: &str) -> bool;

    /// The contents of `path`, empty if the device or the path is not reachable.
    fn list(&self, device_id: &str, path: &str) -> Vec<DeviceEntry>;

    /// Creates `path`, including missing parent segments.
    fn mkdirs(&self, device_id: &str, path: &str) -> bool;

    /// Copies `local` into `path` (which must exist), waiting for the device to
    /// confirm it landed. `dest_name` is the name it gets there, or `local`'s
    /// own when `None`: a device playlist's files are staged as `<id>.<ext>`
    /// so an interrupted push is recoverable by name alone, but pushing THAT
    /// name left every file on the phone titled after its YouTube id (reported).
    fn push(&self, device_id: &str, path: &str, local: &Path, dest_name: Option<&str>) -> bool;

    /// Deletes every one of `paths` in one batch; one not found is skipped.
    fn delete(&self, device_id: &str, paths: &[String]) -> bool;

    /// Copies `path` down to `local`, whose folder must already exist.
    fn pull(&self, device_id: &str, path: &str, local: &Path) -> bool;

    /// Opens `path` in a new Explorer window, as double-clicking it would.
    fn open_in_explorer(&self, device_id: &str, path: &str) -> bool;
}

/// Where there is no MTP: nothing is ever connected.
pub struct NoDevices;

impl PortableDevice for NoDevices {
    fn is_available(&self) -> bool {
        false
    }
    fn list_devices(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }
    fn is_reachable(&self, _: &str) -> bool {
        false
    }
    fn list(&self, _: &str, _: &str) -> Vec<DeviceEntry> {
        Vec::new()
    }
    fn mkdirs(&self, _: &str, _: &str) -> bool {
        false
    }
    fn push(&self, _: &str, _: &str, _: &Path, _: Option<&str>) -> bool {
        false
    }
    fn delete(&self, _: &str, _: &[String]) -> bool {
        false
    }
    fn pull(&self, _: &str, _: &str, _: &Path) -> bool {
        false
    }
    fn open_in_explorer(&self, _: &str, _: &str) -> bool {
        false
    }
}

// === PATHS INSIDE A DEVICE ===

pub fn join_path(base: &str, segment: &str) -> String {
    if base.trim().is_empty() { segment.to_string() } else { format!("{base}/{segment}") }
}

pub fn last_segment(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, last)| last)
}

/// "" for a path with no "/" (a folder right at the root).
pub fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// "Title (format)" -> (title, format), the reverse of
/// `paths::device_playlist_sub_path`. Cut at the LAST " (": a title can carry
/// its own parentheses.
pub fn parse_folder_name(folder_name: &str) -> Option<(&str, &str)> {
    let inner = folder_name.strip_suffix(')')?;
    let open = inner.rfind(" (")?;
    Some((&inner[..open], &inner[open + 2..]))
}

/// What a device playlist's archive hash is computed from, in place of the
/// local base folder: the same playlist in two folders of the phone (or on two
/// phones) is two playlists, as it is on disk.
pub fn hash_key(device_id: &str, base_path: &str) -> Location {
    Location::Uri(format!("mtp:{device_id}:{base_path}"))
}

#[cfg(test)]
pub mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_folder_name_normal_case() {
        assert_eq!(parse_folder_name("Cocina (mp3)"), Some(("Cocina", "mp3")));
    }

    #[test]
    fn parse_folder_name_without_closing_parenthesis_is_none() {
        assert_eq!(parse_folder_name("Cocina (mp3"), None);
    }

    #[test]
    fn parse_folder_name_without_opening_marker_is_none() {
        assert_eq!(parse_folder_name("Cocina"), None);
        assert_eq!(parse_folder_name("Cocina(mp3)"), None);
    }

    #[test]
    fn regression_a_title_with_its_own_parentheses_splits_on_the_last_one() {
        assert_eq!(parse_folder_name("Cancion (Remix) (mp3)"), Some(("Cancion (Remix)", "mp3")));
    }

    #[test]
    fn device_paths_join_and_split_on_slashes() {
        assert_eq!(join_path("", "Music"), "Music");
        assert_eq!(join_path("Music", "Chill (mp3)"), "Music/Chill (mp3)");
        assert_eq!(last_segment("Music/Chill (mp3)"), "Chill (mp3)");
        assert_eq!(last_segment("Music"), "Music");
        assert_eq!(parent_path("Internal/Music/Chill (mp3)"), "Internal/Music");
        assert_eq!(parent_path("Music"), "");
    }

    #[test]
    fn the_hash_key_tells_devices_and_base_folders_apart() {
        assert_eq!(hash_key("usb#1", "Music"), Location::Uri("mtp:usb#1:Music".into()));
        assert_ne!(hash_key("usb#1", "Music"), hash_key("usb#2", "Music"));
        assert_ne!(hash_key("usb#1", "Music"), hash_key("usb#1", "Podcasts"));
    }

    #[test]
    fn no_devices_has_nothing_connected() {
        let none = NoDevices;
        assert!(!none.is_available());
        assert!(none.list_devices().is_empty());
        assert!(!none.is_reachable("usb#1"));
        assert!(!none.mkdirs("usb#1", "Music"));
    }
}
