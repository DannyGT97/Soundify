// The commands are thin: every piece of logic lives in `soundify-core`, which
// knows nothing of Tauri.

mod app_paths;
mod app_update;
mod bootstrap;
mod commands;
mod connect;
mod covers;
mod devices;
mod downloads;
mod mobile;
mod mtp;
mod window_menu;
mod process;
mod tools;

use std::sync::Arc;

use soundify_core::archive::ArchiveStore;
use soundify_core::cover_art::ThumbnailFetcher;
use soundify_core::http::{Http, ReqwestHttp};
use soundify_core::engine::tasks::BackgroundTasks;
use soundify_core::logs::FileLogger;
use soundify_core::{clock, task_history};
use soundify_core::storage::LocalStorage;
use tauri::{Emitter, Manager};

use app_paths::AppPaths;
use commands::AppState;

/// A panic used to close the app leaving nothing in the log: it only went to
/// the console, which the portable does not have. Now it is written down
/// first, and then the usual handling goes on.
fn log_panics() {
  let previous = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |info| {
    log::error!("The app panicked: {info}\n{}", std::backtrace::Backtrace::force_capture());
    log::logger().flush();
    previous(info);
  }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // The staged copy of a new version only swaps the app and leaves.
  if app_update::apply_if_asked() {
    return;
  }
  let paths = AppPaths::resolve();
  // First of all: everything after this can already leave a trace.
  if let Err(e) = FileLogger::new(&paths.logs(), clock::system()).install() {
    eprintln!("No log files: {e}");
  }
  log_panics();
  // Started properly: the way back of the last update is no longer needed.
  app_update::clean_up(&paths);
  let archives = ArchiveStore::new(Arc::new(LocalStorage), paths.archives(), clock::system());
  let phone = mtp::current();
  let tools = bootstrap::start(&paths, &archives, &phone);
  let history = task_history::load(&LocalStorage, &paths.task_history());

  let builder = tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init());
  // The Kotlin side: unpacks the tools, asks for the storage, picks folders.
  #[cfg(target_os = "android")]
  let builder = builder.plugin(mobile::plugin());
  builder
    .manage(app_update::AppUpdates::default())
    .manage(AppState {
      paths,
      archives,
      phone,
      copy_cancelled: Default::default(),
      health: tools.health,
      progress: tools.progress,
      formats: tools.formats,
      tools: tools.paths,
      installing: tools.installing,
      downloads: downloads::Downloads::new(BackgroundTasks::new(clock::system(), history)),
    })
    .setup(|app| {
      // Problems noted down before the front listens are asked for with
      // `tool_health`; this carries the ones that come later.
      let handle = app.handle().clone();
      app.state::<AppState>().health.on_problem(Box::new(move |key| {
        let _ = handle.emit("tool-health", key);
      }));
      let handle = app.handle().clone();
      app.state::<AppState>().progress.on_change(Box::new(move |downloads| {
        let _ = handle.emit("tool-downloads", downloads);
      }));

      // The covers: the thumbnails' cache and the picked covers (inside the
      // archives) are the only local pictures the webview loads.
      let paths = &app.state::<AppState>().paths;
      let thumbnails = paths.thumbnails();
      covers::allow_folder(app.handle(), &thumbnails);
      if let Some(archives) = paths.archives().as_path() {
        covers::allow_folder(app.handle(), archives);
      }
      // Without an HTTP client there are no thumbnails, and nothing else is
      // lost: the `thumbnail` command fails and the gradient tile stays.
      match ReqwestHttp::new(env!("CARGO_PKG_VERSION")) {
        Ok(http) => {
          let handle = app.handle().clone();
          let http: Arc<dyn Http> = Arc::new(http);
          app.manage(ThumbnailFetcher::new(thumbnails, http, move |id| {
            let _ = handle.emit("thumbnail-ready", id);
          }));
        }
        Err(e) => log::warn!("No HTTP client for the thumbnails: {e}"),
      }
      // Startup reconciliation: which ids still have a file (and which files
      // another copy of the app added to the same folder). The front may have
      // read the library before it ends, so it is told to read it again.
      // On Android only once the storage can be read: without access every
      // file would look deleted (`mobile::request_all_files` runs it later).
      mobile::reconcile_in_background(app.handle().clone());
      // Soundify Connect: control between devices on the same network (off until the user turns it on).
      connect::init(app.handle(), &app.state::<AppState>().paths);
      // The daily check for a newer version (on Android it offers the APK).
      app_update::check_at_startup(app.handle());
      // On Android the tools come in the APK and are unpacked now.
      #[cfg(target_os = "android")]
      {
        mobile::listen_player(app.handle());
        mobile::prepare_in_background(app.handle().clone());
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      window_menu::show_system_menu,
      mobile::storage_info,
      mobile::request_all_files,
      mobile::pick_phone_folder,
      mobile::native_player,
      commands::list_playlists,
      connect::connect_status,
      connect::connect_set_enabled,
      connect::connect_set_name,
      connect::connect_devices,
      connect::connect_add,
      connect::connect_pin,
      connect::connect_renew_pin,
      connect::connect_pair,
      connect::connect_forget,
      connect::connect_forget_controller,
      connect::connect_select,
      connect::connect_deselect,
      connect::connect_send,
      connect::connect_publish,
      commands::refresh_library,
      commands::formats,
      commands::tool_health,
      commands::tool_downloads,
      tools::tools_report,
      tools::install_tool,
      tools::check_ytdlp_update,
      tools::open_tools_folder,
      commands::tasks_view,
      commands::clear_task_history,
      commands::set_tasks_window_open,
      commands::start_download,
      commands::create_playlist,
      commands::stop_download,
      commands::remove_queued,
      commands::clear_queue,
      commands::activity_log,
      commands::clear_activity,
      commands::settings,
      commands::preferences,
      commands::save_preferences,
      commands::reset_preferences,
      commands::pick_cookies_file,
      commands::file_exists,
      commands::open_folder,
      commands::player_memory,
      commands::save_player_memory,
      commands::copy_playlist,
      commands::cancel_copy,
      commands::scan_import,
      commands::import_playlists,
      commands::first_video_id,
      commands::delete_playlist_format,
      devices::list_devices,
      devices::list_device_folder,
      devices::create_device_folder,
      devices::open_device_folder,
      devices::create_device_playlist,
      devices::copy_to_device,
      devices::copy_from_device,
      devices::scan_device_import,
      devices::import_from_device,
      app_update::check_app_update,
      app_update::install_app_update,
      covers::thumbnail,
      covers::url_thumbnail,
      covers::pick_cover_image,
      covers::set_cover,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
