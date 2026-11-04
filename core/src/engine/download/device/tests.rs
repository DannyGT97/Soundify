use super::*;
use crate::clock;
use crate::device::fake::{FakeDevice, ID, NAME};
use crate::downloader::{Downloader, StopFlag};
use crate::engine::safety_net::{Patience, RetryProgress};
use crate::model::UserSettings;
use crate::storage::MemoryStorage;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=PL1";
const A: &str = "aaaaaaaaaaa";
const B: &str = "bbbbbbbbbbb";
const BASE: &str = "Internal storage/Music";
const FOLDER: &str = "Internal storage/Music/Clásicos (mp3)";

/// A yt-dlp that lists a two-video playlist and "downloads" by writing a real
/// file into the staging folder (the `-o` template's folder), named after the
/// video's real title, as the real one would before the rename to `<id>.ext`.
struct FakeYtDlp {
    titles: Vec<(&'static str, &'static str)>,
    /// Ids whose download exits with an error.
    broken: Vec<&'static str>,
    /// Run between the download and the push: a phone unplugged right then.
    after_download: Option<Box<dyn Fn() + Send + Sync>>,
    downloads: Mutex<Vec<String>>,
}

impl FakeYtDlp {
    fn new() -> Self {
        Self { titles: vec![(A, "Primero"), (B, "Segundo")], broken: Vec::new(), after_download: None, downloads: Mutex::default() }
    }

    fn downloads(&self) -> Vec<String> {
        self.downloads.lock().unwrap().clone()
    }
}

impl Downloader for FakeYtDlp {
    fn run(&self, request: &YtDlpRequest, _: &StopFlag, on_line: &mut dyn FnMut(OutputLine)) -> io::Result<RunEnd> {
        let args = &request.args;
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let mut out = |line: String| on_line(OutputLine::Stdout(line));
        if has("playlist_title") {
            out("Clásicos".into());
            return Ok(RunEnd::Finished { exit_code: 0 });
        }
        if has("%(playlist_index)s|%(id)s|%(title)s") {
            for (i, (id, title)) in self.titles.iter().enumerate() {
                out(format!("{}|{id}|{title}", i + 1));
            }
            return Ok(RunEnd::Finished { exit_code: 0 });
        }
        if has("--simulate") {
            return Ok(RunEnd::Finished { exit_code: 0 });
        }
        let id = args.last().and_then(|u| u.rsplit_once("v=")).map(|(_, id)| id.to_string()).unwrap_or_default();
        self.downloads.lock().unwrap().push(id.clone());
        if self.broken.contains(&id.as_str()) {
            out("ERROR: something broke".into());
            return Ok(RunEnd::Finished { exit_code: 1 });
        }
        let template = args.iter().position(|a| a == "-o").map(|i| PathBuf::from(&args[i + 1])).unwrap();
        let title = self.titles.iter().find(|(i, _)| *i == id).map_or("Suelto", |(_, t)| t);
        let file = template.parent().unwrap().join(format!("{title}.mp3"));
        fs::write(&file, b"audio")?;
        out(format!("{SEEN_PREFIX}{id}|{title}"));
        out(format!("{MOVED_PREFIX}{id}|{}", file.display()));
        if let Some(hook) = &self.after_download {
            hook();
        }
        Ok(RunEnd::Finished { exit_code: 0 })
    }
}

#[derive(Default)]
struct Recorded {
    keys: Vec<String>,
    notices: Vec<(&'static str, String)>,
    downloaded: Vec<String>,
}

impl DownloadEvents for Recorded {
    fn line(&mut self, line: ActivityLine) {
        if let ActivityLine::Message { key, .. } = line {
            self.keys.push(key);
        }
    }
    fn title(&mut self, _: &str) {}
    fn archive(&mut self, _: &Location) {}
    fn step(&mut self, _: Step, _: u32) {}
    fn progress(&mut self, _: f64) {}
    fn retry(&mut self, _: RetryProgress) {}
    fn downloaded(&mut self, id: &str) {
        self.downloaded.push(id.to_string());
    }
    fn notice(&mut self, status: &'static str, key: &str, _: &[&str]) {
        self.notices.push((status, key.to_string()));
    }
}

struct World {
    store: ArchiveStore,
    settings_file: Location,
    phone: Arc<FakeDevice>,
    scratch: tempfile::TempDir,
    staging: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let storage = Arc::new(MemoryStorage::new());
        let store = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-03 10:00"));
        let settings_file = Location::path("/config/settings.json");
        let settings = UserSettings { working_player_client: Some("web".into()), ..UserSettings::default() };
        settings::save(storage.as_ref(), &settings_file, &settings).unwrap();
        Self {
            store,
            settings_file,
            phone: Arc::new(FakeDevice::new()),
            scratch: tempfile::tempdir().unwrap(),
            staging: tempfile::tempdir().unwrap(),
        }
    }

    fn engine<'a>(&'a self, downloader: &'a dyn Downloader, stop: &'a StopFlag) -> Engine<'a> {
        Engine {
            downloader,
            archives: &self.store,
            settings_file: &self.settings_file,
            ffmpeg_dir: None,
            scratch: self.scratch.path().to_path_buf(),
            stop,
            patience: Patience { file_polls: 0, file_poll: Duration::ZERO, move_attempts: 1, move_pause: Duration::ZERO },
        }
    }

    fn side(&self) -> DeviceSide<'_> {
        DeviceSide { phone: self.phone.as_ref(), staging_root: self.staging.path().to_path_buf() }
    }

    fn target() -> DeviceTarget {
        DeviceTarget { device_id: ID.into(), device_name: NAME.into(), path: BASE.into() }
    }

    fn create(&self, downloader: &dyn Downloader, numbered: bool) -> Created {
        self.engine(downloader, &StopFlag::new())
            .create_device_playlist(&self.side(), PLAYLIST_URL, "MP3", &Self::target(), numbered)
            .expect("the fake lists the playlist")
    }

    fn run(&self, downloader: &dyn Downloader, job: &DeviceJob) -> Recorded {
        let mut events = Recorded::default();
        self.engine(downloader, &StopFlag::new()).run_device(&self.side(), job, &mut events);
        events
    }

    fn sync(&self, downloader: &dyn Downloader, json: &Location) -> Recorded {
        self.run(downloader, &DeviceJob::Playlist { json: json.clone() })
    }

    fn staged(&self, json: &Location) -> Vec<String> {
        let folder = archive_device::staging_folder(self.staging.path(), json).unwrap();
        fs::read_dir(folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
    }
}

fn single(url: &str) -> DeviceJob {
    DeviceJob::Single { url: url.into(), format: "MP3".into(), target: World::target(), cover: None }
}

// === CREATE ===

#[test]
fn creating_registers_the_playlist_in_its_own_subfolder_without_downloading() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);

    let data = world.store.load(&created.json).unwrap();
    assert_eq!(data.device_path.as_deref(), Some(FOLDER));
    assert_eq!(data.device_id.as_deref(), Some(ID));
    assert_eq!(created.count, 2);
    assert!(yt.downloads().is_empty());
}

#[test]
fn the_same_playlist_in_two_device_folders_gets_two_archives() {
    let world = World::new();
    let other = DeviceTarget { path: "Internal storage/Podcasts".into(), ..World::target() };
    let here = device_playlist_json(&world.store, "Clásicos", &World::target(), "MP3").unwrap();
    let there = device_playlist_json(&world.store, "Clásicos", &other, "MP3").unwrap();
    assert_ne!(here, there);
}

// === PLAYLIST SYNC ===

#[test]
fn a_sync_pushes_every_file_under_its_title_and_registers_it() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);

    let events = world.sync(&yt, &created.json);

    let files: Vec<String> = world.phone.pushed().into_iter().filter(|p| p.ends_with(".mp3")).collect();
    assert_eq!(files, vec![format!("{FOLDER}/Primero.mp3"), format!("{FOLDER}/Segundo.mp3")]);
    assert_eq!(events.downloaded, vec![A, B]);
    assert!(events.keys.contains(&"deviceSync.finished".to_string()));
    assert!(events.notices.is_empty(), "a clean run tells nothing: {:?}", events.notices);
    let data = world.store.load(&created.json).unwrap();
    assert_eq!(data.entries[A].device_file_name.as_deref(), Some("Primero.mp3"));
    // The staging copy goes once the device has it.
    assert!(world.staged(&created.json).is_empty());
    // And the manifest travels with the playlist, as a local one does.
    assert!(world.phone.file(&format!("{FOLDER}/soundify-playlist.json")).is_some());
}

#[test]
fn a_second_sync_with_everything_there_downloads_nothing() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);
    world.sync(&yt, &created.json);

    let events = world.sync(&yt, &created.json);

    assert_eq!(yt.downloads().len(), 2, "no new download");
    assert_eq!(events.notices, vec![(OK, "activity.manager.upToDate".to_string())]);
}

#[test]
fn a_file_removed_by_hand_from_the_phone_is_downloaded_again() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);
    world.sync(&yt, &created.json);
    world.phone.delete(ID, &[format!("{FOLDER}/Primero.mp3")]);

    world.sync(&yt, &created.json);

    assert_eq!(yt.downloads(), vec![A, B, A]);
    assert!(world.phone.file(&format!("{FOLDER}/Primero.mp3")).is_some());
}

#[test]
fn a_numbered_playlist_prefixes_the_device_names() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, true);

    world.sync(&yt, &created.json);

    assert!(world.phone.file(&format!("{FOLDER}/001 - Primero.mp3")).is_some());
    assert!(world.phone.file(&format!("{FOLDER}/002 - Segundo.mp3")).is_some());
}

#[test]
fn an_unreachable_phone_stops_before_touching_youtube() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);
    world.phone.set_reachable(false);

    let events = world.sync(&yt, &created.json);

    assert!(yt.downloads().is_empty());
    assert_eq!(events.notices, vec![(FAILED, "deviceSync.disconnected".to_string())]);
}

#[test]
fn a_disconnect_mid_push_stops_the_run_and_keeps_the_file_for_next_time() {
    let world = World::new();
    let mut yt = FakeYtDlp::new();
    let created = world.create(&yt, false);
    let phone = world.phone.clone();
    yt.after_download = Some(Box::new(move || phone.set_reachable(false)));

    let events = world.sync(&yt, &created.json);

    assert_eq!(yt.downloads(), vec![A], "the second video is not even tried");
    assert_eq!(events.notices, vec![(FAILED, "deviceSync.disconnected".to_string())]);
    assert_eq!(world.staged(&created.json), vec![format!("{A}.mp3")]);

    // Plugged back in: the leftover goes first, with no new trip to YouTube.
    world.phone.set_reachable(true);
    yt.after_download = None;
    let events = world.sync(&yt, &created.json);
    assert!(events.keys.contains(&"deviceSync.resuming".to_string()));
    assert_eq!(yt.downloads(), vec![A, B]);
    assert!(world.phone.file(&format!("{FOLDER}/Primero.mp3")).is_some());
    assert!(world.phone.file(&format!("{FOLDER}/Segundo.mp3")).is_some());
}

#[test]
fn a_failed_push_with_the_phone_still_there_is_an_amber_summary() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let created = world.create(&yt, false);
    world.phone.fail_on("Segundo.mp3");

    let events = world.sync(&yt, &created.json);

    assert_eq!(events.downloaded, vec![A]);
    assert!(events.keys.contains(&"deviceSync.pushFailed".to_string()));
    assert_eq!(events.notices, vec![(WARNING, "deviceSync.summaryOneFailed".to_string())]);
    // Kept for the next sync, which will not ask YouTube again.
    assert_eq!(world.staged(&created.json), vec![format!("{B}.mp3")]);
}

#[test]
fn failed_downloads_are_marked_and_summed_up() {
    let world = World::new();
    let mut yt = FakeYtDlp::new();
    yt.broken = vec![A, B];
    let created = world.create(&yt, false);

    let events = world.sync(&yt, &created.json);

    assert!(world.phone.pushed().iter().all(|p| p.ends_with(".json")));
    assert_eq!(events.notices, vec![(WARNING, "deviceSync.summaryManyFailed".to_string())]);
    let data = world.store.load(&created.json).unwrap();
    assert!(data.entries[A].last_failed_at.is_some());
}

// === LOOSE DOWNLOAD ===

#[test]
fn a_loose_download_goes_straight_into_the_chosen_folder() {
    let world = World::new();
    let yt = FakeYtDlp::new();
    let job = single(&format!("https://www.youtube.com/watch?v={A}"));

    let events = world.run(&yt, &job);

    assert_eq!(world.phone.pushed(), vec![format!("{BASE}/Primero.mp3")]);
    assert_eq!(events.downloaded, vec![A]);

    // The same video again: nothing to do.
    let events = world.run(&yt, &job);
    assert_eq!(events.notices, vec![(OK, "activity.alreadyDownloaded.single".to_string())]);
    assert_eq!(yt.downloads().len(), 1);
}

#[test]
fn a_loose_download_needs_a_video_url() {
    let world = World::new();
    let yt = FakeYtDlp::new();

    let events = world.run(&yt, &single("https://example.com/nothing"));

    assert_eq!(events.notices, vec![(FAILED, "activity.error.urlNotRecognised".to_string())]);
    assert!(yt.downloads().is_empty());
}

// === NAMES ===

#[test]
fn device_names_use_the_title_and_the_number() {
    assert_eq!(nice_device_file_name(None, Some("Hola"), A, "MP3"), "Hola.mp3");
    assert_eq!(nice_device_file_name(Some("007"), Some("Hola"), A, "M4A"), "007 - Hola.m4a");
    assert_eq!(nice_device_file_name(Some(" "), Some("Hola"), A, "MP3"), "Hola.mp3");
}

#[test]
fn regression_without_a_real_title_the_name_falls_back_to_the_id() {
    assert_eq!(nice_device_file_name(None, None, A, "MP3"), format!("{A}.mp3"));
    assert_eq!(nice_device_file_name(None, Some("  "), A, "MP3"), format!("{A}.mp3"));
    assert_eq!(nice_device_file_name(None, Some(A), A, "MP3"), format!("{A}.mp3"));
}

#[test]
fn device_names_are_sanitised() {
    let expected = format!("{}.mp3", paths::sanitize_folder_name("AC/DC: Live?"));
    assert_eq!(nice_device_file_name(None, Some("AC/DC: Live?"), A, "MP3"), expected);
}
