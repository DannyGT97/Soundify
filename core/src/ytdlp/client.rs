//! Which yt-dlp `player_client` YouTube lets through right now.
//!
//! YouTube changes often which client it accepts without a bot check, so before
//! downloading each candidate is probed against the real URL and the first one
//! that works is cached (in the settings' `working_player_client`). It is only
//! thrown away when the bot-check marker shows up during a real download.
//! Beware: the winning client can limit the real audio quality (PO Token
//! warning on `mweb`).
//!
//! Only the decisions live here (what to run, how to read the answer); running
//! the probe is the [`crate::downloader::Downloader`]'s job.

/// The clients tried, in order of preference.
pub const CANDIDATES: [&str; 4] = ["web_safari", "web", "mweb", "android"];

/// What yt-dlp prints when YouTube asks to prove it is not a bot.
pub const BOT_CHECK_MARKER: &str = "Sign in to confirm";

/// A client that only gets storyboards back: it "works" but there is nothing to download.
pub const ONLY_IMAGES_MARKER: &str = "Only images are available";

/// The yt-dlp arguments (no executable) that probe `client` against `url`
/// without downloading anything. No `--no-warnings`, on purpose: the markers
/// this probe looks for can arrive as warnings.
pub fn probe_args(url: &str, cookies_file: Option<&str>, client: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "--simulate", "--skip-download",
        "-f", "bestaudio/best",
        "--playlist-items", "1",
        "--extractor-args",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("youtube:player_client={client}"));
    if let Some(cookies) = cookies_file.filter(|c| !c.trim().is_empty()) {
        args.push("--cookies".into());
        args.push(cookies.into());
    }
    args.push(url.into());
    args
}

/// Did the probe work? Only a clean exit with neither marker in the output counts.
pub fn probe_ok(exit_code: i32, output: &[String]) -> bool {
    exit_code == 0 && !mentions_bot_check(output) && !output.iter().any(|l| l.contains(ONLY_IMAGES_MARKER))
}

/// Does this output carry the bot-check marker? Also what invalidates the cached client.
pub fn mentions_bot_check(output: &[String]) -> bool {
    output.iter().any(|l| l.contains(BOT_CHECK_MARKER))
}

/// A probe's output, short enough for one log line.
pub fn truncate(output: &str, max: usize) -> String {
    let flat = output.replace('\n', " | ");
    let flat = flat.trim();
    match flat.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn probe_args_name_the_client_and_end_with_the_url() {
        let args = probe_args("https://y/watch?v=x", None, "mweb");
        assert!(args.contains(&"youtube:player_client=mweb".to_string()));
        assert_eq!(args.last().unwrap(), "https://y/watch?v=x");
        assert!(!args.contains(&"--cookies".to_string()));
        assert!(!args.contains(&"--no-warnings".to_string()), "the markers can arrive as warnings");
    }

    #[test]
    fn cookies_go_in_only_when_there_are_any() {
        let args = probe_args("u", Some("c.txt"), "web");
        let i = args.iter().position(|a| a == "--cookies").unwrap();
        assert_eq!(args[i + 1], "c.txt");
        assert!(!probe_args("u", Some("  "), "web").contains(&"--cookies".to_string()));
    }

    #[test]
    fn a_clean_exit_with_no_markers_is_ok() {
        assert!(probe_ok(0, &lines(&["[youtube] x: Downloading webpage"])));
    }

    #[test]
    fn a_failed_exit_or_any_marker_is_not() {
        assert!(!probe_ok(1, &[]));
        assert!(!probe_ok(0, &lines(&["ERROR: Sign in to confirm you're not a bot"])));
        assert!(!probe_ok(0, &lines(&["WARNING: Only images are available for download"])));
    }

    #[test]
    fn truncate_flattens_lines_and_cuts_on_characters() {
        assert_eq!(truncate(" a\nb ", 10), "a | b");
        assert_eq!(truncate("ñññññ", 3), "ñññ…");
    }
}
