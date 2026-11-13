//! The controller's side: ask a device who it is, pair with it, and keep a
//! WebSocket open to it (state comes in, commands go out).

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::protocol::{BASE, ClientMessage, Command, DeviceInfo, PROTOCOL, PairRequest, PairResponse, ServerMessage};

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
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Unreachable(e) => write!(f, "unreachable: {e}"),
            ClientError::Refused { status, error, .. } => write!(f, "refused ({status}): {error}"),
            ClientError::Protocol(e) => write!(f, "protocol: {e}"),
        }
    }
}

/// `GET /info`. A device with another protocol version is refused here, not later.
pub fn fetch_info(addr: SocketAddr) -> Result<DeviceInfo, ClientError> {
    let (status, body) = request(addr, "GET", "/info", None)?;
    refuse_unless_ok(status, &body)?;
    let info: DeviceInfo = serde_json::from_slice(&body).map_err(|e| ClientError::Protocol(e.to_string()))?;
    if info.v != PROTOCOL {
        return Err(ClientError::Protocol(format!("protocol {} (this app speaks {PROTOCOL})", info.v)));
    }
    Ok(info)
}

/// `POST /pair`: the PIN the other device shows, for the token that lets this one in.
pub fn pair(addr: SocketAddr, controller_id: &str, name: &str, pin: &str) -> Result<String, ClientError> {
    let body = serde_json::to_vec(&PairRequest { controller_id: controller_id.into(), name: name.into(), pin: pin.into() }).map_err(|e| ClientError::Protocol(e.to_string()))?;
    let (status, answer) = request(addr, "POST", "/pair", Some(&body))?;
    refuse_unless_ok(status, &answer)?;
    let response: PairResponse = serde_json::from_slice(&answer).map_err(|e| ClientError::Protocol(e.to_string()))?;
    Ok(response.token)
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

/// One request, one answer, connection closed: (status, body).
fn request(addr: SocketAddr, method: &str, route: &str, body: Option<&[u8]>) -> Result<(u16, Vec<u8>), ClientError> {
    let unreachable = |e: io::Error| ClientError::Unreachable(e.to_string());
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
    let body = body.unwrap_or_default();
    let head = format!("{method} {BASE}{route} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body)).map_err(unreachable)?;
    let mut answer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).map_err(unreachable)?;
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
    outgoing: Sender<String>,
    next_id: AtomicU64,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    /// Connects and does the handshake: a wrong token fails here, not later.
    pub fn open(addr: SocketAddr, token: &str, mut on_event: impl FnMut(Event) + Send + 'static) -> Result<Session, ClientError> {
        let unreachable = |e: io::Error| ClientError::Unreachable(e.to_string());
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(unreachable)?;
        stream.set_nodelay(true).map_err(unreachable)?;
        let mut request = format!("ws://{addr}{BASE}/ws").into_client_request().map_err(|e| ClientError::Protocol(e.to_string()))?;
        let header = format!("Bearer {token}").parse().map_err(|_| ClientError::Protocol("the token is not a header".into()))?;
        request.headers_mut().insert("Authorization", header);
        let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
        let (mut socket, _) = tungstenite::client::client_with_config(request, stream, Some(config)).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                ClientError::Refused { status: response.status().as_u16(), error: "unauthorized".into(), seconds: None }
            }
            other => ClientError::Protocol(other.to_string()),
        })?;
        socket.get_ref().set_read_timeout(Some(Duration::from_millis(100))).map_err(unreachable)?;

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
        Ok(Session { outgoing, next_id: AtomicU64::new(1), stop, thread: Mutex::new(Some(thread)) })
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

fn run(socket: &mut WebSocket<TcpStream>, queue: &mpsc::Receiver<String>, stop: &AtomicBool, on_event: &mut impl FnMut(Event)) -> Option<String> {
    while !stop.load(Ordering::SeqCst) {
        while let Ok(text) = queue.try_recv() {
            if let Err(e) = socket.send(Message::text(text)) {
                return Some(e.to_string());
            }
        }
        match socket.read() {
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
