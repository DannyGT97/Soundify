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

/// Both ends of a WebSocket ping every so often and let go of it if nothing at all arrived for this long: a locked phone or a
/// Wi-Fi that drops closes nothing, and without this the dead connection would only be noticed minutes later.
pub const PING_EVERY: std::time::Duration = std::time::Duration::from_secs(15);
pub const SILENCE_LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

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

/// One file of the playlist a device is playing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub video_id: String,
    pub title: String,
}

/// The playlist a device is playing, in the order it shows it: what a controller lists (a remote playlist) to pick a song from.
/// Which one sounds is in the `PlayerState`, by video id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueInfo {
    pub playlist: String,
    pub items: Vec<QueueItem>,
}

impl QueueInfo {
    /// Cut to what is allowed on the network (a playlist longer than `MAX_QUEUE` is listed up to there).
    pub fn limited(mut self) -> Self {
        self.items.truncate(MAX_QUEUE);
        self
    }
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
    /// `on` absent = flip it. Never written as `null`: the page reads «absent», and `null` is neither a mode nor a bool.
    Shuffle {
        #[serde(skip_serializing_if = "Option::is_none")]
        on: Option<bool>,
    },
    /// `mode` absent = the next one (off, all, one). Never written as `null` (see `Shuffle`): a `null` read as an unknown
    /// mode made «repeat» stop at «one» on the device being controlled.
    Repeat {
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
    },
    /// Play this file of the playlist that is playing (a video id of its `QueueInfo`), from the start.
    #[serde(rename_all = "camelCase")]
    PlayTrack { video_id: String },
    /// Sent when a controller picks this device to control: the device stops controlling another one (if it did), says who took
    /// it and tells the controller what it plays now, so both show the same at once. It changes nothing else.
    Take,
}

/// The most files of a playlist that are listed to a controller (a 700-file playlist fits).
pub const MAX_QUEUE: usize = 5000;

impl Command {
    /// Is it something a player can do? A controller is not trusted to send sane numbers.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Command::Seek { seconds } if !seconds.is_finite() || *seconds < 0.0 => Err("seek: bad position".into()),
            Command::Volume { volume } if !volume.is_finite() || !(0.0..=1.0).contains(volume) => Err("volume: out of range".into()),
            Command::Repeat { mode: Some(mode) } if !["off", "all", "one"].contains(&mode.as_str()) => Err("repeat: unknown mode".into()),
            Command::PlayTrack { video_id } if video_id.is_empty() || video_id.len() > 128 => Err("playTrack: bad video id".into()),
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
    /// The playlist that plays (None = nothing). Said after the first state and whenever the playlist changes: it can be long.
    Queue { queue: Option<QueueInfo> },
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
    /// `desktop`, `phone` or `tablet`: what the controller is (older versions do not say).
    #[serde(default)]
    pub kind: String,
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
    fn an_absent_option_is_left_out_of_the_command_not_written_as_null() {
        // The page gets this command as it is: `mode: null` is not «absent» there.
        assert_eq!(serde_json::to_value(Command::Repeat { mode: None }).unwrap(), serde_json::json!({ "cmd": "repeat" }));
        assert_eq!(serde_json::to_value(Command::Shuffle { on: None }).unwrap(), serde_json::json!({ "cmd": "shuffle" }));
        assert_eq!(serde_json::to_value(Command::Repeat { mode: Some("one".into()) }).unwrap(), serde_json::json!({ "cmd": "repeat", "mode": "one" }));
    }

    #[test]
    fn taking_a_device_over_is_a_command_without_arguments() {
        assert_eq!(serde_json::to_value(Command::Take).unwrap(), serde_json::json!({ "cmd": "take" }));
        assert!(Command::Take.validate().is_ok());
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
    fn picking_a_track_needs_a_video_id_and_a_long_playlist_is_cut() {
        assert!(Command::PlayTrack { video_id: String::new() }.validate().is_err());
        assert!(Command::PlayTrack { video_id: "x".repeat(129) }.validate().is_err());
        assert!(Command::PlayTrack { video_id: "dQw4w9WgXcQ".into() }.validate().is_ok());
        let items = (0..MAX_QUEUE + 10).map(|n| QueueItem { video_id: n.to_string(), title: n.to_string() }).collect();
        assert_eq!(QueueInfo { playlist: "P".into(), items }.limited().items.len(), MAX_QUEUE);
    }

    #[test]
    fn the_queue_travels_as_its_own_message() {
        let message = ServerMessage::Queue { queue: Some(QueueInfo { playlist: "P".into(), items: vec![QueueItem { video_id: "a".into(), title: "A".into() }] }) };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json, serde_json::json!({ "t": "queue", "queue": { "playlist": "P", "items": [{ "videoId": "a", "title": "A" }] } }));
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), message);
    }

    #[test]
    fn the_state_defaults_to_nothing_playing_at_full_volume() {
        let state = PlayerState::default();
        assert!(state.now_playing.is_none() && !state.playing);
        assert_eq!((state.volume, state.repeat.as_str()), (1.0, "off"));
    }
}
