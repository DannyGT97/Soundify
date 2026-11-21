//! Two Soundify apps in one test: the server with a fake player, the client the way the
//! controller uses it, over a real socket on localhost.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::client::{self, ClientError, Event, Session};
use super::pairing::{Pairing, Trusted};
use super::protocol::*;
use super::server::{Host, Server};
use super::tls::{self, Conn, Expect, TlsIdentity};
use crate::Location;
use crate::storage::{MemoryStorage, Storage};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct FakeHost {
    state: Mutex<PlayerState>,
    received: Mutex<Vec<(String, Command)>>,
    /// The controllers whose connection ended.
    left: Mutex<Vec<String>>,
}

impl Host for FakeHost {
    fn info(&self) -> DeviceInfo {
        DeviceInfo { v: PROTOCOL, device_id: "player-1".into(), name: "Salón".into(), kind: "desktop".into(), open: false }
    }

    fn state(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    fn command(&self, who: &Trusted, command: Command) -> Result<(), String> {
        if command == Command::Pause {
            return Err("nothing is playing".into());
        }
        self.received.lock().unwrap().push((who.name.clone(), command));
        Ok(())
    }

    fn controller_left(&self, who: &Trusted) {
        self.left.lock().unwrap().push(who.controller_id.clone());
    }
}

struct Rig {
    server: Server,
    host: Arc<FakeHost>,
    pairing: Arc<Pairing>,
    addr: SocketAddr,
    /// The fingerprint of the player's certificate (what a paired controller pins).
    fingerprint: String,
}

impl Rig {
    fn pinned(&self) -> Expect {
        Expect::Pinned(self.fingerprint.clone())
    }
}

fn rig() -> Rig {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    let pairing = Arc::new(Pairing::load(storage, Location::path("/data/trusted.json")));
    // These tests are about the PIN: the strict way (the open one is tested on its own).
    pairing.set_open(false);
    let host = Arc::new(FakeHost::default());
    let server = Server::start(0, host.clone(), pairing.clone(), Arc::new(TlsIdentity::ephemeral())).unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
    let fingerprint = server.fingerprint().to_string();
    Rig { server, host, pairing, addr, fingerprint }
}

/// Pairs a controller the way the user does: reads the PIN off the player, types it on the controller.
fn paired(rig: &Rig) -> String {
    let pin = rig.pairing.new_pin(Instant::now());
    let paired = client::pair(rig.addr, "phone-1", "Móvil", &pin).unwrap();
    // The certificate it learnt while pairing is the player's own (the PIN proved it).
    assert_eq!(paired.fingerprint, rig.fingerprint);
    paired.token
}

fn open(rig: &Rig, token: &str) -> (Session, Receiver<Event>) {
    let (events, received) = mpsc::channel();
    let session = Session::open(rig.addr, token, &rig.pinned(), move |event| {
        let _ = events.send(event);
    })
    .unwrap();
    (session, received)
}

fn next_message(events: &Receiver<Event>) -> ServerMessage {
    match events.recv_timeout(WAIT).expect("a message arrives") {
        Event::Message(message) => message,
        Event::Closed(reason) => panic!("the connection closed: {reason:?}"),
    }
}

/// Reads messages until one satisfies `wanted` (state updates may come in between).
fn wait_for(events: &Receiver<Event>, wanted: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if let Ok(Event::Message(message)) = events.recv_timeout(Duration::from_millis(200))
            && wanted(&message)
        {
            return message;
        }
    }
    panic!("the message did not come");
}

#[test]
fn an_open_device_gives_a_token_without_a_pin_over_the_network_and_says_so_in_its_info() {
    let rig = rig();
    assert!(client::pair(rig.addr, "c", "C", "").is_err());
    rig.pairing.set_open(true);
    let paired = client::pair(rig.addr, "phone-1", "Móvil", "").unwrap();
    assert_eq!(paired.fingerprint, rig.fingerprint);
    assert!(Session::open(rig.addr, &paired.token, &Expect::Pinned(paired.fingerprint), |_| {}).is_ok());
}

#[test]
fn a_device_does_not_pair_with_itself() {
    let rig = rig();
    rig.pairing.set_open(true);
    assert!(client::pair(rig.addr, "player-1", "Salón", "").is_err());
    assert!(rig.pairing.trusted().is_empty());
}

#[test]
fn anybody_can_ask_who_it_is() {
    let rig = rig();
    let info = client::fetch_info(rig.addr).unwrap();
    assert_eq!((info.name.as_str(), info.device_id.as_str()), ("Salón", "player-1"));
}

#[test]
fn the_pin_trades_for_a_token_and_the_token_opens_the_websocket() {
    let rig = rig();
    let token = paired(&rig);
    let (_session, events) = open(&rig, &token);
    assert!(matches!(next_message(&events), ServerMessage::Hello { device } if device.name == "Salón"));
    assert!(matches!(next_message(&events), ServerMessage::State { state } if state.now_playing.is_none()));
}

#[test]
fn a_wrong_pin_a_missing_pin_and_a_wrong_token_are_refused() {
    let rig = rig();
    // Nobody asked for a PIN.
    assert!(matches!(client::pair(rig.addr, "c", "C", "123456"), Err(ClientError::Refused { status: 410, .. })));
    let pin = rig.pairing.new_pin(Instant::now());
    let wrong = if pin == "000000" { "111111" } else { "000000" };
    assert!(matches!(client::pair(rig.addr, "c", "C", wrong), Err(ClientError::Refused { status: 403, .. })));
    // A token nobody gave.
    assert!(matches!(Session::open(rig.addr, "not-a-token", &rig.pinned(), |_| {}), Err(ClientError::Refused { status: 401, .. })));
}

#[test]
fn five_wrong_pins_lock_the_pairing_over_the_network() {
    let rig = rig();
    let pin = rig.pairing.new_pin(Instant::now());
    let wrong = if pin == "000000" { "111111" } else { "000000" };
    for _ in 0..4 {
        let _ = client::pair(rig.addr, "c", "C", wrong);
    }
    assert!(matches!(client::pair(rig.addr, "c", "C", wrong), Err(ClientError::Refused { status: 429, .. })));
    assert!(matches!(client::pair(rig.addr, "c", "C", &pin), Err(ClientError::Refused { status: 429, .. })));
}

#[test]
fn a_command_reaches_the_player_and_is_acknowledged() {
    let rig = rig();
    let token = paired(&rig);
    let (session, events) = open(&rig, &token);
    let id = session.send(Command::Seek { seconds: 42.0 });
    let ack = wait_for(&events, |m| matches!(m, ServerMessage::Ack { .. }));
    assert_eq!(ack, ServerMessage::Ack { id, ok: true, error: None });
    assert_eq!(rig.host.received.lock().unwrap().as_slice(), [("Móvil".to_string(), Command::Seek { seconds: 42.0 })]);
}

#[test]
fn a_refused_command_comes_back_with_its_reason_and_a_nonsense_one_never_reaches_the_player() {
    let rig = rig();
    let token = paired(&rig);
    let (session, events) = open(&rig, &token);
    let id = session.send(Command::Pause);
    assert_eq!(wait_for(&events, |m| matches!(m, ServerMessage::Ack { .. })), ServerMessage::Ack { id, ok: false, error: Some("nothing is playing".into()) });
    let bad = session.send(Command::Volume { volume: 7.0 });
    assert!(matches!(wait_for(&events, |m| matches!(m, ServerMessage::Ack { id, .. } if *id == bad)), ServerMessage::Ack { ok: false, .. }));
    assert!(rig.host.received.lock().unwrap().is_empty());
}

#[test]
fn what_the_player_publishes_reaches_every_controller() {
    let rig = rig();
    let token = paired(&rig);
    let (_one, first) = open(&rig, &token);
    let (_two, second) = open(&rig, &token);
    let playing = PlayerState {
        now_playing: Some(NowPlaying { video_id: "abc".into(), title: "Canción".into(), playlist: "Mix".into(), format: "MP3".into(), video: false }),
        playing: true,
        position: 12.0,
        duration: 200.0,
        ..PlayerState::default()
    };
    // Both are in (their Hello arrived) before it is published.
    wait_for(&first, |m| matches!(m, ServerMessage::Hello { .. }));
    wait_for(&second, |m| matches!(m, ServerMessage::Hello { .. }));
    rig.server.publish(&playing);
    for events in [&first, &second] {
        let message = wait_for(events, |m| matches!(m, ServerMessage::State { state } if state.playing));
        assert_eq!(message, ServerMessage::State { state: playing.clone() });
    }
}

#[test]
fn a_controller_that_connects_late_gets_the_state_as_it_is() {
    let rig = rig();
    *rig.host.state.lock().unwrap() = PlayerState { playing: true, position: 5.0, ..PlayerState::default() };
    let token = paired(&rig);
    let (_session, events) = open(&rig, &token);
    wait_for(&events, |m| matches!(m, ServerMessage::State { state } if state.playing && state.position == 5.0));
}

#[test]
fn forgetting_a_controller_stops_its_token_from_opening_anything() {
    let rig = rig();
    let token = paired(&rig);
    rig.pairing.forget("phone-1").unwrap();
    assert!(matches!(Session::open(rig.addr, &token, &rig.pinned(), |_| {}), Err(ClientError::Refused { status: 401, .. })));
}

#[test]
fn unknown_routes_and_wrong_methods_are_refused_and_a_huge_body_is_not_read() {
    let rig = rig();
    let raw = |request: &str| raw_bytes(&rig, request).0;
    assert!(raw("GET /nothing HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 404"));
    assert!(raw("DELETE /connect/v1/info HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 405"));
    assert!(raw("POST /connect/v1/pair/start HTTP/1.1\r\nContent-Length: 999999\r\n\r\n").starts_with("HTTP/1.1 400"));
    assert!(raw("garbage\r\n\r\n").starts_with("HTTP/1.1 400"));
    // A WebSocket without a token gets no further than the door.
    assert!(raw("GET /connect/v1/ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").starts_with("HTTP/1.1 401"));
}

/// Asks in the CLEAR, as an app of before TLS (or somebody listening) would.
fn raw_plain(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
}

#[test]
fn in_the_clear_only_who_it_is_is_answered_and_nothing_with_a_secret() {
    let rig = rig();
    assert!(raw_plain(rig.addr, "GET /connect/v1/info HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 200"));
    for request in [
        "POST /connect/v1/pair/start HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        "POST /connect/v1/pair HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        "GET /connect/v1/ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        "GET /connect/v1/media/a?t=00 HTTP/1.1\r\n\r\n",
        "GET /nothing HTTP/1.1\r\n\r\n",
    ] {
        let answer = raw_plain(rig.addr, request);
        assert!(answer.starts_with("HTTP/1.1 426") && answer.contains("\"tls\""), "{request}: {answer}");
    }
}

#[test]
fn a_controller_that_reaches_another_certificate_than_the_paired_one_does_not_connect() {
    let rig = rig();
    let token = paired(&rig);
    // Somebody in between (or the device reinstalled): another certificate at the same address.
    let elsewhere = Expect::Pinned(TlsIdentity::ephemeral().fingerprint().to_string());
    assert!(matches!(Session::open(rig.addr, &token, &elsewhere, |_| {}), Err(ClientError::Untrusted)));
}

#[test]
fn a_token_handed_out_before_tls_opens_nothing() {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    // What 1.2.x wrote: no `tls` mark. Its token travelled in the clear.
    let old = r#"[{"controllerId":"phone-1","name":"Móvil","kind":"phone","tokenHash":"9b0a0a2f3e5d8c7b6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b"}]"#;
    storage.write(&Location::path("/data/trusted.json"), old.as_bytes()).unwrap();
    let pairing = Pairing::load(storage, Location::path("/data/trusted.json"));
    assert!(pairing.trusted().is_empty());
}

#[test]
fn stopping_the_server_closes_the_controllers_sessions() {
    let rig = rig();
    let token = paired(&rig);
    let (_session, events) = open(&rig, &token);
    wait_for(&events, |m| matches!(m, ServerMessage::Hello { .. }));
    rig.server.stop();
    let deadline = Instant::now() + WAIT;
    let mut closed = false;
    while Instant::now() < deadline && !closed {
        closed = matches!(events.recv_timeout(Duration::from_millis(200)), Ok(Event::Closed(_)));
    }
    assert!(closed, "the session was told it is over");
}

#[test]
fn the_player_knows_when_a_controller_is_connected_and_when_it_leaves() {
    let rig = rig();
    let token = paired(&rig);
    assert!(!rig.server.has_controller("phone-1"));
    let (session, events) = open(&rig, &token);
    wait_for(&events, |m| matches!(m, ServerMessage::Hello { .. }));
    assert!(rig.server.has_controller("phone-1"));
    assert!(!rig.server.has_controller("someone-else"));
    drop(session);
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline && rig.host.left.lock().unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.host.left.lock().unwrap().as_slice(), ["phone-1"], "the host was told which controller left");
    assert!(!rig.server.has_controller("phone-1"));
}

#[test]
fn a_controller_that_says_its_new_name_is_stored_under_it() {
    let rig = rig();
    // Paired under a default name (what an old Android app called itself).
    let token = paired(&rig);
    let old = rig.pairing.trusted()[0].name.clone();
    let (_session, events) = {
        let (tx, rx) = mpsc::channel();
        let session = Session::open_with(rig.addr, &token, &rig.pinned(), None, Some("S24+ D 🎵"), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
        (session, rx)
    };
    wait_for(&events, |m| matches!(m, ServerMessage::Hello { .. }));
    assert_ne!(old, "S24+ D 🎵");
    assert_eq!(rig.pairing.trusted()[0].name, "S24+ D 🎵");
}

/// Asks over TLS (the player's certificate pinned) and gives back the whole answer, head and body (bytes: a file is not text).
fn raw_bytes(rig: &Rig, request: &str) -> (String, Vec<u8>) {
    let (mut stream, _): (Conn, String) = tls::connect(rig.addr, &rig.pinned(), WAIT, WAIT).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let split = answer.windows(4).position(|w| w == b"\r\n\r\n").map_or(answer.len(), |at| at + 4);
    (String::from_utf8_lossy(&answer[..split]).into_owned(), answer[split..].to_vec())
}

#[test]
fn a_passed_file_is_read_by_ranges_and_only_with_its_ticket_while_the_pass_lasts() {
    use super::cast::SharedFile;
    let rig = rig();
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("001 - Uno.mp3");
    let cover = dir.path().join("cover.jpg");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&song, &data).unwrap();
    std::fs::write(&cover, b"jpeg").unwrap();
    let tickets = rig.server.tickets();
    let ticket = tickets.grant([
        ("a".to_string(), SharedFile { media: song.clone(), cover: Some(cover) }),
        ("b".to_string(), SharedFile { media: song.clone(), cover: None }),
    ]);
    let get = |path: &str, extra: &str| raw_bytes(&rig, &format!("GET /connect/v1/{path} HTTP/1.1\r\nHost: x\r\n{extra}\r\n"));

    let (head, body) = get(&format!("media/a?t={ticket}"), "Range: bytes=250000-\r\n");
    assert!(head.starts_with("HTTP/1.1 206") && head.contains("Content-Range: bytes 250000-299999/300000"), "{head}");
    assert_eq!(body, &data[250_000..]);
    let (head, body) = get(&format!("media/b?t={ticket}"), "");
    assert!(head.starts_with("HTTP/1.1 200") && head.contains("Content-Type: audio/mpeg"), "{head}");
    assert_eq!(body.len(), data.len());
    let (head, body) = get(&format!("cover/a?t={ticket}"), "");
    assert!(head.starts_with("HTTP/1.1 200") && head.contains("image/jpeg"));
    assert_eq!(body, b"jpeg");
    let (head, _) = raw_bytes(&rig, &format!("HEAD /connect/v1/media/a?t={ticket} HTTP/1.1\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 200") && head.contains("Content-Length: 300000"));

    // Nothing without the ticket, nothing outside the queue, no picture that was not given, no range past the end.
    assert!(get("media/a", "").0.starts_with("HTTP/1.1 404"));
    assert!(get(&format!("media/a?t={}", "0".repeat(64)), "").0.starts_with("HTTP/1.1 404"));
    assert!(get(&format!("media/zzz?t={ticket}"), "").0.starts_with("HTTP/1.1 404"));
    assert!(get(&format!("cover/b?t={ticket}"), "").0.starts_with("HTTP/1.1 404"));
    assert!(get(&format!("media/a?t={ticket}"), "Range: bytes=999999-\r\n").0.starts_with("HTTP/1.1 416"));
    assert!(get(&format!("media/../../secret?t={ticket}"), "").0.starts_with("HTTP/1.1 404"));

    // The pass ends: the same ticket opens nothing.
    tickets.revoke();
    assert!(get(&format!("media/a?t={ticket}"), "").0.starts_with("HTTP/1.1 404"));
}

#[test]
fn a_pass_is_read_from_the_address_it_came_from_not_from_one_it_names() {
    use super::protocol::{CastItem, CastStart};
    let rig = rig();
    let token = paired(&rig);
    let (session, events) = open(&rig, &token);
    let cast = CastStart {
        host: "203.0.113.9".into(),
        port: 40000,
        fingerprint: "0f".repeat(32),
        ticket: "cd".repeat(32),
        playlist: "Rock".into(),
        items: vec![CastItem { video_id: "a".into(), title: "Uno".into(), duration: 1.0, format: "mp3".into(), video: false, cover: false }],
        index: 0,
        position: 0.0,
        playing: true,
        shuffle: false,
        repeat: "off".into(),
    };
    let id = session.send(Command::Cast(Box::new(cast.clone())));
    assert_eq!(wait_for(&events, |m| matches!(m, ServerMessage::Ack { .. })), ServerMessage::Ack { id, ok: true, error: None });
    let received = rig.host.received.lock().unwrap();
    let [(_, Command::Cast(got))] = received.as_slice() else { panic!("{received:?}") };
    assert_eq!(got.host, "127.0.0.1");
    assert_eq!(CastStart { host: cast.host.clone(), ..(**got).clone() }, cast);
}

#[test]
fn the_receiving_device_relays_a_passed_file_one_piece_at_a_time() {
    use super::cast::{self, CastSource, Part, RELAY_PIECE, SharedFile};
    let rig = rig();
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.mp3");
    let cover = dir.path().join("cover.jpg");
    let data: Vec<u8> = (0..(RELAY_PIECE as u32 * 3 / 2)).map(|i| (i % 249) as u8).collect();
    std::fs::write(&song, &data).unwrap();
    std::fs::write(&cover, b"jpeg").unwrap();
    let ticket = rig.server.tickets().grant([("abc_-1".to_string(), SharedFile { media: song, cover: Some(cover) })]);
    let source = CastSource { from: "phone-1".into(), host: "127.0.0.1".into(), port: rig.addr.port(), fingerprint: rig.fingerprint.clone(), ticket: ticket.clone() };
    let header = |relayed: &cast::Relayed, name: &str| relayed.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());

    // The player asks for everything; it gets one piece, and says where it ends.
    let first = cast::relay(&source, Part::Media, "abc_-1", Some("bytes=0-"));
    assert_eq!(first.status, 206);
    assert_eq!(first.body, &data[..RELAY_PIECE as usize]);
    assert_eq!(header(&first, "content-range"), Some(format!("bytes 0-{}/{}", RELAY_PIECE - 1, data.len())));
    assert_eq!(header(&first, "content-type").as_deref(), Some("audio/mpeg"));
    assert_eq!(header(&first, "content-length"), Some(RELAY_PIECE.to_string()));
    // Then for the rest.
    let rest = cast::relay(&source, Part::Media, "abc_-1", Some(&format!("bytes={RELAY_PIECE}-")));
    assert_eq!((rest.status, rest.body.as_slice()), (206, &data[RELAY_PIECE as usize..]));
    let picture = cast::relay(&source, Part::Cover, "abc_-1", None);
    assert_eq!((picture.status, picture.body.as_slice()), (200, b"jpeg".as_slice()));

    // Not in the pass, an unsafe id, an ended pass, a device that is not there.
    assert_eq!(cast::relay(&source, Part::Media, "other", None).status, 404);
    assert_eq!(cast::relay(&source, Part::Media, "../x", None).status, 404);
    rig.server.tickets().revoke();
    assert_eq!(cast::relay(&source, Part::Media, "abc_-1", None).status, 404);
    let gone = CastSource { port: 1, ..source };
    assert_eq!(cast::relay(&gone, Part::Media, "abc_-1", None).status, 502);
}

#[test]
fn the_local_relay_gives_a_native_player_the_whole_range_it_asks_for() {
    use super::cast::{CastSource, Inbox, LocalRelay, RELAY_PIECE, SharedFile};
    let rig = rig();
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.mp3");
    // Two and a half pieces: the answer has to be put together from three reads of the other device.
    let data: Vec<u8> = (0..(RELAY_PIECE as u32 * 5 / 2)).map(|i| (i % 241) as u8).collect();
    std::fs::write(&song, &data).unwrap();
    let ticket = rig.server.tickets().grant([("abc".to_string(), SharedFile { media: song, cover: None })]);
    let inbox = Arc::new(Inbox::default());
    inbox.set(CastSource { from: "phone-1".into(), host: "127.0.0.1".into(), port: rig.addr.port(), fingerprint: rig.fingerprint.clone(), ticket });
    let relay = LocalRelay::start(inbox.clone()).unwrap();
    let base = relay.base();
    let (host_port, path) = base.strip_prefix("http://").unwrap().split_once('/').unwrap();
    let ask = |request: String| {
        let mut stream = TcpStream::connect(host_port).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        let split = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        (String::from_utf8_lossy(&answer[..split]).into_owned(), answer[split..].to_vec())
    };

    // What Media3 asks first: everything.
    let (head, body) = ask(format!("GET /{path}media/abc HTTP/1.1\r\nRange: bytes=0-\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 206") && head.contains(&format!("Content-Length: {}", data.len())), "{head}");
    assert!(head.contains(&format!("Content-Range: bytes 0-{}/{}", data.len() - 1, data.len())));
    assert_eq!(body, data);
    // A seek: from the middle of a piece.
    let (head, body) = ask(format!("GET /{path}media/abc HTTP/1.1\r\nRange: bytes=1500000-\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 206"));
    assert_eq!(body, &data[1_500_000..]);
    // No range: the whole file, 200.
    let (head, body) = ask(format!("GET /{path}media/abc HTTP/1.1\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 200"));
    assert_eq!(body.len(), data.len());

    // Another app of the phone that does not know the secret gets nothing; nor does anybody once the pass ends.
    let wrong = format!("GET /{}/media/abc HTTP/1.1\r\n\r\n", "0".repeat(64));
    assert!(ask(wrong).0.starts_with("HTTP/1.1 404"));
    inbox.clear();
    assert!(ask(format!("GET /{path}media/abc HTTP/1.1\r\n\r\n")).0.starts_with("HTTP/1.1 404"));
}
