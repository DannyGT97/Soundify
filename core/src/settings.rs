//! `data/config/settings.json`: the user's settings.
//!
//! Tolerant on purpose: a missing or broken file gives the defaults instead of
//! stopping the app from starting, and fields from older versions are ignored.

use std::io::Result;

use log::warn;

use crate::Location;
use crate::model::{LastPlayed, PlayerMemory, Preferences, PreferencesPatch, Theme, UserSettings};
use crate::storage::Storage;

pub fn load(storage: &dyn Storage, file: &Location) -> UserSettings {
    if !storage.exists(file).unwrap_or(false) {
        return UserSettings::default();
    }
    match storage.read(file).map(|bytes| serde_json::from_slice(&bytes)) {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            warn!("Broken settings file, using defaults: {e}");
            UserSettings::default()
        }
        Err(e) => {
            warn!("Could not read settings, using defaults: {e}");
            UserSettings::default()
        }
    }
}

pub fn save(storage: &dyn Storage, file: &Location, settings: &UserSettings) -> Result<()> {
    if let Some(parent) = crate::archive::io::parent_of(file) {
        storage.create_dir_all(&parent)?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    storage.write(file, &bytes)
}

pub const LANGUAGES: [&str; 3] = ["es", "en", "ca"];
pub const APPEARANCES: [&str; 9] = ["dark", "light", "dark-neutral", "light-neutral", "dark-red", "dark-blue", "amoled", "light-red", "light-blue"];
pub const PLAYER_PLACEMENTS: [&str; 3] = ["side", "bottom", "both"];

/// The part Ajustes shows. The look is `appearance` only while it still agrees
/// with `theme`: if the Java app switched DARK/LIGHT since, that wins and the
/// look falls back to its plain variant.
pub fn preferences(settings: &UserSettings) -> Preferences {
    let base = if settings.theme == Theme::Light { "light" } else { "dark" };
    let appearance = if APPEARANCES.contains(&settings.appearance.as_str()) && theme_of(&settings.appearance) == settings.theme {
        settings.appearance.clone()
    } else {
        base.to_string()
    };
    let placement = if PLAYER_PLACEMENTS.contains(&settings.player_placement.as_str()) { settings.player_placement.as_str() } else { "both" };
    let language = if LANGUAGES.contains(&settings.language.as_str()) { settings.language.as_str() } else { "es" };
    Preferences {
        appearance,
        language: language.to_string(),
        ui_scale: settings.ui_scale,
        player_placement: placement.to_string(),
        include_id_in_title_name: settings.include_id_in_title_name,
        app_update_check_enabled: settings.app_update_check_enabled,
        auto_updates: settings.auto_updates,
        cookies_file: settings.cookies_file.clone(),
        last_app_update_check: settings.last_app_update_check.clone(),
    }
}

/// Writes a change from Ajustes into `settings`. A value the app does not know
/// (a language or look from a newer version) is left out instead of saved.
pub fn apply(settings: &mut UserSettings, patch: &PreferencesPatch) {
    if let Some(appearance) = patch.appearance.as_deref().filter(|a| APPEARANCES.contains(a)) {
        settings.appearance = appearance.to_string();
        // Kept in step so the Java app, which reads only this, shows the same family.
        settings.theme = theme_of(appearance);
    }
    if let Some(language) = patch.language.as_deref().filter(|l| LANGUAGES.contains(l)) {
        settings.language = language.to_string();
    }
    if let Some(scale) = patch.ui_scale {
        settings.ui_scale = scale;
    }
    if let Some(placement) = patch.player_placement.as_deref().filter(|p| PLAYER_PLACEMENTS.contains(p)) {
        settings.player_placement = placement.to_string();
    }
    if let Some(value) = patch.include_id_in_title_name {
        settings.include_id_in_title_name = value;
    }
    if let Some(value) = patch.app_update_check_enabled {
        settings.app_update_check_enabled = value;
    }
    if let Some(value) = patch.auto_updates {
        settings.auto_updates = value;
    }
    if let Some(cookies) = &patch.cookies_file {
        settings.cookies_file = cookies.trim().to_string();
    }
}

/// Back to the factory values (Ajustes → Avanzado → Reset). The player's
/// memory is not a setting the user chose: it survives.
pub fn reset(settings: &UserSettings) -> UserSettings {
    let mut fresh = UserSettings::default();
    set_player_memory(&mut fresh, &player_memory(settings));
    fresh
}

fn theme_of(appearance: &str) -> Theme {
    if appearance.starts_with("light") { Theme::Light } else { Theme::Dark }
}

/// The player's part of the settings.
pub fn player_memory(settings: &UserSettings) -> PlayerMemory {
    let last = match (&settings.last_played_archive, &settings.last_played_track_id) {
        (Some(archive), Some(track_id)) => Some(LastPlayed {
            archive: archive.clone(),
            track_id: track_id.clone(),
            playlist: settings.last_played_playlist.clone().unwrap_or_default(),
            format: settings.last_played_format.clone().unwrap_or_default(),
            position: settings.last_played_position,
        }),
        _ => None,
    };
    PlayerMemory { volume: settings.player_volume, last }
}

/// Writes the player's part into `settings`; no last file (the bar's ✕)
/// clears all of it, so the next launch starts with no bar.
pub fn set_player_memory(settings: &mut UserSettings, memory: &PlayerMemory) {
    settings.player_volume = memory.volume.clamp(0.0, 1.0);
    let last = memory.last.as_ref();
    settings.last_played_archive = last.map(|l| l.archive.clone());
    settings.last_played_track_id = last.map(|l| l.track_id.clone());
    settings.last_played_playlist = last.map(|l| l.playlist.clone());
    settings.last_played_format = last.map(|l| l.format.clone());
    settings.last_played_position = last.map_or(0.0, |l| l.position.max(0.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Theme;
    use crate::storage::MemoryStorage;

    #[test]
    fn missing_or_broken_gives_the_defaults() {
        let storage = MemoryStorage::new();
        let file = Location::path("/data/config/settings.json");
        assert_eq!(load(&storage, &file), UserSettings::default());
        storage.create_dir_all(&Location::path("/data/config")).unwrap();
        storage.write(&file, b"{ not json").unwrap();
        assert_eq!(load(&storage, &file), UserSettings::default());
    }

    #[test]
    fn the_player_memory_goes_into_the_settings_and_comes_back() {
        let mut settings = UserSettings::default();
        let memory = PlayerMemory {
            volume: 0.4,
            last: Some(LastPlayed {
                archive: "/archives/pl_P_1/mp3/data.json".into(),
                track_id: "aaaaaaaaaaa".into(),
                playlist: "Lo-fi".into(),
                format: "MP3".into(),
                position: 83.5,
            }),
        };
        set_player_memory(&mut settings, &memory);
        assert_eq!(settings.last_played_position, 83.5);
        assert_eq!(player_memory(&settings), memory);
    }

    #[test]
    fn closing_the_player_forgets_the_last_file_but_keeps_the_volume() {
        let mut settings = UserSettings::default();
        set_player_memory(&mut settings, &PlayerMemory {
            volume: 0.3,
            last: Some(LastPlayed {
                archive: "a".into(), track_id: "b".into(), playlist: "c".into(), format: "MP3".into(), position: 10.0,
            }),
        });
        set_player_memory(&mut settings, &PlayerMemory { volume: 0.3, last: None });
        assert_eq!(settings.last_played_archive, None);
        assert_eq!(settings.last_played_track_id, None);
        assert_eq!(settings.last_played_position, 0.0);
        assert_eq!(player_memory(&settings), PlayerMemory { volume: 0.3, last: None });
    }

    #[test]
    fn a_volume_out_of_range_is_kept_within_it() {
        let mut settings = UserSettings::default();
        set_player_memory(&mut settings, &PlayerMemory { volume: 1.7, last: None });
        assert_eq!(settings.player_volume, 1.0);
    }

    #[test]
    fn a_look_keeps_the_java_theme_in_step() {
        let mut settings = UserSettings::default();
        apply(&mut settings, &PreferencesPatch { appearance: Some("light-blue".into()), ..Default::default() });
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(preferences(&settings).appearance, "light-blue");
        apply(&mut settings, &PreferencesPatch { appearance: Some("amoled".into()), ..Default::default() });
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(preferences(&settings).appearance, "amoled");
    }

    #[test]
    fn if_the_java_app_switched_the_theme_since_its_plain_variant_is_shown() {
        let settings = UserSettings { theme: Theme::Light, appearance: "dark-red".into(), ..Default::default() };
        assert_eq!(preferences(&settings).appearance, "light");
        // A file only the Java app wrote: no look at all.
        let java = UserSettings { theme: Theme::Light, ..Default::default() };
        assert_eq!(preferences(&java).appearance, "light");
        assert_eq!(preferences(&UserSettings::default()).appearance, "dark");
    }

    #[test]
    fn only_what_the_patch_carries_changes() {
        let mut settings = UserSettings { cookies_file: "C:/cookies.txt".into(), ..Default::default() };
        apply(&mut settings, &PreferencesPatch { include_id_in_title_name: Some(true), ..Default::default() });
        assert!(settings.include_id_in_title_name);
        assert_eq!(settings.cookies_file, "C:/cookies.txt");
        assert_eq!(settings.language, "es");
    }

    #[test]
    fn unknown_values_are_left_out() {
        let mut settings = UserSettings::default();
        apply(&mut settings, &PreferencesPatch {
            language: Some("fr".into()),
            appearance: Some("sepia".into()),
            player_placement: Some("top".into()),
            ..Default::default()
        });
        assert_eq!(settings.language, "es");
        assert_eq!(settings.appearance, "");
        assert_eq!(preferences(&settings).player_placement, "both");
    }

    #[test]
    fn clearing_the_cookies_leaves_them_empty() {
        let mut settings = UserSettings { cookies_file: "C:/cookies.txt".into(), ..Default::default() };
        apply(&mut settings, &PreferencesPatch { cookies_file: Some(String::new()), ..Default::default() });
        assert_eq!(settings.cookies_file, "");
    }

    #[test]
    fn a_reset_goes_back_to_the_factory_values_but_keeps_the_player() {
        let mut settings = UserSettings {
            appearance: "amoled".into(), language: "ca".into(), ui_scale: 150, cookies_file: "C:/c.txt".into(),
            include_id_in_title_name: true, auto_updates: false, ..Default::default()
        };
        set_player_memory(&mut settings, &PlayerMemory { volume: 0.2, last: None });
        let fresh = reset(&settings);
        assert_eq!(preferences(&fresh), preferences(&UserSettings::default()));
        assert_eq!(fresh.player_volume, 0.2);
    }

    #[test]
    fn survives_a_restart_and_creates_its_folder() {
        let storage = MemoryStorage::new();
        let file = Location::path("/data/config/settings.json");
        let settings = UserSettings { theme: Theme::Light, ..Default::default() };
        save(&storage, &file, &settings).unwrap();
        assert_eq!(load(&storage, &file).theme, Theme::Light);
    }
}
