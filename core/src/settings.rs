//! `data/config/settings.json`: the user's settings.
//!
//! Tolerant on purpose: a missing or broken file gives the defaults instead of
//! stopping the app from starting, and fields from older versions are ignored.

use std::io::Result;

use log::warn;

use crate::Location;
use crate::model::UserSettings;
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
    fn survives_a_restart_and_creates_its_folder() {
        let storage = MemoryStorage::new();
        let file = Location::path("/data/config/settings.json");
        let settings = UserSettings { theme: Theme::Light, ..Default::default() };
        save(&storage, &file, &settings).unwrap();
        assert_eq!(load(&storage, &file).theme, Theme::Light);
    }
}
