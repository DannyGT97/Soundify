//! What only exists on the phone: the Kotlin side (`SoundifyPlugin.kt`, in
//! `gen/android`) unpacks the tools that travel in the APK, asks for access to
//! the storage, picks a folder and keeps the app alive while it downloads.
//! Off Android the commands exist too (the front asks the same questions
//! everywhere) and answer "nothing to do".

use serde::Serialize;
use tauri::AppHandle;

#[cfg(target_os = "android")]
pub use android::{plugin, prepare_in_background};

/// The storage as the front needs it (`StorageInfo` in `src/api/types.ts`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    /// Whether the app may write in the phone's shared folders. Always true off Android.
    pub all_files: bool,
    /// Where the music goes by default (`Music/Soundify`); None off Android.
    pub music_dir: Option<String>,
}

#[tauri::command]
pub async fn storage_info(app: AppHandle) -> Result<StorageInfo, crate::commands::CommandError> {
    run_blocking(move || storage_info_now(&app)).await
}

/// Opens the system's «All files access» screen; true if they granted it.
#[tauri::command]
pub async fn request_all_files(app: AppHandle) -> Result<bool, crate::commands::CommandError> {
    let granted = run_blocking({
        let app = app.clone();
        move || request_all_files_now(&app)
    })
    .await?;
    if granted {
        reconcile_in_background(app);
    }
    Ok(granted)
}

/// Looks which ids still have a file (the startup reconciliation) on a thread
/// of its own, and tells the front to read the library again. With no access
/// to the storage it does nothing: the folders would look empty and every
/// downloaded file would be marked as gone.
pub fn reconcile_in_background(app: AppHandle) {
    std::thread::spawn(move || {
        if !storage_info_now(&app).map(|info| info.all_files).unwrap_or(false) {
            log::info!("No access to the storage yet: the library is not reconciled");
            return;
        }
        use tauri::{Emitter, Manager};
        app.state::<crate::commands::AppState>().archives.clone().reconcile_all();
        let _ = app.emit("file-downloaded", ());
    });
}

/// The system's folder picker, which hands back a real path (the music must
/// be a folder yt-dlp and ffmpeg can write in). None = they cancelled.
#[tauri::command]
pub async fn pick_phone_folder(app: AppHandle) -> Result<Option<String>, crate::commands::CommandError> {
    run_blocking(move || pick_folder_now(&app)).await
}

/// The music player of the phone (`PlaybackService`, a Kotlin service with
/// the notification and lock-screen controls). `action` is one of
/// `playerLoad|playerPlay|playerPause|playerSeek|playerStop|playerGetVolume|
/// playerSetVolume`; what happens in the player (and the phone's volume
/// moving) comes back as the `native-player` event, and what the command
/// itself answers (the volume) as its result.
#[tauri::command]
pub async fn native_player(app: AppHandle, action: String, args: Option<serde_json::Value>) -> Result<serde_json::Value, crate::commands::CommandError> {
    if !PLAYER_ACTIONS.contains(&action.as_str()) {
        return Err(crate::commands::CommandError::io(format!("unknown player action: {action}")));
    }
    run_blocking(move || native_player_now(&app, &action, args.unwrap_or(serde_json::Value::Null))).await
}

const PLAYER_ACTIONS: [&str; 10] =
    ["playerLoad", "playerPlay", "playerPause", "playerSeek", "playerStop", "playerGetVolume", "playerSetVolume", "playerSetModes", "playerOutputDevice", "playerPendingOpen"];

/// Starts listening to the player so its events reach the page. Android only.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn listen_player(app: &AppHandle) {
    #[cfg(target_os = "android")]
    android::listen_player(app);
    #[cfg(not(target_os = "android"))]
    let _ = app;
}

#[cfg(not(target_os = "android"))]
fn native_player_now(_: &AppHandle, _: &str, _: serde_json::Value) -> Result<serde_json::Value, String> {
    Err("there is no native player outside Android".to_string())
}

#[cfg(target_os = "android")]
fn native_player_now(app: &AppHandle, action: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    android::native_player(app, action, args)
}

async fn run_blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, crate::commands::CommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(crate::commands::CommandError::io)?
        .map_err(crate::commands::CommandError::io)
}

/// The foreground notice that keeps Android from freezing the app while it
/// downloads. Does nothing elsewhere.
pub fn set_busy(app: &AppHandle, busy: bool, notice: &BusyNotice) {
    #[cfg(target_os = "android")]
    android::set_busy(app, busy, notice);
    #[cfg(not(target_os = "android"))]
    let _ = (app, busy, notice);
}

/// What that notification says: a title with the state, the line under it and how far it is (0–100; None = not known yet).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub struct BusyNotice {
    pub title: String,
    pub text: String,
    pub percent: Option<u8>,
}

#[cfg(not(target_os = "android"))]
pub(crate) fn storage_info_now(_: &AppHandle) -> Result<StorageInfo, String> {
    Ok(StorageInfo { all_files: true, music_dir: None })
}

#[cfg(not(target_os = "android"))]
fn request_all_files_now(_: &AppHandle) -> Result<bool, String> {
    Ok(true)
}

#[cfg(not(target_os = "android"))]
fn pick_folder_now(_: &AppHandle) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(target_os = "android")]
pub(crate) fn storage_info_now(app: &AppHandle) -> Result<StorageInfo, String> {
    android::storage_info(app)
}

#[cfg(target_os = "android")]
fn request_all_files_now(app: &AppHandle) -> Result<bool, String> {
    android::request_all_files(app)
}

#[cfg(target_os = "android")]
fn pick_folder_now(app: &AppHandle) -> Result<Option<String>, String> {
    android::pick_folder(app)
}

#[cfg(not(target_os = "android"))]
pub fn update_ytdlp_now(_: &AppHandle) -> Result<bool, String> {
    Err("yt-dlp is only updated this way on Android".to_string())
}

/// Updates yt-dlp on the phone (the library downloads the newest stable one
/// only if it is not the one in place). True if a new version was installed.
#[cfg(target_os = "android")]
pub fn update_ytdlp_now(app: &AppHandle) -> Result<bool, String> {
    android::update_ytdlp(app)
}

/// The startup update: skipped if the user is already updating by hand, and the
/// formats are asked again after it (a new yt-dlp may support other ones).
#[cfg(target_os = "android")]
fn update_ytdlp_claimed(app: &AppHandle) {
    use tauri::Manager;
    let state = app.state::<crate::commands::AppState>();
    if !state.installing.lock().unwrap().insert(soundify_core::tools::progress::Tool::Ytdlp) {
        return;
    }
    let result = update_ytdlp_now(app);
    state.installing.lock().unwrap().remove(&soundify_core::tools::progress::Tool::Ytdlp);
    match result {
        Ok(true) => {
            log::info!("yt-dlp was updated");
            crate::bootstrap::cache_formats(&state.tools, &state.formats);
        }
        Ok(false) => log::info!("yt-dlp is up to date"),
        Err(e) => log::warn!("Could not update yt-dlp: {e}"),
    }
}

/// The cookies chooser on the phone: the dialog plugin hands back a `content://`
/// URI there, so Kotlin picks the file and copies it into the app's folder.
#[cfg(target_os = "android")]
pub fn pick_cookies_now(app: &AppHandle) -> Result<Option<String>, String> {
    android::pick_cookies(app)
}

/// The image chooser for a cover on the phone (the dialog plugin returns a `content://` URI there).
#[cfg(target_os = "android")]
pub fn pick_image_now(app: &AppHandle) -> Result<Option<String>, String> {
    android::pick_image(app)
}

/// Shows a folder in the phone's file browser.
#[cfg(target_os = "android")]
pub fn open_folder_now(app: &AppHandle, folder: &str) -> Result<(), String> {
    android::open_folder(app, folder)
}

/// Before downloading an update: may the app install apps? `Ok(false)` = not yet,
/// and the screen where the user allows it is now open.
#[cfg(target_os = "android")]
pub fn allow_install_now(app: &AppHandle) -> Result<bool, String> {
    android::allow_install(app)
}

/// Gives a downloaded and verified APK to Android's installer. `Ok(true)` =
/// the installer is open; `Ok(false)` = the user first has to allow Soundify to
/// install apps (that screen is now open).
#[cfg(target_os = "android")]
pub fn install_apk_now(app: &AppHandle, apk: &std::path::Path) -> Result<bool, String> {
    android::install_apk(app, apk)
}

#[cfg(target_os = "android")]
mod android {
    use std::collections::BTreeMap;

    use serde::Deserialize;
    use soundify_core::tools::Launcher;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{AppHandle, Emitter, Manager, Wry};

    use super::StorageInfo;
    use crate::commands::AppState;

    /// The Kotlin class lives in the app's own package (`gen/android`).
    const PLUGIN_PACKAGE: &str = "com.dannygt97.soundify";

    pub struct Bridge(PluginHandle<Wry>);

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("soundify-android")
            .setup(|app, api| {
                let handle = api.register_android_plugin(PLUGIN_PACKAGE, "SoundifyPlugin")?;
                app.manage(Bridge(handle));
                Ok(())
            })
            .build()
    }

    fn bridge(app: &AppHandle) -> Result<tauri::State<'_, Bridge>, String> {
        app.try_state::<Bridge>().ok_or_else(|| "the Android plugin is not loaded".to_string())
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StorageWire {
        all_files: bool,
        music_dir: String,
    }

    pub fn storage_info(app: &AppHandle) -> Result<StorageInfo, String> {
        let wire: StorageWire = bridge(app)?.0.run_mobile_plugin("storageInfo", ()).map_err(|e| e.to_string())?;
        Ok(StorageInfo { all_files: wire.all_files, music_dir: Some(wire.music_dir) })
    }

    #[derive(Deserialize)]
    struct Granted {
        granted: bool,
    }

    pub fn request_all_files(app: &AppHandle) -> Result<bool, String> {
        let granted: Granted = bridge(app)?.0.run_mobile_plugin("requestAllFiles", ()).map_err(|e| e.to_string())?;
        Ok(granted.granted)
    }

    #[derive(Deserialize)]
    struct Picked {
        path: Option<String>,
        unsupported: Option<String>,
    }

    pub fn pick_folder(app: &AppHandle) -> Result<Option<String>, String> {
        let picked: Picked = bridge(app)?.0.run_mobile_plugin("pickFolder", ()).map_err(|e| e.to_string())?;
        if let Some(uri) = picked.unsupported {
            return Err(format!("that folder has no path on the phone: {uri}"));
        }
        Ok(picked.path)
    }

    /// The cookies file the user picked, already copied where yt-dlp can read it.
    pub fn pick_cookies(app: &AppHandle) -> Result<Option<String>, String> {
        let picked: Picked = bridge(app)?.0.run_mobile_plugin("pickCookies", ()).map_err(|e| e.to_string())?;
        Ok(picked.path)
    }

    /// The cover image the user picked, already copied where the app can read it.
    pub fn pick_image(app: &AppHandle) -> Result<Option<String>, String> {
        let picked: Picked = bridge(app)?.0.run_mobile_plugin("pickImage", ()).map_err(|e| e.to_string())?;
        Ok(picked.path)
    }

    pub fn open_folder(app: &AppHandle, folder: &str) -> Result<(), String> {
        bridge(app)?
            .0
            .run_mobile_plugin::<serde_json::Value>("openFolder", serde_json::json!({ "path": folder }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Installing {
        needs_permission: bool,
    }

    pub fn allow_install(app: &AppHandle) -> Result<bool, String> {
        let answer: Installing = bridge(app)?.0.run_mobile_plugin("allowInstall", ()).map_err(|e| e.to_string())?;
        Ok(!answer.needs_permission)
    }

    pub fn install_apk(app: &AppHandle, apk: &std::path::Path) -> Result<bool, String> {
        let answer: Installing = bridge(app)?
            .0
            .run_mobile_plugin("installApk", serde_json::json!({ "path": apk.to_string_lossy() }))
            .map_err(|e| e.to_string())?;
        Ok(!answer.needs_permission)
    }

    #[derive(serde::Serialize)]
    struct BusyArgs<'a> {
        busy: bool,
        title: &'a str,
        text: &'a str,
        /// 0–100, or -1 while it is not known (the bar then goes back and forth).
        progress: i32,
    }

    pub fn set_busy(app: &AppHandle, busy: bool, notice: &super::BusyNotice) {
        let Ok(bridge) = bridge(app) else { return };
        let args = BusyArgs { busy, title: &notice.title, text: &notice.text, progress: notice.percent.map_or(-1, i32::from) };
        if let Err(e) = bridge.0.run_mobile_plugin::<serde_json::Value>("setBusy", args) {
            log::warn!("Could not change the foreground notice: {e}");
        }
    }

    pub fn native_player(app: &AppHandle, action: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        bridge(app)?.0.run_mobile_plugin::<serde_json::Value>(action, args).map_err(|e| e.to_string())
    }

    /// Every event of the Kotlin player (`time`, `duration`, `ended`, `error`,
    /// `playing`, `skip`) goes to the page as `native-player`, as it came.
    pub fn listen_player(app: &AppHandle) {
        let app = app.clone();
        // On a thread of its own: the plugin answers from the main thread, which is busy running the setup.
        std::thread::spawn(move || listen_player_now(&app));
    }

    fn listen_player_now(app: &AppHandle) {
        let Ok(bridge) = bridge(app) else { return };
        let handle = app.clone();
        let channel = tauri::ipc::Channel::<serde_json::Value>::new(move |body| {
            let value = match body {
                tauri::ipc::InvokeResponseBody::Json(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
                tauri::ipc::InvokeResponseBody::Raw(_) => serde_json::Value::Null,
            };
            let _ = handle.emit("native-player", value);
            Ok(())
        });
        let args = serde_json::json!({ "event": "player", "handler": channel });
        if let Err(e) = bridge.0.run_mobile_plugin::<serde_json::Value>("registerListener", args) {
            log::warn!("Could not listen to the player: {e}");
        }
    }

    /// How the Kotlin side says yt-dlp is started (`SoundifyPlugin.runtimeDescription`).
    #[derive(Deserialize)]
    struct RuntimeWire {
        program: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        ffmpeg: String,
    }

    #[derive(Deserialize)]
    struct Updated {
        updated: bool,
    }

    /// Asks the Kotlin library to put the newest stable yt-dlp in place; true if there was one.
    pub fn update_ytdlp(app: &AppHandle) -> Result<bool, String> {
        let answer: Updated = bridge(app)?.0.run_mobile_plugin("updateYtDlp", ()).map_err(|e| e.to_string())?;
        Ok(answer.updated)
    }

    /// Unpacks the tools on a thread of its own (the first time takes a few
    /// seconds), then tells the front so it stops saying they are missing.
    /// With Ajustes → «actualizar herramientas solas» on, yt-dlp is then brought
    /// up to date: the one in the APK is as old as the APK.
    pub fn prepare_in_background(app: AppHandle) {
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            let wire: Result<RuntimeWire, String> = bridge(&app).and_then(|b| b.0.run_mobile_plugin("prepareRuntime", ()).map_err(|e| e.to_string()));
            let ok = wire.is_ok();
            match wire {
                Ok(wire) => {
                    log::info!("Tools unpacked: {}", wire.program);
                    let launcher = Launcher {
                        program: wire.program.into(),
                        args: wire.args,
                        env: wire.env.into_iter().collect(),
                        ffmpeg: vec![wire.ffmpeg],
                    };
                    state.tools.set_launcher(launcher);
                    crate::bootstrap::cache_formats(&state.tools, &state.formats);
                }
                Err(e) => log::error!("Could not unpack the tools: {e}"),
            }
            let _ = app.emit("tools-ready", ok);
            if ok && soundify_core::settings::load(&soundify_core::storage::LocalStorage, &state.paths.settings()).auto_updates {
                super::update_ytdlp_claimed(&app);
                let _ = app.emit("tools-ready", true);
            }
        });
    }
}
