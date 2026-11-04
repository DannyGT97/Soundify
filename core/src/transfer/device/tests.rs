//! Ported from the Java app's `DevicePlaylistTransferTest`, plus the flows it
//! could only test by hand (the Java app had no fake device): the phone is a
//! [`FakeDevice`], the computer a real temporary folder.

use std::fs;
use std::path::PathBuf;

use super::*;
use crate::device::fake::{FakeDevice, ID, NAME};
use crate::storage::LocalStorage;

const NOW: &str = "2026-11-03 10:00";

struct Fixture {
    _dir: tempfile::TempDir,
    temp: PathBuf,
    music: PathBuf,
    archives: Location,
    scratch: PathBuf,
    storage: LocalStorage,
    phone: FakeDevice,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().to_path_buf();
        let music = temp.join("Cocina (mp3)");
        let archives = temp.join("archives");
        let scratch = temp.join("scratch");
        for folder in [&music, &archives, &scratch] {
            fs::create_dir_all(folder).unwrap();
        }
        Self {
            _dir: dir,
            archives: Location::Path(archives),
            scratch,
            temp,
            music,
            storage: LocalStorage,
            phone: FakeDevice::new(),
        }
    }

    fn loc(&self, path: impl Into<PathBuf>) -> Location {
        Location::Path(path.into())
    }

    /// A LOCAL playlist "Cocina" (mp3) with these (id, file name, content);
    /// an empty content means the archive remembers it but the file is gone.
    fn local_playlist(&self, files: &[(&str, &str, &str)]) -> Location {
        let dir = self.temp.join("archives").join("pl_Cocina_abc").join("mp3");
        fs::create_dir_all(&dir).unwrap();
        let mut data = ArchiveData {
            url: Some("https://www.youtube.com/playlist?list=PL1".into()),
            folder: Some(self.music.to_string_lossy().into_owned()),
            ..Default::default()
        };
        for (id, name, content) in files {
            let path = self.music.join(name);
            if !content.is_empty() {
                fs::write(&path, content).unwrap();
            }
            data.entries.insert(
                id.to_string(),
                ArchiveEntry { title: Some(name.to_string()), file_path: Some(path.to_string_lossy().into_owned()), ..Default::default() },
            );
        }
        let json = self.loc(dir.join(DATA_JSON));
        io::save(&self.storage, &json, &data).unwrap();
        json
    }

    /// A DEVICE playlist "Cocina" (mp3) living at "Music/Cocina (mp3)" on the phone.
    fn device_playlist(&self, files: &[(&str, &str)]) -> Location {
        let dir = self.temp.join("archives").join("pl_Cocina_dev").join("mp3");
        fs::create_dir_all(&dir).unwrap();
        let mut data = ArchiveData {
            url: Some("https://www.youtube.com/playlist?list=PL1".into()),
            title: Some("Cocina".into()),
            device_id: Some(ID.into()),
            device_name: Some(NAME.into()),
            device_path: Some("Music/Cocina (mp3)".into()),
            ..Default::default()
        };
        for (id, name) in files {
            data.entries.insert(
                id.to_string(),
                ArchiveEntry { title: Some(name.to_string()), device_file_name: Some(name.to_string()), ..Default::default() },
            );
        }
        let json = self.loc(dir.join(DATA_JSON));
        io::save(&self.storage, &json, &data).unwrap();
        json
    }

    fn target(&self, path: &str) -> DeviceTarget {
        DeviceTarget { device_id: ID.into(), device_name: NAME.into(), path: path.into() }
    }

    fn to_device(&self, json: &Location, cancel_after: Option<usize>) -> Result<ExportResult> {
        let calls = std::cell::Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            cancel_after.is_some_and(|n| calls.get() > n)
        };
        export_to_device(&self.storage, &self.phone, json, &self.target("Music"), &mut |_, _, _| {}, &cancelled, &self.scratch, NOW)
    }

    /// Registers an archive for "Cocina (mp3)" under `base` on the phone, as a
    /// registered device playlist would have it.
    fn register_at(&self, base: &str) {
        let location = self.target(&join_path(base, "Cocina (mp3)"));
        let json = archive_json(&self.storage, &self.archives, &location, "Cocina", "mp3").unwrap();
        io::save(&self.storage, &json, &ArchiveData::default()).unwrap();
    }
}

fn manifest_on_phone(phone: &FakeDevice, folder: &str) -> PlaylistManifest {
    let bytes = phone.file(&join_path(folder, MANIFEST_NAME)).expect("manifest on the device");
    serde_json::from_slice(&bytes).unwrap()
}

fn item(name: &str) -> ManifestItem {
    ManifestItem { id: name.into(), file_name: Some(name.into()), ..Default::default() }
}

// === IS ALREADY REGISTERED ===

#[test]
fn is_already_registered_false_without_an_archive() {
    let f = Fixture::new();
    assert!(!is_already_registered(&f.storage, &f.archives, &f.target("Music/Cocina (mp3)")));
}

#[test]
fn is_already_registered_false_when_the_folder_name_cannot_be_parsed() {
    let f = Fixture::new();
    assert!(!is_already_registered(&f.storage, &f.archives, &f.target("Music/Cocina")));
}

#[test]
fn is_already_registered_true_once_the_archive_exists() {
    let f = Fixture::new();
    f.register_at("Music");
    assert!(is_already_registered(&f.storage, &f.archives, &f.target("Music/Cocina (mp3)")));
    // Same name under another base folder of the phone: another playlist.
    assert!(!is_already_registered(&f.storage, &f.archives, &f.target("Podcasts/Cocina (mp3)")));
}

#[test]
fn not_yet_registered_filters_out_the_registered_ones() {
    let f = Fixture::new();
    f.register_at("Music");
    let all = [f.target("Music/Cocina (mp3)"), f.target("Podcasts/Cocina (mp3)")];
    assert_eq!(not_yet_registered(&f.storage, &f.archives, &all), vec![f.target("Podcasts/Cocina (mp3)")]);
}

// === ALREADY ON DEVICE / ALREADY LOCAL / BYTES TO PULL ===

#[test]
fn already_on_device_with_the_same_name_and_size() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "1234").unwrap();
    let source = f.loc(f.music.join("a.mp3"));
    let sizes = HashMap::from([("a.mp3".to_string(), 4)]);
    assert!(already_on_device(&f.storage, &source, "a.mp3", &sizes));
}

#[test]
fn not_on_device_when_the_size_differs() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "1234").unwrap();
    let sizes = HashMap::from([("a.mp3".to_string(), 3)]);
    assert!(!already_on_device(&f.storage, &f.loc(f.music.join("a.mp3")), "a.mp3", &sizes));
}

#[test]
fn not_on_device_when_the_name_is_not_listed() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "1234").unwrap();
    assert!(!already_on_device(&f.storage, &f.loc(f.music.join("a.mp3")), "a.mp3", &HashMap::new()));
}

#[test]
fn not_on_device_when_the_local_file_is_gone() {
    let f = Fixture::new();
    let sizes = HashMap::from([("a.mp3".to_string(), 4)]);
    assert!(!already_on_device(&f.storage, &f.loc(f.music.join("a.mp3")), "a.mp3", &sizes));
}

#[test]
fn already_local_with_the_same_size() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "1234").unwrap();
    assert!(already_local(&f.storage, &f.loc(f.music.join("a.mp3")), 4));
}

#[test]
fn not_local_when_the_size_differs() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "12").unwrap();
    assert!(!already_local(&f.storage, &f.loc(f.music.join("a.mp3")), 4));
}

#[test]
fn not_local_when_it_does_not_exist() {
    let f = Fixture::new();
    assert!(!already_local(&f.storage, &f.loc(f.music.join("a.mp3")), 4));
}

#[test]
fn bytes_to_pull_counts_only_what_is_pending() {
    let f = Fixture::new();
    fs::write(f.music.join("a.mp3"), "1234").unwrap();
    let (a, b) = (item("a.mp3"), item("b.mp3"));
    let sizes = HashMap::from([("a.mp3".to_string(), 4), ("b.mp3".to_string(), 10)]);
    assert_eq!(bytes_to_pull(&f.storage, &[&a, &b], &sizes, &f.loc(&f.music)).unwrap(), 10);
}

#[test]
fn bytes_to_pull_leaves_out_what_is_not_on_the_device() {
    let f = Fixture::new();
    let (a, b) = (item("a.mp3"), item("b.mp3"));
    let sizes = HashMap::from([("a.mp3".to_string(), 4)]);
    assert_eq!(bytes_to_pull(&f.storage, &[&a, &b], &sizes, &f.loc(&f.music)).unwrap(), 4);
}

#[test]
fn bytes_to_pull_of_nothing_is_zero() {
    let f = Fixture::new();
    assert_eq!(bytes_to_pull(&f.storage, &[], &HashMap::new(), &f.loc(&f.music)).unwrap(), 0);
}

// === COPYING TO A DEVICE ===

#[test]
fn export_to_device_pushes_the_files_and_the_manifest_into_title_format() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa"), ("id2", "b.mp3", "bb")]);
    let result = f.to_device(&json, None).unwrap();
    assert_eq!(result, ExportResult { copied: 2, skipped: 0, missing: 0, cancelled: false, undone: 0 });
    assert_eq!(f.phone.file("Music/Cocina (mp3)/a.mp3").unwrap(), b"aaaa");
    let manifest = manifest_on_phone(&f.phone, "Music/Cocina (mp3)");
    assert_eq!(manifest.title, "Cocina");
    assert_eq!(manifest.items.len(), 2);
}

#[test]
fn export_to_device_skips_what_is_already_there_and_counts_the_missing() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa"), ("id2", "b.mp3", ""), ("id3", "c.mp3", "cc")]);
    f.phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaaa");
    let result = f.to_device(&json, None).unwrap();
    // b.mp3 has no file locally, so the manifest never lists it as copyable.
    assert_eq!((result.copied, result.skipped, result.missing), (1, 1, 0));
    assert_eq!(f.phone.pushed().iter().filter(|p| p.ends_with(".mp3")).count(), 1);
}

#[test]
fn export_to_device_fails_when_the_phone_does_not_answer() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa")]);
    f.phone.set_reachable(false);
    assert_eq!(f.to_device(&json, None), Err(TransferError::DeviceUnreachable));
}

#[test]
fn a_failed_push_aborts_the_copy_naming_the_file() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa")]);
    f.phone.fail_on("a.mp3");
    assert_eq!(f.to_device(&json, None), Err(TransferError::CopyToDeviceFailed { file_name: "a.mp3".into() }));
}

#[test]
fn regression_cancelling_a_copy_to_the_device_leaves_no_empty_folder_behind() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa"), ("id2", "b.mp3", "bb")]);
    let result = f.to_device(&json, Some(1)).unwrap();
    assert_eq!(result, ExportResult { copied: 0, skipped: 0, missing: 0, cancelled: true, undone: 1 });
    assert!(!f.phone.has_folder("Music/Cocina (mp3)"));
}

#[test]
fn cancelling_keeps_the_folder_when_it_already_had_files() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa"), ("id2", "b.mp3", "bb")]);
    f.phone.put_file("Music/Cocina (mp3)/old.mp3", b"x");
    let result = f.to_device(&json, Some(1)).unwrap();
    assert!(result.cancelled);
    assert_eq!(f.phone.names_in("Music/Cocina (mp3)"), vec!["old.mp3"]);
}

// === COPYING FROM A DEVICE ===

#[test]
fn export_from_device_pulls_the_files_and_leaves_the_manifest_locally() {
    let f = Fixture::new();
    let json = f.device_playlist(&[("id1", "a.mp3"), ("id2", "gone.mp3")]);
    f.phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaaa");
    let destination = f.loc(f.temp.join("copia"));
    fs::create_dir_all(f.temp.join("copia")).unwrap();
    let result = export_from_device(&f.storage, &f.phone, &json, &destination, &mut |_, _, _| {}, &|| false, NOW).unwrap();
    assert_eq!((result.copied, result.skipped, result.missing), (1, 0, 1));
    let target = f.temp.join("copia").join("Cocina (mp3)");
    assert_eq!(fs::read(target.join("a.mp3")).unwrap(), b"aaaa");
    assert!(target.join(MANIFEST_NAME).is_file());
}

#[test]
fn export_from_device_skips_what_is_already_local() {
    let f = Fixture::new();
    let json = f.device_playlist(&[("id1", "a.mp3")]);
    f.phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaaa");
    let target = f.temp.join("copia").join("Cocina (mp3)");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.mp3"), "aaaa").unwrap();
    let result =
        export_from_device(&f.storage, &f.phone, &json, &f.loc(f.temp.join("copia")), &mut |_, _, _| {}, &|| false, NOW).unwrap();
    assert_eq!((result.copied, result.skipped), (0, 1));
}

#[test]
fn cancelling_a_copy_from_the_device_rolls_back_what_it_pulled() {
    let f = Fixture::new();
    let json = f.device_playlist(&[("id1", "a.mp3"), ("id2", "b.mp3")]);
    f.phone.put_file("Music/Cocina (mp3)/a.mp3", b"aaaa");
    f.phone.put_file("Music/Cocina (mp3)/b.mp3", b"bb");
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 1
    };
    let result =
        export_from_device(&f.storage, &f.phone, &json, &f.loc(f.temp.join("copia")), &mut |_, _, _| {}, &cancelled, NOW).unwrap();
    assert_eq!(result, ExportResult { copied: 0, skipped: 0, missing: 0, cancelled: true, undone: 1 });
    assert!(!f.temp.join("copia").join("Cocina (mp3)").join("a.mp3").exists());
}

#[test]
fn export_from_device_without_a_device_folder_has_no_folder() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa")]);
    let result = export_from_device(&f.storage, &f.phone, &json, &f.loc(&f.temp), &mut |_, _, _| {}, &|| false, NOW);
    assert_eq!(result, Err(TransferError::NoFolder));
}

// === THE MANIFEST ON THE DEVICE ===

#[test]
fn write_manifest_quietly_refreshes_the_manifest_on_the_phone() {
    let f = Fixture::new();
    let json = f.device_playlist(&[("id1", "a.mp3")]);
    f.phone.put_file("Music/Cocina (mp3)/soundify-playlist.json", b"old");
    write_manifest_quietly(&f.storage, &f.phone, &json, &f.scratch, NOW);
    let manifest = manifest_on_phone(&f.phone, "Music/Cocina (mp3)");
    assert_eq!(manifest.folder_name, "Cocina (mp3)");
    assert_eq!(manifest.format, "mp3");
    assert_eq!(manifest.items[0].file_name.as_deref(), Some("a.mp3"));
    // The temporary copy does not stay behind.
    assert_eq!(fs::read_dir(&f.scratch).unwrap().count(), 0);
}

#[test]
fn write_manifest_quietly_leaves_a_local_playlist_alone() {
    let f = Fixture::new();
    let json = f.local_playlist(&[("id1", "a.mp3", "aaaa")]);
    write_manifest_quietly(&f.storage, &f.phone, &json, &f.scratch, NOW);
    assert!(f.phone.pushed().is_empty());
}

// === FINDING AND IMPORTING ===

#[test]
fn find_importable_returns_the_root_when_it_carries_a_manifest() {
    let f = Fixture::new();
    f.phone.put_file("Music/Cocina (mp3)/soundify-playlist.json", b"{}");
    assert_eq!(find_importable(&f.phone, ID, NAME, "Music/Cocina (mp3)"), vec![f.target("Music/Cocina (mp3)")]);
}

#[test]
fn find_importable_looks_two_levels_down_sorted_and_not_inside_a_playlist() {
    let f = Fixture::new();
    f.phone.put_file("Music/Zeta (mp3)/soundify-playlist.json", b"{}");
    f.phone.put_file("Music/Zeta (mp3)/Inner (mp3)/soundify-playlist.json", b"{}");
    f.phone.put_file("Music/sub/alfa (m4a)/soundify-playlist.json", b"{}");
    f.phone.put_file("Music/sub/deep/Too deep (mp3)/soundify-playlist.json", b"{}");
    let found = find_importable(&f.phone, ID, NAME, "Music");
    assert_eq!(found, vec![f.target("Music/sub/alfa (m4a)"), f.target("Music/Zeta (mp3)")]);
}

#[test]
fn a_device_manifest_that_is_not_valid_registers_nothing_and_says_why() {
    let f = Fixture::new();
    let location = f.target("Music/Rara (mp3)");
    for text in ["{}", "{\"schema\":1,\"tit", "{\"playlist\":[\"a\"]}"] {
        f.phone.put_file("Music/Rara (mp3)/soundify-playlist.json", text.as_bytes());
        let error = import_from(&f.storage, &f.phone, &f.archives, &location, &f.scratch, NOW).unwrap_err();
        assert_eq!(error, TransferError::InvalidManifest, "{text}");
        assert!(!is_already_registered(&f.storage, &f.archives, &location), "{text}");
    }
}

#[test]
fn a_device_manifest_from_a_newer_version_asks_to_update() {
    let f = Fixture::new();
    let newer = format!("{{\"schema\":{}}}", SCHEMA + 1);
    f.phone.put_file("Music/Rara (mp3)/soundify-playlist.json", newer.as_bytes());
    let error = import_from(&f.storage, &f.phone, &f.archives, &f.target("Music/Rara (mp3)"), &f.scratch, NOW).unwrap_err();
    assert_eq!(error, TransferError::NewerSchema { found: SCHEMA + 1, supported: SCHEMA });
}

#[test]
fn import_from_registers_the_device_playlist_without_copying_anything() {
    let f = Fixture::new();
    let manifest = PlaylistManifest {
        schema: SCHEMA,
        title: "Cocina".into(),
        format: "mp3".into(),
        items: vec![item("a.mp3"), ManifestItem { id: "id2".into(), ..Default::default() }],
        ..Default::default()
    };
    let bytes = serde_json::to_vec(&manifest).unwrap();
    f.phone.put_file("Music/Cocina (mp3)/soundify-playlist.json", &bytes);

    let location = f.target("Music/Cocina (mp3)");
    let result = import_from(&f.storage, &f.phone, &f.archives, &location, &f.scratch, NOW).unwrap();
    assert_eq!((result.total, result.downloaded, result.pending), (2, 1, 1));
    assert!(is_already_registered(&f.storage, &f.archives, &location));
    let again = import_from(&f.storage, &f.phone, &f.archives, &location, &f.scratch, NOW);
    assert!(matches!(again, Err(TransferError::AlreadyRegistered { .. })));
}

#[test]
fn import_all_goes_on_after_one_fails() {
    let f = Fixture::new();
    let manifest = PlaylistManifest { schema: SCHEMA, title: "Cocina".into(), format: "mp3".into(), ..Default::default() };
    f.phone.put_file("Music/Cocina (mp3)/soundify-playlist.json", &serde_json::to_vec(&manifest).unwrap());
    let locations = [f.target("Music/Nothing (mp3)"), f.target("Music/Cocina (mp3)")];
    let mut said = Vec::new();
    let outcomes =
        import_all(&f.storage, &f.phone, &f.archives, &locations, &mut |done, total, _| said.push((done, total)), &f.scratch, NOW);
    assert_eq!(outcomes[0].error, Some(TransferError::NoManifest));
    assert!(outcomes[1].result.is_some());
    assert_eq!(said, vec![(0, 2), (1, 2), (2, 2)]);
}

#[test]
fn a_manifest_from_a_newer_version_is_refused() {
    let f = Fixture::new();
    let manifest = PlaylistManifest { schema: SCHEMA + 1, title: "Cocina".into(), format: "mp3".into(), ..Default::default() };
    f.phone.put_file("Music/Cocina (mp3)/soundify-playlist.json", &serde_json::to_vec(&manifest).unwrap());
    let result = import_from(&f.storage, &f.phone, &f.archives, &f.target("Music/Cocina (mp3)"), &f.scratch, NOW);
    assert_eq!(result, Err(TransferError::NewerSchema { found: SCHEMA + 1, supported: SCHEMA }));
}

#[test]
fn to_archive_data_writes_the_device_fields_and_the_last_number() {
    let manifest = PlaylistManifest {
        title: "Cocina".into(),
        items: vec![
            ManifestItem { id: "a".into(), number: Some("002".into()), file_name: Some("002 - a.mp3".into()), ..Default::default() },
            ManifestItem { id: "b".into(), number: Some("001".into()), ..Default::default() },
        ],
        ..Default::default()
    };
    let location = DeviceTarget { device_id: ID.into(), device_name: NAME.into(), path: "Music/Cocina (mp3)".into() };
    let data = to_archive_data(&manifest, &location, NOW);
    assert!(data.is_device_backed());
    assert_eq!(data.device_path.as_deref(), Some("Music/Cocina (mp3)"));
    assert_eq!(data.last_number.as_deref(), Some("002"));
    assert_eq!(data.entries["a"].device_file_name.as_deref(), Some("002 - a.mp3"));
}
