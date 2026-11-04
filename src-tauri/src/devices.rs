//! The phone commands (`src/api/tauri.ts`): the picker, «Crear Playlist» and
//! the copies to and from a device, and importing what a phone already holds.
//! Every device call blocks (seconds, at worst the per-call bounds of
//! `mtp.rs`), so each one runs off the main thread.

use std::sync::atomic::Ordering;

use log::warn;
use serde::Serialize;
use soundify_core::Location;
use soundify_core::device::{DeviceEntry, DeviceInfo, DeviceTarget, last_segment, parse_folder_name};
use soundify_core::downloader::StopFlag;
use soundify_core::engine::download::{CreateError, DeviceSide};
use soundify_core::storage::{LocalStorage, Storage};
use soundify_core::transfer::{self, ExportResult, TransferError, device as device_transfer};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{AppState, CommandError, CopyOutcome, CopyProgress, CreatedView, ImportCandidate, RegisterOutcome};
use crate::downloads;

/// What the picker shows: whether this system can talk to phones at all, and
/// the ones connected right now.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesView {
    available: bool,
    devices: Vec<DeviceInfo>,
}

#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> Result<DevicesView, CommandError> {
    let phone = state.phone.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let available = phone.is_available();
        DevicesView { available, devices: if available { phone.list_devices() } else { Vec::new() } }
    })
    .await
    .map_err(CommandError::io)
}

/// One folder of a phone; the picker keeps only the subfolders.
#[tauri::command]
pub async fn list_device_folder(state: State<'_, AppState>, device_id: String, path: String) -> Result<Vec<DeviceEntry>, CommandError> {
    let phone = state.phone.clone();
    tauri::async_runtime::spawn_blocking(move || phone.list(&device_id, &path))
        .await
        .map_err(CommandError::io)
}

/// «Nueva carpeta» of the picker; `path` includes the new folder's name.
#[tauri::command]
pub async fn create_device_folder(state: State<'_, AppState>, device_id: String, path: String) -> Result<bool, CommandError> {
    let phone = state.phone.clone();
    tauri::async_runtime::spawn_blocking(move || phone.mkdirs(&device_id, &path))
        .await
        .map_err(CommandError::io)
}

/// «Abrir carpeta» of a device playlist: Windows' own explorer on the phone.
#[tauri::command]
pub async fn open_device_folder(state: State<'_, AppState>, device_id: String, path: String) -> Result<(), CommandError> {
    let phone = state.phone.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || phone.open_in_explorer(&device_id, &path))
        .await
        .map_err(CommandError::io)?;
    if opened { Ok(()) } else { Err(CommandError::key("manager.openFolder.deviceFailed")) }
}

/// «Crear Playlist» with a phone as the destination; `target.path` is the
/// base the picker returned (the playlist gets its own subfolder inside).
#[tauri::command]
pub async fn create_device_playlist(
    state: State<'_, AppState>,
    url: String,
    format: String,
    target: DeviceTarget,
    numbered: bool,
) -> Result<CreatedView, CommandError> {
    if !soundify_core::tools::status::can_download(&state.tools) {
        return Err(CommandError::key(downloads::TOOLS_MISSING));
    }
    let archives = state.archives.clone();
    let tools = state.tools.clone();
    let settings_file = state.paths.settings();
    let phone = state.phone.clone();
    let staging_root = state.paths.device_staging();
    tauri::async_runtime::spawn_blocking(move || {
        let side = DeviceSide { phone: phone.as_ref(), staging_root };
        downloads::with_engine(&archives, &tools, &settings_file, &StopFlag::new(), |engine| {
            engine.create_device_playlist(&side, &url, &format, &target, numbered)
        })
    })
    .await
    .map_err(CommandError::io)?
    .map(CreatedView::from)
    .map_err(|e| match e {
        CreateError::CookiesExpired => CommandError::key("newPlaylist.cookiesExpired"),
        CreateError::QueryFailed => CommandError::key("newPlaylist.queryFailed"),
        CreateError::Io(e) => {
            warn!("Could not register the device playlist: {e}");
            CommandError::with_args("newPlaylist.registerFailed", vec![e.to_string()])
        }
    })
}

/// «Copiar a un dispositivo…»: the playlist's own folder inside `target.path`,
/// with the manifest, so the phone can be imported from elsewhere.
#[tauri::command]
pub async fn copy_to_device(app: AppHandle, state: State<'_, AppState>, block_key: String, target: DeviceTarget) -> Result<ExportResult, CommandError> {
    let cancelled = state.copy_cancelled.clone();
    cancelled.store(false, Ordering::SeqCst);
    let now = state.archives.now();
    let phone = state.phone.clone();
    let scratch = state.paths.device_scratch();
    let json = Location::parse(&block_key);
    tauri::async_runtime::spawn_blocking(move || {
        let mut progress = progress_emitter(&app);
        device_transfer::export_to_device(&LocalStorage, phone.as_ref(), &json, &target, &mut progress,
            &|| cancelled.load(Ordering::SeqCst), &scratch, &now)
    })
    .await
    .map_err(CommandError::io)?
    .map_err(|e| failed_copy(&block_key, e))
}

/// «Copiar a este ordenador…» of a device playlist: the local copy, and (by
/// default) registered right away, as a local copy is.
#[tauri::command]
pub async fn copy_from_device(
    app: AppHandle,
    state: State<'_, AppState>,
    block_key: String,
    destination: String,
    register: bool,
) -> Result<CopyOutcome, CommandError> {
    let cancelled = state.copy_cancelled.clone();
    cancelled.store(false, Ordering::SeqCst);
    let now = state.archives.now();
    let phone = state.phone.clone();
    let archives = state.archives.clone();
    let json = Location::parse(&block_key);
    tauri::async_runtime::spawn_blocking(move || {
        let storage = &LocalStorage;
        let destination = Location::parse(&destination);
        let mut progress = progress_emitter(&app);
        let result = device_transfer::export_from_device(storage, phone.as_ref(), &json, &destination, &mut progress,
            &|| cancelled.load(Ordering::SeqCst), &now)?;
        let data = archives.load(&json).map_err(|e| TransferError::Io(e.to_string()))?;
        let folder_name = device_transfer::build_manifest(storage, &json, &data, &now).folder_name;
        let target = storage.child(&destination, &folder_name)?;
        let register = if !register || result.cancelled {
            RegisterOutcome::NotRequested
        } else {
            RegisterOutcome::of(transfer::import_from(storage, archives.archives(), &target, &now), &target)
        };
        Ok(CopyOutcome::new(result, target.to_string(), register))
    })
    .await
    .map_err(CommandError::io)?
    .map_err(|e| failed_copy(&block_key, e))
}

/// The playlists a phone holds under `root` (two levels down) that are not
/// registered yet. The title comes from the folder's "Title (format)" name:
/// pulling every manifest just to list them would take seconds each.
#[tauri::command]
pub async fn scan_device_import(
    state: State<'_, AppState>,
    device_id: String,
    device_name: String,
    root: String,
) -> Result<DeviceImportScan, CommandError> {
    let phone = state.phone.clone();
    let archives = state.archives.archives().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !phone.is_reachable(&device_id) {
            return Err(CommandError::key("transfer.deviceUnreachable"));
        }
        let found = device_transfer::find_importable(phone.as_ref(), &device_id, &device_name, &root);
        let candidates = device_transfer::not_yet_registered(&LocalStorage, &archives, &found)
            .into_iter()
            .map(|target| {
                let name = last_segment(&target.path).to_string();
                let (title, format) = parse_folder_name(&name).map_or((name.clone(), String::new()), |(t, f)| (t.to_string(), f.to_uppercase()));
                ImportCandidateOn { target, candidate: ImportCandidate::new(name, title, format) }
            })
            .collect();
        Ok(DeviceImportScan { found: found.len(), candidates })
    })
    .await
    .map_err(CommandError::io)?
}

/// A candidate as the folder import shows it, plus where it is on the phone
/// (what [`import_from_device`] takes back).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidateOn {
    #[serde(flatten)]
    candidate: ImportCandidate,
    target: DeviceTarget,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportScan {
    found: usize,
    candidates: Vec<ImportCandidateOn>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportOutcomeView {
    folder: String,
    result: Option<transfer::ImportResult>,
    error: Option<CommandError>,
    already_registered: bool,
}

/// Registers each location; one failing never stops the rest. Nothing is
/// copied: the music is already on the phone. `copy-progress` says which one
/// goes, since pulling a manifest takes seconds.
#[tauri::command]
pub async fn import_from_device(app: AppHandle, state: State<'_, AppState>, targets: Vec<DeviceTarget>) -> Result<Vec<DeviceImportOutcomeView>, CommandError> {
    let phone = state.phone.clone();
    let archives = state.archives.archives().clone();
    let scratch = state.paths.device_scratch();
    let now = state.archives.now();
    tauri::async_runtime::spawn_blocking(move || {
        let mut progress = progress_emitter(&app);
        device_transfer::import_all(&LocalStorage, phone.as_ref(), &archives, &targets, &mut progress, &scratch, &now)
            .into_iter()
            .map(|outcome| DeviceImportOutcomeView {
                folder: last_segment(&outcome.location.path).to_string(),
                result: outcome.result,
                error: outcome.error.map(CommandError::from),
                already_registered: outcome.already_registered,
            })
            .collect()
    })
    .await
    .map_err(CommandError::io)
}

// === PRIVATE HELPERS ===

fn progress_emitter(app: &AppHandle) -> impl FnMut(usize, usize, &str) + '_ {
    move |done, total, file| {
        let _ = app.emit("copy-progress", CopyProgress::new(done, total, file));
    }
}

fn failed_copy(block_key: &str, error: TransferError) -> CommandError {
    warn!("Device copy of {block_key} failed: {error}");
    error.into()
}
