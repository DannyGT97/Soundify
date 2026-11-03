//! Writing the log files, as the Java `LogManager` did: the same three files in
//! `data/logs/`, rotated the same way (10 MB each, at most 10 of each kind).
//!
//! - `soundify-N.log`: the app's own diagnostics (every `log::` call).
//! - `soundify-activity-N.log`: yt-dlp's raw output of the normal downloads.
//!   Its own file so thousands of progress lines do not bury the app's events.
//! - `soundify-retries-N.log`: the same, for the automatic second pass. Its own
//!   file so a playlist that keeps failing can be diagnosed AFTER it happened.
//!
//! Generation 0 is always the one being written; when it fills up, every file
//! moves one number up and the oldest falls off (Java's `FileHandler` with `%g`).
//! The text of each line is [`crate::log_format`]'s.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::clock::Clock;
use crate::log_format;

/// The target of yt-dlp's raw output in a normal download: `log::info!(target: ACTIVITY, ...)`.
pub const ACTIVITY: &str = "soundify.activity";
/// The target of yt-dlp's raw output in the second pass.
pub const RETRIES: &str = "soundify.retries";

pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILES: usize = 10;

/// This app's own crates: everything they say from `info` up is written.
/// Other crates (the webview, HTTP...) only from `warn` up, or their chatter
/// would bury the app's events.
const OWN_CRATES: [&str; 2] = ["soundify_core", "app_lib"];

// === PUBLIC API ===

/// One kind of log file, rotated by size.
pub struct RotatingFile {
    folder: PathBuf,
    base: String,
    max_bytes: u64,
    max_files: usize,
    file: Option<File>,
    written: u64,
}

impl RotatingFile {
    /// `base` is the name before the generation: `soundify` gives `soundify-0.log`.
    pub fn new(folder: &Path, base: &str, max_bytes: u64, max_files: usize) -> Self {
        Self { folder: folder.to_path_buf(), base: base.to_string(), max_bytes, max_files: max_files.max(1), file: None, written: 0 }
    }

    /// The file of generation `n`.
    pub fn path(&self, n: usize) -> PathBuf {
        self.folder.join(format!("{}-{n}.log", self.base))
    }

    /// Appends `text`; moves to a fresh generation 0 first if it would not fit.
    /// A single entry bigger than the limit is still written whole.
    pub fn write(&mut self, text: &str) -> io::Result<()> {
        if self.file.is_none() {
            self.open()?;
        }
        if self.written > 0 && self.written + text.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let file = self.file.as_mut().expect("opened above");
        file.write_all(text.as_bytes())?;
        self.written += text.len() as u64;
        Ok(())
    }

    fn open(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        // Appending, like Java's: a restart goes on in the same file.
        let file = OpenOptions::new().create(true).append(true).open(self.path(0))?;
        self.written = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Closed before renaming: Windows does not rename a file that is open.
        self.file = None;
        let oldest = self.path(self.max_files - 1);
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for n in (0..self.max_files - 1).rev() {
            let from = self.path(n);
            if from.exists() {
                fs::rename(&from, self.path(n + 1))?;
            }
        }
        self.open()
    }
}

/// Java's level names, so old and new logs read the same.
pub fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "SEVERE",
        Level::Warn => "WARNING",
        Level::Info => "INFO",
        Level::Debug => "FINE",
        Level::Trace => "FINEST",
    }
}

/// Whether a record is written at all (see [`OWN_CRATES`]).
pub fn wanted(target: &str, level: Level) -> bool {
    if target == ACTIVITY || target == RETRIES {
        return true;
    }
    let crate_name = target.split("::").next().unwrap_or(target);
    let floor = if OWN_CRATES.contains(&crate_name) { Level::Info } else { Level::Warn };
    level <= floor
}

/// The three files behind the `log` facade.
pub struct FileLogger {
    clock: Clock,
    app: Mutex<RotatingFile>,
    activity: Mutex<RotatingFile>,
    retries: Mutex<RotatingFile>,
}

impl FileLogger {
    pub fn new(folder: &Path, clock: Clock) -> Self {
        let file = |base| Mutex::new(RotatingFile::new(folder, base, MAX_BYTES, MAX_FILES));
        Self { clock, app: file("soundify"), activity: file("soundify-activity"), retries: file("soundify-retries") }
    }

    /// Makes it the app's logger. Once: a second call (or another logger
    /// already in place) is refused and changes nothing.
    pub fn install(self) -> Result<(), log::SetLoggerError> {
        log::set_boxed_logger(Box::new(self))?;
        log::set_max_level(LevelFilter::Info);
        Ok(())
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let at = (self.clock)();
        let message = record.args().to_string();
        let (file, text) = match record.target() {
            ACTIVITY => (&self.activity, log_format::activity_line(at, &message)),
            RETRIES => (&self.retries, log_format::activity_line(at, &message)),
            target => (&self.app, log_format::app_entry(at, target, level_name(record.level()), &message)),
        };
        // Nowhere left to tell a failure to write the log: it is dropped.
        let _ = file.lock().map(|mut f| f.write(&text));
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn writes_into_generation_zero_and_appends_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        RotatingFile::new(dir.path(), "soundify", 1000, 3).write("one\n").unwrap();
        RotatingFile::new(dir.path(), "soundify", 1000, 3).write("two\n").unwrap();
        assert_eq!(read(&dir.path().join("soundify-0.log")), "one\ntwo\n");
    }

    #[test]
    fn a_full_file_moves_one_number_up_and_the_new_text_starts_generation_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RotatingFile::new(dir.path(), "soundify", 10, 3);
        file.write("aaaaaaaa\n").unwrap();
        file.write("bbbbbbbb\n").unwrap();
        assert_eq!(read(&file.path(0)), "bbbbbbbb\n");
        assert_eq!(read(&file.path(1)), "aaaaaaaa\n");
    }

    #[test]
    fn the_oldest_falls_off_past_the_maximum() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RotatingFile::new(dir.path(), "soundify", 4, 3);
        for text in ["111\n", "222\n", "333\n", "444\n"] {
            file.write(text).unwrap();
        }
        assert_eq!(read(&file.path(0)), "444\n");
        assert_eq!(read(&file.path(1)), "333\n");
        assert_eq!(read(&file.path(2)), "222\n");
        assert!(!file.path(3).exists(), "never more than max_files");
    }

    #[test]
    fn an_entry_bigger_than_the_limit_is_still_written_whole() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RotatingFile::new(dir.path(), "soundify", 4, 3);
        file.write("a long entry\n").unwrap();
        assert_eq!(read(&file.path(0)), "a long entry\n");
    }

    #[test]
    fn the_app_writes_from_info_and_other_crates_only_from_warn() {
        assert!(wanted("soundify_core::engine::download", Level::Info));
        assert!(wanted("app_lib::bootstrap", Level::Info));
        assert!(!wanted("soundify_core::engine", Level::Debug));
        assert!(!wanted("reqwest::connect", Level::Info));
        assert!(wanted("reqwest::connect", Level::Warn));
        assert!(wanted(ACTIVITY, Level::Info));
        assert!(wanted(RETRIES, Level::Info));
    }

    #[test]
    fn levels_keep_the_java_names() {
        assert_eq!(level_name(Level::Error), "SEVERE");
        assert_eq!(level_name(Level::Warn), "WARNING");
        assert_eq!(level_name(Level::Info), "INFO");
    }

    #[test]
    fn each_target_goes_to_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let logger = FileLogger::new(dir.path(), crate::clock::fixed("2026-11-03 20:00"));
        let log = |target: &str, message: &str| logger.log(&Record::builder()
            .target(target).level(Level::Info).args(format_args!("{message}")).build());
        log("soundify_core::engine", "started");
        log(ACTIVITY, "[download] 10%");
        log(RETRIES, "[download] retry");
        assert!(read(&dir.path().join("soundify-0.log")).contains("INFO: started"));
        assert!(read(&dir.path().join("soundify-activity-0.log")).starts_with("03/11/2026 20:00:00 [download] 10%"));
        assert!(read(&dir.path().join("soundify-retries-0.log")).trim_end().ends_with("[download] retry"));
        assert!(!read(&dir.path().join("soundify-0.log")).contains("[download]"));
    }
}
