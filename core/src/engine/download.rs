//! A whole download, from start to end (the Java `DownloadCoordinator`): work
//! out the folder and the archive, sync with the source playlist, pick a
//! player_client, run yt-dlp following the ids it prints (`--print` hooks),
//! chain the second pass ([`SafetyNet`]) and resync when it ends.
//!
//! It touches no UI: everything worth showing goes out through
//! [`DownloadEvents`], and it runs on the calling thread until it ends.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::archive::{ArchiveStore, io as archive_io};
use crate::cover_embed;
use crate::downloader::{Downloader, OutputLine, RunEnd, StopFlag, YtDlpRequest};
use crate::engine::activity::ActivityLine;
use crate::engine::safety_net::{Patience, RetryEvents, RetryItem, RetryProgress, SafetyNet};
use crate::engine::tasks::Step;
use crate::library::AUDIO_FORMATS;
use crate::location::Location;
use crate::model::UserSettings;
use crate::paths;
use crate::settings;
use crate::storage::Storage;
use crate::task_history::{FAILED, OK, WARNING};
use crate::ytdlp::cleaning::{clean_leftovers_of, clean_output_folder};
use crate::ytdlp::client::{self, BOT_CHECK_MARKER, CANDIDATES, ONLY_IMAGES_MARKER};
use crate::ytdlp::command::{self, BuildConfig, MOVED_PREFIX, SEEN_PREFIX};
use crate::ytdlp::progress::ProgressParser;
use crate::ytdlp::query::{self, PlaylistEntry};
use crate::ytdlp::unavailable;

/// "ERROR: [youtube] <id>: <reason>" — a video yt-dlp gave up on mid-run.
static UNAVAILABLE_ERROR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ERROR: \[youtube\] ([A-Za-z0-9_-]{11}): (.+)$").unwrap());

/// Only these mean "gone for good". Format failures ("Requested format is not
/// available") and other one-off errors deliberately do NOT count: those can
/// fix themselves (another client, another day).
const UNAVAILABLE_MARKERS: [&str; 3] = ["Video unavailable", "Private video", "This video has been removed"];

// === WHAT TO DOWNLOAD ===

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// «Descarga por URL»: one loose video.
    Single {
        url: String,
        format: String,
        base: Location,
        /// The picture picked for it: replaces the video's thumbnail in the file.
        cover: Option<PathBuf>,
    },
    /// A playlist already registered (see `Engine::create_playlist`), updated
    /// from the Gestor (`known_title`).
    Playlist {
        url: String,
        format: String,
        base: Location,
        known_title: Option<String>,
        from_manager: bool,
    },
}

impl Job {
    /// «Actualizar» of a Gestor row: everything comes from its archive. None
    /// when the archive lacks what a download needs (no url, no folder).
    pub fn update(archives: &ArchiveStore, json: &Location) -> Option<Job> {
        let data = archives.load(json).ok()?;
        let url = data.url.filter(|u| !u.trim().is_empty())?;
        let folder = Location::parse(data.folder.as_deref()?);
        let base = archive_io::parent_of(&folder)?;
        let format = archive_io::format_of(json)?.to_uppercase();
        let known_title = archives
            .list_local_playlists()
            .into_iter()
            .find(|p| p.formats.iter().any(|f| &f.json == json))
            .map(|p| p.title);
        Some(Job::Playlist { url, format, base, known_title, from_manager: true })
    }

    fn parts(&self) -> (&str, &str, &Location) {
        match self {
            Job::Single { url, format, base, .. } | Job::Playlist { url, format, base, .. } => (url, format, base),
        }
    }

    fn is_from_manager(&self) -> bool {
        matches!(self, Job::Playlist { from_manager: true, .. })
    }
}

// === WHAT IT TELLS ===

pub trait DownloadEvents {
    fn line(&mut self, line: ActivityLine);
    /// The playlist's real title, or the file's for a single download.
    fn title(&mut self, title: &str);
    /// The archive this run works on (the Gestor row that is busy).
    fn archive(&mut self, json: &Location);
    fn step(&mut self, step: Step, total: u32);
    fn progress(&mut self, fraction: f64);
    fn retry(&mut self, progress: RetryProgress);
    /// A file came down and is already in the archive: «Ver archivos» can show it.
    fn downloaded(&mut self, id: &str);
    /// How it went, worth a notice: `status` is a `task_history` status.
    fn notice(&mut self, status: &'static str, key: &str, args: &[&str]);
}

/// Everything a run needs from outside.
pub struct Engine<'a> {
    pub downloader: &'a dyn Downloader,
    pub archives: &'a ArchiveStore,
    pub settings_file: &'a Location,
    pub ffmpeg_dir: Option<String>,
    /// Throwaway files of the second pass.
    pub scratch: PathBuf,
    pub stop: &'a StopFlag,
    pub patience: Patience,
}

/// What one run learnt about its playlist.
struct PlaylistState {
    entries: IndexMap<String, PlaylistEntry>,
    /// The ids this run asks for: every entry not downloaded yet.
    pending: Vec<String>,
}

/// The picked picture into a downloaded file. A failure only costs the picture:
/// the file is there, with the thumbnail yt-dlp gave it.
fn embed_picked_cover(file: &Path, image: &Path, format: &str, events: &mut dyn DownloadEvents) {
    if !cover_embed::can_embed(format) {
        return;
    }
    if let Err(e) = cover_embed::embed_cover(file, image) {
        log::warn!("Could not put {} into {}: {e}", image.display(), file.display());
        events.line(ActivityLine::message("activity.cover.notEmbedded", &[]));
    }
}

/// What the lines of the real download leave behind.
#[derive(Default)]
struct Captures {
    captured: Vec<(String, Location)>,
    seen_titles: HashMap<String, String>,
    client_invalidated: bool,
}

// === PUBLIC API ===

impl Engine<'_> {
    pub fn run(&self, job: &Job, events: &mut dyn DownloadEvents) {
        let (url, _, base) = job.parts();
        if url.trim().is_empty() || base.to_string().trim().is_empty() {
            events.line(ActivityLine::message("activity.error.emptyInput", &[]));
            events.notice(FAILED, "activity.error.emptyInput", &[]);
            return;
        }
        if let Err(e) = self.download(job, events) {
            log::error!("Download failed: {e}");
            let error = e.to_string();
            events.line(ActivityLine::message("activity.error.startFailed", &[&error]));
            events.notice(FAILED, "activity.error.startFailed", &[&error]);
        }
    }
}

// === THE RUN ===

impl Engine<'_> {
    fn download(&self, job: &Job, events: &mut dyn DownloadEvents) -> io::Result<()> {
        let storage = self.archives.storage();
        let mut settings = settings::load(storage, self.settings_file);
        let cookies = Some(settings.cookies_file.clone()).filter(|c| !c.trim().is_empty());
        let cookies = cookies.as_deref();
        let (raw_url, format, base) = job.parts();
        let from_manager = job.is_from_manager();

        // A URL like "watch?v=X&list=Y" makes yt-dlp's quick queries see only
        // that one video (real bug, reproduced by hand): normalised before the
        // first query, so everything after uses the clean form.
        let (url, title) = match job {
            Job::Single { .. } => {
                events.line(ActivityLine::message("activity.download.single", &[]));
                (raw_url.trim().to_string(), None)
            }
            Job::Playlist { known_title, .. } => {
                let url = paths::normalize_playlist_url(raw_url.trim());
                let title = self.resolve_title(&url, known_title.as_deref(), format, cookies, events);
                events.title(&title);
                (url, Some(title))
            }
        };

        let (folder, json) = match &title {
            Some(title) => {
                let dir = paths::playlist_archive_dir(storage, self.archives.archives(), title, base, format)?;
                storage.create_dir_all(&dir)?;
                (paths::playlist_folder(storage, base, title, format)?, storage.child(&dir, archive_io::DATA_JSON)?)
            }
            // Straight into the folder picked: a single download is a loose
            // file, and the Java app's "soundify_<ext>" folder read as a
            // playlist the user never made (reported).
            None => {
                storage.create_dir_all(self.archives.archives())?;
                (base.clone(), paths::individual_archive_json(storage, self.archives.archives(), format)?)
            }
        };
        storage.create_dir_all(&folder)?;
        events.archive(&json);

        let offset = self.archives.prepare(&json, &folder, format)?;
        // Numbering is the local playlist's own decision, fixed when it was
        // registered. A single download is never numbered: it lands in any
        // folder at all, where "001 - " would count the user's own files.
        let number_files = title.is_some() && self.archives.is_numbered(&json);
        if number_files && offset > 0 {
            events.line(ActivityLine::message("activity.folder.resuming", &[&format!("{:03}", offset + 1)]));
        }

        // A single video already archived: said out loud instead of letting
        // yt-dlp skip it silently (it looked as if nothing was happening).
        if title.is_none()
            && let Some(id) = paths::extract_video_id(&url)
            && self.archives.known_ids(&json)?.contains(&id)
        {
            events.line(ActivityLine::message("activity.alreadyDownloaded.single", &[format]));
            events.notice(OK, "activity.alreadyDownloaded.single", &[format]);
            return Ok(());
        }

        let mut playlist = None;
        let mut playlist_items = None;
        if let Some(title) = &title {
            events.line(ActivityLine::message("activity.playlist.checking", &[]));
            let (entries, output) = self.fetch_entries(&url, cookies);
            if entries.is_empty() {
                let cookies_expired = query::failed_due_to_cookies(&output);
                let key = if cookies_expired { "activity.sync.cookiesExpired" } else { "activity.sync.failed" };
                events.line(ActivityLine::message(key, &[]));
                if from_manager {
                    let key = if cookies_expired { "activity.manager.cookiesExpired" } else { "activity.manager.checkFailed" };
                    events.notice(FAILED, key, &[title]);
                    return Ok(());
                }
            } else {
                self.sync_with_playlist(&json, &url, &entries)?;
                self.mark_unavailable_from_playlist(&json, &entries, cookies, from_manager, events)?;
                let ids: Vec<String> = entries.keys().cloned().collect();
                let pending = self.archives.ids_pending_download(&json, &ids)?;
                if pending.is_empty() {
                    events.line(ActivityLine::message("activity.alreadyDownloaded.playlist", &[format, title]));
                    if from_manager {
                        events.notice(OK, "activity.manager.upToDate", &[title]);
                    } else {
                        events.notice(OK, "activity.alreadyDownloaded.playlist", &[format, title]);
                    }
                    return Ok(());
                }
                // Only what is missing: a large playlist with one pending item
                // does not walk (and wait out the sleep-interval of) the rest.
                if pending.len() < entries.len() {
                    let mut positions: Vec<u32> = pending.iter().filter_map(|id| entries.get(id)).map(|e| e.position).collect();
                    positions.sort_unstable();
                    playlist_items = Some(positions.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
                    let (n, total) = (pending.len().to_string(), entries.len().to_string());
                    events.line(ActivityLine::message("activity.pending.partial", &[&n, &total]));
                }
                playlist = Some(PlaylistState { entries, pending });
            }
        }
        if self.stop.is_stopped() {
            return Ok(());
        }

        let Some(player_client) = self.player_client(&url, cookies, &mut settings, events) else {
            events.line(ActivityLine::message("activity.botcheck.blocked", &[]));
            events.notice(FAILED, "activity.botcheck.notice", &[]);
            return Ok(());
        };
        if self.stop.is_stopped() {
            return Ok(());
        }

        let txt = archive_io::txt_file_for(&json).ok_or_else(|| io::Error::other("archive without a work file"))?;
        let cfg = BuildConfig {
            ffmpeg_dir: self.ffmpeg_dir.clone(),
            archive_file: txt.to_string(),
            output_path: folder.to_string(),
            url: url.clone(),
            is_playlist: title.is_some(),
            playlist_offset: u32::try_from(offset).unwrap_or(0),
            cookies_file: cookies.map(str::to_string),
            include_id_in_title: settings.include_id_in_title_name,
            number_files,
            player_client: Some(player_client),
            playlist_items,
        };

        let mut parser = ProgressParser::new();
        let run_total = match &playlist {
            Some(p) => {
                parser.lock_total_items((p.entries.len() - p.pending.len()) as i64, p.pending.len() as i64);
                p.pending.len()
            }
            None if title.is_none() => {
                parser.lock_total_items(0, 1);
                1
            }
            // A playlist whose check failed: yt-dlp's own total is the right one.
            None => 0,
        };
        events.step(Step::Download, u32::try_from(run_total).unwrap_or(u32::MAX));

        let request = YtDlpRequest { args: command::build(format, &cfg) };
        let mut captures = Captures::default();
        let single = title.is_none();
        let result = self.downloader.run(&request, self.stop, &mut |line| {
            let text = match line {
                OutputLine::Stdout(t) | OutputLine::Stderr(t) => t,
            };
            self.on_line(text, &json, &folder, single, &mut settings, &mut parser, &mut captures, events);
        });

        let end = match result {
            Ok(end) => end,
            Err(e) => {
                self.commit(&json, &folder, format, &captures, events);
                return Err(e);
            }
        };
        if end == RunEnd::Stopped {
            events.line(ActivityLine::message("activity.stop.download", &[]));
            self.commit(&json, &folder, format, &captures, events);
            self.archives.touch_last_activity(&json)?;
            clean_up(storage, &folder, format, single, &captures);
            return Ok(());
        }

        self.commit(&json, &folder, format, &captures, events);
        if let Job::Single { cover: Some(image), .. } = job {
            for (_, file) in &captures.captured {
                if let Some(path) = file.as_path() {
                    embed_picked_cover(path, image, format, events);
                }
            }
        }
        let items = self.retry_items(&json, format, playlist.as_ref(), &captures);
        clean_up(storage, &folder, format, single, &captures);
        if !items.is_empty() {
            let stopped = self.second_pass(&json, &folder, format, cookies, &settings, &items, events);
            if stopped {
                events.line(ActivityLine::message("activity.stop.safetyNet", &[]));
                self.archives.touch_last_activity(&json)?;
                return Ok(());
            }
        }
        self.archives.touch_last_activity(&json)?;

        // The download may have taken long: the playlist may have changed.
        if title.is_some() {
            events.line(ActivityLine::message("activity.playlist.checking", &[]));
            let (entries, _) = self.fetch_entries(&url, cookies);
            if entries.is_empty() {
                events.line(ActivityLine::message("activity.sync.skipped", &[]));
            } else {
                self.sync_with_playlist(&json, &url, &entries)?;
                events.line(ActivityLine::message("activity.sync.done", &[]));
            }
        }
        Ok(())
    }

    // === QUERIES ===

    /// One quick query: its exit code and every line (stdout and stderr
    /// merged). None when it was stopped or could not even start.
    fn query(&self, args: Vec<String>) -> Option<(i32, Vec<String>)> {
        let mut lines = Vec::new();
        let result = self.downloader.run(&YtDlpRequest { args }, self.stop, &mut |line| {
            lines.push(match line {
                OutputLine::Stdout(t) | OutputLine::Stderr(t) => t,
            });
        });
        match result {
            Ok(RunEnd::Finished { exit_code }) => Some((exit_code, lines)),
            Ok(RunEnd::Stopped) => None,
            Err(e) => {
                log::warn!("yt-dlp query could not start: {e}");
                None
            }
        }
    }

    fn resolve_title(
        &self,
        url: &str,
        known_title: Option<&str>,
        format: &str,
        cookies: Option<&str>,
        events: &mut dyn DownloadEvents,
    ) -> String {
        let output = self.query(query::playlist_title_args(url, cookies)).map(|(_, l)| l.join("\n")).unwrap_or_default();
        if let Some(title) = query::playlist_title(&output) {
            events.line(ActivityLine::message("activity.playlist.detected", &[&title]));
            return title;
        }
        // An UPDATE of a known playlist: a one-off failure must NOT fall back to
        // the generic name, which would make a DIFFERENT archive (the title is
        // part of the folder hash) instead of updating the usual one (reported).
        if let Some(known) = known_title.filter(|t| !t.trim().is_empty()) {
            events.line(ActivityLine::message("activity.playlist.titleFailed", &[known]));
            return known.to_string();
        }
        // Said loud and clear (it used to be silent), so it can be stopped and
        // the cookies or the access checked before a generic archive is born.
        let generic = format!("soundify_playlist_{}", paths::format_extension(format));
        let key = if query::failed_due_to_cookies(&output) {
            "activity.playlist.titleCookiesExpired"
        } else {
            "activity.playlist.titleNoAccess"
        };
        events.line(ActivityLine::message(key, &[&generic]));
        generic
    }

    /// Every id in the playlist RIGHT NOW, plus the raw output to tell why an
    /// empty answer was empty. Empty is a failure, never "the playlist is empty".
    fn fetch_entries(&self, url: &str, cookies: Option<&str>) -> (IndexMap<String, PlaylistEntry>, String) {
        let Some((_, lines)) = self.query(query::playlist_entries_args(url, cookies)) else {
            return (IndexMap::new(), String::new());
        };
        (query::playlist_entries(lines.iter().map(String::as_str)), lines.join("\n"))
    }

    /// The cached client, or the first candidate that passes the probe (then
    /// cached). None when every candidate is blocked.
    fn player_client(
        &self,
        url: &str,
        cookies: Option<&str>,
        settings: &mut UserSettings,
        events: &mut dyn DownloadEvents,
    ) -> Option<String> {
        if let Some(cached) = settings.working_player_client.clone().filter(|c| !c.trim().is_empty()) {
            return Some(cached);
        }
        for candidate in CANDIDATES {
            events.line(ActivityLine::message("client.probing", &[candidate]));
            let (exit_code, output) = self.query(client::probe_args(url, cookies, candidate))?;
            if client::probe_ok(exit_code, &output) {
                log::info!("Working YouTube client found: {candidate}");
                events.line(ActivityLine::message("client.ok", &[candidate]));
                settings.working_player_client = Some(candidate.to_string());
                self.save_settings(settings);
                return Some(candidate.to_string());
            }
            // One line per candidate: without it, "none worked" does not say
            // whether it was a bot-check, the cookies or the network.
            log::warn!(
                "Client '{candidate}' rejected (exit={exit_code}, botCheck={}): {}",
                client::mentions_bot_check(&output),
                client::truncate(&output.join("\n"), 500)
            );
        }
        events.line(ActivityLine::message("client.noneWorked", &[]));
        None
    }

    fn save_settings(&self, settings: &UserSettings) {
        if let Err(e) = settings::save(self.archives.storage(), self.settings_file, settings) {
            log::warn!("Could not save the player client: {e}");
        }
    }

    // === SYNC WITH THE SOURCE PLAYLIST ===

    fn sync_with_playlist(&self, json: &Location, url: &str, entries: &IndexMap<String, PlaylistEntry>) -> io::Result<()> {
        let ids: HashSet<String> = entries.keys().cloned().collect();
        self.archives.sync_playlist_membership(json, Some(url), &ids)?;
        let titles: HashMap<String, String> =
            entries.iter().filter_map(|(id, e)| e.title.clone().map(|t| (id.clone(), t))).collect();
        self.archives.refresh_known_titles(json, &titles)?;
        let items: Vec<(String, String)> =
            entries.iter().map(|(id, e)| (id.clone(), e.title.clone().unwrap_or_default())).collect();
        self.archives.register_new_playlist_items(json, &items)?;
        let order: HashMap<String, i32> =
            entries.iter().map(|(id, e)| (id.clone(), i32::try_from(e.position).unwrap_or(i32::MAX))).collect();
        self.archives.refresh_playlist_order(json, &order)
    }

    /// The ids `--flat-playlist` gave no title for are almost always gone from
    /// YouTube: asked about one by one (once), and marked so they stop counting
    /// as pending.
    fn mark_unavailable_from_playlist(
        &self,
        json: &Location,
        entries: &IndexMap<String, PlaylistEntry>,
        cookies: Option<&str>,
        from_manager: bool,
        events: &mut dyn DownloadEvents,
    ) -> io::Result<()> {
        let untitled: Vec<String> = entries.iter().filter(|(_, e)| e.title.is_none()).map(|(id, _)| id.clone()).collect();
        let to_check = self.archives.ids_needing_availability_check(json, &untitled)?;
        if to_check.is_empty() {
            return Ok(());
        }
        events.line(ActivityLine::message("activity.unavailable.checking", &[&to_check.len().to_string()]));
        let mut marked = 0;
        for id in &to_check {
            if self.stop.is_stopped() {
                break;
            }
            let Some((_, output)) = self.query(query::probe_unavailable_args(id, cookies)) else { break };
            let Some(error) = query::unavailable_error(&output.join("\n")) else { continue };
            // Only what will not come back on its own. With the session blocked
            // by YouTube every video fails, the good ones too: marking then
            // would take the WHOLE playlist out of the pending list.
            if !unavailable::is_permanent(Some(&error)) {
                events.line(ActivityLine::message("activity.unavailable.rateLimited", &[id]));
                break;
            }
            self.archives.mark_unavailable(json, id, &error)?;
            events.line(ActivityLine::message("activity.unavailable.marked", &[id, unavailable::reason_key(Some(&error))]));
            marked += 1;
        }
        if marked > 0 {
            let n = marked.to_string();
            events.line(ActivityLine::message("activity.unavailable.summary", &[&n]));
            if from_manager {
                events.notice(OK, "activity.unavailable.summary", &[&n]);
            }
        }
        Ok(())
    }

    // === THE LINES OF THE REAL DOWNLOAD ===

    #[allow(clippy::too_many_arguments)]
    fn on_line(
        &self,
        text: String,
        json: &Location,
        folder: &Location,
        single: bool,
        settings: &mut UserSettings,
        parser: &mut ProgressParser,
        captures: &mut Captures,
        events: &mut dyn DownloadEvents,
    ) {
        // The cached client stopped working: forgotten, so the next download
        // probes again instead of failing the same way.
        if !captures.client_invalidated && (text.contains(BOT_CHECK_MARKER) || text.contains(ONLY_IMAGES_MARKER)) {
            captures.client_invalidated = true;
            if let Some(previous) = settings.working_player_client.take() {
                self.save_settings(settings);
                events.line(ActivityLine::message("activity.client.stoppedWorking", &[&previous]));
            }
        }
        if let Some(payload) = text.strip_prefix(MOVED_PREFIX) {
            if let Some((id, path)) = payload.split_once('|') {
                let file = Location::parse(path);
                // Saved at once: if the process dies, only the file halfway is lost.
                match self.archives.register_single(json, folder, id, &file) {
                    Ok(()) => events.downloaded(id),
                    Err(e) => log::warn!("Could not register {id}: {e}"),
                }
                captures.captured.push((id.to_string(), file));
            }
            return;
        }
        if let Some(payload) = text.strip_prefix(SEEN_PREFIX) {
            if let Some((id, title)) = payload.split_once('|') {
                captures.seen_titles.insert(id.to_string(), title.to_string());
                events.line(ActivityLine::raw(format!("🎵 {title}")));
                if single {
                    events.title(title);
                }
            }
            return;
        }
        if let Some(fraction) = parser.parse(&text) {
            events.progress(fraction);
        }
        self.handle_unavailable_line(&text, json, events);
        log::info!(target: crate::logs::ACTIVITY, "{text}");
        events.line(ActivityLine::raw(text));
    }

    fn handle_unavailable_line(&self, line: &str, json: &Location, events: &mut dyn DownloadEvents) {
        let Some(caps) = UNAVAILABLE_ERROR.captures(line) else { return };
        let (id, reason) = (&caps[1], &caps[2]);
        if !UNAVAILABLE_MARKERS.iter().any(|m| reason.contains(m)) {
            return;
        }
        match self.archives.mark_unavailable(json, id, reason) {
            Ok(()) => events.line(ActivityLine::message("activity.unavailable.single", &[reason])),
            Err(e) => log::warn!("Could not mark {id} as unavailable: {e}"),
        }
    }

    // === AFTER THE RUN ===

    fn commit(&self, json: &Location, folder: &Location, format: &str, captures: &Captures, events: &mut dyn DownloadEvents) {
        match self.archives.commit(json, folder, format, &captures.captured) {
            Ok(collisions) => {
                for c in collisions {
                    events.line(ActivityLine::message("warn.titleCollision", &[&c.count.to_string(), &c.title]));
                }
            }
            Err(e) => log::error!("Could not update the archive after the download: {e}"),
        }
    }

    /// The ids asked for that did not come down: audio playlists only (the
    /// second pass renames to the number and only makes sense there).
    fn retry_items(&self, json: &Location, format: &str, playlist: Option<&PlaylistState>, captures: &Captures) -> Vec<RetryItem> {
        let Some(playlist) = playlist else { return Vec::new() };
        if !AUDIO_FORMATS.iter().any(|f| f.eq_ignore_ascii_case(format)) {
            return Vec::new();
        }
        let captured: HashSet<&str> = captures.captured.iter().map(|(id, _)| id.as_str()).collect();
        let still_pending: HashSet<String> =
            self.archives.ids_pending_download(json, &playlist.pending).unwrap_or_default().into_iter().collect();
        playlist
            .pending
            .iter()
            .filter(|id| !captured.contains(id.as_str()) && still_pending.contains(*id))
            .filter(|id| !self.archives.is_unavailable(json, id))
            .map(|id| {
                let title = captures
                    .seen_titles
                    .get(id)
                    .cloned()
                    .or_else(|| playlist.entries.get(id).and_then(|e| e.title.clone()))
                    .or_else(|| self.archives.saved_title(json, id))
                    .unwrap_or_else(|| id.clone());
                RetryItem { id: id.clone(), title, number: self.archives.saved_number(json, id) }
            })
            .collect()
    }

    /// Returns true when it was stopped.
    #[allow(clippy::too_many_arguments)]
    fn second_pass(
        &self,
        json: &Location,
        folder: &Location,
        format: &str,
        cookies: Option<&str>,
        settings: &UserSettings,
        items: &[RetryItem],
        events: &mut dyn DownloadEvents,
    ) -> bool {
        events.line(ActivityLine::message("activity.retry.starting", &[&items.len().to_string()]));
        events.step(Step::Retry, u32::try_from(items.len()).unwrap_or(u32::MAX));
        let net = SafetyNet {
            downloader: self.downloader,
            archives: self.archives,
            json,
            folder,
            format,
            cookies_file: cookies.map(str::to_string),
            ffmpeg_dir: self.ffmpeg_dir.clone(),
            cached_client: settings.working_player_client.clone(),
            scratch: self.scratch.clone(),
            stop: self.stop,
            patience: self.patience,
        };
        let outcome = net.run(items, &mut Forward(events));
        for f in &outcome.failures {
            events.line(ActivityLine::message("activity.retry.failedOne", &[&f.title, &f.reason]));
        }
        match outcome.failures.as_slice() {
            [] => {}
            [one] => events.notice(WARNING, "activity.retry.summaryOne", &[&one.title, &one.reason]),
            [first, ..] => {
                let n = outcome.failures.len().to_string();
                events.notice(WARNING, "activity.retry.summaryMany", &[&n, &first.title, &first.reason]);
            }
        }
        outcome.stopped
    }
}

/// The second pass speaks through the same events as the run.
struct Forward<'a, 'b>(&'a mut (dyn DownloadEvents + 'b));

impl RetryEvents for Forward<'_, '_> {
    fn line(&mut self, line: ActivityLine) {
        self.0.line(line);
    }

    fn progress(&mut self, progress: RetryProgress) {
        if let Some(fraction) = progress.fraction {
            let done = (progress.index - 1) as f64 + fraction;
            self.0.progress(done / progress.total.max(1) as f64);
        }
        self.0.retry(progress);
    }

    fn downloaded(&mut self, id: &str) {
        self.0.downloaded(id);
    }
}

/// yt-dlp's leftovers out of the music folder: all of them in a playlist's own
/// folder, only those of this run's file in the folder a single went into.
fn clean_up(storage: &dyn Storage, folder: &Location, format: &str, single: bool, captures: &Captures) {
    if !single {
        clean_output_folder(storage, folder, format);
        return;
    }
    let stems: Vec<String> = captures
        .captured
        .iter()
        .map(|(_, file)| {
            let name = file.name();
            name.rsplit_once('.').map_or(name.clone(), |(stem, _)| stem.to_string())
        })
        .collect();
    clean_leftovers_of(storage, folder, format, &stems);
}

mod create;
mod device;
pub use create::{CreateError, Created};
pub use device::{DeviceJob, DeviceSide, device_playlist_json, nice_device_file_name};

#[cfg(test)]
mod tests;
