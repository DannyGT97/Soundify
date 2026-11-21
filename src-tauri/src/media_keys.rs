//! The media keys of Windows (the keyboard's play/pause/next/previous, a headset's button, the volume flyout's player): Windows
//! routes them to the app that registered a System Media Transport Controls session. Here one is registered for the main
//! window; what a key asks goes to the page as the event `media-key` (it does it with the same functions as the bar's buttons)
//! and the page says what plays (`media_session`) so Windows shows it and its state, shuffle and repeat included.
//! Elsewhere it does nothing (on Android the notification of the player already is this).

use serde::{Deserialize, Serialize};

/// What the page says about what plays here.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub struct MediaUpdate {
    /// Nothing plays (the player is closed): the session hides.
    pub active: bool,
    pub title: String,
    pub subtitle: String,
    pub playing: bool,
    pub shuffle: bool,
    /// `off`, `all` or `one`.
    pub repeat: String,
}

/// What a key asked for.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(not(windows), allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub struct MediaKey {
    /// `play`, `pause`, `stop`, `next`, `previous`, `shuffle`, `repeat`.
    pub action: &'static str,
    /// For `shuffle`: whether it should be on; for `repeat`: `off`, `all` or `one`.
    pub value: Option<String>,
}

#[cfg(windows)]
mod smtc {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Foundation::TypedEventHandler;
    use windows::Media::{
        AutoRepeatModeChangeRequestedEventArgs, MediaPlaybackAutoRepeatMode, MediaPlaybackStatus, MediaPlaybackType,
        ShuffleEnabledChangeRequestedEventArgs, SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows::core::{HSTRING, Ref};

    use super::{MediaKey, MediaUpdate};

    #[derive(Default)]
    pub struct Session(Mutex<Option<SystemMediaTransportControls>>);

    fn send(app: &AppHandle, action: &'static str, value: Option<String>) {
        let _ = app.emit("media-key", MediaKey { action, value });
    }

    /// Registers the session for the main window and listens to its keys.
    pub fn start(app: &AppHandle) {
        let Some(window) = app.get_webview_window("main").or_else(|| app.webview_windows().into_values().next()) else { return };
        let Ok(hwnd) = window.hwnd() else { return };
        match register(app, HWND(hwnd.0)) {
            Ok(smtc) => *app.state::<Session>().0.lock().unwrap() = Some(smtc),
            Err(e) => log::warn!("Media keys: no transport controls session: {e}"),
        }
    }

    fn register(app: &AppHandle, hwnd: HWND) -> windows::core::Result<SystemMediaTransportControls> {
        let interop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // SAFETY: `hwnd` is the handle of this app's own main window.
        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;
        // Hidden until something plays.
        smtc.SetIsEnabled(false)?;

        let handle = app.clone();
        smtc.ButtonPressed(&TypedEventHandler::new(move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            if let Some(args) = args.as_ref() {
                match args.Button()? {
                    SystemMediaTransportControlsButton::Play => send(&handle, "play", None),
                    SystemMediaTransportControlsButton::Pause => send(&handle, "pause", None),
                    SystemMediaTransportControlsButton::Stop => send(&handle, "stop", None),
                    SystemMediaTransportControlsButton::Next => send(&handle, "next", None),
                    SystemMediaTransportControlsButton::Previous => send(&handle, "previous", None),
                    _ => {}
                }
            }
            Ok(())
        }))?;
        let handle = app.clone();
        smtc.ShuffleEnabledChangeRequested(&TypedEventHandler::new(move |_, args: Ref<ShuffleEnabledChangeRequestedEventArgs>| {
            if let Some(args) = args.as_ref() {
                send(&handle, "shuffle", Some(args.RequestedShuffleEnabled()?.to_string()));
            }
            Ok(())
        }))?;
        let handle = app.clone();
        smtc.AutoRepeatModeChangeRequested(&TypedEventHandler::new(move |_, args: Ref<AutoRepeatModeChangeRequestedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let mode = match args.RequestedAutoRepeatMode()? {
                    MediaPlaybackAutoRepeatMode::Track => "one",
                    MediaPlaybackAutoRepeatMode::List => "all",
                    _ => "off",
                };
                send(&handle, "repeat", Some(mode.to_string()));
            }
            Ok(())
        }))?;
        Ok(smtc)
    }

    pub fn update(app: &AppHandle, update: &MediaUpdate) -> windows::core::Result<()> {
        let session = app.state::<Session>().0.lock().unwrap().clone();
        let Some(smtc) = session else { return Ok(()) };
        if !update.active {
            smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
            smtc.SetIsEnabled(false)?;
            return Ok(());
        }
        smtc.SetIsEnabled(true)?;
        let updater = smtc.DisplayUpdater()?;
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(update.title.as_str()))?;
        music.SetArtist(&HSTRING::from(update.subtitle.as_str()))?;
        updater.Update()?;
        smtc.SetPlaybackStatus(if update.playing { MediaPlaybackStatus::Playing } else { MediaPlaybackStatus::Paused })?;
        smtc.SetShuffleEnabled(update.shuffle)?;
        smtc.SetAutoRepeatMode(match update.repeat.as_str() {
            "one" => MediaPlaybackAutoRepeatMode::Track,
            "all" => MediaPlaybackAutoRepeatMode::List,
            _ => MediaPlaybackAutoRepeatMode::None,
        })?;
        Ok(())
    }
}

#[cfg(windows)]
pub use smtc::Session;

/// What the app keeps for the media keys (nothing where there are none).
#[cfg(not(windows))]
#[derive(Default)]
pub struct Session;

pub fn start(app: &tauri::AppHandle) {
    #[cfg(windows)]
    smtc::start(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// The page says what plays here (and in which state): Windows shows it next to the keys.
#[tauri::command]
pub fn media_session(app: tauri::AppHandle, update: MediaUpdate) {
    #[cfg(windows)]
    if let Err(e) = smtc::update(&app, &update) {
        log::warn!("Media keys: could not update the session: {e}");
    }
    #[cfg(not(windows))]
    let _ = (app, update);
}
