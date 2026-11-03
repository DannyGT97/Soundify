//! Installing and updating the managed tools (the Java `YtDlpUpdater`,
//! `FfmpegUpdater` and `DenoUpdater`). Startup only seeds the bundled ffmpeg
//! and keeps an existing yt-dlp up to date; installing what is missing is the
//! user's call, from the first-start assistant or Ajustes → Herramientas.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use super::ToolPaths;
use super::health::{Problem, ToolHealth};
use super::progress::{InstallProgress, Tool};
use super::zip_extract::extract_matching;
use crate::downloader::{Downloader, OutputLine, RunEnd, StopFlag, YtDlpRequest};
use crate::http::Http;

pub const YTDLP_LATEST_RELEASE: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
pub const YTDLP_DOWNLOAD: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
pub const FFMPEG_ZIP: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
pub const DENO_ZIP: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

/// yt-dlp itself starts warning at 90 days.
const MAX_AGE_DAYS: i64 = 90;

// The yt-dlp version is a date: "2026.03.17".
static VERSION_DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{4})\.(\d{2})\.(\d{2})").unwrap());

// === YT-DLP ===

/// What the yt-dlp updater is going to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtDlpPlan {
    UpToDate,
    Update,
    /// The remote version could not be asked for, but the local one is recent.
    KeepRecent,
    /// The remote version could not be asked for and the local one is old.
    ///
    /// This used to be given up on and whatever binary was there was kept — a
    /// real case: a five-month-old one, with which YouTube no longer worked.
    /// But the download URL points straight at "latest": downloading does NOT
    /// need the API. So if the local one is old, the update is attempted
    /// blindly anyway.
    UpdateBlind,
}

pub fn plan_ytdlp(local: Option<&str>, remote: Option<&str>, today: NaiveDate) -> YtDlpPlan {
    match remote {
        Some(remote) if local.is_some_and(|local| normalize(local) == normalize(remote)) => YtDlpPlan::UpToDate,
        Some(_) => YtDlpPlan::Update,
        None if is_stale(local, today) => YtDlpPlan::UpdateBlind,
        None => YtDlpPlan::KeepRecent,
    }
}

/// Has the local binary gone too long without an update? yt-dlp itself warns
/// at 90 days ("Your yt-dlp version is older than 90 days"), and with YouTube
/// changing every other week, an old one stops working.
///
/// The yt-dlp version IS a date ("2026.03.17"), so its age is worked out
/// without asking anyone. Whatever does not look like a date (no version, an
/// odd build) counts as old: better to try to update for nothing than to keep
/// a broken one.
pub fn is_stale(version: Option<&str>, today: NaiveDate) -> bool {
    let Some(caps) = version.and_then(|v| VERSION_DATE.captures(v.trim())) else { return true };
    let part = |i: usize| caps[i].parse::<u32>().ok();
    let released = match (caps[1].parse::<i32>().ok(), part(2), part(3)) {
        (Some(y), Some(m), Some(d)) => NaiveDate::from_ymd_opt(y, m, d),
        _ => None,
    };
    match released {
        Some(released) => released < today - chrono::Duration::days(MAX_AGE_DAYS),
        None => true,
    }
}

/// The `tag_name` of the GitHub "latest release" answer.
pub fn remote_version(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = normalize(json.get("tag_name")?.as_str()?);
    (!tag.is_empty()).then_some(tag)
}

/// What `yt-dlp --version` says, or `None` if it does not run.
pub fn local_ytdlp_version(downloader: &dyn Downloader) -> Option<String> {
    let request = YtDlpRequest { args: vec!["--version".into()] };
    let mut first = None;
    let end = downloader
        .run(&request, &StopFlag::new(), &mut |line| {
            if let OutputLine::Stdout(text) = line
                && first.is_none()
                && !text.trim().is_empty()
            {
                first = Some(text.trim().to_string());
            }
        })
        .ok()?;
    matches!(end, RunEnd::Finished { exit_code: 0 }).then_some(first).flatten()
}

/// Brings the managed yt-dlp up to date. `local` is what `yt-dlp --version`
/// said before starting (`None` if there is none).
pub fn update_ytdlp(
    tools: &ToolPaths,
    http: &dyn Http,
    local: Option<&str>,
    today: NaiveDate,
    health: &ToolHealth,
    progress: &InstallProgress,
) {
    let remote = latest_ytdlp(http);

    let plan = plan_ytdlp(local, remote.as_deref(), today);
    if remote.is_none() {
        health.report(Problem::UpdateCheckFailed, "could not query the latest yt-dlp version");
    }
    let result = match plan {
        YtDlpPlan::UpToDate => {
            log::info!("yt-dlp is already up to date ({})", local.unwrap_or_default());
            return;
        }
        YtDlpPlan::KeepRecent => {
            log::info!("Could not check the remote version, but the local yt-dlp is recent ({local:?}): keeping it.");
            return;
        }
        YtDlpPlan::Update => {
            log::info!("Updating yt-dlp: {local:?} → {remote:?}");
            download_ytdlp(tools, http, progress)
        }
        YtDlpPlan::UpdateBlind => {
            log::warn!("Could not check the remote version and the local yt-dlp ({local:?}) is outdated — downloading the latest one directly.");
            download_ytdlp(tools, http, progress)
        }
    };

    match result {
        Ok(()) => {
            log::info!("yt-dlp updated");
            health.resolve(Problem::YtdlpOutdated);
        }
        Err(e) => {
            log::error!("yt-dlp updater failed: {e}");
            if is_stale(local, today) {
                health.report(
                    Problem::YtdlpOutdated,
                    &format!("local yt-dlp ({local:?}) is outdated and could not be updated: {e}"),
                );
            }
        }
    }
}

/// Downloads the latest yt-dlp, whatever is there: the first install, the
/// «Actualizar» of Ajustes and «Reinstalar» are all this.
pub fn install_ytdlp(tools: &ToolPaths, http: &dyn Http, health: &ToolHealth, progress: &InstallProgress) -> io::Result<()> {
    let result = download_ytdlp(tools, http, progress);
    match &result {
        Ok(()) => {
            log::info!("yt-dlp installed");
            health.resolve(Problem::YtdlpOutdated);
        }
        Err(e) => log::error!("Could not install yt-dlp: {e}"),
    }
    result
}

/// The latest yt-dlp release, if GitHub answers.
pub fn latest_ytdlp(http: &dyn Http) -> Option<String> {
    http.get_string(YTDLP_LATEST_RELEASE, &[("Accept", "application/vnd.github+json")])
        .inspect_err(|e| log::warn!("Could not query the latest yt-dlp version: {e}"))
        .ok()
        .and_then(|body| remote_version(&body))
}

/// Whether `latest` is another version than the local one («Nueva: …»).
pub fn is_newer_ytdlp(local: Option<&str>, latest: &str) -> bool {
    local.is_none_or(|local| normalize(local) != normalize(latest))
}

fn download_ytdlp(tools: &ToolPaths, http: &dyn Http, progress: &InstallProgress) -> io::Result<()> {
    fs::create_dir_all(tools.ytdlp_dir())?;
    let temp = tools.ytdlp_dir().join("yt-dlp.tmp.exe");
    fetch(http, YTDLP_DOWNLOAD, &temp, Tool::Ytdlp, progress)?;
    safe_replace(&tools.ytdlp(), &temp)
}

/// Puts `new_file` where `target` was, keeping the old one as `.old`. A running
/// .exe cannot be deleted on Windows, but it can be renamed.
fn safe_replace(target: &Path, new_file: &Path) -> io::Result<()> {
    let backup = with_suffix(target, ".old");
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    if target.exists() {
        fs::rename(target, &backup)?;
    }
    fs::rename(new_file, target)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn normalize(version: &str) -> String {
    version.replace('v', "").trim().to_string()
}

// === FFMPEG ===

/// Puts the ffmpeg and ffprobe that travel with the app in place if they are
/// missing (the Java `ToolManager.extractAllTools`, which took them out of the
/// JAR). Downloading them instead meant ~115 MB from a slow server on the first
/// start, and no download could convert anything until that ended.
///
/// `bundled` is the folder holding the two. Returns whether they were copied;
/// a bundle without them (a build made without the binaries) is not an error,
/// [`install_ffmpeg`] downloads them then.
pub fn seed_ffmpeg(tools: &ToolPaths, bundled: &Path) -> io::Result<bool> {
    let sources = [bundled.join("ffmpeg.exe"), bundled.join("ffprobe.exe")];
    if ffmpeg_installed(tools) || !sources.iter().all(|s| s.exists()) {
        return Ok(false);
    }
    fs::create_dir_all(tools.ffmpeg_bin())?;
    for (source, target) in sources.iter().zip([tools.ffmpeg(), tools.ffprobe()]) {
        // Through a temporary name: a copy cut halfway must not look installed.
        let temp = with_suffix(&target, ".part");
        let copied = fs::copy(source, &temp).and_then(|_| fs::rename(&temp, &target));
        if copied.is_err() {
            remove_quietly(&temp);
        }
        copied?;
    }
    log::info!("ffmpeg copied from the app's bundle");
    Ok(true)
}

/// Installs ffmpeg and ffprobe if either is missing. They are not updated by
/// themselves: an older ffmpeg converts just as well, unlike yt-dlp.
pub fn install_ffmpeg(tools: &ToolPaths, http: &dyn Http, health: &ToolHealth, progress: &InstallProgress) -> io::Result<()> {
    if ffmpeg_installed(tools) {
        return Ok(());
    }
    log::info!("ffmpeg is missing: downloading it");
    reinstall_ffmpeg(tools, http, health, progress)
}

/// Downloads ffmpeg and ffprobe again, even if they are there («Reinstalar»:
/// in case they broke). The copies in place are only replaced once both new
/// ones are out of the zip, so a failed download leaves them working.
pub fn reinstall_ffmpeg(tools: &ToolPaths, http: &dyn Http, health: &ToolHealth, progress: &InstallProgress) -> io::Result<()> {
    let staging = tools.ffmpeg_dir().join("new");
    let result = (|| {
        let zip = tools.ffmpeg_dir().join("ffmpeg.zip");
        fetch(http, FFMPEG_ZIP, &zip, Tool::Ffmpeg, progress)?;
        // The layout of the zip changes from one version to the next (the
        // folder carries the version number): look for the ending only.
        let extracted = extract_matching(&zip, |name| ffmpeg_entry(name).map(|file| staging.join(file)));
        remove_quietly(&zip);
        extracted?;
        let (ffmpeg, ffprobe) = (staging.join("ffmpeg.exe"), staging.join("ffprobe.exe"));
        if !ffmpeg.exists() || !ffprobe.exists() {
            return Err(io::Error::other("the zip did not carry ffmpeg.exe and ffprobe.exe"));
        }
        fs::create_dir_all(tools.ffmpeg_bin())?;
        safe_replace(&tools.ffmpeg(), &ffmpeg)?;
        safe_replace(&tools.ffprobe(), &ffprobe)
    })();
    remove_dir_quietly(&staging);
    match &result {
        Ok(()) => {
            log::info!("ffmpeg installed");
            health.resolve(Problem::MediaToolMissing);
        }
        Err(e) => log::error!("Could not install ffmpeg: {e}"),
    }
    result
}

fn ffmpeg_installed(tools: &ToolPaths) -> bool {
    tools.ffmpeg().exists() && tools.ffprobe().exists()
}

fn ffmpeg_entry(entry: &str) -> Option<&'static str> {
    if entry.ends_with("bin/ffmpeg.exe") {
        Some("ffmpeg.exe")
    } else if entry.ends_with("bin/ffprobe.exe") {
        Some("ffprobe.exe")
    } else {
        None
    }
}

// === DENO ===

/// Installs Deno, the JS runtime yt-dlp uses to solve YouTube's n-challenge,
/// if it is missing.
///
/// Without it, the good audio formats are lost under SABR and yt-dlp falls back
/// to format 18, or fails outright with "Requested format is not available": a
/// user's 197 failures came from exactly this.
pub fn install_deno(tools: &ToolPaths, http: &dyn Http, health: &ToolHealth, progress: &InstallProgress) -> io::Result<()> {
    if tools.deno().exists() {
        return Ok(());
    }
    log::info!("Deno is missing: downloading it");
    reinstall_deno(tools, http, health, progress)
}

/// Downloads Deno again, even if it is there; the one in place stays until
/// the new one is out of the zip.
pub fn reinstall_deno(tools: &ToolPaths, http: &dyn Http, health: &ToolHealth, progress: &InstallProgress) -> io::Result<()> {
    let staging = tools.deno_dir().join("new");
    let result = (|| {
        let zip = tools.deno_dir().join("deno.zip");
        fetch(http, DENO_ZIP, &zip, Tool::Deno, progress)?;
        let fresh = staging.join("deno.exe");
        let extracted = extract_matching(&zip, |name| name.ends_with("deno.exe").then(|| fresh.clone()));
        remove_quietly(&zip);
        extracted?;
        if !fresh.exists() {
            return Err(io::Error::other("the zip did not carry deno.exe"));
        }
        safe_replace(&tools.deno(), &fresh)
    })();
    remove_dir_quietly(&staging);
    match &result {
        Ok(()) => {
            log::info!("Deno installed");
            health.resolve(Problem::JsRuntimeMissing);
        }
        Err(e) => log::error!("Could not install Deno: {e}"),
    }
    result
}

/// A download the user is told about while it lasts.
fn fetch(http: &dyn Http, url: &str, target: &Path, tool: Tool, progress: &InstallProgress) -> io::Result<()> {
    progress.start(tool);
    let result = http.to_file(url, target, &mut |fraction| progress.advance(tool, fraction));
    progress.finish(tool);
    result
}

fn remove_dir_quietly(path: &Path) {
    if let Err(e) = fs::remove_dir_all(path)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("Could not delete {}: {e}", path.display());
    }
}

fn remove_quietly(path: &Path) {
    if let Err(e) = fs::remove_file(path)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("Could not delete {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloader::ScriptedDownloader;
    use crate::tools::progress::ToolDownload;
    use crate::http::write_atomically;
    use std::collections::HashMap;
    use std::io::{Cursor, Write};
    use std::sync::Mutex;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()
    }

    fn days_ago(days: i64) -> String {
        (today() - chrono::Duration::days(days)).format("%Y.%m.%d").to_string()
    }

    /// Serves canned answers; a URL it does not know fails like a dead network.
    #[derive(Default)]
    struct FakeHttp {
        answers: HashMap<&'static str, Vec<u8>>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn with(mut self, url: &'static str, body: impl Into<Vec<u8>>) -> Self {
            self.answers.insert(url, body.into());
            self
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }

        fn answer(&self, url: &str) -> io::Result<&Vec<u8>> {
            self.asked.lock().unwrap().push(url.to_string());
            self.answers.get(url).ok_or_else(|| io::Error::other("offline"))
        }
    }

    impl Http for FakeHttp {
        fn get_string(&self, url: &str, _headers: &[(&str, &str)]) -> io::Result<String> {
            Ok(String::from_utf8(self.answer(url)?.clone()).unwrap())
        }

        fn to_file(&self, url: &str, target: &Path, on_progress: &mut dyn FnMut(f64)) -> io::Result<()> {
            let body = self.answer(url)?.clone();
            write_atomically(Cursor::new(body), target, None, on_progress)
        }
    }

    fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn release(tag: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","name":"yt-dlp {tag}"}}"#)
    }

    fn managed() -> (tempfile::TempDir, ToolPaths) {
        let dir = tempfile::tempdir().unwrap();
        let tools = ToolPaths::new(dir.path(), true);
        (dir, tools)
    }

    // --- is_stale (ported from YtDlpUpdaterTest) ---

    #[test]
    fn a_recent_version_is_not_stale() {
        for days in [0, 30, 89] {
            assert!(!is_stale(Some(&days_ago(days)), today()), "{days} days");
        }
    }

    #[test]
    fn regression_a_version_older_than_90_days_is_stale() {
        // A real case: a five-month-old yt-dlp with which YouTube no longer
        // worked, kept because the API could not be reached.
        assert!(is_stale(Some(&days_ago(91)), today()));
        assert!(is_stale(Some(&days_ago(150)), today()));
    }

    #[test]
    fn what_does_not_look_like_a_date_counts_as_stale() {
        for version in [None, Some(""), Some("   "), Some("desconocida"), Some("2026.13.99"), Some("1.2.3")] {
            assert!(is_stale(version, today()), "{version:?}");
        }
    }

    #[test]
    fn a_nightly_suffix_is_accepted() {
        let nightly = format!("{}.232310", days_ago(10));
        assert!(!is_stale(Some(&nightly), today()));
    }

    #[test]
    fn odd_versions_never_panic() {
        for version in ["....", "99999.99.99", "2026.02.30"] {
            assert!(is_stale(Some(version), today()), "{version}");
        }
    }

    // --- plan and remote version ---

    #[test]
    fn reads_the_tag_of_the_latest_release() {
        assert_eq!(remote_version(&release("2026.10.22")).as_deref(), Some("2026.10.22"));
        assert_eq!(remote_version(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(remote_version("<html>not json</html>"), None);
    }

    #[test]
    fn plans_what_the_java_updater_did() {
        let recent = days_ago(10);
        let old = days_ago(200);
        assert_eq!(plan_ytdlp(Some(&recent), Some(&recent), today()), YtDlpPlan::UpToDate);
        assert_eq!(plan_ytdlp(Some(&old), Some(&recent), today()), YtDlpPlan::Update);
        assert_eq!(plan_ytdlp(None, Some(&recent), today()), YtDlpPlan::Update);
        assert_eq!(plan_ytdlp(Some(&recent), None, today()), YtDlpPlan::KeepRecent);
        assert_eq!(plan_ytdlp(Some(&old), None, today()), YtDlpPlan::UpdateBlind);
        assert_eq!(plan_ytdlp(None, None, today()), YtDlpPlan::UpdateBlind);
    }

    #[test]
    fn the_local_version_is_the_first_line_yt_dlp_prints() {
        let ok = ScriptedDownloader::new(vec![OutputLine::Stdout("2026.10.22\r".into())], 0);
        assert_eq!(local_ytdlp_version(&ok).as_deref(), Some("2026.10.22"));
        let broken = ScriptedDownloader::new(vec![OutputLine::Stdout("Traceback".into())], 1);
        assert_eq!(local_ytdlp_version(&broken), None);
    }

    // --- update_ytdlp ---

    #[test]
    fn a_newer_release_replaces_the_binary_and_keeps_the_old_one() {
        let (_dir, tools) = managed();
        fs::create_dir_all(tools.ytdlp_dir()).unwrap();
        fs::write(tools.ytdlp(), "old").unwrap();
        let http = FakeHttp::default().with(YTDLP_LATEST_RELEASE, release(&days_ago(1))).with(YTDLP_DOWNLOAD, "new");
        let health = ToolHealth::new();

        update_ytdlp(&tools, &http, Some(&days_ago(200)), today(), &health, &InstallProgress::new());

        assert_eq!(fs::read_to_string(tools.ytdlp()).unwrap(), "new");
        assert_eq!(fs::read_to_string(with_suffix(&tools.ytdlp(), ".old")).unwrap(), "old");
        assert!(health.is_healthy());
    }

    #[test]
    fn an_up_to_date_binary_is_not_downloaded_again() {
        let (_dir, tools) = managed();
        let version = days_ago(5);
        let http = FakeHttp::default().with(YTDLP_LATEST_RELEASE, release(&version));

        update_ytdlp(&tools, &http, Some(&version), today(), &ToolHealth::new(), &InstallProgress::new());

        assert_eq!(http.asked(), [YTDLP_LATEST_RELEASE]);
    }

    #[test]
    fn offline_with_a_recent_binary_keeps_it_and_says_so() {
        let (_dir, tools) = managed();
        let http = FakeHttp::default();
        let health = ToolHealth::new();

        update_ytdlp(&tools, &http, Some(&days_ago(10)), today(), &health, &InstallProgress::new());

        assert_eq!(http.asked(), [YTDLP_LATEST_RELEASE]);
        assert_eq!(health.consequence_key(), Some("toolHealth.mayFail"));
    }

    #[test]
    fn without_the_api_an_old_binary_is_updated_blindly() {
        let (_dir, tools) = managed();
        let http = FakeHttp::default().with(YTDLP_DOWNLOAD, "latest");
        let health = ToolHealth::new();

        update_ytdlp(&tools, &http, Some(&days_ago(150)), today(), &health, &InstallProgress::new());

        assert_eq!(fs::read_to_string(tools.ytdlp()).unwrap(), "latest");
        health.resolve(Problem::UpdateCheckFailed);
        assert!(health.is_healthy(), "the outdated binary was replaced, so only the failed check remains");
    }

    #[test]
    fn an_old_binary_that_cannot_be_updated_is_reported() {
        let (_dir, tools) = managed();
        let health = ToolHealth::new();

        update_ytdlp(&tools, &FakeHttp::default(), Some(&days_ago(150)), today(), &health, &InstallProgress::new());

        health.resolve(Problem::UpdateCheckFailed);
        assert!(!health.is_healthy(), "the outdated yt-dlp must stay reported");
    }

    // --- ffmpeg and deno ---

    #[test]
    fn installs_ffmpeg_and_ffprobe_and_deletes_the_zip() {
        let (_dir, tools) = managed();
        let zip = zip_of(&[
            ("ffmpeg-8.0-essentials_build/bin/ffmpeg.exe", "ffmpeg"),
            ("ffmpeg-8.0-essentials_build/bin/ffprobe.exe", "ffprobe"),
            ("ffmpeg-8.0-essentials_build/bin/ffplay.exe", "ffplay"),
        ]);
        let health = ToolHealth::new();

        install_ffmpeg(&tools, &FakeHttp::default().with(FFMPEG_ZIP, zip), &health, &InstallProgress::new()).unwrap();

        assert_eq!(fs::read_to_string(tools.ffmpeg()).unwrap(), "ffmpeg");
        assert_eq!(fs::read_to_string(tools.ffprobe()).unwrap(), "ffprobe");
        assert!(!tools.ffmpeg_bin().join("ffplay.exe").exists());
        assert!(!tools.ffmpeg_dir().join("ffmpeg.zip").exists());
        assert!(health.is_healthy());
    }

    #[test]
    fn an_installed_ffmpeg_is_left_alone() {
        let (_dir, tools) = managed();
        fs::create_dir_all(tools.ffmpeg_bin()).unwrap();
        fs::write(tools.ffmpeg(), "x").unwrap();
        fs::write(tools.ffprobe(), "x").unwrap();
        let http = FakeHttp::default();

        install_ffmpeg(&tools, &http, &ToolHealth::new(), &InstallProgress::new()).unwrap();

        assert!(http.asked().is_empty());
    }

    #[test]
    fn a_zip_without_ffprobe_installs_nothing() {
        let (_dir, tools) = managed();
        let http = FakeHttp::default().with(FFMPEG_ZIP, zip_of(&[("bin/ffmpeg.exe", "x")]));
        assert!(install_ffmpeg(&tools, &http, &ToolHealth::new(), &InstallProgress::new()).is_err());
        assert!(!tools.ffmpeg().exists(), "half an ffmpeg would pass for installed");
        assert!(!tools.ffmpeg_dir().join("new").exists());
    }

    #[test]
    fn a_failed_reinstall_leaves_the_working_ffmpeg_in_place() {
        let (_dir, tools) = managed();
        fs::create_dir_all(tools.ffmpeg_bin()).unwrap();
        fs::write(tools.ffmpeg(), "mine").unwrap();
        fs::write(tools.ffprobe(), "mine").unwrap();

        assert!(reinstall_ffmpeg(&tools, &FakeHttp::default(), &ToolHealth::new(), &InstallProgress::new()).is_err());

        assert_eq!(fs::read_to_string(tools.ffmpeg()).unwrap(), "mine");
        assert_eq!(fs::read_to_string(tools.ffprobe()).unwrap(), "mine");
    }

    #[test]
    fn a_reinstall_downloads_ffmpeg_even_if_it_is_there() {
        let (_dir, tools) = managed();
        fs::create_dir_all(tools.ffmpeg_bin()).unwrap();
        fs::write(tools.ffmpeg(), "old").unwrap();
        fs::write(tools.ffprobe(), "old").unwrap();
        let zip = zip_of(&[("x/bin/ffmpeg.exe", "new"), ("x/bin/ffprobe.exe", "new")]);

        reinstall_ffmpeg(&tools, &FakeHttp::default().with(FFMPEG_ZIP, zip), &ToolHealth::new(), &InstallProgress::new()).unwrap();

        assert_eq!(fs::read_to_string(tools.ffmpeg()).unwrap(), "new");
        assert_eq!(fs::read_to_string(tools.ffprobe()).unwrap(), "new");
    }

    #[test]
    fn the_download_is_told_while_it_lasts_and_ends_empty() {
        let (_dir, tools) = managed();
        let zip = zip_of(&[("bin/ffmpeg.exe", "x"), ("bin/ffprobe.exe", "x")]);
        let progress = InstallProgress::new();
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        progress.on_change(Box::new(move |list| sink.lock().unwrap().push(list)));

        install_ffmpeg(&tools, &FakeHttp::default().with(FFMPEG_ZIP, zip), &ToolHealth::new(), &progress).unwrap();

        let seen = seen.lock().unwrap();
        // The fake server does not say the size: no percent is made up.
        assert_eq!(seen.first().unwrap(), &vec![ToolDownload { id: "ffmpeg", tool: "ffmpeg", percent: None }]);
        assert!(seen.last().unwrap().is_empty());
    }

    // --- seed_ffmpeg ---

    fn bundle_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in files {
            fs::write(dir.path().join(name), format!("bundled {name}")).unwrap();
        }
        dir
    }

    #[test]
    fn the_bundled_ffmpeg_is_copied_in_and_nothing_is_downloaded() {
        let (_dir, tools) = managed();
        let bundle = bundle_with(&["ffmpeg.exe", "ffprobe.exe"]);
        let http = FakeHttp::default();

        assert!(seed_ffmpeg(&tools, bundle.path()).unwrap());
        install_ffmpeg(&tools, &http, &ToolHealth::new(), &InstallProgress::new()).unwrap();

        assert_eq!(fs::read_to_string(tools.ffmpeg()).unwrap(), "bundled ffmpeg.exe");
        assert_eq!(fs::read_to_string(tools.ffprobe()).unwrap(), "bundled ffprobe.exe");
        assert!(http.asked().is_empty());
        // Both stay in the bundle: it is the app's own folder.
        assert!(bundle.path().join("ffmpeg.exe").exists());
    }

    #[test]
    fn an_installed_ffmpeg_is_not_overwritten_by_the_bundled_one() {
        let (_dir, tools) = managed();
        fs::create_dir_all(tools.ffmpeg_bin()).unwrap();
        fs::write(tools.ffmpeg(), "mine").unwrap();
        fs::write(tools.ffprobe(), "mine").unwrap();

        assert!(!seed_ffmpeg(&tools, bundle_with(&["ffmpeg.exe", "ffprobe.exe"]).path()).unwrap());
        assert_eq!(fs::read_to_string(tools.ffmpeg()).unwrap(), "mine");
    }

    #[test]
    fn a_bundle_without_both_binaries_leaves_it_to_the_download() {
        let (_dir, tools) = managed();
        assert!(!seed_ffmpeg(&tools, bundle_with(&["ffmpeg.exe"]).path()).unwrap());
        assert!(!tools.ffmpeg().exists());
    }

    #[test]
    fn installs_deno() {
        let (_dir, tools) = managed();
        let zip = zip_of(&[("README.md", "doc"), ("deno.exe", "deno")]);
        let health = ToolHealth::new();

        install_deno(&tools, &FakeHttp::default().with(DENO_ZIP, zip), &health, &InstallProgress::new()).unwrap();

        assert_eq!(fs::read_to_string(tools.deno()).unwrap(), "deno");
        assert!(!tools.deno_dir().join("deno.zip").exists());
        assert!(health.is_healthy());
    }

    #[test]
    fn deno_that_cannot_be_installed_is_an_error_for_whoever_asked() {
        let (_dir, tools) = managed();
        let health = ToolHealth::new();
        assert!(install_deno(&tools, &FakeHttp::default(), &health, &InstallProgress::new()).is_err());
        // The screen that asked shows it; a notice on top would say it twice.
        assert!(health.is_healthy());
        assert!(!tools.deno().exists());
    }

    #[test]
    fn installing_deno_clears_its_notice() {
        let (_dir, tools) = managed();
        let health = ToolHealth::new();
        health.report(Problem::JsRuntimeMissing, "no deno");
        install_deno(&tools, &FakeHttp::default().with(DENO_ZIP, zip_of(&[("deno.exe", "deno")])), &health, &InstallProgress::new()).unwrap();
        assert!(health.is_healthy());
    }

    // --- install_ytdlp ---

    #[test]
    fn the_first_yt_dlp_is_installed_into_a_folder_that_did_not_exist() {
        let (_dir, tools) = managed();
        install_ytdlp(&tools, &FakeHttp::default().with(YTDLP_DOWNLOAD, "yt"), &ToolHealth::new(), &InstallProgress::new()).unwrap();
        assert_eq!(fs::read_to_string(tools.ytdlp()).unwrap(), "yt");
    }

    #[test]
    fn a_newer_yt_dlp_is_any_other_version() {
        assert!(is_newer_ytdlp(Some("2026.10.22"), "2026.11.01"));
        assert!(!is_newer_ytdlp(Some("2026.11.01"), "v2026.11.01"));
        assert!(is_newer_ytdlp(None, "2026.11.01"));
    }
}
