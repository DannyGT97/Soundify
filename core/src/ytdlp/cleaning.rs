//! yt-dlp's own leftovers (thumbnails, `.part`, `.json` metadata...) after an
//! audio download, cleaned out of the music folder.

use crate::Location;
use crate::storage::Storage;

/// What yt-dlp may leave behind next to the audio files.
const LEFTOVERS: [&str; 9] = [
    ".png", ".webp", ".jpg", ".jpeg", ".json", ".meta",
    // Leftovers from converting to audio.
    ".mp4", ".webm",
    // An incomplete download, after stopping.
    ".part",
];

/// Deletes yt-dlp's known leftovers from `folder`, never touching files of the
/// audio `format` being downloaded. Returns how many were deleted; a folder that
/// cannot be listed is a silent no-op.
pub fn clean_output_folder(storage: &dyn Storage, folder: &Location, format: &str) -> usize {
    let keep = format!(".{}", format.to_lowercase());
    let Ok(entries) = storage.list(folder) else { return 0 };

    let mut deleted = 0;
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let name = entry.name.to_lowercase();
        if name.ends_with(&keep) || !LEFTOVERS.iter().any(|ext| name.ends_with(ext)) {
            continue;
        }
        match storage.remove_file(&entry.location) {
            Ok(()) => deleted += 1,
            Err(e) => log::warn!("Could not delete leftover {}: {e}", entry.name),
        }
    }
    log::info!("Cleanup done (kept only {keep}), {deleted} deleted");
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalStorage;

    fn folder_with(names: &[&str]) -> (tempfile::TempDir, Location) {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let location = Location::path(dir.path());
        (dir, location)
    }

    #[test]
    fn keeps_the_target_format_deletes_yt_dlp_known_leftovers() {
        let (dir, folder) = folder_with(&["song.mp3", "song.webp", "song.json", "other.part"]);
        clean_output_folder(&LocalStorage, &folder, "MP3");
        assert!(dir.path().join("song.mp3").exists());
        assert!(!dir.path().join("song.webp").exists());
        assert!(!dir.path().join("song.json").exists());
        assert!(!dir.path().join("other.part").exists());
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        let (dir, folder) = folder_with(&["SONG.WEBP"]);
        clean_output_folder(&LocalStorage, &folder, "MP3");
        assert!(!dir.path().join("SONG.WEBP").exists());
    }

    #[test]
    fn an_extension_not_on_the_leftover_list_is_left_alone() {
        let (dir, folder) = folder_with(&["readme.txt"]);
        clean_output_folder(&LocalStorage, &folder, "MP3");
        assert!(dir.path().join("readme.txt").exists());
    }

    #[test]
    fn a_folder_that_cannot_be_listed_is_a_silent_no_op() {
        let (dir, _) = folder_with(&[]);
        let missing = Location::path(dir.path().join("missing"));
        assert_eq!(clean_output_folder(&LocalStorage, &missing, "MP3"), 0);
    }
}
