//! Running yt-dlp, asked through a trait.
//!
//! On desktop it is a process (yt-dlp, with ffmpeg and deno beside it); on
//! Android it is youtubedl-android behind a Kotlin plugin. Both take the same
//! argument list and hand back the output line by line, which is all the rest
//! needs: the progress, the `--print` hooks and the errors are read from those
//! lines, the same as in the Java app.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One run of yt-dlp: the arguments, without the executable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YtDlpRequest {
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputLine {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    Finished { exit_code: i32 },
    /// Stopped from the app. On desktop the whole process tree goes: yt-dlp
    /// starts ffmpeg, and killing only yt-dlp leaves ffmpeg writing the file.
    Stopped,
}

/// Asks a running download to stop. Cloned to whoever may press «Detener».
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub trait Downloader: Send + Sync {
    /// Runs yt-dlp and blocks until it ends or `stop` is raised, calling
    /// `on_line` for every line of output as it comes.
    fn run(
        &self,
        request: &YtDlpRequest,
        stop: &StopFlag,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> io::Result<RunEnd>;
}

/// A downloader that replays recorded output, for tests: the engine is fed
/// real yt-dlp lines without running anything.
#[derive(Debug, Clone, Default)]
pub struct ScriptedDownloader {
    pub lines: Vec<OutputLine>,
    pub exit_code: i32,
}

impl ScriptedDownloader {
    pub fn new(lines: Vec<OutputLine>, exit_code: i32) -> Self {
        Self { lines, exit_code }
    }
}

impl Downloader for ScriptedDownloader {
    fn run(
        &self,
        _request: &YtDlpRequest,
        stop: &StopFlag,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> io::Result<RunEnd> {
        for line in &self.lines {
            if stop.is_stopped() {
                return Ok(RunEnd::Stopped);
            }
            on_line(line.clone());
        }
        Ok(RunEnd::Finished { exit_code: self.exit_code })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(text: &str) -> OutputLine {
        OutputLine::Stdout(text.to_string())
    }

    #[test]
    fn scripted_downloader_replays_every_line_in_order() {
        let downloader = ScriptedDownloader::new(vec![out("[download] 50%"), out("[download] 100%")], 0);
        let mut seen = Vec::new();
        let end = downloader
            .run(&YtDlpRequest::default(), &StopFlag::new(), &mut |line| seen.push(line))
            .unwrap();
        assert_eq!(seen, [out("[download] 50%"), out("[download] 100%")]);
        assert_eq!(end, RunEnd::Finished { exit_code: 0 });
    }

    #[test]
    fn stopping_ends_the_run_before_the_next_line() {
        let downloader = ScriptedDownloader::new(vec![out("one"), out("two"), out("three")], 0);
        let stop = StopFlag::new();
        let mut seen = Vec::new();
        let end = downloader
            .run(&YtDlpRequest::default(), &stop.clone(), &mut |line| {
                seen.push(line);
                stop.stop();
            })
            .unwrap();
        assert_eq!(seen, [out("one")]);
        assert_eq!(end, RunEnd::Stopped);
    }
}
