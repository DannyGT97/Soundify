//! The Tauri app reads the very files the Java app wrote (MIGRATION-PLAN.md
//! 1.3): whoever moves from one to the other loses nothing. The fixtures in
//! `fixtures/java/` have the exact shape Java's Jackson writes (pretty printed,
//! every null spelled out, fields of older and newer versions), anonymised.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use soundify_core::Location;
use soundify_core::archive::{ArchiveStore, EntryStatus};
use soundify_core::clock;
use soundify_core::model::Theme;
use soundify_core::storage::LocalStorage;
use soundify_core::{settings, task_history};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/java");

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

/// The fixture's playlist laid out in a temporary folder, with only the first
/// file really on disk.
fn java_playlist(root: &Path) -> (ArchiveStore, Location) {
    let music = root.join("music");
    let folder = music.join("Example (mp3)");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("001 - Primer título.mp3"), b"x").unwrap();
    let music_dir = music.to_string_lossy().replace('\\', "/");
    let json_text = fixture("playlist_data.json").replace("MUSIC_DIR", &music_dir);

    let archives = root.join("archives");
    let json = archives.join("pl_Example_000000").join("mp3").join("data.json");
    fs::create_dir_all(json.parent().unwrap()).unwrap();
    fs::write(&json, json_text).unwrap();
    let store = ArchiveStore::new(Arc::new(LocalStorage), Location::path(archives), clock::fixed("2026-11-02 10:00"));
    (store, Location::path(json))
}

#[test]
fn reads_a_java_playlist_archive_with_its_states() {
    let dir = tempfile::tempdir().unwrap();
    let (store, json) = java_playlist(dir.path());

    let data = store.load(&json).unwrap();
    assert!(data.number_files);
    assert_eq!(data.last_number.as_deref(), Some("002"));
    // The Java order of the entries is kept.
    assert_eq!(data.entries.keys().collect::<Vec<_>>(), ["zzzzzzzzzz1", "aaaaaaaaaa2", "mmmmmmmmmm3"]);

    let states: Vec<(String, EntryStatus)> = store.list_entries(&json).unwrap().into_iter().map(|v| (v.id, v.status)).collect();
    assert_eq!(states[0], ("mmmmmmmmmm3".into(), EntryStatus::Unavailable { reason: Some("Video unavailable".into()) }));
    assert_eq!(states[1], ("zzzzzzzzzz1".into(), EntryStatus::Ok));
    assert_eq!(
        states[2],
        ("aaaaaaaaaa2".into(), EntryStatus::Failed { reason: Some("HTTP Error 403".into()), at: "2026-09-05 10:00".into() })
    );

    let current = ["zzzzzzzzzz1", "aaaaaaaaaa2", "mmmmmmmmmm3"].map(String::from);
    assert_eq!(store.ids_pending_download(&json, &current).unwrap(), ["aaaaaaaaaa2"]);

    let playlists = store.list_local_playlists();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].title, "Example");
    let mp3 = &playlists[0].formats[0];
    // A failed download counts as failed, not pending (same as Java's Gestor).
    let counts = (mp3.downloaded_count, mp3.pending_count, mp3.failed_count, mp3.unavailable_count);
    assert_eq!(counts, (1, 0, 1, 1));
}

#[test]
fn what_rust_writes_back_java_can_still_read() {
    let dir = tempfile::tempdir().unwrap();
    let (store, json) = java_playlist(dir.path());
    store.sync_playlist_membership(&json, None, &HashSet::from(["zzzzzzzzzz1".to_string()])).unwrap();

    let written: serde_json::Value = serde_json::from_slice(&fs::read(json.as_path().unwrap()).unwrap()).unwrap();
    // Java's names, not Rust's.
    assert_eq!(written["numberFiles"], true);
    assert_eq!(written["lastSyncedAt"], "2026-11-02 10:00");
    assert_eq!(written["entries"]["mmmmmmmmmm3"]["removedFromPlaylistAt"], "2026-11-02 10:00");
    // Already gone from disk: removals do not apply to it (as in Java).
    assert_eq!(written["entries"]["aaaaaaaaaa2"]["removedFromPlaylistAt"], serde_json::Value::Null);
    assert_eq!(written["entries"]["zzzzzzzzzz1"]["playlistIndex"], 2);
}

#[test]
fn reads_java_settings_including_nulls_and_withdrawn_fields() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    fs::write(&file, fixture("settings.json")).unwrap();
    let settings = settings::load(&LocalStorage, &Location::path(&file));
    assert_eq!(settings.theme, Theme::Light);
    assert_eq!(settings.ui_scale, 125);
    assert_eq!(settings.language, "ca");
    assert_eq!(settings.working_player_client.as_deref(), Some("web_safari"));
    assert!(!settings.task_history_visible);
    assert_eq!(settings.player_volume, 0.25);
    assert_eq!(settings.last_played_position, 133.5);
}

#[test]
fn reads_the_java_task_history() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.json");
    fs::write(&file, fixture("tasks.json")).unwrap();
    let history = task_history::load(&LocalStorage, &Location::path(&file));
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].status.as_deref(), Some(task_history::WARNING));
    assert_eq!(history[0].origin.as_deref(), Some("MANAGER"));
}
