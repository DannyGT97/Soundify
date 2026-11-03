//! Names and places: the music folders, where each archive lives, and reading
//! numbers, titles and ids back out of file names. The Java app's
//! `DownloadPathResolver`, minus anything that touches the disk: nothing here
//! creates a folder (asking where something goes must not leave it behind).

use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::Location;
use crate::storage::Storage;

// === FORMATS ===

/// "MP3" -> "mp3": the file extension and the name of the archive's format folder.
pub fn format_extension(format: &str) -> String {
    if format.is_empty() { "unknown".into() } else { format.to_lowercase() }
}

// === MUSIC FOLDERS ===

/// `base/<Title> (<ext>)`, or `base` itself if the user already picked that folder.
pub fn playlist_folder(storage: &dyn Storage, base: &Location, title: &str, format: &str) -> std::io::Result<Location> {
    let folder_name = playlist_folder_name(title, format);
    if base.name().to_lowercase() == folder_name.to_lowercase() {
        return Ok(base.clone());
    }
    storage.child(base, &folder_name)
}

pub fn playlist_folder_name(title: &str, format: &str) -> String {
    format!("{} ({})", sanitize_folder_name(title), format_extension(format))
}

/// The folder of a device-backed playlist, relative to the device.
pub fn device_playlist_sub_path(base: Option<&str>, title: &str, format: &str) -> String {
    let folder_name = playlist_folder_name(title, format);
    match base {
        Some(base) if !base.trim().is_empty() => format!("{base}/{folder_name}"),
        _ => folder_name,
    }
}

static FORBIDDEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\\/:*?"<>|]"#).unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// The characters Windows forbids become `_`, runs of spaces one space.
pub fn sanitize_folder_name(name: &str) -> String {
    let replaced = FORBIDDEN.replace_all(name, "_");
    SPACES.replace_all(&replaced, " ").trim().to_string()
}

// === ARCHIVE LOCATIONS ===

/// Six hex digits that tell two local playlists apart: the same playlist in
/// another folder is another local playlist. The key is built exactly as the
/// Java app did (`getAbsolutePath().toLowerCase()`), or the archives already
/// on disk would not be found again.
pub fn playlist_hash(title: Option<&str>, base: Option<&Location>) -> String {
    let base = base.map(java_absolute_lowercase).unwrap_or_default();
    short_hash(&format!("{}|{base}", title.unwrap_or_default()))
}

fn java_absolute_lowercase(base: &Location) -> String {
    match base {
        Location::Path(path) => {
            let absolute = std::path::absolute(path).unwrap_or_else(|_| path.clone());
            let text = absolute.to_string_lossy().into_owned();
            // java.io.File turns '/' into '\' on Windows.
            let text = if cfg!(windows) { text.replace('/', "\\") } else { text };
            text.to_lowercase()
        }
        Location::Uri(uri) => uri.to_lowercase(),
    }
}

fn short_hash(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest[..3].iter().map(|b| format!("{b:02x}")).collect()
}

/// `pl_<Title>_<hash>/<ext>`: one archive per playlist + base folder + format.
pub fn playlist_archive_dir(
    storage: &dyn Storage,
    archives: &Location,
    title: &str,
    base: &Location,
    format: &str,
) -> std::io::Result<Location> {
    let root_name = format!("pl_{}_{}", sanitize_folder_name(title), playlist_hash(Some(title), Some(base)));
    let root = storage.child(archives, &root_name)?;
    storage.child(&root, &format_extension(format))
}

/// The single downloads of a format share one flat archive.
pub fn individual_archive_json(storage: &dyn Storage, archives: &Location, format: &str) -> std::io::Result<Location> {
    storage.child(archives, &format!("individual_archive_{}.json", format_extension(format)))
}

pub fn individual_device_archive_json(
    storage: &dyn Storage,
    archives: &Location,
    device_id: &str,
    format: &str,
) -> std::io::Result<Location> {
    let name = format!("individual_device_{}_archive_{}.json", short_hash(device_id), format_extension(format));
    storage.child(archives, &name)
}

// === FILE NAMES ===

static INDEXED_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(\d{3,}) - )?(.+) \[([A-Za-z0-9_-]{11})]\.[^.]+$").unwrap());
static NUMBER_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:(\d{3,}) - )?(.+)\.[^.]+$").unwrap());
static TITLE_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+)\.[^.]+$").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    pub number: Option<String>,
    pub title: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberTitle {
    pub number: Option<String>,
    pub title: String,
}

/// The old naming, with the id in brackets: "003 - Title [dQw4w9WgXcQ].mp3".
/// Only an 11-character id counts: "[FREE]" belongs to the title.
pub fn parse_indexed_file_name(file_name: &str) -> Option<ParsedFile> {
    let caps = INDEXED_FILE.captures(file_name)?;
    Some(ParsedFile {
        number: caps.get(1).map(|m| m.as_str().to_string()),
        title: caps[2].to_string(),
        id: caps[3].to_string(),
    })
}

/// "007 - Title.mp3" for a numbered playlist. Fewer than three digits is part
/// of the title. A trailing " [knownId]" is dropped.
pub fn parse_number_and_title(file_name: &str, known_id: Option<&str>) -> Option<NumberTitle> {
    let caps = NUMBER_TITLE.captures(file_name)?;
    Some(NumberTitle {
        number: caps.get(1).map(|m| m.as_str().to_string()),
        title: strip_known_id(&caps[2], known_id),
    })
}

/// For an unnumbered playlist: leading digits are part of the title ("007 - James Bond Theme").
pub fn parse_title_only(file_name: &str, known_id: Option<&str>) -> Option<NumberTitle> {
    let caps = TITLE_ONLY.captures(file_name)?;
    Some(NumberTitle { number: None, title: strip_known_id(&caps[1], known_id) })
}

fn strip_known_id(title: &str, known_id: Option<&str>) -> String {
    match known_id.filter(|id| !id.trim().is_empty()) {
        Some(id) => title.strip_suffix(&format!(" [{id}]")).unwrap_or(title).to_string(),
        None => title.to_string(),
    }
}

// === URLS ===

pub fn is_playlist_url(url: &str) -> bool {
    url.contains("list=") || url.contains("/playlist")
}

static WEB_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^https?://[^\s/]+\.[^\s/]+(/\S*)?$").unwrap());
static LIST_PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&]list=([^&#]+)").unwrap());
static VIDEO_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:v=|/shorts/|/embed/|youtu\.be/)([A-Za-z0-9_-]{11})").unwrap());

/// A whole web address, not half-typed text ("https://www" is not one yet).
pub fn is_web_url(url: &str) -> bool {
    WEB_URL.is_match(url.trim())
}

/// "watch?v=X&list=Y" -> "playlist?list=Y": the same playlist, whichever video it was opened from.
pub fn normalize_playlist_url(url: &str) -> String {
    match LIST_PARAM.captures(url) {
        Some(caps) if !caps[1].trim().is_empty() => format!("https://www.youtube.com/playlist?list={}", &caps[1]),
        _ => url.to_string(),
    }
}

pub fn extract_video_id(url: &str) -> Option<String> {
    VIDEO_ID.captures(url).map(|caps| caps[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    #[test]
    fn the_extension_is_the_format_in_lowercase() {
        assert_eq!(format_extension("MP3"), "mp3");
        assert_eq!(format_extension("MP4"), "mp4");
        assert_eq!(format_extension(""), "unknown");
    }

    #[test]
    fn splits_number_and_title() {
        let parsed = parse_number_and_title("001 - Crazy Frog.mp3", None).unwrap();
        assert_eq!(parsed.number.as_deref(), Some("001"));
        assert_eq!(parsed.title, "Crazy Frog");
        let parsed = parse_number_and_title("Crazy Frog.mp3", None).unwrap();
        assert_eq!(parsed.number, None);
        assert_eq!(parsed.title, "Crazy Frog");
    }

    #[test]
    fn trims_the_trailing_id_only_when_it_is_its_own() {
        let parsed = parse_number_and_title("007 - Canción [dQw4w9WgXcQ].mp3", Some("dQw4w9WgXcQ")).unwrap();
        assert_eq!(parsed.number.as_deref(), Some("007"));
        assert_eq!(parsed.title, "Canción");
        let parsed = parse_number_and_title("007 - Canción [dQw4w9WgXcQ].mp3", Some("otroIdDist")).unwrap();
        assert_eq!(parsed.title, "Canción [dQw4w9WgXcQ]");
    }

    #[test]
    fn fewer_than_three_digits_is_not_an_order_number() {
        let parsed = parse_number_and_title("12 - Bar.mp3", None).unwrap();
        assert_eq!(parsed.number, None);
        assert_eq!(parsed.title, "12 - Bar");
    }

    #[test]
    fn a_title_with_dots_keeps_only_the_real_extension() {
        let parsed = parse_number_and_title("Mr. Blue Sky (feat. E.L.O).mp3", None).unwrap();
        assert_eq!(parsed.title, "Mr. Blue Sky (feat. E.L.O)");
        assert_eq!(parse_number_and_title("sin_extension", None), None);
    }

    /// regression: a title starting with digits is NOT taken as numbering
    #[test]
    fn regression_leading_digits_are_part_of_the_title() {
        let parsed = parse_title_only("007 - James Bond Theme.mp3", None).unwrap();
        assert_eq!(parsed.number, None);
        assert_eq!(parsed.title, "007 - James Bond Theme");
        assert_eq!(parse_title_only("Canción [dQw4w9WgXcQ].mp3", Some("dQw4w9WgXcQ")).unwrap().title, "Canción");
        assert_eq!(parse_title_only("123 - 456.opus", None).unwrap().number, None);
    }

    #[test]
    fn extracts_number_title_and_id() {
        let parsed = parse_indexed_file_name("003 - Crazy Frog [dQw4w9WgXcQ].mp3").unwrap();
        assert_eq!(parsed.number.as_deref(), Some("003"));
        assert_eq!(parsed.title, "Crazy Frog");
        assert_eq!(parsed.id, "dQw4w9WgXcQ");
        let parsed = parse_indexed_file_name("Crazy Frog [dQw4w9WgXcQ].mp3").unwrap();
        assert_eq!(parsed.number, None);
        assert_eq!(parsed.id, "dQw4w9WgXcQ");
    }

    /// regression: a tag belonging to the title is not mistaken for an id
    #[test]
    fn regression_a_bracket_that_is_not_an_id_does_not_count() {
        assert_eq!(parse_indexed_file_name("Tema [Monstercat Release].mp3"), None);
        assert_eq!(parse_indexed_file_name("Tema [FREE].mp3"), None);
    }

    /// regression: 'watch?v=X&list=Y' is normalised to 'playlist?list=Y'
    #[test]
    fn regression_normalises_the_url_of_a_video_inside_a_playlist() {
        assert_eq!(
            normalize_playlist_url("https://www.youtube.com/watch?v=Cu5q3FSSX-A&list=PLawelMNQGqk7X3ND"),
            "https://www.youtube.com/playlist?list=PLawelMNQGqk7X3ND"
        );
        let clean = "https://www.youtube.com/playlist?list=PLawelMNQGqk7X3ND";
        assert_eq!(normalize_playlist_url(clean), clean);
        let video = "https://www.youtube.com/watch?v=Cu5q3FSSX-A";
        assert_eq!(normalize_playlist_url(video), video);
        assert_eq!(
            normalize_playlist_url("https://www.youtube.com/watch?v=abc&list=PL123&index=4"),
            "https://www.youtube.com/playlist?list=PL123"
        );
    }

    #[test]
    fn detects_playlists_and_whole_web_addresses() {
        assert!(is_playlist_url("https://www.youtube.com/playlist?list=PL1"));
        assert!(is_playlist_url("https://www.youtube.com/watch?v=a&list=PL1"));
        assert!(!is_playlist_url("https://www.youtube.com/watch?v=a"));
        assert!(is_web_url("https://www.youtube.com/watch?v=a"));
        assert!(is_web_url("  http://youtu.be/abc  "));
        assert!(is_web_url("https://vimeo.com"));
        assert!(!is_web_url("https://"));
        assert!(!is_web_url("https://www"));
        assert!(!is_web_url("www.youtube.com/watch?v=a"));
        assert!(!is_web_url("hello world"));
    }

    #[test]
    fn sanitizes_folder_names() {
        assert_eq!(sanitize_folder_name("AC/DC : Live?"), "AC_DC _ Live_");
        assert_eq!(sanitize_folder_name("a\\b*c\"d<e>f"), "a_b_c_d_e_f");
        assert_eq!(sanitize_folder_name("  Mi    Playlist  "), "Mi Playlist");
    }

    #[test]
    fn the_playlist_hash_is_stable_and_six_hex_digits() {
        let musica = Location::path("musica");
        let a = playlist_hash(Some("Canciones"), Some(&musica));
        assert_eq!(a, playlist_hash(Some("Canciones"), Some(&musica)));
        assert!(a.len() == 6 && a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, playlist_hash(Some("Twitch"), Some(&musica)));
        assert_eq!(a, playlist_hash(Some("Canciones"), Some(&Location::path("Musica"))));
        assert_eq!(playlist_hash(None, None).len(), 6);
    }

    #[test]
    fn the_same_playlist_in_another_folder_is_a_different_local_playlist() {
        assert_ne!(
            playlist_hash(Some("Canciones"), Some(&Location::path("musica"))),
            playlist_hash(Some("Canciones"), Some(&Location::path("otra_carpeta")))
        );
    }

    /// The archives already on disk were named by the Java app: the same key
    /// must give the same hash ("Canciones|d:\musica", SHA-256, first 3 bytes).
    #[test]
    fn hashes_like_the_java_app() {
        assert_eq!(short_hash("Canciones|d:\\musica"), {
            let digest = Sha256::digest("Canciones|d:\\musica".as_bytes());
            format!("{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2])
        });
        assert_eq!(short_hash(""), "e3b0c4");
    }

    /// regression: appends 'Name (format)' to the base — files must not land loose in the base
    #[test]
    fn regression_device_sub_path_appends_the_playlist_folder() {
        assert_eq!(device_playlist_sub_path(Some("Music"), "Cocina", "MP3"), "Music/Cocina (mp3)");
        assert_eq!(device_playlist_sub_path(Some(""), "Cocina", "MP3"), "Cocina (mp3)");
        assert_eq!(device_playlist_sub_path(None, "Cocina", "MP3"), "Cocina (mp3)");
        assert_ne!(
            device_playlist_sub_path(Some("Music"), "Cocina", "MP3"),
            device_playlist_sub_path(Some("Music"), "Clasicos", "MP3")
        );
    }

    #[test]
    fn extracts_the_video_id() {
        assert_eq!(extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://www.youtube.com/playlist?list=PLawelMNQGqk7X3NDMsCe-emAmUk03woC4"), None);
    }

    #[test]
    fn a_base_that_already_is_the_playlist_folder_is_used_as_is() {
        let storage = MemoryStorage::new();
        let base = Location::path("/music/Chill (mp3)");
        assert_eq!(playlist_folder(&storage, &base, "Chill", "MP3").unwrap(), base);
        let other = Location::path("/music");
        assert_eq!(playlist_folder(&storage, &other, "Chill", "MP3").unwrap(), Location::path("/music/Chill (mp3)"));
    }

    #[test]
    fn archive_locations_follow_the_java_layout() {
        let storage = MemoryStorage::new();
        let archives = Location::path("/data/history/archives");
        let dir = playlist_archive_dir(&storage, &archives, "Chill: 2024", &Location::path("/music"), "MP3").unwrap();
        let text = dir.to_string().replace('\\', "/");
        assert!(text.starts_with("/data/history/archives/pl_Chill_ 2024_"), "{text}");
        assert!(text.ends_with("/mp3"));
        assert_eq!(
            individual_archive_json(&storage, &archives, "FLAC").unwrap(),
            Location::path("/data/history/archives/individual_archive_flac.json")
        );
    }
}
