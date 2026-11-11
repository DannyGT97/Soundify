//! The downloads: each one on its own thread with its own id and its own stop flag, the
//! rest waiting in a queue. How many may run at once, and which ones may not run together,
//! is decided by the planner in `core` (`BackgroundTasks::start_ready`: one at a time, or two
//! with Ajustes → «Varias descargas a la vez», experimental); this only launches what it starts. Turns what the engine says into events for the front
//! (the wiring the Java `SoundifyController` did through `DownloadCoordinator.Ui`).
//!
//! Events: `tasks` (the whole [`TasksView`]), `activity` (one line, with the `taskId` it
//! belongs to) and `notice` (how it went, worth a toast).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use soundify_core::Location;
use soundify_core::archive::ArchiveStore;
use soundify_core::device::DeviceTarget;
use soundify_core::downloader::StopFlag;
use soundify_core::engine::activity::{ActivityLine, ActivityLog};
use soundify_core::engine::download::{DeviceJob, DeviceSide, DownloadEvents, Engine, Job};
use soundify_core::engine::resources::{self, ResourceKey};
use soundify_core::engine::safety_net::{Patience, RetryProgress};
use soundify_core::engine::tasks::{BackgroundTasks, MAX_RUNNING, MAX_RUNNING_PARALLEL, ORIGIN_MANAGER, ORIGIN_URL, Step, TaskId, TasksView};
use soundify_core::storage::LocalStorage;
use soundify_core::task_history;
use soundify_core::tools::ToolPaths;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::process::YtDlpProcess;

/// What the user asked to download (`DownloadRequest` in `src/api/types.ts`).
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DownloadRequest {
    Single { url: String, format: String, folder: String, #[serde(default)] cover: Option<String> },
    /// «Descarga por URL» with a phone as the destination.
    SingleToDevice { url: String, format: String, target: DeviceTarget, #[serde(default)] cover: Option<String> },
    #[serde(rename_all = "camelCase")]
    Update { block_key: String },
}

/// A local download or one that ends on a phone: two engines behind one queue.
enum AnyJob {
    Local(Job),
    Device(DeviceJob),
}

/// The refusal when yt-dlp or ffmpeg are not there.
pub const TOOLS_MISSING: &str = "tools.error.missing";
/// A request put behind the running one, and one refused because that archive is already busy.
const QUEUED: &str = "tasks.queue.added";
const ALREADY_BUSY: &str = "tasks.queue.duplicate";
/// The refusal when a loose download is given a list: that is «Crear Playlist».
const IS_PLAYLIST: &str = "download.error.isPlaylist";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Notice<'a> {
    status: &'static str,
    key: &'a str,
    args: &'a [&'a str],
    /// The task it is about; none for a refusal before anything was queued.
    task_id: Option<TaskId>,
}

/// An activity line and the task that wrote it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaggedLine<'a> {
    task_id: TaskId,
    #[serde(flatten)]
    line: &'a ActivityLine,
}

pub struct Downloads {
    tasks: Mutex<BackgroundTasks>,
    activity: Mutex<ActivityLog>,
    /// One flag per running task, made when it starts and dropped when it ends: a stop
    /// never reaches another task, nor leaks into the next one.
    stops: Mutex<HashMap<TaskId, StopFlag>>,
    /// What the waiting tasks asked for, by the id `BackgroundTasks` gave them.
    /// Lock order: `tasks` first, then this or `stops`.
    pending: Mutex<HashMap<TaskId, DownloadRequest>>,
    /// Serializes the "keep the phone awake" notice, so the last word is always the truth.
    busy_gate: Mutex<()>,
}

// === PUBLIC API ===

impl Downloads {
    pub fn new(tasks: BackgroundTasks) -> Self {
        Self {
            tasks: Mutex::new(tasks),
            activity: Mutex::new(ActivityLog::new()),
            stops: Mutex::default(),
            pending: Mutex::default(),
            busy_gate: Mutex::default(),
        }
    }

    pub fn view(&self) -> TasksView {
        self.tasks.lock().unwrap().view()
    }

    pub fn activity(&self) -> Vec<ActivityLine> {
        self.activity.lock().unwrap().snapshot()
    }

    pub fn clear_activity(&self) {
        self.activity.lock().unwrap().clear();
    }

    pub fn set_window_open(&self, app: &AppHandle, open: bool) {
        self.tasks.lock().unwrap().set_window_open(open);
        self.emit_view(app);
    }

    pub fn clear_history(&self, app: &AppHandle, state: &AppState) {
        self.tasks.lock().unwrap().clear_history();
        self.save_history(state);
        self.emit_view(app);
    }

    /// Stops one running task, or all of them when no id is given.
    pub fn stop(&self, app: &AppHandle, id: Option<TaskId>) {
        let ids = {
            let mut tasks = self.tasks.lock().unwrap();
            match id {
                Some(id) if tasks.request_stop(id) => vec![id],
                Some(_) => Vec::new(),
                None => tasks.request_stop_all(),
            }
        };
        {
            let stops = self.stops.lock().unwrap();
            for id in ids {
                if let Some(flag) = stops.get(&id) {
                    flag.stop();
                }
            }
        }
        self.emit_view(app);
    }

    /// Something that decides what can start changed (the setting for several downloads at once):
    /// whatever was waiting for room may start now.
    pub fn replan(&self, app: &AppHandle) {
        if self.start_ready_and_launch(app).is_empty() {
            return;
        }
        self.emit_view(app);
    }

    /// Takes a waiting task out of the queue (a playlist keeps its registration: only the download is dropped).
    pub fn remove_queued(&self, app: &AppHandle, id: TaskId) {
        {
            let mut tasks = self.tasks.lock().unwrap();
            if tasks.remove_queued(id) {
                self.pending.lock().unwrap().remove(&id);
            }
        }
        // What it was holding back may be able to start now.
        self.start_ready_and_launch(app);
        self.emit_view(app);
    }

    /// Drops everything waiting; the running task goes on.
    pub fn clear_queue(&self, app: &AppHandle) {
        {
            let mut tasks = self.tasks.lock().unwrap();
            let mut pending = self.pending.lock().unwrap();
            for id in tasks.clear_queue() {
                pending.remove(&id);
            }
        }
        self.emit_view(app);
    }

    /// Puts it in the queue and starts whatever the planner lets start (the same archive
    /// twice, running or waiting, is refused).
    pub fn start(&self, app: &AppHandle, request: DownloadRequest) {
        let state = app.state::<AppState>();
        // Refused here whatever the interface let through.
        if !soundify_core::tools::status::can_download(&state.tools) {
            let _ = app.emit("notice", Notice { status: task_history::FAILED, key: TOOLS_MISSING, args: &[], task_id: None });
            return;
        }
        if let DownloadRequest::Single { url, .. } | DownloadRequest::SingleToDevice { url, .. } = &request
            && soundify_core::paths::is_playlist_url(url)
        {
            let _ = app.emit("notice", Notice { status: task_history::FAILED, key: IS_PLAYLIST, args: &[], task_id: None });
            return;
        }
        let (title, origin) = describe(&state.archives, &request);
        let block_key = match &request {
            DownloadRequest::Update { block_key } => Some(block_key.clone()),
            _ => None,
        };
        let needs = reservations(&state.archives, &request);
        let (id, queued_behind) = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(key) = &block_key
                && tasks.is_busy_with(key)
            {
                let _ = app.emit("notice", Notice { status: task_history::WARNING, key: ALREADY_BUSY, args: &[&title], task_id: None });
                return;
            }
            let id = tasks.enqueue(&title, origin, block_key.as_deref(), needs);
            self.pending.lock().unwrap().insert(id, request);
            (id, tasks.view().queued.len())
        };
        let started = self.start_ready_and_launch(app);
        if !started.contains(&id) {
            let position = queued_behind.to_string();
            let _ = app.emit("notice", Notice { status: task_history::OK, key: QUEUED, args: &[&title, &position], task_id: Some(id) });
        }
        self.emit_view(app);
    }
}

// === PRIVATE HELPERS ===

impl Downloads {
    /// Asks the planner what can start now, makes each one's stop flag and launches them.
    /// Returns the ids it started.
    fn start_ready_and_launch(&self, app: &AppHandle) -> Vec<TaskId> {
        let limit = run_limit(&app.state::<AppState>());
        let ready = {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.set_limit(limit);
            self.take_ready(&mut tasks)
        };
        let ids = ready.iter().map(|(id, _)| *id).collect();
        for (id, request) in ready {
            self.begin_run(app);
            self.launch(app, id, request);
        }
        ids
    }

    /// With the `tasks` lock held: starts what the planner allows and pairs each task with
    /// its request and a new stop flag. A task whose request is gone (it should not happen)
    /// is closed as failed rather than left running with nothing to do.
    fn take_ready(&self, tasks: &mut BackgroundTasks) -> Vec<(TaskId, DownloadRequest)> {
        let mut pending = self.pending.lock().unwrap();
        let mut stops = self.stops.lock().unwrap();
        let mut ready = Vec::new();
        for id in tasks.start_ready() {
            match pending.remove(&id) {
                Some(request) => {
                    stops.insert(id, StopFlag::new());
                    ready.push((id, request));
                }
                None => {
                    tasks.fail_to_start(id, "activity.error.emptyInput", &[]);
                }
            }
        }
        ready
    }

    /// A task per thread. When it ends it closes itself, and whatever it was holding back
    /// may start.
    fn launch(&self, app: &AppHandle, id: TaskId, request: DownloadRequest) {
        self.sync_busy(app);
        let app = app.clone();
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            let downloads = &state.downloads;
            match to_job(&state.archives, request) {
                Some(job) => downloads.run(&app, &state, id, &job),
                None => {
                    let mut events = Events::new(&app, id);
                    events.notice(task_history::FAILED, "activity.error.emptyInput", &[]);
                }
            }
            // Closing this one and taking what can start happen under one lock: a request that
            // arrives in between sees either a running task or an empty queue, never a gap.
            let limit = run_limit(&state);
            let ready = {
                let mut tasks = downloads.tasks.lock().unwrap();
                tasks.set_limit(limit);
                tasks.finish(id);
                downloads.stops.lock().unwrap().remove(&id);
                downloads.take_ready(&mut tasks)
            };
            downloads.save_history(&state);
            downloads.emit_view(&app);
            for (next, request) in ready {
                downloads.begin_run(&app);
                downloads.launch(&app, next, request);
            }
            downloads.sync_busy(&app);
        });
    }

    /// Android freezes an app that is only in the background: the notice keeps it going while
    /// anything runs. It always says what is true when it is called, whoever called last.
    fn sync_busy(&self, app: &AppHandle) {
        let _gate = self.busy_gate.lock().unwrap();
        let (running, title) = {
            let tasks = self.tasks.lock().unwrap();
            let title = tasks.view().running.first().map(|r| r.title.clone()).unwrap_or_default();
            (tasks.is_running(), title)
        };
        crate::mobile::set_busy(app, running, &title);
    }

    /// A fresh Activity for the task that just started. When another one is already running its
    /// lines are still being written there (every line says whose it is), so it is left alone.
    fn begin_run(&self, app: &AppHandle) {
        if self.tasks.lock().unwrap().running_count() <= 1 {
            self.clear_activity();
            let _ = app.emit("activity-reset", ());
        }
        self.emit_view(app);
    }

    fn run(&self, app: &AppHandle, state: &AppState, id: TaskId, job: &AnyJob) {
        let stop = self.stops.lock().unwrap().get(&id).cloned().unwrap_or_default();
        with_engine(&state.archives, &state.tools, &state.paths.settings(), &stop, |engine| match job {
            AnyJob::Local(job) => engine.run(job, &mut Events::new(app, id)),
            AnyJob::Device(job) => {
                let side = DeviceSide { phone: state.phone.as_ref(), staging_root: state.paths.device_staging() };
                engine.run_device(&side, job, &mut Events::new(app, id));
            }
        });
    }

    fn save_history(&self, state: &AppState) {
        let history = self.tasks.lock().unwrap().history().to_vec();
        if let Err(e) = task_history::save(&LocalStorage, &state.paths.task_history(), &history) {
            log::warn!("Could not save the task history: {e}");
        }
    }

    fn emit_view(&self, app: &AppHandle) {
        let _ = app.emit("tasks", self.view());
    }
}

/// The engine as a download and «Crear Playlist» both use it.
pub fn with_engine<R>(
    archives: &ArchiveStore,
    tools: &ToolPaths,
    settings_file: &Location,
    stop: &StopFlag,
    work: impl FnOnce(&Engine) -> R,
) -> R {
    // Each run has throwaway files of its own: two tasks at once must not trip over each other's.
    static RUNS: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join("soundify").join(format!("run-{}-{}", std::process::id(), RUNS.fetch_add(1, Ordering::Relaxed)));
    let downloader = YtDlpProcess::new(tools.clone());
    let engine = Engine {
        downloader: &downloader,
        archives,
        settings_file,
        // The folder that holds ffmpeg.exe and ffprobe.exe, as the Java app
        // passed it (the parent of the executable): yt-dlp looks for both
        // right there, and the zip's root folder made it find neither.
        ffmpeg_dir: tools.is_managed().then(|| tools.ffmpeg_bin().to_string_lossy().into_owned()),
        scratch: scratch.clone(),
        stop,
        patience: Patience::default(),
    };
    let result = work(&engine);
    // Only goes if nothing was left in it (a leftover would be someone's work file).
    let _ = std::fs::remove_dir(&scratch);
    result
}

/// How many tasks may run at once: one, or two with the experimental setting.
fn run_limit(state: &AppState) -> usize {
    if soundify_core::settings::load(&LocalStorage, &state.paths.settings()).parallel_blocks { MAX_RUNNING_PARALLEL } else { MAX_RUNNING }
}

/// What the request needs for itself while it runs (see `core::engine::resources`).
fn reservations(archives: &ArchiveStore, request: &DownloadRequest) -> Vec<ResourceKey> {
    match request {
        DownloadRequest::Single { folder, .. } => resources::for_single(folder),
        DownloadRequest::SingleToDevice { .. } => resources::for_device(),
        DownloadRequest::Update { block_key } => resources::for_update(archives, &Location::parse(block_key)),
    }
}

/// The provisional title (the real one arrives once yt-dlp says it) and the
/// i18n key of where it was launched from.
fn describe(archives: &ArchiveStore, request: &DownloadRequest) -> (String, &'static str) {
    match request {
        DownloadRequest::Single { url, .. } | DownloadRequest::SingleToDevice { url, .. } => (url.clone(), ORIGIN_URL),
        DownloadRequest::Update { block_key } => {
            let json = Location::parse(block_key);
            let title = match archives.load(&json) {
                Ok(data) if data.is_device_backed() => data.title,
                _ => match Job::update(archives, &json) {
                    Some(Job::Playlist { known_title, .. }) => known_title,
                    _ => None,
                },
            };
            let title = title.unwrap_or_else(|| block_key.clone());
            (title, ORIGIN_MANAGER)
        }
    }
}

fn to_job(archives: &ArchiveStore, request: DownloadRequest) -> Option<AnyJob> {
    Some(match request {
        DownloadRequest::Single { url, format, folder, cover } => {
            AnyJob::Local(Job::Single { url, format, base: Location::parse(&folder), cover: cover.map(PathBuf::from) })
        }
        DownloadRequest::SingleToDevice { url, format, target, cover } => {
            AnyJob::Device(DeviceJob::Single { url, format, target, cover: cover.map(PathBuf::from) })
        }
        DownloadRequest::Update { block_key } => {
            let json = Location::parse(&block_key);
            // A device playlist has no local folder for the local engine to
            // download into: it goes through the phone's own queue.
            if archives.load(&json).is_ok_and(|data| data.is_device_backed()) {
                AnyJob::Device(DeviceJob::Playlist { json })
            } else {
                AnyJob::Local(Job::update(archives, &json)?)
            }
        }
    })
}

/// The engine's events for one task, turned into state and events for the front.
struct Events<'a> {
    app: &'a AppHandle,
    id: TaskId,
    /// The last whole percent sent: yt-dlp prints a progress line many times
    /// a second, and each one would repaint the indicator for nothing.
    last_percent: Option<i64>,
}

impl<'a> Events<'a> {
    fn new(app: &'a AppHandle, id: TaskId) -> Self {
        Self { app, id, last_percent: None }
    }

    fn with_tasks(&self, change: impl FnOnce(&mut BackgroundTasks, TaskId)) {
        let state = self.app.state::<AppState>();
        change(&mut state.downloads.tasks.lock().unwrap(), self.id);
        state.downloads.emit_view(self.app);
    }
}

impl DownloadEvents for Events<'_> {
    fn line(&mut self, line: ActivityLine) {
        let _ = self.app.emit("activity", TaggedLine { task_id: self.id, line: &line });
        self.app.state::<AppState>().downloads.activity.lock().unwrap().append(line);
    }

    fn title(&mut self, title: &str) {
        self.with_tasks(|tasks, id| tasks.offer_title(id, title));
    }

    fn archive(&mut self, json: &Location) {
        let key = json.to_string();
        self.with_tasks(|tasks, id| tasks.set_block(id, &key));
    }

    fn step(&mut self, step: Step, total: u32) {
        self.last_percent = None;
        self.with_tasks(|tasks, id| tasks.set_step(id, step, total));
    }

    fn progress(&mut self, fraction: f64) {
        let percent = (fraction * 100.0).round() as i64;
        if self.last_percent == Some(percent) {
            return;
        }
        self.last_percent = Some(percent);
        self.with_tasks(|tasks, id| tasks.set_progress(id, fraction));
    }

    fn retry(&mut self, _progress: RetryProgress) {}

    fn downloaded(&mut self, _id: &str) {
        let _ = self.app.emit("file-downloaded", ());
    }

    fn notice(&mut self, status: &'static str, key: &str, args: &[&str]) {
        self.with_tasks(|tasks, id| {
            tasks.report(id, status, key, args);
        });
        let _ = self.app.emit("notice", Notice { status, key, args, task_id: Some(self.id) });
    }

    fn extra_process(&mut self) -> bool {
        let state = self.app.state::<AppState>();
        // The guard is gone before `state` is (returning the call directly keeps it past that).
        #[allow(clippy::let_and_return)]
        let given = state.downloads.tasks.lock().unwrap().claim_extra_process(self.id);
        given
    }

    fn release_extra_process(&mut self) {
        let state = self.app.state::<AppState>();
        state.downloads.tasks.lock().unwrap().release_extra_process(self.id);
        // The process may be what a waiting task needed to start.
        state.downloads.replan(self.app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_activity_line_keeps_its_shape_and_adds_the_task() {
        let line = ActivityLine::message("retry.trying", &["Título", "abc"]);
        let json = serde_json::to_value(TaggedLine { task_id: 7, line: &line }).unwrap();
        let plain = serde_json::to_value(&line).unwrap();
        assert_eq!(json["taskId"], 7);
        for (key, value) in plain.as_object().unwrap() {
            assert_eq!(&json[key], value, "{key} must be the same as without the task");
        }
    }

    #[test]
    fn a_notice_names_its_task_or_none() {
        let notice = Notice { status: "ok", key: "tasks.queue.added", args: &["A", "2"], task_id: Some(3) };
        let json = serde_json::to_value(&notice).unwrap();
        assert_eq!(json["taskId"], 3);
        let refused = serde_json::to_value(Notice { status: "failed", key: "x", args: &[], task_id: None }).unwrap();
        assert!(refused["taskId"].is_null());
    }
}
