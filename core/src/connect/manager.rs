//! Soundify Connect as the app uses it: one object that is both ends. As a player it
//! runs the server, announces itself and shows a PIN; as a controller it sees the
//! others, pairs with them and drives one at a time. The app gives it two things —
//! how to do what a controller asks ([`Player`]) and where to tell the screen what
//! happens ([`Listener`]).

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use log::{info, warn};
use serde::Serialize;

use super::client::{self, ClientError, Event, Session};
use super::discovery::{Change, Discovery};
use super::pairing::{Known, KnownDevices, Pairing, Trusted};
use super::protocol::{Command, DeviceInfo, PROTOCOL, PlayerState, ServerMessage};
use super::server::{Host, Server};

/// The port tried first, so a firewall rule or a bookmark keeps working; any free one if it is taken.
pub const PREFERRED_PORT: u16 = 48650;

/// What the app does when a controller asks for something. It must not block.
pub trait Player: Send + Sync {
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String>;
}

/// What the screen is told.
pub trait Listener: Send + Sync {
    /// The list of devices (or who is connected) changed.
    fn devices_changed(&self);
    /// The device being controlled says what it is doing.
    fn remote_state(&self, device_id: &str, state: PlayerState);
    /// The connection to the device being controlled is over without the user asking (why, if known).
    fn remote_closed(&self, device_id: &str, reason: Option<String>);
}

/// This device, as the others will see it.
#[derive(Debug, Clone)]
pub struct Identity {
    pub device_id: String,
    pub name: String,
    /// `desktop`, `phone` or `tablet`.
    pub kind: String,
}

/// One line of the device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// `ip:port` where it was last seen.
    pub address: Option<String>,
    /// This device holds a token for it.
    pub paired: bool,
    /// It answers on the network now.
    pub online: bool,
    /// It is the one being controlled.
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The device is not on the network (or not known).
    Offline,
    /// Not paired with it yet.
    NotPaired,
    /// It refused: wrong PIN, locked, no PIN shown (the HTTP status and `error` it gave), or a token it no longer accepts.
    Refused { status: u16, error: String, seconds: Option<u64> },
    /// Nothing is being controlled.
    NoSession,
    Other(String),
}

impl From<ClientError> for ConnectError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Unreachable(_) => ConnectError::Offline,
            ClientError::Refused { status, error, seconds } => ConnectError::Refused { status, error, seconds },
            ClientError::Protocol(e) => ConnectError::Other(e),
        }
    }
}

struct Nearby {
    info: DeviceInfo,
    addr: SocketAddr,
    /// Typed in by hand: mDNS does not make it disappear.
    manual: bool,
}

struct Active {
    device_id: String,
    generation: u64,
    session: Session,
}

/// What the server asks the app: its own description and the last state published.
struct HostAdapter {
    info: DeviceInfo,
    state: Arc<Mutex<PlayerState>>,
    player: Arc<dyn Player>,
}

impl Host for HostAdapter {
    fn info(&self) -> DeviceInfo {
        self.info.clone()
    }

    fn state(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    fn command(&self, who: &Trusted, command: Command) -> Result<(), String> {
        self.player.command(who, command)
    }
}

pub struct Connect {
    identity: Mutex<Identity>,
    factory: Box<dyn Fn() -> io::Result<Arc<dyn Discovery>> + Send + Sync>,
    pairing: Arc<Pairing>,
    known: KnownDevices,
    player: Arc<dyn Player>,
    listener: Arc<dyn Listener>,
    state: Arc<Mutex<PlayerState>>,
    running: Mutex<Option<Running>>,
    nearby: Mutex<HashMap<String, Nearby>>,
    active: Mutex<Option<Active>>,
    generation: AtomicU64,
}

struct Running {
    server: Server,
    discovery: Arc<dyn Discovery>,
}

impl Connect {
    /// `factory` makes the discovery each time Connect is turned on (the real one cannot be restarted once stopped).
    pub fn new(
        identity: Identity,
        factory: impl Fn() -> io::Result<Arc<dyn Discovery>> + Send + Sync + 'static,
        pairing: Arc<Pairing>,
        known: KnownDevices,
        player: Arc<dyn Player>,
        listener: Arc<dyn Listener>,
    ) -> Arc<Connect> {
        Arc::new(Connect {
            identity: Mutex::new(identity),
            factory: Box::new(factory),
            pairing,
            known,
            player,
            listener,
            state: Arc::new(Mutex::new(PlayerState::default())),
            running: Mutex::new(None),
            nearby: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            generation: AtomicU64::new(0),
        })
    }

    pub fn identity(&self) -> Identity {
        self.identity.lock().unwrap().clone()
    }

    fn info(&self) -> DeviceInfo {
        let identity = self.identity();
        DeviceInfo { v: PROTOCOL, device_id: identity.device_id, name: identity.name, kind: identity.kind }
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// The port the server listens on, while it runs.
    pub fn port(&self) -> Option<u16> {
        self.running.lock().unwrap().as_ref().map(|running| running.server.port())
    }

    /// Turns it on: listens, announces itself and starts looking for the others.
    pub fn start(self: &Arc<Self>, preferred_port: u16) -> io::Result<()> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        let host = Arc::new(HostAdapter { info: self.info(), state: self.state.clone(), player: self.player.clone() });
        let server = match Server::start(preferred_port, host.clone(), self.pairing.clone()) {
            Ok(server) => server,
            // The preferred port is taken (another copy of the app): any free one.
            Err(_) if preferred_port != 0 => Server::start(0, host, self.pairing.clone())?,
            Err(e) => return Err(e),
        };
        let discovery = (self.factory)()?;
        discovery.announce(&self.info(), server.port())?;
        let me = Arc::downgrade(self);
        discovery.browse(Box::new(move |change| {
            if let Some(connect) = me.upgrade() {
                connect.saw(change);
            }
        }))?;
        info!("Soundify Connect on, port {}", server.port());
        *running = Some(Running { server, discovery });
        Ok(())
    }

    /// Turns it off: lets go of the device being controlled, stops the server and goes quiet.
    pub fn stop(&self) {
        self.deselect();
        let running = self.running.lock().unwrap().take();
        if let Some(running) = running {
            running.discovery.shutdown();
            running.server.stop();
        }
        self.nearby.lock().unwrap().clear();
        self.listener.devices_changed();
    }

    /// A device appeared on or disappeared from the network.
    fn saw(&self, change: Change) {
        let own = self.identity().device_id;
        match change {
            Change::Up(found) => {
                if found.info.device_id == own || found.info.v != PROTOCOL {
                    return;
                }
                let Some(ip) = found.addrs.iter().find(|ip| matches!(ip, IpAddr::V4(_))).or(found.addrs.first()) else { return };
                let addr = SocketAddr::new(*ip, found.port);
                self.nearby.lock().unwrap().insert(found.info.device_id.clone(), Nearby { info: found.info, addr, manual: false });
            }
            Change::Down(id) => {
                let mut nearby = self.nearby.lock().unwrap();
                if nearby.get(&id).is_some_and(|n| n.manual) {
                    return;
                }
                nearby.remove(&id);
            }
        }
        self.listener.devices_changed();
    }

    /// For a network where mDNS does not get through: the address of the other device, as it shows it.
    pub fn add_by_address(&self, addr: SocketAddr) -> Result<DeviceInfo, ConnectError> {
        let info = client::fetch_info(addr)?;
        if info.device_id == self.identity().device_id {
            return Err(ConnectError::Other("that is this device".into()));
        }
        self.nearby.lock().unwrap().insert(info.device_id.clone(), Nearby { info: info.clone(), addr, manual: true });
        self.listener.devices_changed();
        Ok(info)
    }

    /// Everything the user can pick: what is on the network and what it has paired with before.
    pub fn devices(&self) -> Vec<DeviceView> {
        let connected = self.active.lock().unwrap().as_ref().map(|a| a.device_id.clone());
        let nearby = self.nearby.lock().unwrap();
        let mut views: Vec<DeviceView> = nearby
            .values()
            .map(|n| DeviceView {
                id: n.info.device_id.clone(),
                name: n.info.name.clone(),
                kind: n.info.kind.clone(),
                address: Some(n.addr.to_string()),
                paired: self.known.get(&n.info.device_id).is_some(),
                online: true,
                connected: connected.as_deref() == Some(n.info.device_id.as_str()),
            })
            .collect();
        for known in self.known.all() {
            if !nearby.contains_key(&known.device_id) {
                views.push(DeviceView {
                    connected: connected.as_deref() == Some(known.device_id.as_str()),
                    id: known.device_id,
                    name: known.name,
                    kind: known.kind,
                    address: None,
                    paired: true,
                    online: false,
                });
            }
        }
        views.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.online.cmp(&a.online)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        views
    }

    /// Trades the PIN the other device shows for its token.
    pub fn pair(&self, device_id: &str, pin: &str) -> Result<(), ConnectError> {
        let (info, addr) = {
            let nearby = self.nearby.lock().unwrap();
            let device = nearby.get(device_id).ok_or(ConnectError::Offline)?;
            (device.info.clone(), device.addr)
        };
        let me = self.identity();
        let token = client::pair(addr, &me.device_id, &me.name, pin)?;
        self.known.remember(Known { device_id: info.device_id, name: info.name, kind: info.kind, token }).map_err(|e| ConnectError::Other(e.to_string()))?;
        self.listener.devices_changed();
        Ok(())
    }

    /// Takes the pairing away: this device forgets the other's token, and stops controlling it.
    pub fn forget(&self, device_id: &str) -> Result<(), ConnectError> {
        if self.active.lock().unwrap().as_ref().is_some_and(|a| a.device_id == device_id) {
            self.deselect();
        }
        self.known.forget(device_id).map_err(|e| ConnectError::Other(e.to_string()))?;
        self.listener.devices_changed();
        Ok(())
    }

    /// Starts controlling a paired device (the one being controlled until then is let go of).
    pub fn select(self: &Arc<Self>, device_id: &str) -> Result<(), ConnectError> {
        let known = self.known.get(device_id).ok_or(ConnectError::NotPaired)?;
        let addr = self.nearby.lock().unwrap().get(device_id).map(|n| n.addr).ok_or(ConnectError::Offline)?;
        self.deselect();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let me = Arc::downgrade(self);
        let id = device_id.to_string();
        let session = Session::open(addr, &known.token, move |event| {
            let Some(connect) = me.upgrade() else { return };
            match event {
                Event::Message(ServerMessage::State { state }) => connect.listener.remote_state(&id, state),
                Event::Message(_) => {}
                Event::Closed(reason) => connect.closed(&id, generation, reason),
            }
        });
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                // A token the device no longer accepts is of no use: forgotten, so the user pairs again.
                if matches!(error, ClientError::Refused { status: 401, .. }) {
                    let _ = self.known.forget(device_id);
                    self.listener.devices_changed();
                }
                return Err(error.into());
            }
        };
        *self.active.lock().unwrap() = Some(Active { device_id: device_id.to_string(), generation, session });
        self.listener.devices_changed();
        Ok(())
    }

    /// The session ended by itself (the other device went away, the network dropped).
    fn closed(&self, device_id: &str, generation: u64, reason: Option<String>) {
        let ended = {
            let mut active = self.active.lock().unwrap();
            if active.as_ref().is_some_and(|a| a.device_id == device_id && a.generation == generation) { active.take() } else { None }
        };
        // Dropped outside the lock, and `Session::close` knows not to wait for the thread it is called from.
        if ended.is_some() {
            drop(ended);
            self.listener.remote_closed(device_id, reason);
            self.listener.devices_changed();
        }
    }

    /// Back to controlling nothing.
    pub fn deselect(&self) {
        let ended = self.active.lock().unwrap().take();
        if let Some(active) = ended {
            active.session.close();
            self.listener.devices_changed();
        }
    }

    /// The device being controlled, if any.
    pub fn selected(&self) -> Option<String> {
        self.active.lock().unwrap().as_ref().map(|a| a.device_id.clone())
    }

    /// Asks the device being controlled to do something.
    pub fn send(&self, command: Command) -> Result<u64, ConnectError> {
        command.validate().map_err(ConnectError::Other)?;
        let active = self.active.lock().unwrap();
        let active = active.as_ref().ok_or(ConnectError::NoSession)?;
        Ok(active.session.send(command))
    }

    /// What this device is playing now: kept for whoever connects, and sent to those who are.
    pub fn publish(&self, state: PlayerState) {
        *self.state.lock().unwrap() = state.clone();
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.publish(&state);
        }
    }

    /// The PIN to show on this device's screen (a new one if there is none or it expired).
    pub fn pin(&self) -> Option<String> {
        if !self.is_running() {
            return None;
        }
        let now = Instant::now();
        Some(self.pairing.current_pin(now).unwrap_or_else(|| self.pairing.new_pin(now)))
    }

    /// A fresh PIN, even if there was one.
    pub fn renew_pin(&self) -> Option<String> {
        self.is_running().then(|| self.pairing.new_pin(Instant::now()))
    }

    /// Who may control this device.
    pub fn controllers(&self) -> Vec<Trusted> {
        self.pairing.trusted()
    }

    /// Controllers connected at this moment.
    pub fn connected_controllers(&self) -> usize {
        self.running.lock().unwrap().as_ref().map_or(0, |running| running.server.controllers())
    }

    pub fn forget_controller(&self, controller_id: &str) -> Result<(), ConnectError> {
        self.pairing.forget(controller_id).map_err(|e| {
            warn!("Could not forget a controller: {e}");
            ConnectError::Other(e.to_string())
        })?;
        self.listener.devices_changed();
        Ok(())
    }
}
