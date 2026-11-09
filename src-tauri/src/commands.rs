//! What the front can ask the back for (`src/api/tauri.ts`). Each command is
//! a thin call into `core`: the logic and its tests live there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Local;
use log::warn;
use serde::Serialize;
use soundify_core::Location;
use soundify_core::archive::{ArchiveStore, DeleteResult, NumberMode};
use soundify_core::device::PortableDevice;
use soundify_core::downloader::StopFlag;
use soundify_core::engine::download::{CreateError, Created};
use soundify_core::library::{self, AUDIO_FORMATS, Playlist, VIDEO_FORMATS};
use soundify_core::model::{PlayerMemory, Preferences, PreferencesPatch, UserSettings};
use soundify_core::storage::{LocalStorage, Storage};
use soundify_core::engine::activity::ActivityLine;
use soundify_core::engine::tasks::TasksView;
use soundify_core::tools::ToolPaths;
use soundify_core::tools::health::ToolHealth;
use soundify_core::tools::progress::{InstallProgress, ToolDownload};
use soundify_core::ytdlp::query::filter_supported;
use soundify_core::transfer::{self, ExportResult, ImportResult, TransferError};
use soundify_core::settings;
use soundify_core::updater::UpdateError;
use tauri::{AppHandle, Emitter, State};

use crate::app_paths::AppPaths;
use crate::bootstrap::SupportedFormats;
use crate::covers;
use crate::downloads::{self, DownloadRequest, Downloads};

pub struct AppState {
    pub paths: AppPaths,
    pub archives: ArchiveStore,
    /// The phone transport: Windows' MTP, or nothing elsewhere.
    pub phone: Arc<dyn PortableDevice>,
    /// Raised by "Cancelar" while a copy runs; checked between one file and the next.
    pub copy_cancelled: Arc<AtomicBool>,
    pub health: Arc<ToolHealth>,
    pub progress: Arc<InstallProgress>,
    pub formats: Arc<RwLock<SupportedFormats>>,
    pub tools: ToolPaths,
    pub installing: crate::bootstrap::Installing,
    pub downloads: Downloads,
}

/// What a failed command hands the front: an i18n key and its arguments, so
/// the message follows the language in use (the Java app's keys).
#[derive(Debug, Serialize)]
pub struct CommandError {
    key: String,
    args: Vec<String>,
}

impl From<TransferError> for CommandError {
    fn from(error: TransferError) -> Self {
        Self { key: error.key().to_string(), args: error.args() }
    }
}

impl From<UpdateError> for CommandError {
    fn from(error: UpdateError) -> Self {
        Self { key: error.key().to_string(), args: error.args() }
    }
}

impl CommandError {
    pub(crate) fn io(message: impl ToString) -> Self {
        TransferError::Io(message.to_string()).into()
    }

    pub(crate) fn key(key: &str) -> Self {
        Self::with_args(key, Vec::new())
    }

    pub(crate) fn with_args(key: &str, args: Vec<String>) -> Self {
        Self { key: key.to_string(), args }
    }
}

#[derive(Serialize)]
pub struct FormatCatalog {
    audio: Vec<&'static str>,
    video: Vec<&'static str>,
}

/// Always the last known state: never asks YouTube.
#[tauri::command]
pub fn list_playlists(app: AppHandle, state: State<'_, AppState>) -> Vec<Playlist> {
    let playlists = library::library(&state.archives, Local::now().date_naive());
    // The player plays the files straight from the music folders, so the
    // webview is let into each folder that holds one.
    let folders: HashSet<PathBuf> = playlists
        .iter()
        .flat_map(|p| &p.formats)
        .flat_map(|b| &b.tracks)
        .filter_map(|t| t.file.as_deref())
        .filter_map(|file| Path::new(file).parent().map(Path::to_path_buf))
        .collect();
    for folder in folders {
        covers::allow_folder(&app, &folder);
    }
    playlists
}

/// Only what this yt-dlp supports: a format a future yt-dlp drops disappears
/// on its own. Everything while it could not be asked.
#[tauri::command]
pub fn formats(state: State<'_, AppState>) -> FormatCatalog {
    let supported = state.formats.read().unwrap();
    FormatCatalog {
        audio: filter_supported(&AUDIO_FORMATS, &supported.audio),
        video: filter_supported(&VIDEO_FORMATS, &supported.video),
    }
}

/// The `{0}` of the tools notice, or nothing while every tool is in place.
/// What happens later arrives as the `tool-health` event.
#[tauri::command]
pub fn tool_health(state: State<'_, AppState>) -> Option<&'static str> {
    state.health.consequence_key()
}

/// The tools downloading right now. What changes later arrives as the
/// `tool-downloads` event.
#[tauri::command]
pub fn tool_downloads(state: State<'_, AppState>) -> Vec<ToolDownload> {
    state.progress.snapshot()
}

// === DOWNLOADS ===

/// The running task (if any), the unseen result and the history; what
/// changes later arrives as the `tasks` event.
#[tauri::command]
pub fn tasks_view(state: State<'_, AppState>) -> TasksView {
    state.downloads.view()
}

#[tauri::command]
pub fn clear_task_history(app: AppHandle, state: State<'_, AppState>) {
    state.downloads.clear_history(&app, &state);
}

#[tauri::command]
pub fn set_tasks_window_open(app: AppHandle, state: State<'_, AppState>, open: bool) {
    state.downloads.set_window_open(&app, open);
}

#[tauri::command]
pub fn start_download(app: AppHandle, state: State<'_, AppState>, request: DownloadRequest) {
    state.downloads.start(&app, request);
}

#[tauri::command]
pub fn stop_download(app: AppHandle, state: State<'_, AppState>) {
    state.downloads.stop(&app);
}

/// What the activity window shows when it opens; new lines arrive as the
/// `activity` event.
#[tauri::command]
pub fn activity_log(state: State<'_, AppState>) -> Vec<ActivityLine> {
    state.downloads.activity()
}

#[tauri::command]
pub fn clear_activity(state: State<'_, AppState>) {
    state.downloads.clear_activity();
}

/// Missing or broken gives the defaults (see `core::settings`).
#[tauri::command]
pub fn settings(state: State<'_, AppState>) -> UserSettings {
    settings::load(&LocalStorage, &state.paths.settings())
}

/// What Ajustes shows; read once at startup, before the first screen.
#[tauri::command]
pub fn preferences(state: State<'_, AppState>) -> Preferences {
    settings::preferences(&settings::load(&LocalStorage, &state.paths.settings()))
}

/// Saved on each change, like the Java panels: there is no «Guardar» button.
#[tauri::command]
pub fn save_preferences(state: State<'_, AppState>, patch: PreferencesPatch) -> Result<(), CommandError> {
    let file = state.paths.settings();
    let mut current = settings::load(&LocalStorage, &file);
    settings::apply(&mut current, &patch);
    settings::save(&LocalStorage, &file, &current).map_err(CommandError::io)
}

/// Back to the factory values; the front applies what comes back on the spot.
#[tauri::command]
pub fn reset_preferences(state: State<'_, AppState>) -> Result<Preferences, CommandError> {
    let file = state.paths.settings();
    let fresh = settings::reset(&settings::load(&LocalStorage, &file));
    settings::save(&LocalStorage, &file, &fresh).map_err(CommandError::io)?;
    Ok(settings::preferences(&fresh))
}

/// The cookies file chooser, opened where the last one was.
#[tauri::command]
pub async fn pick_cookies_file(app: AppHandle, state: State<'_, AppState>, title: String, filter: String) -> Result<Option<String>, CommandError> {
    // On the phone the dialog plugin hands back a content:// URI that yt-dlp cannot open:
    // Kotlin picks the file and copies it into the app's folder instead.
    #[cfg(target_os = "android")]
    {
        let _ = (&state, &title, &filter);
        tauri::async_runtime::spawn_blocking(move || crate::mobile::pick_cookies_now(&app))
            .await
            .map_err(CommandError::io)?
            .map_err(CommandError::io)
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_dialog::DialogExt;
        let current = settings::load(&LocalStorage, &state.paths.settings()).cookies_file;
        let mut dialog = app.dialog().file().set_title(title).add_filter(filter, &["txt"]);
        if let Some(folder) = Path::new(&current).parent().filter(|f| f.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        Ok(dialog.blocking_pick_file().and_then(|picked| picked.into_path().ok()).map(|path| path.display().to_string()))
    }
}

/// Whether a file is (still) there: the cookies' status line.
#[tauri::command]
pub fn file_exists(path: String) -> bool {
    !path.trim().is_empty() && Path::new(&path).is_file()
}

/// The player's volume and last file, for the bar at startup.
#[tauri::command]
pub fn player_memory(state: State<'_, AppState>) -> PlayerMemory {
    settings::player_memory(&settings::load(&LocalStorage, &state.paths.settings()))
}

/// Written on each new file, on every pause, on letting go of the volume and
/// on closing the app — rarely, and a small file.
#[tauri::command]
pub fn save_player_memory(state: State<'_, AppState>, memory: PlayerMemory) -> Result<(), CommandError> {
    let file = state.paths.settings();
    let mut current = settings::load(&LocalStorage, &file);
    settings::set_player_memory(&mut current, &memory);
    settings::save(&LocalStorage, &file, &current).map_err(CommandError::io)
}

// === CREAR PLAYLIST ===

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedView {
    block_key: String,
    title: String,
    count: usize,
    generic_title: bool,
    number_mode: NumberMode,
}

impl From<Created> for CreatedView {
    fn from(created: Created) -> Self {
        Self {
            block_key: created.json.to_string(),
            title: created.title,
            count: created.count,
            generic_title: created.generic_title,
            number_mode: created.number_mode,
        }
    }
}

/// «Crear Playlist»: asks YouTube for the playlist and registers it, without
/// downloading; «Crear y descargar» then starts an update of `blockKey`. Off
/// the main thread: the two queries take seconds.
#[tauri::command]
pub async fn create_playlist(
    state: State<'_, AppState>,
    url: String,
    format: String,
    folder: String,
    numbered: bool,
) -> Result<CreatedView, CommandError> {
    if !soundify_core::tools::status::can_download(&state.tools) {
        return Err(CommandError::key(downloads::TOOLS_MISSING));
    }
    let archives = state.archives.clone();
    let tools = state.tools.clone();
    let settings_file = state.paths.settings();
    tauri::async_runtime::spawn_blocking(move || {
        downloads::with_engine(&archives, &tools, &settings_file, &StopFlag::new(), |engine| {
            engine.create_playlist(&url, &format, &Location::parse(&folder), numbered)
        })
    })
    .await
    .map_err(CommandError::io)?
    .map(CreatedView::from)
    .map_err(|e| match e {
        CreateError::CookiesExpired => CommandError::key("newPlaylist.cookiesExpired"),
        CreateError::QueryFailed => CommandError::key("newPlaylist.queryFailed"),
        CreateError::Io(e) => {
            warn!("Could not register the playlist: {e}");
            CommandError::with_args("newPlaylist.registerFailed", vec![e.to_string()])
        }
    })
}

/// The video whose thumbnail «Crear Playlist» shows for the URL typed, before
/// the playlist exists. Off the main thread: it may ask YouTube.
#[tauri::command]
pub async fn first_video_id(state: State<'_, AppState>, url: String) -> Result<Option<String>, CommandError> {
    let archives = state.archives.clone();
    let tools = state.tools.clone();
    let settings_file = state.paths.settings();
    tauri::async_runtime::spawn_blocking(move || {
        downloads::with_engine(&archives, &tools, &settings_file, &StopFlag::new(), |engine| engine.first_video_id(&url))
    })
    .await
    .map_err(CommandError::io)
}

// === COPY AND IMPORT ===

/// One step of a copy, emitted as `copy-progress`.
#[derive(Clone, Serialize)]
pub struct CopyProgress {
    done: usize,
    total: usize,
    file: String,
}

impl CopyProgress {
    pub(crate) fn new(done: usize, total: usize, file: &str) -> Self {
        Self { done, total, file: file.to_string() }
    }
}

/// Whether the copy was registered in the Gestor afterwards.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegisterOutcome {
    NotRequested,
    Registered,
    AlreadyRegistered,
    Failed,
}

impl RegisterOutcome {
    /// How registering the copy at `target` went.
    pub(crate) fn of<T>(registered: Result<T, TransferError>, target: &Location) -> Self {
        match registered {
            Ok(_) => Self::Registered,
            Err(TransferError::AlreadyRegistered { .. }) => Self::AlreadyRegistered,
            Err(e) => {
                warn!("Copy done, but registering {target} failed: {e}");
                Self::Failed
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyOutcome {
    result: ExportResult,
    /// The folder the copy left, the one that can be imported elsewhere.
    folder: String,
    register: RegisterOutcome,
}

impl CopyOutcome {
    pub(crate) fn new(result: ExportResult, folder: String, register: RegisterOutcome) -> Self {
        Self { result, folder, register }
    }
}

/// "Copiar a una carpeta local": copies the playlist into its own folder
/// inside `destination`, with the manifest, and (by default, the checkbox of
/// the confirmation) registers that copy in the Gestor right away, so the
/// user does not have to import it by hand afterwards. Off the main thread:
/// several GB can take minutes.
#[tauri::command]
pub async fn copy_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    block_key: String,
    destination: String,
    register: bool,
) -> Result<CopyOutcome, CommandError> {
    let cancelled = state.copy_cancelled.clone();
    cancelled.store(false, Ordering::SeqCst);
    let now = state.archives.now();
    let archives = state.archives.archives().clone();
    let json = Location::parse(&block_key);
    tauri::async_runtime::spawn_blocking(move || {
        let storage = &LocalStorage;
        let destination = Location::parse(&destination);
        let mut progress = |done: usize, total: usize, file: &str| {
            let _ = app.emit("copy-progress", CopyProgress::new(done, total, file));
        };
        let result = transfer::export(storage, &json, &destination, &mut progress,
            &|| cancelled.load(Ordering::SeqCst), &now)?;
        let folder_name = transfer::build_manifest(storage, &json, &now)?.folder_name;
        let target = storage.child(&destination, &folder_name)?;
        let register = if !register || result.cancelled {
            RegisterOutcome::NotRequested
        } else {
            RegisterOutcome::of(transfer::import_from(storage, &archives, &target, &now), &target)
        };
        Ok(CopyOutcome { result, folder: target.to_string(), register })
    })
    .await
    .map_err(CommandError::io)?
    .map_err(|e: TransferError| {
        warn!("Copy of {block_key} failed: {e}");
        e.into()
    })
}

#[tauri::command]
pub fn cancel_copy(state: State<'_, AppState>) {
    state.copy_cancelled.store(true, Ordering::SeqCst);
}

/// A playlist found under the chosen folder that is not registered yet.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    folder: String,
    title: String,
    format: String,
}

impl ImportCandidate {
    pub(crate) fn new(folder: String, title: String, format: String) -> Self {
        Self { folder, title, format }
    }
}

/// What "Importar" found: every playlist under the folder (`found`) and the
/// ones not registered yet, the only ones offered.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    found: usize,
    candidates: Vec<ImportCandidate>,
}

#[tauri::command]
pub async fn scan_import(state: State<'_, AppState>, folder: String) -> Result<ImportScan, CommandError> {
    let archives = state.archives.archives().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let found = transfer::find_importable(&LocalStorage, &Location::parse(&folder));
        let candidates = transfer::not_yet_registered(&LocalStorage, &archives, &found)
            .into_iter()
            .map(|location| {
                let manifest = transfer::read_manifest(&LocalStorage, &location).unwrap_or_default();
                let title = if manifest.title.trim().is_empty() { location.name() } else { manifest.title };
                ImportCandidate { folder: location.to_string(), title, format: manifest.format }
            })
            .collect();
        ImportScan { found: found.len(), candidates }
    })
    .await
    .map_err(CommandError::io)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcomeView {
    folder: String,
    result: Option<ImportResult>,
    error: Option<CommandError>,
    already_registered: bool,
}

/// Registers every folder; one failing never stops the rest.
#[tauri::command]
pub async fn import_playlists(state: State<'_, AppState>, folders: Vec<String>) -> Result<Vec<ImportOutcomeView>, CommandError> {
    let archives = state.archives.archives().clone();
    let now = state.archives.now();
    tauri::async_runtime::spawn_blocking(move || {
        let folders: Vec<Location> = folders.iter().map(|f| Location::parse(f)).collect();
        transfer::import_all(&LocalStorage, &archives, &folders, &now)
            .into_iter()
            .map(|outcome| ImportOutcomeView {
                folder: outcome.folder.to_string(),
                result: outcome.result,
                error: outcome.error.map(CommandError::from),
                already_registered: outcome.already_registered,
            })
            .collect()
    })
    .await
    .map_err(CommandError::io)
}

// === DELETE ===

/// «Eliminar todo» (`files`) or «Eliminar solo el registro»: the record goes
/// either way. Off the main thread: a phone's delete is one slow native call
/// (the Java app went "No responde" doing it on the UI thread, reported).
/// None when only the record was asked for.
#[tauri::command]
pub async fn delete_playlist_format(state: State<'_, AppState>, block_key: String, files: bool) -> Result<Option<DeleteResult>, CommandError> {
    let archives = state.archives.clone();
    let phone = state.phone.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let json = Location::parse(&block_key);
        let deleted = files.then(|| archives.delete_all_downloaded_files(phone.as_ref(), &json));
        archives.forget_playlist_format(&json);
        deleted
    })
    .await
    .map_err(CommandError::io)
}
