//! Where the app keeps its `data/`: the only place that decides it, handed to
//! `core` as plain locations (the Java app's `AppPaths`).
//!
//! Portable on desktop, as in Java: `data/` sits next to the executable, so a
//! new portable starts empty and the music is recovered by importing it.
//! `SOUNDIFY_DATA_DIR` points it elsewhere (in development, at the Java app's
//! own `data/` to see the real library).

use std::path::PathBuf;

use soundify_core::Location;

pub struct AppPaths {
    /// The executable's folder: what an update replaces (`data/` aside).
    pub app_dir: PathBuf,
    pub data: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        let app_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let data = std::env::var_os("SOUNDIFY_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| app_dir.join("data"));
        Self { app_dir, data }
    }

    /// The downloaded updates. Next to `data/`, not inside it, as in Java:
    /// an update never touches either of them.
    pub fn updates(&self) -> PathBuf {
        self.app_dir.join("updates")
    }

    /// Where the new version is unpacked before it takes over.
    pub fn staged_update(&self) -> PathBuf {
        self.updates().join("staged")
    }

    pub fn archives(&self) -> Location {
        Location::path(self.data.join("history").join("archives"))
    }

    pub fn settings(&self) -> Location {
        Location::path(self.data.join("config").join("settings.json"))
    }

    /// yt-dlp, ffmpeg and Deno (managed copies, Windows only).
    pub fn systemtools(&self) -> std::path::PathBuf {
        self.data.join("systemtools")
    }

    /// Video thumbnails for the covers: disposable, fetched again when missing.
    pub fn thumbnails(&self) -> std::path::PathBuf {
        self.data.join("cache").join("thumbnails")
    }

    /// `soundify-N.log` and its activity and retries siblings (see `core::logs`).
    pub fn logs(&self) -> std::path::PathBuf {
        self.data.join("logs")
    }

    /// Where a device playlist's files wait before being pushed to the phone
    /// (one folder per archive, see `core::archive::device::staging_folder`).
    pub fn device_staging(&self) -> PathBuf {
        self.data.join("device-staging")
    }

    /// Short-lived folders for the manifests read from or written to a phone.
    pub fn device_scratch(&self) -> PathBuf {
        self.device_staging().join("tmp")
    }

    pub fn task_history(&self) -> Location {
        Location::path(self.data.join("history").join("tasks.json"))
    }
}
