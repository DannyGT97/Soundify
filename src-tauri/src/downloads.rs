//! The one download at a time: launches the engine on its own thread and
//! turns what it says into events for the front (the wiring the Java
//! `SoundifyController` did through `DownloadCoordinator.Ui`).
//!
//! Events: `tasks` (the whole [`TasksView`]), `activity` (one line) and
//! `notice` (how it went, worth a toast).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use soundify_core::Location;
use soundify_core::archive::ArchiveStore;
use soundify_core::downloader::StopFlag;
use soundify_core::engine::activity::{ActivityLine, ActivityLog};
use soundify_core::device::DeviceTarget;
use soundify_core::engine::download::{DeviceJob, DeviceSide, DownloadEvents, Engine, Job};
use soundify_core::engine::safety_net::{Patience, RetryProgress};
use soundify_core::engine::tasks::{BackgroundTasks, ORIGIN_MANAGER, ORIGIN_URL, Step, TasksView};
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
/// The refusal when a loose download is given a list: that is «Crear Playlist».
const IS_PLAYLIST: &str = "download.error.isPlaylist";

#[derive(Clone, Serialize)]
struct Notice<'a> {
    status: &'static str,
    key: &'a str,
    args: &'a [&'a str],
}

pub struct Downloads {
    tasks: Mutex<BackgroundTasks>,
    activity: Mutex<ActivityLog>,
    /// A fresh one per run: a stop must never leak into the next download.
    stop: Mutex<StopFlag>,
}

// === PUBLIC API ===

impl Downloads {
    pub fn new(tasks: BackgroundTasks) -> Self {
        Self { tasks: Mutex::new(tasks), activity: Mutex::new(ActivityLog::new()), stop: Mutex::default() }
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

    pub fn stop(&self, app: &AppHandle) {
        self.tasks.lock().unwrap().request_stop();
        self.stop.lock().unwrap().stop();
        self.emit_view(app);
    }

    /// Ignored while another one runs: the screens disable their buttons, so
    /// this only guards against a double click.
    pub fn start(&self, app: &AppHandle, request: DownloadRequest) {
        let state = app.state::<AppState>();
        // Refused here whatever the interface let through.
        if !soundify_core::tools::status::can_download(&state.tools) {
            let _ = app.emit("notice", Notice { status: task_history::FAILED, key: TOOLS_MISSING, args: &[] });
            return;
        }
        if let DownloadRequest::Single { url, .. } | DownloadRequest::SingleToDevice { url, .. } = &request
            && soundify_core::paths::is_playlist_url(url)
        {
            let _ = app.emit("notice", Notice { status: task_history::FAILED, key: IS_PLAYLIST, args: &[] });
            return;
        }
        {
            let mut tasks = self.tasks.lock().unwrap();
            if tasks.is_running() {
                return;
            }
            let (title, origin) = describe(&state.archives, &request);
            tasks.start(&title, origin);
        }
        *self.stop.lock().unwrap() = StopFlag::new();
        self.clear_activity();
        let _ = app.emit("activity-reset", ());
        self.emit_view(app);

        let app = app.clone();
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            let downloads = &state.downloads;
            match to_job(&state.archives, request) {
                Some(job) => downloads.run(&app, &state, &job),
                None => {
                    let mut events = Events::new(&app);
                    events.notice(task_history::FAILED, "activity.error.emptyInput", &[]);
                }
            }
            downloads.tasks.lock().unwrap().finish();
            downloads.save_history(&state);
            downloads.emit_view(&app);
        });
    }
}

// === PRIVATE HELPERS ===

impl Downloads {
    fn run(&self, app: &AppHandle, state: &AppState, job: &AnyJob) {
        let stop = self.stop.lock().unwrap().clone();
        with_engine(&state.archives, &state.tools, &state.paths.settings(), &stop, |engine| match job {
            AnyJob::Local(job) => engine.run(job, &mut Events::new(app)),
            AnyJob::Device(job) => {
                let side = DeviceSide { phone: state.phone.as_ref(), staging_root: state.paths.device_staging() };
                engine.run_device(&side, job, &mut Events::new(app));
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
    let downloader = YtDlpProcess::new(tools.clone());
    let engine = Engine {
        downloader: &downloader,
        archives,
        settings_file,
        // The folder that holds ffmpeg.exe and ffprobe.exe, as the Java app
        // passed it (the parent of the executable): yt-dlp looks for both
        // right there, and the zip's root folder made it find neither.
        ffmpeg_dir: tools.is_managed().then(|| tools.ffmpeg_bin().to_string_lossy().into_owned()),
        scratch: std::env::temp_dir().join("soundify"),
        stop,
        patience: Patience::default(),
    };
    work(&engine)
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

/// The engine's events, turned into state and events for the front.
struct Events<'a> {
    app: &'a AppHandle,
    /// The last whole percent sent: yt-dlp prints a progress line many times
    /// a second, and each one would repaint the indicator for nothing.
    last_percent: Option<i64>,
}

impl<'a> Events<'a> {
    fn new(app: &'a AppHandle) -> Self {
        Self { app, last_percent: None }
    }

    fn with_tasks(&self, change: impl FnOnce(&mut BackgroundTasks)) {
        let state = self.app.state::<AppState>();
        change(&mut state.downloads.tasks.lock().unwrap());
        state.downloads.emit_view(self.app);
    }
}

impl DownloadEvents for Events<'_> {
    fn line(&mut self, line: ActivityLine) {
        let _ = self.app.emit("activity", &line);
        self.app.state::<AppState>().downloads.activity.lock().unwrap().append(line);
    }

    fn title(&mut self, title: &str) {
        self.with_tasks(|tasks| tasks.offer_title(title));
    }

    fn archive(&mut self, json: &Location) {
        let key = json.to_string();
        self.with_tasks(|tasks| tasks.set_block(&key));
    }

    fn step(&mut self, step: Step, total: u32) {
        self.last_percent = None;
        self.with_tasks(|tasks| tasks.set_step(step, total));
    }

    fn progress(&mut self, fraction: f64) {
        let percent = (fraction * 100.0).round() as i64;
        if self.last_percent == Some(percent) {
            return;
        }
        self.last_percent = Some(percent);
        self.with_tasks(|tasks| tasks.set_progress(fraction));
    }

    fn retry(&mut self, _progress: RetryProgress) {}

    fn downloaded(&mut self, _id: &str) {
        let _ = self.app.emit("file-downloaded", ());
    }

    fn notice(&mut self, status: &'static str, key: &str, args: &[&str]) {
        self.with_tasks(|tasks| {
            tasks.report(status, key, args);
        });
        let _ = self.app.emit("notice", Notice { status, key, args });
    }
}
