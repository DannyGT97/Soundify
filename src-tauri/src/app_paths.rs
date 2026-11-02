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
    pub data: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        let data = std::env::var_os("SOUNDIFY_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("data")))
                .unwrap_or_else(|| PathBuf::from("data"))
        });
        Self { data }
    }

    pub fn archives(&self) -> Location {
        Location::path(self.data.join("history").join("archives"))
    }

    pub fn settings(&self) -> Location {
        Location::path(self.data.join("config").join("settings.json"))
    }

    pub fn task_history(&self) -> Location {
        Location::path(self.data.join("history").join("tasks.json"))
    }
}
