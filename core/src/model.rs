//! What is kept on disk, with the same JSON shape the Java app writes: a
//! portable that already has `data/` keeps reading it as is.
//!
//! Every field is optional on reading (`#[serde(default)]`) and unknown fields
//! are ignored: a real file can carry fields from an earlier version, and
//! failing on them used to lose the WHOLE file (with settings.json, the user's
//! entire configuration — a real bug in the Java app).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// One video of an archive, keyed by its YouTube id in [`ArchiveData::entries`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchiveEntry {
    /// "007": the number that goes in the file name, set by download order.
    /// Not the same as `playlist_index`.
    pub number: Option<String>,
    pub title: Option<String>,
    /// Where the file is: an absolute path, or a `content://` URI on Android.
    pub file_path: Option<String>,
    pub added_at: Option<String>,
    /// The file was there and is not any more. Does not say why.
    pub deleted_at: Option<String>,
    pub removed_from_playlist_at: Option<String>,
    pub unavailable_at: Option<String>,
    pub unavailable_reason: Option<String>,
    /// Set by the automatic second pass when no client could download it. Not
    /// permanent: the next update tries again.
    pub last_failed_at: Option<String>,
    pub last_failed_reason: Option<String>,
    /// Its position in the source playlist, refreshed on every sync. Only used
    /// to order an unnumbered playlist the way YouTube does.
    pub playlist_index: Option<i32>,
    /// Device-backed playlists (MTP) only: the file name on the device.
    pub device_file_name: Option<String>,
}

/// The registry of one playlist in one format (or of the single downloads of a
/// format): `data/history/archives/pl_<Name>_<hash>/<format>/data.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchiveData {
    /// The source playlist. None for single downloads.
    pub url: Option<String>,
    pub folder: Option<String>,
    // Device-backed playlists (MTP). None in every ordinary playlist; kept so
    // reading and rewriting the Java app's files loses nothing.
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub device_path: Option<String>,
    /// Only device-backed playlists store it: a local one takes it from its folder name.
    pub title: Option<String>,
    /// Decided ONCE when the playlist is registered and never changed: the
    /// number lives in the real file names. Playlists from before this setting
    /// carry an explicit `true`.
    pub number_files: bool,
    pub last_number: Option<String>,
    /// Stamped at the START of a run, right after checking the playlist.
    pub last_synced_at: Option<String>,
    /// Stamped when a run truly finishes: the Gestor sorts by it.
    pub last_activity_at: Option<String>,
    /// In the order they came in, like the Java LinkedHashMap.
    pub entries: IndexMap<String, ArchiveEntry>,
}

impl ArchiveData {
    /// Derived, never written (the Java app had a stray `deviceBacked` field once).
    pub fn is_device_backed(&self) -> bool {
        self.device_id.as_deref().is_some_and(|id| !id.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// `data/config/settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserSettings {
    pub theme: Theme,
    pub ui_scale: i32,
    // Java writes a String it never set as `null`: read as the default instead
    // of rejecting the whole file (which fell back to factory settings).
    #[serde(deserialize_with = "null_as_language")]
    pub language: String,
    #[serde(deserialize_with = "null_as_empty")]
    pub downloads_folder: String,
    #[serde(deserialize_with = "null_as_empty")]
    pub cookies_file: String,
    pub auto_updates: bool,
    pub include_id_in_title_name: bool,
    /// The yt-dlp `player_client` that last got past YouTube's bot check.
    pub working_player_client: Option<String>,
    pub app_update_check_enabled: bool,
    #[serde(deserialize_with = "null_as_empty")]
    pub last_app_update_check: String,
    pub task_history_visible: bool,
    pub player_volume: f64,
    pub last_played_archive: Option<String>,
    pub last_played_track_id: Option<String>,
    pub last_played_playlist: Option<String>,
    pub last_played_format: Option<String>,
    pub last_played_position: f64,
    /// The full look ("dark-blue", "amoled"...): the Java app only knows
    /// `theme`, DARK or LIGHT, and an enum value it does not know would make
    /// its Jackson reject the whole file. A field it does not know, it skips.
    /// See [`crate::settings::preferences`].
    #[serde(deserialize_with = "null_as_empty")]
    pub appearance: String,
    /// Where the player shows: "side", "bottom" or "both".
    #[serde(deserialize_with = "null_as_empty")]
    pub player_placement: String,
    /// A video goes to the floating panel by itself when Now Playing is left.
    pub auto_pip: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            ui_scale: 100,
            language: "es".into(),
            downloads_folder: String::new(),
            cookies_file: String::new(),
            auto_updates: true,
            include_id_in_title_name: false,
            working_player_client: None,
            app_update_check_enabled: true,
            last_app_update_check: String::new(),
            task_history_visible: true,
            player_volume: 0.7,
            last_played_archive: None,
            last_played_track_id: None,
            last_played_playlist: None,
            last_played_format: None,
            last_played_position: 0.0,
            appearance: String::new(),
            player_placement: String::new(),
            auto_pip: true,
        }
    }
}

/// What Ajustes shows and changes (a slice of [`UserSettings`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub appearance: String,
    pub language: String,
    pub ui_scale: i32,
    pub player_placement: String,
    pub auto_pip: bool,
    pub include_id_in_title_name: bool,
    pub app_update_check_enabled: bool,
    pub auto_updates: bool,
    pub cookies_file: String,
    pub last_app_update_check: String,
}

/// A change from Ajustes: only what is set is touched.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreferencesPatch {
    pub appearance: Option<String>,
    pub language: Option<String>,
    pub ui_scale: Option<i32>,
    pub player_placement: Option<String>,
    pub auto_pip: Option<bool>,
    pub include_id_in_title_name: Option<bool>,
    pub app_update_check_enabled: Option<bool>,
    pub auto_updates: Option<bool>,
    pub cookies_file: Option<String>,
}

/// What the player keeps between launches (a slice of [`UserSettings`]): the
/// volume, and the last file with the second it was left at, so the next
/// launch brings it back to the bar, paused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMemory {
    pub volume: f64,
    pub last: Option<LastPlayed>,
}

/// The last file played: its archive (the block it came from), its id, and the
/// playlist and format it was shown under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastPlayed {
    pub archive: String,
    pub track_id: String,
    pub playlist: String,
    pub format: String,
    pub position: f64,
}

/// The `soundify-playlist.json` that sits inside a playlist's music folder: it
/// describes WHAT is in it, never WHERE (no absolute paths, nothing of this
/// session). It is the playlist's passport: any music folder that carries one
/// can be imported as is. See [`crate::transfer`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistManifest {
    pub schema: i32,
    #[serde(deserialize_with = "null_as_empty")]
    pub exported_at: String,
    #[serde(deserialize_with = "null_as_empty")]
    pub title: String,
    pub url: Option<String>,
    #[serde(deserialize_with = "null_as_empty")]
    pub format: String,
    pub number_files: bool,
    #[serde(deserialize_with = "null_as_empty")]
    pub folder_name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub items: Vec<ManifestItem>,
}

/// One id inside the manifest. `file_name` is the file's NAME, with no folder,
/// and None if that item is not downloaded (pending or unavailable): on import
/// it stays pending.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManifestItem {
    #[serde(deserialize_with = "null_as_empty")]
    pub id: String,
    pub number: Option<String>,
    pub title: Option<String>,
    pub file_name: Option<String>,
    pub unavailable_at: Option<String>,
    pub unavailable_reason: Option<String>,
    pub removed_from_playlist_at: Option<String>,
}

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn null_as_language<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_else(|| UserSettings::default().language))
}

/// One finished background task, as `data/history/tasks.json` keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskRecord {
    /// OK / WARNING / FAILED / STOPPED (see [`crate::task_history`]).
    pub status: Option<String>,
    pub title: Option<String>,
    /// An i18n key, so the history follows the language in use.
    pub origin: Option<String>,
    /// The result line: an i18n key when this app wrote it (with
    /// [`TaskRecord::summary_args`]), already translated text when the Java app
    /// did. The front translates it if it is a key and shows it as is if not.
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub summary_args: Vec<String>,
    pub finished_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_from_earlier_versions_still_load() {
        let json = r#"{
            "url": "https://www.youtube.com/playlist?list=PL1",
            "folder": "D:\\Music\\Chill (mp3)",
            "lastReconciledAt": "2025-01-01 10:00",
            "syncWithPlaylist": true,
            "entries": { "abcdefghijk": { "number": "001", "deleteReason": "MANUAL" } }
        }"#;
        let data: ArchiveData = serde_json::from_str(json).unwrap();
        assert_eq!(data.entries["abcdefghijk"].number.as_deref(), Some("001"));
        assert!(!data.number_files);
    }

    #[test]
    fn device_backed_is_derived_and_never_written() {
        let mut data = ArchiveData::default();
        assert!(!data.is_device_backed());
        data.device_id = Some("  ".into());
        assert!(!data.is_device_backed());
        data.device_id = Some("usb#1".into());
        assert!(data.is_device_backed());
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.contains("deviceBacked"));
    }

    #[test]
    fn entries_keep_the_order_they_came_in() {
        let json = r#"{ "entries": { "zzzzzzzzzzz": {}, "aaaaaaaaaaa": {}, "mmmmmmmmmmm": {} } }"#;
        let data: ArchiveData = serde_json::from_str(json).unwrap();
        let ids: Vec<&str> = data.entries.keys().map(String::as_str).collect();
        assert_eq!(ids, ["zzzzzzzzzzz", "aaaaaaaaaaa", "mmmmmmmmmmm"]);
    }

    #[test]
    fn settings_missing_fields_take_the_defaults() {
        let settings: UserSettings = serde_json::from_str(r#"{ "theme": "LIGHT", "removedSetting": 3 }"#).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(settings.ui_scale, 100);
        assert_eq!(settings.player_volume, 0.7);
        assert!(settings.auto_updates);
    }
}
