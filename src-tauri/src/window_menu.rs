//! The window's system menu (restore, move, size, minimize, maximize, close).
//! Windows opens it from its own title bar; ours is drawn by the page, so a
//! right click on it comes here. Not `async`: it must run on the main thread,
//! the one that owns the window.

#[cfg(windows)]
#[tauri::command]
pub fn show_system_menu(window: tauri::Window) {
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnableMenuItem, GetCursorPos, GetSystemMenu, IsZoomed, PostMessageW, SetForegroundWindow, TrackPopupMenu,
        MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, SC_MAXIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        WM_SYSCOMMAND,
    };

    let Ok(handle) = window.hwnd() else { return };
    let hwnd = HWND(handle.0);
    // SAFETY: `hwnd` is this window's, alive for the whole call (we are on its
    // thread, so it cannot close meanwhile); the menu belongs to it.
    unsafe {
        let menu = GetSystemMenu(hwnd, false);
        if menu.is_invalid() {
            return;
        }
        // Windows keeps these up to date only when it opens the menu itself.
        let maximized = IsZoomed(hwnd).as_bool();
        for (item, on) in
            [(SC_RESTORE, maximized), (SC_MAXIMIZE, !maximized), (SC_MOVE, !maximized), (SC_SIZE, !maximized)]
        {
            let state = if on { MF_ENABLED } else { MF_GRAYED };
            let _ = EnableMenuItem(menu, item, MF_BYCOMMAND | state);
        }
        let mut at = POINT::default();
        if GetCursorPos(&mut at).is_err() {
            return;
        }
        // Without this the menu does not close when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, None, hwnd, None);
        if chosen.0 != 0 {
            let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(chosen.0 as usize), LPARAM(0));
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn show_system_menu() {}
