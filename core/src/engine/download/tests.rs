use super::*;
use crate::archive::NumberModeSource;
use crate::clock;
use crate::storage::{MemoryStorage, Storage};
use crate::task_history::{FAILED, OK, WARNING};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=PL1";
const A: &str = "aaaaaaaaaaa";
const B: &str = "bbbbbbbbbbb";
const C: &str = "ccccccccccc";

/// What the fake yt-dlp's real download answers: its lines and exit code, or
/// None to behave as if «Detener» was pressed mid-run.
type DownloadReply = Box<dyn Fn(&[String]) -> Option<(Vec<String>, i32)> + Send + Sync>;

/// A yt-dlp that answers by the arguments it gets, like the real one: each
/// quick query has its own recognisable shape. Every `MOVED` line it prints
/// also leaves the file on disk, as the real move would.
struct FakeYtDlp {
    storage: Arc<MemoryStorage>,
    title: Option<String>,
    entries: Mutex<Vec<String>>,
    working_clients: Vec<&'static str>,
    unavailable: Vec<(&'static str, &'static str)>,
    download: DownloadReply,
    requests: Mutex<Vec<Vec<String>>>,
}

impl FakeYtDlp {
    fn new(storage: Arc<MemoryStorage>, download: DownloadReply) -> Self {
        Self {
            storage,
            title: Some("Clásicos".into()),
            entries: Mutex::new(Vec::new()),
            working_clients: Vec::new(),
            unavailable: Vec::new(),
            download,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn set_entries(&self, lines: &[&str]) {
        *self.entries.lock().unwrap() = lines.iter().map(|l| l.to_string()).collect();
    }

    fn downloads(&self) -> Vec<Vec<String>> {
        self.requests.lock().unwrap().iter().filter(|a| is_download(a)).cloned().collect()
    }

    fn answer(&self, args: &[String]) -> Option<(Vec<String>, i32)> {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        if has("playlist_title") {
            return Some((self.title.iter().cloned().collect(), 0));
        }
        if has("%(playlist_index)s|%(id)s|%(title)s") {
            return Some((self.entries.lock().unwrap().clone(), 0));
        }
        if has("--simulate") && has("--extractor-args") {
            let ok = self.working_clients.iter().any(|c| args.iter().any(|a| a.ends_with(&format!("={c}"))));
            return Some(if ok { (vec![], 0) } else { (vec![format!("ERROR: {BOT_CHECK_MARKER} you're not a bot")], 1) });
        }
        if has("--simulate") {
            let url = args.last().cloned().unwrap_or_default();
            return Some(match self.unavailable.iter().find(|(id, _)| url.ends_with(id)) {
                Some((id, error)) => (vec![format!("ERROR: [youtube] {id}: {error}")], 1),
                None => (vec!["A title".into()], 0),
            });
        }
        (self.download)(args)
    }
}

fn is_download(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--simulate" || a == "--flat-playlist")
}

impl Downloader for FakeYtDlp {
    fn run(&self, request: &YtDlpRequest, stop: &StopFlag, on_line: &mut dyn FnMut(OutputLine)) -> io::Result<RunEnd> {
        self.requests.lock().unwrap().push(request.args.clone());
        let Some((lines, exit_code)) = self.answer(&request.args) else {
            stop.stop();
            return Ok(RunEnd::Stopped);
        };
        for line in lines {
            if let Some((_, path)) = line.strip_prefix(MOVED_PREFIX).and_then(|p| p.split_once('|')) {
                self.storage.write(&Location::parse(path), b"x")?;
            }
            on_line(OutputLine::Stdout(line));
        }
        Ok(RunEnd::Finished { exit_code })
    }
}

fn moved(id: &str, path: &str) -> String {
    format!("{MOVED_PREFIX}{id}|{path}")
}

fn seen(id: &str, title: &str) -> String {
    format!("{SEEN_PREFIX}{id}|{title}")
}

#[derive(Default)]
struct Recorded {
    keys: Vec<String>,
    titles: Vec<String>,
    steps: Vec<(Step, u32)>,
    notices: Vec<(&'static str, String)>,
    downloaded: Vec<String>,
}

impl DownloadEvents for Recorded {
    fn line(&mut self, line: ActivityLine) {
        if let ActivityLine::Message { key, .. } = line {
            self.keys.push(key);
        }
    }
    fn title(&mut self, title: &str) {
        self.titles.push(title.to_string());
    }
    fn archive(&mut self, _: &Location) {}
    fn step(&mut self, step: Step, total: u32) {
        self.steps.push((step, total));
    }
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
    storage: Arc<MemoryStorage>,
    store: ArchiveStore,
    settings_file: Location,
    scratch: tempfile::TempDir,
}

impl World {
    /// `client` is the one already cached from an earlier run (None: probe).
    fn new(client: Option<&str>) -> Self {
        let storage = Arc::new(MemoryStorage::new());
        let store = ArchiveStore::new(storage.clone(), Location::path("/archives"), clock::fixed("2026-11-03 10:00"));
        let settings_file = Location::path("/config/settings.json");
        let settings = UserSettings { working_player_client: client.map(str::to_string), ..UserSettings::default() };
        settings::save(storage.as_ref(), &settings_file, &settings).unwrap();
        Self { storage, store, settings_file, scratch: tempfile::tempdir().unwrap() }
    }

    fn run(&self, downloader: &dyn Downloader, job: &Job) -> Recorded {
        self.run_with(downloader, job, &StopFlag::new())
    }

    fn run_with(&self, downloader: &dyn Downloader, job: &Job, stop: &StopFlag) -> Recorded {
        let mut events = Recorded::default();
        self.engine(downloader, stop).run(job, &mut events);
        events
    }

    fn create(&self, downloader: &dyn Downloader, numbered: bool) -> Result<Created, CreateError> {
        self.engine(downloader, &StopFlag::new()).create_playlist(PLAYLIST_URL, "MP3", &base(), numbered)
    }

    /// «Crear y descargar»: registered first, then a normal update.
    fn create_and_run(&self, downloader: &dyn Downloader) -> Recorded {
        let created = self.create(downloader, true).expect("the fake lists the playlist");
        let job = Job::update(&self.store, &created.json).expect("registered with url and folder");
        self.run(downloader, &job)
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

    fn client(&self) -> Option<String> {
        settings::load(self.storage.as_ref(), &self.settings_file).working_player_client
    }

    fn playlist_json(&self) -> Location {
        let dir = paths::playlist_archive_dir(self.storage.as_ref(), &Location::path("/archives"), "Clásicos", &base(), "MP3");
        self.storage.child(&dir.unwrap(), archive_io::DATA_JSON).unwrap()
    }
}

fn base() -> Location {
    Location::path("/music")
}

fn single(url: &str) -> Job {
    Job::Single { url: url.into(), format: "MP3".into(), base: base(), cover: None }
}

/// The real download of a playlist: every id asked for comes down, except
/// the ones listed in `skip`.
fn downloads_all_but(skip: &'static [&'static str]) -> DownloadReply {
    Box::new(move |args| {
        let mut lines = Vec::new();
        for (id, title) in [(A, "Primero"), (B, "Segundo"), (C, "Tercero")] {
            if skip.contains(&id) || args.last().is_some_and(|u| u.contains("watch?v=") && !u.ends_with(id)) {
                continue;
            }
            lines.push(seen(id, title));
            lines.push(moved(id, &format!("/music/Clásicos (mp3)/{title}.mp3")));
        }
        Some((lines, 0))
    })
}

// === SINGLE DOWNLOADS ===

#[test]
fn a_single_download_registers_its_file_and_takes_the_real_title() {
    let world = World::new(Some("web"));
    let url = format!("https://www.youtube.com/watch?v={A}");
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| Some((vec![seen(A, "Canción"), moved(A, "/music/Canción.mp3")], 0))),
    );

    let events = world.run(&fake, &single(&url));

    let json = paths::individual_archive_json(world.storage.as_ref(), &Location::path("/archives"), "MP3").unwrap();
    assert_eq!(world.store.known_ids(&json).unwrap(), [A]);
    assert_eq!(events.titles, ["Canción"]);
    assert_eq!(events.steps, [(Step::Download, 1)]);
    assert!(events.notices.is_empty(), "nothing to warn about: {:?}", events.notices);
}

#[test]
fn a_single_download_goes_straight_into_the_folder_picked_and_leaves_its_files_alone() {
    let world = World::new(Some("web"));
    world.storage.create_dir_all(&base()).unwrap();
    world.storage.write(&Location::path("/music/holidays.jpg"), b"x").unwrap();
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| {
            Some((vec![seen(A, "Canción"), "[ExtractAudio] /music/Canción.webp".into(), moved(A, "/music/Canción.mp3")], 0))
        }),
    );
    world.storage.write(&Location::path("/music/Canción.webp"), b"x").unwrap();

    world.run(&fake, &single(&format!("https://www.youtube.com/watch?v={A}")));

    let download = &fake.downloads()[0];
    let output = &download[download.iter().position(|a| a == "-o").unwrap() + 1];
    assert!(!output.contains("soundify_"), "no folder of its own: {output}");
    assert!(!output.contains(" - "), "a loose file is never numbered: {output}");
    assert!(world.storage.exists(&Location::path("/music/Canción.mp3")).unwrap());
    assert!(!world.storage.exists(&Location::path("/music/Canción.webp")).unwrap(), "its own leftover goes");
    assert!(world.storage.exists(&Location::path("/music/holidays.jpg")).unwrap(), "the user's picture stays");
}

#[test]
fn a_single_video_already_archived_is_said_instead_of_downloaded_again() {
    let world = World::new(Some("web"));
    let url = format!("https://www.youtube.com/watch?v={A}");
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| Some((vec![moved(A, "/music/Canción.mp3")], 0))),
    );
    world.run(&fake, &single(&url));

    let events = world.run(&fake, &single(&url));

    assert_eq!(fake.downloads().len(), 1, "the second run never reached yt-dlp");
    assert_eq!(events.notices, [(OK, "activity.alreadyDownloaded.single".to_string())]);
}

#[test]
fn an_empty_url_fails_before_doing_anything() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));

    let events = world.run(&fake, &single("  "));

    assert!(fake.requests.lock().unwrap().is_empty());
    assert_eq!(events.notices, [(FAILED, "activity.error.emptyInput".to_string())]);
}

// === PLAYLISTS ===

#[test]
fn a_new_playlist_is_registered_downloaded_and_resynced() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|Segundo")]);

    let events = world.create_and_run(&fake);

    let json = world.playlist_json();
    assert!(world.store.is_numbered(&json), "registered with its numbering");
    let ids = [A.to_string(), B.to_string()];
    assert!(world.store.ids_pending_download(&json, &ids).unwrap().is_empty());
    assert_eq!(events.titles, ["Clásicos"]);
    assert_eq!(events.steps, [(Step::Download, 2)]);
    assert!(events.downloaded.starts_with(&[A.to_string(), B.to_string()]), "each file is told as soon as it is in the archive");
    assert!(events.keys.contains(&"activity.sync.done".to_string()));
    let download = &fake.downloads()[0];
    assert!(!download.iter().any(|a| a == "--playlist-items"), "the whole playlist was pending");
}

#[test]
fn an_update_with_nothing_new_says_it_is_up_to_date() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|Segundo")]);
    world.create_and_run(&fake);

    let job = Job::update(&world.store, &world.playlist_json()).expect("the archive has url and folder");
    let events = world.run(&fake, &job);

    assert_eq!(fake.downloads().len(), 1);
    assert_eq!(events.notices, [(OK, "activity.manager.upToDate".to_string())]);
}

#[test]
fn only_the_missing_items_are_asked_for_by_position() {
    let world = World::new(Some("web"));
    let before = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[C]));
    before.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|Segundo")]);
    world.create_and_run(&before);

    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{C}|Tercero"), &format!("2|{A}|Primero"), &format!("3|{B}|Segundo")]);
    let job = Job::update(&world.store, &world.playlist_json()).unwrap();
    let events = world.run(&fake, &job);

    let download = &fake.downloads()[0];
    let at = download.iter().position(|a| a == "--playlist-items").expect("a partial download");
    assert_eq!(download[at + 1], "1");
    assert!(events.keys.contains(&"activity.pending.partial".to_string()));
    assert_eq!(events.steps, [(Step::Download, 1)]);
}

#[test]
fn a_failed_check_from_the_manager_stops_before_downloading() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero")]);
    world.create(&fake, true).unwrap();
    fake.set_entries(&[]);

    let job = Job::update(&world.store, &world.playlist_json()).unwrap();
    let events = world.run(&fake, &job);

    assert!(fake.downloads().is_empty());
    assert_eq!(events.notices, [(FAILED, "activity.manager.checkFailed".to_string())]);
}

#[test]
fn an_untitled_entry_that_is_really_gone_is_marked_and_not_pending() {
    let world = World::new(Some("web"));
    let mut fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[B]));
    fake.unavailable = vec![(B, "Video unavailable")];
    fake.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|NA")]);

    let events = world.create_and_run(&fake);

    let json = world.playlist_json();
    assert!(world.store.is_unavailable(&json, B));
    assert!(events.keys.contains(&"activity.unavailable.marked".to_string()));
    assert_eq!(events.steps, [(Step::Download, 1)], "B no longer counts as pending");
    assert!(!events.keys.contains(&"activity.retry.starting".to_string()), "nor is it retried");
}

// === THE PLAYER CLIENT ===

#[test]
fn with_no_cached_client_the_first_that_works_is_used_and_saved() {
    let world = World::new(None);
    let mut fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.working_clients = vec!["web"];
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    world.create_and_run(&fake);

    assert_eq!(world.client().as_deref(), Some("web"));
    assert!(fake.downloads()[0].iter().any(|a| a == "youtube:player_client=web"));
}

#[test]
fn when_every_client_is_blocked_nothing_is_downloaded() {
    let world = World::new(None);
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    let events = world.create_and_run(&fake);

    assert!(fake.downloads().is_empty());
    assert_eq!(events.notices, [(FAILED, "activity.botcheck.notice".to_string())]);
    assert_eq!(world.client(), None);
}

#[test]
fn a_bot_check_mid_download_forgets_the_cached_client() {
    let world = World::new(Some("web"));
    let url = format!("https://www.youtube.com/watch?v={A}");
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| Some((vec![format!("ERROR: [youtube] {A}: {BOT_CHECK_MARKER} you're not a bot")], 1))),
    );

    let events = world.run(&fake, &single(&url));

    assert_eq!(world.client(), None);
    assert_eq!(events.keys.iter().filter(|k| *k == "activity.client.stoppedWorking").count(), 1);
}

// === STOP AND SECOND PASS ===

#[test]
fn a_stopped_download_keeps_what_came_down_and_does_not_resync() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), Box::new(|_| None));
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    let events = world.create_and_run(&fake);

    assert!(events.keys.contains(&"activity.stop.download".to_string()));
    assert!(!events.keys.contains(&"activity.sync.done".to_string()));
    assert!(events.notices.is_empty());
}

#[test]
fn what_the_normal_run_missed_goes_to_the_second_pass() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|args| {
            let url = args.last().cloned().unwrap_or_default();
            if url.ends_with(B) {
                // The retry of B: yt-dlp refuses it every time.
                return Some((vec![format!("ERROR: [youtube] {B}: Requested format is not available")], 1));
            }
            Some((vec![seen(A, "Primero"), moved(A, "/music/Clásicos (mp3)/Primero.mp3")], 1))
        }),
    );
    fake.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|Segundo")]);

    let events = world.create_and_run(&fake);

    assert!(events.steps.contains(&(Step::Retry, 1)));
    assert_eq!(events.notices, [(WARNING, "activity.retry.summaryOne".to_string())]);
    assert!(events.keys.contains(&"activity.retry.failedOne".to_string()));
}

// === CREAR PLAYLIST ===

#[test]
fn creating_a_playlist_registers_it_without_downloading_anything() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero"), &format!("2|{B}|Segundo")]);

    let created = world.create(&fake, false).unwrap();

    assert_eq!(created.json, world.playlist_json());
    assert_eq!((created.title.as_str(), created.count, created.generic_title), ("Clásicos", 2, false));
    assert_eq!(created.number_mode.source, NumberModeSource::Requested);
    assert!(fake.downloads().is_empty(), "«Crear» alone downloads nothing");
    assert!(world.storage.exists(&Location::path("/music/Clásicos (mp3)")).unwrap());
    assert!(!world.store.is_numbered(&created.json));
    let ids = [A.to_string(), B.to_string()];
    assert_eq!(world.store.ids_pending_download(&created.json, &ids).unwrap(), ids, "both still to download");
    assert!(world.store.list_local_playlists().iter().any(|p| p.title == "Clásicos"), "already in the Gestor");
}

#[test]
fn without_a_title_the_playlist_gets_the_generic_one() {
    let world = World::new(Some("web"));
    let mut fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.title = None;
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    let created = world.create(&fake, true).unwrap();

    assert_eq!(created.title, "soundify_playlist_mp3");
    assert!(created.generic_title);
}

#[test]
fn a_playlist_that_cannot_be_listed_is_not_registered() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));

    assert!(matches!(world.create(&fake, true), Err(CreateError::QueryFailed)));
    assert!(world.store.list_local_playlists().is_empty());
}

#[test]
fn creating_it_again_keeps_the_numbering_it_was_given_first() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), downloads_all_but(&[]));
    fake.set_entries(&[&format!("1|{A}|Primero")]);
    world.create(&fake, true).unwrap();

    let again = world.create(&fake, false).unwrap();

    assert_eq!(again.number_mode.source, NumberModeSource::SameFormat);
    assert!(again.number_mode.number_files, "immutable once fixed");
}
