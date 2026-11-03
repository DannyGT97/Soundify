//! The app's own updates: the check at startup, Ajustes → Actualizaciones and
//! the hand-over to the new version (the wiring the Java `StartupUpdateCheck`,
//! `UpdatesSettingsController` and `UpdateInstaller.handOver` did). What is
//! published, how it is verified and how the swap is done live in
//! `core::updater`, with their tests.
//!
//! Events: `app-update` (a newer version found by the startup check) and
//! `app-update-progress` (the download, then "verified", then "installing").

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Local;
use log::{error, info, warn};
use serde::Serialize;
use soundify_core::clock;
use soundify_core::http::ReqwestHttp;
use soundify_core::logs::FileLogger;
use soundify_core::settings;
use soundify_core::storage::LocalStorage;
use soundify_core::updater::{self, APPLY_FLAG, Update};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_paths::AppPaths;
use crate::commands::{AppState, CommandError};

/// A margin before querying at startup: the app has just opened and is
/// extracting tools and reading archives; this is in no hurry at all.
const STARTUP_DELAY: Duration = Duration::from_secs(8);

/// How long the new copy waits for the old app to let go of its executable.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct AppUpdates {
    /// The update found, while «Descargar e instalar» is on offer.
    found: Mutex<Option<Update>>,
    /// One installation at a time: a second click must not download twice.
    installing: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
enum Progress<'a> {
    Downloading { version: &'a str, percent: u32 },
    Verified,
    Installing { version: &'a str },
}

// === THE "NEW VERSION" SIDE ===

/// Started as `soundify --apply-update <folder>`: this copy is the new
/// version, running from `updates/staged`. It waits for the old app to close,
/// swaps it and relaunches it in its usual place. True if it did (the process
/// must end there: no window opens from the staged copy).
pub fn apply_if_asked() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(app_dir) = updater::app_dir_from_args(&args) else {
        return false;
    };
    // The app's own log folder: the staged copy has no `data/` of its own.
    if let Err(e) = FileLogger::new(&app_dir.join("data").join("logs"), clock::system()).install() {
        eprintln!("No log files: {e}");
    }
    let (Ok(exe), Some(name)) = (std::env::current_exe(), exe_name()) else {
        error!("Could not tell where this copy runs from: nothing installed");
        return true;
    };
    let installed_exe = app_dir.join(&name);
    info!("Installing the update into {}", app_dir.display());

    if !updater::wait_until_writable(&installed_exe, RELEASE_TIMEOUT) {
        warn!("{} is still in use after {RELEASE_TIMEOUT:?}; trying anyway", installed_exe.display());
    }
    match exe.parent().map(|source| updater::swap_into(source, &app_dir)) {
        Some(Ok(())) => info!("Update installed"),
        // `swap_into` has already put back what it moved aside: the old
        // version is relaunched, which is better than leaving nothing open.
        Some(Err(e)) => error!("The update could not be installed: {e}"),
        None => error!("This copy has no folder: nothing installed"),
    }
    relaunch(&installed_exe, &app_dir);
    true
}

/// What an update leaves behind once the new version has started properly.
pub fn clean_up(paths: &AppPaths) {
    updater::clean_up_after_update(&paths.app_dir, &paths.staged_update());
}

// === THE STARTUP CHECK ===

/// At most one query a day, and only with the switch on. Nobody is told when
/// there is nothing new or the query failed: they asked for nothing, and it
/// is looked at again tomorrow.
pub fn check_at_startup(app: &AppHandle) {
    let paths = &app.state::<AppState>().paths;
    let saved = settings::load(&LocalStorage, &paths.settings());
    if !updater::should_check_today(saved.app_update_check_enabled, &saved.last_app_update_check, today()) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        match find_newer(&app) {
            Ok(Some(update)) => {
                info!("Daily check: a new version is out ({})", update.version);
                let _ = app.emit("app-update", &update);
            }
            Ok(None) => info!("Daily check: no new version"),
            Err(e) => info!("Could not check for updates: {e:?}"),
        }
    });
}

// === COMMANDS ===

/// «Buscar actualizaciones»: the newer version, or nothing if this is the newest.
#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<Option<Update>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || find_newer(&app)).await.map_err(CommandError::io)?
}

/// Downloads, verifies and hands over to the version found. If anything does
/// not add up NOTHING is installed, and that is said: by now the user has
/// accepted and is waiting. On success the app closes; the new copy reopens it.
#[tauri::command]
pub async fn install_app_update(app: AppHandle, updates: State<'_, AppUpdates>) -> Result<(), CommandError> {
    // A development build runs from `target/`: there is no portable to replace.
    if cfg!(debug_assertions) {
        return Err(CommandError::key("update.devBuild"));
    }
    let Some(update) = updates.found.lock().unwrap().clone() else {
        return Err(CommandError::key("update.nothingFound"));
    };
    if updates.installing.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || install(&worker, &update)).await.map_err(CommandError::io);
    app.state::<AppUpdates>().installing.store(false, Ordering::SeqCst);
    result??;
    app.exit(0);
    Ok(())
}

// === PRIVATE ===

fn find_newer(app: &AppHandle) -> Result<Option<Update>, CommandError> {
    let http = ReqwestHttp::new(env!("CARGO_PKG_VERSION")).map_err(CommandError::io)?;
    let found = updater::find_newer(&http, env!("CARGO_PKG_VERSION"));

    // The day is recorded WHATEVER the outcome: the point of it being daily is
    // not repeating it, not repeating it until there is news.
    let file = app.state::<AppState>().paths.settings();
    let mut saved = settings::load(&LocalStorage, &file);
    saved.last_app_update_check = updater::iso_date(today());
    if let Err(e) = settings::save(&LocalStorage, &file, &saved) {
        warn!("Could not record the update check: {e}");
    }

    let found = found?;
    *app.state::<AppUpdates>().found.lock().unwrap() = found.clone();
    Ok(found)
}

fn install(app: &AppHandle, update: &Update) -> Result<(), CommandError> {
    let paths = &app.state::<AppState>().paths;
    let http = ReqwestHttp::new(env!("CARGO_PKG_VERSION")).map_err(CommandError::io)?;
    let name = exe_name().ok_or_else(|| CommandError::io("no executable name"))?;

    let mut last = u32::MAX;
    let zip = updater::download(&http, update, &paths.updates(), &mut |fraction| {
        let percent = (fraction * 100.0).round() as u32;
        // Every chunk would flood the front; only whole percents go out.
        if percent != last {
            last = percent;
            let _ = app.emit("app-update-progress", Progress::Downloading { version: &update.version, percent });
        }
    })?;
    let _ = app.emit("app-update-progress", Progress::Verified);

    let staged_root = updater::stage(&zip, &paths.staged_update(), &name)?;
    hand_over(&staged_root.join(&name), &staged_root, &paths.app_dir).map_err(CommandError::io)?;
    // From here on the new copy is in charge, waiting for this one to close.
    let _ = app.emit("app-update-progress", Progress::Installing { version: &update.version });
    Ok(())
}

/// Launches the staged copy to do the swap. It waits for this executable to
/// be writable instead of for a pid: that is exactly what the swap needs.
fn hand_over(staged_exe: &Path, staged_root: &Path, app_dir: &Path) -> std::io::Result<()> {
    info!("Handing over to the new version: {} {APPLY_FLAG} {}", staged_exe.display(), app_dir.display());
    Command::new(staged_exe).arg(APPLY_FLAG).arg(app_dir).current_dir(staged_root).spawn().map(|_| ())
}

fn relaunch(exe: &Path, app_dir: &Path) {
    if let Err(e) = Command::new(exe).current_dir(app_dir).spawn() {
        error!("Could not reopen {}: {e}", exe.display());
    }
}

/// `soundify.exe` (or whatever the executable is called today): the swap and
/// the relaunch go by the running file's name, never a literal.
fn exe_name() -> Option<String> {
    let exe: PathBuf = std::env::current_exe().ok()?;
    Some(exe.file_name()?.to_string_lossy().into_owned())
}

fn today() -> chrono::NaiveDate {
    Local::now().date_naive()
}
