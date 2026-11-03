//! What Ajustes → Herramientas and the first-start assistant show of each
//! tool: whether it is there and which version. The Java app installed them
//! all by itself at startup and said nothing until a download failed; now the
//! user sees what is missing and installs it from one place.

use std::path::Path;

use serde::Serialize;

use super::ToolPaths;
use super::progress::Tool;

/// One tool as the front shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    /// What the tool says of itself; `None` if it is missing or does not answer.
    pub version: Option<String>,
    /// Missing, but a copy travels with the app: installing it is a copy, not a download.
    pub bundled: bool,
    /// Without it nothing downloads. Deno is only recommended: without it
    /// YouTube sometimes offers the low quality alone.
    pub required: bool,
}

/// Every tool, in the order the screens list them. `version_of` runs the
/// tool and returns what it printed (`None` if it could not run); it is only
/// called for the tools that are there.
pub fn statuses(tools: &ToolPaths, bundled_ffmpeg: Option<&Path>, version_of: &dyn Fn(Tool) -> Option<String>) -> Vec<ToolStatus> {
    Tool::ALL
        .into_iter()
        .map(|tool| {
            let installed = is_installed(tools, tool);
            let bundled = !installed && tool == Tool::Ffmpeg && bundled_ffmpeg.is_some_and(has_ffmpeg);
            ToolStatus {
                id: tool.id(),
                name: tool.name(),
                installed,
                version: if installed { version_of(tool).and_then(|out| parse_version(tool, &out)) } else { None },
                bundled,
                required: tool != Tool::Deno,
            }
        })
        .collect()
}

/// Whether the tool can be run. Outside Windows the system's tools are used
/// and the app cannot tell from a path: there, whatever answers counts.
pub fn is_installed(tools: &ToolPaths, tool: Tool) -> bool {
    if !tools.is_managed() {
        return true;
    }
    match tool {
        Tool::Ytdlp => tools.ytdlp().exists(),
        Tool::Ffmpeg => tools.ffmpeg().exists() && tools.ffprobe().exists(),
        Tool::Deno => tools.deno().exists(),
    }
}

/// The arguments that make the tool print its version.
pub fn version_args(tool: Tool) -> &'static [&'static str] {
    match tool {
        Tool::Ffmpeg => &["-version"],
        Tool::Ytdlp | Tool::Deno => &["--version"],
    }
}

/// The version out of what the tool printed:
/// - yt-dlp: the first line, "2026.10.22";
/// - ffmpeg: "ffmpeg version 7.1-essentials_build-www.gyan.dev Copyright…" → "7.1";
/// - Deno: "deno 2.5.4 (stable, release, x86_64-pc-windows-msvc)" → "2.5.4".
pub fn parse_version(tool: Tool, output: &str) -> Option<String> {
    let first = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    let version = match tool {
        Tool::Ytdlp => first,
        Tool::Ffmpeg => short_ffmpeg_version(first.strip_prefix("ffmpeg version ")?.split_whitespace().next()?),
        Tool::Deno => first.strip_prefix("deno ")?.split_whitespace().next()?,
    };
    Some(version.to_string())
}

/// "7.1-essentials_build-www.gyan.dev" → "7.1", "n7.1" → "7.1". A build from
/// git ("2026-10-01-git-…") carries a date before the dash, which is kept whole.
fn short_ffmpeg_version(raw: &str) -> &str {
    let raw = raw.strip_prefix('n').unwrap_or(raw);
    match raw.split_once('-') {
        Some((number, _)) if number.contains('.') => number,
        _ => raw,
    }
}

fn has_ffmpeg(folder: &Path) -> bool {
    folder.join("ffmpeg.exe").exists() && folder.join("ffprobe.exe").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_each_tools_version() {
        assert_eq!(parse_version(Tool::Ytdlp, "\n2026.10.22\r\n").as_deref(), Some("2026.10.22"));
        assert_eq!(
            parse_version(Tool::Ffmpeg, "ffmpeg version 7.1-essentials_build-www.gyan.dev Copyright (c) 2000-2024\nbuilt with gcc").as_deref(),
            Some("7.1")
        );
        assert_eq!(parse_version(Tool::Ffmpeg, "ffmpeg version n7.0.2 Copyright").as_deref(), Some("7.0.2"));
        assert_eq!(
            parse_version(Tool::Ffmpeg, "ffmpeg version 2026-10-01-git-abc123-full_build Copyright").as_deref(),
            Some("2026-10-01-git-abc123-full_build")
        );
        assert_eq!(
            parse_version(Tool::Deno, "deno 2.5.4 (stable, release, x86_64-pc-windows-msvc)\nv8 14.0").as_deref(),
            Some("2.5.4")
        );
    }

    #[test]
    fn what_is_not_a_version_is_none() {
        assert_eq!(parse_version(Tool::Ytdlp, "   \n"), None);
        assert_eq!(parse_version(Tool::Ffmpeg, "'ffmpeg' is not recognized"), None);
        assert_eq!(parse_version(Tool::Deno, "error: unknown"), None);
    }

    #[test]
    fn lists_what_is_there_and_asks_only_those_their_version() {
        let dir = tempfile::tempdir().unwrap();
        let tools = ToolPaths::new(dir.path(), true);
        fs::create_dir_all(tools.ytdlp_dir()).unwrap();
        fs::write(tools.ytdlp(), "x").unwrap();
        let asked = std::cell::RefCell::new(Vec::new());

        let list = statuses(&tools, None, &|tool| {
            asked.borrow_mut().push(tool);
            Some("2026.10.22".into())
        });

        assert_eq!(list.iter().map(|s| (s.id, s.installed)).collect::<Vec<_>>(), [("ytdlp", true), ("ffmpeg", false), ("deno", false)]);
        assert_eq!(list[0].version.as_deref(), Some("2026.10.22"));
        assert_eq!(*asked.borrow(), [Tool::Ytdlp]);
        assert_eq!(list.iter().map(|s| s.required).collect::<Vec<_>>(), [true, true, false]);
    }

    #[test]
    fn ffmpeg_half_there_is_missing_and_the_bundle_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let tools = ToolPaths::new(dir.path().join("tools"), true);
        fs::create_dir_all(tools.ffmpeg_bin()).unwrap();
        fs::write(tools.ffmpeg(), "x").unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("ffmpeg.exe"), "x").unwrap();
        fs::write(bundle.join("ffprobe.exe"), "x").unwrap();

        let ffmpeg = &statuses(&tools, Some(&bundle), &|_| None)[1];

        assert!(!ffmpeg.installed, "without ffprobe yt-dlp cannot convert");
        assert!(ffmpeg.bundled);
    }

    #[test]
    fn the_systems_tools_always_count_as_there() {
        let tools = ToolPaths::new("nowhere", false);
        assert!(Tool::ALL.into_iter().all(|tool| is_installed(&tools, tool)));
    }
}
