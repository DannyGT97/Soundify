//! The file explorer's commands (`core::browse` does the work): the drives and
//! usual folders, a folder's contents, «Nueva carpeta», and letting the webview
//! show the one image the user picked.
//!
//! All of them hit the disk (a dead network drive can take seconds), so none runs
//! on the main thread.

use std::io::ErrorKind;
use std::path::Path;

use log::warn;
use serde::Serialize;
use soundify_core::Location;
use soundify_core::browse::{self, BrowseEntry, BrowseRoot};
use soundify_core::storage::LocalStorage;
use tauri::{AppHandle, Manager};

use crate::commands::CommandError;

#[derive(Serialize)]
pub struct BrowseRoots {
    places: Vec<BrowseRoot>,
    drives: Vec<BrowseRoot>,
}

#[tauri::command(async)]
pub fn browse_roots() -> BrowseRoots {
    BrowseRoots { places: browse::home_dir().map(|home| browse::places(&home)).unwrap_or_default(), drives: browse::drives() }
}

/// A folder's contents. The error says why in a key the explorer has a state for.
#[tauri::command(async)]
pub fn browse_list(path: String) -> Result<Vec<BrowseEntry>, CommandError> {
    browse::list(&LocalStorage, &Location::path(&path)).map_err(|e| {
        warn!("Could not list {path}: {e}");
        CommandError::key(match e.kind() {
            ErrorKind::PermissionDenied => "explorer.error.blocked",
            ErrorKind::NotFound => "explorer.error.gone",
            _ => "explorer.error.unreadable",
        })
    })
}

/// «Nueva carpeta»: the new folder's path comes back.
#[tauri::command(async)]
pub fn browse_mkdir(parent: String, name: String) -> Result<String, CommandError> {
    browse::make_folder(&LocalStorage, &Location::path(&parent), &name)
        .map(|made| made.to_string())
        .map_err(|e| {
            if let browse::BrowseError::Io(message) = &e {
                warn!("Could not create {name} in {parent}: {message}");
            }
            CommandError::key(e.key())
        })
}

/// The webview shows a local file only inside the asset protocol's scope: the
/// image the user picked is let in (and nothing else of that folder).
#[tauri::command(async)]
pub fn allow_picked_file(app: AppHandle, path: String) {
    if let Err(e) = app.asset_protocol_scope().allow_file(Path::new(&path)) {
        warn!("Could not open {path} to the webview: {e}");
    }
}
