//! What two Soundify apps say to each other (see `PLAN-CONNECT.md`): the device's
//! description, the player's state, the commands and their answers. Everything is
//! JSON, one message per WebSocket text frame.

use serde::{Deserialize, Serialize};

/// The version of this protocol; a different one is refused instead of guessed at.
/// 2: everything but `/info` over TLS, pairing with SPAKE2 (1.2.x spoke 1, in the clear).
pub const PROTOCOL: u32 = 2;
/// The mDNS service every Soundify announces.
pub const SERVICE_TYPE: &str = "_soundify-connect._tcp.local.";
/// Where the HTTP routes live (`/info`, `/pair`, `/ws`).
pub const BASE: &str = "/connect/v1";

/// A device name as an HTTP header value (percent-encoded: a header carries visible ASCII only, a name may have accents or emoji).
pub fn encode_name(name: &str) -> String {
    let mut out = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b' ') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The name a header carried, or None if it is not a name worth keeping (empty, too long, or with control characters).
pub fn decode_name(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut raw = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            raw.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            raw.push(bytes[i]);
            i += 1;
        }
    }
    let name = String::from_utf8(raw).ok()?;
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= 40 && !name.chars().any(char::is_control)).then(|| name.to_string())
}

#[cfg(test)]
mod name_tests {
    use super::{decode_name, encode_name};

    #[test]
    fn a_name_goes_through_a_header_and_comes_back() {
        for name in ["MSI-PC", "S24+ D", "Móvil de Dani 🎵", "H MP2 D"] {
            let encoded = encode_name(name);
            assert!(encoded.is_ascii() && !encoded.contains(['\r', '\n']), "{encoded}");
            assert_eq!(decode_name(&encoded).as_deref(), Some(name));
        }
    }

    #[test]
    fn a_bad_name_is_not_kept() {
        assert_eq!(decode_name(""), None);
        assert_eq!(decode_name("%0A"), None);
        assert_eq!(decode_name("%zz"), None);
        assert_eq!(decode_name(&"a".repeat(41)), None);
    }
}

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
    /// «Escuchar en»: the controller passes what IT plays to this device, which plays it reading the files from the controller
    /// (`GET /connect/v1/media/<videoId>?t=<ticket>` on `host:port`). The queue comes in the order it sounds there, so
    /// «next» is the same on both sides. See [`CastStart`].
    Cast(Box<CastStart>),
    /// The controller ended its pass («Traer aquí», «Detener», passed to another device): stop playing what it passed.
    CastEnd {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Asks for the files of one playlist of this device's library (the key of a [`LibraryEntry`]): the device answers with a
    /// `ServerMessage::Browse`. Changes nothing of what plays: it is how a controller lists a playlist that is not playing.
    Browse { key: String },
    /// Plays this file of that playlist of this device's library, from the start (what a controller does when it picks a file
    /// of a playlist it browsed).
    #[serde(rename_all = "camelCase")]
    PlayFrom { key: String, video_id: String },
}

/// One playlist (in one format) of a device's own library: what a controller lists to pick from before anything plays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    /// Only means something to the device that gave it (it is sent back in `Browse` and `PlayFrom`).
    pub key: String,
    pub title: String,
    /// `MP3`, `MP4`...
    pub format: String,
    pub video: bool,
    /// Files on that device (the ones that can be played).
    pub count: u32,
    /// The video id of its first file: its picture is that video's thumbnail (the same one the lists use), which the
    /// controller fetches itself if it does not have it. Absent from older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

/// The most playlists of a library that are listed to a controller.
pub const MAX_LIBRARY: usize = 1000;

/// One file of a queue that is passed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastItem {
    pub video_id: String,
    pub title: String,
    /// Seconds (0 = not known).
    #[serde(default)]
    pub duration: f64,
    /// The extension of the file (`mp3`, `mp4`...): what the player needs to know before reading it.
    pub format: String,
    #[serde(default)]
    pub video: bool,
    /// Has a picture to ask for (`GET /connect/v1/cover/<videoId>`).
    #[serde(default)]
    pub cover: bool,
}

/// What a pass carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastStart {
    /// Where to read from. NOT trusted from the message: the server writes there the address the command came from
    /// (a controller cannot make this device fetch from a third one).
    #[serde(default)]
    pub host: String,
    /// The port of the controller's own Connect server.
    pub port: u16,
    /// The fingerprint of the controller's TLS certificate: the files are read from that certificate only.
    #[serde(default)]
    pub fingerprint: String,
    pub ticket: String,
    pub playlist: String,
    pub items: Vec<CastItem>,
    /// Which one sounds.
    pub index: usize,
    /// The second it is at.
    pub position: f64,
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: String,
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
            Command::Cast(cast) => cast.validate(),
            Command::Browse { key } if key.is_empty() || key.len() > 1024 => Err("browse: bad key".into()),
            Command::PlayFrom { key, video_id } if key.is_empty() || key.len() > 1024 || video_id.is_empty() || video_id.len() > 128 => Err("playFrom: bad key or id".into()),
            _ => Ok(()),
        }
    }
}

impl CastStart {
    fn validate(&self) -> Result<(), String> {
        if self.port == 0 || !super::cast::is_ticket(&self.ticket) || !super::tls::is_fingerprint(&self.fingerprint) {
            return Err("cast: bad source".into());
        }
        if self.items.is_empty() || self.items.len() > MAX_QUEUE || self.index >= self.items.len() {
            return Err("cast: bad queue".into());
        }
        if !self.position.is_finite() || self.position < 0.0 || !["off", "all", "one"].contains(&self.repeat.as_str()) {
            return Err("cast: bad state".into());
        }
        let bad = |item: &CastItem| {
            item.video_id.is_empty() || item.video_id.len() > 128 || item.title.len() > 1024 || item.format.is_empty() || item.format.len() > 16
                || !item.duration.is_finite() || item.duration < 0.0
        };
        if self.items.iter().any(bad) {
            return Err("cast: bad item".into());
        }
        Ok(())
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
    /// The playlists of the device's own library (summary: no files). Said when a controller connects and when it changes.
    Library { playlists: Vec<LibraryEntry> },
    /// The files of the playlist a controller asked for (`Command::Browse`); `None` = there is no such playlist (any more).
    Browse { key: String, queue: Option<QueueInfo> },
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
    /// The controller's SPAKE2 message made with the PIN (base64; empty for an open device). The PIN itself never travels.
    #[serde(default)]
    pub spake: String,
}

/// The first half of a pairing, answered: the token (an open device), or SPAKE2's answer and where to confirm it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairStarted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spake: Option<String>,
}

/// The second half: the controller's proof that it knew the PIN and saw this device's certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairConfirm {
    pub session: String,
    pub confirm: String,
}

/// The answer to a good confirmation: the secret both sides keep (base64), and the device's own proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResponse {
    pub token: String,
    pub confirm: String,
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

    fn cast() -> CastStart {
        CastStart {
            host: String::new(),
            port: 43210,
            fingerprint: "0f".repeat(32),
            ticket: "ab".repeat(32),
            playlist: "Rock".into(),
            items: vec![
                CastItem { video_id: "a".into(), title: "Uno".into(), duration: 200.0, format: "mp3".into(), video: false, cover: true },
                CastItem { video_id: "b".into(), title: "Dos".into(), duration: 0.0, format: "mp3".into(), video: false, cover: false },
            ],
            index: 1,
            position: 12.5,
            playing: true,
            shuffle: false,
            repeat: "all".into(),
        }
    }

    #[test]
    fn a_pass_travels_flat_and_back() {
        let message = ClientMessage::Cmd { id: 3, command: Command::Cast(Box::new(cast())) };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["cmd"], "cast");
        assert_eq!(json["items"][0]["videoId"], "a");
        assert_eq!(json["port"], 43210);
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), message);
        let end: ClientMessage = serde_json::from_str(r#"{"t":"cmd","id":4,"cmd":"castEnd"}"#).unwrap();
        assert_eq!(end, ClientMessage::Cmd { id: 4, command: Command::CastEnd { reason: None } });
        assert_eq!(serde_json::to_value(Command::CastEnd { reason: None }).unwrap(), serde_json::json!({ "cmd": "castEnd" }));
    }

    #[test]
    fn a_pass_that_makes_no_sense_is_refused() {
        assert!(Command::Cast(Box::new(cast())).validate().is_ok());
        let broken: Vec<fn(&mut CastStart)> = vec![
            |c| c.port = 0,
            |c| c.ticket = "short".into(),
            |c| c.ticket = "zz".repeat(32),
            |c| c.fingerprint = "short".into(),
            |c| c.items.clear(),
            |c| c.index = 2,
            |c| c.position = f64::NAN,
            |c| c.position = -1.0,
            |c| c.repeat = "twice".into(),
            |c| c.items[0].video_id = String::new(),
            |c| c.items[0].format = String::new(),
            |c| c.items[0].duration = f64::INFINITY,
        ];
        for (n, breaks) in broken.iter().enumerate() {
            let mut bad = cast();
            breaks(&mut bad);
            assert!(Command::Cast(Box::new(bad)).validate().is_err(), "case {n}");
        }
    }

    #[test]
    fn browsing_a_library_travels_and_is_checked() {
        let entry = LibraryEntry { key: "pl1:MP3".into(), title: "Rock".into(), format: "MP3".into(), video: false, count: 12, cover: Some("abc".into()) };
        let message = ServerMessage::Library { playlists: vec![entry] };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["t"], "library");
        assert_eq!(json["playlists"][0]["count"], 12);
        assert_eq!(serde_json::from_value::<ServerMessage>(json).unwrap(), message);
        let browse = ClientMessage::Cmd { id: 1, command: Command::PlayFrom { key: "pl1:MP3".into(), video_id: "abc".into() } };
        let json = serde_json::to_value(&browse).unwrap();
        assert_eq!((json["cmd"].as_str(), json["videoId"].as_str()), (Some("playFrom"), Some("abc")));
        assert_eq!(serde_json::from_value::<ClientMessage>(json).unwrap(), browse);
        assert!(Command::Browse { key: String::new() }.validate().is_err());
        assert!(Command::PlayFrom { key: "k".into(), video_id: "x".repeat(129) }.validate().is_err());
        assert!(Command::Browse { key: "pl1:MP3".into() }.validate().is_ok());
    }
}
