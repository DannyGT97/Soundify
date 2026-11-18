//! Soundify Connect in the app (the logic is in `core::connect`): what the front can
//! ask, and the two ends the core needs — how to do what a controller asks (hand it to
//! the page, which owns the queue) and where to tell the screen what happens.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use soundify_core::Location;
use soundify_core::connect::beacon::{Beacon, Composite};
use soundify_core::connect::cast::{CastSource, Inbox, LocalRelay, SharedFile};
use soundify_core::connect::protocol::{CastItem, CastStart};
use soundify_core::connect::tls::TlsIdentity;
use soundify_core::connect::discovery::{Discovery, Mdns};
use soundify_core::connect::stats::short;
use soundify_core::connect::manager::{Connect, ConnectError, DeviceView, Identity, Listener, PREFERRED_PORT, Player};
use soundify_core::connect::pairing::{KnownDevices, Pairing, Trusted, new_device_id};
use soundify_core::connect::protocol::{Command, LibraryEntry, PlayerState, QueueInfo};
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
    /// Any device on the network may pair without a PIN (like Spotify: they just show up). Off (the default since TLS) =
    /// a PIN is asked for.
    #[serde(default)]
    open: bool,
    /// The settings were brought to the TLS version once: the PIN is asked for again, whatever 1.2.x had (open was its
    /// default, and with it anybody on the network could control this device).
    #[serde(default)]
    secure: bool,
}

pub struct ConnectState {
    connect: Arc<Connect>,
    config: Mutex<Config>,
    file: Location,
    /// Why it could not start, for the window to say.
    error: Arc<Mutex<Option<String>>>,
}

impl ConnectState {
    /// For the diagnostic: is it on, who it is, who it sees and who is connected. Short ids, no addresses beyond the local network.
    pub fn summary(&self) -> String {
        let config = self.config.lock().unwrap().clone();
        let mut lines = vec![format!(
            "enabled={} running={} open={} name={} id={} port={}",
            config.enabled, self.connect.is_running(), config.open, config.name, short(&config.device_id), self.connect.port().map_or("-".to_string(), |p| p.to_string())
        )];
        if let Some(error) = self.error.lock().unwrap().as_ref() {
            lines.push(format!("error: {error}"));
        }
        lines.push(format!("controllers now: {}", self.connect.controllers().len()));
        for device in self.connect.devices() {
            lines.push(format!(
                "- {} #{} {} online={} ready={} controlled={} playing={} paired={}",
                device.name, short(&device.id), device.kind, device.online, device.ready, device.connected, device.playing, device.paired
            ));
        }
        lines.join("
")
    }
}

/// What a controller asks goes to the page: it owns the queue.
struct AppPlayer {
    app: AppHandle,
    /// «Escuchar en»: where what another device passes is read from.
    inbox: Arc<Inbox>,
    /// Android only, started with the first pass: what Media3 reads those files through.
    local: Mutex<Option<LocalRelay>>,
}

impl AppPlayer {
    /// Where the native player of Android reads a pass from (None elsewhere, or if it could not start: then the webview).
    fn cast_base(&self) -> Option<String> {
        if !cfg!(target_os = "android") {
            return None;
        }
        let mut local = self.local.lock().unwrap();
        if local.is_none() {
            match LocalRelay::start(self.inbox.clone()) {
                Ok(relay) => *local = Some(relay),
                Err(e) => warn!("Soundify Connect: no local relay for the native player ({e}); the pass sounds through the webview"),
            }
        }
        local.as_ref().map(LocalRelay::base)
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IncomingCommand<'a> {
    from: &'a str,
    command: &'a Command,
    /// A pass on Android: where the native player reads its files (`LocalRelay`, on 127.0.0.1 with a secret).
    #[serde(skip_serializing_if = "Option::is_none")]
    cast_base: Option<String>,
}

impl Player for AppPlayer {
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String> {
        let mut cast_base = None;
        let command = match command {
            // The page gets what to show and play, not where it is read from: the ticket stays here (`cast.rs` adds it).
            Command::Cast(mut cast) => {
                cast_base = self.cast_base();
                self.inbox.set(CastSource { from: who.controller_id.clone(), host: cast.host.clone(), port: cast.port, fingerprint: cast.fingerprint.clone(), ticket: cast.ticket.clone() });
                info!("Soundify Connect: {} #{} passes «{}» here ({} files)", who.name, short(&who.controller_id), cast.playlist, cast.items.len());
                cast.ticket.clear();
                cast.host.clear();
                cast.fingerprint.clear();
                if cast_base.is_some()
                    && let Some(memory) = self.app.try_state::<crate::cast::CastMemory>()
                {
                    *memory.0.lock().unwrap() = Some(crate::cast::CastSnapshot { from: who.name.clone(), pass: (*cast).clone(), cast_base: cast_base.clone() });
                }
                Command::Cast(cast)
            }
            // An older pass of another device does not end the one that plays now.
            Command::CastEnd { reason } => {
                if !self.inbox.end_from(&who.controller_id) {
                    return Ok(());
                }
                if let Some(memory) = self.app.try_state::<crate::cast::CastMemory>() {
                    *memory.0.lock().unwrap() = None;
                }
                Command::CastEnd { reason }
            }
            other => other,
        };
        self.app.emit("connect-command", IncomingCommand { from: &who.name, command: &command, cast_base }).map_err(|e| e.to_string())
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
struct RemoteLibrary<'a> {
    device_id: &'a str,
    playlists: &'a [LibraryEntry],
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RemoteBrowse<'a> {
    device_id: &'a str,
    key: &'a str,
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

    fn remote_library(&self, device_id: &str, playlists: Vec<LibraryEntry>) {
        let _ = self.app.emit("connect-remote-library", RemoteLibrary { device_id, playlists: &playlists });
    }

    fn remote_browse(&self, device_id: &str, key: &str, queue: Option<QueueInfo>) {
        let _ = self.app.emit("connect-remote-browse", RemoteBrowse { device_id, key, queue: &queue });
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
    let known = KnownDevices::load(storage.clone(), Location::path(dir.join("devices.json")));
    pairing.set_open(config.open);
    // This device's certificate, made once (its fingerprint is what the others pin). The key never leaves this folder.
    let tls = match TlsIdentity::load_or_create(storage.as_ref(), &Location::path(dir.join("tls-cert.der")), &Location::path(dir.join("tls-key.der"))) {
        Ok(tls) => tls,
        Err(e) => {
            warn!("Soundify Connect: could not keep its certificate ({e}); a temporary one is used");
            TlsIdentity::ephemeral()
        }
    };
    let identity = Identity { device_id: config.device_id.clone(), name: config.name.clone(), kind: device_kind().into() };
    let handle = app.clone();
    let connect = Connect::new(
        identity,
        discovery,
        pairing,
        known,
        Arc::new(tls),
        Arc::new(AppPlayer { app: app.clone(), inbox: app.state::<crate::cast::CastInbox>().0.clone(), local: Mutex::new(None) }),
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
    if let Some(mut config) = storage.read(file).ok().and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok()) {
        if !config.secure {
            config.open = false;
            config.secure = true;
            save_config(file, &config);
        }
        return config;
    }
    // On from the start (devices are found by themselves), but pairing asks for the PIN: open is a choice in Ajustes.
    let config = Config { device_id: new_device_id(), name: default_name(), enabled: true, open: false, secure: true };
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
        ConnectError::Untrusted => CommandError::key("connect.error.untrusted"),
        ConnectError::Refused { status, error, seconds } => match (status, error.as_str()) {
            (403, _) => CommandError::key("connect.error.wrongPin"),
            (410, _) => CommandError::key("connect.error.noPin"),
            (429, _) => CommandError::with_args("connect.error.locked", vec![seconds.unwrap_or(60).to_string()]),
            (401, _) => CommandError::key("connect.error.unauthorized"),
            (426, "newer") => CommandError::key("connect.error.newer"),
            (426, _) => CommandError::key("connect.error.outdated"),
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
    /// Seconds since 1970 when it last connected.
    last_seen: Option<u64>,
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
    /// The fingerprint of this device's certificate (to compare by eye with what another device shows).
    fingerprint: String,
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
        fingerprint: state.connect.fingerprint(),
        selected: state.connect.selected(),
        controllers: state.connect.controllers().into_iter().map(|t| ControllerView { id: t.controller_id, name: t.name, kind: t.kind, last_seen: t.last_seen }).collect(),
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
    // A pass and its end only go through `connect_cast` / `connect_cast_end` (which grant and revoke what may be read).
    if matches!(command, Command::Cast(_) | Command::CastEnd { .. } | Command::Take) {
        return Err(CommandError::with_args("connect.error.other", vec!["not a button".into()]));
    }
    state.connect.send(command).map(|_| ()).map_err(error)
}

/// One file of what the page passes: what the other device shows, and where it is HERE (never sent: it is read by id).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastFile {
    video_id: String,
    title: String,
    #[serde(default)]
    duration: f64,
    format: String,
    #[serde(default)]
    video: bool,
    file: String,
    #[serde(default)]
    cover: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastRequest {
    device_id: String,
    playlist: String,
    items: Vec<CastFile>,
    index: usize,
    position: f64,
    playing: bool,
    shuffle: bool,
    repeat: String,
}

/// Only a file that exists and is music or video may be shared; a picture only if it is one. The page asks, but what is
/// opened to the network is decided here.
fn shareable(path: &str, kinds: &[&str]) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(path);
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    (kinds.contains(&extension.as_str()) && path.is_file()).then_some(path)
}

const MEDIA: &[&str] = &["mp3", "m4a", "aac", "opus", "ogg", "oga", "flac", "wav", "mp4", "m4v", "webm", "mkv", "mov"];
const PICTURES: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// «Escuchar en»: passes what plays here to `deviceId`. It reads the files from here (only these, while it lasts).
#[tauri::command(async)]
pub fn connect_cast(app: AppHandle, state: State<'_, ConnectState>, request: CastRequest) -> Result<(), CommandError> {
    let sounding = request.items.get(request.index).map(|item| item.video_id.clone());
    let mut items = Vec::new();
    let mut files = Vec::new();
    for item in request.items {
        let Some(media) = shareable(&item.file, MEDIA) else { continue };
        let cover = item.cover.as_deref().and_then(|c| shareable(c, PICTURES));
        items.push(CastItem { video_id: item.video_id.clone(), title: item.title, duration: item.duration, format: item.format, video: item.video, cover: cover.is_some() });
        files.push((item.video_id, SharedFile { media, cover }));
    }
    // What sounds must still be there (and in the list): otherwise it is not passed at all.
    let Some(index) = items.iter().position(|item| Some(&item.video_id) == sounding.as_ref()) else {
        return Err(CommandError::key("connect.error.nothingToPass"));
    };
    let pass = CastStart {
        host: String::new(),
        port: 0,
        fingerprint: String::new(),
        ticket: String::new(),
        playlist: request.playlist,
        items,
        index,
        position: request.position,
        playing: request.playing,
        shuffle: request.shuffle,
        repeat: request.repeat,
    };
    state.connect.cast_to(&request.device_id, pass, files).map_err(error)?;
    crate::mobile::set_casting(&app, true);
    let current = state.connect.state_of(&request.device_id);
    show_on_phone(&app, &request.device_id, current.as_ref());
    Ok(())
}

/// What a pass that sounds here needs to be shown again after a reload of the page (None: nothing passed to this device).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastCurrent {
    #[serde(flatten)]
    snapshot: crate::cast::CastSnapshot,
    state: PlayerState,
}

#[tauri::command]
pub fn connect_cast_current(state: State<'_, ConnectState>, memory: State<'_, crate::cast::CastMemory>) -> Option<CastCurrent> {
    let snapshot = memory.0.lock().unwrap().clone()?;
    Some(CastCurrent { snapshot, state: state.connect.own_state() })
}

/// The pass ends: nothing more can be read from here, and the other device stops.
#[tauri::command]
pub fn connect_cast_end(app: AppHandle, state: State<'_, ConnectState>, reason: String) {
    state.connect.cast_end(&reason);
    crate::mobile::set_casting(&app, false);
}

/// The playlist a device said it is playing (the one being controlled: it is its remote playlist).
#[tauri::command]
pub fn connect_queue_of(state: State<'_, ConnectState>, device_id: String) -> Option<QueueInfo> {
    state.connect.queue_of(&device_id)
}

/// The playlists a device said its library has (the one being controlled: what to pick from before anything plays).
#[tauri::command]
pub fn connect_library_of(state: State<'_, ConnectState>, device_id: String) -> Vec<LibraryEntry> {
    state.connect.library_of(&device_id)
}

/// This device's own library (playlists without files), for those who control it.
#[tauri::command]
pub fn connect_publish_library(state: State<'_, ConnectState>, playlists: Vec<LibraryEntry>) {
    state.connect.publish_library(playlists);
}

/// The files of a playlist of this device's library that a controller asked for.
#[tauri::command]
pub fn connect_publish_browse(state: State<'_, ConnectState>, key: String, queue: Option<QueueInfo>) {
    state.connect.publish_browse(&key, queue);
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
