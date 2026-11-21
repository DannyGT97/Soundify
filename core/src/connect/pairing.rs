//! Who may control this device. There is no account to log in with, so a device
//! that wants to be controlled shows a PIN on its screen; whoever types it on the
//! other device gets a token (32 random bytes) that both keep. After that the
//! controller shows the token and needs no PIN. Only the token's SHA-256 stays on
//! the player's side, so a copy of its file cannot be used to control it.
//!
//! That is the default (strict). `set_open(true)` lets any device on the network pair without a PIN (what Spotify
//! feels like: it just shows up); it is a choice in Ajustes, with a warning.
//!
//! The PIN never travels: both sides run SPAKE2 with it (a password-authenticated key exchange), so somebody listening,
//! or even sitting in between, learns nothing they could try PINs against offline; each guess costs them a try. The key
//! it gives is used to CONFIRM, in both directions, the TLS certificate each side saw: a device in between, which shows
//! its own certificate, is caught there. The controller then pins that certificate (`tls`).
//!
//! The PIN is short, so it is guarded: it lasts two minutes, works once, and five
//! wrong tries lock every attempt for a minute (and throw the PIN away).

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use super::protocol::PairRequest;
use crate::Location;
use crate::secret::{self, Opened, Plain, Sealer};
use crate::storage::Storage;

/// How long a PIN is good for.
pub const PIN_LIFETIME: Duration = Duration::from_secs(120);
/// Wrong tries before everything is refused for [`LOCK_TIME`].
pub const MAX_FAILURES: u32 = 5;
pub const LOCK_TIME: Duration = Duration::from_secs(60);
/// Devices that may control this one (a home has a handful).
pub const MAX_TRUSTED: usize = 32;
/// How long the second half of a pairing may wait for the first.
const EXCHANGE_LIFETIME: Duration = Duration::from_secs(30);
/// Pairings half done at once (somebody starting many only fills this; each wrong finish costs a try).
const MAX_EXCHANGES: usize = 4;
const SPAKE_CONTROLLER: &[u8] = b"soundify-connect-controller";
const SPAKE_DEVICE: &[u8] = b"soundify-connect-device";

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
    /// `desktop`, `phone` or `tablet` (empty if it did not say).
    #[serde(default)]
    pub kind: String,
    /// SHA-256 of the token, in hex.
    pub token_hash: String,
    /// When it last paired or was connected (seconds since 1970). Empty in files from before it existed.
    #[serde(default)]
    pub last_seen: Option<u64>,
    /// Paired over TLS. Those of before (their token travelled in the clear) are let go of when the file is read.
    #[serde(default)]
    pub tls: bool,
}

/// A controller that has not connected for this long is let go of (an app that was reinstalled or whose data was cleared
/// comes back with another id and leaves its old entry behind).
pub const FORGET_AFTER_SECS: u64 = 30 * 24 * 60 * 60;
/// A new `last_seen` is only written to disk when it moved this much.
const SEEN_SAVE_EVERY_SECS: u64 = 60;

/// Seconds since 1970 now.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Refuses while it is locked; lifts the lock once the time is over.
fn check_lock(state: &mut State, now: Instant) -> Result<(), PairError> {
    if let Some(until) = state.locked_until {
        if until > now {
            return Err(PairError::Locked((until - now).as_secs() + 1));
        }
        state.locked_until = None;
        state.failures = 0;
    }
    Ok(())
}

struct State {
    /// Pairing needs no PIN.
    open: bool,
    pin: Option<(String, Instant)>,
    failures: u32,
    locked_until: Option<Instant>,
    trusted: Vec<Trusted>,
    /// First halves of pairings with a PIN, waiting for their confirmation.
    exchanges: Vec<Exchange>,
}

struct Exchange {
    session: String,
    request: PairRequest,
    key: Vec<u8>,
    until: Instant,
}

/// What the first half of a pairing answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// An open device: here is the token, nothing else to prove.
    Token(String),
    /// The PIN is asked for: SPAKE2's answer, and the session to confirm it in.
    Challenge { session: String, spake: String },
}

/// What a good confirmation gets: the token, and this device's own proof that it knew the PIN and saw the same certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub token: String,
    pub confirm: String,
}

/// The proof one side gives: HMAC with the SPAKE2 key over who it is, the controller and the certificate it saw.
fn proof(key: &[u8], side: &str, controller_id: &str, fingerprint: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let tag = hmac::sign(&key, format!("soundify-pair|{side}|{controller_id}|{fingerprint}").as_bytes());
    tag.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// The controller's side of a pairing with a PIN.
pub struct PinExchange {
    spake: Spake2<Ed25519Group>,
}

impl PinExchange {
    /// Starts with the PIN the user typed: what to send in `PairRequest::spake`.
    pub fn start(pin: &str) -> (PinExchange, String) {
        let (spake, message) = Spake2::<Ed25519Group>::start_a(&Password::new(pin.trim().as_bytes()), &Identity::new(SPAKE_CONTROLLER), &Identity::new(SPAKE_DEVICE));
        (PinExchange { spake }, URL_SAFE_NO_PAD.encode(message))
    }

    /// With the device's answer: the confirmation to send, and the one the device must give back (over the certificate
    /// THIS side saw: a different one on the other side, somebody in between, makes them differ).
    pub fn finish(self, answer: &str, controller_id: &str, fingerprint_seen: &str) -> Result<(String, String), PairError> {
        let message = URL_SAFE_NO_PAD.decode(answer).map_err(|_| PairError::Invalid)?;
        let key = self.spake.finish(&message).map_err(|_| PairError::Invalid)?;
        Ok((proof(&key, "controller", controller_id, fingerprint_seen), proof(&key, "device", controller_id, fingerprint_seen)))
    }
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
        let trusted: Vec<Trusted> = storage.read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        // A token handed out in the clear (before TLS) may have been overheard: it opens nothing any more.
        let trusted = trusted.into_iter().filter(|t| t.tls).collect();
        let state = State { open: false, pin: None, failures: 0, locked_until: None, trusted, exchanges: Vec::new() };
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

    /// The first half of a pairing. An open device hands out the token at once; a strict one runs SPAKE2 with its PIN
    /// and waits for the confirmation ([`Pairing::finish`]).
    pub fn start(&self, request: &PairRequest, now: Instant) -> Result<Started, PairError> {
        if request.controller_id.is_empty() || request.controller_id.len() > 64 || request.name.len() > 80 || request.kind.len() > 16 || request.spake.len() > 256 {
            return Err(PairError::Invalid);
        }
        let mut state = self.state.lock().unwrap();
        check_lock(&mut state, now)?;
        if state.open {
            return self.trust(&mut state, request).map(Started::Token);
        }
        let Some((pin, _)) = state.pin.clone().filter(|(_, until)| *until > now) else {
            state.pin = None;
            return Err(PairError::NoPin);
        };
        if request.spake.is_empty() {
            return Err(PairError::WrongPin);
        }
        let message = URL_SAFE_NO_PAD.decode(&request.spake).map_err(|_| PairError::Invalid)?;
        let (spake, answer) = Spake2::<Ed25519Group>::start_b(&Password::new(pin.as_bytes()), &Identity::new(SPAKE_CONTROLLER), &Identity::new(SPAKE_DEVICE));
        let key = spake.finish(&message).map_err(|_| PairError::Invalid)?;
        let session = random_token();
        state.exchanges.retain(|e| e.until > now);
        if state.exchanges.len() >= MAX_EXCHANGES {
            state.exchanges.remove(0);
        }
        state.exchanges.push(Exchange { session: session.clone(), request: request.clone(), key, until: now + EXCHANGE_LIFETIME });
        Ok(Started::Challenge { session, spake: URL_SAFE_NO_PAD.encode(answer) })
    }

    /// The second half: the controller proves it knew the PIN and saw THIS device's certificate (`own_fingerprint`).
    /// A wrong proof is a wrong PIN (or somebody in between) and costs a try.
    pub fn finish(&self, session: &str, confirm: &str, own_fingerprint: &str, now: Instant) -> Result<Finished, PairError> {
        let mut state = self.state.lock().unwrap();
        check_lock(&mut state, now)?;
        let Some(at) = state.exchanges.iter().position(|e| same(e.session.as_bytes(), session.as_bytes()) && e.until > now) else {
            return Err(PairError::NoPin);
        };
        let exchange = state.exchanges.remove(at);
        if state.pin.as_ref().is_none_or(|(_, until)| *until <= now) {
            state.pin = None;
            return Err(PairError::NoPin);
        }
        let wanted = proof(&exchange.key, "controller", &exchange.request.controller_id, own_fingerprint);
        if !same(wanted.as_bytes(), confirm.trim().as_bytes()) {
            state.failures += 1;
            if state.failures >= MAX_FAILURES {
                state.pin = None;
                state.exchanges.clear();
                state.locked_until = Some(now + LOCK_TIME);
                return Err(PairError::Locked(LOCK_TIME.as_secs()));
            }
            return Err(PairError::WrongPin);
        }
        // Good: the PIN is used up (and any other half-done pairing with it).
        state.pin = None;
        state.exchanges.clear();
        let token = self.trust(&mut state, &exchange.request)?;
        Ok(Finished { token, confirm: proof(&exchange.key, "device", &exchange.request.controller_id, own_fingerprint) })
    }

    /// This controller gets a token of its own (a second pairing replaces the first).
    fn trust(&self, state: &mut State, request: &PairRequest) -> Result<String, PairError> {
        state.failures = 0;
        let token = random_token();
        let entry = Trusted { controller_id: request.controller_id.clone(), name: request.name.clone(), kind: request.kind.clone(), token_hash: hash(&token), last_seen: Some(now_secs()), tls: true };
        state.trusted.retain(|t| t.controller_id != entry.controller_id);
        // A computer that lost its data pairs again with a new id: its old entry (same name) would stay in the list for ever.
        // Only computers: a name is unique among them on a network, while two phones may well be called the same.
        if entry.kind == "desktop" {
            state.trusted.retain(|t| !(t.kind == "desktop" && t.name == entry.name));
        }
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

    /// The controller is connected (or just was): remembers when. Written to disk at most once a minute.
    pub fn touch(&self, controller_id: &str, now: u64) {
        let mut state = self.state.lock().unwrap();
        let Some(entry) = state.trusted.iter_mut().find(|t| t.controller_id == controller_id) else { return };
        if entry.last_seen.is_some_and(|seen| now.saturating_sub(seen) < SEEN_SAVE_EVERY_SECS) {
            return;
        }
        entry.last_seen = Some(now);
        if let Err(e) = self.save(&state.trusted) {
            log::warn!("Could not save when a controller was last seen: {e}");
        }
    }

    /// Lets go of the controllers that have not connected for `max_age` seconds. One without a date (from an older
    /// version) starts counting now. Returns how many went.
    pub fn prune(&self, now: u64, max_age: u64) -> usize {
        let mut state = self.state.lock().unwrap();
        let before = state.trusted.len();
        let mut changed = false;
        for entry in state.trusted.iter_mut().filter(|t| t.last_seen.is_none()) {
            entry.last_seen = Some(now);
            changed = true;
        }
        state.trusted.retain(|t| now.saturating_sub(t.last_seen.unwrap_or(now)) <= max_age);
        let gone = before - state.trusted.len();
        if (changed || gone > 0) && let Err(e) = self.save(&state.trusted) {
            log::warn!("Could not save the controllers: {e}");
        }
        gone
    }

    pub fn trusted(&self) -> Vec<Trusted> {
        self.state.lock().unwrap().trusted.clone()
    }

    /// A controller that is already trusted says its name now (it was renamed, or it was paired under a default one): the stored name follows.
    /// True if it changed.
    pub fn rename(&self, controller_id: &str, name: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(entry) = state.trusted.iter_mut().find(|t| t.controller_id == controller_id && t.name != name) else { return false };
        entry.name = name.to_string();
        if let Err(e) = self.save(&state.trusted) {
            log::warn!("Could not save the new name of a controller: {e}");
        }
        true
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
    /// The fingerprint of its TLS certificate, pinned when it paired (empty: paired before there was TLS; it is pinned
    /// at the first connection).
    #[serde(default)]
    pub fingerprint: String,
}

/// The controller's side: the devices it has been paired with, and their tokens.
pub struct KnownDevices {
    storage: Arc<dyn Storage>,
    file: Location,
    list: Mutex<Vec<Known>>,
    /// Seals the file: the tokens in it are what lets this device into the others (`secret.rs`).
    sealer: Arc<dyn Sealer>,
}

impl KnownDevices {
    /// The file as plain text (what it always was).
    pub fn load(storage: Arc<dyn Storage>, file: Location) -> Self {
        Self::load_sealed(storage, file, Arc::new(Plain))
    }

    /// The file sealed with `sealer`. A plain file (from before) opens and is sealed straight away; a sealed one that this user
    /// cannot open (the folder was moved to another computer) is kept under another name and the list starts empty: the
    /// devices are paired again with their PIN, nothing is lost that was not lost already.
    pub fn load_sealed(storage: Arc<dyn Storage>, file: Location, sealer: Arc<dyn Sealer>) -> Self {
        let bytes = storage.read(&file).ok();
        let (list, needs_sealing): (Vec<Known>, bool) = match bytes.as_deref().map(|bytes| secret::read(sealer.as_ref(), bytes)) {
            None => (Vec::new(), false),
            Some(Opened::Plain(plain)) => (serde_json::from_slice(&plain).unwrap_or_default(), sealer.protects()),
            Some(Opened::Sealed(plain)) => (serde_json::from_slice(&plain).unwrap_or_default(), false),
            Some(Opened::Unreadable) => {
                log::warn!("Soundify Connect: the file of paired devices cannot be opened by this user here (another computer?): kept aside, pair again");
                if let Some(bytes) = &bytes {
                    let _ = storage.write(&Location::path(format!("{file}.unreadable")), bytes);
                }
                (Vec::new(), false)
            }
        };
        // Paired before TLS (no certificate pinned): their tokens travelled in the clear and the devices no longer accept
        // them. They would only show as «paired, offline» for ever; pairing again with the PIN brings them back.
        let list: Vec<Known> = list.into_iter().filter(|known| !known.fingerprint.is_empty()).collect();
        let known = Self { storage, file, list: Mutex::new(list.clone()), sealer };
        if needs_sealing {
            let _ = known.save(&list);
        }
        known
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
        let plain = serde_json::to_vec_pretty(list).map_err(io::Error::other)?;
        let (bytes, _) = secret::write(self.sealer.as_ref(), &plain);
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

    /// What the user types on the controller: who it is and the PIN.
    struct Ask {
        request: PairRequest,
        pin: String,
    }

    fn request(pin: &str) -> Ask {
        Ask { request: PairRequest { controller_id: "phone-1".into(), name: "Móvil".into(), kind: "phone".into(), spake: String::new() }, pin: pin.into() }
    }

    const FINGERPRINT: &str = "the certificate this device shows";

    /// A whole pairing as the controller does it (both halves, and its check of the device's proof).
    fn pair(pairing: &Pairing, ask: &Ask, now: Instant) -> Result<String, PairError> {
        pair_seeing(pairing, ask, now, FINGERPRINT)
    }

    fn pair_seeing(pairing: &Pairing, ask: &Ask, now: Instant, seen: &str) -> Result<String, PairError> {
        let (exchange, spake) = PinExchange::start(&ask.pin);
        let request = PairRequest { spake: if ask.pin.is_empty() { String::new() } else { spake }, ..ask.request.clone() };
        match pairing.start(&request, now)? {
            Started::Token(token) => Ok(token),
            Started::Challenge { session, spake } => {
                let (confirm, expected) = exchange.finish(&spake, &request.controller_id, seen)?;
                let finished = pairing.finish(&session, &confirm, FINGERPRINT, now)?;
                assert_eq!(finished.confirm, expected, "the device proves it knew the PIN too");
                Ok(finished.token)
            }
        }
    }

    #[test]
    fn a_strict_device_is_the_default_and_an_open_one_pairs_without_a_pin() {
        let storage = Arc::new(MemoryStorage::default());
        storage.create_dir_all(&Location::path("/data")).unwrap();
        let pairing = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert!(!pairing.is_open(), "the PIN is asked for by default");
        assert_eq!(pair(&pairing, &request(""), Instant::now()), Err(PairError::NoPin));
        pairing.set_open(true);
        let token = pair(&pairing, &request(""), Instant::now()).unwrap();
        assert!(pairing.authenticate(&token).is_some());
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
        let token = pair(&pairing, &request(&pin), now).unwrap();
        assert_eq!(pairing.authenticate(&token).unwrap().controller_id, "phone-1");
        // The PIN is used up.
        assert_eq!(pair(&pairing, &request(&pin), now), Err(PairError::NoPin));
        assert!(pairing.authenticate("another token").is_none());
    }

    #[test]
    fn a_pin_that_nobody_asked_for_or_that_expired_is_refused() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        assert_eq!(pair(&pairing, &request("123456"), now), Err(PairError::NoPin));
        let pin = pairing.new_pin(now);
        assert_eq!(pair(&pairing, &request(&pin), now + PIN_LIFETIME + Duration::from_secs(1)), Err(PairError::NoPin));
        assert_eq!(pairing.current_pin(now + PIN_LIFETIME + Duration::from_secs(1)), None);
    }

    #[test]
    fn five_wrong_tries_lock_it_and_throw_the_pin_away() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let wrong = if pin == "000000" { "111111" } else { "000000" };
        for _ in 0..MAX_FAILURES - 1 {
            assert_eq!(pair(&pairing, &request(wrong), now), Err(PairError::WrongPin));
        }
        assert_eq!(pair(&pairing, &request(wrong), now), Err(PairError::Locked(60)));
        // Not even the right one gets in while it is locked, and the PIN is gone anyway.
        assert!(matches!(pair(&pairing, &request(&pin), now + Duration::from_secs(5)), Err(PairError::Locked(_))));
        // After the wait there is no PIN left: a new one has to be asked for.
        assert_eq!(pair(&pairing, &request(&pin), now + LOCK_TIME + Duration::from_secs(1)), Err(PairError::NoPin));
    }

    #[test]
    fn pairing_again_replaces_the_controllers_old_token() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let first = pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        let second = pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        assert!(pairing.authenticate(&first).is_none());
        assert!(pairing.authenticate(&second).is_some());
        assert_eq!(pairing.trusted().len(), 1);
    }

    #[test]
    fn a_computer_that_pairs_again_with_a_new_id_replaces_its_old_entry_but_two_phones_of_one_name_both_stay() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let as_kind = |id: &str, kind: &str| Ask { request: PairRequest { controller_id: id.into(), name: "MSI-PC".into(), kind: kind.into(), spake: String::new() }, pin: pairing.new_pin(now) };
        let old = pair(&pairing, &as_kind("pc-old", "desktop"), now).unwrap();
        let new = pair(&pairing, &as_kind("pc-new", "desktop"), now).unwrap();
        assert!(pairing.authenticate(&old).is_none() && pairing.authenticate(&new).is_some());
        assert_eq!(pairing.trusted().len(), 1);
        pair(&pairing, &as_kind("phone-a", "phone"), now).unwrap();
        pair(&pairing, &as_kind("phone-b", "phone"), now).unwrap();
        assert_eq!(pairing.trusted().len(), 3);
    }

    #[test]
    fn only_the_hash_of_the_token_is_kept_and_it_survives_a_restart() {
        let (storage, pairing) = pairing();
        let now = Instant::now();
        let token = pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        let saved = String::from_utf8(storage.read(&Location::path("/data/connect-trusted.json")).unwrap()).unwrap();
        assert!(!saved.contains(&token));
        let again = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert!(again.authenticate(&token).is_some());
    }

    #[test]
    fn forgetting_takes_the_access_away() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let token = pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        pairing.forget("phone-1").unwrap();
        assert!(pairing.authenticate(&token).is_none());
    }

    #[test]
    fn a_controller_is_stamped_when_it_pairs_and_the_stamp_follows_its_connections() {
        let (storage, pairing) = pairing();
        let now = Instant::now();
        pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        let paired = pairing.trusted()[0].last_seen.unwrap();
        assert!(paired > 0);
        // Within the minute nothing changes (and nothing is written)...
        pairing.touch("phone-1", paired + 10);
        assert_eq!(pairing.trusted()[0].last_seen, Some(paired));
        // ...after it, the new moment is kept, also after a restart.
        pairing.touch("phone-1", paired + 500);
        assert_eq!(pairing.trusted()[0].last_seen, Some(paired + 500));
        let again = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert_eq!(again.trusted()[0].last_seen, Some(paired + 500));
        // Somebody who is not trusted is not stamped (nor invented).
        pairing.touch("nobody", paired + 900);
        assert_eq!(pairing.trusted().len(), 1);
    }

    #[test]
    fn controllers_not_seen_for_thirty_days_are_let_go_of_and_the_others_stay() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let day = 24 * 60 * 60;
        pair(&pairing, &request(&pairing.new_pin(now)), now).unwrap();
        let mut other = request(&pairing.new_pin(now));
        other.request.controller_id = "phone-2".into();
        pair(&pairing, &other, now).unwrap();
        let start = pairing.trusted()[0].last_seen.unwrap();
        pairing.touch("phone-2", start + 29 * day);
        // On day 31 the first one (never seen again) goes and the second one (seen on day 29) stays.
        assert_eq!(pairing.prune(start + 31 * day, FORGET_AFTER_SECS), 1);
        let left: Vec<String> = pairing.trusted().into_iter().map(|t| t.controller_id).collect();
        assert_eq!(left, ["phone-2"]);
        // Exactly thirty days is still in time.
        assert_eq!(pairing.prune(start + 29 * day + 30 * day, FORGET_AFTER_SECS), 0);
    }

    #[test]
    fn a_controller_from_before_the_date_existed_starts_counting_at_the_first_cleanup() {
        let (storage, _) = pairing();
        let old = r#"[{"controllerId":"old","name":"Viejo","kind":"phone","tokenHash":"x","tls":true}]"#;
        storage.write(&Location::path("/data/connect-trusted.json"), old.as_bytes()).unwrap();
        let pairing = Pairing::load(storage, Location::path("/data/connect-trusted.json"));
        assert_eq!(pairing.trusted()[0].last_seen, None);
        assert_eq!(pairing.prune(1_000, FORGET_AFTER_SECS), 0);
        assert_eq!(pairing.trusted()[0].last_seen, Some(1_000));
        assert_eq!(pairing.prune(1_000 + FORGET_AFTER_SECS + 1, FORGET_AFTER_SECS), 1);
    }

    #[test]
    fn a_malformed_request_is_refused_before_it_costs_a_try() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let mut bad = request(&pin);
        bad.request.controller_id = String::new();
        assert_eq!(pair(&pairing, &bad, now), Err(PairError::Invalid));
        assert!(pair(&pairing, &request(&pin), now).is_ok());
    }

    #[test]
    fn the_known_devices_are_remembered_and_forgotten() {
        let storage = Arc::new(MemoryStorage::default());
        storage.create_dir_all(&Location::path("/data")).unwrap();
        let file = Location::path("/data/connect-devices.json");
        let known = KnownDevices::load(storage.clone(), file.clone());
        known.remember(Known { device_id: "tv".into(), name: "Salón".into(), kind: "desktop".into(), token: "t".into(), fingerprint: "ab".repeat(32) }).unwrap();
        assert_eq!(KnownDevices::load(storage.clone(), file.clone()).get("tv").unwrap().name, "Salón");
        // One of before TLS (no certificate pinned) is not read back.
        known.remember(Known { device_id: "old".into(), name: "Viejo".into(), kind: "phone".into(), token: "t".into(), fingerprint: String::new() }).unwrap();
        assert!(KnownDevices::load(storage.clone(), file.clone()).get("old").is_none());
        known.forget("tv").unwrap();
        assert!(KnownDevices::load(storage, file).all().is_empty());
    }

    fn paired(token: &str) -> Known {
        Known { device_id: "tv".into(), name: "Salón".into(), kind: "desktop".into(), token: token.into(), fingerprint: "ab".repeat(32) }
    }

    fn data() -> (Arc<MemoryStorage>, Location) {
        let storage = Arc::new(MemoryStorage::default());
        storage.create_dir_all(&Location::path("/data")).unwrap();
        (storage, Location::path("/data/devices.json"))
    }

    #[test]
    fn the_tokens_of_the_known_devices_are_not_written_as_plain_text_and_come_back() {
        use crate::secret::tests::Reverse;
        let (storage, file) = data();
        let sealer: Arc<dyn Sealer> = Arc::new(Reverse { fail_seal: false, fail_open: false });
        KnownDevices::load_sealed(storage.clone(), file.clone(), sealer.clone()).remember(paired("secret-token")).unwrap();
        let on_disk = storage.read(&file).unwrap();
        assert!(on_disk.starts_with(crate::secret::MAGIC));
        assert!(!String::from_utf8_lossy(&on_disk).contains("secret-token"));
        assert_eq!(KnownDevices::load_sealed(storage, file, sealer).get("tv").unwrap().token, "secret-token");
    }

    #[test]
    fn a_plain_file_from_before_is_read_and_sealed_straight_away() {
        use crate::secret::tests::Reverse;
        let (storage, file) = data();
        KnownDevices::load(storage.clone(), file.clone()).remember(paired("old-token")).unwrap();
        assert!(String::from_utf8_lossy(&storage.read(&file).unwrap()).contains("old-token"));
        let sealer: Arc<dyn Sealer> = Arc::new(Reverse { fail_seal: false, fail_open: false });
        assert_eq!(KnownDevices::load_sealed(storage.clone(), file.clone(), sealer).get("tv").unwrap().token, "old-token");
        assert!(storage.read(&file).unwrap().starts_with(crate::secret::MAGIC), "sealed now, without anybody asking");
    }

    #[test]
    fn a_file_this_user_cannot_open_is_kept_aside_and_never_overwritten_by_the_empty_list() {
        use crate::secret::tests::Reverse;
        let (storage, file) = data();
        KnownDevices::load_sealed(storage.clone(), file.clone(), Arc::new(Reverse { fail_seal: false, fail_open: false })).remember(paired("t")).unwrap();
        let sealed = storage.read(&file).unwrap();
        // Another computer: it cannot be opened.
        let elsewhere = KnownDevices::load_sealed(storage.clone(), file.clone(), Arc::new(Reverse { fail_seal: false, fail_open: true }));
        assert!(elsewhere.all().is_empty());
        assert_eq!(storage.read(&Location::path("/data/devices.json.unreadable")).unwrap(), sealed);
        assert_eq!(storage.read(&file).unwrap(), sealed, "loading does not touch it");
    }

    #[test]
    fn when_sealing_fails_the_tokens_are_still_kept() {
        use crate::secret::tests::Reverse;
        let (storage, file) = data();
        let broken: Arc<dyn Sealer> = Arc::new(Reverse { fail_seal: true, fail_open: false });
        KnownDevices::load_sealed(storage.clone(), file.clone(), broken.clone()).remember(paired("kept")).unwrap();
        assert_eq!(KnownDevices::load_sealed(storage, file, broken).get("tv").unwrap().token, "kept");
    }

    #[test]
    fn device_ids_are_32_hex_characters_and_different() {
        let (a, b) = (new_device_id(), new_device_id());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn somebody_in_between_is_caught_even_with_the_right_pin() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        // The controller saw ANOTHER certificate (the one of a device sitting in between): the proofs differ.
        assert_eq!(pair_seeing(&pairing, &request(&pin), now, "the certificate of somebody in between"), Err(PairError::WrongPin));
        assert!(pairing.trusted().is_empty());
    }

    #[test]
    fn a_confirmation_for_a_session_that_does_not_exist_or_expired_gets_nothing() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        assert_eq!(pairing.finish("made up", "00", FINGERPRINT, now), Err(PairError::NoPin));
        let (_, spake) = PinExchange::start(&pin);
        let mut ask = request(&pin);
        ask.request.spake = spake;
        let Ok(Started::Challenge { session, .. }) = pairing.start(&ask.request, now) else { panic!() };
        assert_eq!(pairing.finish(&session, "00", FINGERPRINT, now + EXCHANGE_LIFETIME + Duration::from_secs(1)), Err(PairError::NoPin));
    }

    #[test]
    fn the_pin_never_travels() {
        let (_, pairing) = pairing();
        let now = Instant::now();
        let pin = pairing.new_pin(now);
        let (_, spake) = PinExchange::start(&pin);
        assert!(!spake.contains(&pin));
        let mut ask = request(&pin);
        ask.request.spake = spake;
        let Ok(Started::Challenge { spake: answer, .. }) = pairing.start(&ask.request, now) else { panic!() };
        assert!(!answer.contains(&pin));
    }
}
