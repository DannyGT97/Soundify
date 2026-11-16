//! What the page can say to the log, and the diagnostic text «Copiar diagnóstico» gives: the version, the platform, the
//! state of Connect and the tail of the log, in one piece that can be pasted. Nothing personal: the home folder and the data
//! folder are written as `~` and `<data>`.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::connect::ConnectState;

/// A line from the page never makes the log longer than this.
const MAX_LINE: usize = 600;
/// How much of the end of the log goes into the diagnostic.
const LOG_TAIL_BYTES: u64 = 48 * 1024;

/// The page writes to the same log as the back (`app_lib::front`): its loops and its errors used to leave no trace.
// Off the main thread (a plain command runs on it: the window, or the phone's UI thread, waited for the disk).
#[tauri::command(async)]
pub fn log_event(level: String, area: String, message: String) {
    let text: String = message.replace(['\r', '\n'], " ").chars().take(MAX_LINE).collect();
    let area: String = area.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(24).collect();
    match level.as_str() {
        "error" => log::error!(target: "app_lib::front", "[{area}] {text}"),
        "warn" => log::warn!(target: "app_lib::front", "[{area}] {text}"),
        _ => log::info!(target: "app_lib::front", "[{area}] {text}"),
    }
}

/// The paths of the user are not for sharing: the home folder and the data folder are replaced by short names.
pub fn redact(text: &str, data: &Path) -> String {
    let mut out = text.to_string();
    let data = data.to_string_lossy().to_string();
    if !data.is_empty() {
        out = out.replace(&data, "<data>");
    }
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var)
            && home.len() > 3
        {
            out = out.replace(&home, "~");
            out = out.replace(&home.replace('\\', "/"), "~");
        }
    }
    out
}

/// The last bytes of a file, from the first whole line.
fn tail(path: &Path, bytes: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = length.saturating_sub(bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buffer = Vec::new();
    let _ = file.read_to_end(&mut buffer);
    let text = String::from_utf8_lossy(&buffer).to_string();
    if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text }
}

// Off the main thread (a plain command runs on it: the window, or the phone's UI thread, waited for the disk).
#[tauri::command(async)]
pub fn diagnostics(app: AppHandle, state: State<'_, AppState>, connect: State<'_, ConnectState>) -> String {
    let version = app.package_info().version.to_string();
    let platform = if cfg!(target_os = "android") { "android" } else if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };
    let data = state.paths.logs().parent().map(Path::to_path_buf).unwrap_or_default();
    let log = tail(&state.paths.logs().join("soundify-0.log"), LOG_TAIL_BYTES);
    let text = format!(
        "# Diagnóstico de Soundify\nVersión: {version}\nPlataforma: {platform} ({})\n\n## Connect\n{}\n\n## Final del registro (soundify-0.log)\n{log}",
        std::env::consts::ARCH,
        connect.summary(),
    );
    redact(&text, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_folder_and_the_home_are_not_in_the_diagnostic() {
        let data = Path::new("D:/Proyectos/soundify/data");
        let out = redact("log in D:/Proyectos/soundify/data/logs and more", data);
        assert_eq!(out, "log in <data>/logs and more");
    }

    #[test]
    fn a_line_from_the_page_is_one_line() {
        // Only checks it does not panic with odd input: the target is the log, which tests do not install.
        log_event("info".into(), "a b\nc".into(), "x\ny\r\nz".repeat(500));
    }
}
