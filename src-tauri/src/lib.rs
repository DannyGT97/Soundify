// Views only for now: no commands yet. The download pipeline (yt-dlp, ffmpeg,
// deno as sidecars) and the archive logic will be registered here as commands.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
