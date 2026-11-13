//! What two Soundify apps say to each other (see `PLAN-CONNECT.md`): the device's
//! description, the player's state, the commands and their answers. Everything is
//! JSON, one message per WebSocket text frame.

use serde::{Deserialize, Serialize};

/// The version of this protocol; a different one is refused instead of guessed at.
pub const PROTOCOL: u32 = 1;
/// The mDNS service every Soundify announces.
pub const SERVICE_TYPE: &str = "_soundify-connect._tcp.local.";
/// Where the HTTP routes live (`/info`, `/pair`, `/ws`).
pub const BASE: &str = "/connect/v1";

/// What a device is, as it announces itself and as `GET /info` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub v: u32,
    /// Chosen once at random and kept: the same device is the same one next time.
    pub device_id: String,
    /// What the user calls it ("PC de Dani").
    pub name: String,
    /// `desktop`, `phone` or `tablet`.
    pub kind: String,
    /// It lets any device on the network pair without a PIN (the default: like Spotify, it just shows up).
    /// Off = «ask for a PIN», the stricter way. Only `GET /info` says it truthfully: it can change while the app runs.
    #[serde(default)]
    pub open: bool,
}

/// What a player is playing. The video id, not a path: it means the same on every device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub video_id: String,
    pub title: String,
    pub playlist: String,
    pub format: String,
    pub video: bool,
}

/// The whole state of a player at a moment. Sent in full every time: it is small.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub now_playing: Option<NowPlaying>,
    pub playing: bool,
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    /// 0 to 1.
    pub volume: f64,
    pub shuffle: bool,
    /// `off`, `all` or `one`.
    pub repeat: String,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self { now_playing: None, playing: false, position: 0.0, duration: 0.0, volume: 1.0, shuffle: false, repeat: "off".into() }
    }
}

/// One file of a queue that is handed to another device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub video_id: String,
    pub title: String,
}

/// What a controller can ask a player to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek { seconds: f64 },
    Volume { volume: f64 },
    /// `on` absent = flip it.
    Shuffle { on: Option<bool> },
    /// `mode` absent = the next one (off, all, one).
    Repeat { mode: Option<String> },
    /// Take the playback over: this queue (by video id), starting at `index`, at `position` seconds.
    #[serde(rename_all = "camelCase")]
    Load { playlist: String, queue: Vec<QueueItem>, index: usize, position: f64, autoplay: bool },
}

/// The most files a handed-over queue can have (a 700-file playlist fits).
pub const MAX_QUEUE: usize = 5000;

impl Command {
    /// Is it something a player can do? A controller is not trusted to send sane numbers.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Command::Seek { seconds } if !seconds.is_finite() || *seconds < 0.0 => Err("seek: bad position".into()),
            Command::Volume { volume } if !volume.is_finite() || !(0.0..=1.0).contains(volume) => Err("volume: out of range".into()),
            Command::Repeat { mode: Some(mode) } if !["off", "all", "one"].contains(&mode.as_str()) => Err("repeat: unknown mode".into()),
            Command::Load { queue, index, position, .. } => {
                if queue.is_empty() || queue.len() > MAX_QUEUE {
                    Err("load: the queue is empty or too long".into())
                } else if *index >= queue.len() {
                    Err("load: the index is out of the queue".into())
                } else if !position.is_finite() || *position < 0.0 {
                    Err("load: bad position".into())
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

/// Player → controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ServerMessage {
    /// First thing after the handshake: who answers.
    Hello { device: DeviceInfo },
    State { state: PlayerState },
    Ack { id: u64, ok: bool, error: Option<String> },
}

/// Controller → player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ClientMessage {
    Cmd {
        id: u64,
        #[serde(flatten)]
        command: Command,
    },
}

/// The body of `POST /pair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub controller_id: String,
    pub name: String,
    pub pin: String,
}

/// The answer to a good PIN: the secret both sides keep (base64).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResponse {
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_travels_flat_with_its_id() {
        let message = ClientMessage::Cmd { id: 7, command: Command::Seek { seconds: 12.5 } };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json, serde_json::json!({ "t": "cmd", "id": 7, "cmd": "seek", "seconds": 12.5 }));
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), message);
    }

    #[test]
    fn commands_without_arguments_parse() {
        let message: ClientMessage = serde_json::from_str(r#"{"t":"cmd","id":1,"cmd":"next"}"#).unwrap();
        assert_eq!(message, ClientMessage::Cmd { id: 1, command: Command::Next });
        let shuffle: ClientMessage = serde_json::from_str(r#"{"t":"cmd","id":2,"cmd":"shuffle"}"#).unwrap();
        assert_eq!(shuffle, ClientMessage::Cmd { id: 2, command: Command::Shuffle { on: None } });
    }

    #[test]
    fn an_unknown_command_does_not_parse() {
        assert!(serde_json::from_str::<ClientMessage>(r#"{"t":"cmd","id":1,"cmd":"format-disk"}"#).is_err());
    }

    #[test]
    fn nonsense_numbers_are_refused() {
        assert!(Command::Seek { seconds: -1.0 }.validate().is_err());
        assert!(Command::Seek { seconds: f64::NAN }.validate().is_err());
        assert!(Command::Volume { volume: 1.5 }.validate().is_err());
        assert!(Command::Repeat { mode: Some("sideways".into()) }.validate().is_err());
        assert!(Command::Volume { volume: 0.4 }.validate().is_ok());
    }

    #[test]
    fn a_load_needs_a_queue_the_index_points_into() {
        let item = |id: &str| QueueItem { video_id: id.into(), title: id.into() };
        let load = |queue: Vec<QueueItem>, index| Command::Load { playlist: "P".into(), queue, index, position: 3.0, autoplay: true };
        assert!(load(vec![], 0).validate().is_err());
        assert!(load(vec![item("a")], 1).validate().is_err());
        assert!(load(vec![item("a"), item("b")], 1).validate().is_ok());
        assert!(load((0..MAX_QUEUE + 1).map(|n| item(&n.to_string())).collect(), 0).validate().is_err());
    }

    #[test]
    fn the_state_defaults_to_nothing_playing_at_full_volume() {
        let state = PlayerState::default();
        assert!(state.now_playing.is_none() && !state.playing);
        assert_eq!((state.volume, state.repeat.as_str()), (1.0, "off"));
    }
}
