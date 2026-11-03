//! The second pass after a playlist download in audio (the Java
//! `SafetyNetService`): for the ids that were asked for and did not come down
//! in the normal run, it retries them one by one, trying the cached
//! player_client first and then every candidate. If none works, the entry is
//! marked with `lastFailedAt`/`lastFailedReason` — not permanent, the next
//! update retries it on its own.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use regex::Regex;

use crate::archive::ArchiveStore;
use crate::downloader::{Downloader, OutputLine, RunEnd, StopFlag, YtDlpRequest};
use crate::engine::activity::ActivityLine;
use crate::location::Location;
use crate::paths;
use crate::ytdlp::cleaning::clean_output_folder;
use crate::ytdlp::client::CANDIDATES;
use crate::ytdlp::command::{self, BuildConfig, MOVED_PREFIX, SEEN_PREFIX};

static PERCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3}(?:\.\d+)?)%").unwrap());

/// A file of the playlist to retry: its saved number (if it ever had one) is
/// given back to it, so it returns to its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryItem {
    pub id: String,
    pub title: String,
    pub number: Option<String>,
}

/// A file that could not be downloaded even after trying every client, with
/// the reason as an i18n key (`failReason.*`) or yt-dlp's own text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetryOutcome {
    pub failures: Vec<Failure>,
    pub stopped: bool,
}

/// Where the pass is, for the progress row.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryProgress {
    pub title: String,
    /// 1-based.
    pub index: usize,
    pub total: usize,
    /// None while there is no real percentage yet.
    pub fraction: Option<f64>,
    /// (candidate, how many) once the cached client failed. Without it the
    /// row stays pinned at "1/1" through every attempt and it looks stuck
    /// (reported: "it just keeps retrying the whole time").
    pub attempt: Option<(usize, usize)>,
}

pub trait RetryEvents {
    fn line(&mut self, line: ActivityLine);
    fn progress(&mut self, progress: RetryProgress);
    /// The file is in the archive now.
    fn downloaded(&mut self, id: &str);
}

/// How long to wait for the file and the rename (Windows can hold a file a
/// moment after ffmpeg closes it). Zero in tests.
#[derive(Debug, Clone, Copy)]
pub struct Patience {
    pub file_polls: u32,
    pub file_poll: Duration,
    pub move_attempts: u32,
    pub move_pause: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self { file_polls: 8, file_poll: Duration::from_millis(500), move_attempts: 5, move_pause: Duration::from_millis(400) }
    }
}

pub struct SafetyNet<'a> {
    pub downloader: &'a dyn Downloader,
    pub archives: &'a ArchiveStore,
    pub json: &'a Location,
    pub folder: &'a Location,
    pub format: &'a str,
    pub cookies_file: Option<String>,
    pub ffmpeg_dir: Option<String>,
    pub cached_client: Option<String>,
    /// Where the throwaway archive goes: NEVER the playlist's real .txt.
    pub scratch: PathBuf,
    pub stop: &'a StopFlag,
    pub patience: Patience,
}

// === PUBLIC API ===

impl SafetyNet<'_> {
    /// Retries `items` one after another (never in parallel: each one sees the
    /// lastNumber the previous one left, so new numbers do not repeat).
    pub fn run(&self, items: &[RetryItem], events: &mut dyn RetryEvents) -> RetryOutcome {
        let mut outcome = RetryOutcome::default();
        for (i, item) in items.iter().enumerate() {
            if self.stop.is_stopped() {
                outcome.stopped = true;
                break;
            }
            events.line(ActivityLine::message("retry.trying", &[&item.title, &item.id]));
            match self.retry_one(item, i + 1, items.len(), events) {
                Attempted::Done | Attempted::Skipped => {}
                Attempted::Stopped => {
                    outcome.stopped = true;
                    break;
                }
                Attempted::Failed(reason) => {
                    events.line(ActivityLine::message("retry.failed", &[&item.title, &reason]));
                    if let Err(e) = self.archives.mark_failed(self.json, &item.id, Some(&item.title), &reason) {
                        log::warn!("Could not mark {} as failed: {e}", item.id);
                    }
                    outcome.failures.push(Failure { title: item.title.clone(), reason });
                }
            }
        }
        events.line(ActivityLine::message(if outcome.stopped { "retry.cancelled" } else { "retry.finished" }, &[]));
        outcome
    }
}

/// Cleans up a yt-dlp error line so it can be shown: it strips the technical
/// prefix ("ERROR: [youtube] <id>: ") and the trailing advice about
/// command-line flags, which adds nothing for someone using the app and only
/// makes the message enormous.
pub fn clean_error_line(line: &str) -> Option<String> {
    let mut s = line.trim();
    if let Some(rest) = s.strip_prefix("ERROR:") {
        s = rest.trim();
    }
    // Real format: "[youtube] <id>: message" (also "[youtube:tab]").
    if s.starts_with('[') {
        if let Some(close) = s.find("] ").filter(|&c| c > 0) {
            s = s[close + 2..].trim();
        }
        // "<id>: message" — the id is already known, repeating it is pointless.
        // What comes before is required to be a short token with no spaces, so
        // a message carrying a colon of its own is not truncated by mistake.
        if let Some(colon) = s.find(": ").filter(|&c| c > 0 && c <= 20 && !s[..c].contains(' ')) {
            s = s[colon + 2..].trim();
        }
    }
    // " Use --" covers every piece of yt-dlp advice about command-line flags
    // (--cookies, --list-formats...), of no use to someone using the app.
    for noise in [" Use --", " See  https", " See https", " You can manually pass"] {
        if let Some(cut) = s.find(noise).filter(|&c| c > 0) {
            s = s[..cut].trim();
        }
    }
    (!s.is_empty()).then(|| s.to_string())
}

/// The raw yt-dlp error translated into something that tells the user whether
/// there is anything they CAN do.
///
/// The error used to be stored as-is ("Sign in to confirm your age..."), which
/// explains the what but not the what-to-do — and these cases are retried on
/// every sync on purpose (with cookies they do work), so without saying it the
/// user sees the same failure over and over (reported). Whatever is not
/// recognised is returned untouched: an error in English beats a "could not"
/// that says nothing.
pub fn actionable_reason(raw: Option<&str>) -> String {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return "failReason.noClient".to_string();
    };
    let error = raw.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| error.contains(n));
    let key = if has(&["confirm your age", "age-restricted", "inappropriate for some users"]) {
        "failReason.ageRestricted"
    } else if has(&["not a bot"]) {
        "failReason.botCheck"
    } else if has(&["members-only", "members only", "join this channel"]) {
        "failReason.membersOnly"
    } else if has(&["private video"]) {
        "failReason.privateVideo"
    } else if has(&["available in your country", "blocked it in your country"]) {
        "failReason.geoBlocked"
    } else {
        return raw.to_string();
    };
    key.to_string()
}

// === ONE ITEM ===

enum Attempted {
    Done,
    /// Nothing to try it in (the folder is gone): neither done nor failed.
    Skipped,
    Failed(String),
    Stopped,
}

impl SafetyNet<'_> {
    fn retry_one(&self, item: &RetryItem, index: usize, total: usize, events: &mut dyn RetryEvents) -> Attempted {
        if !self.archives.storage().exists(self.folder).unwrap_or(false) {
            events.line(ActivityLine::message("retry.folderGone", &[&item.title]));
            return Attempted::Skipped;
        }
        let mut last_error: Option<String> = None;
        // First the cached client; then every candidate in turn.
        let mut client = self.cached_client.clone();
        let mut attempt: Option<usize> = None;
        loop {
            // No real percentage yet: the process has not even started.
            events.progress(RetryProgress {
                title: item.title.clone(),
                index,
                total,
                fraction: None,
                attempt: attempt.map(|a| (a + 1, CANDIDATES.len())),
            });
            match self.attempt(item, client.as_deref(), index, total, attempt, &mut last_error, events) {
                Some(RunEnd::Stopped) => return Attempted::Stopped,
                Some(RunEnd::Finished { exit_code: 0 }) => return Attempted::Done,
                _ => {}
            }
            let next = attempt.map_or(0, |a| a + 1);
            let Some(candidate) = CANDIDATES.get(next) else {
                return Attempted::Failed(actionable_reason(last_error.as_deref()));
            };
            events.line(ActivityLine::message("retry.nextClient", &[&item.title, candidate]));
            attempt = Some(next);
            client = Some(candidate.to_string());
        }
    }

    /// One download attempt. Some(Finished{0}) only when the file was captured
    /// and restored; None when yt-dlp could not even start.
    #[allow(clippy::too_many_arguments)]
    fn attempt(
        &self,
        item: &RetryItem,
        client: Option<&str>,
        index: usize,
        total: usize,
        attempt: Option<usize>,
        last_error: &mut Option<String>,
        events: &mut dyn RetryEvents,
    ) -> Option<RunEnd> {
        let temp_archive = self.scratch.join(format!("soundify_safetynet_{}.txt", item.id));
        let _ = std::fs::create_dir_all(&self.scratch);
        let _ = std::fs::remove_file(&temp_archive);

        let cfg = BuildConfig {
            ffmpeg_dir: self.ffmpeg_dir.clone(),
            archive_file: temp_archive.to_string_lossy().into_owned(),
            output_path: self.folder.to_string(),
            url: format!("https://www.youtube.com/watch?v={}", item.id),
            // A loose download of a single video: the final name is decided
            // here when renaming, not by yt-dlp.
            is_playlist: false,
            playlist_offset: 0,
            cookies_file: self.cookies_file.clone(),
            include_id_in_title: false,
            number_files: true,
            player_client: client.map(str::to_string),
            playlist_items: None,
        };
        let request = YtDlpRequest { args: command::build(self.format, &cfg) };

        let mut captured: Option<String> = None;
        let result = self.downloader.run(&request, self.stop, &mut |line| {
            let text = match line {
                OutputLine::Stdout(t) | OutputLine::Stderr(t) => t,
            };
            if let Some(payload) = text.strip_prefix(MOVED_PREFIX) {
                if let Some((_, path)) = payload.split_once('|') {
                    captured = Some(path.to_string());
                }
                return;
            }
            if text.starts_with(SEEN_PREFIX) {
                return;
            }
            // The last real error is kept so the user can be told WHY.
            if text.starts_with("ERROR:")
                && let Some(cleaned) = clean_error_line(&text)
            {
                *last_error = Some(cleaned);
            }
            // Only ONE video per process here, so the % yt-dlp reports is that
            // file's own — no need for the playlist-wide ProgressParser.
            if text.starts_with("[download]")
                && let Some(percent) = PERCENT.captures(&text).and_then(|c| c[1].parse::<f64>().ok())
            {
                events.progress(RetryProgress {
                    title: item.title.clone(),
                    index,
                    total,
                    fraction: Some(percent / 100.0),
                    attempt: attempt.map(|a| (a + 1, CANDIDATES.len())),
                });
            }
            log::info!(target: crate::logs::RETRIES, "{text}");
            events.line(ActivityLine::raw(text));
        });
        let _ = std::fs::remove_file(&temp_archive);

        let end = match result {
            Ok(end) => end,
            Err(e) => {
                log::warn!("Retry of {} could not start: {e}", item.id);
                events.line(ActivityLine::message("retry.prepareFailed", &[&item.title, &e.to_string()]));
                self.clean_leftovers();
                return None;
            }
        };
        let outcome = match (end, captured) {
            (RunEnd::Finished { exit_code: 0 }, Some(path)) => {
                self.restore(item, &path, events);
                RunEnd::Finished { exit_code: 0 }
            }
            (RunEnd::Stopped, _) => RunEnd::Stopped,
            // A clean exit with no file captured is a failed attempt too.
            _ => RunEnd::Finished { exit_code: -1 },
        };
        self.clean_leftovers();
        Some(outcome)
    }

    fn clean_leftovers(&self) {
        clean_output_folder(self.archives.storage(), self.folder, self.format);
    }

    // === RENAME TO THE MATCHING NUMBER + REGISTER IN THE ARCHIVE ===

    fn restore(&self, item: &RetryItem, captured: &str, events: &mut dyn RetryEvents) {
        let storage = self.archives.storage();
        let mut downloaded = Location::parse(captured);
        if !self.wait_until_exists(&downloaded) {
            match self.recently_modified_candidate() {
                Some(fallback) => {
                    events.line(ActivityLine::message("retry.pathGuessed", &[captured, &fallback.name()]));
                    downloaded = fallback;
                }
                None => {
                    events.line(ActivityLine::message("retry.fileMissing", &[&item.title, captured]));
                    return;
                }
            }
        }

        // Number: the one it already had (it gets its original place back), or
        // the next free one computed RIGHT NOW, so several new ids recovered in
        // a row do not repeat it. An unnumbered playlist has none to give.
        let number = if self.archives.is_numbered(self.json) {
            item.number.clone().filter(|n| !n.trim().is_empty()).or_else(|| self.archives.next_free_number(self.json))
        } else {
            None
        };
        let ext = paths::format_extension(self.format);
        let safe_title = paths::sanitize_folder_name(&item.title);
        let target_name = match number {
            Some(n) => format!("{n} - {safe_title}.{ext}"),
            None => format!("{safe_title}.{ext}"),
        };

        let result = storage.child(self.folder, &target_name).and_then(|target| {
            if target != downloaded {
                self.move_with_retry(&downloaded, &target)?;
            }
            self.archives
                .register_single(self.json, self.folder, &item.id, &target)
                .map_err(|e| std::io::Error::other(e.to_string()))
        });
        match result {
            Ok(()) => {
                events.downloaded(&item.id);
                events.line(ActivityLine::message("retry.downloadedOnRetry", &[&target_name]));
            }
            Err(e) => {
                log::error!("Failed to restore {}: {e}", item.title);
                events.line(ActivityLine::message("retry.registerFailed", &[&item.title, &e.to_string()]));
            }
        }
    }

    fn wait_until_exists(&self, file: &Location) -> bool {
        let storage = self.archives.storage();
        for _ in 0..self.patience.file_polls {
            if storage.exists(file).unwrap_or(false) {
                return true;
            }
            std::thread::sleep(self.patience.file_poll);
        }
        storage.exists(file).unwrap_or(false)
    }

    /// The newest file of the format modified within the last minute: the
    /// captured path can come mangled (encoding) while the file is right there.
    /// Only where files have a modification time to ask (a local path).
    fn recently_modified_candidate(&self) -> Option<Location> {
        let ext = format!(".{}", paths::format_extension(self.format).to_lowercase());
        let cutoff = SystemTime::now() - Duration::from_secs(60);
        self.archives
            .storage()
            .list(self.folder)
            .ok()?
            .into_iter()
            .filter(|e| !e.is_dir && e.name.to_lowercase().ends_with(&ext))
            .filter_map(|e| {
                let modified = std::fs::metadata(e.location.as_path()?).and_then(|m| m.modified()).ok()?;
                (modified >= cutoff).then_some((modified, e.location))
            })
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, location)| location)
    }

    fn move_with_retry(&self, from: &Location, to: &Location) -> std::io::Result<()> {
        let storage = self.archives.storage();
        let mut last = None;
        for _ in 0..self.patience.move_attempts.max(1) {
            if storage.exists(to).unwrap_or(false) {
                let _ = storage.remove_file(to);
            }
            match storage.rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(self.patience.move_pause);
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("move failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    // === clean_error_line ===

    #[test]
    fn strips_the_prefix_and_the_id() {
        assert_eq!(
            clean_error_line(&format!("ERROR: [youtube] {ID}: Private video. Sign in if you've been granted access to this video")).as_deref(),
            Some("Private video. Sign in if you've been granted access to this video")
        );
    }

    #[test]
    fn cuts_the_advice_about_flags() {
        let line = format!("ERROR: [youtube] {ID}: Sign in to confirm your age. Use --cookies-from-browser or --cookies for the authentication.");
        assert_eq!(clean_error_line(&line).as_deref(), Some("Sign in to confirm your age."));
    }

    #[test]
    fn cuts_the_faq_link_with_one_or_two_spaces() {
        let two = format!("ERROR: [youtube] {ID}: Video unavailable. See  https://github.com/yt-dlp/yt-dlp/wiki/FAQ");
        let one = format!("ERROR: [youtube] {ID}: Video unavailable. See https://github.com/yt-dlp/yt-dlp/wiki/FAQ");
        assert_eq!(clean_error_line(&two).as_deref(), Some("Video unavailable."));
        assert_eq!(clean_error_line(&one).as_deref(), Some("Video unavailable."));
    }

    #[test]
    fn cuts_the_manual_cookies_advice() {
        let line = format!("ERROR: [youtube] {ID}: This video is unavailable. You can manually pass cookies with --cookies");
        assert_eq!(clean_error_line(&line).as_deref(), Some("This video is unavailable."));
    }

    #[test]
    fn understands_the_tab_extractor() {
        assert_eq!(clean_error_line(&format!("ERROR: [youtube:tab] {ID}: Video unavailable")).as_deref(), Some("Video unavailable"));
    }

    #[test]
    fn keeps_a_message_whose_first_part_has_spaces() {
        // "Playlist PL123" is not an id: it must not be cut.
        assert_eq!(
            clean_error_line("ERROR: [youtube:tab] Playlist PL123: Unable to recognize playlist").as_deref(),
            Some("Playlist PL123: Unable to recognize playlist")
        );
    }

    #[test]
    fn a_line_without_extractor_stays_whole() {
        assert_eq!(
            clean_error_line("ERROR: unable to download video data: HTTP Error 403: Forbidden").as_deref(),
            Some("unable to download video data: HTTP Error 403: Forbidden")
        );
    }

    #[test]
    fn nothing_left_is_none() {
        assert_eq!(clean_error_line("ERROR:"), None);
        assert_eq!(clean_error_line("   "), None);
    }

    #[test]
    fn the_result_comes_trimmed() {
        let cleaned = clean_error_line(&format!("  ERROR: [youtube] {ID}: Sign in to confirm your age.  Use --cookies  ")).unwrap();
        assert_eq!(cleaned.trim(), cleaned);
        assert!(!cleaned.contains("--cookies"));
    }

    // === actionable_reason ===

    #[test]
    fn age_restriction_says_what_to_do() {
        let reason = actionable_reason(Some(&format!(
            "[youtube] {ID}: Sign in to confirm your age. This video may be inappropriate for some users."
        )));
        assert_eq!(reason, "failReason.ageRestricted");
    }

    #[test]
    fn bot_check_is_recognised() {
        assert_eq!(actionable_reason(Some("Sign in to confirm you're not a bot. Use --cookies-from-browser")), "failReason.botCheck");
    }

    #[test]
    fn members_private_and_country_are_recognised() {
        assert_eq!(actionable_reason(Some("Join this channel to get access to members-only content")), "failReason.membersOnly");
        assert_eq!(actionable_reason(Some("Private video. Sign in if you've been granted access")), "failReason.privateVideo");
        assert_eq!(
            actionable_reason(Some("The uploader has not made this video available in your country")),
            "failReason.geoBlocked"
        );
    }

    #[test]
    fn an_unknown_error_is_returned_untouched() {
        let odd = "Some brand new yt-dlp error nobody has seen";
        assert_eq!(actionable_reason(Some(odd)), odd);
    }

    #[test]
    fn no_error_at_all_says_no_client_worked() {
        assert_eq!(actionable_reason(None), "failReason.noClient");
        assert_eq!(actionable_reason(Some("  ")), "failReason.noClient");
    }

    // === THE PASS ===

    use crate::clock;
    use crate::downloader::ScriptedDownloader;
    use crate::storage::{MemoryStorage, Storage};
    use std::sync::Arc;

    const JSON: &str = "/archives/pl_P_000000/mp3/data.json";
    const FOLDER: &str = "/music/P (mp3)";

    #[derive(Default)]
    struct Recorded {
        keys: Vec<String>,
        attempts: Vec<Option<(usize, usize)>>,
        downloaded: Vec<String>,
    }

    impl RetryEvents for Recorded {
        fn line(&mut self, line: ActivityLine) {
            if let ActivityLine::Message { key, .. } = line {
                self.keys.push(key);
            }
        }

        fn progress(&mut self, progress: RetryProgress) {
            if progress.fraction.is_none() {
                self.attempts.push(progress.attempt);
            }
        }

        fn downloaded(&mut self, id: &str) {
            self.downloaded.push(id.to_string());
        }
    }

    fn setup() -> (ArchiveStore, Arc<MemoryStorage>, tempfile::TempDir) {
        let storage = Arc::new(MemoryStorage::new());
        storage.create_dir_all(&Location::path(FOLDER)).unwrap();
        let store = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-03 10:00"));
        let items = vec![("aaa".to_string(), "A".to_string()), ("bbb".to_string(), "B / C".to_string())];
        store.register_playlist(&Location::path(JSON), Some("https://x"), Some(&Location::path(FOLDER)), &items, true).unwrap();
        (store, storage, tempfile::tempdir().unwrap())
    }

    fn pass<'a>(
        downloader: &'a dyn Downloader,
        store: &'a ArchiveStore,
        json: &'a Location,
        folder: &'a Location,
        stop: &'a StopFlag,
        scratch: &tempfile::TempDir,
    ) -> SafetyNet<'a> {
        SafetyNet {
            downloader,
            archives: store,
            json,
            folder,
            format: "mp3",
            cookies_file: None,
            ffmpeg_dir: None,
            cached_client: Some("web".into()),
            scratch: scratch.path().to_path_buf(),
            stop,
            patience: Patience { file_polls: 0, file_poll: Duration::ZERO, move_attempts: 1, move_pause: Duration::ZERO },
        }
    }

    #[test]
    fn a_recovered_file_takes_its_saved_number_and_is_registered() {
        let (store, storage, scratch) = setup();
        let (json, folder) = (Location::path(JSON), Location::path(FOLDER));
        storage.write(&Location::path("/music/P (mp3)/B _ C.mp3"), b"x").unwrap();
        let downloader = ScriptedDownloader::new(
            vec![
                OutputLine::Stdout("[download]  50.0% of 3.00MiB".into()),
                OutputLine::Stdout(format!("{MOVED_PREFIX}bbb|/music/P (mp3)/B _ C.mp3")),
            ],
            0,
        );
        let stop = StopFlag::new();
        let mut events = Recorded::default();
        let item = RetryItem { id: "bbb".into(), title: "B / C".into(), number: Some("007".into()) };

        let outcome = pass(&downloader, &store, &json, &folder, &stop, &scratch).run(&[item], &mut events);

        assert_eq!(outcome, RetryOutcome::default());
        assert!(storage.exists(&Location::path("/music/P (mp3)/007 - B _ C.mp3")).unwrap());
        assert!(!store.ids_pending_download(&json, &["bbb".to_string()]).unwrap().contains(&"bbb".to_string()));
        assert_eq!(events.keys, ["retry.trying", "retry.downloadedOnRetry", "retry.finished"]);
        assert_eq!(events.attempts, [None], "the cached client was enough");
        assert_eq!(events.downloaded, ["bbb"], "«Ver archivos» hears of it at once");
    }

    #[test]
    fn when_every_client_fails_it_is_marked_failed_with_the_reason() {
        let (store, _, scratch) = setup();
        let (json, folder) = (Location::path(JSON), Location::path(FOLDER));
        let downloader = ScriptedDownloader::new(
            vec![OutputLine::Stderr("ERROR: [youtube] aaa: Sign in to confirm your age. Use --cookies".to_string())],
            1,
        );
        let stop = StopFlag::new();
        let mut events = Recorded::default();
        let item = RetryItem { id: "aaa".into(), title: "A".into(), number: None };

        let outcome = pass(&downloader, &store, &json, &folder, &stop, &scratch).run(&[item], &mut events);

        assert_eq!(outcome.failures, [Failure { title: "A".into(), reason: "failReason.ageRestricted".into() }]);
        let entry = store.load(&json).unwrap().entries["aaa"].clone();
        assert_eq!(entry.last_failed_reason.as_deref(), Some("failReason.ageRestricted"));
        // The cached client, then each of the candidates.
        assert_eq!(events.attempts, [None, Some((1, 4)), Some((2, 4)), Some((3, 4)), Some((4, 4))]);
        assert_eq!(events.keys.iter().filter(|k| *k == "retry.nextClient").count(), 4);
        assert_eq!(events.keys.last().map(String::as_str), Some("retry.finished"));
    }

    #[test]
    fn a_stop_ends_the_pass_as_cancelled() {
        let (store, _, scratch) = setup();
        let (json, folder) = (Location::path(JSON), Location::path(FOLDER));
        let downloader = ScriptedDownloader::new(vec![OutputLine::Stdout("x".into())], 0);
        let stop = StopFlag::new();
        stop.stop();
        let mut events = Recorded::default();
        let item = RetryItem { id: "aaa".into(), title: "A".into(), number: None };

        let outcome = pass(&downloader, &store, &json, &folder, &stop, &scratch).run(&[item], &mut events);

        assert!(outcome.stopped);
        assert_eq!(events.keys, ["retry.cancelled"]);
        assert!(store.load(&json).unwrap().entries["aaa"].last_failed_at.is_none());
    }

    #[test]
    fn a_vanished_folder_is_said_and_skipped() {
        let (store, _, scratch) = setup();
        let json = Location::path(JSON);
        let gone = Location::path("/music/gone");
        let downloader = ScriptedDownloader::new(Vec::new(), 0);
        let stop = StopFlag::new();
        let mut events = Recorded::default();
        let item = RetryItem { id: "aaa".into(), title: "A".into(), number: None };

        let outcome = pass(&downloader, &store, &json, &gone, &stop, &scratch).run(&[item], &mut events);

        assert!(outcome.failures.is_empty());
        assert_eq!(events.keys, ["retry.trying", "retry.folderGone", "retry.finished"]);
    }
}
