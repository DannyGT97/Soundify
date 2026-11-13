//! Two apps on one (in-memory) network: the whole way from «I see it» to «I control it».

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::discovery::{Discovery, MemoryDiscovery, MemoryNetwork};
use super::manager::{Connect, ConnectError, Identity, Listener, Player};
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
    closed: Mutex<Vec<String>>,
}

impl Listener for Heard {
    fn devices_changed(&self) {}

    fn remote_state(&self, device_id: &str, state: PlayerState) {
        self.states.lock().unwrap().push((device_id.to_string(), state));
    }

    fn remote_closed(&self, device_id: &str, _: Option<String>) {
        self.closed.lock().unwrap().push(device_id.to_string());
    }
}

struct App {
    connect: Arc<Connect>,
    commands: Arc<Commands>,
    heard: Arc<Heard>,
}

fn app(network: &Arc<MemoryNetwork>, id: &str, name: &str) -> App {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::default());
    storage.create_dir_all(&Location::path("/data")).unwrap();
    let pairing = Arc::new(Pairing::load(storage.clone(), Location::path("/data/trusted.json")));
    let known = KnownDevices::load(storage, Location::path("/data/devices.json"));
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
    App { connect, commands, heard }
}

fn wait(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("it did not happen in time");
}

/// A player and a controller, both on, with the player's PIN typed on the controller.
fn pair_up() -> (App, App) {
    let network = Arc::new(MemoryNetwork::default());
    let player = app(&network, "tv", "Salón");
    let controller = app(&network, "phone", "Móvil");
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| controller.connect.devices().iter().any(|d| d.id == "tv"));
    let pin = player.connect.pin().unwrap();
    controller.connect.pair("tv", &pin).unwrap();
    (player, controller)
}

#[test]
fn a_device_sees_the_others_but_not_itself() {
    let network = Arc::new(MemoryNetwork::default());
    let (one, two) = (app(&network, "a", "Uno"), app(&network, "b", "Dos"));
    one.connect.start(0).unwrap();
    two.connect.start(0).unwrap();
    wait(|| one.connect.devices().len() == 1 && two.connect.devices().len() == 1);
    assert_eq!(one.connect.devices()[0].id, "b");
    let seen = &two.connect.devices()[0];
    assert_eq!((seen.id.as_str(), seen.online, seen.paired, seen.connected), ("a", true, false, false));
}

#[test]
fn nothing_is_pinned_until_it_is_turned_on() {
    let network = Arc::new(MemoryNetwork::default());
    let one = app(&network, "a", "Uno");
    assert!(!one.connect.is_running());
    assert_eq!(one.connect.pin(), None);
    one.connect.start(0).unwrap();
    assert!(one.connect.pin().is_some());
    one.connect.stop();
    assert_eq!(one.connect.pin(), None);
}

#[test]
fn pairing_with_the_pin_makes_the_device_known_and_it_stays_when_it_goes_offline() {
    let (player, controller) = pair_up();
    assert!(controller.connect.devices().iter().any(|d| d.id == "tv" && d.paired));
    assert_eq!(player.connect.controllers()[0].name, "Móvil");
    player.connect.stop();
    wait(|| controller.connect.devices().iter().any(|d| d.id == "tv" && !d.online));
    assert!(controller.connect.devices()[0].paired);
}

#[test]
fn a_wrong_pin_pairs_nothing() {
    let network = Arc::new(MemoryNetwork::default());
    let (player, controller) = (app(&network, "tv", "Salón"), app(&network, "phone", "Móvil"));
    player.connect.start(0).unwrap();
    controller.connect.start(0).unwrap();
    wait(|| !controller.connect.devices().is_empty());
    let pin = player.connect.pin().unwrap();
    let wrong = if pin == "000000" { "111111" } else { "000000" };
    assert!(matches!(controller.connect.pair("tv", wrong), Err(ConnectError::Refused { status: 403, .. })));
    assert!(!controller.connect.devices()[0].paired);
    // Not paired: it cannot be controlled either.
    assert_eq!(controller.connect.select("tv"), Err(ConnectError::NotPaired));
    assert_eq!(controller.connect.pair("nobody", "123456"), Err(ConnectError::Offline));
}

#[test]
fn controlling_a_device_sends_it_commands_and_shows_its_state() {
    let (player, controller) = pair_up();
    controller.connect.select("tv").unwrap();
    assert_eq!(controller.connect.selected().as_deref(), Some("tv"));
    assert!(controller.connect.devices().iter().any(|d| d.id == "tv" && d.connected));

    controller.connect.send(Command::Next).unwrap();
    controller.connect.send(Command::Volume { volume: 0.5 }).unwrap();
    wait(|| player.commands.0.lock().unwrap().len() == 2);
    assert_eq!(player.commands.0.lock().unwrap().as_slice(), [Command::Next, Command::Volume { volume: 0.5 }]);

    player.connect.publish(PlayerState { playing: true, position: 30.0, ..PlayerState::default() });
    wait(|| controller.heard.states.lock().unwrap().iter().any(|(id, s)| id == "tv" && s.playing && s.position == 30.0));
    assert_eq!(player.connect.connected_controllers(), 1);
}

#[test]
fn sending_without_a_selected_device_or_a_nonsense_command_fails_here() {
    let (_player, controller) = pair_up();
    assert_eq!(controller.connect.send(Command::Next), Err(ConnectError::NoSession));
    controller.connect.select("tv").unwrap();
    assert!(matches!(controller.connect.send(Command::Volume { volume: 9.0 }), Err(ConnectError::Other(_))));
}

#[test]
fn choosing_none_lets_go_of_the_device_without_calling_it_a_drop() {
    let (player, controller) = pair_up();
    controller.connect.select("tv").unwrap();
    controller.connect.deselect();
    assert_eq!(controller.connect.selected(), None);
    assert_eq!(controller.connect.send(Command::Next), Err(ConnectError::NoSession));
    wait(|| player.connect.connected_controllers() == 0);
    assert!(controller.heard.closed.lock().unwrap().is_empty());
}

#[test]
fn when_the_player_goes_away_the_controller_is_told() {
    let (player, controller) = pair_up();
    controller.connect.select("tv").unwrap();
    player.connect.stop();
    wait(|| controller.heard.closed.lock().unwrap().as_slice() == ["tv"]);
    assert_eq!(controller.connect.selected(), None);
}

#[test]
fn a_token_the_player_no_longer_accepts_is_forgotten_so_the_user_pairs_again() {
    let (player, controller) = pair_up();
    player.connect.forget_controller("phone").unwrap();
    assert!(matches!(controller.connect.select("tv"), Err(ConnectError::Refused { status: 401, .. })));
    assert!(controller.connect.devices().iter().all(|d| !(d.id == "tv" && d.paired)));
}

#[test]
fn forgetting_a_device_stops_controlling_it() {
    let (_player, controller) = pair_up();
    controller.connect.select("tv").unwrap();
    controller.connect.forget("tv").unwrap();
    assert_eq!(controller.connect.selected(), None);
    assert!(controller.connect.devices().iter().all(|d| !d.paired));
}

#[test]
fn a_late_controller_gets_what_was_published_before() {
    let (player, controller) = pair_up();
    player.connect.publish(PlayerState { playing: true, position: 9.0, volume: 0.3, ..PlayerState::default() });
    controller.connect.select("tv").unwrap();
    wait(|| controller.heard.states.lock().unwrap().iter().any(|(_, s)| s.position == 9.0 && s.volume == 0.3));
}
