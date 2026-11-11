//! One playlist, two yt-dlp processes («lanes»). Each lane gets its own half of the
//! pending items (`--playlist-items`), its own numbering start (`--autonumber-start`)
//! and its own copy of the work file (`--download-archive`); everything they print
//! comes back through ONE channel to the calling thread, which is the only one that
//! registers files, tells the UI and decides to stop. So the archive sees a single
//! writer, as in a sequential run.
//!
//! What this file does not do: decide anything about the archive or the UI. It splits
//! work, runs the processes and keeps the progress of all of them in one number.

use std::collections::HashSet;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::downloader::{Downloader, OutputLine, RunEnd, StopFlag, YtDlpRequest};
use crate::ytdlp::command;
use crate::ytdlp::progress::ProgressParser;

/// The most yt-dlp processes one playlist ever runs at once. Every one is another
/// stream of requests to YouTube, so this is a hard cap, whatever the settings say.
pub const MAX_WORKERS: usize = 2;

/// Fewer pending items than this are not worth a second process.
pub const MIN_ITEMS_TO_SPLIT: usize = 2;

/// What Ajustes' switch means: off = one process, on = [`MAX_WORKERS`].
pub fn workers_for(enabled: bool) -> usize {
    if enabled { MAX_WORKERS } else { 1 }
}

/// What a yt-dlp 429 line looks like: YouTube asking to go slower.
pub fn is_rate_limited(line: &str) -> bool {
    line.contains("HTTP Error 429")
}

// === SPLITTING ===

/// The part of the playlist one lane downloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// Positions in the playlist (1-based, ascending).
    pub positions: Vec<u32>,
    /// The last number already used before this share (`--autonumber-start` is this + 1).
    pub offset: u32,
}

/// A pending item: where it is in the playlist and what it will be called on disk.
#[derive(Debug, Clone)]
pub struct Item {
    pub position: u32,
    /// Two items with the same key would write the same file name.
    pub name_key: String,
}

impl Item {
    pub fn new(position: u32, title: Option<&str>, id: &str) -> Self {
        let cleaned = title.map(command::cleaned_title).filter(|t| !t.is_empty());
        Self { position, name_key: cleaned.unwrap_or_else(|| id.to_string()) }
    }
}

/// Splits `items` between up to `workers` lanes.
///
/// - A **numbered** playlist is cut into contiguous halves by position: each lane
///   numbers from where the previous one's block ends, so the numbers keep the
///   playlist's order (a failure leaves a gap in them, nothing else).
/// - An **unnumbered** one has no order to keep, but two videos with the same title
///   would write the same file at the same time: those go to the same lane, and the
///   groups are dealt to the lane with fewer items.
///
/// One share (or none) comes back when there is nothing worth splitting.
pub fn split(items: &[Item], offset: u32, numbered: bool, workers: usize) -> Vec<Share> {
    let workers = workers.clamp(1, MAX_WORKERS);
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by_key(|i| i.position);
    sorted.dedup_by_key(|i| i.position);
    if workers == 1 || sorted.len() < MIN_ITEMS_TO_SPLIT {
        let positions: Vec<u32> = sorted.iter().map(|i| i.position).collect();
        return if positions.is_empty() { Vec::new() } else { vec![Share { positions, offset }] };
    }

    let mut lanes: Vec<Vec<u32>> = vec![Vec::new(); workers];
    if numbered {
        let per = sorted.len().div_ceil(workers);
        for (i, chunk) in sorted.chunks(per).enumerate() {
            lanes[i] = chunk.iter().map(|item| item.position).collect();
        }
    } else {
        let mut by_key: Vec<(&str, Vec<u32>)> = Vec::new();
        for item in &sorted {
            match by_key.iter_mut().find(|(key, _)| *key == item.name_key) {
                Some((_, positions)) => positions.push(item.position),
                None => by_key.push((&item.name_key, vec![item.position])),
            }
        }
        for (_, positions) in by_key {
            let lighter = (0..workers).min_by_key(|&i| lanes[i].len()).unwrap_or(0);
            lanes[lighter].extend(positions);
        }
        for lane in &mut lanes {
            lane.sort_unstable();
        }
    }

    let mut next_offset = offset;
    let mut shares = Vec::new();
    for positions in lanes.into_iter().filter(|p| !p.is_empty()) {
        let count = u32::try_from(positions.len()).unwrap_or(u32::MAX);
        shares.push(Share { positions, offset: next_offset });
        if numbered {
            next_offset = next_offset.saturating_add(count);
        }
    }
    shares
}

/// `[1,2,3,5,9,10]` -> `"1-3,5,9-10"`: what `--playlist-items` takes, short even for 700 items.
pub fn item_ranges(positions: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < positions.len() {
        let start = positions[i];
        let mut end = start;
        while i + 1 < positions.len() && positions[i + 1] == end + 1 {
            i += 1;
            end = positions[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}-{end}") });
        i += 1;
    }
    parts.join(",")
}

// === PROGRESS OF ALL THE LANES AS ONE NUMBER ===

struct LaneProgress {
    parser: ProgressParser,
    /// Ids yt-dlp gave up on in this lane: they do not advance the bar.
    failed: HashSet<String>,
    /// How many items this lane has to bring.
    items: usize,
    /// How far it is (0..=1).
    fraction: f64,
}

pub struct Progress {
    lanes: Vec<LaneProgress>,
    /// Items on disk before this run: the bar picks up where the playlist was.
    already_done: usize,
    /// Whole playlist, 0 when the run is not bounded (yt-dlp's own total is the right one then).
    total: usize,
}

impl Progress {
    /// A run whose total is not known beforehand (the playlist check failed).
    pub fn unbounded() -> Self {
        Self { lanes: vec![LaneProgress::new(ProgressParser::new(), 0)], already_done: 0, total: 0 }
    }

    /// `items[i]` is what lane `i` brings, `already_done` what was on disk before.
    pub fn bounded(already_done: usize, items: &[usize]) -> Self {
        let lanes = items
            .iter()
            .map(|&n| {
                let mut parser = ProgressParser::new();
                parser.lock_total_items(0, n as i64);
                LaneProgress::new(parser, n)
            })
            .collect();
        Self { lanes, already_done, total: already_done + items.iter().sum::<usize>() }
    }

    pub fn failed(&mut self, lane: usize, id: &str) {
        self.lanes[lane].failed.insert(id.to_string());
    }

    pub fn recovered(&mut self, lane: usize, id: &str) {
        self.lanes[lane].failed.remove(id);
    }

    /// The overall progress (0..=1) this line of `lane` moves the download to, if it does.
    pub fn parse(&mut self, lane: usize, line: &str) -> Option<f64> {
        let l = &mut self.lanes[lane];
        l.parser.set_skipped(l.failed.len());
        l.fraction = l.parser.parse(line)?;
        Some(self.overall())
    }

    fn overall(&self) -> f64 {
        if self.total == 0 {
            return self.lanes[0].fraction;
        }
        let done: f64 = self.already_done as f64 + self.lanes.iter().map(|l| l.fraction * l.items as f64).sum::<f64>();
        (done / self.total as f64).clamp(0.0, 1.0)
    }
}

impl LaneProgress {
    fn new(parser: ProgressParser, items: usize) -> Self {
        Self { parser, failed: HashSet::new(), items, fraction: 0.0 }
    }
}

// === RUNNING ===

/// How often a waiting run looks at the stop button.
const POLL: Duration = Duration::from_millis(100);

/// Runs the requests, one process each. `on_line(lane, line)` is called on THIS thread for
/// every line of any of them, in the order they arrive; if it returns a lane, that process
/// (and only that one) is stopped. A stop of `stop` stops all.
///
/// With a single request nothing is spawned and `stop` goes straight to the process.
pub fn run(
    downloader: &dyn Downloader,
    stop: &StopFlag,
    requests: &[YtDlpRequest],
    on_line: &mut dyn FnMut(usize, OutputLine) -> Option<usize>,
) -> Vec<io::Result<RunEnd>> {
    if let [request] = requests {
        return vec![downloader.run(request, stop, &mut |line| {
            on_line(0, line);
        })];
    }
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(usize, OutputLine)>();
        let flags: Vec<StopFlag> = requests.iter().map(|_| StopFlag::new()).collect();
        let workers: Vec<_> = requests
            .iter()
            .enumerate()
            .map(|(lane, request)| {
                let (tx, all) = (tx.clone(), flags.clone());
                scope.spawn(move || {
                    let result = downloader.run(request, &all[lane], &mut |line| {
                        let _ = tx.send((lane, line));
                    });
                    // A process that could not even run: the others stop too, so the error
                    // is not found after a whole share has been downloaded for nothing.
                    if result.is_err() {
                        all.iter().for_each(StopFlag::stop);
                    }
                    result
                })
            })
            .collect();
        drop(tx);
        loop {
            match rx.recv_timeout(POLL) {
                Ok((lane, line)) => {
                    if let Some(to_stop) = on_line(lane, line) {
                        flags[to_stop].stop();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if stop.is_stopped() {
                flags.iter().for_each(StopFlag::stop);
            }
        }
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_else(|_| Err(io::Error::other("a download worker panicked"))))
            .collect()
    })
}

/// How the lanes ended, as one: an error wins, then a stop (not counting the lanes the app
/// stopped itself in `backed_off`: those only left their items for the second pass).
pub fn combine(results: Vec<io::Result<RunEnd>>, backed_off: &HashSet<usize>) -> io::Result<RunEnd> {
    let mut ends = Vec::new();
    for (lane, result) in results.into_iter().enumerate() {
        match result {
            Err(e) => return Err(e),
            Ok(RunEnd::Stopped) if backed_off.contains(&lane) => {}
            Ok(end) => ends.push(end),
        }
    }
    if ends.contains(&RunEnd::Stopped) {
        return Ok(RunEnd::Stopped);
    }
    let exit_code = ends
        .iter()
        .find_map(|e| match e {
            RunEnd::Finished { exit_code } if *exit_code != 0 => Some(*exit_code),
            _ => None,
        })
        .unwrap_or(0);
    Ok(RunEnd::Finished { exit_code })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    use super::*;

    fn items(positions: &[u32]) -> Vec<Item> {
        positions.iter().map(|&p| Item::new(p, Some(&format!("Canción {p}")), &format!("id{p}"))).collect()
    }

    fn titled(entries: &[(u32, &str)]) -> Vec<Item> {
        entries.iter().map(|&(p, t)| Item::new(p, Some(t), &format!("id{p}"))).collect()
    }

    // === SPLITTING ===

    #[test]
    fn a_numbered_playlist_is_cut_into_two_contiguous_halves_that_continue_the_numbering() {
        let shares = split(&items(&[1, 2, 3, 4, 5]), 10, true, 2);
        assert_eq!(shares, [
            Share { positions: vec![1, 2, 3], offset: 10 },
            Share { positions: vec![4, 5], offset: 13 },
        ]);
    }

    #[test]
    fn the_shares_are_disjoint_and_together_are_every_item() {
        let all: Vec<u32> = (1..=701).collect();
        for numbered in [true, false] {
            let shares = split(&items(&all), 0, numbered, 2);
            assert_eq!(shares.len(), 2);
            let mut joined: Vec<u32> = shares.iter().flat_map(|s| s.positions.clone()).collect();
            joined.sort_unstable();
            assert_eq!(joined, all, "numbered = {numbered}");
        }
    }

    #[test]
    fn items_not_in_order_or_repeated_are_sorted_and_asked_once() {
        let shares = split(&items(&[9, 3, 3, 7, 1]), 0, true, 2);
        let joined: Vec<u32> = shares.iter().flat_map(|s| s.positions.clone()).collect();
        assert_eq!(joined, [1, 3, 7, 9]);
    }

    #[test]
    fn one_worker_or_a_single_item_is_one_share_with_the_offset_untouched() {
        assert_eq!(split(&items(&[4, 5, 6]), 7, true, 1), [Share { positions: vec![4, 5, 6], offset: 7 }]);
        assert_eq!(split(&items(&[4]), 7, true, 2), [Share { positions: vec![4], offset: 7 }]);
        assert!(split(&[], 7, true, 2).is_empty());
    }

    #[test]
    fn the_workers_asked_for_are_capped() {
        assert_eq!(split(&items(&(1..=30).collect::<Vec<_>>()), 0, true, 16).len(), MAX_WORKERS);
        assert_eq!(workers_for(false), 1);
        assert_eq!(workers_for(true), MAX_WORKERS);
    }

    #[test]
    fn an_unnumbered_playlist_keeps_equal_titles_in_the_same_lane() {
        // «Intro» three times and a title cleaned of its noise that equals another one:
        // two processes must never write the same file name at the same time.
        let entries = [
            (1, "Intro"), (2, "Otra"), (3, "Intro"), (4, "Una más"),
            (5, "Hola (Official Video)"), (6, "Intro"), (7, "hola"),
        ];
        let shares = split(&titled(&entries), 0, false, 2);
        assert_eq!(shares.len(), 2);
        let lane_of = |p: u32| shares.iter().position(|s| s.positions.contains(&p)).unwrap();
        assert_eq!(lane_of(1), lane_of(3));
        assert_eq!(lane_of(1), lane_of(6));
        assert_eq!(lane_of(5), lane_of(7));
        // Unnumbered: nothing to continue.
        assert!(shares.iter().all(|s| s.offset == 0));
    }

    #[test]
    fn an_unnumbered_playlist_stays_balanced_when_every_title_differs() {
        let shares = split(&items(&(1..=10).collect::<Vec<_>>()), 0, false, 2);
        assert_eq!((shares[0].positions.len(), shares[1].positions.len()), (5, 5));
    }

    #[test]
    fn an_untitled_item_is_told_apart_by_its_id() {
        let two = vec![Item::new(1, None, "aaaaaaaaaaa"), Item::new(2, None, "bbbbbbbbbbb")];
        assert_eq!(split(&two, 0, false, 2).len(), 2);
    }

    #[test]
    fn positions_become_short_ranges() {
        assert_eq!(item_ranges(&[1, 2, 3, 5, 9, 10]), "1-3,5,9-10");
        assert_eq!(item_ranges(&[4]), "4");
        assert_eq!(item_ranges(&[]), "");
        assert_eq!(item_ranges(&(1..=700).collect::<Vec<_>>()), "1-700");
    }

    // === PROGRESS ===

    fn close(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("no progress was emitted");
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn one_lane_reads_progress_as_the_parser_always_did() {
        let mut p = Progress::bounded(400, &[300]);
        close(p.parse(0, "[download] Downloading item 1 of 300"), 400.0 / 700.0);
        close(p.parse(0, "[download] Downloading item 300 of 300"), 699.0 / 700.0);
        close(p.parse(0, "[download] 100% of 3.00MiB"), 1.0);
    }

    #[test]
    fn two_lanes_add_up_in_one_bar_that_counts_what_was_already_there() {
        let mut p = Progress::bounded(100, &[50, 50]);
        // Lane 0 is at its item 11 (10 done), lane 1 still at its first.
        close(p.parse(0, "[download] Downloading item 11 of 50"), 110.0 / 200.0);
        close(p.parse(1, "[download] Downloading item 1 of 50"), 110.0 / 200.0);
        // Lane 1 moves on: 20 done there.
        close(p.parse(1, "[download] Downloading item 21 of 50"), 130.0 / 200.0);
        close(p.parse(1, "[download]  50.0% of 3.00MiB"), 130.5 / 200.0);
    }

    #[test]
    fn a_failed_item_of_one_lane_does_not_advance_the_bar() {
        let mut p = Progress::bounded(0, &[10, 10]);
        p.parse(0, "[download] Downloading item 6 of 10");
        p.failed(0, "aaaaaaaaaaa");
        close(p.parse(0, "[download] Downloading item 6 of 10"), 4.0 / 20.0);
        p.recovered(0, "aaaaaaaaaaa");
        close(p.parse(0, "[download] Downloading item 6 of 10"), 5.0 / 20.0);
        // The other lane's failures are its own.
        close(p.parse(1, "[download] Downloading item 1 of 10"), 5.0 / 20.0);
    }

    #[test]
    fn an_unbounded_run_trusts_yt_dlp_s_own_total() {
        let mut p = Progress::unbounded();
        p.parse(0, "[youtube:tab] Playlist X: Downloading 10 items of 10");
        close(p.parse(0, "[download] Downloading item 3 of 10"), 0.2);
    }

    // === RUNNING ===

    /// Waits until `n` threads are in, or the time is out: true only if they all met.
    struct Meeting {
        arrived: Mutex<usize>,
        all: Condvar,
        n: usize,
    }

    impl Meeting {
        fn new(n: usize) -> Self {
            Self { arrived: Mutex::new(0), all: Condvar::new(), n }
        }

        fn wait(&self) -> bool {
            let mut arrived = self.arrived.lock().unwrap();
            *arrived += 1;
            self.all.notify_all();
            let deadline = Instant::now() + Duration::from_secs(5);
            while *arrived < self.n {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return false;
                }
                arrived = self.all.wait_timeout(arrived, left).unwrap().0;
            }
            true
        }
    }

    /// Prints what it was given as its «url» (last argument) and reports how many ran at once.
    struct Probe {
        meeting: Meeting,
        met: AtomicUsize,
        running: AtomicUsize,
        most: AtomicUsize,
        /// Lines each lane prints, by the lane's name (the last argument).
        script: fn(&str) -> Vec<&'static str>,
        /// The lanes that, once they have printed, stay alive until they are stopped
        /// (a download that takes long): no race between the stop and the end.
        hang: &'static [&'static str],
    }

    impl Downloader for Probe {
        fn run(&self, request: &YtDlpRequest, stop: &StopFlag, on_line: &mut dyn FnMut(OutputLine)) -> io::Result<RunEnd> {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            if self.meeting.wait() {
                self.met.fetch_add(1, Ordering::SeqCst);
            }
            let name = request.args.last().cloned().unwrap_or_default();
            let mut end = RunEnd::Finished { exit_code: 0 };
            for line in (self.script)(&name) {
                if stop.is_stopped() {
                    end = RunEnd::Stopped;
                    break;
                }
                on_line(OutputLine::Stdout(line.to_string()));
            }
            if end == (RunEnd::Finished { exit_code: 0 }) && self.hang.contains(&name.as_str()) {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !stop.is_stopped() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(2));
                }
                if stop.is_stopped() {
                    end = RunEnd::Stopped;
                }
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            Ok(end)
        }
    }

    fn probe(n: usize, script: fn(&str) -> Vec<&'static str>) -> Probe {
        hanging(n, script, &[])
    }

    fn hanging(n: usize, script: fn(&str) -> Vec<&'static str>, hang: &'static [&'static str]) -> Probe {
        Probe {
            meeting: Meeting::new(n), met: AtomicUsize::new(0), running: AtomicUsize::new(0), most: AtomicUsize::new(0),
            script, hang,
        }
    }

    fn request(name: &str) -> YtDlpRequest {
        YtDlpRequest { args: vec![name.to_string()] }
    }

    #[test]
    fn two_requests_really_run_at_the_same_time_and_never_more() {
        let p = probe(2, |_| vec!["a"]);
        let mut lines = Vec::new();
        let results = run(&p, &StopFlag::new(), &[request("one"), request("two")], &mut |lane, line| {
            let OutputLine::Stdout(t) = line else { panic!() };
            lines.push((lane, t));
            None
        });
        assert_eq!(p.met.load(Ordering::SeqCst), 2, "they never ran together");
        assert_eq!(p.most.load(Ordering::SeqCst), 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&(0, "a".to_string())) && lines.contains(&(1, "a".to_string())));
        assert!(results.iter().all(|r| matches!(r, Ok(RunEnd::Finished { exit_code: 0 }))));
    }

    #[test]
    fn a_single_request_runs_on_the_calling_thread_and_hands_its_stop_over() {
        let p = probe(1, |_| vec!["a", "b", "c"]);
        let stop = StopFlag::new();
        let mut seen = 0;
        let results = run(&p, &stop, &[request("one")], &mut |lane, _| {
            assert_eq!(lane, 0);
            seen += 1;
            stop.stop();
            None
        });
        assert_eq!(seen, 1, "the stop reached the process");
        assert!(matches!(results[0], Ok(RunEnd::Stopped)));
    }

    #[test]
    fn stopping_stops_both_processes() {
        // Each lane prints a line and stays alive: both are running when the stop comes.
        let p = hanging(2, |_| vec!["1"], &["one", "two"]);
        let stop = StopFlag::new();
        let mut count = 0;
        let results = run(&p, &stop, &[request("one"), request("two")], &mut |_, _| {
            count += 1;
            if count == 2 {
                stop.stop();
            }
            None
        });
        assert!(results.iter().all(|r| matches!(r, Ok(RunEnd::Stopped))), "{results:?}");
        assert_eq!(count, 2);
    }

    #[test]
    fn the_handler_can_stop_one_lane_and_the_other_goes_on() {
        let p = hanging(2, |name| if name == "two" { vec!["x"] } else { vec!["y"; 4] }, &["two"]);
        let mut from_one = 0;
        let results = run(&p, &StopFlag::new(), &[request("one"), request("two")], &mut |lane, _| {
            if lane == 1 {
                return Some(1);
            }
            from_one += 1;
            None
        });
        assert!(matches!(results[1], Ok(RunEnd::Stopped)));
        assert!(matches!(results[0], Ok(RunEnd::Finished { exit_code: 0 })));
        assert_eq!(from_one, 4, "the lane that was not stopped printed everything");
    }

    // === HOW THEY END ===

    #[test]
    fn combine_prefers_an_error_then_a_stop_then_the_first_failing_exit_code() {
        let none = HashSet::new();
        let ok = || Ok(RunEnd::Finished { exit_code: 0 });
        assert!(combine(vec![ok(), Err(io::Error::other("x"))], &none).is_err());
        assert_eq!(combine(vec![ok(), Ok(RunEnd::Stopped)], &none).unwrap(), RunEnd::Stopped);
        assert_eq!(
            combine(vec![ok(), Ok(RunEnd::Finished { exit_code: 1 })], &none).unwrap(),
            RunEnd::Finished { exit_code: 1 }
        );
        assert_eq!(combine(vec![ok(), ok()], &none).unwrap(), RunEnd::Finished { exit_code: 0 });
    }

    #[test]
    fn a_lane_the_app_stopped_itself_is_not_a_stop_of_the_run() {
        let backed_off: HashSet<usize> = [1].into();
        let end = combine(vec![Ok(RunEnd::Finished { exit_code: 0 }), Ok(RunEnd::Stopped)], &backed_off).unwrap();
        assert_eq!(end, RunEnd::Finished { exit_code: 0 });
        // But the user's stop still counts when it hit the other one.
        assert_eq!(combine(vec![Ok(RunEnd::Stopped), Ok(RunEnd::Stopped)], &backed_off).unwrap(), RunEnd::Stopped);
    }

    #[test]
    fn a_429_line_is_recognised() {
        assert!(is_rate_limited("ERROR: unable to download video data: HTTP Error 429: Too Many Requests"));
        assert!(!is_rate_limited("ERROR: [youtube] abcdefghijk: Video unavailable"));
    }
}
