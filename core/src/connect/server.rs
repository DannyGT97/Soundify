//! The player's side: a small server on the local network. `GET /info` says who it
//! is, `POST /pair` trades a PIN for a token, and `GET /ws` (with the token) opens
//! the WebSocket where the state comes out and the commands come in.
//!
//! One thread accepts, one thread per connection (at most [`MAX_CONNECTIONS`]),
//! everything blocking: the app has no async runtime to lean on and the traffic is a
//! few messages a second.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message, WebSocket};

use super::http::{self, Request};
use super::pairing::{PairError, Pairing, Trusted};
use super::protocol::{BASE, ClientMessage, Command, DeviceInfo, PairRequest, PairResponse, PlayerState, ServerMessage};

/// Connections served at once (controllers plus whatever else knocks).
pub const MAX_CONNECTIONS: usize = 16;
/// How long a WebSocket may stay silent to our pings before it is let go of.
const PING_EVERY: Duration = Duration::from_secs(20);
/// The most one message may weigh: a queue of [`super::protocol::MAX_QUEUE`] files.
const MAX_MESSAGE: usize = 2 * 1024 * 1024;
/// What waits to go out to one controller before it is dropped as too slow.
const OUTBOX: usize = 64;

/// What the app plays with: the server only knows how to ask.
pub trait Host: Send + Sync {
    fn info(&self) -> DeviceInfo;
    /// The state as it is now (sent to a controller as soon as it connects).
    fn state(&self) -> PlayerState;
    /// Does what the controller asked. `Err` is the reason, which goes back in the `ack`.
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String>;
}

struct Shared {
    host: Arc<dyn Host>,
    pairing: Arc<Pairing>,
    stop: AtomicBool,
    connections: AtomicUsize,
    /// One per open WebSocket, with the number it is taken out by when it ends.
    outboxes: Mutex<Vec<(u64, SyncSender<ServerMessage>)>>,
    next_outbox: AtomicU64,
}

pub struct Server {
    shared: Arc<Shared>,
    port: u16,
    accepting: Mutex<Option<JoinHandle<()>>>,
}

impl Server {
    /// Starts listening on every interface. `port` 0 = any free one.
    pub fn start(port: u16, host: Arc<dyn Host>, pairing: Arc<Pairing>) -> io::Result<Server> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            host,
            pairing,
            stop: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            outboxes: Mutex::new(Vec::new()),
            next_outbox: AtomicU64::new(1),
        });
        let accepting = {
            let shared = shared.clone();
            thread::Builder::new().name("connect-accept".into()).spawn(move || accept_loop(listener, shared))?
        };
        info!("Soundify Connect listening on port {port}");
        Ok(Server { shared, port, accepting: Mutex::new(Some(accepting)) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Tells every controller what the player is doing now.
    pub fn publish(&self, state: &PlayerState) {
        let mut outboxes = self.shared.outboxes.lock().unwrap();
        outboxes.retain(|(_, outbox)| match outbox.try_send(ServerMessage::State { state: state.clone() }) {
            Ok(()) => true,
            // A controller that does not keep up is dropped: it reconnects and gets the state again.
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        });
    }

    /// Controllers connected right now.
    pub fn controllers(&self) -> usize {
        self.shared.outboxes.lock().unwrap().len()
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.accepting.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    shared.connections.fetch_sub(1, Ordering::SeqCst);
                    let mut stream = stream;
                    let _ = http::respond_json(&mut stream, 503, &serde_json::json!({ "error": "busy" }));
                    continue;
                }
                let shared = shared.clone();
                let spawned = thread::Builder::new().name("connect-connection".into()).spawn(move || {
                    if let Err(e) = serve(&shared, stream) {
                        debug!("Connect connection from {peer} ended: {e}");
                    }
                    shared.connections.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    warn!("Could not start a thread for a Connect connection");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(40)),
            Err(e) => {
                warn!("Connect accept failed: {e}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn serve(shared: &Arc<Shared>, mut stream: TcpStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = match http::read_request(&mut stream) {
        Ok(request) => request,
        Err(e) => {
            let _ = http::respond_json(&mut stream, 400, &serde_json::json!({ "error": "badRequest" }));
            return Err(e);
        }
    };
    let route = request.path.strip_prefix(BASE).unwrap_or("");
    match (request.method.as_str(), route) {
        ("GET", "/info") => http::respond_json(&mut stream, 200, &serde_json::to_value(shared.host.info()).unwrap_or_default()),
        ("POST", "/pair") => pair(shared, &mut stream, &request),
        ("GET", "/ws") => websocket(shared, stream, &request),
        (_, "/info" | "/pair" | "/ws") => http::respond_json(&mut stream, 405, &serde_json::json!({ "error": "method" })),
        _ => http::respond_json(&mut stream, 404, &serde_json::json!({ "error": "notFound" })),
    }
}

fn pair(shared: &Shared, stream: &mut TcpStream, request: &Request) -> io::Result<()> {
    let Ok(body) = serde_json::from_slice::<PairRequest>(&request.body) else {
        return http::respond_json(stream, 400, &serde_json::json!({ "error": "badRequest" }));
    };
    match shared.pairing.pair(&body, Instant::now()) {
        Ok(token) => http::respond_json(stream, 200, &serde_json::to_value(PairResponse { token }).unwrap_or_default()),
        Err(PairError::NoPin) => http::respond_json(stream, 410, &serde_json::json!({ "error": "noPin" })),
        Err(PairError::WrongPin) => http::respond_json(stream, 403, &serde_json::json!({ "error": "wrongPin" })),
        Err(PairError::Locked(seconds)) => http::respond_json(stream, 429, &serde_json::json!({ "error": "locked", "seconds": seconds })),
        Err(PairError::Invalid) => http::respond_json(stream, 400, &serde_json::json!({ "error": "badRequest" })),
        Err(PairError::Storage(e)) => {
            warn!("Could not save the paired controller: {e}");
            http::respond_json(stream, 503, &serde_json::json!({ "error": "storage" }))
        }
    }
}

fn websocket(shared: &Arc<Shared>, mut stream: TcpStream, request: &Request) -> io::Result<()> {
    let who = match request.bearer().and_then(|token| shared.pairing.authenticate(token)) {
        Some(who) => who,
        None => return http::respond_json(&mut stream, 401, &serde_json::json!({ "error": "unauthorized" })),
    };
    let Some(key) = request.header("sec-websocket-key").filter(|_| request.wants_websocket()) else {
        return http::respond_json(&mut stream, 400, &serde_json::json!({ "error": "badRequest" }));
    };
    http::respond_switching(&mut stream, key)?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;

    let (outbox, inbox) = mpsc::sync_channel(OUTBOX);
    let number = shared.next_outbox.fetch_add(1, Ordering::SeqCst);
    shared.outboxes.lock().unwrap().push((number, outbox));
    info!("Soundify Connect: {} controls this device", who.name);
    let result = session(shared, stream, &who, &inbox);
    shared.outboxes.lock().unwrap().retain(|(n, _)| *n != number);
    info!("Soundify Connect: {} left", who.name);
    result
}

fn session(shared: &Shared, stream: TcpStream, who: &Trusted, inbox: &Receiver<ServerMessage>) -> io::Result<()> {
    let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let mut socket = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    send(&mut socket, &ServerMessage::Hello { device: shared.host.info() })?;
    send(&mut socket, &ServerMessage::State { state: shared.host.state() })?;
    let mut last_ping = Instant::now();
    while !shared.stop.load(Ordering::SeqCst) {
        loop {
            match inbox.try_recv() {
                Ok(message) => send(&mut socket, &message)?,
                Err(mpsc::TryRecvError::Empty) => break,
                // The server dropped this controller (too slow, or a new state could not be queued).
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            socket.send(Message::Ping(Vec::new().into())).map_err(io::Error::other)?;
            last_ping = Instant::now();
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let answer = answer(shared, who, text.as_str());
                send(&mut socket, &answer)?;
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok(()),
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    let _ = socket.close(None);
    Ok(())
}

/// The `ack` for whatever the controller sent.
fn answer(shared: &Shared, who: &Trusted, text: &str) -> ServerMessage {
    let Ok(ClientMessage::Cmd { id, command }) = serde_json::from_str::<ClientMessage>(text) else {
        return ServerMessage::Ack { id: 0, ok: false, error: Some("unknown message".into()) };
    };
    let outcome = command.validate().and_then(|()| shared.host.command(who, command));
    match outcome {
        Ok(()) => ServerMessage::Ack { id, ok: true, error: None },
        Err(error) => ServerMessage::Ack { id, ok: false, error: Some(error) },
    }
}

fn send(socket: &mut WebSocket<TcpStream>, message: &ServerMessage) -> io::Result<()> {
    let text = serde_json::to_string(message).map_err(io::Error::other)?;
    socket.send(Message::text(text)).map_err(io::Error::other)
}
