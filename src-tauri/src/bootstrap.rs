//! What runs once at startup (the Java `Bootstrap`): the tools' folders, the
//! updaters, the supported-formats cache and the reconciliation of the
//! archives. Everything slow goes on a thread of its own and nothing here is
//! fatal: the app opens with whatever it has, and what is missing is reported
//! through [`ToolHealth`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use chrono::Local;
use soundify_core::archive::ArchiveStore;
use soundify_core::device::PortableDevice;
use soundify_core::downloader::{Downloader, OutputLine, StopFlag, YtDlpRequest};
use soundify_core::http::ReqwestHttp;
use soundify_core::settings;
use soundify_core::storage::LocalStorage;
use soundify_core::tools::health::{Problem, ToolHealth};
use soundify_core::tools::progress::{InstallProgress, Tool};
use soundify_core::tools::status;
use soundify_core::tools::{ToolPaths, updaters};
use soundify_core::ytdlp::query;

use crate::app_paths::AppPaths;
use crate::process::YtDlpProcess;

/// The tools being installed or updated right now: a second install of the
/// same one while the first runs would write over the same files.
pub type Installing = Arc<Mutex<BTreeSet<Tool>>>;

/// What this yt-dlp says it supports, lower-cased. Empty = "could not check".
#[derive(Debug, Default, Clone)]
pub struct SupportedFormats {
    pub audio: Vec<String>,
    pub video: Vec<String>,
}

pub struct Tools {
    pub paths: ToolPaths,
    pub health: Arc<ToolHealth>,
    pub progress: Arc<InstallProgress>,
    pub formats: Arc<RwLock<SupportedFormats>>,
    pub installing: Installing,
}

pub fn start(app_paths: &AppPaths, archives: &ArchiveStore, phone: &Arc<dyn PortableDevice>) -> Tools {
    let tools = Tools {
        paths: ToolPaths::for_this_platform(app_paths.systemtools()),
        health: Arc::new(ToolHealth::new()),
        progress: Arc::new(InstallProgress::new()),
        formats: Arc::default(),
        installing: Installing::default(),
    };

    // Startup reconciliation: which ids still have a file. Never fatal: the
    // library is "last known state".
    let reconciling = archives.clone();
    std::thread::spawn(move || reconciling.reconcile_all());
    // The device archives on a thread of their own: a phone that does not
    // answer must never hold up the local ones.
    let (reconciling, phone, scratch) = (archives.clone(), phone.clone(), app_paths.device_scratch());
    std::thread::spawn(move || {
        if let Err(e) = std::fs::create_dir_all(&scratch) {
            log::warn!("Could not create {}: {e}", scratch.display());
        }
        reconciling.reconcile_device_archives(phone.as_ref(), &scratch)
    });

    if tools.paths.is_managed() {
        start_updaters(&tools, settings::load(&LocalStorage, &app_paths.settings()).auto_updates);
    } else {
        let (paths, formats) = (tools.paths.clone(), tools.formats.clone());
        std::thread::spawn(move || cache_formats(&paths, &formats));
    }
    tools
}

/// The managed copies (Windows). Only what needs no asking: the ffmpeg that
/// travels with the app is put in place, and the tools already there are kept
/// up to date, when a newer version exists (if Ajustes → Actualizaciones allows
/// it). What is missing is installed from the first-start assistant or from Ajustes: downloading
/// ~180 MB unasked, with nothing on screen, made the first start look stuck.
/// Elsewhere the system's tools are used and nothing is downloaded — the Java
/// app ran the yt-dlp and ffmpeg updaters on Linux too, for nothing.
fn start_updaters(tools: &Tools, auto_updates: bool) {
    for folder in tools.paths.folders() {
        if let Err(e) = std::fs::create_dir_all(&folder) {
            log::error!("Could not create {}: {e}", folder.display());
        }
    }

    // In this order: the update must see the ffmpeg the seed has just copied.
    let (paths, health, progress, installing) =
        (tools.paths.clone(), tools.health.clone(), tools.progress.clone(), tools.installing.clone());
    std::thread::spawn(move || {
        if let Some(bundled) = bundled_ffmpeg()
            && let Err(e) = updaters::seed_ffmpeg(&paths, &bundled)
        {
            log::warn!("Could not copy the bundled ffmpeg: {e}");
        }
        if auto_updates {
            update_heavy_tools(&paths, &health, &progress, &installing);
        }
    });

    // The formats are asked for after the update: the new yt-dlp may support
    // more (or fewer) than the old one.
    let (paths, health, progress, formats, installing) =
        (tools.paths.clone(), tools.health.clone(), tools.progress.clone(), tools.formats.clone(), tools.installing.clone());
    std::thread::spawn(move || {
        if !paths.ytdlp().exists() {
            return;
        }
        if auto_updates {
            match ReqwestHttp::new(env!("CARGO_PKG_VERSION")) {
                Ok(http) => {
                    let local = updaters::local_ytdlp_version(&YtDlpProcess::new(paths.clone()));
                    while_claimed(&installing, Tool::Ytdlp, || {
                        updaters::update_ytdlp(&paths, &http, local.as_deref(), Local::now().date_naive(), &health, &progress);
                    });
                }
                Err(e) => health.report(Problem::UpdateCheckFailed, &format!("no HTTP client: {e}")),
            }
        }
        cache_formats(&paths, &formats);
    });
}

/// ffmpeg and Deno, only if a higher version is out. Nothing here is reported
/// to the user but the download itself: the copies in place keep working.
fn update_heavy_tools(paths: &ToolPaths, health: &ToolHealth, progress: &InstallProgress, installing: &Installing) {
    let http = match ReqwestHttp::new(env!("CARGO_PKG_VERSION")) {
        Ok(http) => http,
        Err(e) => {
            log::warn!("No HTTP client to update ffmpeg and Deno: {e}");
            return;
        }
    };
    let version_of = |tool| crate::tools::version_output(paths, tool).and_then(|out| status::parse_version(tool, &out));
    while_claimed(installing, Tool::Ffmpeg, || {
        updaters::update_ffmpeg(paths, &http, version_of(Tool::Ffmpeg).as_deref(), health, progress);
    });
    while_claimed(installing, Tool::Deno, || {
        updaters::update_deno(paths, &http, version_of(Tool::Deno).as_deref(), health, progress);
    });
}

/// Runs `work` unless the user is already installing that tool by hand.
fn while_claimed(installing: &Installing, tool: Tool, work: impl FnOnce()) {
    if !installing.lock().unwrap().insert(tool) {
        return;
    }
    work();
    installing.lock().unwrap().remove(&tool);
}

/// Where the bundle's `resources/ffmpeg/*` ends up (`tauri.conf.json`): next
/// to the executable on Windows, which is the resource folder there — also
/// under `target/debug` in development. Read before Tauri starts, so not
/// through `PathResolver`.
pub(crate) fn bundled_ffmpeg() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("tools").join("ffmpeg"))
}

/// Asks `yt-dlp --help` which formats it supports. If it cannot be asked, the
/// cache stays empty and every format is offered.
pub(crate) fn cache_formats(paths: &ToolPaths, formats: &RwLock<SupportedFormats>) {
    let process = YtDlpProcess::new(paths.clone());
    let mut help = String::new();
    let request = YtDlpRequest { args: query::help_args() };
    let result = process.run(&request, &StopFlag::new(), &mut |line| {
        if let OutputLine::Stdout(text) = line {
            help.push_str(&text);
            help.push('\n');
        }
    });
    if let Err(e) = result {
        log::warn!("Could not ask yt-dlp for its formats: {e}");
        return;
    }
    let supported = SupportedFormats {
        audio: query::parse_supported_list(&help, query::AUDIO_FORMATS_MARKER),
        video: query::parse_supported_list(&help, query::VIDEO_CONTAINERS_MARKER),
    };
    log::info!("yt-dlp supports audio {:?} and video {:?}", supported.audio, supported.video);
    *formats.write().unwrap() = supported;
}
