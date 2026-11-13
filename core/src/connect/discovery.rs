//! Finding each other on the local network. Behind a trait: the real one speaks mDNS
//! (`_soundify-connect._tcp`, what Spotify's ZeroConf also does), the one for tests
//! is a list in memory. An address typed in by hand ([`super::manager::Connect::add_by_address`])
//! covers the networks where mDNS does not get through.

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use super::protocol::{DeviceInfo, PROTOCOL, SERVICE_TYPE};

/// A Soundify seen on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub info: DeviceInfo,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Up(Found),
    /// Gone from the network (its device id).
    Down(String),
}

pub trait Discovery: Send + Sync {
    /// Makes this device visible to the others.
    fn announce(&self, info: &DeviceInfo, port: u16) -> io::Result<()>;
    /// Stops being visible.
    fn withdraw(&self);
    /// Tells `on_change` about every Soundify that appears or goes (those already there too).
    fn browse(&self, on_change: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()>;
    fn shutdown(&self);
}

// === mDNS ===

pub struct Mdns {
    daemon: ServiceDaemon,
    announced: Mutex<Option<String>>,
}

impl Mdns {
    pub fn new() -> io::Result<Self> {
        let daemon = ServiceDaemon::new().map_err(io::Error::other)?;
        Ok(Self { daemon, announced: Mutex::new(None) })
    }
}

impl Discovery for Mdns {
    fn announce(&self, info: &DeviceInfo, port: u16) -> io::Result<()> {
        let properties = [("v", PROTOCOL.to_string()), ("name", info.name.clone()), ("kind", info.kind.clone())];
        let host = format!("soundify-{}.local.", &info.device_id[..info.device_id.len().min(12)]);
        let service = ServiceInfo::new(SERVICE_TYPE, &info.device_id, &host, "", port, &properties[..]).map_err(io::Error::other)?.enable_addr_auto();
        let fullname = service.get_fullname().to_string();
        self.daemon.register(service).map_err(io::Error::other)?;
        *self.announced.lock().unwrap() = Some(fullname);
        Ok(())
    }

    fn withdraw(&self) {
        if let Some(fullname) = self.announced.lock().unwrap().take() {
            let _ = self.daemon.unregister(&fullname);
        }
    }

    fn browse(&self, on_change: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()> {
        let events = self.daemon.browse(SERVICE_TYPE).map_err(io::Error::other)?;
        thread::Builder::new().name("connect-browse".into()).spawn(move || {
            // The instance name is the device id; a removal only carries the full name.
            while let Ok(event) = events.recv() {
                match event {
                    ServiceEvent::ServiceResolved(service) => {
                        let Some(info) = device_of(&service) else {
                            debug!("A service of {SERVICE_TYPE} without the expected data: {}", service.get_fullname());
                            continue;
                        };
                        let addrs: Vec<IpAddr> = service.get_addresses_v4().into_iter().map(IpAddr::V4).collect();
                        if !addrs.is_empty() {
                            on_change(Change::Up(Found { info, addrs, port: service.get_port() }));
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        if let Some(id) = fullname.split('.').next() {
                            on_change(Change::Down(id.to_string()));
                        }
                    }
                    _ => {}
                }
            }
        })?;
        Ok(())
    }

    fn shutdown(&self) {
        self.withdraw();
        match self.daemon.shutdown() {
            Ok(done) => {
                let _ = done.recv_timeout(Duration::from_secs(2));
            }
            Err(e) => warn!("Could not stop the mDNS daemon: {e}"),
        }
    }
}

fn device_of(service: &mdns_sd::ResolvedService) -> Option<DeviceInfo> {
    let version: u32 = service.get_property_val_str("v")?.parse().ok()?;
    if version != PROTOCOL {
        return None;
    }
    let id = service.get_fullname().split('.').next()?;
    Some(DeviceInfo {
        v: version,
        device_id: unescape(id),
        name: service.get_property_val_str("name")?.to_string(),
        kind: service.get_property_val_str("kind").unwrap_or("desktop").to_string(),
    })
}

/// mDNS escapes some characters of an instance name; ours are hex, so nothing changes, but be safe.
fn unescape(name: &str) -> String {
    name.replace("\\.", ".")
}

// === IN MEMORY, for the tests ===

type Listener = Arc<dyn Fn(Change) + Send + Sync>;
type Shared = Arc<Mutex<Box<dyn Fn(Change) + Send + 'static>>>;

/// A network in a box: devices announce into it and browse it, with no sockets of its own.
#[derive(Default)]
pub struct MemoryNetwork {
    announced: Mutex<HashMap<String, Found>>,
    listeners: Mutex<Vec<Listener>>,
}

pub struct MemoryDiscovery {
    network: Arc<MemoryNetwork>,
    mine: Mutex<Option<String>>,
}

impl MemoryDiscovery {
    pub fn on(network: &Arc<MemoryNetwork>) -> Self {
        Self { network: network.clone(), mine: Mutex::new(None) }
    }
}

impl Discovery for MemoryDiscovery {
    fn announce(&self, info: &DeviceInfo, port: u16) -> io::Result<()> {
        let found = Found { info: info.clone(), addrs: vec![IpAddr::from([127, 0, 0, 1])], port };
        self.network.announced.lock().unwrap().insert(info.device_id.clone(), found.clone());
        *self.mine.lock().unwrap() = Some(info.device_id.clone());
        for listener in self.network.listeners.lock().unwrap().iter() {
            listener(Change::Up(found.clone()));
        }
        Ok(())
    }

    fn withdraw(&self) {
        if let Some(id) = self.mine.lock().unwrap().take() {
            self.network.announced.lock().unwrap().remove(&id);
            for listener in self.network.listeners.lock().unwrap().iter() {
                listener(Change::Down(id.clone()));
            }
        }
    }

    fn browse(&self, on_change: Box<dyn Fn(Change) + Send + 'static>) -> io::Result<()> {
        // The same callback is told about what is there and about what comes later.
        let shared: Shared = Arc::new(Mutex::new(on_change));
        for found in self.network.announced.lock().unwrap().values() {
            (shared.lock().unwrap())(Change::Up(found.clone()));
        }
        let later = shared.clone();
        self.network.listeners.lock().unwrap().push(Arc::new(move |change| (later.lock().unwrap())(change)));
        Ok(())
    }

    fn shutdown(&self) {
        self.withdraw();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn info(id: &str) -> DeviceInfo {
        DeviceInfo { v: PROTOCOL, device_id: id.into(), name: format!("Equipo {id}"), kind: "desktop".into() }
    }

    #[test]
    fn the_memory_network_tells_late_browsers_about_what_is_already_there_and_about_departures() {
        let network = Arc::new(MemoryNetwork::default());
        let (a, b) = (MemoryDiscovery::on(&network), MemoryDiscovery::on(&network));
        a.announce(&info("a"), 1000).unwrap();
        let (changes, seen) = mpsc::channel();
        b.browse(Box::new(move |change| {
            let _ = changes.send(change);
        }))
        .unwrap();
        assert!(matches!(seen.recv_timeout(Duration::from_secs(1)), Ok(Change::Up(found)) if found.info.device_id == "a" && found.port == 1000));
        a.withdraw();
        assert_eq!(seen.recv_timeout(Duration::from_secs(1)), Ok(Change::Down("a".into())));
    }

    /// With the real mDNS: needs a network interface with multicast, so it is run by hand
    /// (`cargo test -p soundify-core mdns -- --ignored`).
    #[test]
    #[ignore = "needs a real network"]
    fn two_real_mdns_daemons_see_each_other() {
        let (a, b) = (Mdns::new().unwrap(), Mdns::new().unwrap());
        let (changes, seen) = mpsc::channel();
        b.browse(Box::new(move |change| {
            let _ = changes.send(change);
        }))
        .unwrap();
        a.announce(&info("aaaabbbbccccdddd"), 48650).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut found = None;
        while std::time::Instant::now() < deadline && found.is_none() {
            if let Ok(Change::Up(f)) = seen.recv_timeout(Duration::from_millis(500)) {
                found = Some(f);
            }
        }
        let found = found.expect("the announcement was seen");
        assert_eq!((found.info.device_id.as_str(), found.info.name.as_str(), found.port), ("aaaabbbbccccdddd", "Equipo aaaabbbbccccdddd", 48650));
        a.shutdown();
        b.shutdown();
    }
}
