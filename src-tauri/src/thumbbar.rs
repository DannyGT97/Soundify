//! The buttons Windows shows under the preview of the app's window when the mouse is over its icon in the taskbar (like Spotify's):
//! previous, play/pause, next, shuffle and repeat. They are the «thumbnail toolbar» of the window (`ITaskbarList3`): the window is
//! subclassed to hear them, a click goes to the page as the same `media-key` event the keyboard's media keys send, and the page's
//! `media_session` updates say which icon each one shows (the pause icon while it plays, shuffle and repeat in the accent colour
//! while they are on). The icons are the PNGs of `icons/thumbbar` (`scripts/make-thumbbar-icons.mjs`): light for the dark taskbar,
//! dark for the light one. Nothing here does anything off Windows.

use serde::Deserialize;

/// The texts of the buttons' tooltips, already in the language of the app.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Tips {
    pub previous: String,
    pub play: String,
    pub pause: String,
    pub next: String,
    pub shuffle: String,
    pub repeat: String,
}

#[cfg(windows)]
pub use imp::{start, update};

#[cfg(not(windows))]
pub fn start(_app: &tauri::AppHandle) {}

#[cfg(not(windows))]
pub fn update(_app: &tauri::AppHandle, _update: &crate::media_keys::MediaUpdate) {}

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::Win32::UI::Shell::{
        DefSubclassProc, ITaskbarList3, SetWindowSubclass, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED, THBF_ENABLED, THBN_CLICKED, THUMBBUTTON,
        TaskbarList,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, HICON, LR_DEFAULTCOLOR, RegisterWindowMessageW, WM_COMMAND};
    use windows::core::{PCWSTR, w};

    use crate::media_keys::{MediaKey, MediaUpdate};

    const PREVIOUS: u32 = 1;
    const PLAY: u32 = 2;
    const NEXT: u32 = 3;
    const SHUFFLE: u32 = 4;
    const REPEAT: u32 = 5;

    /// Every icon as a handle, by name (loaded once, for the taskbar's theme).
    struct Icons(Vec<(&'static str, HICON)>);

    impl Icons {
        fn get(&self, name: &str) -> HICON {
            self.0.iter().find(|(n, _)| *n == name).map(|(_, icon)| *icon).unwrap_or_default()
        }
    }

    struct State {
        app: AppHandle,
        hwnd: HWND,
        taskbar: ITaskbarList3,
        icons: Icons,
        added: bool,
        last: Option<MediaUpdate>,
    }

    // SAFETY: the COM object and the handles are only touched on the main thread (the window's own: the subclass runs there and
    // `update` goes through `run_on_main_thread`).
    unsafe impl Send for State {}

    static STATE: Mutex<Option<State>> = Mutex::new(None);

    macro_rules! png {
        ($theme:literal, $name:literal) => {
            ($name, include_bytes!(concat!("../icons/thumbbar/", $theme, "/", $name, ".png")) as &[u8])
        };
    }

    fn pngs(light: bool) -> Vec<(&'static str, &'static [u8])> {
        macro_rules! set {
            ($theme:literal) => {
                vec![
                    png!($theme, "previous"), png!($theme, "next"), png!($theme, "play"), png!($theme, "pause"),
                    png!($theme, "shuffle-off"), png!($theme, "shuffle-on"), png!($theme, "repeat-off"), png!($theme, "repeat-all"), png!($theme, "repeat-one"),
                ]
            };
        }
        if light { set!("light") } else { set!("dark") }
    }

    /// Is the taskbar light? (Windows' own setting: the preview takes its colours from it.)
    fn taskbar_is_light() -> bool {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `value` and `size` live through the call.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w!("SystemUsesLightTheme"),
                RRF_RT_REG_DWORD, None, Some((&mut value as *mut u32).cast()), Some(&mut size),
            )
        };
        read.is_ok() && value == 1
    }

    fn load_icons() -> Icons {
        let loaded = pngs(taskbar_is_light())
            .into_iter()
            .filter_map(|(name, bytes)| {
                // SAFETY: `bytes` is a PNG (the format Windows reads as an icon resource since Vista).
                unsafe { CreateIconFromResourceEx(bytes, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR) }.ok().map(|icon| (name, icon))
            })
            .collect();
        Icons(loaded)
    }

    fn tip(text: &str) -> [u16; 260] {
        let mut out = [0u16; 260];
        for (slot, unit) in out.iter_mut().take(259).zip(text.encode_utf16()) {
            *slot = unit;
        }
        out
    }

    fn button(id: u32, icon: HICON, text: &str, enabled: bool) -> THUMBBUTTON {
        THUMBBUTTON {
            dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
            iId: id,
            hIcon: icon,
            szTip: tip(text),
            dwFlags: if enabled { THBF_ENABLED } else { THBF_DISABLED },
            ..Default::default()
        }
    }

    /// The five buttons as `update` says they should look (before anything plays: disabled, with the plain icons).
    fn buttons(icons: &Icons, update: Option<&MediaUpdate>) -> [THUMBBUTTON; 5] {
        let blank = MediaUpdate { active: false, title: String::new(), subtitle: String::new(), playing: false, shuffle: false, repeat: "off".into(), tips: Default::default() };
        let u = update.unwrap_or(&blank);
        let enabled = u.active;
        let repeat_icon = match u.repeat.as_str() {
            "one" => "repeat-one",
            "all" => "repeat-all",
            _ => "repeat-off",
        };
        [
            button(PREVIOUS, icons.get("previous"), &u.tips.previous, enabled),
            button(PLAY, icons.get(if u.playing { "pause" } else { "play" }), if u.playing { &u.tips.pause } else { &u.tips.play }, enabled),
            button(NEXT, icons.get("next"), &u.tips.next, enabled),
            button(SHUFFLE, icons.get(if u.shuffle { "shuffle-on" } else { "shuffle-off" }), &u.tips.shuffle, enabled),
            button(REPEAT, icons.get(repeat_icon), &u.tips.repeat, enabled),
        ]
    }

    /// Puts the buttons under the preview (the taskbar says when its button for the window exists: `TaskbarButtonCreated`).
    fn add_buttons(state: &mut State) {
        let list = buttons(&state.icons, state.last.as_ref());
        // SAFETY: the window is this app's own and `list` lives through the call.
        match unsafe { state.taskbar.ThumbBarAddButtons(state.hwnd, &list) } {
            Ok(()) => {
                state.added = true;
                log::info!("Taskbar: the preview's buttons are in place");
            }
            Err(e) => log::warn!("Taskbar: could not add the preview's buttons: {e}"),
        }
    }

    fn refresh(state: &mut State) {
        if !state.added {
            return;
        }
        let list = buttons(&state.icons, state.last.as_ref());
        // SAFETY: as above.
        if let Err(e) = unsafe { state.taskbar.ThumbBarUpdateButtons(state.hwnd, &list) } {
            log::warn!("Taskbar: could not update the preview's buttons: {e}");
        }
    }

    fn send(app: &AppHandle, action: &'static str, value: Option<String>) {
        let _ = app.emit("media-key", MediaKey { action, value });
    }

    /// A button of the preview was pressed: what it asks, from how things are now.
    fn clicked(state: &State, id: u32) {
        let last = state.last.as_ref();
        let playing = last.is_some_and(|u| u.playing);
        let shuffle = last.is_some_and(|u| u.shuffle);
        let repeat = last.map_or("off", |u| u.repeat.as_str());
        match id {
            PREVIOUS => send(&state.app, "previous", None),
            NEXT => send(&state.app, "next", None),
            PLAY => send(&state.app, if playing { "pause" } else { "play" }, None),
            SHUFFLE => send(&state.app, "shuffle", Some((!shuffle).to_string())),
            REPEAT => send(&state.app, "repeat", Some(match repeat { "off" => "all", "all" => "one", _ => "off" }.to_string())),
            _ => {}
        }
    }

    /// The window's messages: the taskbar's «your button exists» and a click on one of the preview's buttons.
    unsafe extern "system" fn subclass(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, created: usize) -> LRESULT {
        if message == created as u32 && created != 0 {
            if let Some(state) = STATE.lock().unwrap().as_mut() {
                add_buttons(state);
            }
        } else if message == WM_COMMAND && ((wparam.0 >> 16) & 0xFFFF) as u32 == THBN_CLICKED {
            if let Some(state) = STATE.lock().unwrap().as_ref() {
                clicked(state, (wparam.0 & 0xFFFF) as u32);
            }
            return LRESULT(0);
        }
        // SAFETY: the next procedure of the subclass chain, with what this one got.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    pub fn start(app: &AppHandle) {
        let Some(window) = app.get_webview_window("main").or_else(|| app.webview_windows().into_values().next()) else { return };
        let Ok(hwnd) = window.hwnd() else { return };
        let hwnd = HWND(hwnd.0);
        // SAFETY: COM was started by the window's own thread (this is the main one).
        let taskbar: windows::core::Result<ITaskbarList3> = unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) };
        let taskbar = match taskbar {
            // SAFETY: nothing to hold on to: initialises the object.
            Ok(taskbar) => match unsafe { taskbar.HrInit() } {
                Ok(()) => taskbar,
                Err(e) => return log::warn!("Taskbar: the preview's buttons cannot start: {e}"),
            },
            Err(e) => return log::warn!("Taskbar: the preview's buttons cannot start: {e}"),
        };
        let icons = load_icons();
        let mut state = State { app: app.clone(), hwnd, taskbar, icons, added: false, last: None };
        // The button of the window may exist already (or not yet: then the taskbar says so with a message).
        add_buttons(&mut state);
        *STATE.lock().unwrap() = Some(state);
        // SAFETY: a subclass of this app's own window, which lives as long as the app.
        let created = unsafe { RegisterWindowMessageW(PCWSTR(w!("TaskbarButtonCreated").0)) } as usize;
        // SAFETY: the procedure and the message it waits for (as the reference data) are valid for the window's life.
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass), 0x534F_554E, created) }.as_bool() {
            log::warn!("Taskbar: the window could not be listened to");
        }
    }

    /// What plays now (from the page): the buttons follow it. Done on the main thread, where the taskbar's object lives.
    pub fn update(app: &AppHandle, update: &MediaUpdate) {
        let update = update.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(state) = STATE.lock().unwrap().as_mut() {
                state.last = Some(update);
                refresh(state);
            }
        });
    }
}
