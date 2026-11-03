//! The covers' commands (`core::cover_art` does the work) and the asset
//! protocol's scope: the webview can only load a local file through
//! `convertFileSrc` if it falls inside a folder allowed here.

use std::path::Path;

use log::warn;
use soundify_core::Location;
use soundify_core::cover_art::{self, IMAGE_EXTENSIONS, Thumbnail, ThumbnailFetcher};
use soundify_core::storage::LocalStorage;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::commands::CommandError;

/// Lets the webview load files from inside `dir` (and its subfolders).
pub fn allow_folder<R: Runtime>(app: &AppHandle<R>, dir: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        warn!("Could not open {} to the webview: {e}", dir.display());
    }
}

/// The thumbnail of `id` if it is on disk; otherwise it is fetched in the
/// background and `thumbnail-ready` carries the id once it lands.
#[tauri::command]
pub fn thumbnail(covers: State<'_, ThumbnailFetcher>, id: String) -> Option<String> {
    match covers.thumbnail(&id) {
        Thumbnail::Ready(file) => Some(file.display().to_string()),
        Thumbnail::Pending | Thumbnail::Unavailable => None,
    }
}

/// The file picker for a cover. The picked file is opened to the webview, so
/// «Crear Playlist» can show it before there is a playlist to copy it into.
/// It belongs to the app's window: without a parent it was a loose window,
/// and pressing «Cambiar portada» again with it open stacked a second one.
#[tauri::command]
pub async fn pick_cover_image(app: AppHandle, window: WebviewWindow, title: String, filter: String) -> Option<String> {
    let picked = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title(title)
        .add_filter(filter, &IMAGE_EXTENSIONS)
        .blocking_pick_file()?;
    let path = picked.into_path().ok()?;
    if let Err(e) = app.asset_protocol_scope().allow_file(&path) {
        warn!("Could not open {} to the webview: {e}", path.display());
    }
    Some(path.display().to_string())
}

/// Makes `image` the cover of the playlist `block_key` belongs to (every
/// format of it shows the same one).
#[tauri::command]
pub fn set_cover(block_key: String, image: String) -> Result<(), CommandError> {
    let root = cover_art::root_dir_of(&Location::parse(&block_key)).ok_or_else(|| CommandError::key("cover.choose.failed"))?;
    cover_art::set_custom_cover(&LocalStorage, &root, &Location::parse(&image)).map_err(|e| {
        warn!("Could not set the cover of {root}: {e}");
        CommandError::key("cover.choose.failed")
    })
}
