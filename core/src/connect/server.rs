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
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message, WebSocket};

use super::http::{self, Request};
use super::pairing::{self, PairError, Pairing, Trusted};
use super::resilient::Resilient;
use super::stats::{self, short};
use super::protocol::{BASE, ClientMessage, Command, DeviceInfo, PING_EVERY, PairRequest, PairResponse, PlayerState, QueueInfo, SILENCE_LIMIT, ServerMessage};

/// Connections served at once (controllers plus whatever else knocks).
pub const MAX_CONNECTIONS: usize = 16;
/// The most one message may weigh: a queue of [`super::protocol::MAX_QUEUE`] files.
const MAX_MESSAGE: usize = 2 * 1024 * 1024;

/// What the app plays with: the server only knows how to ask.
pub trait Host: Send + Sync {
    fn info(&self) -> DeviceInfo;
    /// The state as it is now (sent to a controller as soon as it connects).
    fn state(&self) -> PlayerState;
    /// The playlist that plays now (sent to a controller after the state, when there is one).
    fn queue(&self) -> Option<QueueInfo> {
        None
    }
    /// Does what the controller asked. `Err` is the reason, which goes back in the `ack`.
    fn command(&self, who: &Trusted, command: Command) -> Result<(), String>;
    /// A controller connected and said where its own server listens: a chance to find it back (when mDNS did not).
    fn controller_seen(&self, _who: &Trusted, _ip: std::net::IpAddr, _port: u16) {}
}

/// What is waiting to go out to ONE controller: only the LAST state and the LAST playlist. A controller that reads slowly (or a device that
/// publishes a hundred states a second) gets the latest, not a queue to overflow: an old state is of no use once there is a newer one, and a
/// controller was once dropped over and over because a flood filled a queue of 64.
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Pending>,
}

#[derive(Default)]
struct Pending {
    state: Option<PlayerState>,
    queue: Option<Option<QueueInfo>>,
}

impl Mailbox {
    fn put_state(&self, state: &PlayerState) {
        self.pending.lock().unwrap().state = Some(state.clone());
    }

    fn put_queue(&self, queue: &Option<QueueInfo>) {
        self.pending.lock().unwrap().queue = Some(queue.clone());
    }

    /// What is waiting, and the mailbox is empty again.
    fn take(&self) -> Pending {
        std::mem::take(&mut *self.pending.lock().unwrap())
    }
}

/// One controller's mailbox, with the number it is taken out by when it leaves.
struct Outbox {
    number: u64,
    mailbox: Arc<Mailbox>,
}

struct Shared {
    host: Arc<dyn Host>,
    pairing: Arc<Pairing>,
    stop: AtomicBool,
    connections: AtomicUsize,
    /// One per open WebSocket, with the number it is taken out by when it ends.
    outboxes: Mutex<Vec<Outbox>>,
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
        // Blocking: the thread sleeps until someone connects (polling woke it 25 times a second, also on a phone in a pocket).
        // `stop` wakes it with a connection of its own.
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))?;
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
        for outbox in self.shared.outboxes.lock().unwrap().iter() {
            outbox.mailbox.put_state(state);
            stats::states_sent(1);
        }
    }

    /// Tells every controller which playlist is playing now.
    pub fn publish_queue(&self, queue: &Option<QueueInfo>) {
        for outbox in self.shared.outboxes.lock().unwrap().iter() {
            outbox.mailbox.put_queue(queue);
            stats::queues_sent(1);
        }
    }

    /// Controllers connected right now.
    pub fn controllers(&self) -> usize {
        self.shared.outboxes.lock().unwrap().len()
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.accepting.lock().unwrap().take() {
            // `accept` is waiting: a connection to itself lets it see the flag. If even that fails (a firewall in
            // the way), the thread is left behind rather than waiting for it forever; it ends with the next caller.
            let wake = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
            if TcpStream::connect_timeout(&wake, Duration::from_secs(1)).is_ok() {
                let _ = thread.join();
            }
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
            Ok(_) if shared.stop.load(Ordering::SeqCst) => break,
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
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                warn!("Connect accept failed: {e}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn serve(shared: &Arc<Shared>, mut stream: TcpStream) -> io::Result<()> {
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
    // This same app (its announcement read under another name): it does not control itself.
    if body.controller_id == shared.host.info().device_id {
        return http::respond_json(stream, 400, &serde_json::json!({ "error": "badRequest" }));
    }
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
    let mut who = match request.bearer().and_then(|token| shared.pairing.authenticate(token)) {
        Some(who) => who,
        None => return http::respond_json(&mut stream, 401, &serde_json::json!({ "error": "unauthorized" })),
    };
    // Paired with itself by an older version: refused, and the entry let go of.
    if who.controller_id == shared.host.info().device_id {
        let _ = shared.pairing.forget(&who.controller_id);
        return http::respond_json(&mut stream, 401, &serde_json::json!({ "error": "unauthorized" }));
    }
    let Some(key) = request.header("sec-websocket-key").filter(|_| request.wants_websocket()) else {
        return http::respond_json(&mut stream, 400, &serde_json::json!({ "error": "badRequest" }));
    };
    http::respond_switching(&mut stream, key)?;
    // The name the controller has now, if it says one: the stored one (the one it was paired under) follows it.
    if let Some(name) = request.header("x-soundify-name").and_then(super::protocol::decode_name)
        && shared.pairing.rename(&who.controller_id, &name)
    {
        info!("Soundify Connect: {} #{} is now called {name}", who.name, short(&who.controller_id));
        who.name = name;
    }
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    if let (Some(port), Ok(peer)) = (request.header("x-soundify-port").and_then(|p| p.trim().parse::<u16>().ok()), stream.peer_addr()) {
        shared.host.controller_seen(&who, peer.ip(), port);
    }

    let mailbox = Arc::new(Mailbox::default());
    let number = shared.next_outbox.fetch_add(1, Ordering::SeqCst);
    let now = {
        let mut outboxes = shared.outboxes.lock().unwrap();
        outboxes.push(Outbox { number, mailbox: mailbox.clone() });
        outboxes.len()
    };
    let (from, started) = (stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "?".into()), Instant::now());
    stats::accepted(1);
    info!("Soundify Connect: IN  {} #{} controls this device (from {from}, session {number}, {now} controllers now)", who.name, short(&who.controller_id));
    shared.pairing.touch(&who.controller_id, pairing::now_secs());
    let result = session(shared, stream, &who, &mailbox);
    shared.pairing.touch(&who.controller_id, pairing::now_secs());
    shared.outboxes.lock().unwrap().retain(|o| o.number != number);
    stats::closed(1);
    let lasted = format!("after {:.1}s", started.elapsed().as_secs_f32());
    match &result {
        Ok(reason) => info!("Soundify Connect: IN  {} #{} left ({reason}) session {number} {lasted}", who.name, short(&who.controller_id)),
        Err(e) => info!("Soundify Connect: IN  {} #{} left (error: {e}) session {number} {lasted}", who.name, short(&who.controller_id)),
    }
    result.map(|_| ())
}

/// Serves one controller until it ends; `Ok` says why.
fn session(shared: &Shared, stream: TcpStream, who: &Trusted, mailbox: &Mailbox) -> io::Result<&'static str> {
    let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let mut socket = WebSocket::from_raw_socket(Resilient::new(stream), Role::Server, Some(config));
    send(&mut socket, &ServerMessage::Hello { device: shared.host.info() })?;
    send(&mut socket, &ServerMessage::State { state: shared.host.state() })?;
    if let Some(queue) = shared.host.queue() {
        send(&mut socket, &ServerMessage::Queue { queue: Some(queue) })?;
    }
    let mut last_ping = Instant::now();
    let mut last_heard = Instant::now();
    while !shared.stop.load(Ordering::SeqCst) {
        // The latest state and playlist that were published since the last look (each one replaces the one before).
        let pending = mailbox.take();
        if let Some(state) = pending.state {
            send(&mut socket, &ServerMessage::State { state })?;
        }
        if let Some(queue) = pending.queue {
            send(&mut socket, &ServerMessage::Queue { queue })?;
        }
        if last_heard.elapsed() >= SILENCE_LIMIT {
            return Ok("it stopped answering");
        }
        if last_ping.elapsed() >= PING_EVERY {
            socket.send(Message::Ping(Vec::new().into())).map_err(io::Error::other)?;
            last_ping = Instant::now();
            // Still connected: it counts as seen (so a connection that lasts for weeks is not let go of).
            shared.pairing.touch(&who.controller_id, pairing::now_secs());
        }
        let read = socket.read();
        if !matches!(&read, Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)) {
            last_heard = Instant::now();
        }
        match read {
            Ok(Message::Text(text)) => {
                stats::msgs_in(1);
                let answer = answer(shared, who, text.as_str());
                send(&mut socket, &answer)?;
            }
            Ok(Message::Close(_)) => return Ok("it closed the connection"),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok("connection closed"),
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    let _ = socket.close(None);
    Ok("this device stopped")
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

fn send(socket: &mut WebSocket<Resilient<TcpStream>>, message: &ServerMessage) -> io::Result<()> {
    stats::msgs_out(1);
    let text = serde_json::to_string(message).map_err(io::Error::other)?;
    socket.send(Message::text(text)).map_err(io::Error::other)
}

#[cfg(test)]
mod mailbox_tests {
    use super::*;

    fn state(position: f64) -> PlayerState {
        PlayerState { position, ..PlayerState::default() }
    }

    #[test]
    fn only_the_latest_state_and_playlist_wait_for_a_slow_controller() {
        let mailbox = Mailbox::default();
        for position in 0..1000 {
            mailbox.put_state(&state(position as f64));
        }
        mailbox.put_queue(&None);
        let pending = mailbox.take();
        assert_eq!(pending.state.map(|s| s.position), Some(999.0));
        assert!(matches!(pending.queue, Some(None)));
        // Taken: nothing is sent twice.
        let again = mailbox.take();
        assert!(again.state.is_none() && again.queue.is_none());
    }
}
