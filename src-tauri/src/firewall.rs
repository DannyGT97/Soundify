//! Windows' firewall and Soundify Connect. Without a rule for the app, Windows drops what other
//! devices send to it (the port and the discovery), and a portable app has no installer to make
//! the rule: Windows asks once, and if that question was closed without answering, nothing works.
//! Here the user can ask for the rule from the app (Windows then shows its own administrator
//! confirmation), and is told when the network is «Public», which blocks incoming traffic anyway.
//! Nothing here runs unless the user presses the button.

use serde::Serialize;

use crate::commands::CommandError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    /// There is a Windows firewall to ask (false everywhere else).
    supported: bool,
    /// A rule that lets this app in already exists.
    allowed: bool,
    /// The network in use is «Public»: it blocks incoming connections whatever the rules say.
    public_network: bool,
}

/// The rule's name: the same for every copy, told apart by the program it points to.
#[cfg(windows)]
const RULE: &str = "Soundify Connect";

#[tauri::command]
pub async fn connect_firewall_status() -> FirewallStatus {
    tauri::async_runtime::spawn_blocking(status).await.unwrap_or(FirewallStatus { supported: false, allowed: false, public_network: false })
}

/// Asks Windows for the rule (a confirmation of the administrator comes up); the status afterwards comes back.
#[tauri::command]
pub async fn connect_firewall_allow() -> Result<FirewallStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(allow).await.map_err(CommandError::io)?
}

/// Marks the connected «Public» networks as «Private» (Windows asks to confirm as administrator): the only way a
/// firewall rule can let anything in. Only when the user presses the button, and meant for the network at home.
#[tauri::command]
pub async fn connect_network_private() -> Result<FirewallStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(make_private).await.map_err(CommandError::io)?
}

#[cfg(not(windows))]
fn make_private() -> Result<FirewallStatus, CommandError> {
    Ok(status())
}

#[cfg(not(windows))]
fn status() -> FirewallStatus {
    FirewallStatus { supported: false, allowed: false, public_network: false }
}

#[cfg(not(windows))]
fn allow() -> Result<FirewallStatus, CommandError> {
    Ok(status())
}

#[cfg(windows)]
mod windows_impl {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    use super::{CommandError, FirewallStatus, RULE};

    /// No console window flashes when a tool is run.
    const NO_WINDOW: u32 = 0x0800_0000;

    fn exe() -> Option<PathBuf> {
        std::env::current_exe().ok()
    }

    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).creation_flags(NO_WINDOW).output().ok()?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Is there a rule with this name that points at THIS executable? (The listing is in the system's language: the path is what is looked for.)
    fn rule_exists(exe: &str) -> bool {
        let Some(listing) = run("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={RULE}"), "verbose"]) else { return false };
        listing.to_lowercase().contains(&exe.to_lowercase())
    }

    /// Does any connected network have the «Public» category?
    fn public_network() -> bool {
        run("powershell", &["-NoProfile", "-NonInteractive", "-Command", "(Get-NetConnectionProfile | ForEach-Object { $_.NetworkCategory }) -join ','"])
            .is_some_and(|categories| categories.split(',').any(|c| c.trim().eq_ignore_ascii_case("Public")))
    }

    pub fn status() -> FirewallStatus {
        let allowed = exe().is_some_and(|exe| rule_exists(&exe.to_string_lossy()));
        FirewallStatus { supported: true, allowed, public_network: public_network() }
    }

    /// The script travels encoded (`-EncodedCommand`): no quoting of it can go wrong.
    pub fn make_private() -> Result<FirewallStatus, CommandError> {
        use base64::Engine;
        let inner = "Get-NetConnectionProfile | Where-Object { $_.NetworkCategory -eq 'Public' } | Set-NetConnectionProfile -NetworkCategory Private";
        let utf16: Vec<u8> = inner.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        let script = format!(
            "try {{ Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand','{encoded}'); exit 0 }} catch {{ exit 2 }}"
        );
        let result = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(NO_WINDOW).status().map_err(CommandError::io)?;
        if !result.success() {
            return Err(CommandError::key("connect.firewall.cancelled"));
        }
        let after = status();
        if after.public_network { Err(CommandError::key("connect.network.failed")) } else { Ok(after) }
    }

    pub fn allow() -> Result<FirewallStatus, CommandError> {
        let exe = exe().ok_or_else(|| CommandError::key("connect.firewall.failed"))?;
        // The rule lets the app in on PRIVATE networks only (home, not a café). The path travels in an environment
        // variable, not in the command text: no quoting of a path with spaces or odd characters can go wrong.
        let script = format!(
            "try {{ Start-Process -FilePath netsh -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @('advfirewall','firewall','add','rule','name=\"{RULE}\"','dir=in','action=allow',('program=\"' + $env:SOUNDIFY_EXE + '\"'),'profile=private','enable=yes','protocol=any'); exit 0 }} catch {{ exit 2 }}"
        );
        let result = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .env("SOUNDIFY_EXE", &exe)
            .creation_flags(NO_WINDOW)
            .status()
            .map_err(CommandError::io)?;
        if !result.success() {
            // The user closed the administrator's confirmation (or has no rights to give it).
            return Err(CommandError::key("connect.firewall.cancelled"));
        }
        let after = status();
        if after.allowed { Ok(after) } else { Err(CommandError::key("connect.firewall.failed")) }
    }
}

#[cfg(windows)]
fn status() -> FirewallStatus {
    windows_impl::status()
}

#[cfg(windows)]
fn allow() -> Result<FirewallStatus, CommandError> {
    windows_impl::allow()
}

#[cfg(windows)]
fn make_private() -> Result<FirewallStatus, CommandError> {
    windows_impl::make_private()
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn the_status_can_be_read_without_changing_anything() {
        let status = super::status();
        println!("firewall: allowed={} public_network={}", status.allowed, status.public_network);
        assert!(status.supported);
    }
}
