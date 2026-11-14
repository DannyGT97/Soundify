//! Soundify Connect in the app (the logic is in `core::connect`): what the front can
//! ask, and the two ends the core needs — how to do what a controller asks (hand it to
//! the page, which owns the queue) and where to tell the screen what happens.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use soundify_core::Location;
use soundify_core::connect::beacon::{Beacon, Composite};
use soundify_core::connect::discovery::{Discovery, Mdns};
use soundify_core::connect::manager::{Connect, ConnectError, DeviceView, Identity, Listener, PREFERRED_PORT, Player};
use soundify_core::connect::pairing::{KnownDevices, Pairing, Trusted, new_device_id};
use soundify_core::connect::protocol::{Command, PlayerState, QueueInfo};
use soundify_core::settings;
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
    /// Any device on the network may pair without a PIN (like Spotify: they just show up). Off = a PIN is asked for.
    #[serde(default = "yes")]
    open: bool,
}

fn yes() -> bool {
    true
}

pub struct ConnectState {
    connect: Arc<Connect>,
    config: Mutex<Config>,
    file: Location,
    /// Why it could not start, for the window to say.
    error: Arc<Mutex<Option<String>>>,
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
struct RemoteQueue<'a> {
    device_id: &'a str,
    queue: &'a Option<QueueInfo>,
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
        show_on_phone(&self.app, device_id, Some(&state));
    }

    fn remote_queue(&self, device_id: &str, queue: Option<QueueInfo>) {
        let _ = self.app.emit("connect-remote-queue", RemoteQueue { device_id, queue: &queue });
    }

    fn remote_closed(&self, device_id: &str, reason: Option<String>) {
        show_on_phone(&self.app, device_id, None);
        let _ = self.app.emit("connect-remote-closed", RemoteClosed { device_id, reason: reason.as_deref() });
    }
}

/// While another device is controlled, the phone's media notification shows what IT plays and where (`state` None = it
/// is over). Only for the device being controlled; does nothing outside Android.
fn show_on_phone(app: &AppHandle, device_id: &str, state: Option<&PlayerState>) {
    if !cfg!(target_os = "android") {
        return;
    }
    let connect = &app.state::<ConnectState>().connect;
    if state.is_some() && connect.selected().as_deref() != Some(device_id) {
        return;
    }
    let Some(state) = state else {
        crate::mobile::remote_session(app, None);
        return;
    };
    let Some(playing) = state.now_playing.as_ref() else {
        crate::mobile::remote_session(app, None);
        return;
    };
    let name = connect.devices().into_iter().find(|device| device.id == device_id).map(|device| device.name).unwrap_or_default();
    crate::mobile::remote_session(
        app,
        Some(&crate::mobile::RemoteSession {
            device: on_device(&language(app), &name),
            title: playing.title.clone(),
            playlist: playing.playlist.clone(),
            playing: state.playing,
            position: state.position,
            duration: state.duration,
            shuffle: state.shuffle,
            repeat: state.repeat.clone(),
            volume: state.volume,
        }),
    );
}

fn language(app: &AppHandle) -> String {
    settings::load(&LocalStorage, &app.state::<crate::commands::AppState>().paths.settings()).language
}

/// «En "PC"»: where the music sounds, in the app's language.
fn on_device(language: &str, name: &str) -> String {
    match language {
        "en" => format!("On \"{name}\""),
        "ca" => format!("A «{name}»"),
        _ => format!("En «{name}»"),
    }
}

/// A button of the phone's notification of the device being controlled: asked of that device from here, because the
/// page may be asleep (screen off). `seconds` only means something for `seek`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn notification_action(app: &AppHandle, action: &str, seconds: f64) {
    let command = match action {
        "play" => Command::Play,
        "pause" => Command::Pause,
        "next" => Command::Next,
        "previous" => Command::Previous,
        "shuffle" => Command::Shuffle { on: None },
        "repeat" => Command::Repeat { mode: None },
        "seek" => Command::Seek { seconds },
        // The phone's volume buttons while another device is controlled: `seconds` carries the volume (0 to 1).
        "volume" => Command::Volume { volume: seconds.clamp(0.0, 1.0) },
        _ => return,
    };
    if let Err(e) = app.state::<ConnectState>().connect.send(command) {
        warn!("A button of the notification could not be sent to the device being controlled: {e:?}");
    }
}

/// Two ways of finding the others at once: mDNS (the standard one) and a UDP beacon (for the networks where
/// multicast does not get through). If one cannot start, the other is enough.
fn discovery() -> std::io::Result<Arc<dyn Discovery>> {
    let mut parts: Vec<Arc<dyn Discovery>> = Vec::new();
    match Mdns::new() {
        Ok(mdns) => parts.push(Arc::new(mdns)),
        Err(e) => warn!("Soundify Connect: no mDNS: {e}"),
    }
    match Beacon::new() {
        Ok(beacon) => parts.push(Arc::new(beacon)),
        Err(e) => warn!("Soundify Connect: no UDP beacon: {e}"),
    }
    if parts.is_empty() {
        return Err(std::io::Error::other("neither mDNS nor the UDP beacon could start"));
    }
    Ok(Arc::new(Composite::new(parts)))
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
    pairing.set_open(config.open);
    let identity = Identity { device_id: config.device_id.clone(), name: config.name.clone(), kind: device_kind().into() };
    let handle = app.clone();
    let connect = Connect::new(
        identity,
        discovery,
        pairing,
        known,
        Arc::new(AppPlayer { app: app.clone() }),
        Arc::new(AppListener { app: handle }),
    );
    let enabled = config.enabled;
    let error: Arc<Mutex<Option<String>>> = Arc::default();
    app.manage(ConnectState { connect: connect.clone(), config: Mutex::new(config), file, error: error.clone() });
    if enabled {
        let app = app.clone();
        std::thread::spawn(move || {
            use_phone_name(&app);
            if let Err(e) = turn_on(&app, &connect) {
                warn!("Soundify Connect could not start: {e}");
                *error.lock().unwrap() = Some(e.to_string());
                let _ = app.emit("connect-devices", ());
            }
        });
    }
}

/// An Android phone first called itself «Teléfono Android»: now it uses the name its owner gave it (or its model), unless
/// the user already chose one. Does nothing elsewhere.
fn use_phone_name(app: &AppHandle) {
    if !cfg!(target_os = "android") {
        return;
    }
    let state = app.state::<ConnectState>();
    if state.config.lock().unwrap().name != LEGACY_PHONE_NAME {
        return;
    }
    let Some(name) = crate::mobile::device_name(app).filter(|name| !name.trim().is_empty()) else { return };
    let mut config = state.config.lock().unwrap();
    config.name = name.clone();
    state.connect.set_name(&name);
    save_config(&state.file, &config);
}

const LEGACY_PHONE_NAME: &str = "Teléfono Android";

/// On Android mDNS needs a lock on the Wi-Fi radio, or the phone drops the multicast packets it lives on, and
/// the app has to stay awake (a foreground service with its notice) or Android closes its network in the background.
fn turn_on(app: &AppHandle, connect: &Arc<Connect>) -> std::io::Result<()> {
    let (title, text) = notice(app, &connect.identity().name);
    crate::mobile::set_multicast(app, true, &title, &text);
    connect.start(PREFERRED_PORT)
}

fn turn_off(app: &AppHandle, connect: &Connect) {
    connect.stop();
    crate::mobile::set_multicast(app, false, "", "");
}

/// What the notification that keeps Connect awake says, in the app's language.
fn notice(app: &AppHandle, name: &str) -> (String, String) {
    let language = settings::load(&LocalStorage, &app.state::<crate::commands::AppState>().paths.settings()).language;
    match language.as_str() {
        "en" => ("Soundify Connect is on".into(), format!("Visible on your network as \"{name}\"")),
        "ca" => ("Soundify Connect activat".into(), format!("Visible a la teva xarxa com a «{name}»")),
        _ => ("Soundify Connect activado".into(), format!("Visible en tu red como «{name}»")),
    }
}

fn load_config(file: &Location) -> Config {
    let storage = LocalStorage;
    if let Some(config) = storage.read(file).ok().and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok()) {
        return config;
    }
    // On from the start: it is what makes the other devices just show up. Off in Ajustes, one switch.
    let config = Config { device_id: new_device_id(), name: default_name(), enabled: true, open: true };
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
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    /// Pairs without a PIN.
    open: bool,
    /// Where this device can be reached on the network (`ip:port`), to type it on another one.
    addresses: Vec<String>,
    /// The other devices are found by themselves (false: only by address).
    discovery: bool,
    /// Why Connect could not start, if it could not.
    error: Option<String>,
    /// Android 16+: the permission to use the local network (None where it does not apply). Without it nothing on the Wi-Fi gets through.
    local_network: Option<bool>,
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
pub fn connect_status(app: AppHandle, state: State<'_, ConnectState>) -> Status {
    let config = state.config.lock().unwrap().clone();
    Status {
        enabled: config.enabled,
        open: config.open,
        addresses: state.connect.addresses(),
        discovery: state.connect.discovers(),
        error: state.error.lock().unwrap().clone(),
        local_network: crate::mobile::local_network_granted(&app),
        running: state.connect.is_running(),
        port: state.connect.port(),
        name: config.name,
        device_id: config.device_id,
        selected: state.connect.selected(),
        controllers: state.connect.controllers().into_iter().map(|t| ControllerView { id: t.controller_id, name: t.name, kind: t.kind }).collect(),
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
    *state.error.lock().unwrap() = result.as_ref().err().map(ToString::to_string);
    result.map_err(CommandError::io)?;
    let mut config = state.config.lock().unwrap();
    config.enabled = enabled;
    save_config(&state.file, &config);
    info!("Soundify Connect {}", if enabled { "on" } else { "off" });
    Ok(())
}

/// Asks Android for the permission to use the local network (the system's dialog, or the app's settings if it was refused for good).
#[tauri::command]
pub fn connect_request_local_network(app: AppHandle) {
    crate::mobile::request_local_network(&app);
}

/// Open (devices pair by themselves) or strict (the PIN is asked for).
#[tauri::command]
pub fn connect_set_open(state: State<'_, ConnectState>, open: bool) {
    state.connect.set_open(open);
    let mut config = state.config.lock().unwrap();
    config.open = open;
    save_config(&state.file, &config);
}

/// The name the others see. Applied the next time Connect is turned on.
#[tauri::command]
pub fn connect_set_name(state: State<'_, ConnectState>, name: String) -> Result<(), CommandError> {
    let name = name.trim().chars().take(60).collect::<String>();
    if name.is_empty() {
        return Err(CommandError::key("connect.error.name"));
    }
    let mut config = state.config.lock().unwrap();
    state.connect.set_name(&name);
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

/// Starts controlling a device; what it is doing right now comes back, to show it at once.
#[tauri::command]
pub async fn connect_select(app: AppHandle, state: State<'_, ConnectState>, device_id: String) -> Result<Option<PlayerState>, CommandError> {
    let connect = state.connect.clone();
    let shown = (app.clone(), device_id.clone());
    let current = tauri::async_runtime::spawn_blocking(move || connect.select(&device_id).map(|()| connect.state_of(&device_id)))
        .await
        .map_err(CommandError::io)?
        .map_err(error)?;
    show_on_phone(&shown.0, &shown.1, current.as_ref());
    Ok(current)
}

#[tauri::command]
pub fn connect_deselect(app: AppHandle, state: State<'_, ConnectState>) {
    state.connect.deselect();
    show_on_phone(&app, "", None);
}

/// An order for the device being controlled.
#[tauri::command]
pub fn connect_send(state: State<'_, ConnectState>, command: Command) -> Result<(), CommandError> {
    state.connect.send(command).map(|_| ()).map_err(error)
}

/// The playlist a device said it is playing (the one being controlled: it is its remote playlist).
#[tauri::command]
pub fn connect_queue_of(state: State<'_, ConnectState>, device_id: String) -> Option<QueueInfo> {
    state.connect.queue_of(&device_id)
}

/// The playlist this device is playing, for those who control it.
#[tauri::command]
pub fn connect_publish_queue(state: State<'_, ConnectState>, queue: Option<QueueInfo>) {
    state.connect.publish_queue(queue);
}

/// What this device plays now, for those who control it.
#[tauri::command]
pub fn connect_publish(state: State<'_, ConnectState>, player: PlayerState) {
    state.connect.publish(player);
}
