//! Soundify Connect as the app uses it: one object that is both ends. As a player it
//! runs the server and announces itself; as a controller it sees the others, pairs with
//! them by itself when they are open (no PIN: like Spotify, a device just shows up) and
//! keeps a WebSocket to each one, so it always knows what every device is playing and
//! one tap is enough to control it. The app gives it two things — how to do what a
//! controller asks ([`Player`]) and where to tell the screen what happens ([`Listener`]).

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::{info, warn};
use serde::Serialize;

use super::client::{self, ClientError, Event, Session};
use super::discovery::{Change, Discovery};
use super::pairing::{Known, KnownDevices, Pairing, Trusted};
use super::protocol::{Command, DeviceInfo, PROTOCOL, PlayerState, ServerMessage};
use super::server::{Host, Server};

/// The port tried first, so a firewall rule or a bookmark keeps working; any free one if it is taken.
pub const PREFERRED_PORT: u16 = 48650;

/// Waits (seconds) between the tries to get a dropped connection back, while the device is still on the network.
const RECONNECT_DELAYS: [u64; 5] = [2, 4, 8, 16, 32];

/// What the app does when a controller asks for something. It must not block.
pub trait Player: Send + Sync {
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String>;
}

/// What the screen is told.
pub trait Listener: Send + Sync {
    /// The list of devices (or what one of them plays, or who is connected) changed.
    fn devices_changed(&self);
    /// A device says what it is doing.
    fn remote_state(&self, device_id: &str, state: PlayerState);
    /// The connection to the device being controlled is over without the user asking.
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

/// What a device is playing, for the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingView {
    pub title: String,
    pub playlist: String,
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
    /// It can be controlled this instant: paired, and the connection to it is open.
    pub ready: bool,
    /// It is the one being controlled.
    pub connected: bool,
    /// It only pairs with a PIN (it is strict): the PIN it shows has to be typed here.
    pub needs_pin: bool,
    /// It is playing right now.
    pub playing: bool,
    pub now_playing: Option<PlayingView>,
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
    /// What it last said it is doing.
    state: Option<PlayerState>,
    /// It is strict: pairing needs the PIN it shows.
    needs_pin: bool,
}

/// The open connection to a paired device (its `generation` tells an old one's end from the current one's).
struct Watch {
    generation: u64,
    session: Session,
}

/// What the server asks the app: its own description and the last state published.
struct HostAdapter {
    info: DeviceInfo,
    pairing: Arc<Pairing>,
    state: Arc<Mutex<PlayerState>>,
    player: Arc<dyn Player>,
}

impl Host for HostAdapter {
    fn info(&self) -> DeviceInfo {
        DeviceInfo { open: self.pairing.is_open(), ..self.info.clone() }
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
    /// One per paired device that is on the network: how its state is known and how it is controlled.
    sessions: Mutex<HashMap<String, Watch>>,
    selected: Mutex<Option<String>>,
    generation: AtomicU64,
    /// Devices a connection is being made to right now (so two events do not make two); `true` = asked again meanwhile.
    busy: Mutex<HashMap<String, bool>>,
    /// Devices the user forgot: not paired again by themselves until it is asked for.
    forgotten: Mutex<HashSet<String>>,
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
            sessions: Mutex::new(HashMap::new()),
            selected: Mutex::new(None),
            generation: AtomicU64::new(0),
            busy: Mutex::new(HashMap::new()),
            forgotten: Mutex::new(HashSet::new()),
        })
    }

    pub fn identity(&self) -> Identity {
        self.identity.lock().unwrap().clone()
    }

    fn info(&self) -> DeviceInfo {
        let identity = self.identity();
        DeviceInfo { v: PROTOCOL, device_id: identity.device_id, name: identity.name, kind: identity.kind, open: self.pairing.is_open() }
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// The port the server listens on, while it runs.
    pub fn port(&self) -> Option<u16> {
        self.running.lock().unwrap().as_ref().map(|running| running.server.port())
    }

    /// Open (any device pairs by itself) or strict (the PIN is asked for).
    pub fn set_open(&self, open: bool) {
        self.pairing.set_open(open);
        self.listener.devices_changed();
    }

    pub fn is_open(&self) -> bool {
        self.pairing.is_open()
    }

    /// Turns it on: listens, announces itself and starts looking for the others.
    pub fn start(self: &Arc<Self>, preferred_port: u16) -> io::Result<()> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        let host = Arc::new(HostAdapter { info: self.info(), pairing: self.pairing.clone(), state: self.state.clone(), player: self.player.clone() });
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

    /// Turns it off: lets go of every device, stops the server and goes quiet.
    pub fn stop(&self) {
        let running = self.running.lock().unwrap().take();
        *self.selected.lock().unwrap() = None;
        let sessions: Vec<Watch> = self.sessions.lock().unwrap().drain().map(|(_, watch)| watch).collect();
        for watch in sessions {
            watch.session.close();
        }
        if let Some(running) = running {
            running.discovery.shutdown();
            running.server.stop();
        }
        self.nearby.lock().unwrap().clear();
        self.listener.devices_changed();
    }

    /// A device appeared on or disappeared from the network.
    fn saw(self: &Arc<Self>, change: Change) {
        let own = self.identity().device_id;
        match change {
            Change::Up(found) => {
                if found.info.device_id == own || found.info.v != PROTOCOL {
                    return;
                }
                let Some(ip) = found.addrs.iter().find(|ip| matches!(ip, IpAddr::V4(_))).or(found.addrs.first()) else { return };
                let addr = SocketAddr::new(*ip, found.port);
                let id = found.info.device_id.clone();
                {
                    let mut nearby = self.nearby.lock().unwrap();
                    let (state, needs_pin, manual) = nearby.get(&id).map(|n| (n.state.clone(), n.needs_pin, n.manual)).unwrap_or((None, false, false));
                    nearby.insert(id.clone(), Nearby { info: found.info, addr, manual, state, needs_pin });
                }
                self.listener.devices_changed();
                self.connect_to(&id);
            }
            Change::Down(id) => {
                {
                    let mut nearby = self.nearby.lock().unwrap();
                    if nearby.get(&id).is_some_and(|n| n.manual) {
                        return;
                    }
                    nearby.remove(&id);
                }
                self.drop_session(&id);
                self.listener.devices_changed();
            }
        }
    }

    /// Lets go of the connection to a device that is gone, telling the screen if it was the one being controlled.
    fn drop_session(&self, id: &str) {
        let watch = self.sessions.lock().unwrap().remove(id);
        let was_selected = {
            let mut selected = self.selected.lock().unwrap();
            let was = selected.as_deref() == Some(id);
            if was {
                *selected = None;
            }
            was
        };
        if let Some(watch) = watch {
            watch.session.close();
        }
        if was_selected {
            self.listener.remote_closed(id, None);
        }
    }

    /// For a network where mDNS does not get through: the address of the other device, as it shows it.
    pub fn add_by_address(self: &Arc<Self>, addr: SocketAddr) -> Result<DeviceInfo, ConnectError> {
        let info = client::fetch_info(addr)?;
        if info.device_id == self.identity().device_id {
            return Err(ConnectError::Other("that is this device".into()));
        }
        self.forgotten.lock().unwrap().remove(&info.device_id);
        self.nearby.lock().unwrap().insert(info.device_id.clone(), Nearby { info: info.clone(), addr, manual: true, state: None, needs_pin: !info.open });
        self.listener.devices_changed();
        self.connect_to(&info.device_id);
        Ok(info)
    }

    // === CONNECTING TO THE OTHERS ===

    /// In the background: pairs (if the device is open and not paired yet) and opens the connection to it.
    fn connect_to(self: &Arc<Self>, id: &str) {
        if self.sessions.lock().unwrap().contains_key(id) {
            return;
        }
        {
            let mut busy = self.busy.lock().unwrap();
            // Already being tried: but what it found out may be out of date (a PIN was just typed), so once more after.
            if let Some(again) = busy.get_mut(id) {
                *again = true;
                return;
            }
            busy.insert(id.to_string(), false);
        }
        let (me, id) = (self.clone(), id.to_string());
        let spawned = thread::Builder::new().name("connect-attach".into()).spawn(move || {
            loop {
                if !me.sessions.lock().unwrap().contains_key(&id) {
                    me.attach(&id, true);
                }
                let mut busy = me.busy.lock().unwrap();
                if busy.get(&id).copied().unwrap_or(false) {
                    busy.insert(id.clone(), false);
                    continue;
                }
                busy.remove(&id);
                break;
            }
            me.listener.devices_changed();
        });
        if spawned.is_err() {
            warn!("Could not start a thread to connect to a device");
        }
    }

    /// Pairs if needed and opens the session. `retry`: a token the device refused is forgotten and pairing is tried once more.
    fn attach(self: &Arc<Self>, id: &str, retry: bool) {
        let Some((addr, name)) = self.nearby.lock().unwrap().get(id).map(|n| (n.addr, n.info.name.clone())) else { return };
        let token = match self.known.get(id) {
            Some(known) => known.token,
            None => match self.pair_by_itself(id, addr) {
                Some(token) => token,
                None => return,
            },
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (me, device) = (Arc::downgrade(self), id.to_string());
        let opened = Session::open(addr, &token, move |event| {
            let Some(connect) = me.upgrade() else { return };
            match event {
                Event::Message(ServerMessage::State { state }) => connect.got_state(&device, state),
                Event::Message(_) => {}
                Event::Closed(reason) => connect.session_closed(&device, generation, reason),
            }
        });
        match opened {
            Ok(session) => {
                let old = self.sessions.lock().unwrap().insert(id.to_string(), Watch { generation, session });
                if let Some(old) = old {
                    old.session.close();
                }
                info!("Soundify Connect: connected to {name}");
            }
            // A token the device no longer accepts is of no use: forgotten, and tried again as a new device.
            Err(ClientError::Refused { status: 401, .. }) => {
                let _ = self.known.forget(id);
                if retry {
                    self.attach(id, false);
                }
            }
            Err(e) => info!("Soundify Connect: could not connect to {name}: {e}"),
        }
    }

    /// An open device lets anybody in: asks it for a token with no PIN. A strict one is marked as needing the PIN.
    fn pair_by_itself(&self, id: &str, addr: SocketAddr) -> Option<String> {
        if self.forgotten.lock().unwrap().contains(id) {
            return None;
        }
        let info = client::fetch_info(addr).ok()?;
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.needs_pin = !info.open;
        }
        if !info.open {
            return None;
        }
        let me = self.identity();
        let token = client::pair(addr, &me.device_id, &me.name, "").ok()?;
        self.known.remember(Known { device_id: info.device_id, name: info.name, kind: info.kind, token: token.clone() }).ok()?;
        Some(token)
    }

    fn got_state(&self, id: &str, state: PlayerState) {
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.state = Some(state.clone());
        }
        self.listener.remote_state(id, state);
        self.listener.devices_changed();
    }

    /// The connection ended by itself (the other app closed, the network dropped): tried again for a while.
    fn session_closed(self: &Arc<Self>, id: &str, generation: u64, reason: Option<String>) {
        let ended = {
            let mut sessions = self.sessions.lock().unwrap();
            if sessions.get(id).is_some_and(|watch| watch.generation == generation) { sessions.remove(id) } else { None }
        };
        // Dropped outside the lock, and `Session::close` knows not to wait for the thread it is called from.
        let Some(ended) = ended else { return };
        drop(ended);
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.state = None;
        }
        let was_selected = {
            let mut selected = self.selected.lock().unwrap();
            let was = selected.as_deref() == Some(id);
            if was {
                *selected = None;
            }
            was
        };
        if was_selected {
            self.listener.remote_closed(id, reason);
        }
        self.listener.devices_changed();
        self.reconnect(id);
    }

    fn reconnect(self: &Arc<Self>, id: &str) {
        let (me, id) = (self.clone(), id.to_string());
        let spawned = thread::Builder::new().name("connect-reconnect".into()).spawn(move || {
            for seconds in RECONNECT_DELAYS {
                thread::sleep(Duration::from_secs(seconds));
                if !me.is_running() || !me.nearby.lock().unwrap().contains_key(&id) || me.sessions.lock().unwrap().contains_key(&id) {
                    return;
                }
                me.connect_to(&id);
                thread::sleep(Duration::from_secs(1));
                if me.sessions.lock().unwrap().contains_key(&id) {
                    return;
                }
            }
        });
        if spawned.is_err() {
            warn!("Could not start a thread to reconnect to a device");
        }
    }

    // === THE LIST ===

    /// Everything the user can pick: what is on the network and what it has paired with before.
    pub fn devices(&self) -> Vec<DeviceView> {
        let selected = self.selected.lock().unwrap().clone();
        let ready: HashSet<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        let nearby = self.nearby.lock().unwrap();
        let view = |id: &str, name: &str, kind: &str, address: Option<String>, near: Option<&Nearby>| DeviceView {
            id: id.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            address,
            paired: self.known.get(id).is_some(),
            online: near.is_some(),
            ready: ready.contains(id),
            connected: selected.as_deref() == Some(id),
            needs_pin: near.is_some_and(|n| n.needs_pin) && self.known.get(id).is_none(),
            playing: near.and_then(|n| n.state.as_ref()).is_some_and(|s| s.playing && s.now_playing.is_some()),
            now_playing: near
                .and_then(|n| n.state.as_ref())
                .and_then(|s| s.now_playing.as_ref())
                .map(|p| PlayingView { title: p.title.clone(), playlist: p.playlist.clone() }),
        };
        let mut views: Vec<DeviceView> = nearby.values().map(|n| view(&n.info.device_id, &n.info.name, &n.info.kind, Some(n.addr.to_string()), Some(n))).collect();
        for known in self.known.all() {
            if !nearby.contains_key(&known.device_id) {
                views.push(view(&known.device_id, &known.name, &known.kind, None, None));
            }
        }
        views.sort_by(|a, b| {
            b.connected.cmp(&a.connected).then(b.playing.cmp(&a.playing)).then(b.online.cmp(&a.online)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        views
    }

    /// With a strict device: trades the PIN it shows for its token.
    pub fn pair(self: &Arc<Self>, device_id: &str, pin: &str) -> Result<(), ConnectError> {
        let (info, addr) = {
            let nearby = self.nearby.lock().unwrap();
            let device = nearby.get(device_id).ok_or(ConnectError::Offline)?;
            (device.info.clone(), device.addr)
        };
        let me = self.identity();
        let token = client::pair(addr, &me.device_id, &me.name, pin)?;
        self.forgotten.lock().unwrap().remove(device_id);
        self.known.remember(Known { device_id: info.device_id, name: info.name, kind: info.kind, token }).map_err(|e| ConnectError::Other(e.to_string()))?;
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(device_id) {
            nearby.needs_pin = false;
        }
        self.listener.devices_changed();
        self.connect_to(device_id);
        Ok(())
    }

    /// Takes the pairing away: this device forgets the other's token and does not pair with it again by itself.
    pub fn forget(&self, device_id: &str) -> Result<(), ConnectError> {
        self.forgotten.lock().unwrap().insert(device_id.to_string());
        self.drop_session(device_id);
        self.known.forget(device_id).map_err(|e| ConnectError::Other(e.to_string()))?;
        self.listener.devices_changed();
        Ok(())
    }

    // === CONTROLLING ===

    /// Starts controlling a device it is connected to (the one being controlled until then is let go of).
    pub fn select(self: &Arc<Self>, device_id: &str) -> Result<(), ConnectError> {
        if self.known.get(device_id).is_none() {
            return Err(ConnectError::NotPaired);
        }
        if !self.nearby.lock().unwrap().contains_key(device_id) {
            return Err(ConnectError::Offline);
        }
        // Normally the connection is already there; if it is not (it dropped a moment ago), try now.
        if !self.sessions.lock().unwrap().contains_key(device_id) {
            self.attach(device_id, true);
            if !self.sessions.lock().unwrap().contains_key(device_id) {
                return Err(if self.known.get(device_id).is_none() { ConnectError::NotPaired } else { ConnectError::Offline });
            }
        }
        *self.selected.lock().unwrap() = Some(device_id.to_string());
        self.listener.devices_changed();
        Ok(())
    }

    /// Back to controlling nothing (the connection stays: what the device plays is still shown).
    pub fn deselect(&self) {
        let was = self.selected.lock().unwrap().take();
        if was.is_some() {
            self.listener.devices_changed();
        }
    }

    /// The device being controlled, if any.
    pub fn selected(&self) -> Option<String> {
        self.selected.lock().unwrap().clone()
    }

    /// What the device being controlled said last (what to show right away on choosing it).
    pub fn state_of(&self, device_id: &str) -> Option<PlayerState> {
        self.nearby.lock().unwrap().get(device_id).and_then(|n| n.state.clone())
    }

    /// Asks the device being controlled to do something.
    pub fn send(&self, command: Command) -> Result<u64, ConnectError> {
        command.validate().map_err(ConnectError::Other)?;
        let selected = self.selected.lock().unwrap().clone().ok_or(ConnectError::NoSession)?;
        let sessions = self.sessions.lock().unwrap();
        let watch = sessions.get(&selected).ok_or(ConnectError::NoSession)?;
        Ok(watch.session.send(command))
    }

    // === BEING CONTROLLED ===

    /// What this device is playing now: kept for whoever connects, and sent to those who are.
    pub fn publish(&self, state: PlayerState) {
        *self.state.lock().unwrap() = state.clone();
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.publish(&state);
        }
    }

    /// The PIN to show on this device's screen when it is strict (a new one if there is none or it expired).
    pub fn pin(&self) -> Option<String> {
        if !self.is_running() || self.pairing.is_open() {
            return None;
        }
        let now = Instant::now();
        Some(self.pairing.current_pin(now).unwrap_or_else(|| self.pairing.new_pin(now)))
    }

    /// A fresh PIN, even if there was one.
    pub fn renew_pin(&self) -> Option<String> {
        (self.is_running() && !self.pairing.is_open()).then(|| self.pairing.new_pin(Instant::now()))
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
