//! The external tools Soundify runs: yt-dlp, ffmpeg (with ffprobe) and Deno,
//! the JS runtime yt-dlp needs for YouTube. Where they live, how they are
//! installed and kept up to date, and the notice when one is missing.
//!
//! Desktop only. On Windows the app keeps its own copies in
//! `data/systemtools/` (the Java `ToolManager`); elsewhere the system's tools
//! on the PATH are used. Android brings its own through youtubedl-android.

pub mod health;
pub mod progress;
pub mod status;
pub mod updaters;
pub mod zip_extract;

use std::path::PathBuf;

/// Where each tool is. The same layout as the Java app, so a `data/` folder
/// moved from one to the other keeps its tools.
#[derive(Debug, Clone)]
pub struct ToolPaths {
    root: PathBuf,
    managed: bool,
}

impl ToolPaths {
    /// `managed`: the app installs and updates its own copies under `systemtools`.
    pub fn new(systemtools: impl Into<PathBuf>, managed: bool) -> Self {
        Self { root: systemtools.into(), managed }
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
    fn unmanaged_tools_are_left_to_the_path() {
        let tools = ToolPaths::new("data/systemtools", false);
        assert_eq!(tools.ytdlp(), PathBuf::from("yt-dlp"));
        assert_eq!(tools.ffmpeg(), PathBuf::from("ffmpeg"));
        assert_eq!(tools.deno(), PathBuf::from("deno"));
    }
}
