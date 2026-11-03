//! yt-dlp's own leftovers (thumbnails, `.part`, `.json` metadata...) after an
//! audio download, cleaned out of the music folder.

use crate::Location;
use crate::archive::io::MANIFEST_NAME;
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
/// cannot be listed is a silent no-op. For a playlist's own folder: everything
/// in it is the app's.
pub fn clean_output_folder(storage: &dyn Storage, folder: &Location, format: &str) -> usize {
    clean(storage, folder, format, |_| true)
}

/// The same, but only for the leftovers of the files named by `stems` (the
/// name without extension of each file this run wrote). For a single
/// download: it goes into whatever folder the user picked, and the other
/// pictures or `.json` files in there are not ours to delete.
pub fn clean_leftovers_of(storage: &dyn Storage, folder: &Location, format: &str, stems: &[String]) -> usize {
    if stems.is_empty() {
        return 0;
    }
    let stems: Vec<String> = stems.iter().map(|s| s.to_lowercase()).collect();
    clean(storage, folder, format, |name| stems.iter().any(|stem| name.starts_with(stem.as_str())))
}

fn clean(storage: &dyn Storage, folder: &Location, format: &str, ours: impl Fn(&str) -> bool) -> usize {
    let keep = format!(".{}", format.to_lowercase());
    let Ok(entries) = storage.list(folder) else { return 0 };

    let mut deleted = 0;
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let name = entry.name.to_lowercase();
        // The manifest is the playlist's passport (see `PlaylistTransfer`), not
        // a yt-dlp leftover, whatever its extension says.
        if name == MANIFEST_NAME || name.ends_with(&keep) || !LEFTOVERS.iter().any(|ext| name.ends_with(ext)) || !ours(&name) {
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

    #[test]
    fn the_playlist_manifest_is_never_taken_for_a_leftover() {
        let (dir, folder) = folder_with(&[MANIFEST_NAME, "song.json"]);
        clean_output_folder(&LocalStorage, &folder, "MP3");
        assert!(dir.path().join(MANIFEST_NAME).exists());
        assert!(!dir.path().join("song.json").exists());
    }

    #[test]
    fn a_single_download_only_cleans_up_after_its_own_files() {
        let (dir, folder) = folder_with(&["Canción.mp3", "Canción.webp", "Canción.mp4", "holidays.jpg", "notes.json"]);
        assert_eq!(clean_leftovers_of(&LocalStorage, &folder, "MP3", &["Canción".into()]), 2);
        assert!(dir.path().join("Canción.mp3").exists());
        assert!(!dir.path().join("Canción.webp").exists());
        assert!(!dir.path().join("Canción.mp4").exists());
        assert!(dir.path().join("holidays.jpg").exists(), "the user's own files in that folder are not ours");
        assert!(dir.path().join("notes.json").exists());
    }

    #[test]
    fn with_nothing_downloaded_a_single_download_cleans_nothing() {
        let (dir, folder) = folder_with(&["holidays.jpg"]);
        assert_eq!(clean_leftovers_of(&LocalStorage, &folder, "MP3", &[]), 0);
        assert!(dir.path().join("holidays.jpg").exists());
    }
}
