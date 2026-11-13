//! Soundify Connect in the app (the logic is in `core::connect`): what the front can
//! ask, and the two ends the core needs — how to do what a controller asks (hand it to
//! the page, which owns the queue) and where to tell the screen what happens.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use soundify_core::Location;
use soundify_core::connect::discovery::{Discovery, Mdns};
use soundify_core::connect::manager::{Connect, ConnectError, DeviceView, Identity, Listener, PREFERRED_PORT, Player};
use soundify_core::connect::pairing::{KnownDevices, Pairing, Trusted, new_device_id};
use soundify_core::connect::protocol::{Command, PlayerState};
use soundify_core::storage::{LocalStorage, Storage};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_paths::AppPaths;
use crate::commands::CommandError;

/// What is kept between runs: who this device is, what it is called and whether Connect is on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    device_id: String,
    name: String,
    enabled: bool,
}

pub struct ConnectState {
    connect: Arc<Connect>,
    config: Mutex<Config>,
    file: Location,
}

/// What a controller asks goes to the page: it owns the queue.
struct AppPlayer {
    app: AppHandle,
}

#[derive(Serialize, Clone)]
struct IncomingCommand<'a> {
    from: &'a str,
    command: &'a Command,
}

impl Player for AppPlayer {
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String> {
        self.app.emit("connect-command", IncomingCommand { from: &who.name, command: &command }).map_err(|e| e.to_string())
    }
}

/// What happens is told to the page, which asks again for what it shows.
struct AppListener {
    app: AppHandle,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RemoteState<'a> {
    device_id: &'a str,
    state: &'a PlayerState,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RemoteClosed<'a> {
    device_id: &'a str,
    reason: Option<&'a str>,
}

impl Listener for AppListener {
    fn devices_changed(&self) {
        let _ = self.app.emit("connect-devices", ());
    }

    fn remote_state(&self, device_id: &str, state: PlayerState) {
        let _ = self.app.emit("connect-remote-state", RemoteState { device_id, state: &state });
    }

    fn remote_closed(&self, device_id: &str, reason: Option<String>) {
        let _ = self.app.emit("connect-remote-closed", RemoteClosed { device_id, reason: reason.as_deref() });
    }
}

/// Builds Connect and, if it was left on, turns it on (in a thread of its own: it opens sockets).
pub fn init(app: &AppHandle, paths: &AppPaths) {
    let dir = paths.connect_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("Could not create {}: {e}", dir.display());
    }
    let file = Location::path(dir.join("connect.json"));
    let config = load_config(&file);
    let storage: Arc<dyn Storage> = Arc::new(LocalStorage);
    let pairing = Arc::new(Pairing::load(storage.clone(), Location::path(dir.join("trusted.json"))));
    let known = KnownDevices::load(storage, Location::path(dir.join("devices.json")));
    let identity = Identity { device_id: config.device_id.clone(), name: config.name.clone(), kind: device_kind().into() };
    let handle = app.clone();
    let connect = Connect::new(
        identity,
        || Ok(Arc::new(Mdns::new()?) as Arc<dyn Discovery>),
        pairing,
        known,
        Arc::new(AppPlayer { app: app.clone() }),
        Arc::new(AppListener { app: handle }),
    );
    let enabled = config.enabled;
    app.manage(ConnectState { connect: connect.clone(), config: Mutex::new(config), file });
    if enabled {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = turn_on(&app, &connect) {
                warn!("Soundify Connect could not start: {e}");
            }
        });
    }
}

/// On Android mDNS needs a lock on the Wi-Fi radio, or the phone drops the multicast packets it lives on.
fn turn_on(app: &AppHandle, connect: &Arc<Connect>) -> std::io::Result<()> {
    crate::mobile::set_multicast(app, true);
    connect.start(PREFERRED_PORT)
}

fn turn_off(app: &AppHandle, connect: &Connect) {
    connect.stop();
    crate::mobile::set_multicast(app, false);
}

fn load_config(file: &Location) -> Config {
    let storage = LocalStorage;
    if let Some(config) = storage.read(file).ok().and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok()) {
        return config;
    }
    let config = Config { device_id: new_device_id(), name: default_name(), enabled: false };
    save_config(file, &config);
    config
}

fn save_config(file: &Location, config: &Config) {
    match serde_json::to_vec_pretty(config) {
        Ok(bytes) => {
            if let Err(e) = LocalStorage.write(file, &bytes) {
                warn!("Could not save the Connect settings: {e}");
            }
        }
        Err(e) => warn!("Could not write the Connect settings: {e}"),
    }
}

/// What the machine calls itself, unless the user renames it.
fn default_name() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|name| !name.trim().is_empty()))
        .unwrap_or_else(|| if cfg!(target_os = "android") { "Teléfono Android".into() } else { "Soundify".into() })
}

fn device_kind() -> &'static str {
    if cfg!(target_os = "android") { "phone" } else { "desktop" }
}

fn error(error: ConnectError) -> CommandError {
    match error {
        ConnectError::Offline => CommandError::key("connect.error.offline"),
        ConnectError::NotPaired => CommandError::key("connect.error.notPaired"),
        ConnectError::NoSession => CommandError::key("connect.error.noSession"),
        ConnectError::Refused { status, error, seconds } => match (status, error.as_str()) {
            (403, _) => CommandError::key("connect.error.wrongPin"),
            (410, _) => CommandError::key("connect.error.noPin"),
            (429, _) => CommandError::with_args("connect.error.locked", vec![seconds.unwrap_or(60).to_string()]),
            (401, _) => CommandError::key("connect.error.unauthorized"),
            _ => CommandError::with_args("connect.error.other", vec![error]),
        },
        ConnectError::Other(message) => CommandError::with_args("connect.error.other", vec![message]),
    }
}

// === WHAT THE FRONT ASKS ===

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerView {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    running: bool,
    port: Option<u16>,
    name: String,
    device_id: String,
    /// The device being controlled, if any.
    selected: Option<String>,
    /// Who may control this device.
    controllers: Vec<ControllerView>,
    connected_controllers: usize,
}

#[tauri::command]
pub fn connect_status(state: State<'_, ConnectState>) -> Status {
    let config = state.config.lock().unwrap().clone();
    Status {
        enabled: config.enabled,
        running: state.connect.is_running(),
        port: state.connect.port(),
        name: config.name,
        device_id: config.device_id,
        selected: state.connect.selected(),
        controllers: state.connect.controllers().into_iter().map(|t| ControllerView { id: t.controller_id, name: t.name }).collect(),
        connected_controllers: state.connect.connected_controllers(),
    }
}

/// Turns Connect on or off (and remembers it).
#[tauri::command]
pub async fn connect_set_enabled(app: AppHandle, state: State<'_, ConnectState>, enabled: bool) -> Result<(), CommandError> {
    let connect = state.connect.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            turn_on(&app, &connect)
        } else {
            turn_off(&app, &connect);
            Ok(())
        }
    })
    .await
    .map_err(CommandError::io)?;
    result.map_err(CommandError::io)?;
    let mut config = state.config.lock().unwrap();
    config.enabled = enabled;
    save_config(&state.file, &config);
    info!("Soundify Connect {}", if enabled { "on" } else { "off" });
    Ok(())
}

/// The name the others see. Applied the next time Connect is turned on.
#[tauri::command]
pub fn connect_set_name(state: State<'_, ConnectState>, name: String) -> Result<(), CommandError> {
    let name = name.trim().chars().take(60).collect::<String>();
    if name.is_empty() {
        return Err(CommandError::key("connect.error.name"));
    }
    let mut config = state.config.lock().unwrap();
    config.name = name;
    save_config(&state.file, &config);
    Ok(())
}

#[tauri::command]
pub fn connect_devices(state: State<'_, ConnectState>) -> Vec<DeviceView> {
    state.connect.devices()
}

/// «Añadir por dirección»: `192.168.1.20` or `192.168.1.20:48650`.
#[tauri::command]
pub async fn connect_add(state: State<'_, ConnectState>, address: String) -> Result<(), CommandError> {
    let addr = parse_address(&address).ok_or_else(|| CommandError::key("connect.error.address"))?;
    let connect = state.connect.clone();
    tauri::async_runtime::spawn_blocking(move || connect.add_by_address(addr)).await.map_err(CommandError::io)?.map(|_| ()).map_err(error)
}

fn parse_address(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    text.parse::<SocketAddr>().ok().or_else(|| text.parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, PREFERRED_PORT)))
}

/// The PIN to show on this device's screen.
#[tauri::command]
pub fn connect_pin(state: State<'_, ConnectState>) -> Option<String> {
    state.connect.pin()
}

#[tauri::command]
pub fn connect_renew_pin(state: State<'_, ConnectState>) -> Option<String> {
    state.connect.renew_pin()
}

#[tauri::command]
pub async fn connect_pair(state: State<'_, ConnectState>, device_id: String, pin: String) -> Result<(), CommandError> {
    let connect = state.connect.clone();
    tauri::async_runtime::spawn_blocking(move || connect.pair(&device_id, &pin)).await.map_err(CommandError::io)?.map_err(error)
}

#[tauri::command]
pub fn connect_forget(state: State<'_, ConnectState>, device_id: String) -> Result<(), CommandError> {
    state.connect.forget(&device_id).map_err(error)
}

#[tauri::command]
pub fn connect_forget_controller(state: State<'_, ConnectState>, controller_id: String) -> Result<(), CommandError> {
    state.connect.forget_controller(&controller_id).map_err(error)
}

/// Starts controlling a paired device.
#[tauri::command]
pub async fn connect_select(state: State<'_, ConnectState>, device_id: String) -> Result<(), CommandError> {
    let connect = state.connect.clone();
    tauri::async_runtime::spawn_blocking(move || connect.select(&device_id)).await.map_err(CommandError::io)?.map_err(error)
}

#[tauri::command]
pub fn connect_deselect(state: State<'_, ConnectState>) {
    state.connect.deselect();
}

/// An order for the device being controlled.
#[tauri::command]
pub fn connect_send(state: State<'_, ConnectState>, command: Command) -> Result<(), CommandError> {
    state.connect.send(command).map(|_| ()).map_err(error)
}

/// What this device plays now, for those who control it.
#[tauri::command]
pub fn connect_publish(state: State<'_, ConnectState>, player: PlayerState) {
    state.connect.publish(player);
}
