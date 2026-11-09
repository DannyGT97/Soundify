//! Ajustes → Herramientas and the first-start assistant: what each tool is,
//! and installing, reinstalling or updating it on demand. The work itself
//! lives in `soundify_core::tools`; the progress reaches the front through the
//! same `tool-downloads` event as before.

use std::process::Command;

use serde::Serialize;
use soundify_core::http::ReqwestHttp;
use soundify_core::tools::health::ToolHealth;
use soundify_core::tools::progress::{InstallProgress, Tool};
use soundify_core::tools::status::{self, ToolStatus};
use soundify_core::tools::{ToolPaths, updaters};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::bootstrap;
use crate::commands::{AppState, CommandError};
use crate::process;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsReport {
    /// The app keeps its own copies (Windows). Elsewhere the system's tools
    /// are used and there is nothing to install from here.
    managed: bool,
    /// Android: the tools travel inside the APK and are unpacked at startup.
    built_in: bool,
    folder: String,
    tools: Vec<ToolStatus>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YtdlpCheck {
    /// `None` when GitHub did not answer.
    latest: Option<String>,
    newer: bool,
}

/// Each tool, whether it is there and its version. Runs the tools that are
/// there to ask, so it goes off the main thread.
#[tauri::command]
pub async fn tools_report(state: State<'_, AppState>) -> Result<ToolsReport, CommandError> {
    let tools = state.tools.clone();
    tauri::async_runtime::spawn_blocking(move || ToolsReport {
        managed: tools.is_managed(),
        built_in: tools.uses_launcher(),
        folder: tools.ytdlp_dir().parent().map(|p| p.display().to_string()).unwrap_or_default(),
        tools: status::statuses(&tools, bootstrap::bundled_ffmpeg().as_deref(), &|tool| version_output(&tools, tool)),
    })
    .await
    .map_err(CommandError::io)
}

/// Installs `id` if it is missing, or downloads it again with `reinstall`
/// (for yt-dlp, the latest one: «Actualizar» is this too). ffmpeg comes from
/// the copy that travels with the app when there is one.
#[tauri::command]
pub async fn install_tool(app: tauri::AppHandle, state: State<'_, AppState>, id: String, reinstall: bool) -> Result<(), CommandError> {
    let tool = Tool::from_id(&id).ok_or_else(|| CommandError::io(format!("unknown tool {id}")))?;
    // Android: only yt-dlp can be updated; everything else is part of the APK.
    let on_phone = state.tools.uses_launcher() && tool == Tool::Ytdlp;
    if !state.tools.is_managed() && !on_phone {
        return Err(CommandError::io("the system's tools are not installed from the app"));
    }
    if !state.installing.lock().unwrap().insert(tool) {
        return Err(CommandError::key("tools.error.busy"));
    }
    let (tools, health, progress, formats, installing) =
        (state.tools.clone(), state.health.clone(), state.progress.clone(), state.formats.clone(), state.installing.clone());
    let result = tauri::async_runtime::spawn_blocking(move || {
        let installed = if on_phone {
            crate::mobile::update_ytdlp_now(&app).map(drop).map_err(std::io::Error::other)
        } else {
            install(&tools, tool, reinstall, &health, &progress)
        };
        installing.lock().unwrap().remove(&tool);
        // A new yt-dlp may support other formats than the old one.
        if installed.is_ok() && tool == Tool::Ytdlp {
            bootstrap::cache_formats(&tools, &formats);
        }
        installed
    })
    .await
    .map_err(CommandError::io)?;
    result.map_err(|e| CommandError::with_args("tools.error.failed", vec![tool.name().to_string(), e.to_string()]))
}

/// «Buscar actualización» of yt-dlp: asks GitHub, downloads nothing.
#[tauri::command]
pub async fn check_ytdlp_update(state: State<'_, AppState>) -> Result<YtdlpCheck, CommandError> {
    let tools = state.tools.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let latest = updaters::latest_ytdlp(&http().map_err(CommandError::io)?);
        let local = version_output(&tools, Tool::Ytdlp).and_then(|out| status::parse_version(Tool::Ytdlp, &out));
        let newer = latest.as_deref().is_some_and(|latest| updaters::is_newer_ytdlp(local.as_deref(), latest));
        Ok(YtdlpCheck { latest, newer })
    })
    .await
    .map_err(CommandError::io)?
}

/// «Abrir carpeta»: where the tools live, in the system's explorer.
#[tauri::command]
pub fn open_tools_folder(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let folder = state.tools.ytdlp_dir().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    std::fs::create_dir_all(&folder).map_err(CommandError::io)?;
    app.opener().open_path(folder.to_string_lossy(), None::<&str>).map_err(CommandError::io)
}

// === PRIVATE HELPERS ===

fn install(
    tools: &ToolPaths,
    tool: Tool,
    reinstall: bool,
    health: &ToolHealth,
    progress: &InstallProgress,
) -> std::io::Result<()> {
    if tool == Tool::Ffmpeg && !reinstall
        && let Some(bundled) = bootstrap::bundled_ffmpeg()
        && updaters::seed_ffmpeg(tools, &bundled)?
    {
        return Ok(());
    }
    let http = http()?;
    match (tool, reinstall) {
        (Tool::Ytdlp, _) => updaters::install_ytdlp(tools, &http, health, progress),
        (Tool::Ffmpeg, false) => updaters::install_ffmpeg(tools, &http, health, progress),
        (Tool::Ffmpeg, true) => updaters::reinstall_ffmpeg(tools, &http, health, progress),
        (Tool::Deno, false) => updaters::install_deno(tools, &http, health, progress),
        (Tool::Deno, true) => updaters::reinstall_deno(tools, &http, health, progress),
    }
}

fn http() -> std::io::Result<ReqwestHttp> {
    ReqwestHttp::new(env!("CARGO_PKG_VERSION")).map_err(std::io::Error::other)
}

/// What the tool prints when asked its version; `None` if it does not run.
pub fn version_output(tools: &ToolPaths, tool: Tool) -> Option<String> {
    let mut command = if tools.uses_launcher() {
        // Android: the tools are libraries of the APK, started the way the launcher says.
        let launcher = tools.launcher()?;
        let mut command = match tool {
            Tool::Ytdlp => {
                let mut c = Command::new(&launcher.program);
                c.args(&launcher.args);
                c
            }
            Tool::Ffmpeg => {
                let (program, fixed) = launcher.ffmpeg.split_first()?;
                let mut c = Command::new(program);
                c.args(fixed);
                c
            }
            // QuickJS takes Deno's place there.
            Tool::Deno => return None,
        };
        command.envs(launcher.env.iter().map(|(k, v)| (k, v)));
        command
    } else {
        Command::new(match tool {
            Tool::Ytdlp => tools.ytdlp(),
            Tool::Ffmpeg => tools.ffmpeg(),
            Tool::Deno => tools.deno(),
        })
    };
    command.args(status::version_args(tool));
    process::detach(&mut command);
    let output = command.output().inspect_err(|e| log::warn!("Could not ask {} its version: {e}", tool.name())).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
