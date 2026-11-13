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
use crate::Location;
use crate::storage::{MemoryStorage, Storage};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct FakeHost {
    state: Mutex<PlayerState>,
    received: Mutex<Vec<(String, Command)>>,
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
}

struct Rig {
    server: Server,
    host: Arc<FakeHost>,
    pairing: Arc<Pairing>,
    addr: SocketAddr,
}

fn rig() -> Rig {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    let pairing = Arc::new(Pairing::load(storage, Location::path("/data/trusted.json")));
    // These tests are about the PIN: the strict way (the open one is tested on its own).
    pairing.set_open(false);
    let host = Arc::new(FakeHost::default());
    let server = Server::start(0, host.clone(), pairing.clone()).unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
    Rig { server, host, pairing, addr }
}

/// Pairs a controller the way the user does: reads the PIN off the player, types it on the controller.
fn paired(rig: &Rig) -> String {
    let pin = rig.pairing.new_pin(Instant::now());
    client::pair(rig.addr, "phone-1", "Móvil", &pin).unwrap()
}

fn open(rig: &Rig, token: &str) -> (Session, Receiver<Event>) {
    let (events, received) = mpsc::channel();
    let session = Session::open(rig.addr, token, move |event| {
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
    let token = client::pair(rig.addr, "phone-1", "Móvil", "").unwrap();
    assert!(Session::open(rig.addr, &token, |_| {}).is_ok());
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
    assert!(matches!(Session::open(rig.addr, "not-a-token", |_| {}), Err(ClientError::Refused { status: 401, .. })));
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
    assert!(matches!(Session::open(rig.addr, &token, |_| {}), Err(ClientError::Refused { status: 401, .. })));
}

#[test]
fn unknown_routes_and_wrong_methods_are_refused_and_a_huge_body_is_not_read() {
    let rig = rig();
    let raw = |request: &str| {
        let mut stream = TcpStream::connect(rig.addr).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    };
    assert!(raw("GET /nothing HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 404"));
    assert!(raw("DELETE /connect/v1/info HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 405"));
    assert!(raw("POST /connect/v1/pair HTTP/1.1\r\nContent-Length: 999999\r\n\r\n").starts_with("HTTP/1.1 400"));
    assert!(raw("garbage\r\n\r\n").starts_with("HTTP/1.1 400"));
    // A WebSocket without a token gets no further than the door.
    assert!(raw("GET /connect/v1/ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").starts_with("HTTP/1.1 401"));
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
