//! A playlist (or one loose video) whose music lives on a phone (the Java
//! `DeviceSyncService`). Deliberately NOT a device branch inside
//! [`Engine::run`]: that one leans on `ArchiveStore::prepare`/`commit`, built
//! entirely around a local folder and `entry.file_path`. Fed a device archive,
//! their self-repair rule for entries with no file ("a bare unavailable marker,
//! clear any deleted_at") would silently un-delete every pending device entry.
//! Two jobs, two pieces: this one reuses the low-level parts (the command
//! builder, the downloader, the queries) and never calls the local-only ones.
//!
//! **Simpler than the local pipeline on purpose**: one video at a time (like
//! the second pass) instead of one batched yt-dlp run, and a single
//! player_client attempt per item. Each file is staged locally as
//! `<id>.<ext>` and only then pushed under its nice name: a file left in the
//! staging folder by an interrupted push is recoverable by its name alone,
//! without guessing.
//!
//! **What survives an interruption**: a file that finished downloading but
//! could not be confirmed on the device (a disconnect mid-push) stays in the
//! staging folder, never deleted. The next sync pushes it straight away, with
//! no new trip to YouTube.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::thread;

use regex::Regex;

use crate::archive::{ArchiveStore, device as archive_device, io as archive_io};
use crate::device::{DeviceTarget, PortableDevice};
use crate::downloader::{OutputLine, RunEnd, YtDlpRequest};
use crate::engine::activity::ActivityLine;
use crate::engine::download::{CreateError, Created, DownloadEvents, Engine};
use crate::engine::tasks::Step;
use crate::library::AUDIO_FORMATS;
use crate::location::Location;
use crate::paths;
use crate::settings;
use crate::storage::LocalStorage;
use crate::task_history::{FAILED, OK, WARNING};
use crate::ytdlp::cleaning::clean_output_folder;
use crate::ytdlp::command::{self, BuildConfig, MOVED_PREFIX, SEEN_PREFIX};
use crate::ytdlp::progress::ProgressParser;
use crate::ytdlp::query::{self, PlaylistEntry};

/// Only a file genuinely named `<id>.<ext>` in the staging folder is a
/// leftover of this service: anything else there is not its to touch.
static STAGED_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{11}$").unwrap());

// === WHAT TO SYNC ===

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceJob {
    /// A device playlist already registered, updated from the Gestor.
    Playlist { json: Location },
    /// «Descarga por URL» sent to a phone: no title, membership or entries
    /// query, just this one video ("son archivos sueltos").
    Single { url: String, format: String, target: DeviceTarget },
}

/// Everything a device run needs besides the [`Engine`].
pub struct DeviceSide<'a> {
    pub phone: &'a dyn PortableDevice,
    /// `data/device-staging`: one stable folder per archive under it.
    pub staging_root: PathBuf,
}

/// What one push attempt found out.
#[derive(Debug, PartialEq, Eq)]
enum PushOutcome {
    Success,
    Failed,
    /// The device itself is gone, not just this one file: the whole run stops
    /// rather than let every remaining item time out one by one.
    Disconnected,
}

/// One run's fixed facts.
struct Run<'a> {
    json: &'a Location,
    device_id: &'a str,
    device_path: &'a str,
    format: &'a str,
    staging: PathBuf,
    /// Read once per run: the archive's own immutable choice. Always false
    /// for a loose download (that parity with local, numbering from the
    /// destination folder, is a separate design left out on purpose).
    numbered: bool,
    /// Gates what only makes sense for a PLAYLIST: the manifest (a loose
    /// download has no folder of its own to put one in).
    single: bool,
}

/// What one item's download left behind.
#[derive(Default)]
struct ItemCaptures {
    staged: Option<PathBuf>,
    real_title: Option<String>,
}

// === PUBLIC API ===

impl Engine<'_> {
    pub fn run_device(&self, side: &DeviceSide, job: &DeviceJob, events: &mut dyn DownloadEvents) {
        let result = match job {
            DeviceJob::Playlist { json } => self.sync_device_playlist(side, json, events),
            DeviceJob::Single { url, format, target } => self.download_to_device(side, url, format, target, events),
        };
        if let Err(e) = result {
            log::error!("Device sync failed: {e}");
            let error = e.to_string();
            events.line(ActivityLine::message("activity.error.startFailed", &[&error]));
            events.notice(FAILED, "activity.error.startFailed", &[&error]);
        }
    }

    /// «Crear Playlist» with a phone as the destination: registered WITHOUT
    /// downloading, like [`Engine::create_playlist`]. `target.path` is the
    /// BASE the picker returned, never the final folder: the playlist gets its
    /// own "Name (format)" subfolder, or every device playlist picked into the
    /// same base would dump its files together (real bug, reported: files
    /// landed straight in "Music").
    pub fn create_device_playlist(
        &self,
        side: &DeviceSide,
        raw_url: &str,
        format: &str,
        target: &DeviceTarget,
        numbered: bool,
    ) -> Result<Created, CreateError> {
        let found = self.look_up_new_playlist(raw_url, format)?;
        let (url, title, entries) = (&found.url, &found.title, &found.entries);
        let json = device_playlist_json(self.archives, title, target, format)?;
        let number_mode = self.archives.resolve_number_mode(&json, numbered);
        let device_path = paths::device_playlist_sub_path(Some(&target.path), title, format);
        let placed = DeviceTarget { path: device_path, ..target.clone() };
        let items: Vec<(String, String)> =
            entries.iter().map(|(id, e)| (id.clone(), e.title.clone().unwrap_or_default())).collect();
        self.archives.register_device_playlist(
            side.phone,
            &self.scratch,
            &json,
            Some(url),
            Some(title),
            &placed,
            &items,
            number_mode.number_files,
        )?;
        let order: HashMap<String, i32> =
            entries.iter().map(|(id, e)| (id.clone(), i32::try_from(e.position).unwrap_or(i32::MAX))).collect();
        self.archives.refresh_playlist_order(&json, &order)?;
        log::info!("Device playlist \"{title}\" registered ({} ids) at {}:{}", entries.len(), target.device_name, placed.path);
        Ok(Created { json, title: found.title.clone(), count: entries.len(), generic_title: found.generic_title, number_mode })
    }
}

/// A device target has no local base folder to hash against: a synthetic key
/// (never a real path, only fed into the SAME hash a local base goes through)
/// keeps playlist+device+path as stable and independent as playlist+folder.
pub fn device_playlist_json(archives: &ArchiveStore, title: &str, target: &DeviceTarget, format: &str) -> io::Result<Location> {
    let storage = archives.storage();
    let base = Location::Uri(format!("mtp:{}:{}", target.device_id, target.path));
    let dir = paths::playlist_archive_dir(storage, archives.archives(), title, &base, format)?;
    storage.create_dir_all(&dir)?;
    storage.child(&dir, archive_io::DATA_JSON)
}

// === THE TWO STARTS ===

impl Engine<'_> {
    fn sync_device_playlist(&self, side: &DeviceSide, json: &Location, events: &mut dyn DownloadEvents) -> io::Result<()> {
        let data = self.archives.load(json)?;
        let (Some(device_id), Some(device_path)) = (data.device_id.clone(), data.device_path.clone()) else {
            return Err(io::Error::other("not a device playlist"));
        };
        let url = data.url.clone().filter(|u| !u.trim().is_empty()).ok_or_else(|| io::Error::other("archive without a url"))?;
        let format = archive_io::format_of(json).ok_or_else(|| io::Error::other("archive without a format"))?.to_uppercase();
        let staging = archive_device::staging_folder(&side.staging_root, json).ok_or_else(|| io::Error::other("no staging folder"))?;
        events.archive(json);
        let run = Run {
            json,
            device_id: &device_id,
            device_path: &device_path,
            format: &format,
            staging,
            numbered: data.number_files,
            single: false,
        };

        // Checked once, up front, before touching YouTube at all: no point
        // downloading anything with nowhere to push it to right now.
        if !side.phone.is_reachable(&device_id) {
            return self.report_disconnected(&run, events);
        }

        let mut settings = settings::load(self.archives.storage(), self.settings_file);
        let cookies = Some(settings.cookies_file.clone()).filter(|c| !c.trim().is_empty());
        let cookies = cookies.as_deref();
        let url = paths::normalize_playlist_url(url.trim());
        let known = data.title.clone().or(data.device_name.clone());
        let title = self.resolve_title(&url, known.as_deref(), &format, cookies, events);
        events.title(&title);

        let Some(player_client) = self.player_client(&url, cookies, &mut settings, events) else {
            events.line(ActivityLine::message("activity.botcheck.blocked", &[]));
            events.notice(FAILED, "activity.botcheck.notice", &[]);
            return Ok(());
        };

        events.line(ActivityLine::message("activity.playlist.checking", &[]));
        let (entries, output) = self.fetch_entries(&url, cookies);
        if entries.is_empty() {
            let cookies_expired = query::failed_due_to_cookies(&output);
            let key = if cookies_expired { "activity.sync.cookiesExpired" } else { "activity.sync.failed" };
            events.line(ActivityLine::message(key, &[]));
            let key = if cookies_expired { "activity.manager.cookiesExpired" } else { "activity.manager.checkFailed" };
            events.notice(FAILED, key, &[&title]);
            return Ok(());
        }
        self.sync_with_playlist(json, &url, &entries)?;
        // Against what is REALLY on the device right now: without it, a file
        // removed by hand from the phone stayed "downloaded" until the app
        // restarted, so syncing never asked for it again (reported). Local
        // never needed this: its pending check rescans the real folder.
        self.archives.reconcile_device_archive_now(side.phone, json);

        fs::create_dir_all(&run.staging)?;
        // Leftovers of an interrupted push go FIRST: they are already
        // downloaded, only unconfirmed on the device.
        if !self.push_leftovers(side, &run, events) {
            return self.report_disconnected(&run, events);
        }

        let ids: Vec<String> = entries.keys().cloned().collect();
        let queue: Vec<(String, String)> = self
            .archives
            .ids_pending_download(json, &ids)?
            .into_iter()
            .map(|id| {
                let title = entries.get(&id).and_then(|e| e.title.clone()).filter(|t| !t.trim().is_empty());
                let title = title.unwrap_or_else(|| id.clone());
                (id, title)
            })
            .collect();
        if queue.is_empty() {
            events.line(ActivityLine::message("activity.alreadyDownloaded.playlist", &[&format, &title]));
            events.notice(OK, "activity.manager.upToDate", &[&title]);
            return Ok(());
        }
        self.process_queue(side, &run, &player_client, &queue, &entries, events)
    }

    fn download_to_device(
        &self,
        side: &DeviceSide,
        url: &str,
        format: &str,
        target: &DeviceTarget,
        events: &mut dyn DownloadEvents,
    ) -> io::Result<()> {
        let storage = self.archives.storage();
        storage.create_dir_all(self.archives.archives())?;
        let json = paths::individual_device_archive_json(storage, self.archives.archives(), &target.device_id, format)?;
        let staging = archive_device::staging_folder(&side.staging_root, &json).ok_or_else(|| io::Error::other("no staging folder"))?;
        events.archive(&json);
        events.line(ActivityLine::message("activity.download.single", &[]));
        let run = Run {
            json: &json,
            device_id: &target.device_id,
            device_path: &target.path,
            format,
            staging,
            numbered: false,
            single: true,
        };

        if !side.phone.is_reachable(&target.device_id) {
            return self.report_disconnected(&run, events);
        }
        // Unlike a playlist (registered before any sync), a loose download's
        // archive is only touched from here: this is its "register" moment.
        self.archives.register_device_individual(&json, target)?;

        // Unlike the local check (a courtesy notice on a miss), a miss here
        // aborts: the id names the staged file and keys the whole queue.
        let Some(video_id) = paths::extract_video_id(url) else {
            events.line(ActivityLine::message("activity.error.urlNotRecognised", &[]));
            events.notice(FAILED, "activity.error.urlNotRecognised", &[]);
            return Ok(());
        };
        if self.archives.known_ids(&json)?.contains(&video_id) {
            events.line(ActivityLine::message("activity.alreadyDownloaded.single", &[format]));
            events.notice(OK, "activity.alreadyDownloaded.single", &[format]);
            return Ok(());
        }

        let mut settings = settings::load(storage, self.settings_file);
        let cookies = Some(settings.cookies_file.clone()).filter(|c| !c.trim().is_empty());
        let Some(player_client) = self.player_client(url, cookies.as_deref(), &mut settings, events) else {
            events.line(ActivityLine::message("activity.botcheck.blocked", &[]));
            events.notice(FAILED, "activity.botcheck.notice", &[]);
            return Ok(());
        };

        fs::create_dir_all(&run.staging)?;
        if !self.push_leftovers(side, &run, events) {
            return self.report_disconnected(&run, events);
        }
        let queue = vec![(video_id.clone(), video_id)];
        self.process_queue(side, &run, &player_client, &queue, &Default::default(), events)
    }
}

// === THE QUEUE (one download after another) ===

impl Engine<'_> {
    fn process_queue(
        &self,
        side: &DeviceSide,
        run: &Run,
        player_client: &str,
        queue: &[(String, String)],
        entries: &indexmap::IndexMap<String, PlaylistEntry>,
        events: &mut dyn DownloadEvents,
    ) -> io::Result<()> {
        let settings = settings::load(self.archives.storage(), self.settings_file);
        let cookies = Some(settings.cookies_file).filter(|c| !c.trim().is_empty());
        events.step(Step::Download, u32::try_from(queue.len()).unwrap_or(u32::MAX));
        // Ordinary per-item failures (the device stayed connected): a run that
        // quietly failed on a few items must not look like a perfect one.
        let mut failed_titles: Vec<String> = Vec::new();

        for (done, (id, title)) in queue.iter().enumerate() {
            if self.stop.is_stopped() {
                return self.finish_stopped(side, run, events);
            }
            if !run.single {
                events.title(title);
            }
            events.line(ActivityLine::message("deviceSync.downloading", &[title]));
            // A separate yt-dlp per video: nothing tells the parser which item
            // it is on, so it is told every time (a raw done/total jump before
            // a single byte came down showed 100% for a loose download's whole
            // run — reported).
            let mut parser = ProgressParser::new();
            parser.lock_total_items(done as i64, (queue.len() - done) as i64);

            let captures = match self.download_item(run, id, player_client, cookies.as_deref(), &mut parser, events) {
                Ok(Some(captures)) => captures,
                Ok(None) => return self.finish_stopped(side, run, events),
                Err(e) => {
                    log::error!("Could not download {id}: {e}");
                    self.item_failed(run, id, title, &mut failed_titles, events);
                    continue;
                }
            };
            if AUDIO_FORMATS.iter().any(|f| f.eq_ignore_ascii_case(run.format)) {
                clean_output_folder(&LocalStorage, &Location::Path(run.staging.clone()), run.format);
            }
            let Some(staged) = captures.staged.filter(|p| self.wait_until_exists(p)) else {
                self.item_failed(run, id, title, &mut failed_titles, events);
                continue;
            };

            // Renamed to <id>.<ext> BEFORE the push (not after): a file left
            // behind by an interrupted push is then recoverable by name alone,
            // whether the interruption came here or during the push itself.
            let id_named = run.staging.join(format!("{id}.{}", paths::format_extension(run.format)));
            if staged != id_named
                && let Err(e) = self.move_with_retry(&staged, &id_named)
            {
                log::warn!("Could not rename {} before pushing: {e}", staged.display());
                continue;
            }
            // yt-dlp's own title over the queued one: --flat-playlist reads the
            // LISTING page, which can carry a translated title the video's own
            // page never does (reported: device file names in English).
            let real_title = captures
                .real_title
                .filter(|t| !t.trim().is_empty())
                .or_else(|| entries.get(id).and_then(|e| e.title.clone()))
                .unwrap_or_else(|| title.clone());
            let number = self.next_number_for(run, id);
            let name = nice_device_file_name(number.as_deref(), Some(&real_title), id, run.format);
            match self.push_one(side, run, &id_named, id, &real_title, &name, number.as_deref(), events) {
                PushOutcome::Success => events.downloaded(id),
                PushOutcome::Failed => failed_titles.push(real_title),
                PushOutcome::Disconnected => return self.report_disconnected(run, events),
            }
        }

        events.line(ActivityLine::message("deviceSync.finished", &[]));
        report_failures(&failed_titles, events);
        self.end_of_run(side, run);
        Ok(())
    }

    /// One video into the staging folder. None when it was stopped.
    fn download_item(
        &self,
        run: &Run,
        id: &str,
        player_client: &str,
        cookies: Option<&str>,
        parser: &mut ProgressParser,
        events: &mut dyn DownloadEvents,
    ) -> io::Result<Option<ItemCaptures>> {
        // A throwaway archive: the real one is keyed on the DEVICE copy, and
        // yt-dlp must not skip a video only because it was staged before.
        let work = self.scratch.join(format!("soundify_devicesync_{id}.txt"));
        fs::create_dir_all(&self.scratch)?;
        let _ = fs::remove_file(&work);
        let cfg = BuildConfig {
            ffmpeg_dir: self.ffmpeg_dir.clone(),
            archive_file: work.to_string_lossy().into_owned(),
            output_path: run.staging.to_string_lossy().into_owned(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            is_playlist: false,
            playlist_offset: 0,
            cookies_file: cookies.map(str::to_string),
            include_id_in_title: false,
            number_files: false,
            player_client: Some(player_client.to_string()),
            playlist_items: None,
        };
        let mut captures = ItemCaptures::default();
        let single = run.single;
        let request = YtDlpRequest { args: command::build(run.format, &cfg) };
        let end = self.downloader.run(&request, self.stop, &mut |line| {
            let text = match line {
                OutputLine::Stdout(t) | OutputLine::Stderr(t) => t,
            };
            if let Some(payload) = text.strip_prefix(MOVED_PREFIX) {
                if let Some((_, path)) = payload.split_once('|') {
                    captures.staged = Some(PathBuf::from(path));
                }
                return;
            }
            if let Some(payload) = text.strip_prefix(SEEN_PREFIX) {
                if let Some((_, title)) = payload.split_once('|') {
                    captures.real_title = Some(title.to_string());
                    events.line(ActivityLine::raw(format!("🎵 {title}")));
                    // A loose download has no title query up front: the label
                    // showed the bare id until now (reported).
                    if single {
                        events.title(title);
                    }
                }
                return;
            }
            if let Some(fraction) = parser.parse(&text) {
                events.progress(fraction);
            }
            log::info!(target: crate::logs::ACTIVITY, "{text}");
            events.line(ActivityLine::raw(text));
        });
        let _ = fs::remove_file(&work);
        match end? {
            RunEnd::Stopped => Ok(None),
            RunEnd::Finished { exit_code: 0 } => Ok(Some(captures)),
            RunEnd::Finished { .. } => Ok(Some(ItemCaptures::default())),
        }
    }

    fn item_failed(&self, run: &Run, id: &str, title: &str, failed: &mut Vec<String>, events: &mut dyn DownloadEvents) {
        events.line(ActivityLine::message("deviceSync.itemDownloadFailed", &[title]));
        if let Err(e) = self.archives.mark_failed(run.json, id, Some(title), "download failed") {
            log::warn!("Could not mark {id} as failed: {e}");
        }
        failed.push(title.to_string());
    }

    /// The previous number when the id already had one reserved (a redownload
    /// after its device copy was removed keeps ITS slot instead of jumping to
    /// the end, the bug local's registration fixed), the next free otherwise.
    /// Read fresh for every file: each sees the bump the previous one made.
    fn next_number_for(&self, run: &Run, id: &str) -> Option<String> {
        if !run.numbered {
            return None;
        }
        self.archives
            .saved_number(run.json, id)
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.archives.next_free_number(run.json))
    }
}

// === PUSH TO THE DEVICE ===

impl Engine<'_> {
    /// Pushes one staged file and, only once that is confirmed, registers it
    /// and deletes the local copy. On an ordinary failure the file stays where
    /// it is: the next sync retries it without asking YouTube again.
    #[allow(clippy::too_many_arguments)]
    fn push_one(
        &self,
        side: &DeviceSide,
        run: &Run,
        local: &Path,
        id: &str,
        title: &str,
        device_file_name: &str,
        number: Option<&str>,
        events: &mut dyn DownloadEvents,
    ) -> PushOutcome {
        events.line(ActivityLine::message("deviceSync.pushing", &[device_file_name]));
        let pushed = side.phone.mkdirs(run.device_id, run.device_path)
            && side.phone.push(run.device_id, run.device_path, local, Some(device_file_name));
        if pushed {
            if let Err(e) = fs::remove_file(local) {
                log::warn!("Pushed but could not delete the staging copy {}: {e}", local.display());
            }
            if let Err(e) = self.archives.register_device_single(run.json, id, device_file_name, Some(title), number) {
                log::warn!("Could not register {id} after pushing it: {e}");
            }
            events.line(ActivityLine::message("deviceSync.pushed", &[title]));
            return PushOutcome::Success;
        }
        // A SEPARATE, fresh check: a push that ran out of time says nothing
        // about why. Treating it as a plain per-file failure left a real
        // disconnection reported as "this file failed" (reported: "se para
        // pero no hay notificación").
        if !side.phone.is_reachable(run.device_id) {
            return PushOutcome::Disconnected;
        }
        events.line(ActivityLine::message("deviceSync.pushFailed", &[title]));
        PushOutcome::Failed
    }

    /// Files sitting in staging from an earlier run interrupted between the
    /// download and the confirmed push. False when a disconnect was found:
    /// the caller must not go on to download anything new either.
    fn push_leftovers(&self, side: &DeviceSide, run: &Run, events: &mut dyn DownloadEvents) -> bool {
        let Ok(dir) = fs::read_dir(&run.staging) else { return true };
        let mut files: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for file in files {
            if self.stop.is_stopped() {
                return true;
            }
            let Some(id) = file.file_stem().and_then(|s| s.to_str()).filter(|s| STAGED_ID.is_match(s)) else { continue };
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let title = self.archives.saved_title(run.json, id);
            let number = self.next_number_for(run, id);
            let device_name = nice_device_file_name(number.as_deref(), title.as_deref(), id, run.format);
            events.line(ActivityLine::message("deviceSync.resuming", &[&name]));
            let shown = title.as_deref().unwrap_or(id);
            match self.push_one(side, run, &file, id, shown, &device_name, number.as_deref(), events) {
                PushOutcome::Success => events.downloaded(id),
                PushOutcome::Failed => {}
                PushOutcome::Disconnected => return false,
            }
        }
        true
    }

    /// Never a technical trace: whatever was pushed stays pushed, whatever was
    /// not stays in staging, and reconnecting picks up exactly from here.
    fn report_disconnected(&self, run: &Run, events: &mut dyn DownloadEvents) -> io::Result<()> {
        log::warn!("Device {} unreachable during a sync of {}", run.device_id, run.json);
        events.line(ActivityLine::message("deviceSync.disconnected", &[]));
        events.notice(FAILED, "deviceSync.disconnected", &[]);
        Ok(())
    }

    fn finish_stopped(&self, side: &DeviceSide, run: &Run, events: &mut dyn DownloadEvents) -> io::Result<()> {
        events.line(ActivityLine::message("deviceSync.cancelled", &[]));
        self.end_of_run(side, run);
        Ok(())
    }

    /// The end of a run, finished or stopped (whatever got pushed is worth
    /// recording), never after a disconnect: pushing a manifest to a device
    /// just found unreachable would only wait out another timeout for nothing.
    fn end_of_run(&self, side: &DeviceSide, run: &Run) {
        if !run.single {
            let now = self.archives.now();
            crate::transfer::device::write_manifest_quietly(self.archives.storage(), side.phone, run.json, &self.scratch, &now);
        }
        // What the Gestor sorts by: a long sync stays on top once it TRULY
        // finishes (last_synced_at was stamped at its start).
        if let Err(e) = self.archives.touch_last_activity(run.json) {
            log::warn!("Could not stamp the last activity: {e}");
        }
    }

    fn wait_until_exists(&self, file: &Path) -> bool {
        for _ in 0..self.patience.file_polls {
            if file.exists() {
                return true;
            }
            thread::sleep(self.patience.file_poll);
        }
        file.exists()
    }

    fn move_with_retry(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut last = None;
        for attempt in 1..=self.patience.move_attempts.max(1) {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            if attempt < self.patience.move_attempts {
                thread::sleep(self.patience.move_pause);
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
    }
}

/// The name a pushed file shows under on the device: "NNN - " when the
/// archive numbers, then the sanitised real title, or the bare id when there
/// is none. Deliberately NOT the staging name (`<id>.<ext>`): pushing that
/// left every file on the phone titled after its YouTube id (reported).
pub fn nice_device_file_name(number: Option<&str>, title: Option<&str>, id: &str, format: &str) -> String {
    let base = match title.filter(|t| !t.trim().is_empty() && *t != id) {
        Some(title) => paths::sanitize_folder_name(title),
        None => id.to_string(),
    };
    let prefix = number.filter(|n| !n.trim().is_empty()).map(|n| format!("{n} - ")).unwrap_or_default();
    format!("{prefix}{base}.{}", paths::format_extension(format))
}

/// Amber, not red: these retry on their own at the next sync, nothing the
/// user has to act on (red looked too alarming — reported).
fn report_failures(failed: &[String], events: &mut dyn DownloadEvents) {
    match failed {
        [] => {}
        [one] => events.notice(WARNING, "deviceSync.summaryOneFailed", &[one]),
        [first, ..] => events.notice(WARNING, "deviceSync.summaryManyFailed", &[&failed.len().to_string(), first]),
    }
}

#[cfg(test)]
mod tests;
