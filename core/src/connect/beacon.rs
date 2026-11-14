//! The second way to be found, for the networks where mDNS does not get through (a router
//! that filters multicast, a phone that drops it): every few seconds the device broadcasts who
//! it is on UDP and listens for the others. Plain and robust; it only works inside one network.
//! [`Composite`] runs it next to mDNS: a device is there while ANY of them still sees it.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::discovery::{Change, Discovery, Found};
use super::protocol::{DeviceInfo, PROTOCOL};

/// Where the beacons are sent and heard (UDP broadcast).
pub const BEACON_PORT: u16 = 48651;
/// How often a device says it is here, and how long without hearing it before it is taken as gone.
const BEACON_EVERY: Duration = Duration::from_secs(3);
const BEACON_EXPIRES: Duration = Duration::from_secs(30);

/// The address this device uses on the local network (the one the route to the outside would use):
/// what to type on another device to reach this one, and where to send the directed broadcast.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // A route is looked up, nothing is sent (192.0.2.1 is a documentation address).
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BeaconMessage {
    soundify: u32,
    id: String,
    name: String,
    kind: String,
    port: u16,
}

pub struct Beacon {
    socket: UdpSocket,
    stop: Arc<AtomicBool>,
    announced: Arc<Mutex<Option<BeaconMessage>>>,
    sending: AtomicBool,
}

impl Beacon {
    /// Fails if the port is taken (another copy of the app on this machine already listens to it).
    pub fn new() -> io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", BEACON_PORT))?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        Ok(Self { socket, stop: Arc::default(), announced: Arc::default(), sending: AtomicBool::new(false) })
    }
}

impl Discovery for Beacon {
    fn announce(&self, info: &DeviceInfo, port: u16) -> io::Result<()> {
        *self.announced.lock().unwrap() = Some(BeaconMessage { soundify: 1, id: info.device_id.clone(), name: info.name.clone(), kind: info.kind.clone(), port });
        if self.sending.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let (socket, stop, announced) = (self.socket.try_clone()?, self.stop.clone(), self.announced.clone());
        thread::Builder::new().name("connect-beacon-send".into()).spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let message = announced.lock().unwrap().as_ref().and_then(|m| serde_json::to_vec(m).ok());
                if let Some(bytes) = message {
                    // The limited broadcast, and the one of this device's own network (the former may leave by the wrong interface).
                    let mut targets = vec![SocketAddr::from(([255, 255, 255, 255], BEACON_PORT))];
                    if let Some(ip) = local_ipv4() {
                        let [a, b, c, _] = ip.octets();
                        targets.push(SocketAddr::from(([a, b, c, 255], BEACON_PORT)));
                    }
                    for target in targets {
                        let _ = socket.send_to(&bytes, target);
                    }
                }
                // Slept in short steps, so stopping does not wait for a whole period.
                for _ in 0..6 {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(BEACON_EVERY / 6);
                }
            }
        })?;
        Ok(())
    }

    fn withdraw(&self) {
        *self.announced.lock().unwrap() = None;
    }

    fn browse(&self, on_change: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()> {
        let (socket, stop) = (self.socket.try_clone()?, self.stop.clone());
        thread::Builder::new().name("connect-beacon-listen".into()).spawn(move || {
            // Who was heard, when, from where and under which name.
            let mut seen: HashMap<String, (Instant, IpAddr, u16, String)> = HashMap::new();
            let mut buffer = [0u8; 1024];
            while !stop.load(Ordering::SeqCst) {
                if let Ok((length, from)) = socket.recv_from(&mut buffer)
                    && let Ok(message) = serde_json::from_slice::<BeaconMessage>(&buffer[..length])
                    && message.soundify == 1
                    && !message.id.is_empty()
                    && message.id.len() <= 64
                {
                    let changed = seen.get(&message.id).is_none_or(|(_, ip, port, name)| *ip != from.ip() || *port != message.port || *name != message.name);
                    seen.insert(message.id.clone(), (Instant::now(), from.ip(), message.port, message.name.clone()));
                    if changed {
                        let info = DeviceInfo { v: PROTOCOL, device_id: message.id, name: message.name, kind: message.kind, open: false };
                        on_change(Change::Up(Found { info, addrs: vec![from.ip()], port: message.port }));
                    }
                }
                let gone: Vec<String> = seen.iter().filter(|(_, (at, ..))| at.elapsed() > BEACON_EXPIRES).map(|(id, _)| id.clone()).collect();
                for id in gone {
                    seen.remove(&id);
                    on_change(Change::Down(id));
                }
            }
        })?;
        Ok(())
    }

    fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

type SharedCallback = Arc<Mutex<Box<dyn Fn(Change) + Send + 'static>>>;
type SeenBy = Arc<Mutex<HashMap<String, HashSet<usize>>>>;

/// Several ways of finding each other at the same time.
pub struct Composite {
    parts: Vec<Arc<dyn Discovery>>,
}

impl Composite {
    pub fn new(parts: Vec<Arc<dyn Discovery>>) -> Self {
        Self { parts }
    }
}

impl Discovery for Composite {
    fn announce(&self, info: &DeviceInfo, port: u16) -> io::Result<()> {
        let mut any = false;
        let mut last = None;
        for part in &self.parts {
            match part.announce(info, port) {
                Ok(()) => any = true,
                Err(e) => last = Some(e),
            }
        }
        if any { Ok(()) } else { Err(last.unwrap_or_else(|| io::Error::other("nothing to announce with"))) }
    }

    fn withdraw(&self) {
        for part in &self.parts {
            part.withdraw();
        }
    }

    fn browse(&self, on_change: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()> {
        let callback: SharedCallback = Arc::new(Mutex::new(on_change));
        let seen_by: SeenBy = Arc::default();
        let mut any = false;
        let mut last = None;
        for (index, part) in self.parts.iter().enumerate() {
            let (callback, seen_by) = (callback.clone(), seen_by.clone());
            let result = part.browse(Box::new(move |change| {
                let mut seen = seen_by.lock().unwrap();
                match change {
                    Change::Up(found) => {
                        seen.entry(found.info.device_id.clone()).or_default().insert(index);
                        drop(seen);
                        (callback.lock().unwrap())(Change::Up(found));
                    }
                    // Gone only when no way of finding it sees it any more.
                    Change::Down(id) => {
                        let gone = seen.get_mut(&id).is_none_or(|ways| {
                            ways.remove(&index);
                            ways.is_empty()
                        });
                        if gone {
                            seen.remove(&id);
                            drop(seen);
                            (callback.lock().unwrap())(Change::Down(id));
                        }
                    }
                }
            }));
            match result {
                Ok(()) => any = true,
                Err(e) => last = Some(e),
            }
        }
        if any { Ok(()) } else { Err(last.unwrap_or_else(|| io::Error::other("nothing to browse with"))) }
    }

    fn shutdown(&self) {
        for part in &self.parts {
            part.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::super::discovery::{MemoryDiscovery, MemoryNetwork};
    use super::*;

    fn info(id: &str) -> DeviceInfo {
        DeviceInfo { v: PROTOCOL, device_id: id.into(), name: format!("Equipo {id}"), kind: "desktop".into(), open: false }
    }

    #[test]
    fn a_device_is_gone_only_when_no_way_of_finding_it_sees_it_any_more() {
        let (one, two) = (Arc::new(MemoryNetwork::default()), Arc::new(MemoryNetwork::default()));
        let announcer_one = MemoryDiscovery::on(&one);
        let announcer_two = MemoryDiscovery::on(&two);
        let both = Composite::new(vec![Arc::new(MemoryDiscovery::on(&one)), Arc::new(MemoryDiscovery::on(&two))]);
        let (changes, seen) = mpsc::channel();
        both.browse(Box::new(move |change| {
            let _ = changes.send(change);
        }))
        .unwrap();
        announcer_one.announce(&info("a"), 1000).unwrap();
        announcer_two.announce(&info("a"), 1000).unwrap();
        assert!(matches!(seen.recv_timeout(Duration::from_secs(1)), Ok(Change::Up(found)) if found.info.device_id == "a"));
        // One way loses it: still there (nothing says it is gone).
        while seen.recv_timeout(Duration::from_millis(100)).is_ok() {}
        announcer_one.withdraw();
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err(), "the second way still sees it");
        // The other one too: gone.
        announcer_two.withdraw();
        assert_eq!(seen.recv_timeout(Duration::from_secs(1)), Ok(Change::Down("a".into())));
    }

    #[test]
    fn the_announcement_works_with_one_way_failing() {
        struct Broken;
        impl Discovery for Broken {
            fn announce(&self, _: &DeviceInfo, _: u16) -> io::Result<()> {
                Err(io::Error::other("no"))
            }
            fn withdraw(&self) {}
            fn browse(&self, _: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()> {
                Err(io::Error::other("no"))
            }
            fn shutdown(&self) {}
        }
        let network = Arc::new(MemoryNetwork::default());
        let composite = Composite::new(vec![Arc::new(Broken), Arc::new(MemoryDiscovery::on(&network))]);
        assert!(composite.announce(&info("a"), 1).is_ok());
        assert!(composite.browse(Box::new(|_| {})).is_ok());
        assert!(Composite::new(vec![Arc::new(Broken)]).announce(&info("a"), 1).is_err());
    }

    #[test]
    fn two_beacons_on_this_machine_see_each_other_when_the_network_allows_it() {
        // Only one can own the port: the first to bind listens, the second reports it is taken.
        let Ok(first) = Beacon::new() else { return };
        assert!(Beacon::new().is_err());
        let (changes, seen) = mpsc::channel();
        first
            .browse(Box::new(move |change| {
                let _ = changes.send(change);
            }))
            .unwrap();
        first.announce(&info("beacon-test"), 4242).unwrap();
        // Broadcast to oneself is not guaranteed on every machine: this only checks it does not misbehave.
        if let Ok(Change::Up(found)) = seen.recv_timeout(Duration::from_secs(5)) {
            assert_eq!((found.info.device_id.as_str(), found.port), ("beacon-test", 4242));
        }
        first.shutdown();
    }

    #[test]
    fn the_local_address_is_a_real_one_or_none() {
        if let Some(ip) = local_ipv4() {
            assert!(!ip.is_loopback() && !ip.is_unspecified());
        }
    }
}
