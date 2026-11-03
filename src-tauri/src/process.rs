//! Running yt-dlp as a process: the desktop [`Downloader`] (the Java
//! `YtDlpProcess` and the process side of `DownloadThreadService`).

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use soundify_core::downloader::{Downloader, OutputLine, RunEnd, StopFlag, YtDlpRequest};
use soundify_core::tools::ToolPaths;

/// The single place yt-dlp processes are built: any invocation (a download or
/// a query) gets the same environment, Deno on the PATH included.
pub struct YtDlpProcess {
    tools: ToolPaths,
}

impl YtDlpProcess {
    pub fn new(tools: ToolPaths) -> Self {
        Self { tools }
    }

    fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new(self.tools.ytdlp());
        command.args(with_utf8_encoding(args));
        // Without this, on Windows yt-dlp (Python) can dump its output in the
        // console encoding (cp1252) when stdout is a pipe. Any title with an
        // accent then arrives corrupted, and since the final path is captured
        // on that same line (--print after_move), it no longer matches the real
        // file — the archive marks it "deleted" when it is not. This is the
        // Python side; reading the stream as UTF-8 is the other.
        command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
        // Deno's folder in front of the PATH so yt-dlp can solve YouTube's
        // n-challenge. Only if the managed binary exists; otherwise the system's
        // 'deno' (if any) is trusted.
        if self.tools.is_managed() && self.tools.deno().exists() {
            let current = std::env::var_os("PATH").unwrap_or_default();
            let dirs = std::iter::once(self.tools.deno_dir()).chain(std::env::split_paths(&current));
            if let Ok(path) = std::env::join_paths(dirs) {
                command.env("PATH", path);
            }
        }
        command
    }
}

impl Downloader for YtDlpProcess {
    fn run(&self, request: &YtDlpRequest, stop: &StopFlag, on_line: &mut dyn FnMut(OutputLine)) -> io::Result<RunEnd> {
        run_lines(self.command(&request.args), stop, on_line)
    }
}

/// Adds "--encoding utf-8" unless the arguments already carry an encoding.
///
/// The environment variables are NOT enough: the Windows yt-dlp is a packaged
/// executable that ignores them and keeps writing cp1252. Checked by hand: an
/// "á" came out as the byte 0xE1 instead of 0xC3 0xA1. It was not cosmetic:
/// each file's final path is captured through this same output, so the path
/// stored in the archive did not match the file on disk, and every file with
/// accents or an "ñ" stayed "pending" despite being downloaded (real bug: out of
/// 11 files only the one with a 100% ASCII title came up "OK").
fn with_utf8_encoding(args: &[String]) -> Vec<String> {
    if args.iter().any(|a| a == "--encoding") {
        return args.to_vec();
    }
    let mut all = vec!["--encoding".to_string(), "utf-8".to_string()];
    all.extend_from_slice(args);
    all
}

/// Runs `command`, handing each line of stdout and stderr to `on_line` as it
/// comes, until it ends or `stop` is raised.
///
/// Each pipe gets a reader thread of its own. Reading stdout to the end first
/// and stderr afterwards hangs: the pipes have a few KB of buffer, and with
/// YouTube asking for a bot-check on every item of a 726-item playlist, the
/// Java app's id query hung for four minutes that way (reported). Java fixed it
/// by mixing stderr into stdout; here both are read at once and stay apart.
pub fn run_lines(mut command: Command, stop: &StopFlag, on_line: &mut dyn FnMut(OutputLine)) -> io::Result<RunEnd> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    detach(&mut command);
    let mut child = command.spawn()?;

    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        read_lines(out, tx.clone(), OutputLine::Stdout);
    }
    if let Some(err) = child.stderr.take() {
        read_lines(err, tx, OutputLine::Stderr);
    }

    loop {
        if stop.is_stopped() {
            kill_tree(&mut child);
            return Ok(RunEnd::Stopped);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => on_line(line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait()?;
    Ok(RunEnd::Finished { exit_code: status.code().unwrap_or(-1) })
}

fn read_lines<R: Read + Send + 'static>(pipe: R, tx: mpsc::Sender<OutputLine>, wrap: fn(String) -> OutputLine) {
    std::thread::spawn(move || {
        for bytes in BufReader::new(pipe).split(b'\n') {
            let Ok(bytes) = bytes else { break };
            let text = String::from_utf8_lossy(&bytes);
            if tx.send(wrap(text.trim_end_matches('\r').to_string())).is_err() {
                break;
            }
        }
    });
}

/// Kills the whole tree: yt-dlp starts ffmpeg, and killing only yt-dlp leaves
/// ffmpeg writing the file.
fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let mut killer = {
        let mut c = Command::new("taskkill");
        c.args(["/T", "/F", "/PID", &pid]);
        c
    };
    #[cfg(not(windows))]
    let mut killer = {
        let mut c = Command::new("kill");
        c.args(["-KILL", &format!("-{pid}")]);
        c
    };
    killer.stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut killer);
    if let Err(e) = killer.status() {
        log::warn!("Could not kill the process tree of {pid}: {e}");
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// No console window on Windows; its own process group elsewhere, so the
/// whole tree can be killed at once.
pub(crate) fn detach(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn adds_the_utf8_encoding_once() {
        let args = vec!["--version".to_string()];
        assert_eq!(with_utf8_encoding(&args), ["--encoding", "utf-8", "--version"]);
        let already = vec!["--encoding".to_string(), "cp1252".to_string()];
        assert_eq!(with_utf8_encoding(&already), already);
    }

    #[test]
    fn hands_over_both_pipes_and_the_exit_code() {
        let mut lines = Vec::new();
        let end = run_lines(shell("echo hola&& echo fallo 1>&2&& exit 3"), &StopFlag::new(), &mut |l| lines.push(l)).unwrap();
        assert_eq!(end, RunEnd::Finished { exit_code: 3 });
        assert!(lines.contains(&OutputLine::Stdout("hola".into())), "{lines:?}");
        assert!(lines.iter().any(|l| matches!(l, OutputLine::Stderr(t) if t.trim() == "fallo")), "{lines:?}");
    }

    #[test]
    fn stop_ends_a_long_run_quickly() {
        let script = if cfg!(windows) { "ping -n 30 127.0.0.1" } else { "sleep 30" };
        let stop = StopFlag::new();
        let raiser = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            raiser.stop();
        });
        let started = Instant::now();
        let end = run_lines(shell(script), &stop, &mut |_| {}).unwrap();
        assert_eq!(end, RunEnd::Stopped);
        assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    }

    #[test]
    fn a_missing_executable_is_an_error_not_a_hang() {
        let result = run_lines(Command::new("soundify-no-such-tool"), &StopFlag::new(), &mut |_| {});
        assert!(result.is_err());
    }
}
