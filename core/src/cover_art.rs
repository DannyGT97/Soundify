//! The real pictures on the covers (the Java `CoverArt`, minus the drawing):
//! a playlist's own, picked by the user, or else its first video's thumbnail.
//! The gradient tile the front draws stays underneath as the fallback.
//!
//! Cutting the picture to the tile's shape is the front's job (`src/covers.ts`,
//! `crop`): the webview draws the image anyway, so there is no need to decode
//! it here.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::http::Http;
use crate::location::Location;
use crate::storage::Storage;

/// The picked cover, inside the playlist's own archive folder. Called .jpg
/// whatever it really is (as in Java): the webview reads the picture by its
/// contents, not by its name.
pub const CUSTOM_COVER: &str = "cover.jpg";

/// What the cover picker offers: the types both the Java app and the webview read.
pub const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "bmp", "webp"];

// === PURE LOGIC ===

/// An 11-character YouTube id, and nothing else: it ends up in a file name, so
/// `../` and the like must never get through.
pub fn is_video_id(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A video's thumbnail on YouTube. "hqdefault" is the one that always exists
/// (the bigger "maxresdefault" is missing on many videos); it is 4:3 with the
/// 16:9 picture letterboxed inside — see `crop` in the front.
pub fn thumbnail_url(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

/// The archive folder of a playlist (`pl_<Name>_<hash>`) from one of its formats' data.json.
pub fn root_dir_of(json: &Location) -> Option<Location> {
    json.as_path()?.parent()?.parent().map(Location::path)
}

/// Whether `bytes` start like one of the pictures the picker offers. Checked
/// by contents, not by name: the Java app asked JavaFX to decode it, and a
/// renamed file must not become a cover that never shows.
pub fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"BM")
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

// === PUBLIC API — FILES ===

/// The picked cover of the playlist whose archive folder is `root_dir`, if there is one.
pub fn custom_cover(storage: &dyn Storage, root_dir: &Location) -> Option<Location> {
    let cover = storage.child(root_dir, CUSTOM_COVER).ok()?;
    storage.exists(&cover).unwrap_or(false).then_some(cover)
}

/// Makes `image` the playlist's cover (a copy: the original can be moved or
/// deleted afterwards). Fails if it is not a picture.
pub fn set_custom_cover(storage: &dyn Storage, root_dir: &Location, image: &Location) -> io::Result<()> {
    let bytes = storage.read(image)?;
    if !looks_like_image(&bytes) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not an image: {image}")));
    }
    storage.create_dir_all(root_dir)?;
    storage.write(&storage.child(root_dir, CUSTOM_COVER)?, &bytes)
}

// === THUMBNAILS ===

/// Fetches video thumbnails to `data/cache/thumbnails/<id>.jpg`, one at a time
/// on a single thread of its own (a Gestor full of playlists must not open
/// dozens of connections at once). Each one is fetched once: a failure is not
/// retried until the next start, so a dead network is not asked again on every
/// redraw.
pub struct ThumbnailFetcher {
    dir: PathBuf,
    queue: Mutex<Sender<String>>,
    state: Arc<Mutex<FetchState>>,
}

#[derive(Default)]
struct FetchState {
    fetching: HashSet<String>,
    failed: HashSet<String>,
}

/// What [`ThumbnailFetcher::thumbnail`] knows right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Thumbnail {
    /// On disk already.
    Ready(PathBuf),
    /// Being fetched: `on_ready` will be called with the id.
    Pending,
    /// Not an id, or it failed this session.
    Unavailable,
}

impl ThumbnailFetcher {
    /// `on_ready` gets the id of every thumbnail that lands on disk, on the fetcher's thread.
    pub fn new(dir: PathBuf, http: Arc<dyn Http>, on_ready: impl Fn(&str) + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel::<String>();
        let state = Arc::new(Mutex::new(FetchState::default()));
        let worker_state = state.clone();
        let worker_dir = dir.clone();
        std::thread::Builder::new()
            .name("cover-art-fetcher".into())
            .spawn(move || {
                for id in receiver {
                    let target = thumbnail_file(&worker_dir, &id);
                    let fetched = http.to_file(&thumbnail_url(&id), &target, &mut |_| {});
                    let mut state = worker_state.lock().unwrap_or_else(|e| e.into_inner());
                    state.fetching.remove(&id);
                    match fetched {
                        Ok(()) => {
                            drop(state);
                            on_ready(&id);
                        }
                        Err(e) => {
                            log::debug!("Could not fetch the thumbnail of {id}: {e}");
                            state.failed.insert(id);
                        }
                    }
                }
            })
            .expect("could not start the thumbnail fetcher");
        Self { dir, queue: Mutex::new(sender), state }
    }

    /// The thumbnail of `video_id`: its file if it is on disk, otherwise it is
    /// queued (once) and [`Thumbnail::Pending`] is returned.
    pub fn thumbnail(&self, video_id: &str) -> Thumbnail {
        if !is_video_id(video_id) {
            return Thumbnail::Unavailable;
        }
        let target = thumbnail_file(&self.dir, video_id);
        if target.is_file() {
            return Thumbnail::Ready(target);
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.failed.contains(video_id) {
            return Thumbnail::Unavailable;
        }
        if state.fetching.insert(video_id.to_string()) {
            let queued = self.queue.lock().unwrap_or_else(|e| e.into_inner()).send(video_id.to_string());
            if queued.is_err() {
                state.fetching.remove(video_id);
                return Thumbnail::Unavailable;
            }
        }
        Thumbnail::Pending
    }
}

/// Where a video's thumbnail is kept once fetched.
pub fn thumbnail_file(dir: &Path, video_id: &str) -> PathBuf {
    dir.join(format!("{video_id}.jpg"))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    use super::*;
    use crate::http::write_atomically;
    use crate::storage::MemoryStorage;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10];

    /// Serves a JPEG for every URL but the ones listed as offline, and counts the requests.
    struct FakeHttp {
        offline: Vec<String>,
        asked: Mutex<Vec<String>>,
    }

    impl Http for FakeHttp {
        fn get_string(&self, _url: &str, _headers: &[(&str, &str)]) -> io::Result<String> {
            Err(io::Error::other("not used"))
        }

        fn to_file(&self, url: &str, target: &Path, on_progress: &mut dyn FnMut(f64)) -> io::Result<()> {
            self.asked.lock().unwrap().push(url.to_string());
            if self.offline.iter().any(|id| url.contains(id.as_str())) {
                return Err(io::Error::other("offline"));
            }
            write_atomically(JPEG, target, None, on_progress)
        }
    }

    fn fetcher(offline: &[&str]) -> (tempfile::TempDir, Arc<FakeHttp>, ThumbnailFetcher, Receiver<String>) {
        let dir = tempfile::tempdir().unwrap();
        let http = Arc::new(FakeHttp { offline: offline.iter().map(|s| s.to_string()).collect(), asked: Mutex::default() });
        let (sender, receiver) = mpsc::channel();
        let fetcher = ThumbnailFetcher::new(dir.path().to_path_buf(), http.clone(), move |id| {
            sender.send(id.to_string()).unwrap();
        });
        (dir, http, fetcher, receiver)
    }

    /// Waits until the fetcher's queue has run dry (the next id is answered after every earlier one).
    fn drain(fetcher: &ThumbnailFetcher, receiver: &Receiver<String>) {
        assert_eq!(fetcher.thumbnail("zzzzzzzzzzz"), Thumbnail::Pending);
        while receiver.recv_timeout(Duration::from_secs(5)).unwrap() != "zzzzzzzzzzz" {}
    }

    #[test]
    fn video_ids_are_eleven_safe_characters() {
        assert!(is_video_id("dQw4w9WgXcQ"));
        assert!(is_video_id("a_b-c_d-e_f"));
        assert!(!is_video_id(""));
        assert!(!is_video_id("short"));
        assert!(!is_video_id("../../etc/pw"));
    }

    #[test]
    fn the_thumbnail_is_youtubes_hqdefault() {
        assert_eq!(thumbnail_url("dQw4w9WgXcQ"), "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg");
    }

    #[test]
    fn the_root_dir_is_two_levels_above_the_data_json() {
        let json = Location::path("/archives/pl_P_1a2b/mp3/data.json");
        assert_eq!(root_dir_of(&json), Some(Location::path("/archives/pl_P_1a2b")));
    }

    #[test]
    fn pictures_are_told_by_their_contents() {
        assert!(looks_like_image(JPEG));
        assert!(looks_like_image(b"\x89PNG\r\n\x1a\n...."));
        assert!(looks_like_image(b"GIF89a..."));
        assert!(looks_like_image(b"BM...."));
        assert!(looks_like_image(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!looks_like_image(b"RIFF\0\0\0\0WAVEfmt "));
        assert!(!looks_like_image(b"ID3\x04 an mp3 renamed to .jpg"));
        assert!(!looks_like_image(b""));
    }

    #[test]
    fn a_picked_picture_is_copied_as_the_playlists_cover() {
        let storage = MemoryStorage::new();
        let root = Location::path("/archives/pl_P_1a2b");
        let image = Location::path("/pictures/mine.png");
        storage.create_dir_all(&Location::path("/pictures")).unwrap();
        storage.write(&image, b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert_eq!(custom_cover(&storage, &root), None);

        set_custom_cover(&storage, &root, &image).unwrap();

        let cover = custom_cover(&storage, &root).unwrap();
        assert_eq!(cover, Location::path("/archives/pl_P_1a2b/cover.jpg"));
        assert_eq!(storage.read(&cover).unwrap(), b"\x89PNG\r\n\x1a\nrest");
    }

    #[test]
    fn something_that_is_not_a_picture_is_refused_and_nothing_is_written() {
        let storage = MemoryStorage::new();
        let root = Location::path("/archives/pl_P_1a2b");
        let text = Location::path("/pictures/notes.jpg");
        storage.create_dir_all(&Location::path("/pictures")).unwrap();
        storage.write(&text, b"just text").unwrap();

        assert!(set_custom_cover(&storage, &root, &text).is_err());
        assert_eq!(custom_cover(&storage, &root), None);
    }

    #[test]
    fn a_thumbnail_is_fetched_once_and_then_served_from_disk() {
        let (dir, http, fetcher, receiver) = fetcher(&[]);
        assert_eq!(fetcher.thumbnail("dQw4w9WgXcQ"), Thumbnail::Pending);
        // Asked again while it is still on its way: not queued a second time.
        let _ = fetcher.thumbnail("dQw4w9WgXcQ");
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)).unwrap(), "dQw4w9WgXcQ");
        drain(&fetcher, &receiver);

        let file = dir.path().join("dQw4w9WgXcQ.jpg");
        assert_eq!(fetcher.thumbnail("dQw4w9WgXcQ"), Thumbnail::Ready(file.clone()));
        assert_eq!(std::fs::read(file).unwrap(), JPEG);
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked.iter().filter(|u| u.contains("dQw4w9WgXcQ")).count(), 1);
    }

    #[test]
    fn a_failed_thumbnail_is_not_asked_again_this_session() {
        let (_dir, http, fetcher, receiver) = fetcher(&["aaaaaaaaaaa"]);
        assert_eq!(fetcher.thumbnail("aaaaaaaaaaa"), Thumbnail::Pending);
        drain(&fetcher, &receiver);

        assert_eq!(fetcher.thumbnail("aaaaaaaaaaa"), Thumbnail::Unavailable);
        assert_eq!(http.asked.lock().unwrap().iter().filter(|u| u.contains("aaaaaaaaaaa")).count(), 1);
    }

    #[test]
    fn what_is_not_a_video_id_never_reaches_the_network() {
        let (_dir, http, fetcher, _receiver) = fetcher(&[]);
        assert_eq!(fetcher.thumbnail("../../etc/pw"), Thumbnail::Unavailable);
        assert!(http.asked.lock().unwrap().is_empty());
    }
}
