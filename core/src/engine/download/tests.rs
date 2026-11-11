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
    /// The app has no second process to lend.
    no_extra_process: bool,
    extra_asked: u32,
    extra_given_back: u32,
}

impl DownloadEvents for Recorded {
    fn extra_process(&mut self) -> bool {
        self.extra_asked += 1;
        !self.no_extra_process
    }
    fn release_extra_process(&mut self) {
        self.extra_given_back += 1;
    }
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

    /// The user has a cookies file set in Ajustes → Avanzado.
    fn with_cookies(self, path: &str) -> Self {
        let mut settings = settings::load(self.storage.as_ref(), &self.settings_file);
        settings.cookies_file = path.to_string();
        settings::save(self.storage.as_ref(), &self.settings_file, &settings).unwrap();
        self
    }

    fn run(&self, downloader: &dyn Downloader, job: &Job) -> Recorded {
        self.run_with(downloader, job, &StopFlag::new())
    }

    fn run_with(&self, downloader: &dyn Downloader, job: &Job, stop: &StopFlag) -> Recorded {
        let mut events = Recorded::default();
        self.engine(downloader, stop).run(job, &mut events);
        events
    }

    /// A run whose events are given (to say the app cannot lend a second process).
    fn run_events(&self, downloader: &dyn Downloader, job: &Job, mut events: Recorded) -> Recorded {
        self.engine(downloader, &StopFlag::new()).run(job, &mut events);
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

/// Eleven-character ids for a long playlist: `id000000001`, `id000000002`…
fn long_id(n: usize) -> String {
    format!("id{n:09}")
}

fn bot_error(id: &str) -> String {
    format!("ERROR: [youtube] {id}: {BOT_CHECK_MARKER} you're not a bot")
}

#[test]
fn a_run_where_youtube_blocks_every_video_stops_after_a_streak_and_skips_the_second_pass() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| Some(((1..=300).map(|n| bot_error(&long_id(n))).collect(), 1))),
    );
    let entries: Vec<String> = (1..=300).map(|n| format!("{n}|{}|Título {n}", long_id(n))).collect();
    fake.set_entries(&entries.iter().map(String::as_str).collect::<Vec<_>>());

    let events = world.create_and_run(&fake);

    assert!(events.keys.contains(&"activity.botcheck.streak".to_string()));
    assert_eq!(events.notices, [(FAILED, "activity.botcheck.streakNoticeNoCookies".to_string())]);
    assert_eq!(fake.downloads().len(), 1, "no second pass over videos that fail the same way");
    assert!(!events.steps.iter().any(|(step, _)| *step == Step::Retry));
}

#[test]
fn with_cookies_set_the_bot_check_notice_says_they_may_have_expired() {
    let world = World::new(Some("web")).with_cookies("/cookies.txt");
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|_| Some(((1..=300).map(|n| bot_error(&long_id(n))).collect(), 1))),
    );
    let entries: Vec<String> = (1..=300).map(|n| format!("{n}|{}|Título {n}", long_id(n))).collect();
    fake.set_entries(&entries.iter().map(String::as_str).collect::<Vec<_>>());

    let events = world.create_and_run(&fake);

    assert_eq!(events.notices, [(FAILED, "activity.botcheck.streakNotice".to_string())]);
}

#[test]
fn a_download_in_between_resets_the_bot_check_streak() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|args| {
            if args.last().is_some_and(|u| u.contains("watch?v=")) {
                return Some((vec![], 0));
            }
            // Four blocked, one that works, four blocked: never five in a row.
            let mut lines: Vec<String> = (1..=4).map(|n| bot_error(&long_id(n))).collect();
            lines.push(seen(A, "Primero"));
            lines.push(moved(A, "/music/Clásicos (mp3)/Primero.mp3"));
            lines.extend((5..=8).map(|n| bot_error(&long_id(n))));
            Some((lines, 1))
        }),
    );
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    let events = world.create_and_run(&fake);

    assert!(!events.keys.contains(&"activity.botcheck.streak".to_string()));
}

#[test]
fn the_same_video_failing_over_and_over_counts_once() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|args| {
            if args.last().is_some_and(|u| u.contains("watch?v=")) {
                return Some((vec![], 0));
            }
            // yt-dlp prints the same error once per player client it tries.
            Some(((0..10).map(|_| bot_error(A)).collect(), 1))
        }),
    );
    fake.set_entries(&[&format!("1|{A}|Primero")]);

    let events = world.create_and_run(&fake);

    assert!(!events.keys.contains(&"activity.botcheck.streak".to_string()));
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

// === TWO PROCESSES ON ONE PLAYLIST (ajuste «Dos descargas a la vez») ===

impl World {
    /// Ajustes → Avanzado → «Dos descargas a la vez en una playlist».
    fn with_parallel(self) -> Self {
        let mut settings = settings::load(self.storage.as_ref(), &self.settings_file);
        settings.parallel_downloads = true;
        settings::save(self.storage.as_ref(), &self.settings_file, &settings).unwrap();
        self
    }
}

/// «1-3,5,9-10» → [1, 2, 3, 5, 9, 10].
fn positions(spec: &str) -> Vec<u32> {
    spec.split(',')
        .flat_map(|part| match part.split_once('-') {
            Some((a, b)) => (a.parse::<u32>().unwrap()..=b.parse::<u32>().unwrap()).collect::<Vec<_>>(),
            None => vec![part.parse::<u32>().unwrap()],
        })
        .collect()
}

fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).map(|at| args[at + 1].as_str())
}

fn is_retry(args: &[String]) -> bool {
    args.last().is_some_and(|u| u.contains("watch?v="))
}

fn numbered_playlist(fake: &FakeYtDlp, count: u32) {
    let entries: Vec<String> = (1..=count).map(|n| format!("{n}|{}|Canción {n}", long_id(n as usize))).collect();
    fake.set_entries(&entries.iter().map(String::as_str).collect::<Vec<_>>());
}

/// The playlist download of the real yt-dlp: it takes the positions it is asked for, numbers
/// them from `--autonumber-start` and brings down every one except those in `skip`. A retry of
/// a single video (`watch?v=`) fails, so what is skipped stays pending.
fn serves_positions(skip: &'static [u32]) -> DownloadReply {
    Box::new(move |args| {
        if is_retry(args) {
            return Some((vec![], 1));
        }
        let first: u32 = after(args, "--autonumber-start").and_then(|n| n.parse().ok()).unwrap_or(1);
        let asked = after(args, "--playlist-items").map(positions).unwrap_or_else(|| (1..=100).collect());
        let mut lines = Vec::new();
        for (k, position) in asked.iter().enumerate() {
            if skip.contains(position) {
                continue;
            }
            let (id, title) = (long_id(*position as usize), format!("Canción {position}"));
            lines.push(seen(&id, &title));
            lines.push(moved(&id, &format!("/music/Clásicos (mp3)/{:03} - {title}.mp3", first + k as u32)));
        }
        Some((lines, 0))
    })
}

fn main_downloads(fake: &FakeYtDlp) -> Vec<Vec<String>> {
    fake.downloads().into_iter().filter(|d| !is_retry(d)).collect()
}

#[test]
fn off_by_default_one_process_takes_the_whole_playlist() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);

    let events = world.create_and_run(&fake);

    assert_eq!(fake.downloads().len(), 1);
    assert!(!events.keys.contains(&"activity.workers.two".to_string()));
}

#[test]
fn two_processes_split_the_pending_items_with_nothing_in_common() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 5);

    let events = world.create_and_run(&fake);

    let downloads = fake.downloads();
    assert_eq!(downloads.len(), 2, "one request per process");
    let mut all: Vec<u32> = Vec::new();
    for request in &downloads {
        all.extend(positions(after(request, "--playlist-items").expect("each one is told its own items")));
    }
    let own: HashSet<u32> = all.iter().copied().collect();
    assert_eq!(all.len(), own.len(), "no position asked for twice: {all:?}");
    assert_eq!(own, (1..=5).collect::<HashSet<u32>>(), "together, everything that was pending");
    assert!(events.keys.contains(&"activity.workers.two".to_string()));
    assert_eq!(events.steps, [(Step::Download, 5)], "the progress counts the whole run, not each process");
    // A single commit leaves every file registered in the archive.
    let json = world.playlist_json();
    let ids: Vec<String> = (1..=5).map(long_id).collect();
    assert!(world.store.ids_pending_download(&json, &ids).unwrap().is_empty());
    let mut told = events.downloaded.clone();
    told.sort();
    assert_eq!(told, ids, "each file told once");
}

#[test]
fn the_second_process_numbers_on_from_where_the_first_one_ends() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 5);

    world.create_and_run(&fake);

    // The processes start on their own threads: which request is logged first is not fixed.
    let mut downloads = fake.downloads();
    downloads.sort_by_key(|d| after(d, "--autonumber-start").unwrap().parse::<u32>().unwrap());
    let count = positions(after(&downloads[0], "--playlist-items").unwrap()).len() as u32;
    assert_eq!(after(&downloads[0], "--autonumber-start"), Some("1"));
    assert_eq!(after(&downloads[1], "--autonumber-start"), Some((count + 1).to_string().as_str()), "the numbers of the original order");
    // The files carry those numbers: 001..005, each exactly once.
    let files: Vec<String> = (1..=5).map(|n| format!("/music/Clásicos (mp3)/{n:03} - Canción {n}.mp3")).collect();
    for file in files {
        assert!(world.storage.exists(&Location::path(&file)).unwrap(), "{file}");
    }
}

#[test]
fn the_second_process_works_on_a_copy_of_the_work_file_that_is_removed_afterwards() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);

    world.create_and_run(&fake);

    let mut files: Vec<String> = fake.downloads().iter().map(|d| after(d, "--download-archive").unwrap().to_string()).collect();
    files.sort();
    let (copy, own) = (files[0].clone(), files[1].clone()); // "archive.lane2.txt" sorts before "archive.txt"
    assert_ne!(own, copy, "two processes never write to the same file");
    assert!(own.ends_with("archive.txt"), "{own}");
    assert!(copy.ends_with("archive.lane2.txt"), "{copy}");
    assert!(!world.storage.exists(&Location::parse(&copy)).unwrap(), "the copy does not stay behind");
}

#[test]
fn the_second_process_gets_its_own_copy_of_the_cookies() {
    let world = World::new(Some("web")).with_cookies("/cookies.txt").with_parallel();
    world.storage.write(&Location::path("/cookies.txt"), b"# cookies").unwrap();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);

    world.create_and_run(&fake);

    // The processes start on their own threads: which request is logged first is not fixed.
    let mut cookies: Vec<String> = fake.downloads().iter().map(|d| after(d, "--cookies").unwrap().to_string()).collect();
    cookies.sort();
    assert_eq!(cookies.len(), 2);
    assert_eq!(cookies[1], "/cookies.txt");
    let copy = cookies[0].as_str();
    assert!(copy.ends_with("cookies.lane2.txt"), "{copy}");
    assert!(!world.storage.exists(&Location::parse(copy)).unwrap());
    assert!(world.storage.exists(&Location::path("/cookies.txt")).unwrap(), "the user's file is never touched");
}

#[test]
fn a_single_pending_item_is_not_worth_a_second_process() {
    let world = World::new(Some("web")).with_parallel();
    let before = FakeYtDlp::new(world.storage.clone(), serves_positions(&[3]));
    numbered_playlist(&before, 3);
    world.create_and_run(&before);

    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 3);
    let job = Job::update(&world.store, &world.playlist_json()).unwrap();
    let events = world.run(&fake, &job);

    let downloads = main_downloads(&fake);
    assert_eq!(downloads.len(), 1);
    assert_eq!(after(&downloads[0], "--playlist-items"), Some("3"));
    assert!(!events.keys.contains(&"activity.workers.two".to_string()));
}

#[test]
fn a_resumed_run_asks_only_for_what_is_still_pending() {
    let world = World::new(Some("web")).with_parallel();
    // Positions 2 and 4 do not come down the first time.
    let before = FakeYtDlp::new(world.storage.clone(), serves_positions(&[2, 4]));
    numbered_playlist(&before, 4);
    world.create_and_run(&before);
    let json = world.playlist_json();
    let ids: Vec<String> = (1..=4).map(long_id).collect();
    assert_eq!(world.store.ids_pending_download(&json, &ids).unwrap(), [long_id(2), long_id(4)]);

    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);
    let job = Job::update(&world.store, &json).unwrap();
    let events = world.run(&fake, &job);

    let mut asked: Vec<u32> = Vec::new();
    for request in main_downloads(&fake) {
        asked.extend(positions(after(&request, "--playlist-items").unwrap()));
    }
    asked.sort_unstable();
    assert_eq!(asked, [2, 4], "what was already down is not asked for again");
    assert_eq!(events.steps.first(), Some(&(Step::Download, 2)));
    assert!(world.store.ids_pending_download(&json, &ids).unwrap().is_empty());
}

#[test]
fn stopping_stops_both_processes_and_keeps_the_archive() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), Box::new(|_| None));
    numbered_playlist(&fake, 4);

    let events = world.create_and_run(&fake);

    assert_eq!(fake.downloads().len(), 2, "both were launched");
    assert!(events.keys.contains(&"activity.stop.download".to_string()));
    assert!(!events.keys.contains(&"activity.sync.done".to_string()));
    assert!(!events.steps.iter().any(|(step, _)| *step == Step::Retry), "no second pass after a stop");
    let ids: Vec<String> = (1..=4).map(long_id).collect();
    assert_eq!(world.store.ids_pending_download(&world.playlist_json(), &ids).unwrap().len(), 4, "still registered and pending");
}

#[test]
fn a_429_in_the_second_process_leaves_its_items_for_the_second_pass() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(
        world.storage.clone(),
        Box::new(|args| {
            if is_retry(args) {
                let id = args.last().unwrap().rsplit('=').next().unwrap().to_string();
                return Some((vec![seen(&id, "Rescatada"), moved(&id, &format!("/music/Clásicos (mp3)/{id}.mp3"))], 0));
            }
            // The second process is told to slow down before getting anything.
            if after(args, "--download-archive").is_some_and(|f| f.ends_with("archive.lane2.txt")) {
                return Some((vec!["ERROR: unable to download video data: HTTP Error 429: Too Many Requests".into()], 1));
            }
            serves_positions(&[])(args)
        }),
    );
    numbered_playlist(&fake, 4);

    let events = world.create_and_run(&fake);

    assert!(events.keys.contains(&"activity.workers.backoff".to_string()));
    assert!(events.steps.contains(&(Step::Retry, 2)), "the two items of the slowed-down process: {:?}", events.steps);
    let ids: Vec<String> = (1..=4).map(long_id).collect();
    assert!(world.store.ids_pending_download(&world.playlist_json(), &ids).unwrap().is_empty());
}

#[test]
fn a_single_download_never_uses_two_processes() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), Box::new(|_| Some((vec![seen(A, "Canción"), moved(A, "/music/Canción.mp3")], 0))));

    let events = world.run(&fake, &single(&format!("https://www.youtube.com/watch?v={A}")));

    assert_eq!(fake.downloads().len(), 1);
    assert!(!events.keys.contains(&"activity.workers.two".to_string()));
}

// === THE APP'S PROCESS LIMIT (several downloads at once) ===

#[test]
fn the_second_process_is_asked_for_and_given_back() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);

    let events = world.create_and_run(&fake);

    assert_eq!(fake.downloads().len(), 2);
    assert_eq!((events.extra_asked, events.extra_given_back), (1, 1), "borrowed once, returned once");
}

#[test]
fn without_a_second_process_to_spare_the_playlist_goes_on_with_one() {
    let world = World::new(Some("web")).with_parallel();
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);
    let created = world.create(&fake, true).expect("the fake lists the playlist");
    let job = Job::update(&world.store, &created.json).unwrap();

    let events = world.run_events(&fake, &job, Recorded { no_extra_process: true, ..Recorded::default() });

    let downloads = main_downloads(&fake);
    assert_eq!(downloads.len(), 1, "one process takes everything");
    assert_eq!(after(&downloads[0], "--playlist-items"), None, "the whole playlist, not a share of it");
    assert!(!events.keys.contains(&"activity.workers.two".to_string()));
    assert_eq!((events.extra_asked, events.extra_given_back), (1, 0), "nothing to give back");
    let ids: Vec<String> = (1..=4).map(long_id).collect();
    assert!(world.store.ids_pending_download(&world.playlist_json(), &ids).unwrap().is_empty());
}

#[test]
fn a_playlist_that_does_not_share_does_not_ask_for_the_second_process() {
    let world = World::new(Some("web"));
    let fake = FakeYtDlp::new(world.storage.clone(), serves_positions(&[]));
    numbered_playlist(&fake, 4);

    let events = world.create_and_run(&fake);

    assert_eq!(events.extra_asked, 0, "with the setting off the limit never comes into it");
}
