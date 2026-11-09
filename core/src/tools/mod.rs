//! The external tools Soundify runs: yt-dlp, ffmpeg (with ffprobe) and Deno,
//! the JS runtime yt-dlp needs for YouTube. Where they live, how they are
//! installed and kept up to date, and the notice when one is missing.
//!
//! On Windows the app keeps its own copies in `data/systemtools/` (the Java
//! `ToolManager`); elsewhere the system's tools on the PATH are used. Android
//! brings its own, inside the APK (youtubedl-android: Python running yt-dlp,
//! ffmpeg and QuickJS as libraries), and says how to start them with a
//! [`Launcher`] once the Kotlin side has unpacked them.

pub mod health;
pub mod progress;
pub mod status;
pub mod updaters;
pub mod zip_extract;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Where each tool is. The same layout as the Java app, so a `data/` folder
/// moved from one to the other keeps its tools.
#[derive(Debug, Clone)]
pub struct ToolPaths {
    root: PathBuf,
    managed: bool,
    /// Android: how to start yt-dlp. The slot is shared by every clone and is
    /// filled once the runtime is unpacked, so the paths handed out before
    /// that see it too.
    launcher: Option<Arc<OnceLock<Launcher>>>,
}

/// How yt-dlp is started where it is not an executable of its own. On Android
/// it is `libpython.so yt-dlp ...`, with the libraries and certificates found
/// through the environment, plus `--ffmpeg-location` and `--js-runtimes`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Launcher {
    pub program: PathBuf,
    /// Before yt-dlp's own arguments (the script, the paths of ffmpeg and the JS runtime).
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// `ffmpeg -version` and the like: the program and its fixed arguments.
    pub ffmpeg: Vec<String>,
}

impl ToolPaths {
    /// `managed`: the app installs and updates its own copies under `systemtools`.
    pub fn new(systemtools: impl Into<PathBuf>, managed: bool) -> Self {
        Self { root: systemtools.into(), managed, launcher: None }
    }

    /// Tools that travel inside the app and start through a [`Launcher`] that
    /// is not known yet: `slot` is filled when the runtime is ready.
    pub fn with_launcher(systemtools: impl Into<PathBuf>, slot: Arc<OnceLock<Launcher>>) -> Self {
        Self { root: systemtools.into(), managed: false, launcher: Some(slot) }
    }

    /// The launcher, if tools start through one and it is ready.
    pub fn launcher(&self) -> Option<&Launcher> {
        self.launcher.as_ref().and_then(|slot| slot.get())
    }

    /// Says how to start the tools (the runtime is unpacked). True the first
    /// time; a second call changes nothing.
    pub fn set_launcher(&self, launcher: Launcher) -> bool {
        self.launcher.as_ref().is_some_and(|slot| slot.set(launcher).is_ok())
    }

    /// Tools start through a launcher (ready or not).
    pub fn uses_launcher(&self) -> bool {
        self.launcher.is_some()
    }

    /// Managed on Windows, the system's tools anywhere else (as in Java).
    pub fn for_this_platform(systemtools: impl Into<PathBuf>) -> Self {
        Self::new(systemtools, cfg!(windows))
    }

    pub fn is_managed(&self) -> bool {
        self.managed
    }

    pub fn ytdlp_dir(&self) -> PathBuf {
        self.root.join("yt-dlp")
    }

    pub fn ffmpeg_dir(&self) -> PathBuf {
        self.root.join("ffmpeg")
    }

    pub fn ffmpeg_bin(&self) -> PathBuf {
        self.ffmpeg_dir().join("bin")
    }

    pub fn deno_dir(&self) -> PathBuf {
        self.root.join("deno")
    }

    pub fn ytdlp(&self) -> PathBuf {
        self.tool(self.ytdlp_dir(), "yt-dlp")
    }

    pub fn ffmpeg(&self) -> PathBuf {
        self.tool(self.ffmpeg_bin(), "ffmpeg")
    }

    pub fn ffprobe(&self) -> PathBuf {
        self.tool(self.ffmpeg_bin(), "ffprobe")
    }

    pub fn deno(&self) -> PathBuf {
        self.tool(self.deno_dir(), "deno")
    }

    /// The folders startup creates (the Java `Bootstrap.initFolders`).
    pub fn folders(&self) -> [PathBuf; 3] {
        [self.ytdlp_dir(), self.ffmpeg_bin(), self.deno_dir()]
    }

    /// The managed copy, or the bare name for the PATH to resolve.
    fn tool(&self, dir: PathBuf, name: &str) -> PathBuf {
        if self.managed { dir.join(format!("{name}.exe")) } else { PathBuf::from(name) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_tools_live_in_the_same_folders_as_in_java() {
        let tools = ToolPaths::new("data/systemtools", true);
        let root = PathBuf::from("data/systemtools");
        assert_eq!(tools.ytdlp(), root.join("yt-dlp").join("yt-dlp.exe"));
        assert_eq!(tools.ffmpeg(), root.join("ffmpeg").join("bin").join("ffmpeg.exe"));
        assert_eq!(tools.ffprobe(), root.join("ffmpeg").join("bin").join("ffprobe.exe"));
        assert_eq!(tools.deno(), root.join("deno").join("deno.exe"));
    }

    #[test]
    fn the_launcher_shows_up_in_every_clone_once_it_is_ready() {
        let slot = Arc::new(OnceLock::new());
        let early = ToolPaths::with_launcher("data/systemtools", slot.clone());
        let later = early.clone();
        assert!(early.uses_launcher());
        assert!(early.launcher().is_none());
        slot.set(Launcher { program: "python".into(), ..Launcher::default() }).unwrap();
        assert_eq!(later.launcher().map(|l| l.program.clone()), Some(PathBuf::from("python")));
        assert!(early.launcher().is_some());
        assert!(!ToolPaths::new("x", false).uses_launcher());
    }

    #[test]
    fn unmanaged_tools_are_left_to_the_path() {
        let tools = ToolPaths::new("data/systemtools", false);
        assert_eq!(tools.ytdlp(), PathBuf::from("yt-dlp"));
        assert_eq!(tools.ffmpeg(), PathBuf::from("ffmpeg"));
        assert_eq!(tools.deno(), PathBuf::from("deno"));
    }
}
