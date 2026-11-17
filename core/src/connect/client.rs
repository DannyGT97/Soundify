//! The controller's side: ask a device who it is, pair with it, and keep a
//! WebSocket open to it (state comes in, commands go out). All but `/info` over TLS, with the device's certificate
//! pinned (`tls::Expect`).

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::pairing::PinExchange;
use super::protocol::{BASE, ClientMessage, Command, DeviceInfo, PING_EVERY, PROTOCOL, PairConfirm, PairRequest, PairResponse, PairStarted, SILENCE_LIMIT, ServerMessage};
use super::resilient::Resilient;
use super::tls::{self, Conn, Expect};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// What a device answers to `/info` or `/pair` is a few hundred bytes.
const MAX_ANSWER: usize = 64 * 1024;
const MAX_MESSAGE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Nothing answered at that address (or it is not a Soundify).
    Unreachable(String),
    /// The device answered no: its HTTP status and the `error` it gave.
    Refused { status: u16, error: String, seconds: Option<u64> },
    /// It answered something this version does not understand.
    Protocol(String),
    /// Its certificate is not the one pinned when it was paired (reinstalled, or somebody in between), or it could not
    /// prove it knew the PIN.
    Untrusted,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Unreachable(e) => write!(f, "unreachable: {e}"),
            ClientError::Refused { status, error, .. } => write!(f, "refused ({status}): {error}"),
            ClientError::Protocol(e) => write!(f, "protocol: {e}"),
            ClientError::Untrusted => write!(f, "untrusted: {}", tls::UNTRUSTED),
        }
    }
}

/// `GET /info`. A device with another protocol version is refused here, not later.
pub fn fetch_info(addr: SocketAddr) -> Result<DeviceInfo, ClientError> {
    let (status, body) = request_plain(addr, "/info")?;
    refuse_unless_ok(status, &body)?;
    let info: DeviceInfo = serde_json::from_slice(&body).map_err(|e| ClientError::Protocol(e.to_string()))?;
    // Another version: said as a refusal the screen can word («update it» / «update this one»), not as gibberish.
    if info.v != PROTOCOL {
        let error = if info.v < PROTOCOL { "outdated" } else { "newer" };
        return Err(ClientError::Refused { status: 426, error: error.into(), seconds: None });
    }
    Ok(info)
}

/// A pairing that worked: the token, and the fingerprint of the device's certificate to pin from now on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub token: String,
    pub fingerprint: String,
}

/// Pairs with the PIN the other device shows (empty for an open device).
pub fn pair(addr: SocketAddr, controller_id: &str, name: &str, pin: &str) -> Result<Paired, ClientError> {
    pair_as(addr, controller_id, name, "", pin)
}

/// `kind`: what this device is (`desktop`, `phone`, `tablet`), so the other one draws the right icon.
///
/// With a PIN: SPAKE2 (the PIN never travels), then both sides prove they share the key AND saw the same certificate.
/// Somebody in between shows another certificate and fails that proof, whatever they relay.
pub fn pair_as(addr: SocketAddr, controller_id: &str, name: &str, kind: &str, pin: &str) -> Result<Paired, ClientError> {
    let pin = pin.trim();
    let (exchange, spake) = if pin.is_empty() { (None, String::new()) } else {
        let (exchange, spake) = PinExchange::start(pin);
        (Some(exchange), spake)
    };
    let body = serde_json::to_vec(&PairRequest { controller_id: controller_id.into(), name: name.into(), kind: kind.into(), spake }).map_err(|e| ClientError::Protocol(e.to_string()))?;
    let (status, answer, seen) = request(addr, &Expect::Learn, "POST", "/pair/start", Some(&body))?;
    refuse_unless_ok(status, &answer)?;
    let started: PairStarted = serde_json::from_slice(&answer).map_err(|e| ClientError::Protocol(e.to_string()))?;
    if let Some(token) = started.token {
        // An open device: no PIN to prove anything with; its certificate is pinned as it was first seen.
        return Ok(Paired { token, fingerprint: seen });
    }
    let (Some(exchange), Some(session), Some(answer)) = (exchange, started.session, started.spake) else {
        return Err(ClientError::Refused { status: 403, error: "wrongPin".into(), seconds: None });
    };
    let (confirm, expected) = exchange.finish(&answer, controller_id, &seen).map_err(|_| ClientError::Protocol("bad SPAKE2 answer".into()))?;
    let body = serde_json::to_vec(&PairConfirm { session, confirm }).map_err(|e| ClientError::Protocol(e.to_string()))?;
    // The same certificate as the first half, or nothing.
    let (status, answer, _) = request(addr, &Expect::Pinned(seen.clone()), "POST", "/pair/finish", Some(&body))?;
    refuse_unless_ok(status, &answer)?;
    let response: PairResponse = serde_json::from_slice(&answer).map_err(|e| ClientError::Protocol(e.to_string()))?;
    if !tls::same(response.confirm.as_bytes(), expected.as_bytes()) {
        return Err(ClientError::Untrusted);
    }
    Ok(Paired { token: response.token, fingerprint: seen })
}

fn refuse_unless_ok(status: u16, body: &[u8]) -> Result<(), ClientError> {
    if status == 200 {
        return Ok(());
    }
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    Err(ClientError::Refused {
        status,
        error: json.get("error").and_then(|e| e.as_str()).unwrap_or("error").to_string(),
        seconds: json.get("seconds").and_then(|s| s.as_u64()),
    })
}

fn io_error(e: io::Error) -> ClientError {
    if tls::is_untrusted(&e) { ClientError::Untrusted } else { ClientError::Unreachable(e.to_string()) }
}

/// `GET /info`, the one request in the clear.
fn request_plain(addr: SocketAddr, route: &str) -> Result<(u16, Vec<u8>), ClientError> {
    let unreachable = |e: io::Error| ClientError::Unreachable(e.to_string());
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
    exchange(&mut Conn::Plain(Resilient::new(stream)), addr, "GET", route, &[])
}

/// One request over TLS, one answer, connection closed: (status, body, the fingerprint the device showed).
fn request(addr: SocketAddr, expect: &Expect, method: &str, route: &str, body: Option<&[u8]>) -> Result<(u16, Vec<u8>, String), ClientError> {
    let (mut conn, seen) = tls::connect(addr, expect, CONNECT_TIMEOUT, IO_TIMEOUT).map_err(io_error)?;
    let (status, answer) = exchange(&mut conn, addr, method, route, body.unwrap_or_default())?;
    Ok((status, answer, seen))
}

fn exchange(conn: &mut Conn, addr: SocketAddr, method: &str, route: &str, body: &[u8]) -> Result<(u16, Vec<u8>), ClientError> {
    let unreachable = |e: io::Error| ClientError::Unreachable(e.to_string());
    let head = format!("{method} {BASE}{route} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body)).and_then(|()| conn.flush()).map_err(unreachable)?;
    let mut answer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = match conn.read(&mut chunk) {
            Ok(read) => read,
            // A TLS peer that closes without saying so (a plain close after the answer) is the end of the answer.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => 0,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(unreachable(e)),
        };
        if read == 0 {
            break;
        }
        answer.extend_from_slice(&chunk[..read]);
        if answer.len() > MAX_ANSWER {
            return Err(ClientError::Protocol("the answer is too long".into()));
        }
    }
    let end = answer.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| ClientError::Protocol("no HTTP answer".into()))?;
    let head = std::str::from_utf8(&answer[..end]).map_err(|_| ClientError::Protocol("the answer is not text".into()))?;
    let status = head.lines().next().and_then(|line| line.split(' ').nth(1)).and_then(|code| code.parse::<u16>().ok()).ok_or_else(|| ClientError::Protocol("no HTTP status".into()))?;
    Ok((status, answer[end + 4..].to_vec()))
}

/// What the connection to a device tells the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Message(ServerMessage),
    /// The connection is over (why, if it is known).
    Closed(Option<String>),
}

/// An open WebSocket to a device.
pub struct Session {
    /// The fingerprint of the device's certificate (the pinned one, or the one learnt now).
    fingerprint: String,
    outgoing: Sender<String>,
    next_id: AtomicU64,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    /// Connects and does the handshake: a wrong token fails here, not later.
    pub fn open(addr: SocketAddr, token: &str, expect: &Expect, on_event: impl FnMut(Event) + Send + 'static) -> Result<Session, ClientError> {
        Self::open_with(addr, token, expect, None, None, on_event)
    }

    /// `own_port`: where this device's own server listens, so the other one can find this one back.
    pub fn open_with(addr: SocketAddr, token: &str, expect: &Expect, own_port: Option<u16>, own_name: Option<&str>, mut on_event: impl FnMut(Event) + Send + 'static) -> Result<Session, ClientError> {
        let unreachable = |e: io::Error| ClientError::Unreachable(e.to_string());
        let (stream, fingerprint) = tls::connect(addr, expect, CONNECT_TIMEOUT, IO_TIMEOUT).map_err(io_error)?;
        // `ws://`: the TLS is already underneath; tungstenite only does the WebSocket.
        let mut request = format!("ws://{addr}{BASE}/ws").into_client_request().map_err(|e| ClientError::Protocol(e.to_string()))?;
        let header = format!("Bearer {token}").parse().map_err(|_| ClientError::Protocol("the token is not a header".into()))?;
        request.headers_mut().insert("Authorization", header);
        if let Some(port) = own_port.and_then(|p| p.to_string().parse().ok()) {
            request.headers_mut().insert("X-Soundify-Port", port);
        }
        // Its name as it is NOW: a device paired under a default name (or renamed since) is shown by the one it has.
        if let Some(name) = own_name.and_then(|n| super::protocol::encode_name(n).parse().ok()) {
            request.headers_mut().insert("X-Soundify-Name", name);
        }
        let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
        let (mut socket, _) = tungstenite::client::client_with_config(request, stream, Some(config)).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                ClientError::Refused { status: response.status().as_u16(), error: "unauthorized".into(), seconds: None }
            }
            other => ClientError::Protocol(other.to_string()),
        })?;
        socket.get_ref().tcp().set_read_timeout(Some(Duration::from_millis(100))).map_err(unreachable)?;

        let (outgoing, queue) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("connect-session".into())
            .spawn(move || {
                let reason = run(&mut socket, &queue, &flag, &mut on_event);
                on_event(Event::Closed(reason));
            })
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        Ok(Session { fingerprint, outgoing, next_id: AtomicU64::new(1), stop, thread: Mutex::new(Some(thread)) })
    }

    /// The fingerprint of the device's certificate.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Sends a command; the id comes back in the `ack`.
    pub fn send(&self, command: Command) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(text) = serde_json::to_string(&ClientMessage::Cmd { id, command }) {
            let _ = self.outgoing.send(text);
        }
        id
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.lock().unwrap().take() {
            // Dropped from inside its own thread (the last event of a closed session): nothing to wait for.
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

fn run(socket: &mut WebSocket<Conn>, queue: &mpsc::Receiver<String>, stop: &AtomicBool, on_event: &mut impl FnMut(Event)) -> Option<String> {
    let mut last_ping = Instant::now();
    let mut last_heard = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        while let Ok(text) = queue.try_recv() {
            if let Err(e) = socket.send(Message::text(text)) {
                return Some(e.to_string());
            }
        }
        // A phone that locks, or a Wi-Fi that drops, closes nothing: the connection is only known to be dead because nothing arrives.
        if last_heard.elapsed() >= SILENCE_LIMIT {
            return Some("no answer".into());
        }
        if last_ping.elapsed() >= PING_EVERY {
            if let Err(e) = socket.send(Message::Ping(Vec::new().into())) {
                return Some(e.to_string());
            }
            last_ping = Instant::now();
        }
        let read = socket.read();
        if !matches!(&read, Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)) {
            last_heard = Instant::now();
        }
        match read {
            Ok(Message::Text(text)) => match serde_json::from_str::<ServerMessage>(text.as_str()) {
                Ok(message) => on_event(Event::Message(message)),
                Err(e) => return Some(format!("bad message: {e}")),
            },
            Ok(Message::Close(_)) => return None,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return None,
            Err(e) => return Some(e.to_string()),
        }
    }
    let _ = socket.close(None);
    None
}
