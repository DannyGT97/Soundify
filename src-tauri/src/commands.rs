//! What the front can ask the back for (`src/api/tauri.ts`). Each command is
//! a thin call into `core`: the logic and its tests live there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Local;
use log::warn;
use serde::Serialize;
use soundify_core::Location;
use soundify_core::archive::ArchiveStore;
use soundify_core::library::{self, AUDIO_FORMATS, Playlist, VIDEO_FORMATS};
use soundify_core::model::{TaskRecord, UserSettings};
use soundify_core::storage::{LocalStorage, Storage};
use soundify_core::transfer::{self, ExportResult, ImportResult, TransferError};
use soundify_core::{settings, task_history};
use tauri::{AppHandle, Emitter, State};

use crate::app_paths::AppPaths;

pub struct AppState {
    pub paths: AppPaths,
    pub archives: ArchiveStore,
    /// Raised by "Cancelar" while a copy runs; checked between one file and the next.
    pub copy_cancelled: Arc<AtomicBool>,
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

impl CommandError {
    fn io(message: impl ToString) -> Self {
        TransferError::Io(message.to_string()).into()
    }
}

#[derive(Serialize)]
pub struct FormatCatalog {
    audio: Vec<&'static str>,
    video: Vec<&'static str>,
}

/// Always the last known state: never asks YouTube.
#[tauri::command]
pub fn list_playlists(state: State<'_, AppState>) -> Vec<Playlist> {
    library::library(&state.archives, Local::now().date_naive())
}

#[tauri::command]
pub fn formats() -> FormatCatalog {
    FormatCatalog { audio: AUDIO_FORMATS.to_vec(), video: VIDEO_FORMATS.to_vec() }
}

#[tauri::command]
pub fn task_history(state: State<'_, AppState>) -> Vec<TaskRecord> {
    task_history::load(&LocalStorage, &state.paths.task_history())
}

#[tauri::command]
pub fn clear_task_history(state: State<'_, AppState>) -> Result<(), String> {
    task_history::save(&LocalStorage, &state.paths.task_history(), &[]).map_err(|e| e.to_string())
}

/// Missing or broken gives the defaults (see `core::settings`).
#[tauri::command]
pub fn settings(state: State<'_, AppState>) -> UserSettings {
    settings::load(&LocalStorage, &state.paths.settings())
}

// === COPY AND IMPORT ===

/// One step of a copy, emitted as `copy-progress`.
#[derive(Clone, Serialize)]
struct CopyProgress {
    done: usize,
    total: usize,
    file: String,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyOutcome {
    result: ExportResult,
    /// The folder the copy left, the one that can be imported elsewhere.
    folder: String,
    register: RegisterOutcome,
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
            let _ = app.emit("copy-progress", CopyProgress { done, total, file: file.to_string() });
        };
        let result = transfer::export(storage, &json, &destination, &mut progress,
            &|| cancelled.load(Ordering::SeqCst), &now)?;
        let folder_name = transfer::build_manifest(storage, &json, &now)?.folder_name;
        let target = storage.child(&destination, &folder_name)?;
        let register = if !register || result.cancelled {
            RegisterOutcome::NotRequested
        } else {
            match transfer::import_from(storage, &archives, &target, &now) {
                Ok(_) => RegisterOutcome::Registered,
                Err(TransferError::AlreadyRegistered { .. }) => RegisterOutcome::AlreadyRegistered,
                Err(e) => {
                    warn!("Copy done, but registering {target} failed: {e}");
                    RegisterOutcome::Failed
                }
            }
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
