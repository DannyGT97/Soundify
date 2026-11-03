//! What runs once at startup (the Java `Bootstrap`): the tools' folders, the
//! updaters, the supported-formats cache and the reconciliation of the
//! archives. Everything slow goes on a thread of its own and nothing here is
//! fatal: the app opens with whatever it has, and what is missing is reported
//! through [`ToolHealth`].

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::Local;
use soundify_core::archive::ArchiveStore;
use soundify_core::device::PortableDevice;
use soundify_core::downloader::{Downloader, OutputLine, StopFlag, YtDlpRequest};
use soundify_core::http::ReqwestHttp;
use soundify_core::settings;
use soundify_core::storage::LocalStorage;
use soundify_core::tools::health::{Problem, ToolHealth};
use soundify_core::tools::progress::InstallProgress;
use soundify_core::tools::{ToolPaths, updaters};
use soundify_core::ytdlp::query;

use crate::app_paths::AppPaths;
use crate::process::YtDlpProcess;

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
}

pub fn start(app_paths: &AppPaths, archives: &ArchiveStore, phone: &Arc<dyn PortableDevice>) -> Tools {
    let tools = Tools {
        paths: ToolPaths::for_this_platform(app_paths.systemtools()),
        health: Arc::new(ToolHealth::new()),
        progress: Arc::new(InstallProgress::new()),
        formats: Arc::default(),
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
/// travels with the app is put in place, and a yt-dlp already there is kept
/// up to date (if Ajustes → Herramientas allows it). What is missing is
/// installed from the first-start assistant or from Ajustes: downloading
/// ~180 MB unasked, with nothing on screen, made the first start look stuck.
/// Elsewhere the system's tools are used and nothing is downloaded — the Java
/// app ran the yt-dlp and ffmpeg updaters on Linux too, for nothing.
fn start_updaters(tools: &Tools, auto_updates: bool) {
    for folder in tools.paths.folders() {
        if let Err(e) = std::fs::create_dir_all(&folder) {
            log::error!("Could not create {}: {e}", folder.display());
        }
    }

    let paths = tools.paths.clone();
    std::thread::spawn(move || {
        if let Some(bundled) = bundled_ffmpeg()
            && let Err(e) = updaters::seed_ffmpeg(&paths, &bundled)
        {
            log::warn!("Could not copy the bundled ffmpeg: {e}");
        }
    });

    // The formats are asked for after the update: the new yt-dlp may support
    // more (or fewer) than the old one.
    let (paths, health, progress, formats) =
        (tools.paths.clone(), tools.health.clone(), tools.progress.clone(), tools.formats.clone());
    std::thread::spawn(move || {
        if !paths.ytdlp().exists() {
            return;
        }
        if auto_updates {
            match ReqwestHttp::new(env!("CARGO_PKG_VERSION")) {
                Ok(http) => {
                    let local = updaters::local_ytdlp_version(&YtDlpProcess::new(paths.clone()));
                    updaters::update_ytdlp(&paths, &http, local.as_deref(), Local::now().date_naive(), &health, &progress);
                }
                Err(e) => health.report(Problem::UpdateCheckFailed, &format!("no HTTP client: {e}")),
            }
        }
        cache_formats(&paths, &formats);
    });
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
