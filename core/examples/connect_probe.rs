//! A probe of Soundify Connect against a real device (a phone by adb, for instance), standing in for another Soundify:
//! pairs with the PIN the device shows (TLS + SPAKE2), opens the session with its certificate pinned, and passes it one
//! file served from here («Escuchar en»). Prints what the device says. Not part of the app.
//!
//!   cargo run -p soundify-core --example connect_probe -- <ip:port> <pin> <file> [own port] [seconds]
//!
//! With adb: `adb forward tcp:<port> tcp:<port of the app>` (the device then sees this probe as 127.0.0.1, so it reads
//! the file from its own 127.0.0.1) and `adb reverse tcp:<own port> tcp:<own port>`.

use std::net::{SocketAddr, TcpStream};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use soundify_core::Location;
use soundify_core::connect::cast::SharedFile;
use soundify_core::connect::client::{self, ClientError, Event, Session};
use soundify_core::connect::pairing::{Pairing, Trusted};
use soundify_core::connect::protocol::{CastItem, CastStart, Command, DeviceInfo, PROTOCOL, PlayerState, ServerMessage};
use soundify_core::connect::server::{Host, Server};
use soundify_core::connect::tls::{Expect, TlsIdentity};
use soundify_core::storage::{MemoryStorage, Storage};

struct Quiet;

impl Host for Quiet {
    fn info(&self) -> DeviceInfo {
        DeviceInfo { v: PROTOCOL, device_id: "probe-pc".into(), name: "Sonda PC".into(), kind: "desktop".into(), open: false }
    }
    fn state(&self) -> PlayerState {
        PlayerState::default()
    }
    fn command(&self, _: &Trusted, _: Command) -> Result<(), String> {
        Err("this is a probe".into())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: connect_probe <ip:port> <pin> <file> [own port]");
        std::process::exit(2);
    }
    let addr: SocketAddr = args[1].parse().expect("ip:port");
    let pin = &args[2];
    let file = PathBuf::from(&args[3]);
    let own_port: u16 = args.get(4).and_then(|p| p.parse().ok()).unwrap_or(48777);
    let seconds: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(25);

    let info = client::fetch_info(addr).expect("info");
    println!("INFO      {} ({}) protocol {} open={}", info.name, info.kind, info.v, info.open);

    // In the clear, nothing but /info.
    let mut plain = TcpStream::connect(addr).unwrap();
    plain.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    plain.write_all(b"POST /connect/v1/pair/start HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    let mut answer = String::new();
    let _ = plain.read_to_string(&mut answer);
    println!("PLAIN     {}", answer.lines().next().unwrap_or("(nothing)"));

    let started = Instant::now();
    let paired = client::pair(addr, "probe-pc", "Sonda PC", pin).expect("pair");
    println!("PAIRED    in {} ms, certificate {}…", started.elapsed().as_millis(), &paired.fingerprint[..16]);

    // Another certificate than the one pinned: refused.
    let wrong = Expect::Pinned(TlsIdentity::ephemeral().fingerprint().to_string());
    println!("WRONG CERT {:?}", Session::open(addr, &paired.token, &wrong, |_| {}).err().map(|e| matches!(e, ClientError::Untrusted)));

    // Our own server, to be read from.
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/p")).unwrap();
    let server = Server::start(own_port, Arc::new(Quiet), Arc::new(Pairing::load(storage, Location::path("/p/t.json"))), Arc::new(TlsIdentity::ephemeral())).expect("own server");
    // PROBE_ITEMS=3: a queue of several files (all the same one), and a «next» every 8 s, as a person pressing it.
    let count: usize = std::env::var("PROBE_ITEMS").ok().and_then(|n| n.parse().ok()).unwrap_or(1);
    let ticket = server.tickets().grant((1..=count).map(|n| (format!("probe{n}"), SharedFile { media: file.clone(), cover: None })));

    let (tx, rx) = mpsc::channel();
    let session = Session::open(addr, &paired.token, &Expect::Pinned(paired.fingerprint.clone()), move |event| {
        let _ = tx.send(event);
    })
    .expect("session");
    println!("SESSION   open");

    // PROBE_MODE=library: as a controller of a phone that plays nothing: list its library, open a playlist, play a song of it.
    if std::env::var("PROBE_MODE").as_deref() == Ok("library") {
        library_mode(&session, &rx);
        return;
    }
    let pass = CastStart {
        host: String::new(),
        port: server.port(),
        fingerprint: server.fingerprint().to_string(),
        ticket,
        playlist: "Prueba de la sonda".into(),
        items: (1..=count)
            .map(|n| CastItem { video_id: format!("probe{n}"), title: format!("Canción {n} de la sonda"), duration: 90.0, format: file.extension().and_then(|e| e.to_str()).unwrap_or("mp3").to_lowercase(), video: false, cover: false })
            .collect(),
        index: 0,
        position: 5.0,
        playing: true,
        shuffle: false,
        repeat: "off".into(),
    };
    session.send(Command::Cast(Box::new(pass)));
    println!("CAST      sent (the phone reads the file from here)");

    let until = Instant::now() + Duration::from_secs(seconds);
    let mut paused = false;
    let mut last_next = Instant::now();
    let mut step_no = 0usize;
    while Instant::now() < until {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Event::Message(ServerMessage::State { state })) => println!(
                "STATE     playing={} at {:.1}s of {:.1}s  {}",
                state.playing,
                state.position,
                state.duration,
                state.now_playing.map(|n| n.title).unwrap_or_default()
            ),
            Ok(Event::Message(ServerMessage::Ack { ok, error, .. })) => println!("ACK       ok={ok} {}", error.unwrap_or_default()),
            Ok(Event::Message(other)) => println!("MESSAGE   {}", serde_json::to_string(&other).unwrap_or_default().chars().take(120).collect::<String>()),
            Ok(Event::Closed(reason)) => {
                println!("CLOSED    {reason:?}");
                return;
            }
            Err(_) => {}
        }
        if count > 1 && last_next.elapsed() > Duration::from_secs(8) {
            last_next = Instant::now();
            // PROBE_SCRIPT=next,play:3,toggle : one step every 8 s (default: only «next»).
            let script = std::env::var("PROBE_SCRIPT").unwrap_or_else(|_| "next".into());
            let steps: Vec<&str> = script.split(',').collect();
            let step = steps[step_no % steps.len()];
            step_no += 1;
            let command = match step.split_once(':') {
                Some(("play", n)) => Command::PlayTrack { video_id: format!("probe{n}") },
                _ if step == "toggle" => Command::Toggle,
                _ => Command::Next,
            };
            println!("STEP      {step}");
            session.send(command);
        }
        if !paused && until - Instant::now() < Duration::from_secs(8) {
            paused = true;
            session.send(Command::Pause);
            println!("PAUSE     sent");
        }
    }
    session.send(Command::CastEnd { reason: Some("probe".into()) });
    std::thread::sleep(Duration::from_secs(2));
    server.tickets().revoke();
    while let Ok(event) = rx.try_recv() {
        if let Event::Message(ServerMessage::State { state }) = event {
            println!("STATE     playing={} {}", state.playing, state.now_playing.map(|n| n.title).unwrap_or_default());
        }
    }
    println!("DONE");
}

fn library_mode(session: &Session, rx: &mpsc::Receiver<Event>) {
    let until = Instant::now() + Duration::from_secs(16);
    let (mut asked, mut played) = (false, false);
    let mut key = String::new();
    while Instant::now() < until {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Event::Message(ServerMessage::Library { playlists })) => {
                println!("LIBRARY   {} playlists", playlists.len());
                for entry in playlists.iter().take(5) {
                    println!("          {} · {} · {} files · key={}", entry.title, entry.format, entry.count, entry.key);
                }
                if !asked && let Some(first) = playlists.first() {
                    asked = true;
                    key = first.key.clone();
                    session.send(Command::Browse { key: key.clone() });
                    println!("BROWSE    asked for {key}");
                }
            }
            Ok(Event::Message(ServerMessage::Browse { key: got, queue })) => {
                let items = queue.as_ref().map_or(0, |q| q.items.len());
                println!("BROWSED   {got}: {items} files, first: {}", queue.as_ref().and_then(|q| q.items.first()).map_or("-", |i| i.title.as_str()));
                if !played && let Some(item) = queue.and_then(|q| q.items.into_iter().nth(1)) {
                    played = true;
                    session.send(Command::PlayFrom { key: key.clone(), video_id: item.video_id });
                    println!("PLAYFROM  sent (second file)");
                }
            }
            Ok(Event::Message(ServerMessage::State { state })) => println!("STATE     playing={} {}", state.playing, state.now_playing.map(|n| n.title).unwrap_or_default()),
            Ok(Event::Message(ServerMessage::Ack { ok, error, .. })) => println!("ACK       ok={ok} {}", error.unwrap_or_default()),
            Ok(Event::Closed(reason)) => {
                println!("CLOSED    {reason:?}");
                return;
            }
            _ => {}
        }
    }
    session.send(Command::Pause);
    println!("DONE");
}
