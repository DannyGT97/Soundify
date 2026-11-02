//! Ported from the Java app's `PlaylistTransferTest`, against a real temporary
//! folder: "downloaded" means the file EXISTS. The Java cases that passed a
//! `null` folder or archive have no Rust counterpart (the types do not allow it).

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::archive::maintenance;
use crate::storage::LocalStorage;

const ID_1: &str = "dQw4w9WgXcQ";
const ID_2: &str = "aBcDeFgHiJk";
const ID_3: &str = "zZyYxXwWvVu";
const NOW: &str = "2026-08-25 10:00";

struct Fixture {
    _dir: tempfile::TempDir,
    temp: PathBuf,
    music: PathBuf,
    json: Location,
    destination: PathBuf,
    storage: LocalStorage,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().to_path_buf();
        let music = temp.join("Canciones (mp3)");
        let archive_dir = temp.join("archives").join("pl_Canciones_abc").join("mp3");
        let destination = temp.join("destino");
        for folder in [&music, &archive_dir, &destination] {
            fs::create_dir_all(folder).unwrap();
        }
        Self {
            json: Location::Path(archive_dir.join("data.json")),
            _dir: dir,
            temp,
            music,
            destination,
            storage: LocalStorage,
        }
    }

    fn loc(path: &Path) -> Location {
        Location::Path(path.to_path_buf())
    }

    fn real_file(&self, name: &str, content: &str) -> PathBuf {
        let file = self.music.join(name);
        fs::write(&file, content).unwrap();
        file
    }

    fn save(&self, json: &Location, data: &ArchiveData) {
        io::save(&self.storage, json, data).unwrap();
    }

    fn load(&self) -> ArchiveData {
        io::load(&self.storage, &self.json).unwrap()
    }

    /// One downloaded, one pending and one unavailable.
    fn write_archive(&self, numbered: bool) {
        let mut data = ArchiveData {
            url: Some("https://www.youtube.com/playlist?list=PLtest".into()),
            folder: Some(path_string(&self.music)),
            number_files: numbered,
            ..Default::default()
        };
        let name = if numbered { "001 - Tema bajado.mp3" } else { "Tema bajado.mp3" };
        let file = self.real_file(name, "audio");
        data.entries.insert(ID_1.into(), entry("Tema bajado", numbered.then_some("001"), Some(&file)));
        data.entries.insert(ID_2.into(), entry("Tema pendiente", numbered.then_some("002"), None));
        let mut private = entry("Tema privado", None, None);
        private.unavailable_at = Some("2026-08-20T10:00:00".into());
        private.unavailable_reason = Some("vídeo privado".into());
        data.entries.insert(ID_3.into(), private);
        self.save(&self.json, &data);
    }

    /// A second downloaded file, added to the archive after `write_archive`.
    fn add_second_file(&self) {
        let file = self.real_file("002 - Otro.mp3", "otro");
        let mut data = self.load();
        data.entries.insert("kKlLmMnNoOp".into(), entry("Otro", Some("002"), Some(&file)));
        self.save(&self.json, &data);
    }

    fn export(&self) -> ExportResult {
        self.export_with(&mut |_, _, _| {}, &|| false)
    }

    fn export_with(&self, progress: &mut dyn FnMut(usize, usize, &str), cancelled: &dyn Fn() -> bool) -> ExportResult {
        export(&self.storage, &self.json, &Self::loc(&self.destination), progress, cancelled, NOW).unwrap()
    }

    fn exported(&self) -> PathBuf {
        self.destination.join("Canciones (mp3)")
    }

    fn manifest_at(&self, folder: &Path) -> PlaylistManifest {
        read_manifest(&self.storage, &Self::loc(folder)).unwrap()
    }

    fn write_manifest_at(&self, folder: &Path, manifest: &PlaylistManifest) {
        write_manifest(&self.storage, &Self::loc(folder), manifest).unwrap();
    }

    fn archives(&self) -> Location {
        Self::loc(&self.temp.join("archives"))
    }

    /// A manifest that does know which file each id is.
    fn manifest_with_files(&self, file_names: &[&str]) -> PlaylistManifest {
        let items = file_names
            .iter()
            .enumerate()
            .map(|(i, name)| ManifestItem {
                id: format!("id000000{i}AA"),
                title: Some(format!("Tema {i}")),
                file_name: Some(name.to_string()),
                ..Default::default()
            })
            .collect();
        PlaylistManifest {
            schema: SCHEMA,
            exported_at: "2026-08-25T10:00:00".into(),
            title: "Canciones".into(),
            url: Some("https://www.youtube.com/playlist?list=PLtest".into()),
            format: "mp3".into(),
            number_files: false,
            folder_name: "Canciones (mp3)".into(),
            items,
        }
    }

    /// An archive that knows the ids but NOT their files (what "Crear Playlist" leaves).
    fn archive_without_files(&self, how_many: usize) -> ArchiveData {
        let mut data = ArchiveData { folder: Some(path_string(&self.music)), ..Default::default() };
        for i in 0..how_many {
            data.entries.insert(
                format!("id000000{i}AA"),
                ArchiveEntry {
                    title: Some(format!("Tema {i}")),
                    added_at: Some("2026-08-25 21:27".into()),
                    ..Default::default()
                },
            );
        }
        data
    }

    /// A believable playlist folder: music and manifest inside.
    fn exported_folder_inside(&self, parent: &Path, folder_name: &str) -> PathBuf {
        let folder = parent.join(folder_name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Algo.mp3"), "audio").unwrap();
        let mut manifest = manifest_for(&title_from_folder_name(folder_name, "mp3"), "mp3");
        manifest.folder_name = folder_name.into();
        self.write_manifest_at(&folder, &manifest);
        folder
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn entry(title: &str, number: Option<&str>, file: Option<&Path>) -> ArchiveEntry {
    ArchiveEntry {
        title: Some(title.into()),
        number: number.map(Into::into),
        file_path: file.map(path_string),
        added_at: Some("2026-08-25T10:00:00".into()),
        ..Default::default()
    }
}

fn item<'a>(manifest: &'a PlaylistManifest, id: &str) -> &'a ManifestItem {
    manifest.items.iter().find(|i| i.id == id).unwrap()
}

/// A loose manifest, with no archive involved: what is in a folder brought from outside.
fn manifest_for(title: &str, format: &str) -> PlaylistManifest {
    PlaylistManifest {
        schema: SCHEMA,
        exported_at: "2026-08-25T10:00:00".into(),
        title: title.into(),
        url: Some(format!("https://www.youtube.com/playlist?list=PL{title}")),
        format: format.into(),
        number_files: false,
        folder_name: format!("{title} ({format})"),
        items: vec![],
    }
}

// === THE MANIFEST ===

#[test]
fn the_manifest_describes_the_playlist_without_absolute_paths() {
    let f = Fixture::new();
    f.write_archive(true);

    let manifest = build_manifest(&f.storage, &f.json, NOW).unwrap();

    assert_eq!(manifest.title, "Canciones");
    assert_eq!(manifest.format, "mp3");
    assert_eq!(manifest.folder_name, "Canciones (mp3)");
    assert!(manifest.number_files);
    assert_eq!(manifest.items.len(), 3);
    let downloaded = item(&manifest, ID_1).file_name.as_deref().unwrap();
    assert_eq!(downloaded, "001 - Tema bajado.mp3");
    assert!(!downloaded.contains(std::path::MAIN_SEPARATOR), "the folder is set by whoever imports");
}

#[test]
fn what_is_not_downloaded_travels_without_a_file() {
    let f = Fixture::new();
    f.write_archive(true);

    let manifest = build_manifest(&f.storage, &f.json, NOW).unwrap();

    assert_eq!(item(&manifest, ID_2).file_name, None);
    assert_eq!(item(&manifest, ID_3).file_name, None);
    assert_eq!(item(&manifest, ID_3).unavailable_reason.as_deref(), Some("vídeo privado"), "not retried at the destination");
    assert_eq!(downloaded_items(&manifest).len(), 1);
}

#[test]
fn the_title_comes_from_the_folder_name() {
    assert_eq!(title_from_folder_name("Canciones (mp3)", "mp3"), "Canciones");
    assert_eq!(title_from_folder_name("Mi (mix) raro (mp4)", "mp4"), "Mi (mix) raro");
    // If it does not end in " (format)" it is left as-is, with no blind trimming.
    assert_eq!(title_from_folder_name("Suelta", "mp3"), "Suelta");
    assert_eq!(title_from_folder_name("", "mp3"), "playlist");
}

// === EXPORTING ===

#[test]
fn exporting_copies_what_is_downloaded_and_leaves_the_manifest_beside_it() {
    let f = Fixture::new();
    f.write_archive(true);

    let result = f.export();

    assert_eq!((result.copied, result.skipped, result.cancelled), (1, 0, false));
    assert_eq!(fs::read_to_string(f.exported().join("001 - Tema bajado.mp3")).unwrap(), "audio");
    assert!(f.exported().join(MANIFEST_NAME).exists());
}

#[test]
fn regression_restarting_an_interrupted_export_does_not_recopy_what_is_there() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();

    let second = f.export();

    assert_eq!(second.copied, 0);
    assert_eq!(second.skipped, 1, "same name and same size: it is not touched");
}

#[test]
fn a_file_of_a_different_size_is_copied_again() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();
    // A copy that was cut short leaves the file incomplete.
    let half = f.exported().join("001 - Tema bajado.mp3");
    fs::write(&half, "a").unwrap();

    assert_eq!(f.export().copied, 1);
    assert_eq!(fs::read_to_string(&half).unwrap(), "audio");
}

#[test]
fn cancelling_rolls_back_what_was_copied_and_leaves_no_manifest() {
    let f = Fixture::new();
    f.write_archive(true);
    // Two downloaded files: it is cancelled after the first.
    f.add_second_file();

    let copied_one = std::cell::Cell::new(false);
    let result = f.export_with(&mut |_, _, _| copied_one.set(true), &|| copied_one.get());

    assert!(result.cancelled);
    assert_eq!(result.copied, 0, "what was copied is undone");
    assert_eq!(result.undone, 1);
    assert!(!f.exported().join(MANIFEST_NAME).exists(), "whoever cancels does not want half an export");
    assert!(!f.exported().exists(), "and if the folder is left empty, it goes with it");
}

#[test]
fn cancelling_does_not_touch_what_an_earlier_export_had_left() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();
    // One more file is added to the playlist and it is cancelled on the fly.
    f.add_second_file();

    let result = f.export_with(&mut |_, _, _| {}, &|| true);

    assert!(result.cancelled);
    assert_eq!(result.undone, 0, "nothing new was copied");
    assert!(f.exported().join("001 - Tema bajado.mp3").exists(), "not ours to delete");
    assert!(f.exported().join(MANIFEST_NAME).exists(), "and its manifest is not touched either");
}

#[test]
fn progress_goes_item_by_item() {
    let f = Fixture::new();
    f.write_archive(true);
    let mut seen = String::new();

    f.export_with(&mut |done, total, _| seen.push_str(&format!("{done}/{total} ")), &|| false);

    assert_eq!(seen, "1/1 ");
}

// === IMPORTING ===

#[test]
fn importing_rebuilds_the_paths_against_the_new_folder() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();

    let manifest = f.manifest_at(&f.exported());
    let format_folder = f.destination.join(&manifest.folder_name);
    let imported = to_archive_data(&f.storage, &manifest, &Fixture::loc(&format_folder), NOW).unwrap();

    assert_eq!(imported.folder, Some(path_string(&format_folder)));
    assert_eq!(imported.entries[ID_1].file_path, Some(path_string(&format_folder.join("001 - Tema bajado.mp3"))));
    assert_eq!(imported.entries[ID_2].file_path, None);
    assert!(imported.number_files, "the numbering mode travels: it is immutable");
    assert_eq!(imported.url.as_deref(), Some("https://www.youtube.com/playlist?list=PLtest"));
}

#[test]
fn what_was_copied_counts_as_downloaded_and_the_rest_stays_pending() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();

    let manifest = f.manifest_at(&f.exported());
    let imported = to_archive_data(&f.storage, &manifest, &Fixture::loc(&f.exported()), NOW).unwrap();

    assert_eq!(count_downloaded(&f.storage, &imported), 1);
    assert_eq!(imported.entries[ID_2].file_path, None);
    assert_eq!(imported.entries[ID_3].unavailable_at.as_deref(), Some("2026-08-20T10:00:00"), "not retried");
    assert_eq!(imported.last_number.as_deref(), Some("002"));
}

#[test]
fn if_the_file_is_not_there_it_does_not_count_as_downloaded() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();
    // Somebody deletes the mp3 at the destination (or the copy never reached it).
    fs::remove_file(f.exported().join("001 - Tema bajado.mp3")).unwrap();

    let manifest = f.manifest_at(&f.exported());
    let imported = to_archive_data(&f.storage, &manifest, &Fixture::loc(&f.exported()), NOW).unwrap();

    assert_eq!(count_downloaded(&f.storage, &imported), 0, "the file has to exist, whatever the JSON says");
}

#[test]
fn with_no_manifest_it_says_so_clearly() {
    let f = Fixture::new();
    let error = read_manifest(&f.storage, &Fixture::loc(&f.exported())).unwrap_err();
    assert_eq!(error, TransferError::NoManifest);
    assert!(error.to_string().contains(MANIFEST_NAME));
    assert_eq!(error.args(), [MANIFEST_NAME]);
}

#[test]
fn a_manifest_from_a_newer_version_is_rejected_not_half_read() {
    let f = Fixture::new();
    let mut future = manifest_for("Canciones", "mp3");
    future.schema = SCHEMA + 1;
    f.write_manifest_at(&f.destination, &future);

    let error = read_manifest(&f.storage, &Fixture::loc(&f.destination)).unwrap_err();

    assert_eq!(error, TransferError::NewerSchema { found: SCHEMA + 1, supported: SCHEMA });
}

#[test]
fn regression_two_exports_to_the_same_destination_coexist() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();

    // A second playlist, same destination: another folder, another manifest.
    let other_music = f.temp.join("Clasicos (mp4)");
    fs::create_dir_all(&other_music).unwrap();
    fs::write(other_music.join("Algo.mp4"), "video").unwrap();
    let other_json = Fixture::loc(&f.temp.join("archives/pl_Clasicos_xyz/mp4/data.json"));
    let mut other = ArchiveData { folder: Some(path_string(&other_music)), ..Default::default() };
    other.entries.insert("kKlLmMnNoOp".into(), entry("Algo", None, Some(&other_music.join("Algo.mp4"))));
    f.save(&other_json, &other);

    export(&f.storage, &other_json, &Fixture::loc(&f.destination), &mut |_, _, _| {}, &|| false, NOW).unwrap();

    // The manifest goes inside each folder, so neither overwrites the other.
    assert_eq!(f.manifest_at(&f.exported()).title, "Canciones");
    assert_eq!(f.manifest_at(&f.destination.join("Clasicos (mp4)")).title, "Clasicos");
}

#[test]
fn pointing_at_the_playlist_folder_or_at_the_one_containing_it_both_work() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();
    let playlist = Fixture::loc(&f.exported());
    let manifest = f.manifest_at(&f.exported());

    // The playlist's folder (where the manifest is left today).
    assert_eq!(resolve_format_folder(&f.storage, &playlist, &manifest).unwrap(), playlist);
    // And the one above, which is how earlier exports were left.
    assert_eq!(resolve_format_folder(&f.storage, &Fixture::loc(&f.destination), &manifest).unwrap(), playlist);
}

#[test]
fn regression_deleting_an_imported_playlist_leaves_neither_the_manifest_nor_the_folder() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();

    // The imported playlist's archive points at the copied folder, which has the
    // music AND the manifest inside.
    let manifest = f.manifest_at(&f.exported());
    let imported_json = Fixture::loc(&f.temp.join("archives/importada/mp3/data.json"));
    f.save(&imported_json, &to_archive_data(&f.storage, &manifest, &Fixture::loc(&f.exported()), NOW).unwrap());

    let result = maintenance::delete_all_downloaded_files(&f.storage, &imported_json);

    assert_eq!(result.deleted, 1);
    assert_eq!(result.left_over, 0, "the manifest was written by the app: it is not somebody else's file");
    assert!(!f.exported().join(MANIFEST_NAME).exists());
    assert!(!f.exported().exists(), "with nothing inside, the folder goes with it");
}

#[test]
fn if_the_files_are_nowhere_to_be_found_it_says_so() {
    let f = Fixture::new();
    f.write_archive(true);
    let manifest = build_manifest(&f.storage, &f.json, NOW).unwrap();

    let error = resolve_format_folder(&f.storage, &Fixture::loc(&f.destination), &manifest).unwrap_err();

    assert!(error.to_string().contains("Canciones (mp3)"));
}

#[test]
fn import_from_registers_it_once_and_refuses_it_twice() {
    let f = Fixture::new();
    f.write_archive(true);
    f.export();
    let archives = Fixture::loc(&f.temp.join("archivos de otra instalacion"));
    let folder = Fixture::loc(&f.exported());

    let result = import_from(&f.storage, &archives, &folder, NOW).unwrap();

    assert_eq!((result.total, result.downloaded, result.pending), (3, 1, 1), "the unavailable one is not pending");
    assert!(is_already_registered(&f.storage, &archives, &folder));
    let again = import_from(&f.storage, &archives, &folder, NOW).unwrap_err();
    assert_eq!(again, TransferError::AlreadyRegistered { title: "Canciones".into(), format: "mp3".into() });
}

// === ODDS AND ENDS ===

#[test]
fn already_there_when_name_and_size_match() {
    let f = Fixture::new();
    let source = Fixture::loc(&f.real_file("a.mp3", "12345"));
    let target_path = f.destination.join("a.mp3");
    let target = Fixture::loc(&target_path);

    assert!(!already_there(&f.storage, &source, &target), "it is not there yet");
    fs::write(&target_path, "12345").unwrap();
    assert!(already_there(&f.storage, &source, &target));
    fs::write(&target_path, "12").unwrap();
    assert!(!already_there(&f.storage, &source, &target), "half of it does not count");
}

#[test]
fn the_size_is_shown_in_readable_units() {
    assert_eq!(readable_size(512), "512 B");
    assert_eq!(readable_size(2048), "2 KB");
    assert_eq!(readable_size(5 * 1024 * 1024), "5 MB");
    assert_eq!(readable_size(3 * 512 * 1024 * 1024), "1.5 GB");
}

// === THE MANIFEST LIVES WITH THE PLAYLIST (not only in the copies) ===

#[test]
fn the_manifest_stays_with_the_playlist_without_anyone_copying_anything() {
    let f = Fixture::new();
    f.write_archive(true);

    write_manifest_quietly(&f.storage, &f.json, NOW);

    assert!(f.music.join(MANIFEST_NAME).is_file(), "that folder has to be importable as-is");
    assert_eq!(f.manifest_at(&f.music).title, "Canciones");
    assert!(has_manifest(&f.storage, &Fixture::loc(&f.music)));
}

#[test]
fn single_downloads_carry_no_manifest() {
    let f = Fixture::new();
    f.write_archive(true);
    // The same contents with the flat scheme of single downloads: no pl_* folder.
    let individual = f.temp.join("archives/individual_archive_mp3.json");
    fs::copy(f.json.as_path().unwrap(), &individual).unwrap();

    write_manifest_quietly(&f.storage, &Fixture::loc(&individual), NOW);

    assert!(!f.music.join(MANIFEST_NAME).exists());
}

#[test]
fn if_the_folder_is_gone_nothing_is_written_and_it_is_not_recreated() {
    let f = Fixture::new();
    f.write_archive(true);
    let ghost = f.temp.join("unidad desconectada/Canciones (mp3)");
    let mut data = f.load();
    data.folder = Some(path_string(&ghost));
    f.save(&f.json, &data);

    write_manifest_quietly(&f.storage, &f.json, NOW);

    assert!(!ghost.exists());
}

// === COPYING: NEVER OVER ITSELF ===

#[test]
fn regression_copying_to_the_base_folder_ended_up_copying_the_playlist_over_itself() {
    let f = Fixture::new();
    f.write_archive(true);

    // The temp folder is the base: the copy would go to "<temp>/Canciones (mp3)",
    // which is precisely the source.
    let error = export(&f.storage, &f.json, &Fixture::loc(&f.temp), &mut |_, _, _| {}, &|| false, NOW).unwrap_err();
    assert!(error.to_string().contains("Canciones"), "{error}");
    assert_eq!(error.args(), ["Canciones"]);

    // And the source is left intact.
    assert_eq!(fs::read_to_string(f.music.join("001 - Tema bajado.mp3")).unwrap(), "audio");
}

#[test]
fn the_same_folder_is_the_same_even_if_the_path_takes_a_detour() {
    let f = Fixture::new();
    let direct = Fixture::loc(&f.music);
    let detour = Fixture::loc(&f.temp.join(".").join("x").join("..").join("Canciones (mp3)"));
    let other_case = Fixture::loc(&f.temp.join("CANCIONES (MP3)"));

    assert!(is_same_folder(&direct, &detour));
    assert!(is_same_folder(&direct, &other_case));
    assert!(!is_same_folder(&direct, &Fixture::loc(&f.destination)));
    assert!(!is_same_folder(&direct, &Location::uri("content://x/Canciones (mp3)")));
}

// === FINDING PLAYLISTS TO IMPORT ===

#[test]
fn pointing_at_a_playlist_folder_finds_that_one() {
    let f = Fixture::new();
    let folder = f.exported_folder_inside(&f.temp, "Cocina (mp3)");

    assert_eq!(find_importable(&f.storage, &Fixture::loc(&folder)), [Fixture::loc(&folder)]);
}

#[test]
fn pointing_at_the_folder_that_contains_them_finds_them_all_at_once() {
    let f = Fixture::new();
    let music = f.temp.join("Musica");
    f.exported_folder_inside(&music, "Cocina (mp3)");
    f.exported_folder_inside(&music, "Clasicos (mp3)");
    // One level further down: somebody who keeps them sorted into folders.
    f.exported_folder_inside(&music.join("Viejas"), "Ochenta (mp3)");
    // And something that is not a playlist: it does not appear.
    fs::create_dir_all(music.join("Fotos")).unwrap();

    let found = find_importable(&f.storage, &Fixture::loc(&music));

    let names: Vec<String> = found.iter().map(Location::name).collect();
    assert_eq!(names, ["Clasicos (mp3)", "Cocina (mp3)", "Ochenta (mp3)"], "all three, in order");
}

#[test]
fn it_does_not_search_inside_an_already_found_playlist() {
    let f = Fixture::new();
    let kitchen = f.exported_folder_inside(&f.temp, "Cocina (mp3)");
    // A manifest NESTED inside another playlist (a copy made by hand): the outer one counts.
    f.exported_folder_inside(&kitchen, "Copia (mp3)");
    // The fixture's own folders carry no manifest.

    assert_eq!(find_importable(&f.storage, &Fixture::loc(&f.temp)), [Fixture::loc(&kitchen)]);
}

#[test]
fn where_there_are_no_playlists_none_is_invented() {
    let f = Fixture::new();
    fs::create_dir_all(f.temp.join("vacia/mas vacia")).unwrap();

    assert!(find_importable(&f.storage, &Fixture::loc(&f.temp.join("vacia"))).is_empty());
    assert!(find_importable(&f.storage, &Fixture::loc(&f.temp.join("no existe"))).is_empty());
}

#[test]
fn one_failure_does_not_stop_the_batch() {
    let f = Fixture::new();
    let without = f.temp.join("Sin nada");
    let neither = f.temp.join("Tampoco");
    fs::create_dir_all(&without).unwrap();
    fs::create_dir_all(&neither).unwrap();

    let outcomes = import_all(&f.storage, &f.archives(), &[Fixture::loc(&without), Fixture::loc(&neither)], NOW);

    assert_eq!(outcomes.len(), 2, "both are attempted");
    assert!(outcomes.iter().all(|o| !o.ok() && !o.already_registered));
    assert_eq!(outcomes[0].error, Some(TransferError::NoManifest), "the reason says what is missing");
}

// === IS IT ALREADY REGISTERED HERE? ===
//
// Checked against the SAME calculation import_from() uses to decide where to
// write: if the two drifted apart, the list would say one thing and the import
// would do another (which is exactly what was reported).

#[test]
fn asking_where_the_archive_would_go_does_not_create_the_archive_folder() {
    let f = Fixture::new();
    let folder = f.temp.join("Cocina (mp3)");
    fs::create_dir_all(&folder).unwrap();

    let target = resolve_target(&f.storage, &f.archives(), &Fixture::loc(&folder), &manifest_for("Cocina", "mp3")).unwrap();

    assert!(!target.json.as_path().unwrap().parent().unwrap().exists(), "asking may have no side effects");
}

#[test]
fn regression_the_same_playlist_as_mp3_and_as_mp4_are_two_local_playlists() {
    let f = Fixture::new();
    let mp3 = f.temp.join("Cocina (mp3)");
    let mp4 = f.temp.join("Cocina (mp4)");
    fs::create_dir_all(&mp3).unwrap();
    fs::create_dir_all(&mp4).unwrap();

    let json = |folder: &Path, format| {
        let target = resolve_target(&f.storage, &f.archives(), &Fixture::loc(folder), &manifest_for("Cocina", format));
        target.unwrap().json.as_path().unwrap().to_path_buf()
    };
    let (json_mp3, json_mp4) = (json(&mp3, "mp3"), json(&mp4, "mp4"));

    assert_ne!(json_mp3, json_mp4);
    assert!(json_mp3.parent().unwrap().ends_with("mp3"));
    assert!(json_mp4.parent().unwrap().ends_with("mp4"));
    // The same playlist: they share the pl_Cocina_hash folder above.
    assert_eq!(json_mp3.parent().unwrap().parent(), json_mp4.parent().unwrap().parent());
}

#[test]
fn the_same_playlist_under_another_base_folder_is_another_local_playlist() {
    let f = Fixture::new();
    let here = f.temp.join("uno/Cocina (mp3)");
    let there = f.temp.join("dos/Cocina (mp3)");
    fs::create_dir_all(&here).unwrap();
    fs::create_dir_all(&there).unwrap();
    let manifest = manifest_for("Cocina", "mp3");

    let a = resolve_target(&f.storage, &f.archives(), &Fixture::loc(&here), &manifest).unwrap();
    let b = resolve_target(&f.storage, &f.archives(), &Fixture::loc(&there), &manifest).unwrap();

    assert_ne!(a.json, b.json);
}

#[test]
fn the_music_folder_the_manifest_yields_is_the_one_that_holds_the_music() {
    let f = Fixture::new();
    let manifest = manifest_for("Cocina", "mp3");
    // Pointing at the playlist's folder, that same one is the right one.
    let inside = f.temp.join("Cocina (mp3)");
    fs::create_dir_all(&inside).unwrap();
    let target = resolve_target(&f.storage, &f.archives(), &Fixture::loc(&inside), &manifest).unwrap();
    assert_eq!(target.format_folder, Fixture::loc(&inside));

    // And pointing at the one above (manifests of the old scheme), the one inside.
    let parent = f.temp.join("suelta");
    let music = parent.join("Cocina (mp3)");
    fs::create_dir_all(&music).unwrap();
    let target = resolve_target(&f.storage, &f.archives(), &Fixture::loc(&parent), &manifest).unwrap();
    assert_eq!(target.format_folder, Fixture::loc(&music));
}

#[test]
fn when_in_doubt_it_is_not_taken_as_registered() {
    let f = Fixture::new();
    let without = Fixture::loc(&f.temp.join("Sin manifiesto"));
    fs::create_dir_all(without.as_path().unwrap()).unwrap();

    assert!(!is_already_registered(&f.storage, &f.archives(), &without));
    assert!(!is_already_registered(&f.storage, &f.archives(), &Fixture::loc(&f.temp.join("ni existe"))));
    // What cannot be checked is proposed, and import_from() will say why it cannot.
    assert_eq!(not_yet_registered(&f.storage, &f.archives(), std::slice::from_ref(&without)), [without]);
}

// === THE MANIFEST MAY NEITHER GET WORSE NOR FALL SHORT ===

#[test]
fn regression_registering_a_playlist_over_downloaded_music_used_to_flatten_its_manifest() {
    let f = Fixture::new();
    f.real_file("Tema 0.mp3", "audio");
    f.real_file("Tema 1.mp3", "audio");
    f.write_manifest_at(&f.music, &f.manifest_with_files(&["Tema 0.mp3", "Tema 1.mp3"]));

    // The archive knows of no file: the new manifest would come out with both empty.
    f.save(&f.json, &f.archive_without_files(2));
    write_manifest_quietly(&f.storage, &f.json, NOW);

    let after = f.manifest_at(&f.music);
    assert_eq!(after.items.iter().filter(|i| i.file_name.is_some()).count(), 2, "the names it knew are kept");
}

#[test]
fn but_it_does_not_resurrect_the_name_of_a_file_that_is_gone() {
    let f = Fixture::new();
    f.real_file("Tema 0.mp3", "audio");
    // "Tema 1.mp3" is in the manifest but NOT on disk (deleted by hand).
    f.write_manifest_at(&f.music, &f.manifest_with_files(&["Tema 0.mp3", "Tema 1.mp3"]));

    f.save(&f.json, &f.archive_without_files(2));
    write_manifest_quietly(&f.storage, &f.json, NOW);

    let after = f.manifest_at(&f.music);
    assert_eq!(item(&after, "id0000000AA").file_name.as_deref(), Some("Tema 0.mp3"));
    assert_eq!(item(&after, "id0000001AA").file_name, None, "never claim something is there when it is not");
}

#[test]
fn what_the_archive_does_know_wins_over_the_old_manifest() {
    let f = Fixture::new();
    let new_name = f.real_file("Nombre nuevo.mp3", "audio");
    f.real_file("Nombre viejo.mp3", "audio");
    f.write_manifest_at(&f.music, &f.manifest_with_files(&["Nombre viejo.mp3"]));

    let mut data = f.archive_without_files(1);
    data.entries.get_mut("id0000000AA").unwrap().file_path = Some(path_string(&new_name));
    f.save(&f.json, &data);
    write_manifest_quietly(&f.storage, &f.json, NOW);

    assert_eq!(item(&f.manifest_at(&f.music), "id0000000AA").file_name.as_deref(), Some("Nombre nuevo.mp3"));
}

// === RELINKING FROM THE MANIFEST ===

#[test]
fn regression_the_music_was_in_the_folder_and_everything_showed_up_as_pending() {
    let f = Fixture::new();
    // As yt-dlp leaves it, with the character swapped: matching by title would NOT work.
    let real = "Cómo descalcificar la cafetera ｜ NESCAFÉ.mp3";
    let file = f.real_file(real, "audio");
    f.write_manifest_at(&f.music, &f.manifest_with_files(&[real]));
    let mut data = f.archive_without_files(1);
    data.entries.get_mut("id0000000AA").unwrap().title = Some("Cómo descalcificar la cafetera | NESCAFÉ".into());
    data.entries.get_mut("id0000000AA").unwrap().deleted_at = Some("2026-08-25 21:30".into());

    let relinked = relink_from_manifest(&f.storage, &mut data, &Fixture::loc(&f.music));

    assert_eq!(relinked, 1);
    let entry = &data.entries["id0000000AA"];
    assert_eq!(entry.file_path, Some(path_string(&file)));
    assert_eq!(entry.title.as_deref(), Some("Cómo descalcificar la cafetera ｜ NESCAFÉ"), "the real file's title");
    assert_eq!(entry.deleted_at, None, "it had a file from the start: this is not a removal");
}

#[test]
fn relinking_only_fills_gaps() {
    let f = Fixture::new();
    f.real_file("Tema 0.mp3", "audio");
    let already = f.real_file("Otro.mp3", "audio");
    f.write_manifest_at(&f.music, &f.manifest_with_files(&["Tema 0.mp3"]));
    let mut data = f.archive_without_files(1);
    data.entries.get_mut("id0000000AA").unwrap().file_path = Some(path_string(&already));

    assert_eq!(relink_from_manifest(&f.storage, &mut data, &Fixture::loc(&f.music)), 0);
    assert_eq!(data.entries["id0000000AA"].file_path, Some(path_string(&already)), "never overwrites a known file");
    assert_eq!(data.entries.len(), 1, "and it adds no ids the archive did not have");
}

#[test]
fn what_does_not_exist_is_not_relinked() {
    let f = Fixture::new();
    f.write_manifest_at(&f.music, &f.manifest_with_files(&["No está.mp3"]));
    let mut data = f.archive_without_files(1);

    assert_eq!(relink_from_manifest(&f.storage, &mut data, &Fixture::loc(&f.music)), 0);
    assert_eq!(data.entries["id0000000AA"].file_path, None);
}

#[test]
fn with_no_manifest_there_is_nothing_to_relink() {
    let f = Fixture::new();
    let mut data = f.archive_without_files(1);

    assert_eq!(relink_from_manifest(&f.storage, &mut data, &Fixture::loc(&f.music)), 0);
    assert_eq!(relink_from_manifest(&f.storage, &mut data, &Fixture::loc(&f.temp.join("ni existe"))), 0);
}
