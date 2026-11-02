//! `data/history/tasks.json`: the finished background tasks, newest first,
//! kept between launches up to [`MAX_ENTRIES`].

use std::io::Result;

use log::warn;

use crate::Location;
use crate::model::TaskRecord;
use crate::storage::Storage;

pub const MAX_ENTRIES: usize = 20;

pub const OK: &str = "OK";
pub const WARNING: &str = "WARNING";
pub const STOPPED: &str = "STOPPED";
pub const FAILED: &str = "FAILED";

/// Newest first; the oldest fall off past the cap.
pub fn add(history: &mut Vec<TaskRecord>, record: TaskRecord) {
    history.insert(0, record);
    history.truncate(MAX_ENTRIES);
}

/// The worse of two outcomes: a task that warned and then failed ended
/// failed. `None` is "nothing yet", beaten by anything.
pub fn worse<'a>(a: Option<&'a str>, b: Option<&'a str>) -> Option<&'a str> {
    fn rank(status: Option<&str>) -> i32 {
        match status {
            Some(FAILED) => 3,
            Some(STOPPED) => 2,
            Some(WARNING) => 1,
            Some(_) => 0,
            None => -1,
        }
    }
    if rank(b) > rank(a) { b } else { a }
}

/// A missing or broken file is an empty history: never a reason not to start.
pub fn load(storage: &dyn Storage, file: &Location) -> Vec<TaskRecord> {
    if !storage.exists(file).unwrap_or(false) {
        return Vec::new();
    }
    let mut history: Vec<TaskRecord> = match storage.read(file).map(|bytes| serde_json::from_slice(&bytes)) {
        Ok(Ok(history)) => history,
        Ok(Err(e)) => {
            warn!("Broken task history, starting empty: {e}");
            Vec::new()
        }
        Err(e) => {
            warn!("Could not read task history: {e}");
            Vec::new()
        }
    };
    history.truncate(MAX_ENTRIES);
    history
}

pub fn save(storage: &dyn Storage, file: &Location, history: &[TaskRecord]) -> Result<()> {
    if let Some(parent) = crate::archive::io::parent_of(file) {
        storage.create_dir_all(&parent)?;
    }
    let bytes = serde_json::to_vec_pretty(history).map_err(std::io::Error::other)?;
    storage.write(file, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn record(title: &str) -> TaskRecord {
        TaskRecord { status: Some(OK.into()), title: Some(title.into()), ..Default::default() }
    }

    #[test]
    fn newest_first_and_capped() {
        let mut history = Vec::new();
        for i in 0..25 {
            add(&mut history, record(&format!("t{i}")));
        }
        assert_eq!(history.len(), MAX_ENTRIES);
        assert_eq!(history[0].title.as_deref(), Some("t24"));
        assert_eq!(history[MAX_ENTRIES - 1].title.as_deref(), Some("t5"));
    }

    #[test]
    fn the_worst_outcome_wins() {
        assert_eq!(worse(Some(WARNING), Some(FAILED)), Some(FAILED));
        assert_eq!(worse(Some(FAILED), Some(WARNING)), Some(FAILED));
        assert_eq!(worse(Some(STOPPED), Some(OK)), Some(STOPPED));
        assert_eq!(worse(None, Some(OK)), Some(OK));
        assert_eq!(worse(Some(OK), None), Some(OK));
    }

    #[test]
    fn survives_a_restart() {
        let storage = MemoryStorage::new();
        let file = Location::path("/data/history/tasks.json");
        let mut history = Vec::new();
        add(&mut history, record("a"));
        add(&mut history, record("b"));
        save(&storage, &file, &history).unwrap();
        assert_eq!(load(&storage, &file), history);
    }

    #[test]
    fn missing_or_broken_is_empty() {
        let storage = MemoryStorage::new();
        let file = Location::path("/data/history/tasks.json");
        assert!(load(&storage, &file).is_empty());
        storage.create_dir_all(&Location::path("/data/history")).unwrap();
        storage.write(&file, b"[{").unwrap();
        assert!(load(&storage, &file).is_empty());
    }
}
