// The commands are thin: every piece of logic lives in `soundify-core`, which
// knows nothing of Tauri. The download pipeline (yt-dlp, ffmpeg, deno) joins
// them in later phases of MIGRATION-PLAN.md.

mod app_paths;
mod commands;

use std::sync::Arc;

use soundify_core::archive::ArchiveStore;
use soundify_core::clock;
use soundify_core::storage::LocalStorage;

use app_paths::AppPaths;
use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let paths = AppPaths::resolve();
  let archives = ArchiveStore::new(Arc::new(LocalStorage), paths.archives(), clock::system());

  // Startup reconciliation (the Java Bootstrap): which ids still have a file.
  // Off the main thread, and never fatal: the library is "last known state".
  let reconciling = archives.clone();
  std::thread::spawn(move || {
    reconciling.reconcile_all();
  });

  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init())
    .manage(AppState { paths, archives, copy_cancelled: Default::default() })
    .invoke_handler(tauri::generate_handler![
      commands::list_playlists,
      commands::formats,
      commands::task_history,
      commands::clear_task_history,
      commands::settings,
      commands::copy_playlist,
      commands::cancel_copy,
      commands::scan_import,
      commands::import_playlists,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
