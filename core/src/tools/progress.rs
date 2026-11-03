//! How far the tools being downloaded have got, for the notice the user sees
//! meanwhile. Without it the first start of a computer with no ffmpeg looked
//! stuck: ~115 MB from a slow server and nothing on screen but "ffmpeg is
//! missing" when trying to download.
//!
//! Same shape as [`super::health::ToolHealth`]: the updaters write, one
//! listener is told, and one that arrives late still hears the current state.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

/// The tools the app downloads by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tool {
    Ffmpeg,
    Deno,
    Ytdlp,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Ytdlp, Tool::Ffmpeg, Tool::Deno];

    /// How the front names it ("ytdlp", "ffmpeg", "deno").
    pub fn id(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
            Tool::Ytdlp => "ytdlp",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|tool| tool.id() == id)
    }

    /// As the notice names it.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "Deno",
            Tool::Ytdlp => "yt-dlp",
        }
    }
}

/// One tool on its way. `percent` is `None` when the server does not say the size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolDownload {
    pub id: &'static str,
    pub tool: &'static str,
    pub percent: Option<u8>,
}

/// Receives every tool still downloading, each time that changes. Empty = all done.
pub type Listener = Box<dyn Fn(Vec<ToolDownload>) + Send + Sync>;

#[derive(Default)]
pub struct InstallProgress {
    running: Mutex<BTreeMap<Tool, Option<u8>>>,
    listener: Mutex<Option<Listener>>,
}

// === PUBLIC API ===

impl InstallProgress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self, tool: Tool) {
        self.change(|running| running.insert(tool, None) != Some(None));
    }

    /// `fraction` between 0 and 1. Only a new whole percent is passed on: the
    /// download reports every 8 KB.
    pub fn advance(&self, tool: Tool, fraction: f64) {
        let percent = (fraction.clamp(0.0, 1.0) * 100.0).floor() as u8;
        self.change(|running| running.insert(tool, Some(percent)) != Some(Some(percent)));
    }

    /// Done, well or badly: a failure is told by `ToolHealth`, not here.
    pub fn finish(&self, tool: Tool) {
        self.change(|running| running.remove(&tool).is_some());
    }

    pub fn snapshot(&self) -> Vec<ToolDownload> {
        to_list(&self.running.lock().unwrap())
    }

    /// Hears the current state at once if something is already downloading.
    pub fn on_change(&self, listener: Listener) {
        let now = self.snapshot();
        if !now.is_empty() {
            listener(now);
        }
        *self.listener.lock().unwrap() = Some(listener);
    }

    // === PRIVATE HELPERS ===

    fn change(&self, apply: impl FnOnce(&mut BTreeMap<Tool, Option<u8>>) -> bool) {
        let list = {
            let mut running = self.running.lock().unwrap();
            if !apply(&mut running) {
                return;
            }
            to_list(&running)
        };
        if let Some(listener) = self.listener.lock().unwrap().as_ref() {
            listener(list);
        }
    }
}

fn to_list(running: &BTreeMap<Tool, Option<u8>>) -> Vec<ToolDownload> {
    running.iter().map(|(tool, percent)| ToolDownload { id: tool.id(), tool: tool.name(), percent: *percent }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn recorder(progress: &InstallProgress) -> Arc<Mutex<Vec<Vec<ToolDownload>>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        progress.on_change(Box::new(move |list| sink.lock().unwrap().push(list)));
        seen
    }

    fn download(tool: &'static str, percent: Option<u8>) -> ToolDownload {
        let id = Tool::ALL.into_iter().find(|t| t.name() == tool).unwrap().id();
        ToolDownload { id, tool, percent }
    }

    #[test]
    fn ids_go_both_ways() {
        for tool in Tool::ALL {
            assert_eq!(Tool::from_id(tool.id()), Some(tool));
        }
        assert_eq!(Tool::from_id("ffprobe"), None);
    }

    #[test]
    fn tells_each_new_whole_percent_and_the_end() {
        let progress = InstallProgress::new();
        let seen = recorder(&progress);
        progress.start(Tool::Ffmpeg);
        progress.advance(Tool::Ffmpeg, 0.451);
        progress.advance(Tool::Ffmpeg, 0.459);
        progress.advance(Tool::Ffmpeg, 0.46);
        progress.finish(Tool::Ffmpeg);
        assert_eq!(*seen.lock().unwrap(), vec![
            vec![download("ffmpeg", None)],
            vec![download("ffmpeg", Some(45))],
            vec![download("ffmpeg", Some(46))],
            vec![],
        ]);
    }

    #[test]
    fn several_tools_at_once_are_told_together() {
        let progress = InstallProgress::new();
        progress.start(Tool::Deno);
        progress.start(Tool::Ffmpeg);
        progress.advance(Tool::Deno, 0.5);
        assert_eq!(progress.snapshot(), vec![download("ffmpeg", None), download("Deno", Some(50))]);
    }

    #[test]
    fn a_late_listener_still_hears_what_is_downloading() {
        let progress = InstallProgress::new();
        progress.start(Tool::Ffmpeg);
        progress.advance(Tool::Ffmpeg, 0.2);
        let seen = recorder(&progress);
        assert_eq!(*seen.lock().unwrap(), vec![vec![download("ffmpeg", Some(20))]]);
    }

    #[test]
    fn finishing_what_never_started_says_nothing() {
        let progress = InstallProgress::new();
        let seen = recorder(&progress);
        progress.finish(Tool::Deno);
        assert!(seen.lock().unwrap().is_empty());
    }
}
