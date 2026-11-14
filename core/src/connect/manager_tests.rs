//! Two apps on one (in-memory) network: the whole way from «I see it» to «I control it».

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::discovery::{Change, Discovery, MemoryDiscovery, MemoryNetwork};
use super::manager::{Connect, ConnectError, DeviceView, Identity, Listener, Player};
use super::pairing::{KnownDevices, Pairing, Trusted};
use super::protocol::*;
use crate::Location;
use crate::storage::{MemoryStorage, Storage};

#[derive(Default)]
struct Commands(Mutex<Vec<Command>>);

impl Player for Commands {
    fn command(&self, _: &Trusted, command: Command) -> Result<(), String> {
        self.0.lock().unwrap().push(command);
        Ok(())
    }
}

#[derive(Default)]
struct Heard {
    states: Mutex<Vec<(String, PlayerState)>>,
    queues: Mutex<Vec<(String, Option<QueueInfo>)>>,
    closed: Mutex<Vec<String>>,
}

impl Listener for Heard {
    fn devices_changed(&self) {}

    fn remote_state(&self, device_id: &str, state: PlayerState) {
        self.states.lock().unwrap().push((device_id.to_string(), state));
    }

    fn remote_queue(&self, device_id: &str, queue: Option<QueueInfo>) {
        self.queues.lock().unwrap().push((device_id.to_string(), queue));
    }

    fn remote_closed(&self, device_id: &str, _: Option<String>) {
        self.closed.lock().unwrap().push(device_id.to_string());
    }
}

struct App {
    connect: Arc<Connect>,
    commands: Arc<Commands>,
    heard: Arc<Heard>,
    storage: Arc<MemoryStorage>,
}

/// `strict`: the PIN is asked for to pair (the default is open: devices pair by themselves).
fn app(network: &Arc<MemoryNetwork>, id: &str, name: &str, strict: bool) -> App {
    let storage = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    app_on(network, id, name, strict, storage)
}

fn app_on(network: &Arc<MemoryNetwork>, id: &str, name: &str, strict: bool, storage: Arc<MemoryStorage>) -> App {
    let shared: Arc<dyn Storage> = storage.clone();
    let pairing = Arc::new(Pairing::load(shared.clone(), Location::path("/data/trusted.json")));
    pairing.set_open(!strict);
    let known = KnownDevices::load(shared, Location::path("/data/devices.json"));
    let commands = Arc::new(Commands::default());
    let heard = Arc::new(Heard::default());
    let network = network.clone();
    let connect = Connect::new(
        Identity { device_id: id.into(), name: name.into(), kind: "desktop".into() },
        move || Ok(Arc::new(MemoryDiscovery::on(&network)) as Arc<dyn Discovery>),
        pairing,
        known,
        commands.clone(),
        heard.clone(),
    );
    App { connect, commands, heard, storage }
}

fn wait(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("it did not happen in time");
}

fn device(app: &App, id: &str) -> Option<DeviceView> {
    app.connect.devices().into_iter().find(|d| d.id == id)
}

/// Two open devices, on, and the controller already connected to the player (nobody typed anything).
fn open_pair() -> (App, App) {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv", "Salón", false);
    let controller = app(&network, "phone", "Móvil", false);
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| d.ready));
    (player, controller)
}

/// A strict player and a controller, with the player's PIN typed on the controller.
fn strict_pair() -> (App, App) {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv", "Salón", true);
    let controller = app(&network, "phone", "Móvil", false);
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some());
    let pin = player.connect.pin().unwrap();
    controller.connect.pair("tv", &pin).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| d.ready));
    (player, controller)
}

// === OPEN: THE DEFAULT, NOTHING TO TYPE ===

#[test]
fn open_devices_find_pair_and_connect_by_themselves_in_both_directions() {
    let (player, controller) = open_pair();
    let seen = device(&controller, "tv").unwrap();
    assert_eq!((seen.name.as_str(), seen.online, seen.paired, seen.ready, seen.needs_pin), ("Salón", true, true, true, false));
    // The player sees the phone too and is connected to it: either can control the other.
    wait(|| device(&player, "phone").is_some_and(|d| d.ready));
    assert_eq!(player.connect.controllers()[0].name, "Móvil");
}

#[test]
fn a_device_never_lists_itself_and_there_is_no_pin_to_show_while_open() {
    let (player, controller) = open_pair();
    assert!(device(&controller, "phone").is_none());
    assert_eq!(player.connect.pin(), None);
    assert!(player.connect.is_open());
}

#[test]
fn what_another_device_plays_shows_in_the_list_without_controlling_it() {
    let (player, controller) = open_pair();
    assert!(!device(&controller, "tv").unwrap().playing);
    let now = NowPlaying { video_id: "abc".into(), title: "Canción".into(), playlist: "Mix".into(), format: "MP3".into(), video: false };
    player.connect.publish(PlayerState { now_playing: Some(now), playing: true, position: 3.0, duration: 100.0, ..PlayerState::default() });
    wait(|| device(&controller, "tv").is_some_and(|d| d.playing));
    let tv = device(&controller, "tv").unwrap();
    assert_eq!(tv.now_playing.map(|p| (p.title, p.playlist)), Some(("Canción".to_string(), "Mix".to_string())));
    assert!(!tv.connected);
    assert_eq!(controller.connect.selected(), None);
}

#[test]
fn one_tap_controls_a_device_it_is_already_connected_to() {
    let (player, controller) = open_pair();
    controller.connect.select("tv").unwrap();
    assert!(device(&controller, "tv").unwrap().connected);
    controller.connect.send(Command::Next).unwrap();
    controller.connect.send(Command::Volume { volume: 0.5 }).unwrap();
    // Choosing it told it that it is now controlled from here (`Take`), before anything else.
    wait(|| player.commands.0.lock().unwrap().len() == 3);
    assert_eq!(player.commands.0.lock().unwrap().as_slice(), [Command::Take, Command::Next, Command::Volume { volume: 0.5 }]);
    player.connect.publish(PlayerState { playing: true, position: 30.0, ..PlayerState::default() });
    wait(|| controller.heard.states.lock().unwrap().iter().any(|(id, s)| id == "tv" && s.playing && s.position == 30.0));
}

#[test]
fn a_device_that_joins_later_is_connected_to_as_soon_as_it_appears() {
    let network = Arc::new(MemoryNetwork::default());
    let controller = app(&network, "phone", "Móvil", false);
    controller.connect.start(0).unwrap();
    assert!(controller.connect.devices().is_empty());
    let player = app(&network, "tv", "Salón", false);
    player.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| d.ready));
}

#[test]
fn forgetting_an_open_device_is_not_undone_by_itself() {
    let (_player, controller) = open_pair();
    controller.connect.select("tv").unwrap();
    controller.connect.forget("tv").unwrap();
    assert_eq!(controller.connect.selected(), None);
    let tv = device(&controller, "tv").unwrap();
    assert!(!tv.paired && !tv.ready);
    // Even if the device announces itself again, it stays unpaired until the user asks for it.
    std::thread::sleep(Duration::from_millis(300));
    assert!(!device(&controller, "tv").unwrap().ready);
}

#[test]
fn when_the_player_goes_away_the_controller_is_told_once_and_the_device_stays_known() {
    let (player, controller) = open_pair();
    controller.connect.select("tv").unwrap();
    player.connect.stop();
    wait(|| controller.heard.closed.lock().unwrap().as_slice() == ["tv"]);
    assert_eq!(controller.connect.selected(), None);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !device(&controller, "tv").is_some_and(|d| !d.online && d.paired) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(device(&controller, "tv").is_some_and(|d| !d.online && d.paired), "devices: {:?}", controller.connect.devices());
}

#[test]
fn the_player_that_comes_back_is_connected_to_again() {
    let (player, controller) = open_pair();
    player.connect.stop();
    let until = |wanted: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !wanted() {
            std::thread::sleep(Duration::from_millis(20));
        }
        wanted()
    };
    assert!(until(&|| device(&controller, "tv").is_some_and(|d| !d.online)), "first: {:?}", controller.connect.devices());
    player.connect.start(0).unwrap();
    assert!(until(&|| device(&controller, "tv").is_some_and(|d| d.online && d.ready)), "second: {:?}", controller.connect.devices());
}

#[test]
fn discovery_losing_a_device_whose_connection_is_up_does_not_make_it_flicker() {
    // A phone that sleeps loses the broadcasts it is found by, but its connection is the proof that it is there.
    let (player, controller) = open_pair();
    controller.connect.saw(Change::Down("tv".into()));
    std::thread::sleep(Duration::from_millis(200));
    assert!(device(&controller, "tv").is_some_and(|d| d.online && d.ready), "devices: {:?}", controller.connect.devices());
    // Once the connection really ends, what discovery said then counts: it is gone (not tried again for a minute).
    player.connect.stop();
    wait(|| device(&controller, "tv").is_some_and(|d| !d.online));
    assert!(controller.connect.selected().is_none());
}

// === THE PLAYLIST THAT PLAYS ===

fn queue(playlist: &str, ids: &[&str]) -> QueueInfo {
    QueueInfo { playlist: playlist.into(), items: ids.iter().map(|id| QueueItem { video_id: (*id).into(), title: format!("Canción {id}") }).collect() }
}

#[test]
fn the_playlist_a_device_plays_reaches_the_controller_when_it_connects_and_when_it_changes() {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv", "Salón", false);
    let controller = app(&network, "phone", "Móvil", false);
    player.connect.start(0).unwrap();
    // Published before anybody was there: whoever connects gets it with the first state.
    player.connect.publish_queue(Some(queue("Rock", &["a", "b", "c"])));
    controller.connect.start(0).unwrap();
    wait(|| controller.connect.queue_of("tv").is_some());
    assert_eq!(controller.connect.queue_of("tv").unwrap(), queue("Rock", &["a", "b", "c"]));
    // And a change goes out at once.
    player.connect.publish_queue(Some(queue("Jazz", &["x", "y"])));
    wait(|| controller.connect.queue_of("tv").is_some_and(|q| q.playlist == "Jazz"));
    assert_eq!(controller.heard.queues.lock().unwrap().last().unwrap().0, "tv");
    player.connect.publish_queue(None);
    wait(|| controller.connect.queue_of("tv").is_none());
}

#[test]
fn a_controller_picks_a_track_of_the_remote_playlist_and_the_player_gets_it() {
    let (player, controller) = open_pair();
    controller.connect.select("tv").unwrap();
    controller.connect.send(Command::PlayTrack { video_id: "b".into() }).unwrap();
    wait(|| player.commands.0.lock().unwrap().as_slice() == [Command::Take, Command::PlayTrack { video_id: "b".into() }]);
    assert!(controller.connect.send(Command::PlayTrack { video_id: String::new() }).is_err());
}

// === STRICT: THE PIN ===

#[test]
fn a_strict_device_is_not_paired_by_itself_and_says_it_needs_the_pin() {
    let network = Arc::new(MemoryNetwork::default());
    let (player, controller) = (app(&network, "tv", "Salón", true), app(&network, "phone", "Móvil", false));
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| d.needs_pin));
    let tv = device(&controller, "tv").unwrap();
    assert!(!tv.paired && !tv.ready);
    assert_eq!(controller.connect.select("tv"), Err(ConnectError::NotPaired));
    assert!(player.connect.pin().is_some(), "a strict device shows its PIN");
}

#[test]
fn the_right_pin_pairs_a_strict_device_and_connects_to_it() {
    let (player, controller) = strict_pair();
    let tv = device(&controller, "tv").unwrap();
    assert!(tv.paired && tv.ready && !tv.needs_pin);
    assert_eq!(player.connect.controllers()[0].name, "Móvil");
}

#[test]
fn a_wrong_pin_pairs_nothing() {
    let network = Arc::new(MemoryNetwork::default());
    let (player, controller) = (app(&network, "tv", "Salón", true), app(&network, "phone", "Móvil", false));
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some());
    let pin = player.connect.pin().unwrap();
    let wrong = if pin == "000000" { "111111" } else { "000000" };
    assert!(matches!(controller.connect.pair("tv", wrong), Err(ConnectError::Refused { status: 403, .. })));
    assert!(!device(&controller, "tv").unwrap().paired);
    assert_eq!(controller.connect.pair("nobody", "123456"), Err(ConnectError::Offline));
}

#[test]
fn a_token_the_strict_player_revoked_is_forgotten_at_the_next_connection() {
    let (player, controller) = strict_pair();
    player.connect.forget_controller("phone").unwrap();
    // The controller reconnects (its app restarted): the old token is refused and dropped, the PIN is needed again.
    controller.connect.stop();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| !d.paired && d.needs_pin));
}

#[test]
fn the_known_devices_survive_a_restart_of_the_controller() {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv", "Salón", false);
    let first = app(&network, "phone", "Móvil", false);
    player.connect.start(0).unwrap();
    first.connect.start(0).unwrap();
    wait(|| device(&first, "tv").is_some_and(|d| d.ready));
    first.connect.stop();
    // The same storage: a restarted app.
    let second = app_on(&network, "phone", "Móvil", false, first.storage.clone());
    assert!(device(&second, "tv").is_some_and(|d| d.paired && !d.online));
    second.connect.start(0).unwrap();
    wait(|| device(&second, "tv").is_some_and(|d| d.ready));
}

// === COMMON ===

#[test]
fn nothing_is_visible_until_it_is_turned_on() {
    let network = Arc::new(MemoryNetwork::default());
    let one = app(&network, "a", "Uno", true);
    assert!(!one.connect.is_running());
    assert_eq!(one.connect.pin(), None);
    one.connect.start(0).unwrap();
    assert!(one.connect.pin().is_some());
    one.connect.stop();
    assert_eq!(one.connect.pin(), None);
}

#[test]
fn sending_without_a_selected_device_or_a_nonsense_command_fails_here() {
    let (_player, controller) = open_pair();
    assert_eq!(controller.connect.send(Command::Next), Err(ConnectError::NoSession));
    controller.connect.select("tv").unwrap();
    assert!(matches!(controller.connect.send(Command::Volume { volume: 9.0 }), Err(ConnectError::Other(_))));
}

#[test]
fn choosing_none_lets_go_of_the_device_without_calling_it_a_drop() {
    let (_player, controller) = open_pair();
    controller.connect.select("tv").unwrap();
    controller.connect.deselect();
    assert_eq!(controller.connect.selected(), None);
    assert_eq!(controller.connect.send(Command::Next), Err(ConnectError::NoSession));
    assert!(controller.heard.closed.lock().unwrap().is_empty());
    // Still connected: what it plays is still known and it can be chosen again at once.
    assert!(device(&controller, "tv").unwrap().ready);
    controller.connect.select("tv").unwrap();
}

#[test]
fn a_late_controller_gets_what_was_published_before() {
    let (player, controller) = open_pair();
    controller.connect.deselect();
    player.connect.publish(PlayerState { playing: true, position: 9.0, volume: 0.3, ..PlayerState::default() });
    wait(|| controller.heard.states.lock().unwrap().iter().any(|(_, s)| s.position == 9.0 && s.volume == 0.3));
    assert_eq!(controller.connect.state_of("tv").map(|s| s.position), Some(9.0));
}

#[test]
fn a_device_added_by_address_finds_the_other_back_by_itself() {
    // Two networks that do not see each other: no discovery between them at all.
    let (player, controller) = (app(&Arc::new(MemoryNetwork::default()), "tv", "Salón", false), app(&Arc::new(MemoryNetwork::default()), "phone", "Móvil", false));
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    assert!(controller.connect.devices().is_empty() && player.connect.devices().is_empty());
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], player.connect.port().unwrap()));
    controller.connect.add_by_address(addr).unwrap();
    wait(|| device(&controller, "tv").is_some_and(|d| d.ready));
    // Whoever was typed in tells where it listens, so the other side finds it back and can control it too.
    wait(|| device(&player, "phone").is_some_and(|d| d.ready));
}

#[test]
fn this_device_says_where_it_can_be_reached_only_while_it_runs() {
    let network = Arc::new(MemoryNetwork::default());
    let one = app(&network, "a", "Uno", false);
    assert!(one.connect.addresses().is_empty());
    one.connect.start(0).unwrap();
    assert!(one.connect.discovers());
    if let Some(address) = one.connect.addresses().first() {
        assert!(address.ends_with(&format!(":{}", one.connect.port().unwrap())));
    }
}

#[test]
fn a_computer_that_comes_back_with_a_new_id_does_not_leave_its_old_one_in_the_list() {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv-new", "Salón", false);
    // The controller paired once with the same computer under an id it has since lost, and with another one called differently.
    let storage = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    let shared: Arc<dyn Storage> = storage.clone();
    let before = KnownDevices::load(shared, Location::path("/data/devices.json"));
    before.remember(super::pairing::Known { device_id: "tv-old".into(), name: "Salón".into(), kind: "desktop".into(), token: "t".into() }).unwrap();
    before.remember(super::pairing::Known { device_id: "other".into(), name: "Cocina".into(), kind: "desktop".into(), token: "t".into() }).unwrap();
    let controller = app_on(&network, "phone", "Móvil", false, storage);
    assert!(device(&controller, "tv-old").is_some());
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| device(&controller, "tv-new").is_some_and(|d| d.ready));
    wait(|| device(&controller, "tv-old").is_none());
    assert!(device(&controller, "other").is_some(), "a device with another name stays");
}
