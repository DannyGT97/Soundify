//! The yt-dlp arguments for a download, built from a [`BuildConfig`]. Pure
//! list building: nothing is run here. The executable is not in the list: the
//! [`Downloader`](crate::downloader::Downloader) knows it (a process on desktop,
//! youtubedl-android on Android).

use crate::library::is_video_format;

// === TRACKING MARKERS ===
// Prefixes of the lines yt-dlp prints through --print so the app can capture
// each file's id without having to read it from the file name. They are
// filtered out before being shown in the Activity window.
pub const SEEN_PREFIX: &str = "SOUNDIFY_SEEN|";
pub const MOVED_PREFIX: &str = "SOUNDIFY_MOVED|";

/// Title noise removed with `--replace-in-metadata title <regex> ""`, safer than
/// `--parse-metadata` for cleaning titles.
const TITLE_NOISE: [&str; 13] = [
    r"(?i)\s*[\(\[]\s*official\s*(music\s*)?video\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*official\s*audio\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*lyrics?\s*(video)?\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*audio\s*oficial\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*video\s*oficial\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*music\s*video\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*visuali[sz]er\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*explicit\s*[\)\]]",
    r"(?i)\s*[\(\[]\s*(4k|8k|hd)\s*[\)\]]",
    r"(?i)\s*official\s*(music\s*)?video",
    r"(?i)\s*official\s*audio",
    r"(?i)\s*lyrics?\s*video",
    r"(?i)\s*music\s*video",
];

static TITLE_NOISE_REGEXES: std::sync::LazyLock<Vec<regex::Regex>> =
    std::sync::LazyLock::new(|| TITLE_NOISE.iter().filter_map(|p| regex::Regex::new(p).ok()).collect());

/// The title as yt-dlp will write it after the cleaning above, in lower case and trimmed:
/// two titles with the same result land on the same file name (Windows does not tell
/// upper from lower case), which is what must never happen in two processes at once.
pub fn cleaned_title(title: &str) -> String {
    let mut text = title.to_string();
    for re in TITLE_NOISE_REGEXES.iter() {
        text = re.replace_all(&text, "").into_owned();
    }
    text.trim().to_lowercase()
}

#[derive(Debug, Clone, Default)]
pub struct BuildConfig {
    /// The folder ffmpeg is in. None where the downloader brings its own (Android).
    pub ffmpeg_dir: Option<String>,
    pub archive_file: String,
    pub output_path: String,
    pub url: String,
    pub is_playlist: bool,
    pub playlist_offset: u32,
    pub cookies_file: Option<String>,
    pub include_id_in_title: bool,
    /// Whether to number the files ("001 - Title.mp3") or not ("Title.mp3").
    /// It comes from the local playlist's archive (`numberFiles`), NOT from
    /// Settings: it is decided when registering it and does not change
    /// afterwards. A loose download (not a playlist) is never numbered: it
    /// goes into whatever folder the user picked.
    pub number_files: bool,
    pub player_client: Option<String>,
    /// yt-dlp indexes (1-based, comma-separated, "342,344,345" for instance) to
    /// ask for instead of the whole playlist — None/blank = unfiltered, all of it
    /// is walked. Worked out by comparing the real playlist against what is
    /// already archived before starting.
    pub playlist_items: Option<String>,
    /// Fragments yt-dlp may fetch at once (`--concurrent-fragments`). 0 or 1 = the
    /// argument is not sent. Only DASH/HLS streams come in fragments, so it does
    /// nothing for a plain MP3; it is capped at [`MAX_CONCURRENT_FRAGMENTS`].
    pub concurrent_fragments: u32,
}

/// The most fragments at once the app ever asks for: every one is another request to YouTube.
pub const MAX_CONCURRENT_FRAGMENTS: u32 = 2;

/// What Ajustes' switch means: off = yt-dlp's own default (one at a time), on = the conservative value.
pub fn fragments_for(enabled: bool) -> u32 {
    if enabled { MAX_CONCURRENT_FRAGMENTS } else { 0 }
}

/// The arguments to download `cfg.url` in `format` (an audio or a video format name).
pub fn build(format: &str, cfg: &BuildConfig) -> Vec<String> {
    let mut cmd = Vec::new();
    if is_video_format(format) {
        add_video_format(&mut cmd, format);
    } else {
        add_audio_format(&mut cmd, format);
    }
    add_title_cleaning(&mut cmd);
    add_common(&mut cmd, cfg);
    add_output(&mut cmd, cfg);
    add_tracking(&mut cmd);
    push(&mut cmd, &["--no-write-thumbnail", "--clean-infojson", "--no-mtime", &cfg.url]);
    cmd
}

// === FORMAT BLOCKS ===

fn add_audio_format(cmd: &mut Vec<String>, format: &str) {
    let name = format.to_lowercase();
    push(cmd, &["-f", "bestaudio/best", "-x", "--audio-format", &name, "--audio-quality", "0"]);
    // The MPEG-4 family (M4A, AAC, ALAC) and MP3 take --embed-metadata; the
    // Xiph/Ogg ones (OPUS, VORBIS) and FLAC only --add-metadata.
    if matches!(name.as_str(), "mp3" | "m4a" | "aac" | "alac") {
        push(cmd, &["--embed-metadata"]);
    }
    push(cmd, &["--add-metadata"]);
    // WAV's own tagging is minimal (plain RIFF INFO chunks, no real album art
    // support) — conservative on purpose (no --embed-thumbnail) until a real
    // download confirms whether it is even needed here.
    if name != "wav" {
        push(cmd, &["--embed-thumbnail"]);
    }
}

/// All video containers share the exact same recipe — pick the best streams,
/// merge/remux into the requested container, embed metadata and thumbnail.
///
/// `--remux-video` is the fix for a real bug (reported: "MKV no funciona"):
/// `--merge-output-format` alone is only honoured when yt-dlp actually needs to
/// merge separate audio+video streams — if the format it picks already comes as
/// one, that flag is silently ignored and the output stays in whatever container
/// the source stream was (often mp4 or webm). `--remux-video` forces the
/// container either way.
fn add_video_format(cmd: &mut Vec<String>, format: &str) {
    let ext = format.to_lowercase();
    push(cmd, &["-f", "bv*+ba/b", "--merge-output-format", &ext, "--remux-video", &ext,
                "--embed-metadata", "--add-metadata", "--embed-thumbnail"]);
}

// === SHARED BLOCKS ===

fn add_common(cmd: &mut Vec<String>, cfg: &BuildConfig) {
    push(cmd, &["--download-archive", &cfg.archive_file]);
    if let Some(dir) = &cfg.ffmpeg_dir {
        push(cmd, &["--ffmpeg-location", dir]);
    }
    push(cmd, &["--yes-playlist", "--ignore-errors"]);
    if cfg.concurrent_fragments > 1 {
        let n = cfg.concurrent_fragments.min(MAX_CONCURRENT_FRAGMENTS);
        push(cmd, &["--concurrent-fragments", &n.to_string()]);
    }

    if cfg.is_playlist {
        if cfg.number_files {
            // Ids already archived do not consume autonumber numbering (it only
            // increments on real downloads), so starting here is safe.
            push(cmd, &["--autonumber-start", &(cfg.playlist_offset + 1).to_string()]);
        }

        // If we already know which items are new, only those indexes are asked
        // of yt-dlp — that way a big playlist with a single new file does not
        // have to walk through (nor wait out the sleep-interval of) the rest,
        // already downloaded.
        if let Some(items) = non_blank(&cfg.playlist_items) {
            push(cmd, &["--playlist-items", items]);
        }

        // No pause between items (11/11/2026, measured against the real playlist, desktop,
        // yt-dlp 2026.10.26, no cookies): 254 videos in a row without any sleep, ~5.8 s
        // each against ~10 s with `--sleep-requests 1 --min-sleep-interval 1
        // --max-sleep-interval 3`, with not one HTTP 403. What YouTube did was cut with
        // «Sign in to confirm you're not a bot» after ~300 videos in about an hour, and
        // that is a quota per hour, not a matter of pace: the pauses did not avoid it,
        // they only made reaching it take longer (the bot-check stop in `download.rs`
        // handles it). If 403s show up on bursts, `--sleep-requests 1` is the lever.
    }

    let cookies = non_blank(&cfg.cookies_file);
    if let Some(cookies) = cookies {
        push(cmd, &["--cookies", cookies]);
    }

    // The player_client normally arrives already resolved (really probed against
    // the real URL before getting here, see `client`). If none is resolved, it
    // falls back to a sensible default depending on whether there are cookies.
    let client = non_blank(&cfg.player_client)
        .unwrap_or(if cookies.is_some() { "web_safari" } else { "web" });
    push(cmd, &["--extractor-args", &format!("youtube:player_client={client}"), "--newline"]);
}

fn add_title_cleaning(cmd: &mut Vec<String>) {
    for pattern in TITLE_NOISE {
        push(cmd, &["--replace-in-metadata", "title", pattern, ""]);
    }
}

fn add_output(cmd: &mut Vec<String>, cfg: &BuildConfig) {
    let path = format!("{}{}", cfg.output_path, std::path::MAIN_SEPARATOR);
    // The id is no longer needed in the name for internal tracking (it is
    // captured through --print), so it only appears if the user asks for it.
    let name = if cfg.include_id_in_title { "%(title)s [%(id)s].%(ext)s" } else { "%(title)s.%(ext)s" };

    let template = if cfg.is_playlist {
        // An unnumbered playlist: the name is just the title, the same as in a loose download.
        let number = if cfg.number_files { "%(autonumber)03d - " } else { "" };
        format!("{path}{number}{name}")
    } else if cfg.number_files && cfg.playlist_offset > 0 {
        format!("{path}{:03} - {name}", cfg.playlist_offset + 1)
    } else {
        format!("{path}{name}")
    };
    push(cmd, &["-o", &template]);
}

/// The id → final file link, without depending on the name.
fn add_tracking(cmd: &mut Vec<String>) {
    // "video" (--print's default phase) fires when the metadata is extracted,
    // before the --download-archive is checked, so it records EVERY id of the
    // playlist even if they are already archived and skip the real download. It
    // also carries the title: that is the first moment yt-dlp knows it.
    let seen = format!("{SEEN_PREFIX}%(id)s|%(title)s");
    // "after_move" only fires for what has really been downloaded and moved to
    // its final location (after post-processing).
    let moved = format!("after_move:{MOVED_PREFIX}%(id)s|%(filepath)s");
    // --print implies --quiet and --simulate by default; that is explicitly
    // neutralised so the download and the progress stay normal.
    push(cmd, &["-O", &seen, "-O", &moved, "--no-quiet", "--no-simulate"]);
}

fn push(cmd: &mut Vec<String>, args: &[&str]) {
    cmd.extend(args.iter().map(|a| (*a).to_string()));
}

fn non_blank(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{AUDIO_FORMATS, VIDEO_FORMATS};

    fn config(is_playlist: bool, playlist_offset: u32, number_files: bool,
              cookies: Option<&str>, client: Option<&str>) -> BuildConfig {
        BuildConfig {
            ffmpeg_dir: Some("ffmpeg/bin".into()),
            archive_file: "archive.txt".into(),
            output_path: "Music".into(),
            url: "https://example.com/watch?v=abc".into(),
            is_playlist,
            playlist_offset,
            cookies_file: cookies.map(Into::into),
            number_files,
            player_client: client.map(Into::into),
            ..BuildConfig::default()
        }
    }

    /// The argument right after `flag`.
    fn after<'a>(cmd: &'a [String], flag: &str) -> &'a str {
        let i = cmd.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} missing"));
        &cmd[i + 1]
    }

    fn has(cmd: &[String], flag: &str) -> bool {
        cmd.iter().any(|a| a == flag)
    }

    #[test]
    fn a_loose_numbered_download_uses_the_folder_offset_not_autonumber() {
        let cmd = build("MP3", &config(false, 5, true, None, None));
        assert!(after(&cmd, "-o").ends_with("006 - %(title)s.%(ext)s"));
        assert!(!has(&cmd, "--autonumber-start"));
    }

    #[test]
    fn a_numbered_playlist_uses_autonumber_start_and_the_autonumber_template() {
        let cmd = build("MP3", &config(true, 5, true, None, None));
        assert_eq!(after(&cmd, "--autonumber-start"), "6");
        assert!(after(&cmd, "-o").ends_with("%(autonumber)03d - %(title)s.%(ext)s"));
    }

    #[test]
    fn an_unnumbered_playlist_adds_no_numbering_at_all() {
        let cmd = build("MP3", &config(true, 5, false, None, None));
        assert!(!has(&cmd, "--autonumber-start"));
        assert_eq!(after(&cmd, "-o"), format!("Music{}%(title)s.%(ext)s", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn regression_mkv_carries_remux_video_so_it_converts_even_when_no_merge_was_needed() {
        let cmd = build("MKV", &config(false, 0, false, None, None));
        assert_eq!(after(&cmd, "--merge-output-format"), "mkv");
        assert_eq!(after(&cmd, "--remux-video"), "mkv");
    }

    #[test]
    fn every_video_container_embeds_metadata_and_thumbnail_alike() {
        for format in VIDEO_FORMATS {
            let cmd = build(format, &config(false, 0, false, None, None));
            assert!(has(&cmd, "--embed-metadata"), "{format} should embed metadata");
            assert!(has(&cmd, "--embed-thumbnail"), "{format} should embed a thumbnail");
            assert_eq!(after(&cmd, "--merge-output-format"), format.to_lowercase());
        }
    }

    #[test]
    fn wav_stays_conservative_with_no_embed_thumbnail() {
        let cmd = build("WAV", &config(false, 0, false, None, None));
        assert_eq!(after(&cmd, "--audio-format"), "wav");
        assert!(!has(&cmd, "--embed-thumbnail"));
    }

    #[test]
    fn every_audio_format_builds_a_real_extract_audio_command() {
        for format in AUDIO_FORMATS {
            let cmd = build(format, &config(false, 0, false, None, None));
            assert!(has(&cmd, "-x"), "{format} should extract audio");
            assert_eq!(after(&cmd, "--audio-format"), format.to_lowercase());
        }
    }

    #[test]
    fn the_metadata_flags_of_each_audio_format_are_those_of_the_java_app() {
        // Table-driven now; this pins it down against the old per-format switch.
        for (format, embed_metadata) in [("MP3", true), ("OPUS", false), ("M4A", true), ("FLAC", false),
                                         ("AAC", true), ("ALAC", true), ("VORBIS", false), ("WAV", false)] {
            let cmd = build(format, &config(false, 0, false, None, None));
            assert_eq!(has(&cmd, "--embed-metadata"), embed_metadata, "{format}");
            assert!(has(&cmd, "--add-metadata"), "{format}");
        }
    }

    #[test]
    fn player_client_falls_back_to_web_without_cookies() {
        let cmd = build("MP3", &config(false, 0, false, None, None));
        assert_eq!(after(&cmd, "--extractor-args"), "youtube:player_client=web");
    }

    #[test]
    fn player_client_falls_back_to_web_safari_with_cookies() {
        let cmd = build("MP3", &config(false, 0, false, Some("cookies.txt"), None));
        assert_eq!(after(&cmd, "--extractor-args"), "youtube:player_client=web_safari");
    }

    #[test]
    fn an_explicit_player_client_always_wins_over_the_fallback() {
        let cmd = build("MP3", &config(false, 0, false, Some("cookies.txt"), Some("mweb")));
        assert_eq!(after(&cmd, "--extractor-args"), "youtube:player_client=mweb");
    }

    #[test]
    fn the_url_is_always_the_last_argument() {
        let cmd = build("MP4", &config(true, 0, true, None, None));
        assert_eq!(cmd.last().unwrap(), "https://example.com/watch?v=abc");
    }

    #[test]
    fn playlist_items_only_go_with_a_playlist_and_nothing_sleeps() {
        let mut cfg = config(true, 0, false, None, None);
        cfg.playlist_items = Some("3,5".into());
        let cmd = build("MP3", &cfg);
        assert_eq!(after(&cmd, "--playlist-items"), "3,5");
        // Measured: the pauses only slowed it down (see `add_common`).
        assert!(!has(&cmd, "--sleep-requests") && !has(&cmd, "--min-sleep-interval"));

        cfg.is_playlist = false;
        let cmd = build("MP3", &cfg);
        assert!(!has(&cmd, "--playlist-items"));
    }

    #[test]
    fn concurrent_fragments_are_not_sent_by_default() {
        let cfg = config(true, 0, true, None, None);
        assert_eq!(cfg.concurrent_fragments, 0);
        assert!(!has(&build("MP3", &cfg), "--concurrent-fragments"));
        assert!(!has(&build("MP4", &cfg), "--concurrent-fragments"));
    }

    #[test]
    fn concurrent_fragments_go_with_the_conservative_value_when_on() {
        let mut cfg = config(false, 0, false, None, None);
        cfg.concurrent_fragments = fragments_for(true);
        for format in ["MP3", "MP4"] {
            assert_eq!(after(&build(format, &cfg), "--concurrent-fragments"), "2");
        }
        assert_eq!(fragments_for(false), 0);
    }

    #[test]
    fn concurrent_fragments_are_capped_and_one_means_none() {
        let mut cfg = config(false, 0, false, None, None);
        cfg.concurrent_fragments = 16;
        assert_eq!(after(&build("MP4", &cfg), "--concurrent-fragments"), MAX_CONCURRENT_FRAGMENTS.to_string());
        cfg.concurrent_fragments = 1;
        assert!(!has(&build("MP4", &cfg), "--concurrent-fragments"));
    }
}
