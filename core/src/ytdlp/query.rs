//! Quick yt-dlp queries that do NOT download (metadata): the arguments for each
//! and how to read its answer. Running them is the
//! [`crate::downloader::Downloader`]'s job, always with stdout and stderr
//! MERGED — reading them separately hung when yt-dlp wrote a lot to stderr
//! (reported, with YouTube asking for a bot check on every item).

use indexmap::IndexMap;

use crate::cover_art;

/// The literal phrase YouTube returns when the authenticated account (the
/// configured cookies) has no permission to see THAT particular playlist — a
/// genuine sign of "expired cookies, or the wrong account", different from a
/// 403 caused by a bot check (that one carries other text, see
/// [`super::client::BOT_CHECK_MARKER`]). Real diagnosed case: an expired
/// cookies.txt — YouTube answered literally "YouTube said: ERROR - The caller
/// does not have permission" instead of a normal bot check.
pub const COOKIE_PERMISSION_DENIED_MARKER: &str = "does not have permission";

/// The `--help` markers of the two format lists. The placeholder (" FORMAT")
/// is part of the marker on purpose, see [`parse_supported_list`].
pub const AUDIO_FORMATS_MARKER: &str = "--audio-format FORMAT";
pub const VIDEO_CONTAINERS_MARKER: &str = "--merge-output-format FORMAT";

/// Position (1-based) + title of a playlist item, exactly as yt-dlp sees it
/// through `--flat-playlist` — the title here is the only reliable source for
/// videos that later turn out to be unavailable: `--flat-playlist` reads the
/// playlist page itself (not each individual video), so it often keeps a title
/// ("Deleted video", the real cached title...) even for ids whose video can no
/// longer really be extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub position: u32,
    pub title: Option<String>,
}

// === ARGUMENTS (no executable) ===

/// The playlist's name, without downloading anything. Fast (~1-2 seconds).
pub fn playlist_title_args(url: &str, cookies_file: Option<&str>) -> Vec<String> {
    flat_first_item_args("playlist_title", url, cookies_file)
}

/// The id of the playlist's first video, for its default cover.
pub fn first_video_id_args(url: &str, cookies_file: Option<&str>) -> Vec<String> {
    flat_first_item_args("id", url, cookies_file)
}

/// The playlist's own data, without any of its items: its thumbnails are in there.
pub fn playlist_thumbnail_args(url: &str, cookies_file: Option<&str>) -> Vec<String> {
    with_cookies_and_url(&["--flat-playlist", "--playlist-items", "0", "--dump-single-json", "--no-warnings"], url, cookies_file)
}

/// The thumbnail of one video, on any site yt-dlp can read.
pub fn video_thumbnail_args(url: &str, cookies_file: Option<&str>) -> Vec<String> {
    with_cookies_and_url(&["--skip-download", "--no-playlist", "--print", "thumbnail", "--no-warnings"], url, cookies_file)
}

/// Every id that is in the playlist RIGHT NOW, with its position and title.
///
/// `--download-archive` is deliberately NOT used here, and the `SOUNDIFY_SEEN`
/// hook of the real downloads is not reused either: that hook does not fire for
/// the ids yt-dlp skips because they are already archived, so a list built on
/// it never includes what you already had — literally the opposite of what is
/// needed to detect items that left the playlist, or to know what is new.
///
/// The position is what lets the caller hand yt-dlp only the pending items
/// through `--playlist-items` — so a large playlist with just 1 pending item
/// does not have to walk (and wait out the sleep-interval of) every other one.
pub fn playlist_entries_args(url: &str, cookies_file: Option<&str>) -> Vec<String> {
    with_cookies_and_url(
        &["--flat-playlist", "--print", "%(playlist_index)s|%(id)s|%(title)s", "--no-warnings"],
        url,
        cookies_file,
    )
}

/// Asks about ONE single video, without downloading it, to find out why it
/// cannot be downloaded. Used only with the ids the playlist query returned
/// WITHOUT a title: with `--flat-playlist`, an empty title is almost always a
/// sign that YouTube no longer gives metadata for that video (checked against
/// a real playlist: 39 out of 721). Asking about all 721 would be absurd;
/// asking about those 39, once, is not.
pub fn probe_unavailable_args(video_id: &str, cookies_file: Option<&str>) -> Vec<String> {
    with_cookies_and_url(
        &["--simulate", "--no-warnings", "--print", "%(title)s"],
        &format!("https://www.youtube.com/watch?v={video_id}"),
        cookies_file,
    )
}

/// yt-dlp's own `--help`: no network, answered by the binary itself.
pub fn help_args() -> Vec<String> {
    vec!["--help".into()]
}

// === READING THE ANSWERS ===

/// The playlist title in a [`playlist_title_args`] output, if there is one.
pub fn playlist_title(output: &str) -> Option<String> {
    first_non_diagnostic_line(output).filter(|t| *t != "NA").map(str::to_string)
}

/// The video id in a [`first_video_id_args`] output, if it looks like one.
pub fn first_video_id(output: &str) -> Option<String> {
    first_non_diagnostic_line(output).filter(|id| cover_art::is_video_id(id)).map(str::to_string)
}

/// The best thumbnail in a [`playlist_thumbnail_args`] output: yt-dlp sorts them
/// from worst to best, and a line that is not JSON is one of its messages.
pub fn playlist_thumbnail(output: &str) -> Option<String> {
    let data: serde_json::Value = output.lines().find_map(|line| serde_json::from_str(line.trim()).ok().filter(serde_json::Value::is_object))?;
    let url = data.get("thumbnails")?.as_array()?.iter().rev().find_map(|t| t.get("url")?.as_str())?;
    is_web_address(url).then(|| url.to_string())
}

/// The address in a [`video_thumbnail_args`] output ("NA" when it has none).
pub fn video_thumbnail(output: &str) -> Option<String> {
    first_non_diagnostic_line(output).filter(|line| is_web_address(line)).map(str::to_string)
}

fn is_web_address(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

/// The entries in a [`playlist_entries_args`] output, in playlist order. The
/// diagnostic lines that arrive mixed in (merged output) do not match the
/// "index|id|title" format, so this parsing discards them by itself.
///
/// An empty result caused by a failure must never be treated as "the playlist
/// really is empty" — the caller has to check this before acting.
pub fn playlist_entries<'a>(lines: impl IntoIterator<Item = &'a str>) -> IndexMap<String, PlaylistEntry> {
    let mut entries = IndexMap::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "NA" {
            continue;
        }
        let Some((index_part, rest)) = trimmed.split_once('|') else { continue };
        let (id, title) = match rest.split_once('|') {
            Some((id, title)) => (id, Some(title.trim())),
            None => (rest, None),
        };
        if id.is_empty() || id == "NA" {
            continue;
        }
        // playlist_index can come back as "NA" on playlists with no fixed order —
        // with no reliable index that single item cannot be requested through
        // --playlist-items, so it is ignored here (it stays covered by
        // --download-archive in the normal full download).
        let Ok(position) = index_part.parse() else { continue };
        let title = title.filter(|t| !t.is_empty() && *t != "NA").map(str::to_string);
        entries.insert(id.to_string(), PlaylistEntry { position, title });
    }
    entries
}

/// Did a query that came back empty fail because of the cookies (not the
/// network, a malformed URL...)? Lets the caller give a more useful warning
/// than a generic "could not query".
pub fn failed_due_to_cookies(output: &str) -> bool {
    output.contains(COOKIE_PERMISSION_DENIED_MARKER)
}

/// The yt-dlp error in a [`probe_unavailable_args`] output, or `None` if the
/// video IS available. When in doubt it is `None`: NOT marking is always safer
/// than marking too much, because marking is what takes an item out of the
/// pending list for good.
pub fn unavailable_error(output: &str) -> Option<String> {
    output.lines().map(str::trim).find(|l| l.starts_with("ERROR:")).map(str::to_string)
}

/// Pulls the comma-separated list out of a line like `"--audio-format FORMAT
/// ... (currently supported: best (default), aac, alac, flac, m4a, mp3, opus,
/// vorbis, wav). ..."` — real yt-dlp `--help` output, line-wrapped by the
/// terminal, which is exactly why this cannot be one simple regex: the wrapping
/// can land "(currently" and "supported:" on two different lines, and
/// `--audio-format`'s own list carries a NESTED parenthesis ("best (default)")
/// that a naive "stop at the first )" would cut the list off at.
///
/// `option_marker` must be precise enough to match ONLY the real option's own
/// definition line — bare `--audio-format` is NOT (real bug): `--help` mentions
/// that substring earlier, inside the description of `--alias`, which sits
/// BEFORE the real definition and, being nearer to `--merge-output-format`'s
/// "supported:", made this silently return the VIDEO list when asked for audio.
/// Appending the placeholder (" FORMAT") tells the two apart — see
/// [`AUDIO_FORMATS_MARKER`]/[`VIDEO_CONTAINERS_MARKER`].
///
/// Returns the lower-cased tokens in order, or an empty list if the marker or
/// "supported:" is not found (an unexpected `--help` shape from a future
/// yt-dlp): that means "could not check", never "yt-dlp supports nothing".
pub fn parse_supported_list(help_text: &str, option_marker: &str) -> Vec<String> {
    let Some(option_idx) = help_text.find(option_marker) else { return Vec::new() };
    let Some(supported_idx) = help_text[option_idx..].find("supported:").map(|i| option_idx + i) else {
        return Vec::new();
    };

    // "(default)" is the one nested parenthesis this text ever carries —
    // stripped first so the closing paren found next is really the list's own.
    let after = help_text[supported_idx + "supported:".len()..].replace("(default)", "");
    let Some(closing) = after.find(')') else { return Vec::new() };

    let mut result: Vec<String> = Vec::new();
    // trim() alone clears the line-wrap noise too: whatever indentation/newline
    // sits before the next word ends up as THAT token's own leading whitespace.
    for token in after[..closing].split(',').map(|p| p.trim().to_lowercase()) {
        if !token.is_empty() && !result.contains(&token) {
            result.push(token);
        }
    }
    result
}

/// The app's formats that this yt-dlp really supports, in the app's order
/// (the Java `DownloadPathResolver.availableAudioFormats`). An empty
/// `supported` means "could not check", so everything is offered: hiding every
/// format because `--help` changed shape would leave nothing to download with.
pub fn filter_supported(all: &[&'static str], supported: &[String]) -> Vec<&'static str> {
    if supported.is_empty() {
        return all.to_vec();
    }
    all.iter().copied().filter(|format| supported.iter().any(|s| s.eq_ignore_ascii_case(format))).collect()
}

// === HELPERS ===

fn flat_first_item_args(field: &str, url: &str, cookies_file: Option<&str>) -> Vec<String> {
    with_cookies_and_url(
        &["--flat-playlist", "--print", field, "--playlist-items", "1", "--no-warnings"],
        url,
        cookies_file,
    )
}

fn with_cookies_and_url(base: &[&str], url: &str, cookies_file: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    if let Some(cookies) = cookies_file.filter(|c| !c.trim().is_empty()) {
        args.push("--cookies".into());
        args.push(cookies.into());
    }
    args.push(url.into());
    args
}

/// The first line of the output that looks like data rather than a yt-dlp
/// message. Needed because the output comes merged: without it, a leading
/// "[youtube] Extracting URL..." would slip through as if it were the playlist
/// title — and that title ends up being the name of the destination folder.
fn first_non_diagnostic_line(output: &str) -> Option<&str> {
    output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('[') && !l.starts_with("WARNING:") && !l.starts_with("ERROR:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_playlist_thumbnail_is_the_last_one_listed() {
        let output = "[youtube:tab] Extracting URL\n{\"title\":\"x\",\"thumbnails\":[{\"url\":\"https://i/small.jpg\"},{\"url\":\"https://i/big.jpg\"}]}\n";
        assert_eq!(playlist_thumbnail(output).as_deref(), Some("https://i/big.jpg"));
    }

    #[test]
    fn a_playlist_without_thumbnails_has_none() {
        assert_eq!(playlist_thumbnail("{\"title\":\"x\",\"thumbnails\":[]}"), None);
        assert_eq!(playlist_thumbnail("{\"title\":\"x\"}"), None);
        assert_eq!(playlist_thumbnail("ERROR: nope"), None);
        assert_eq!(playlist_thumbnail("{\"thumbnails\":[{\"url\":\"file:///etc/passwd\"}]}"), None);
    }

    #[test]
    fn the_video_thumbnail_skips_messages_and_says_none_for_na() {
        assert_eq!(video_thumbnail("[vimeo] Extracting\nhttps://i/a.jpg\n").as_deref(), Some("https://i/a.jpg"));
        assert_eq!(video_thumbnail("NA\n"), None);
    }

    // The --help fixtures are real (unmodified) excerpts of `yt-dlp --help`,
    // line-wrapped exactly as the terminal left them.

    // Note the NESTED parenthesis in "best (default)".
    const AUDIO_HELP: &str = "
    --audio-format FORMAT           Format to convert the audio to when -x is
                                    used. (currently supported: best (default),
                                    aac, alac, flac, m4a, mp3, opus, vorbis,
                                    wav). You can specify multiple rules using
";

    // Note "(currently" and "supported:" land on two DIFFERENT wrapped lines.
    const VIDEO_HELP: &str = "
    --merge-output-format FORMAT    Containers that may be used when merging
                                    formats, separated by \"/\", e.g. \"mp4/mkv\".
                                    Ignored if no merge is required. (currently
                                    supported: avi, flv, mkv, mov, mp4, webm)
";

    // --alias's description mentions "--audio-format" TWICE well before the real
    // definition, with --merge-output-format's definition in between.
    const FULL_HELP_WITH_ALIAS_COLLISION: &str = "
    --alias ALIAS OPTIONS            Create aliases for an option string. Unless
                                    an alias starts with a dash \"-\", it is
                                    prefixed with \"--\". Arguments are parsed
                                    according to the Python string formatting
                                    mini-language. E.g. --alias get-audio,-X \"-S
                                    aext:{0},abr -x --audio-format {0}\" creates
                                    options \"--get-audio\" and \"-X\" that takes an
                                    argument (ARG0) and expands to \"-S
                                    aext:ARG0,abr -x --audio-format ARG0\". All
                                    defined aliases are listed in the --help
                                    output.

    --merge-output-format FORMAT    Containers that may be used when merging
                                    formats, separated by \"/\", e.g. \"mp4/mkv\".
                                    Ignored if no merge is required. (currently
                                    supported: avi, flv, mkv, mov, mp4, webm)

    --audio-format FORMAT           Format to convert the audio to when -x is
                                    used. (currently supported: best (default),
                                    aac, alac, flac, m4a, mp3, opus, vorbis,
                                    wav). You can specify multiple rules using
";

    const AUDIO: [&str; 9] = ["best", "aac", "alac", "flac", "m4a", "mp3", "opus", "vorbis", "wav"];
    const VIDEO: [&str; 6] = ["avi", "flv", "mkv", "mov", "mp4", "webm"];

    // === SUPPORTED FORMATS ===

    #[test]
    fn audio_pulls_all_formats_out_despite_the_nested_default_parenthesis() {
        assert_eq!(parse_supported_list(AUDIO_HELP, "--audio-format"), AUDIO);
    }

    #[test]
    fn video_pulls_all_containers_out_across_the_line_wrap() {
        assert_eq!(parse_supported_list(VIDEO_HELP, "--merge-output-format"), VIDEO);
    }

    #[test]
    fn a_missing_option_marker_is_empty() {
        assert!(parse_supported_list(AUDIO_HELP, "--some-future-option").is_empty());
        assert!(parse_supported_list("", "--audio-format").is_empty());
    }

    #[test]
    fn a_marker_without_a_supported_list_is_empty_not_a_guess() {
        assert!(parse_supported_list("--audio-format FORMAT   some unrelated future text", "--audio-format").is_empty());
    }

    #[test]
    fn regression_a_bare_audio_format_marker_latches_onto_the_alias_mention_and_returns_the_video_list() {
        // The real bug: this used to be what the app asked for.
        assert_eq!(parse_supported_list(FULL_HELP_WITH_ALIAS_COLLISION, "--audio-format"), VIDEO);
    }

    #[test]
    fn regression_the_format_placeholder_skips_the_alias_mention_and_finds_the_audio_list() {
        assert_eq!(parse_supported_list(FULL_HELP_WITH_ALIAS_COLLISION, AUDIO_FORMATS_MARKER), AUDIO);
    }

    #[test]
    fn regression_the_same_precise_marker_still_finds_the_video_list() {
        assert_eq!(parse_supported_list(FULL_HELP_WITH_ALIAS_COLLISION, VIDEO_CONTAINERS_MARKER), VIDEO);
    }

    // === PLAYLIST ENTRIES ===

    #[test]
    fn reads_index_id_and_title_in_playlist_order() {
        let entries = playlist_entries(["2|bbbbbbbbbbb|Second", "1|aaaaaaaaaaa|First | with a bar"]);
        let ids: Vec<&String> = entries.keys().collect();
        assert_eq!(ids, ["bbbbbbbbbbb", "aaaaaaaaaaa"]);
        assert_eq!(entries["aaaaaaaaaaa"], PlaylistEntry { position: 1, title: Some("First | with a bar".into()) });
    }

    #[test]
    fn a_missing_or_na_title_is_none() {
        let entries = playlist_entries(["1|aaaaaaaaaaa|NA", "2|bbbbbbbbbbb|", "3|ccccccccccc"]);
        assert!(entries.values().all(|e| e.title.is_none()));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn diagnostic_lines_and_unreadable_entries_are_skipped() {
        let entries = playlist_entries([
            "[youtube:tab] Extracting URL: https://...",
            "WARNING: something",
            "NA",
            "",
            "NA|aaaaaaaaaaa|No fixed order",
            "4|NA|No id",
            "5|ddddddddddd|Kept",
        ]);
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["ddddddddddd"]);
    }

    // === SINGLE ANSWERS ===

    #[test]
    fn the_title_skips_yt_dlp_messages_regression_the_folder_named_after_a_log_line() {
        let output = "[youtube:tab] Extracting URL: https://x\nWARNING: meh\n  Mi Playlist  \n";
        assert_eq!(playlist_title(output).as_deref(), Some("Mi Playlist"));
        assert_eq!(playlist_title("NA\n"), None);
        assert_eq!(playlist_title("ERROR: nope\n"), None);
    }

    #[test]
    fn the_first_video_id_must_look_like_an_id() {
        assert_eq!(first_video_id("[x] y\ndQw4w9WgXcQ\n").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(first_video_id("not an id\n"), None);
    }

    #[test]
    fn cookie_permission_errors_are_told_apart() {
        assert!(failed_due_to_cookies("ERROR: YouTube said: ERROR - The caller does not have permission"));
        assert!(!failed_due_to_cookies("ERROR: Sign in to confirm you're not a bot"));
    }

    #[test]
    fn unavailable_error_is_the_first_error_line_or_nothing() {
        assert_eq!(
            unavailable_error("[youtube] x\nERROR: [youtube] x: Private video\nERROR: second").as_deref(),
            Some("ERROR: [youtube] x: Private video")
        );
        assert_eq!(unavailable_error("A real title\n"), None);
    }

    #[test]
    fn query_args_carry_cookies_only_when_given_and_end_with_the_url() {
        let args = playlist_entries_args("https://p", Some("c.txt"));
        assert_eq!(args.last().unwrap(), "https://p");
        assert!(args.windows(2).any(|w| w[0] == "--cookies" && w[1] == "c.txt"));
        assert!(!playlist_title_args("u", None).contains(&"--cookies".to_string()));
        assert_eq!(probe_unavailable_args("abc", None).last().unwrap(), "https://www.youtube.com/watch?v=abc");
    }

    #[test]
    fn filters_the_app_formats_by_what_yt_dlp_supports() {
        let supported = vec!["wav".to_string(), "mp3".to_string()];
        assert_eq!(filter_supported(&["MP3", "OPUS", "WAV"], &supported), ["MP3", "WAV"]);
    }

    #[test]
    fn nothing_known_about_yt_dlp_offers_every_format() {
        assert_eq!(filter_supported(&["MP3", "OPUS"], &[]), ["MP3", "OPUS"]);
    }
}
