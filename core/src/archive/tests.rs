use std::collections::HashSet;
use std::sync::Arc;

use super::*;
use crate::clock;
use crate::storage::MemoryStorage;

fn path(text: &str) -> Location {
    Location::path(text)
}

fn store() -> (ArchiveStore, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    storage.create_dir_all(&path("/music/P (mp3)")).unwrap();
    let store = ArchiveStore::new(storage.clone(), path("/archives"), clock::fixed("2026-11-02 10:00"));
    (store, storage)
}

fn items(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(id, title)| (id.to_string(), title.to_string())).collect()
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

const JSON: &str = "/archives/pl_P_000000/mp3/data.json";

#[test]
fn pending_means_never_seen_or_known_without_a_file_but_never_unavailable() {
    let (store, storage) = store();
    let json = path(JSON);
    let folder = path("/music/P (mp3)");
    store.register_playlist(&json, Some("https://x"), Some(&folder), &items(&[("aaa", "A"), ("bbb", "B"), ("ccc", "C")]), true).unwrap();
    storage.write(&path("/music/P (mp3)/001 - A.mp3"), b"x").unwrap();
    store.commit(&json, &folder, "mp3", &[("aaa".into(), path("/music/P (mp3)/001 - A.mp3"))]).unwrap();
    store.mark_unavailable(&json, "ccc", "Private video").unwrap();

    let pending = store.ids_pending_download(&json, &ids(&["aaa", "bbb", "ccc", "ddd"])).unwrap();
    assert_eq!(pending, ids(&["bbb", "ddd"]));
}

#[test]
fn regression_a_file_deleted_by_hand_is_pending_again() {
    let (store, storage) = store();
    let json = path(JSON);
    let folder = path("/music/P (mp3)");
    store.register_playlist(&json, None, Some(&folder), &items(&[("aaa", "A")]), true).unwrap();
    storage.write(&path("/music/P (mp3)/001 - A.mp3"), b"x").unwrap();
    store.commit(&json, &folder, "mp3", &[("aaa".into(), path("/music/P (mp3)/001 - A.mp3"))]).unwrap();
    storage.remove_file(&path("/music/P (mp3)/001 - A.mp3")).unwrap();

    assert_eq!(store.ids_pending_download(&json, &ids(&["aaa"])).unwrap(), ids(&["aaa"]));
    // It keeps its number for the second attempt.
    assert_eq!(store.saved_number(&json, "aaa").as_deref(), Some("001"));
}

#[test]
fn the_numbering_mode_is_fixed_when_the_playlist_is_born() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "A")]), true).unwrap();
    store.register_playlist(&json, None, None, &items(&[("bbb", "B")]), false).unwrap();
    assert!(store.is_numbered(&json));
    assert_eq!(store.known_ids(&json).unwrap(), ids(&["aaa", "bbb"]));
}

#[test]
fn registering_again_never_touches_known_ids() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "Old")]), false).unwrap();
    store.register_playlist(&json, None, None, &items(&[("aaa", "New")]), false).unwrap();
    assert_eq!(store.saved_title(&json, "aaa").as_deref(), Some("Old"));
}

#[test]
fn the_numbering_mode_is_inherited_from_another_format() {
    let (store, _) = store();
    let mp3 = path(JSON);
    let flac = path("/archives/pl_P_000000/flac/data.json");
    assert_eq!(store.resolve_number_mode(&flac, true), NumberMode { number_files: true, source: NumberModeSource::Requested });
    store.register_playlist(&mp3, None, None, &[], false).unwrap();
    assert_eq!(store.resolve_number_mode(&flac, true), NumberMode { number_files: false, source: NumberModeSource::OtherFormat });
    assert_eq!(store.resolve_number_mode(&mp3, true), NumberMode { number_files: false, source: NumberModeSource::SameFormat });
}

#[test]
fn a_title_equal_to_the_id_is_not_a_saved_title() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "aaa"), ("bbb", "")]), false).unwrap();
    assert_eq!(store.saved_title(&json, "aaa"), None);
    assert_eq!(store.saved_title(&json, "bbb"), None);
}

#[test]
fn the_next_free_number_follows_the_last_one() {
    let (store, storage) = store();
    let json = path(JSON);
    let folder = path("/music/P (mp3)");
    assert_eq!(store.next_free_number(&json).as_deref(), Some("001"));
    store.register_playlist(&json, None, Some(&folder), &[], true).unwrap();
    storage.write(&path("/music/P (mp3)/007 - A.mp3"), b"x").unwrap();
    store.commit(&json, &folder, "mp3", &[("aaa".into(), path("/music/P (mp3)/007 - A.mp3"))]).unwrap();
    assert_eq!(store.next_free_number(&json).as_deref(), Some("008"));
}

#[test]
fn removals_are_stamped_but_nothing_is_deleted() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "A"), ("bbb", "B")]), false).unwrap();
    let now: HashSet<String> = ["aaa".to_string()].into();
    store.sync_playlist_membership(&json, Some("https://new"), &now).unwrap();
    let data = store.load(&json).unwrap();
    assert_eq!(data.url.as_deref(), Some("https://new"));
    assert_eq!(data.entries["bbb"].removed_from_playlist_at.as_deref(), Some("2026-11-02 10:00"));
    assert_eq!(data.entries["aaa"].removed_from_playlist_at, None);
}

#[test]
fn failed_keeps_a_real_title_and_only_fills_a_blank_one() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "Real"), ("bbb", "")]), false).unwrap();
    store.mark_failed(&json, "aaa", Some("Other"), "403").unwrap();
    store.mark_failed(&json, "bbb", Some("Filled"), "403").unwrap();
    let data = store.load(&json).unwrap();
    assert_eq!(data.entries["aaa"].title.as_deref(), Some("Real"));
    assert_eq!(data.entries["bbb"].title.as_deref(), Some("Filled"));
    assert_eq!(data.entries["aaa"].last_failed_reason.as_deref(), Some("403"));
    assert!(!store.is_unavailable(&json, "aaa"));
}

#[test]
fn availability_is_only_checked_for_entries_with_nothing_known() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "Known"), ("bbb", "")]), false).unwrap();
    let checked = store.ids_needing_availability_check(&json, &ids(&["aaa", "bbb", "zzz"])).unwrap();
    assert_eq!(checked, ids(&["bbb"]));
}

#[test]
fn the_playlist_order_ignores_missing_indexes() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("aaa", "A")]), false).unwrap();
    let order = [("aaa".to_string(), 4), ("zzz".to_string(), 1)].into();
    store.refresh_playlist_order(&json, &order).unwrap();
    let data = store.load(&json).unwrap();
    assert_eq!(data.entries["aaa"].playlist_index, Some(4));
    assert!(!data.entries.contains_key("zzz"));
}

// === Ported from ArchiveStoreTest (Numbering, Commit, ArchiveTxt, Statuses) ===

const FOLDER: &str = "/music/P (mp3)";

fn file(name: &str) -> String {
    format!("{FOLDER}/{name}")
}

fn entry(number: Option<&str>, title: Option<&str>, file_path: Option<String>) -> ArchiveEntry {
    ArchiveEntry {
        number: number.map(String::from),
        title: title.map(String::from),
        file_path,
        added_at: Some("2026-01-01 10:00".into()),
        ..Default::default()
    }
}

fn new_data(numbered: bool) -> ArchiveData {
    ArchiveData {
        number_files: numbered,
        folder: Some(FOLDER.into()),
        url: Some("https://www.youtube.com/playlist?list=PL123".into()),
        ..Default::default()
    }
}

/// A real file in the music folder; returns its path.
fn real_file(storage: &MemoryStorage, name: &str) -> String {
    storage.write(&path(&file(name)), b"audio").unwrap();
    file(name)
}

fn seeded(data: ArchiveData) -> (ArchiveStore, Arc<MemoryStorage>, Location) {
    let (store, storage) = store();
    let json = path(JSON);
    store.save(&json, &data).unwrap();
    (store, storage, json)
}

fn status_of(store: &ArchiveStore, json: &Location, id: &str) -> EntryStatus {
    store.list_entries(json).unwrap().into_iter().find(|v| v.id == id).expect("the entry is missing").status
}

#[test]
fn the_offset_comes_from_what_is_actually_in_the_folder() {
    let (store, storage) = store();
    let json = path(JSON);
    let mut data = new_data(true);
    data.last_number = Some("010".into());
    data.entries.insert("id1".into(), entry(Some("001"), Some("Una"), Some(real_file(&storage, "001 - Una.mp3"))));
    data.entries.insert("id2".into(), entry(Some("002"), Some("Otra"), Some(real_file(&storage, "002 - Otra.mp3"))));
    store.save(&json, &data).unwrap();
    assert_eq!(store.prepare(&json, &path(FOLDER), "mp3").unwrap(), 2);
}

#[test]
fn regression_delete_every_file_and_the_numbering_starts_again_at_001() {
    // Real reported bug: "I delete the videos, download again and it starts
    // at 011". lastNumber was a historical watermark that could only grow.
    let mut data = new_data(true);
    data.last_number = Some("010".into());
    for i in 1..=10 {
        let number = format!("{i:03}");
        data.entries.insert(format!("id_{number}"), entry(Some(&number), Some("X"), Some(file(&format!("{number} - X {i}.mp3")))));
    }
    let (store, _, json) = seeded(data);
    assert_eq!(store.prepare(&json, &path(FOLDER), "mp3").unwrap(), 0, "the next download would start at 011 again");
    assert_eq!(store.load(&json).unwrap().last_number, None);
}

#[test]
fn a_numbered_orphan_in_the_folder_counts_too() {
    // A loose file the archive does not know about: the next number must not
    // overwrite it.
    let (store, storage, json) = seeded(new_data(true));
    real_file(&storage, "005 - Huérfana.mp3");
    assert_eq!(store.prepare(&json, &path(FOLDER), "mp3").unwrap(), 5);
}

#[test]
fn regression_on_redownload_the_item_gets_its_own_original_number_back() {
    // Real bug: one track was 002; on deleting and redownloading it, yt-dlp
    // gave it 003 (its order within the run) and it stayed that way.
    let (store, storage) = store();
    let json = path(JSON);
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("Primera"), Some(real_file(&storage, "001 - Primera.mp3"))));
    data.entries.insert("id2".into(), entry(Some("002"), Some("Crazy Frog"), Some(file("002 - Crazy Frog.mp3"))));
    store.save(&json, &data).unwrap();

    let just_downloaded = real_file(&storage, "003 - Crazy Frog.mp3");
    store.commit(&json, &path(FOLDER), "mp3", &[("id2".into(), path(&just_downloaded))]).unwrap();

    let crazy_frog = store.load(&json).unwrap().entries["id2"].clone();
    assert_eq!(crazy_frog.number.as_deref(), Some("002"));
    assert!(storage.exists(&path(&file("002 - Crazy Frog.mp3"))).unwrap(), "renamed to its usual number");
    assert!(!storage.exists(&path(&file("003 - Crazy Frog.mp3"))).unwrap(), "the batch's number is not left behind");
    // Joined with the platform's separator, like any path the storage builds.
    let renamed = storage.child(&path(FOLDER), "002 - Crazy Frog.mp3").unwrap().to_string();
    assert_eq!(crazy_frog.file_path, Some(renamed));
}

#[test]
fn in_an_unnumbered_playlist_no_number_is_assigned() {
    let (store, storage, json) = seeded(new_data(false));
    let downloaded = real_file(&storage, "Crazy Frog.mp3");
    store.commit(&json, &path(FOLDER), "mp3", &[("id1".into(), path(&downloaded))]).unwrap();
    let data = store.load(&json).unwrap();
    assert_eq!(data.entries["id1"].number, None);
    assert_eq!(data.last_number, None);
    assert_eq!(data.entries["id1"].title.as_deref(), Some("Crazy Frog"));
}

#[test]
fn commit_registers_a_new_item_with_its_title_and_path() {
    let (store, storage, json) = seeded(new_data(true));
    let downloaded = real_file(&storage, "001 - Canción Nueva.mp3");
    store.commit(&json, &path(FOLDER), "mp3", &[("id1".into(), path(&downloaded))]).unwrap();
    let e = store.load(&json).unwrap().entries["id1"].clone();
    assert_eq!(e.number.as_deref(), Some("001"));
    assert_eq!(e.title.as_deref(), Some("Canción Nueva"));
    assert_eq!(e.file_path, Some(downloaded));
    assert_eq!(e.deleted_at, None);
}

#[test]
fn regression_the_id_must_not_overwrite_a_real_title_already_stored() {
    // The safety net's second pass downloads without knowing the title and
    // leaves the file named "<id>.mp3". Before, that id was recorded as the
    // title and there was no way of getting the good one back.
    let mut data = new_data(false);
    data.entries.insert("id1".into(), entry(None, Some("Título Real"), Some(file("Título Real.mp3"))));
    let (store, storage, json) = seeded(data);
    let downloaded = real_file(&storage, "id1.mp3");
    store.commit(&json, &path(FOLDER), "mp3", &[("id1".into(), path(&downloaded))]).unwrap();
    assert_eq!(store.load(&json).unwrap().entries["id1"].title.as_deref(), Some("Título Real"));
}

#[test]
fn a_successful_download_clears_earlier_failure_marks() {
    let mut data = new_data(true);
    let mut e = entry(Some("001"), Some("Maroon 5"), None);
    e.last_failed_at = Some("2026-01-01 10:00".into());
    e.last_failed_reason = Some("Sign in to confirm your age".into());
    e.unavailable_at = Some("2026-01-01 10:00".into());
    e.deleted_at = Some("2026-01-01 10:00".into());
    data.entries.insert("id1".into(), e);
    let (store, storage, json) = seeded(data);
    let downloaded = real_file(&storage, "001 - Maroon 5.mp3");
    store.commit(&json, &path(FOLDER), "mp3", &[("id1".into(), path(&downloaded))]).unwrap();

    let after = store.load(&json).unwrap().entries["id1"].clone();
    assert_eq!(after.last_failed_at, None);
    assert_eq!(after.last_failed_reason, None);
    assert_eq!(after.unavailable_at, None);
    assert_eq!(after.deleted_at, None);
}

#[test]
fn a_file_no_longer_there_is_marked_as_deleted() {
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("Canción"), Some(file("001 - Canción.mp3"))));
    let (store, _, json) = seeded(data);
    store.prepare(&json, &path(FOLDER), "mp3").unwrap();
    assert!(store.load(&json).unwrap().entries["id1"].deleted_at.is_some());
}

#[test]
fn an_entry_that_never_had_a_file_is_not_marked_as_deleted() {
    // Created only by mark_unavailable(): there was never anything to delete.
    let mut data = new_data(true);
    let mut e = entry(None, None, None);
    e.unavailable_at = Some("2026-01-02 10:00".into());
    data.entries.insert("id1".into(), e);
    let (store, _, json) = seeded(data);
    store.prepare(&json, &path(FOLDER), "mp3").unwrap();
    assert_eq!(store.load(&json).unwrap().entries["id1"].deleted_at, None);
}

#[test]
fn prepare_tells_yt_dlp_to_skip_only_what_is_really_on_disk() {
    let (store, storage) = store();
    let json = path(JSON);
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("Descargada"), Some(real_file(&storage, "001 - Descargada.mp3"))));
    data.entries.insert("id2".into(), entry(Some("002"), Some("Borrada"), Some(file("002 - Borrada.mp3"))));
    let mut unavailable = entry(None, None, None);
    unavailable.unavailable_at = Some("2026-01-02 10:00".into());
    data.entries.insert("id3".into(), unavailable);
    store.save(&json, &data).unwrap();

    store.prepare(&json, &path(FOLDER), "mp3").unwrap();

    let txt = String::from_utf8(storage.read(&io::txt_file_for(&json).unwrap()).unwrap()).unwrap();
    assert!(txt.contains("youtube id1"), "the one that IS downloaded is missing");
    assert!(txt.contains("youtube id3"), "the one marked unavailable is missing");
    assert!(!txt.contains("youtube id2"), "the deleted one must NOT be skipped");
}

#[test]
fn a_downloaded_one_shows_as_ok_and_without_file_as_pending() {
    let (store, storage) = store();
    let json = path(JSON);
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("Canción"), Some(real_file(&storage, "001 - Canción.mp3"))));
    data.entries.insert("id2".into(), entry(Some("002"), Some("Otra"), Some(file("002 - Otra.mp3"))));
    store.save(&json, &data).unwrap();
    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Ok);
    assert!(matches!(status_of(&store, &json, "id2"), EntryStatus::Pending { .. }));
}

#[test]
fn regression_what_is_no_longer_in_the_playlist_does_not_count_as_pending() {
    // Real bug: "I deleted them from the playlist on YouTube and they still
    // show up as pending", and they inflated the Manager's counter.
    let mut data = new_data(true);
    let mut e = entry(Some("001"), Some("Retirada"), Some(file("001 - Retirada.mp3")));
    e.removed_from_playlist_at = Some("2026-01-02 10:00".into());
    data.entries.insert("id1".into(), e);
    let (store, _, json) = seeded(data);
    assert!(matches!(status_of(&store, &json, "id1"), EntryStatus::Removed { .. }));
}

#[test]
fn the_run_failure_is_shown_with_its_reason() {
    let mut data = new_data(true);
    let mut e = entry(Some("001"), Some("Maroon 5"), Some(file("001 - Maroon 5.mp3")));
    e.last_failed_at = Some("2026-01-02 10:00".into());
    e.last_failed_reason = Some("Sign in to confirm your age".into());
    data.entries.insert("id1".into(), e);
    let (store, _, json) = seeded(data);
    assert_eq!(
        status_of(&store, &json, "id1"),
        EntryStatus::Failed { reason: Some("Sign in to confirm your age".into()), at: "2026-01-02 10:00".into() }
    );
}

#[test]
fn unavailable_beats_every_other_status() {
    let mut data = new_data(true);
    let mut e = entry(Some("001"), Some("Privada"), Some(file("001 - Privada.mp3")));
    e.unavailable_at = Some("2026-01-02 10:00".into());
    e.unavailable_reason = Some("Private video".into());
    e.last_failed_at = Some("2026-01-02 10:00".into());
    e.removed_from_playlist_at = Some("2026-01-02 10:00".into());
    data.entries.insert("id1".into(), e);
    let (store, _, json) = seeded(data);
    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Unavailable { reason: Some("Private video".into()) });
}

#[test]
fn registering_new_items_leaves_them_pending_and_keeps_the_known_ones() {
    // Regression: they used to appear all at once, already green, when the
    // download ended (they did not exist in the archive until fetched).
    let (store, storage) = store();
    let json = path(JSON);
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("La de siempre"), Some(real_file(&storage, "001 - La de siempre.mp3"))));
    store.save(&json, &data).unwrap();

    let added = store.register_new_playlist_items(&json, &items(&[("id1", "Título distinto"), ("id2", "Nueva")])).unwrap();

    assert_eq!(added, 1, "only id2 is new");
    assert_eq!(store.saved_title(&json, "id1").as_deref(), Some("La de siempre"));
    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Ok);
    assert_eq!(status_of(&store, &json, "id2"), EntryStatus::Pending { deleted_at: None });
    assert_eq!(store.register_new_playlist_items(&json, &[]).unwrap(), 0);
}

#[test]
fn what_is_already_on_disk_is_linked_instead_of_counted_as_pending() {
    // It really happens with two copies of the app over the same music folder
    // (the portable and the one started from the IDE, say): one downloads it
    // and the other knows nothing about it. Without this, the second would
    // take it as pending and download it over the top again.
    let (store, storage) = store();
    let json = path(JSON);
    let mut known = new_data(false);
    known.entries.insert("id1".into(), entry(None, Some("Ya bajada"), Some(real_file(&storage, "Ya bajada.mp3"))));
    store.save(&json, &known).unwrap();
    crate::transfer::write_manifest_quietly(storage.as_ref(), &json, "2026-11-02 10:00");

    // Another archive over that SAME music folder, which knows nothing.
    store.save(&json, &new_data(false)).unwrap();
    store.register_new_playlist_items(&json, &items(&[("id1", "Ya bajada")])).unwrap();

    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Ok, "the manifest knew where it was");
}

#[test]
fn the_startup_reconciliation_links_what_another_copy_downloaded_into_the_same_folder() {
    // The portable app had imported the playlist; the one from the IDE then
    // downloaded more into the same folder and rewrote the manifest. Those files
    // are `NNN - Title`, with no id, and the portable's archive has no number
    // for them: only the manifest says whose they are.
    let (store, storage) = store();
    let json = path(JSON);

    // The other copy's archive: both downloaded, and it leaves its manifest.
    let mut other = new_data(true);
    other.entries.insert("id1".into(), entry(Some("001"), Some("Uno"), Some(real_file(&storage, "001 - Uno.mp3"))));
    other.entries.insert("id2".into(), entry(Some("002"), Some("Dos"), Some(real_file(&storage, "002 - Dos.mp3"))));
    store.save(&json, &other).unwrap();
    crate::transfer::write_manifest_quietly(storage.as_ref(), &json, "2026-11-02 10:00");

    // This copy only knew the first one, and the second as pending without number.
    let mut mine = new_data(true);
    mine.entries.insert("id1".into(), entry(Some("001"), Some("Uno"), Some(file("001 - Uno.mp3"))));
    mine.entries.insert("id2".into(), entry(None, Some("Dos"), None));
    store.save(&json, &mine).unwrap();

    store.reconcile_all();

    assert_eq!(status_of(&store, &json, "id2"), EntryStatus::Ok);
    assert_eq!(store.saved_number(&json, "id2").as_deref(), Some("002"));
}

#[test]
fn regression_a_number_alone_never_links_a_file_that_is_another_video() {
    // Reported: the archive imported from Java numbered the videos one way, and
    // the other copy had downloaded them under different numbers into the same
    // folder. Linking by number alone put "002 - Dos" on the entry that Java
    // numbered 002 (a different video) and even took its title: the whole
    // list came out with other titles in every position.
    let (store, storage) = store();
    let json = path(JSON);

    let mut other = new_data(true);
    other.entries.insert("id1".into(), entry(Some("001"), Some("Uno"), Some(real_file(&storage, "001 - Uno.mp3"))));
    other.entries.insert("id2".into(), entry(Some("002"), Some("Dos"), Some(real_file(&storage, "002 - Dos.mp3"))));
    store.save(&json, &other).unwrap();
    crate::transfer::write_manifest_quietly(storage.as_ref(), &json, "2026-11-02 10:00");

    // The imported archive: same videos, swapped numbers, files that do not exist.
    let mut imported = new_data(true);
    imported.entries.insert("id1".into(), entry(Some("002"), Some("Uno"), Some(file("002 - Uno.mp3"))));
    imported.entries.insert("id2".into(), entry(Some("001"), Some("Dos"), Some(file("001 - Dos.mp3"))));
    store.save(&json, &imported).unwrap();

    store.reconcile_all();

    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Ok);
    assert_eq!(status_of(&store, &json, "id2"), EntryStatus::Ok);
    assert_eq!(store.saved_number(&json, "id1").as_deref(), Some("001"), "the number is the one in the file's name");
    assert_eq!(store.saved_number(&json, "id2").as_deref(), Some("002"));
    let titles: Vec<_> = store.list_entries(&json).unwrap().into_iter().map(|v| (v.id, v.title)).collect();
    assert!(titles.contains(&("id1".to_string(), Some("Uno".to_string()))), "{titles:?}");
    assert!(titles.contains(&("id2".to_string(), Some("Dos".to_string()))), "{titles:?}");
}

#[test]
fn a_number_still_links_a_renamed_file_when_the_title_matches() {
    // The old behaviour that is worth keeping: the file is there under its
    // number, only the path stored in the archive is stale (the folder moved).
    let (store, storage) = store();
    let json = path(JSON);
    real_file(&storage, "001 - Uno.mp3");
    let mut data = new_data(true);
    data.entries.insert("id1".into(), entry(Some("001"), Some("Uno"), Some("/otra/carpeta/001 - Uno.mp3".into())));
    store.save(&json, &data).unwrap();

    store.reconcile_all();

    assert_eq!(status_of(&store, &json, "id1"), EntryStatus::Ok);
}

// === DEVICE PLAYLISTS ===

fn device_target() -> crate::device::DeviceTarget {
    use crate::device::fake::{ID, NAME};
    crate::device::DeviceTarget { device_id: ID.into(), device_name: NAME.into(), path: "Music/P (mp3)".into() }
}

#[test]
fn a_device_playlist_is_registered_with_its_place_on_the_device_and_its_manifest() {
    let (store, _) = store();
    let json = path(JSON);
    let phone = crate::device::fake::FakeDevice::new();
    phone.mkdirs(crate::device::fake::ID, "Music/P (mp3)");
    let scratch = tempfile::tempdir().unwrap();
    store
        .register_device_playlist(&phone, scratch.path(), &json, Some("https://x"), Some("P"), &device_target(), &items(&[("aaa", "A")]), true)
        .unwrap();
    let data = store.load(&json).unwrap();
    assert!(data.is_device_backed());
    assert_eq!(data.device_path.as_deref(), Some("Music/P (mp3)"));
    assert_eq!(data.title.as_deref(), Some("P"));
    assert!(data.number_files);
    assert!(data.entries.contains_key("aaa"));
    assert!(phone.file("Music/P (mp3)/soundify-playlist.json").is_some());
}

#[test]
fn registering_a_device_file_clears_old_marks_and_bumps_the_number() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_device_individual(&json, &device_target()).unwrap();
    store.mark_unavailable(&json, "aaa", "Private video").unwrap();
    store.register_device_single(&json, "aaa", "007 - A.mp3", Some("A"), Some("007")).unwrap();
    let data = store.load(&json).unwrap();
    let entry = &data.entries["aaa"];
    assert_eq!(entry.device_file_name.as_deref(), Some("007 - A.mp3"));
    assert_eq!(entry.unavailable_at, None);
    assert_eq!(entry.number.as_deref(), Some("007"));
    assert_eq!(data.last_number.as_deref(), Some("007"));
}

#[test]
fn a_title_that_is_only_the_id_never_replaces_a_real_one() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_device_individual(&json, &device_target()).unwrap();
    store.register_device_single(&json, "aaa", "A.mp3", Some("A"), None).unwrap();
    store.register_device_single(&json, "aaa", "A.mp3", Some("aaa"), None).unwrap();
    assert_eq!(store.load(&json).unwrap().entries["aaa"].title.as_deref(), Some("A"));
}

#[test]
fn deleting_everything_of_a_device_playlist_goes_to_the_phone() {
    let (store, _) = store();
    let json = path(JSON);
    let phone = crate::device::fake::FakeDevice::new();
    phone.put_file("Music/P (mp3)/A.mp3", b"a");
    store.register_device_individual(&json, &device_target()).unwrap();
    store.register_device_single(&json, "aaa", "A.mp3", Some("A"), None).unwrap();
    let result = store.delete_all_downloaded_files(&phone, &json);
    assert_eq!(result, DeleteResult { deleted: 1, unregistered: 0, left_over: 0 });
    assert!(!phone.has_folder("Music/P (mp3)"));
}

#[test]
fn concurrent_changes_to_one_archive_lose_none() {
    let (store, _) = store();
    let json = path(JSON);
    store.register_playlist(&json, None, None, &items(&[("seed", "Seed")]), true).unwrap();

    // Eight threads each register their own items on the same archive: without the store's
    // write lock the last save would erase the others'.
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let store = store.clone();
            let json = json.clone();
            std::thread::spawn(move || {
                for k in 0..10 {
                    let pair = items(&[(&format!("id{n}_{k}"), &format!("Title {n} {k}"))]);
                    store.register_new_playlist_items(&json, &pair).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(store.known_ids(&json).unwrap().len(), 81);
}

#[test]
fn concurrent_changes_to_different_archives_stay_apart() {
    let (store, _) = store();
    let jsons: Vec<Location> = (0..4).map(|n| path(&format!("/archives/pl_P_00000{n}/mp3/data.json"))).collect();
    for (n, json) in jsons.iter().enumerate() {
        store.register_playlist(json, None, None, &items(&[(&format!("seed{n}"), "Seed")]), true).unwrap();
    }
    let threads: Vec<_> = jsons
        .iter()
        .cloned()
        .enumerate()
        .map(|(n, json)| {
            let store = store.clone();
            std::thread::spawn(move || {
                for k in 0..10 {
                    let pair = items(&[(&format!("a{n}_{k}"), "T")]);
                    store.register_new_playlist_items(&json, &pair).unwrap();
                    store.touch_last_activity(&json).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    for (n, json) in jsons.iter().enumerate() {
        let known = store.known_ids(json).unwrap();
        assert_eq!(known.len(), 11, "archive {n}: {known:?}");
        assert!(known.iter().all(|id| id.starts_with(&format!("a{n}_")) || id == &format!("seed{n}")), "nothing crossed over: {known:?}");
    }
}
