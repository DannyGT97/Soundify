//! Who may control this device. There is no account to log in with, so a device
//! that wants to be controlled shows a PIN on its screen; whoever types it on the
//! other device gets a token (32 random bytes) that both keep. After that the
//! controller shows the token and needs no PIN. Only the token's SHA-256 stays on
//! the player's side, so a copy of its file cannot be used to control it.
//!
//! That is the strict way. By default a device is OPEN: any device on the network may pair
//! with it without a PIN (what Spotify feels like: it just shows up), because what a
//! controller can do is only play, pause and the like. `set_open(false)` asks for the PIN again.
//!
//! The PIN is short, so it is guarded: it lasts two minutes, works once, and five
//! wrong tries lock every attempt for a minute (and throw the PIN away).

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::protocol::PairRequest;
use crate::Location;
use crate::storage::Storage;

/// How long a PIN is good for.
pub const PIN_LIFETIME: Duration = Duration::from_secs(120);
/// Wrong tries before everything is refused for [`LOCK_TIME`].
pub const MAX_FAILURES: u32 = 5;
pub const LOCK_TIME: Duration = Duration::from_secs(60);
/// Devices that may control this one (a home has a handful).
pub const MAX_TRUSTED: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// Nobody asked for a PIN here (or it expired).
    NoPin,
    WrongPin,
    /// Too many tries: this many seconds to wait.
    Locked(u64),
    /// The request itself is malformed.
    Invalid,
    /// The file with the trusted devices could not be saved.
    Storage(String),
}

/// A controller this device lets in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trusted {
    pub controller_id: String,
    pub name: String,
    /// SHA-256 of the token, in hex.
    pub token_hash: String,
}

struct State {
    /// Pairing needs no PIN.
    open: bool,
    pin: Option<(String, Instant)>,
    failures: u32,
    locked_until: Option<Instant>,
    trusted: Vec<Trusted>,
}

/// The player's side of the trust: its PIN and the controllers it has accepted.
pub struct Pairing {
    storage: Arc<dyn Storage>,
    file: Location,
    state: Mutex<State>,
}

impl Pairing {
    /// Reads the trusted controllers from `file` (none if it is missing or damaged).
    pub fn load(storage: Arc<dyn Storage>, file: Location) -> Self {
        let trusted = storage.read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let state = State { open: true, pin: None, failures: 0, locked_until: None, trusted };
        Self { storage, file, state: Mutex::new(state) }
    }

    /// Open (no PIN to pair) or strict (the PIN is asked for).
    pub fn set_open(&self, open: bool) {
        self.state.lock().unwrap().open = open;
    }

    pub fn is_open(&self) -> bool {
        self.state.lock().unwrap().open
    }

    /// A new PIN, replacing the one there was. The user reads it off this device's screen.
    pub fn new_pin(&self, now: Instant) -> String {
        let pin = random_pin();
        let mut state = self.state.lock().unwrap();
        state.pin = Some((pin.clone(), now + PIN_LIFETIME));
        pin
    }

    /// The PIN being shown, if it has not expired.
    pub fn current_pin(&self, now: Instant) -> Option<String> {
        let state = self.state.lock().unwrap();
        state.pin.as_ref().filter(|(_, until)| *until > now).map(|(pin, _)| pin.clone())
    }

    /// Checks the PIN a controller typed and, if it is right, hands out its token.
    pub fn pair(&self, request: &PairRequest, now: Instant) -> Result<String, PairError> {
        if request.controller_id.is_empty() || request.controller_id.len() > 64 || request.name.len() > 80 {
            return Err(PairError::Invalid);
        }
        let mut state = self.state.lock().unwrap();
        if let Some(until) = state.locked_until {
            if until > now {
                return Err(PairError::Locked((until - now).as_secs() + 1));
            }
            state.locked_until = None;
            state.failures = 0;
        }
        if !state.open {
            let Some((pin, _)) = state.pin.clone().filter(|(_, until)| *until > now) else {
                state.pin = None;
                return Err(PairError::NoPin);
            };
            if !same(pin.as_bytes(), request.pin.trim().as_bytes()) {
                state.failures += 1;
                if state.failures >= MAX_FAILURES {
                    state.pin = None;
                    state.locked_until = Some(now + LOCK_TIME);
                    return Err(PairError::Locked(LOCK_TIME.as_secs()));
                }
                return Err(PairError::WrongPin);
            }
        }
        // Good: the PIN is used up, and this controller gets a token of its own (a second pairing replaces the first).
        state.pin = None;
        state.failures = 0;
        let token = random_token();
        let entry = Trusted { controller_id: request.controller_id.clone(), name: request.name.clone(), token_hash: hash(&token) };
        state.trusted.retain(|t| t.controller_id != entry.controller_id);
        if state.trusted.len() >= MAX_TRUSTED {
            state.trusted.remove(0);
        }
        state.trusted.push(entry);
        self.save(&state.trusted).map_err(|e| PairError::Storage(e.to_string()))?;
        Ok(token)
    }

    /// Who this token belongs to, if anybody.
    pub fn authenticate(&self, token: &str) -> Option<Trusted> {
        let wanted = hash(token);
        let state = self.state.lock().unwrap();
        // Every entry is compared, always: how long it takes must not say which one nearly matched.
        let mut found = None;
        for entry in &state.trusted {
            if same(entry.token_hash.as_bytes(), wanted.as_bytes()) {
                found = Some(entry.clone());
            }
        }
        found
    }

    pub fn trusted(&self) -> Vec<Trusted> {
        self.state.lock().unwrap().trusted.clone()
    }

    /// Takes a controller's access away.
    pub fn forget(&self, controller_id: &str) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.trusted.retain(|t| t.controller_id != controller_id);
        self.save(&state.trusted)
    }

    fn save(&self, trusted: &[Trusted]) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(trusted).map_err(io::Error::other)?;
        self.storage.write(&self.file, &bytes)
    }
}

/// A device this one controls: where it was last seen and the token it gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Known {
    pub device_id: String,
    pub name: String,
    pub kind: String,
    pub token: String,
}

/// The controller's side: the devices it has been paired with, and their tokens.
pub struct KnownDevices {
    storage: Arc<dyn Storage>,
    file: Location,
    list: Mutex<Vec<Known>>,
}

impl KnownDevices {
    pub fn load(storage: Arc<dyn Storage>, file: Location) -> Self {
        let list = storage.read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { storage, file, list: Mutex::new(list) }
    }

    pub fn all(&self) -> Vec<Known> {
        self.list.lock().unwrap().clone()
    }

    pub fn get(&self, device_id: &str) -> Option<Known> {
        self.list.lock().unwrap().iter().find(|k| k.device_id == device_id).cloned()
    }

    pub fn remember(&self, known: Known) -> io::Result<()> {
        let mut list = self.list.lock().unwrap();
        list.retain(|k| k.device_id != known.device_id);
        list.push(known);
        self.save(&list)
    }

    pub fn forget(&self, device_id: &str) -> io::Result<()> {
        let mut list = self.list.lock().unwrap();
        list.retain(|k| k.device_id != device_id);
        self.save(&list)
    }

    fn save(&self, list: &[Known]) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(list).map_err(io::Error::other)?;
        self.storage.write(&self.file, &bytes)
    }
}

/// Six digits, from the system's random numbers (no modulo bias worth speaking of in a PIN).
fn random_pin() -> String {
    let mut bytes = [0u8; 4];
    SystemRandom::new().fill(&mut bytes).expect("the system has random numbers");
    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("the system has random numbers");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A device id: 16 random bytes in hex, chosen once.
pub fn new_device_id() -> String {
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).expect("the system has random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Equal contents, compared in the same time wherever they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |difference, (x, y)| difference | (x ^ y)) == 0
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    /// A strict one: the PIN is asked for (the open way has its own tests).
    fn pairing() -> (Arc<MemoryStorage>, Pairing) {
        let storage = Arc::new(MemoryStorage::default());
        let file = Location::path("/data/connect-trusted.json");
        storage.create_dir_all(&Location::path("/data")).unwrap();
        let pairing = Pairing::load(storage.clone(), file);
        pairing.set_open(false);
        (storage, pairing)
    }

    fn request(pin: &str) -> PairRequest {
        PairRequest { controller_id: "phone-1".into(), name: "Móvil".into(), pin: pin.into() }
    }

    #[test]
    fn an_open_device_pairs_without_a_pin_and_a_strict_one_does_not() {
        let storage = Arc::new(MemoryStorage::default());
        storage.create_dir_all(&Location::path("/data")).unwrap();
        let pairing = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert!(pairing.is_open(), "open is the default");
        let token = pairing.pair(&request(""), Instant::now()).unwrap();
        assert!(pairing.authenticate(&token).is_some());
        pairing.set_open(false);
        assert_eq!(pairing.pair(&request(""), Instant::now()), Err(PairError::NoPin));
    }

    #[test]
    fn a_pin_is_six_digits() {
        let (_, pairing) = pairing();
        let pin = pairing.new_pin(Instant::now());
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn the_right_pin_gives_a_token_that_authenticates_once() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let token = pairing.pair(&request(&pin), now).unwrap();
        assert_eq!(pairing.authenticate(&token).unwrap().controller_id, "phone-1");
        // The PIN is used up.
        assert_eq!(pairing.pair(&request(&pin), now), Err(PairError::NoPin));
        assert!(pairing.authenticate("another token").is_none());
    }

    #[test]
    fn a_pin_that_nobody_asked_for_or_that_expired_is_refused() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        assert_eq!(pairing.pair(&request("123456"), now), Err(PairError::NoPin));
        let pin = pairing.new_pin(now);
        assert_eq!(pairing.pair(&request(&pin), now + PIN_LIFETIME + Duration::from_secs(1)), Err(PairError::NoPin));
        assert_eq!(pairing.current_pin(now + PIN_LIFETIME + Duration::from_secs(1)), None);
    }

    #[test]
    fn five_wrong_tries_lock_it_and_throw_the_pin_away() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let wrong = if pin == "000000" { "111111" } else { "000000" };
        for _ in 0..MAX_FAILURES - 1 {
            assert_eq!(pairing.pair(&request(wrong), now), Err(PairError::WrongPin));
        }
        assert_eq!(pairing.pair(&request(wrong), now), Err(PairError::Locked(60)));
        // Not even the right one gets in while it is locked, and the PIN is gone anyway.
        assert!(matches!(pairing.pair(&request(&pin), now + Duration::from_secs(5)), Err(PairError::Locked(_))));
        // After the wait there is no PIN left: a new one has to be asked for.
        assert_eq!(pairing.pair(&request(&pin), now + LOCK_TIME + Duration::from_secs(1)), Err(PairError::NoPin));
    }

    #[test]
    fn pairing_again_replaces_the_controllers_old_token() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let first = pairing.pair(&request(&pairing.new_pin(now)), now).unwrap();
        let second = pairing.pair(&request(&pairing.new_pin(now)), now).unwrap();
        assert!(pairing.authenticate(&first).is_none());
        assert!(pairing.authenticate(&second).is_some());
        assert_eq!(pairing.trusted().len(), 1);
    }

    #[test]
    fn only_the_hash_of_the_token_is_kept_and_it_survives_a_restart() {
        let (storage, pairing) = pairing();
        let now = Instant::now();
        let token = pairing.pair(&request(&pairing.new_pin(now)), now).unwrap();
        let saved = String::from_utf8(storage.read(&Location::path("/data/connect-trusted.json")).unwrap()).unwrap();
        assert!(!saved.contains(&token));
        let again = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert!(again.authenticate(&token).is_some());
    }

    #[test]
    fn forgetting_takes_the_access_away() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let token = pairing.pair(&request(&pairing.new_pin(now)), now).unwrap();
        pairing.forget("phone-1").unwrap();
        assert!(pairing.authenticate(&token).is_none());
    }

    #[test]
    fn a_malformed_request_is_refused_before_it_costs_a_try() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let mut bad = request(&pin);
        bad.controller_id = String::new();
        assert_eq!(pairing.pair(&bad, now), Err(PairError::Invalid));
        assert!(pairing.pair(&request(&pin), now).is_ok());
    }

    #[test]
    fn the_known_devices_are_remembered_and_forgotten() {
        let storage = Arc::new(MemoryStorage::default());
        storage.create_dir_all(&Location::path("/data")).unwrap();
        let file = Location::path("/data/connect-devices.json");
        let known = KnownDevices::load(storage.clone(), file.clone());
        known.remember(Known { device_id: "tv".into(), name: "Salón".into(), kind: "desktop".into(), token: "t".into() }).unwrap();
        assert_eq!(KnownDevices::load(storage.clone(), file.clone()).get("tv").unwrap().name, "Salón");
        known.forget("tv").unwrap();
        assert!(KnownDevices::load(storage, file).all().is_empty());
    }

    #[test]
    fn device_ids_are_32_hex_characters_and_different() {
        let (a, b) = (new_device_id(), new_device_id());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
