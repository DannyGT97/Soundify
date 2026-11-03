//! The single place to fetch something over HTTP: text (the GitHub API) or a
//! file (the yt-dlp, ffmpeg and Deno binaries). The Java `HttpDownloader`.
//!
//! There used to be four nearly identical implementations spread across the
//! updaters and the update checker, each of them remembering (or not) to follow
//! redirects, send a User-Agent, check the status code or clean up the half
//! written file when something failed.
//!
//! Behind a trait so the updaters can be tested without a network.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub trait Http: Send + Sync {
    /// The response body as text. Fails if the status is not 200: the caller
    /// must never get to parse an error body as if it were good data.
    fn get_string(&self, url: &str, headers: &[(&str, &str)]) -> io::Result<String>;

    /// Downloads to `target`, creating whatever folders are needed.
    ///
    /// `on_progress` receives the progress between 0 and 1, only if the server
    /// reports the size: when it does not, no fake progress is invented. On
    /// failure the half written file is deleted: leaving it would make the tool
    /// look installed.
    fn to_file(&self, url: &str, target: &Path, on_progress: &mut dyn FnMut(f64)) -> io::Result<()>;
}

/// The real one, over `reqwest`.
pub struct ReqwestHttp {
    client: reqwest::blocking::Client,
}

impl ReqwestHttp {
    pub fn new(app_version: &str) -> io::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            // GitHub REJECTS requests with no User-Agent, so this is not optional.
            .user_agent(format!("Soundify/{app_version}"))
            // Binary downloads (GitHub releases, gyan.dev) always answer with a
            // 302 to a CDN; reqwest follows up to 10 by default, set explicitly
            // so nobody turns it off thinking it is noise.
            .redirect(reqwest::redirect::Policy::limited(10))
            // A download of ffmpeg (~100 MB) on a slow line takes minutes: only
            // the connection gets a limit, never the whole transfer.
            .timeout(None)
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(io::Error::other)?;
        Ok(Self { client })
    }

    fn get(&self, url: &str, headers: &[(&str, &str)]) -> io::Result<reqwest::blocking::Response> {
        let mut request = self.client.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().map_err(io::Error::other)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(io::Error::other(format!("HTTP {status} fetching {url}")));
        }
        Ok(response)
    }
}

impl Http for ReqwestHttp {
    fn get_string(&self, url: &str, headers: &[(&str, &str)]) -> io::Result<String> {
        self.get(url, headers)?.text().map_err(io::Error::other)
    }

    fn to_file(&self, url: &str, target: &Path, on_progress: &mut dyn FnMut(f64)) -> io::Result<()> {
        let result = self.get(url, &[]).and_then(|response| {
            let expected = response.content_length();
            write_atomically(response, target, expected, on_progress)
        });
        if result.is_err() {
            remove_quietly(target);
        }
        result
    }
}

/// Copies `reader` into `target` through a `.part` file that is only renamed
/// at the end: if the process dies mid-download, no file is left with the
/// good name and incomplete contents.
pub fn write_atomically(
    mut reader: impl Read,
    target: &Path,
    expected_bytes: Option<u64>,
    on_progress: &mut dyn FnMut(f64),
) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = part_path(target);
    let copied = (|| {
        let mut out = fs::File::create(&temp)?;
        let mut buffer = [0u8; 8192];
        let mut written = 0u64;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])?;
            written += read as u64;
            if let Some(fraction) = fraction(written, expected_bytes) {
                on_progress(fraction);
            }
        }
        out.flush()
    })();
    if let Err(e) = copied {
        remove_quietly(&temp);
        return Err(e);
    }
    fs::rename(&temp, target)
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    target.with_file_name(name)
}

fn fraction(written: u64, expected: Option<u64>) -> Option<f64> {
    match expected {
        Some(total) if total > 0 => Some((written as f64 / total as f64).min(1.0)),
        _ => None,
    }
}

fn remove_quietly(path: &Path) {
    if let Err(e) = fs::remove_file(path)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("Could not delete the incomplete file {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Hands out some bytes, then fails, like a connection dropped mid-download.
    struct Broken(usize);

    impl Read for Broken {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::other("connection reset"));
            }
            let n = self.0.min(buf.len());
            buf[..n].fill(b'x');
            self.0 -= n;
            Ok(n)
        }
    }

    #[test]
    fn writes_the_file_and_creates_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a/b/yt-dlp.exe");
        write_atomically(Cursor::new(b"binary"), &target, None, &mut |_| {}).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"binary");
        assert!(!part_path(&target).exists(), "the .part file must be gone");
    }

    #[test]
    fn overwrites_an_earlier_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deno.zip");
        fs::write(&target, "old").unwrap();
        write_atomically(Cursor::new(b"new"), &target, None, &mut |_| {}).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn reports_progress_only_when_the_size_is_known() {
        let dir = tempfile::tempdir().unwrap();
        let data = vec![0u8; 20_000];

        let mut seen = Vec::new();
        write_atomically(Cursor::new(&data), &dir.path().join("a"), Some(20_000), &mut |f| seen.push(f)).unwrap();
        assert!(!seen.is_empty());
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "never goes backwards: {seen:?}");
        assert_eq!(*seen.last().unwrap(), 1.0);

        let mut called = false;
        write_atomically(Cursor::new(&data), &dir.path().join("b"), None, &mut |_| called = true).unwrap();
        assert!(!called, "no size, no made-up progress");
    }

    #[test]
    fn a_server_that_lies_about_the_size_never_goes_past_one() {
        assert_eq!(fraction(150, Some(100)), Some(1.0));
        assert_eq!(fraction(50, Some(0)), None);
    }

    #[test]
    fn a_dropped_connection_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ffmpeg.zip");
        let result = write_atomically(Broken(10_000), &target, None, &mut |_| {});
        assert!(result.is_err());
        assert!(!target.exists());
        assert!(!part_path(&target).exists());
    }

    #[test]
    #[ignore = "goes to the network"]
    fn real_download_from_github() {
        let http = ReqwestHttp::new("test").unwrap();
        let body = http
            .get_string(crate::tools::updaters::YTDLP_LATEST_RELEASE, &[("Accept", "application/vnd.github+json")])
            .unwrap();
        assert!(crate::tools::updaters::remote_version(&body).is_some(), "{body}");
    }
}
