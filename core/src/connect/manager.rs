//! Soundify Connect as the app uses it: one object that is both ends. As a player it
//! runs the server and announces itself; as a controller it sees the others, pairs with
//! them by itself when they are open (no PIN; strict, the default, asks for it once) and
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

use super::cast::SharedFile;
use super::client::{self, ClientError, Event, Session};
use super::discovery::{Change, Discovery};
use super::pairing::{self, Known, KnownDevices, Pairing, Trusted};
use super::protocol::{CastStart, Command, DeviceInfo, LibraryEntry, MAX_LIBRARY, PROTOCOL, PlayerState, QueueInfo, ServerMessage};
use super::server::{Host, Server};
use super::stats::{self, short};
use super::tls::{Expect, TlsIdentity};

/// The port tried first, so a firewall rule or a bookmark keeps working; any free one if it is taken.
pub const PREFERRED_PORT: u16 = 48650;

/// Waits (seconds) between the tries to get a dropped connection back, while the device is still on the network.
const RECONNECT_DELAYS: [u64; 8] = [1, 2, 4, 8, 16, 30, 30, 30];

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
    /// A device says which playlist it is playing (None = none).
    fn remote_queue(&self, _device_id: &str, _queue: Option<QueueInfo>) {}
    /// A device says which playlists it has (its own library).
    fn remote_library(&self, _device_id: &str, _playlists: Vec<LibraryEntry>) {}
    /// A device gives the files of a playlist of its library that was asked for.
    fn remote_browse(&self, _device_id: &str, _key: &str, _queue: Option<QueueInfo>) {}
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
    /// It speaks another version of Connect: `outdated` (update IT) or `newer` (update THIS one). Not connected to.
    pub version: Option<String>,
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
    /// Its certificate is not the one it was paired with (reinstalled, or somebody in between): pair again with the PIN.
    Untrusted,
    Other(String),
}

impl From<ClientError> for ConnectError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Unreachable(_) => ConnectError::Offline,
            ClientError::Refused { status, error, seconds } => ConnectError::Refused { status, error, seconds },
            ClientError::Protocol(e) => ConnectError::Other(e),
            ClientError::Untrusted => ConnectError::Untrusted,
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
    /// The playlist it last said it is playing.
    queue: Option<QueueInfo>,
    /// It is strict: pairing needs the PIN it shows.
    needs_pin: bool,
    /// Found by it connecting here, not announced on the network: nothing will say when it is gone, so it is let go of as soon as its connection fails.
    learned: bool,
    /// The playlists of its own library, as it last said.
    library: Vec<LibraryEntry>,
    /// Another version of Connect (`outdated` / `newer`): shown, never connected to.
    version: Option<&'static str>,
}

/// What another version of Connect is called on the screen.
fn mismatch(v: u32) -> Option<&'static str> {
    match v.cmp(&PROTOCOL) {
        std::cmp::Ordering::Less => Some("outdated"),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some("newer"),
    }
}

/// The open connection to a paired device (its `generation` tells an old one's end from the current one's).
struct Watch {
    generation: u64,
    session: Session,
    /// When it was opened, for the log (how long a connection lasted says a lot about why it ended).
    since: Instant,
}

/// What the server asks the app: its own description and the last state published.
struct HostAdapter {
    me: std::sync::Weak<Connect>,
    info: DeviceInfo,
    pairing: Arc<Pairing>,
    state: Arc<Mutex<PlayerState>>,
    queue: Arc<Mutex<Option<QueueInfo>>>,
    library: Arc<Mutex<Vec<LibraryEntry>>>,
    player: Arc<dyn Player>,
}

impl Host for HostAdapter {
    fn info(&self) -> DeviceInfo {
        DeviceInfo { open: self.pairing.is_open(), ..self.info.clone() }
    }

    fn state(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    fn queue(&self) -> Option<QueueInfo> {
        self.queue.lock().unwrap().clone()
    }

    fn library(&self) -> Vec<LibraryEntry> {
        self.library.lock().unwrap().clone()
    }

    fn command(&self, who: &Trusted, command: Command) -> Result<(), String> {
        self.player.command(who, command)
    }

    fn controller_seen(&self, _who: &Trusted, ip: IpAddr, port: u16) {
        if let Some(connect) = self.me.upgrade() {
            connect.learn(SocketAddr::new(ip, port));
        }
    }
}

pub struct Connect {
    identity: Mutex<Identity>,
    factory: Box<dyn Fn() -> io::Result<Arc<dyn Discovery>> + Send + Sync>,
    pairing: Arc<Pairing>,
    known: KnownDevices,
    /// This device's certificate (the server shows it; a pass tells the receiver to expect it).
    tls: Arc<TlsIdentity>,
    player: Arc<dyn Player>,
    listener: Arc<dyn Listener>,
    state: Arc<Mutex<PlayerState>>,
    /// «Escuchar en»: the device this one passed what it plays to.
    casting: Mutex<Option<String>>,
    queue: Arc<Mutex<Option<QueueInfo>>>,
    /// This device's own library, as the page last said (kept for whoever connects).
    library: Arc<Mutex<Vec<LibraryEntry>>>,
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
    /// When each device last went off the network: a device found by connection a moment before it left is not added back.
    departures: Mutex<HashMap<String, Instant>>,
    /// Devices discovery said left while their connection was up: they go when it ends (see `saw`).
    swallowed: Mutex<HashSet<String>>,
    /// States that arrived from each device in this statistics window (to name the one that floods).
    received: Mutex<HashMap<String, u64>>,
}

struct Running {
    server: Server,
    /// None when no way of finding the others could be started.
    discovery: Option<Arc<dyn Discovery>>,
}

impl Connect {
    /// `factory` makes the discovery each time Connect is turned on (the real one cannot be restarted once stopped).
    pub fn new(
        identity: Identity,
        factory: impl Fn() -> io::Result<Arc<dyn Discovery>> + Send + Sync + 'static,
        pairing: Arc<Pairing>,
        known: KnownDevices,
        tls: Arc<TlsIdentity>,
        player: Arc<dyn Player>,
        listener: Arc<dyn Listener>,
    ) -> Arc<Connect> {
        Arc::new(Connect {
            identity: Mutex::new(identity),
            factory: Box::new(factory),
            pairing,
            known,
            tls,
            player,
            listener,
            state: Arc::new(Mutex::new(PlayerState::default())),
            casting: Mutex::new(None),
            queue: Arc::new(Mutex::new(None)),
            library: Arc::new(Mutex::new(Vec::new())),
            running: Mutex::new(None),
            nearby: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            selected: Mutex::new(None),
            generation: AtomicU64::new(0),
            busy: Mutex::new(HashMap::new()),
            forgotten: Mutex::new(HashSet::new()),
            departures: Mutex::new(HashMap::new()),
            swallowed: Mutex::new(HashSet::new()),
            received: Mutex::new(HashMap::new()),
        })
    }

    /// The user renamed this device (what the others see from the next time it is announced).
    pub fn set_name(&self, name: &str) {
        self.identity.lock().unwrap().name = name.to_string();
    }

    pub fn identity(&self) -> Identity {
        self.identity.lock().unwrap().clone()
    }

    fn info(&self) -> DeviceInfo {
        let identity = self.identity();
        DeviceInfo { v: PROTOCOL, device_id: identity.device_id, name: identity.name, kind: identity.kind, open: self.pairing.is_open() }
    }

    /// The fingerprint of this device's certificate (what the others pin; shown in Ajustes to compare by eye).
    pub fn fingerprint(&self) -> String {
        self.tls.fingerprint().to_string()
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// The port the server listens on, while it runs.
    pub fn port(&self) -> Option<u16> {
        self.running.lock().unwrap().as_ref().map(|running| running.server.port())
    }

    /// Can the other devices be found by themselves? False: only by address (or when they connect here).
    pub fn discovers(&self) -> bool {
        self.running.lock().unwrap().as_ref().is_some_and(|running| running.discovery.is_some())
    }

    /// Where this device can be reached on the local network, for typing on another one (`ip:port`).
    pub fn addresses(&self) -> Vec<String> {
        let Some(port) = self.port() else { return Vec::new() };
        super::beacon::local_ipv4().map(|ip| vec![format!("{ip}:{port}")]).unwrap_or_default()
    }

    /// A device connected here and said where it listens: it is added too, so it is found back without anybody typing its address.
    fn learn(self: &Arc<Self>, addr: SocketAddr) {
        let already = self.nearby.lock().unwrap().values().any(|n| n.addr == addr);
        if already {
            return;
        }
        let (me, triggered) = (self.clone(), Instant::now());
        let spawned = thread::Builder::new().name("connect-learn".into()).spawn(move || {
            // Its own description (and its id) comes from itself.
            let _ = me.add(addr, false, triggered);
        });
        if spawned.is_err() {
            warn!("Could not start a thread to find a device back");
        }
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
        // An older version could pair with itself (its mDNS name renamed, read as another device): that entry and the
        // known «device» are let go of, so it neither shows in the lists nor is connected to.
        let own = self.identity().device_id;
        if self.pairing.trusted().iter().any(|t| t.controller_id == own) {
            let _ = self.pairing.forget(&own);
        }
        if self.known.get(&own).is_some() {
            let _ = self.known.forget(&own);
        }
        let host = Arc::new(HostAdapter { me: Arc::downgrade(self), info: self.info(), pairing: self.pairing.clone(), state: self.state.clone(), queue: self.queue.clone(), library: self.library.clone(), player: self.player.clone() });
        let server = match Server::start(preferred_port, host.clone(), self.pairing.clone(), self.tls.clone()) {
            Ok(server) => server,
            // The preferred port is taken (another copy of the app): any free one.
            Err(_) if preferred_port != 0 => Server::start(0, host, self.pairing.clone(), self.tls.clone())?,
            Err(e) => return Err(e),
        };
        // Finding the others automatically is a plus: if it cannot be done the server still runs, and the
        // devices are reached by address (or find this one when it connects to them).
        let discovery = match (self.factory)() {
            Ok(discovery) => {
                let me = Arc::downgrade(self);
                let announced = discovery.announce(&self.info(), server.port());
                let browsing = discovery.browse(Box::new(move |change| {
                    if let Some(connect) = me.upgrade() {
                        connect.saw(change);
                    }
                }));
                if let Err(e) = announced.as_ref().and(browsing.as_ref()) {
                    warn!("Soundify Connect: automatic discovery is not complete: {e}");
                }
                (announced.is_ok() || browsing.is_ok()).then_some(discovery)
            }
            Err(e) => {
                warn!("Soundify Connect: no automatic discovery: {e}");
                None
            }
        };
        info!("Soundify Connect on, port {}", server.port());
        *running = Some(Running { server, discovery });
        drop(running);
        self.report_stats();
        Ok(())
    }

    /// Writes the counters as one line every [`stats::WINDOW`] while Connect is on (as a warning if they look like a storm).
    fn report_stats(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let spawned = thread::Builder::new().name("connect-stats".into()).spawn(move || {
            stats::take();
            loop {
                // A second at a time: when Connect is turned off the thread ends within one.
                for _ in 0..stats::WINDOW.as_secs() {
                    thread::sleep(Duration::from_secs(1));
                    if me.upgrade().is_none_or(|connect| !connect.is_running()) {
                        return;
                    }
                }
                let Some(connect) = me.upgrade() else { return };
                let snapshot = stats::take();
                // Who sent too many: its app is looping (an old version does it), and it shows here as the PC's own refreshes.
                let received: Vec<(String, u64)> = connect.received.lock().unwrap().drain().collect();
                for (id, n) in received.into_iter().filter(|(_, n)| *n > stats::WARN_STATES) {
                    let name = connect.nearby.lock().unwrap().get(&id).map(|d| d.info.name.clone()).unwrap_or_else(|| "?".into());
                    warn!("Soundify Connect: {name} #{} sent {n} states in {}s: it is flooding (its app may be an old version with the publish loop)", short(&id), stats::WINDOW.as_secs());
                }
                if snapshot.is_quiet() {
                    continue;
                }
                let line = snapshot.line(stats::WINDOW.as_secs(), connect.controllers().len(), connect.sessions.lock().unwrap().len());
                if snapshot.is_suspicious() {
                    warn!("Soundify Connect: {line} <- more than usual: connections or states are bouncing");
                } else {
                    info!("Soundify Connect: {line}");
                }
            }
        });
        if spawned.is_err() {
            warn!("Could not start the Connect statistics thread");
        }
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
            if let Some(discovery) = &running.discovery {
                discovery.shutdown();
            }
            running.server.stop();
        }
        self.nearby.lock().unwrap().clear();
        self.listener.devices_changed();
    }

    /// A device appeared on or disappeared from the network.
    pub(crate) fn saw(self: &Arc<Self>, change: Change) {
        let own = self.identity().device_id;
        match change {
            Change::Up(found) => {
                if found.info.device_id == own {
                    return;
                }
                let version = mismatch(found.info.v);
                // Another version of Soundify on THIS very device (the installed app next to a test copy, say): it is not
                // another device, and could not be connected to or forgotten from here. Same version is left alone: two copies
                // of this one on a machine (dev and installed) do see each other, as they always did.
                if version.is_some() && super::beacon::local_ipv4().is_some_and(|mine| found.addrs.contains(&IpAddr::V4(mine))) {
                    return;
                }
                self.swallowed.lock().unwrap().remove(&found.info.device_id);
                let Some(ip) = found.addrs.iter().find(|ip| matches!(ip, IpAddr::V4(_))).or(found.addrs.first()) else { return };
                let addr = SocketAddr::new(*ip, found.port);
                let id = found.info.device_id.clone();
                {
                    let mut nearby = self.nearby.lock().unwrap();
                    let (state, queue, library, needs_pin, manual) = nearby
                        .get(&id)
                        .map(|n| (n.state.clone(), n.queue.clone(), n.library.clone(), n.needs_pin, n.manual))
                        .unwrap_or((None, None, Vec::new(), false, false));
                    nearby.insert(id.clone(), Nearby { info: found.info, addr, manual, state, queue, library, needs_pin, learned: false, version });
                }
                self.listener.devices_changed();
                if version.is_none() {
                    self.connect_to(&id);
                }
            }
            Change::Down(id) => {
                // A live connection is the proof that it is there: a phone that sleeps loses the broadcasts it lives on and
                // would flicker. If the connection ends later, this is what then says it is gone (`session_closed`).
                if self.sessions.lock().unwrap().contains_key(&id) {
                    self.swallowed.lock().unwrap().insert(id);
                    return;
                }
                {
                    let mut nearby = self.nearby.lock().unwrap();
                    if nearby.get(&id).is_some_and(|n| n.manual) {
                        return;
                    }
                    nearby.remove(&id);
                    self.departures.lock().unwrap().insert(id.clone(), Instant::now());
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
        self.add(addr, true, Instant::now())
    }

    /// `manual`: the user typed it, so it stays until forgotten; otherwise it was found by connection and goes when it stops answering.
    /// `asked_at`: when the reason to add it arose; if the device left the network after that, it is not added.
    fn add(self: &Arc<Self>, addr: SocketAddr, manual: bool, asked_at: Instant) -> Result<DeviceInfo, ConnectError> {
        let info = client::fetch_info(addr)?;
        // It answered, but it may have left the network while it did: then it is not added back.
        if !manual && self.departures.lock().unwrap().get(&info.device_id).is_some_and(|left| *left >= asked_at) {
            return Err(ConnectError::Offline);
        }
        if info.device_id == self.identity().device_id {
            return Err(ConnectError::Other("that is this device".into()));
        }
        if manual {
            self.forgotten.lock().unwrap().remove(&info.device_id);
        } else if self.forgotten.lock().unwrap().contains(&info.device_id) {
            return Err(ConnectError::Other("forgotten".into()));
        }
        {
            let mut nearby = self.nearby.lock().unwrap();
            // Already known (announced on the network, or typed in): what is known about it is better than a guess.
            if !manual && nearby.contains_key(&info.device_id) {
                return Ok(info);
            }
            nearby.insert(info.device_id.clone(), Nearby { info: info.clone(), addr, manual, state: None, queue: None, library: Vec::new(), needs_pin: !info.open, learned: !manual, version: None });
        }
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
            me.forget_if_learned_and_unreachable(&id);
            me.listener.devices_changed();
        });
        if spawned.is_err() {
            warn!("Could not start a thread to connect to a device");
        }
    }

    /// Pairs if needed and opens the session. `retry`: a token the device refused is forgotten and pairing is tried once more.
    fn attach(self: &Arc<Self>, id: &str, retry: bool) {
        let Some((addr, name)) = self.nearby.lock().unwrap().get(id).map(|n| (n.addr, n.info.name.clone())) else { return };
        // A device paired before TLS has no certificate pinned, and its token travelled in the clear: it is paired again.
        if self.known.get(id).is_some_and(|known| known.fingerprint.is_empty()) {
            info!("Soundify Connect: {name} #{} was paired before TLS: pairing again", short(id));
            let _ = self.known.forget(id);
        }
        let (token, fingerprint) = match self.known.get(id) {
            Some(known) => (known.token, known.fingerprint),
            None => match self.pair_by_itself(id, addr) {
                Some(paired) => paired,
                None => return,
            },
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (me, device) = (Arc::downgrade(self), id.to_string());
        let opened = Session::open_with(addr, &token, &Expect::Pinned(fingerprint), self.port(), Some(&self.identity().name), move |event| {
            let Some(connect) = me.upgrade() else { return };
            match event {
                Event::Message(ServerMessage::State { state }) => connect.got_state(&device, state),
                Event::Message(ServerMessage::Queue { queue }) => connect.got_queue(&device, queue),
                Event::Message(ServerMessage::Library { playlists }) => connect.got_library(&device, playlists),
                Event::Message(ServerMessage::Browse { key, queue }) => connect.listener.remote_browse(&device, &key, queue),
                Event::Message(_) => {}
                Event::Closed(reason) => connect.session_closed(&device, generation, reason),
            }
        });
        match opened {
            Ok(session) => {
                let old = self.sessions.lock().unwrap().insert(id.to_string(), Watch { generation, session, since: Instant::now() });
                if let Some(old) = old {
                    old.session.close();
                }
                stats::opened(1);
                info!("Soundify Connect: OUT {name} #{} connected (generation {generation}, {} links now)", short(id), self.sessions.lock().unwrap().len());
                self.forget_stale_twins(id);
            }
            // A token the device no longer accepts is of no use: forgotten, and tried again as a new device.
            Err(ClientError::Refused { status: 401, .. }) => {
                let _ = self.known.forget(id);
                if retry {
                    self.attach(id, false);
                }
            }
            // Another certificate than the one it was paired with: NOT connected, and the user is asked for the PIN again
            // (a reinstalled device passes it; somebody in between does not). The token is kept until then.
            Err(ClientError::Untrusted) => {
                warn!("Soundify Connect: {name} #{} shows another certificate than the one it was paired with: not connected", short(id));
                stats::tls_failed(1);
                if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
                    nearby.needs_pin = true;
                }
                let _ = self.known.forget(id);
            }
            Err(e) => info!("Soundify Connect: could not connect to {name}: {e}"),
        }
    }

    /// An open device lets anybody in: asks it for a token with no PIN. A strict one is marked as needing the PIN.
    fn pair_by_itself(&self, id: &str, addr: SocketAddr) -> Option<(String, String)> {
        if self.forgotten.lock().unwrap().contains(id) {
            return None;
        }
        let info = match client::fetch_info(addr) {
            Ok(info) => info,
            // Found by a way that does not say its version (the UDP beacon): it is another one. Shown as such, left alone.
            Err(ClientError::Refused { status: 426, error, .. }) => {
                if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
                    nearby.version = Some(if error == "newer" { "newer" } else { "outdated" });
                }
                return None;
            }
            Err(_) => return None,
        };
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.needs_pin = !info.open;
        }
        if !info.open {
            return None;
        }
        let me = self.identity();
        let paired = client::pair_as(addr, &me.device_id, &me.name, &me.kind, "").ok()?;
        self.known.remember(Known { device_id: info.device_id, name: info.name, kind: info.kind, token: paired.token.clone(), fingerprint: paired.fingerprint.clone() }).ok()?;
        Some((paired.token, paired.fingerprint))
    }

    fn got_state(&self, id: &str, state: PlayerState) {
        stats::states_received(1);
        *self.received.lock().unwrap().entry(id.to_string()).or_insert(0) += 1;
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.state = Some(state.clone());
        }
        self.listener.remote_state(id, state);
        self.listener.devices_changed();
    }

    fn got_queue(&self, id: &str, queue: Option<QueueInfo>) {
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.queue = queue.clone();
        }
        self.listener.remote_queue(id, queue);
    }

    fn got_library(&self, id: &str, mut playlists: Vec<LibraryEntry>) {
        playlists.truncate(MAX_LIBRARY);
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.library = playlists.clone();
        }
        self.listener.remote_library(id, playlists);
    }

    /// The connection ended by itself (the other app closed, the network dropped): tried again for a while.
    fn session_closed(self: &Arc<Self>, id: &str, generation: u64, reason: Option<String>) {
        let ended = {
            let mut sessions = self.sessions.lock().unwrap();
            if sessions.get(id).is_some_and(|watch| watch.generation == generation) { sessions.remove(id) } else { None }
        };
        // Dropped outside the lock, and `Session::close` knows not to wait for the thread it is called from.
        let Some(ended) = ended else { return };
        let (lasted, name) = (ended.since.elapsed(), self.nearby.lock().unwrap().get(id).map(|n| n.info.name.clone()).unwrap_or_else(|| "?".into()));
        drop(ended);
        stats::closed(1);
        info!("Soundify Connect: OUT {name} #{} ended after {:.1}s ({})", short(id), lasted.as_secs_f32(), reason.as_deref().unwrap_or("closed by the other side"));
        if let Some(nearby) = self.nearby.lock().unwrap().get_mut(id) {
            nearby.state = None;
            nearby.queue = None;
        }
        // Discovery said it left while the connection was still up (and nothing has said it came back): now it is gone.
        let left = self.swallowed.lock().unwrap().remove(id) && !self.nearby.lock().unwrap().get(id).is_some_and(|n| n.manual);
        // Found only by its connection: when that ends it is gone (if it is back it connects again and is found again).
        if left || self.nearby.lock().unwrap().get(id).is_some_and(|n| n.learned) {
            self.nearby.lock().unwrap().remove(id);
            self.departures.lock().unwrap().insert(id.to_string(), Instant::now());
            if self.selected.lock().unwrap().as_deref() == Some(id) {
                *self.selected.lock().unwrap() = None;
                self.listener.remote_closed(id, reason);
            }
            self.listener.devices_changed();
            return;
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

    /// A computer that lost its data (a new id) shows up as a second device with the same name, left over in the list as
    /// «no connection» for ever. When a computer is connected, the other paired ones with ITS name and kind that are not on
    /// the network are that same computer's old identity: they are let go of (it pairs again by itself if it is open).
    /// Only computers: a name is unique among them on a network, while two phones may well be called the same.
    fn forget_stale_twins(&self, id: &str) {
        let Some((name, kind)) = self.nearby.lock().unwrap().get(id).map(|n| (n.info.name.clone(), n.info.kind.clone())) else { return };
        if kind != "desktop" {
            return;
        }
        for twin in self.known.all() {
            let online = self.nearby.lock().unwrap().contains_key(&twin.device_id);
            if twin.device_id != id && twin.kind == kind && twin.name == name && !online {
                info!("Soundify Connect: {name} paired again with another id: the old one ({}) is forgotten", twin.device_id);
                let _ = self.known.forget(&twin.device_id);
                self.listener.devices_changed();
            }
        }
    }

    /// A device found by connection that cannot be connected to is not there (nothing else will say so).
    fn forget_if_learned_and_unreachable(&self, id: &str) {
        if self.sessions.lock().unwrap().contains_key(id) {
            return;
        }
        let mut nearby = self.nearby.lock().unwrap();
        if nearby.get(id).is_some_and(|n| n.learned) {
            nearby.remove(id);
            self.departures.lock().unwrap().insert(id.to_string(), Instant::now());
        }
    }

    fn reconnect(self: &Arc<Self>, id: &str) {
        let (me, id) = (self.clone(), id.to_string());
        let spawned = thread::Builder::new().name("connect-reconnect".into()).spawn(move || {
            for seconds in RECONNECT_DELAYS {
                thread::sleep(Duration::from_secs(seconds));
                if !me.is_running() || !me.nearby.lock().unwrap().contains_key(&id) || me.sessions.lock().unwrap().contains_key(&id) {
                    return;
                }
                stats::reconnects(1);
                me.connect_to(&id);
                thread::sleep(Duration::from_secs(1));
                if me.sessions.lock().unwrap().contains_key(&id) {
                    return;
                }
            }
            // It did not come back: a device found by connection (nobody typed it, nobody announces it) is let go of.
            let removed = {
                let mut nearby = me.nearby.lock().unwrap();
                if nearby.get(&id).is_some_and(|n| !n.manual) { nearby.remove(&id).is_some() } else { false }
            };
            if removed {
                me.listener.devices_changed();
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
            version: near.and_then(|n| n.version).map(str::to_string),
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
        let paired = client::pair_as(addr, &me.device_id, &me.name, &me.kind, pin)?;
        self.forgotten.lock().unwrap().remove(device_id);
        self.known
            .remember(Known { device_id: info.device_id, name: info.name, kind: info.kind, token: paired.token, fingerprint: paired.fingerprint })
            .map_err(|e| ConnectError::Other(e.to_string()))?;
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
        // The device is told it is now controlled from here: it stops controlling something else, if it did, and says what it plays.
        if let Some(watch) = self.sessions.lock().unwrap().get(device_id) {
            watch.session.send(Command::Take);
        }
        self.listener.devices_changed();
        Ok(())
    }

    // === «ESCUCHAR EN»: PASSING WHAT PLAYS HERE ===

    /// Passes what this device plays to `device_id`: it reads `files` from here (only those, with a ticket that ends with
    /// [`Connect::cast_end`]) and plays them from `pass.index` at `pass.position`. From then on this device controls it.
    pub fn cast_to(self: &Arc<Self>, device_id: &str, mut pass: CastStart, files: Vec<(String, SharedFile)>) -> Result<(), ConnectError> {
        let (port, tickets) = {
            let running = self.running.lock().unwrap();
            let running = running.as_ref().ok_or(ConnectError::Offline)?;
            (running.server.port(), running.server.tickets())
        };
        // Passed to another one before: that one stops (its ticket ends with the new grant).
        if let Some(before) = self.casting.lock().unwrap().clone()
            && before != device_id
        {
            self.send_to(&before, Command::CastEnd { reason: Some("passed".into()) });
        }
        self.control(device_id)?;
        pass.host = String::new();
        pass.port = port;
        pass.fingerprint = self.tls.fingerprint().to_string();
        pass.ticket = tickets.grant(files);
        let command = Command::Cast(Box::new(pass));
        if let Err(error) = command.validate() {
            tickets.revoke();
            return Err(ConnectError::Other(error));
        }
        if !self.send_to(device_id, command) {
            tickets.revoke();
            return Err(ConnectError::Offline);
        }
        *self.casting.lock().unwrap() = Some(device_id.to_string());
        info!("Soundify Connect: passed what plays here to #{}", short(device_id));
        self.listener.devices_changed();
        Ok(())
    }

    /// The pass ends («Traer aquí», «Detener», the user plays something else here): nothing more can be read from here,
    /// and the device that played it is told (it stops).
    pub fn cast_end(&self, reason: &str) {
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.tickets().revoke();
        }
        if let Some(device) = self.casting.lock().unwrap().take() {
            self.send_to(&device, Command::CastEnd { reason: Some(reason.chars().take(32).collect()) });
            info!("Soundify Connect: the pass to #{} ended ({reason})", short(&device));
            self.listener.devices_changed();
        }
    }

    /// The device this one passed what it plays to, if any.
    pub fn casting(&self) -> Option<String> {
        self.casting.lock().unwrap().clone()
    }

    /// Becomes the device controlled from here WITHOUT telling it «taken» (a pass says more than that on its own).
    fn control(self: &Arc<Self>, device_id: &str) -> Result<(), ConnectError> {
        if self.known.get(device_id).is_none() {
            return Err(ConnectError::NotPaired);
        }
        if !self.sessions.lock().unwrap().contains_key(device_id) {
            self.attach(device_id, true);
            if !self.sessions.lock().unwrap().contains_key(device_id) {
                return Err(if self.known.get(device_id).is_none() { ConnectError::NotPaired } else { ConnectError::Offline });
            }
        }
        *self.selected.lock().unwrap() = Some(device_id.to_string());
        Ok(())
    }

    fn send_to(&self, device_id: &str, command: Command) -> bool {
        self.sessions.lock().unwrap().get(device_id).map(|watch| watch.session.send(command)).is_some()
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

    /// What this device last said it plays (what the page published): to put the screen back as it was if the page was
    /// reloaded while a pass sounded here.
    pub fn own_state(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    /// What the device being controlled said last (what to show right away on choosing it).
    pub fn state_of(&self, device_id: &str) -> Option<PlayerState> {
        self.nearby.lock().unwrap().get(device_id).and_then(|n| n.state.clone())
    }

    /// The playlists a device said its library has.
    pub fn library_of(&self, device_id: &str) -> Vec<LibraryEntry> {
        self.nearby.lock().unwrap().get(device_id).map(|n| n.library.clone()).unwrap_or_default()
    }

    /// The playlist a device said it is playing.
    pub fn queue_of(&self, device_id: &str) -> Option<QueueInfo> {
        self.nearby.lock().unwrap().get(device_id).and_then(|n| n.queue.clone())
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

    /// The playlist this device is playing: kept for whoever connects, and sent to those who are connected.
    pub fn publish_queue(&self, queue: Option<QueueInfo>) {
        let queue = queue.map(QueueInfo::limited);
        *self.queue.lock().unwrap() = queue.clone();
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.publish_queue(&queue);
        }
    }

    /// This device's own library (its playlists, without files): kept for whoever connects, and sent to those who are.
    pub fn publish_library(&self, mut playlists: Vec<LibraryEntry>) {
        playlists.truncate(MAX_LIBRARY);
        *self.library.lock().unwrap() = playlists.clone();
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.publish_library(&playlists);
        }
    }

    /// The files of a playlist of this device's library that a controller asked for (`None`: there is none by that key).
    pub fn publish_browse(&self, key: &str, queue: Option<QueueInfo>) {
        let queue = queue.map(QueueInfo::limited);
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.server.publish_browse(key, &queue);
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
        let gone = self.pairing.prune(pairing::now_secs(), pairing::FORGET_AFTER_SECS);
        if gone > 0 {
            info!("Soundify Connect: {gone} controller(s) not seen for 30 days were let go of");
        }
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
