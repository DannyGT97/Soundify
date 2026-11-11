//! What runs in the background (one download or sync at a time today), what
//! waits its turn behind it and what already finished: the state behind the
//! menu's indicator and the «Tareas en segundo plano» window (the Java
//! `BackgroundTasks`).
//!
//! Every task has an id from the moment it is asked for, and says which things
//! it needs to itself ([`ResourceKey`]: its archive, its folder, the phone).
//! The planner ([`BackgroundTasks::start_ready`]) starts the waiting ones in
//! order, up to [`BackgroundTasks::limit`] at once, and never two that share a
//! key, nor one that would overtake an earlier one it shares a key with. The
//! limit is [`MAX_RUNNING`] (1) unless Ajustes → Avanzado turns on several downloads at once
//! ([`MAX_RUNNING_PARALLEL`], experimental). On top of that, every task holds a yt-dlp process
//! and may borrow a second one for its playlist, and all of them together never pass
//! [`MAX_PROCESSES`]: two tasks run one process each, and a task that already has two keeps the
//! rest waiting.
//!
//! The engine does not say what a task is: whoever launches it notes it down.
//! How it ended comes from the engine's notices, and a notice that arrives a
//! moment AFTER the task closed still corrects the last record
//! ([`LATE_REPORT_SECONDS`]).

use std::collections::VecDeque;

use chrono::NaiveDateTime;
use serde::Serialize;

use crate::clock::{self, Clock};
use crate::engine::resources::{self, ResourceKey};
use crate::model::TaskRecord;
use crate::task_history::{self, FAILED, OK, STOPPED};

/// How long after finishing a notice still amends the last record: the
/// engine's "all downloaded" / "blocked" notices can land just after IDLE.
pub const LATE_REPORT_SECONDS: i64 = 5;

/// How many tasks run at once in the app unless the user asks for more (the default).
pub const MAX_RUNNING: usize = 1;
/// How many tasks run at once with Ajustes → Avanzado → «Varias descargas a la vez» (experimental).
pub const MAX_RUNNING_PARALLEL: usize = 2;
/// How many yt-dlp processes all the tasks together may have (a task borrows a second one for its playlist).
pub const MAX_PROCESSES: usize = 2;

pub const ORIGIN_URL: &str = "tasks.origin.url";
pub const ORIGIN_MANAGER: &str = "tasks.origin.manager";

/// What a task is called from the moment it is asked for until the app closes;
/// never reused.
pub type TaskId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Prepare,
    Download,
    Retry,
}

/// A task in progress, as the front shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningTask {
    pub id: TaskId,
    pub title: String,
    /// An i18n key ([`ORIGIN_URL`], [`ORIGIN_MANAGER`]).
    pub origin: String,
    pub step: Step,
    /// 0–1 while downloading; None while it cannot tell (preparing).
    pub fraction: Option<f64>,
    /// How many files this run downloads (1 for a single one).
    pub total: u32,
    /// The archive it works on (the Gestor's block key), once the engine says it.
    pub block_key: Option<String>,
}

/// A task waiting its turn: first in, first out among those that share something.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedTask {
    /// What the front asks to remove it by.
    pub id: TaskId,
    pub title: String,
    pub origin: String,
    /// The archive it will work on, when the request already says it (a Gestor sync).
    pub block_key: Option<String>,
    /// What it needs to itself; the front has no use for it.
    #[serde(skip)]
    pub resources: Vec<ResourceKey>,
}

/// Everything the front needs, in one piece.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksView {
    /// In the order they started.
    pub running: Vec<RunningTask>,
    /// In the order they will start.
    pub queued: Vec<QueuedTask>,
    /// The status of a result nobody has looked at yet (the indicator's mark).
    pub unseen: Option<String>,
    pub history: Vec<TaskRecord>,
}

struct Current {
    task: RunningTask,
    resources: Vec<ResourceKey>,
    outcome: Option<&'static str>,
    summary: Option<(String, Vec<String>)>,
    stop_requested: bool,
    /// The yt-dlp processes it holds: 1, or 2 while its playlist runs with two.
    processes: usize,
}

pub struct BackgroundTasks {
    clock: Clock,
    limit: usize,
    process_limit: usize,
    running: Vec<Current>,
    queue: VecDeque<QueuedTask>,
    next_id: TaskId,
    /// The last task that closed, and when: only it can still be amended by a late notice.
    last_finished: Option<(TaskId, NaiveDateTime)>,
    window_open: bool,
    unseen: Option<String>,
    history: Vec<TaskRecord>,
}

impl BackgroundTasks {
    pub fn new(clock: Clock, history: Vec<TaskRecord>) -> Self {
        Self {
            clock,
            limit: MAX_RUNNING,
            process_limit: MAX_PROCESSES,
            running: Vec::new(),
            queue: VecDeque::new(),
            next_id: 1,
            last_finished: None,
            window_open: false,
            unseen: None,
            history,
        }
    }

    /// How many may run at once (at least 1). The app never calls this: it is how the planner is tested.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Changes how many may run at once (at least 1). It only decides what starts next: what
    /// already runs is not touched.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
    }

    /// The yt-dlp processes the running tasks hold now.
    pub fn processes_in_use(&self) -> usize {
        self.running.iter().map(|c| c.processes).sum()
    }

    /// A task wants a second yt-dlp process for its playlist. Only if the total stays within
    /// [`MAX_PROCESSES`] and nobody waiting could use that process instead (a task that can start
    /// now): two tasks beat two processes on one. False = go on with one.
    pub fn claim_extra_process(&mut self, id: TaskId) -> bool {
        if self.processes_in_use() >= self.process_limit {
            return false;
        }
        let held: Vec<ResourceKey> = self.running.iter().flat_map(|c| c.resources.iter().cloned()).collect();
        let someone_could_start = self.running.len() < self.limit && self.queue.iter().any(|task| !resources::conflict(&held, &task.resources));
        if someone_could_start {
            return false;
        }
        match self.find_mut(id) {
            Some(current) if current.processes < 2 => {
                current.processes = 2;
                true
            }
            _ => false,
        }
    }

    /// The task gives its second process back: whatever waited for it may start (the caller asks
    /// the planner).
    pub fn release_extra_process(&mut self, id: TaskId) {
        if let Some(current) = self.find_mut(id) {
            current.processes = 1;
        }
    }

    pub fn view(&self) -> TasksView {
        TasksView {
            running: self.running.iter().map(|c| c.task.clone()).collect(),
            queued: self.queue.iter().cloned().collect(),
            unseen: self.unseen.clone(),
            history: self.history.clone(),
        }
    }

    pub fn history(&self) -> &[TaskRecord] {
        &self.history
    }

    /// Something is in progress.
    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    pub fn is_running_id(&self, id: TaskId) -> bool {
        self.find(id).is_some()
    }

    fn find(&self, id: TaskId) -> Option<&Current> {
        self.running.iter().find(|c| c.task.id == id)
    }

    fn find_mut(&mut self, id: TaskId) -> Option<&mut Current> {
        self.running.iter_mut().find(|c| c.task.id == id)
    }

    // === THE QUEUE AND THE PLANNER ===

    /// Puts a task behind the others; returns its id. It starts with the next
    /// [`BackgroundTasks::start_ready`].
    pub fn enqueue(&mut self, title: &str, origin: &str, block_key: Option<&str>, resources: Vec<ResourceKey>) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        self.queue.push_back(QueuedTask {
            id,
            title: title.to_string(),
            origin: origin.to_string(),
            block_key: block_key.map(str::to_string),
            resources,
        });
        id
    }

    /// Starts every waiting task that can: there is room, and it shares nothing
    /// with a running task nor with an earlier task that is still waiting.
    /// Returns the ids it started, in order.
    pub fn start_ready(&mut self) -> Vec<TaskId> {
        let mut held: Vec<ResourceKey> = self.running.iter().flat_map(|c| c.resources.iter().cloned()).collect();
        let mut started = Vec::new();
        let mut waiting = VecDeque::new();
        while let Some(task) = self.queue.pop_front() {
            let room = self.running.len() < self.limit && self.processes_in_use() < self.process_limit;
            if room && !resources::conflict(&held, &task.resources) {
                held.extend(task.resources.iter().cloned());
                started.push(task.id);
                self.begin(task);
            } else {
                // It keeps its place: whoever comes after cannot take what it needs.
                held.extend(task.resources.iter().cloned());
                waiting.push_back(task);
            }
        }
        self.queue = waiting;
        started
    }

    /// Drops a waiting task; false when it is not waiting any more (it started).
    pub fn remove_queued(&mut self, id: TaskId) -> bool {
        let before = self.queue.len();
        self.queue.retain(|task| task.id != id);
        self.queue.len() != before
    }

    /// Empties the queue (what runs is not touched). Returns what was dropped.
    pub fn clear_queue(&mut self) -> Vec<TaskId> {
        self.queue.drain(..).map(|task| task.id).collect()
    }

    /// The archive is being worked on or waits its turn: a second request for it is refused.
    pub fn is_busy_with(&self, block_key: &str) -> bool {
        self.running.iter().any(|c| c.task.block_key.as_deref() == Some(block_key))
            || self.queue.iter().any(|task| task.block_key.as_deref() == Some(block_key))
    }

    fn begin(&mut self, task: QueuedTask) {
        self.running.push(Current {
            task: RunningTask {
                id: task.id,
                title: task.title,
                origin: task.origin,
                step: Step::Prepare,
                fraction: None,
                total: 1,
                block_key: task.block_key,
            },
            resources: task.resources,
            outcome: None,
            summary: None,
            stop_requested: false,
            processes: 1,
        });
        self.unseen = None;
    }

    // === LIFECYCLE ===

    /// A task that starts at once, whatever runs and whatever it shares (no limit, no reservations):
    /// for what must not wait. The title may still be provisional (a URL) until
    /// [`BackgroundTasks::offer_title`] gives the real one.
    pub fn start(&mut self, title: &str, origin: &str) -> TaskId {
        let id = self.enqueue(title, origin, None, Vec::new());
        let task = self.queue.pop_back().expect("just enqueued");
        self.begin(task);
        id
    }

    pub fn offer_title(&mut self, id: TaskId, title: &str) {
        if let Some(current) = self.find_mut(id)
            && !title.trim().is_empty()
        {
            current.task.title = title.to_string();
        }
    }

    /// The archive the task works on: the Gestor outlines its block.
    pub fn set_block(&mut self, id: TaskId, block_key: &str) {
        if let Some(current) = self.find_mut(id) {
            current.task.block_key = Some(block_key.to_string());
        }
    }

    pub fn set_step(&mut self, id: TaskId, step: Step, total: u32) {
        if let Some(current) = self.find_mut(id) {
            current.task.step = step;
            current.task.total = total.max(1);
            current.task.fraction = if step == Step::Prepare { None } else { Some(0.0) };
        }
    }

    pub fn set_progress(&mut self, id: TaskId, fraction: f64) {
        if let Some(current) = self.find_mut(id) {
            current.task.fraction = Some(fraction.clamp(0.0, 1.0));
        }
    }

    /// Marks one task as asked to stop; false when it is not running.
    pub fn request_stop(&mut self, id: TaskId) -> bool {
        match self.find_mut(id) {
            Some(current) => {
                current.stop_requested = true;
                true
            }
            None => false,
        }
    }

    /// Marks every running task as asked to stop; returns their ids.
    pub fn request_stop_all(&mut self) -> Vec<TaskId> {
        for current in &mut self.running {
            current.stop_requested = true;
        }
        self.running.iter().map(|c| c.task.id).collect()
    }

    /// How a task is going: while running the worse outcome wins; within
    /// [`LATE_REPORT_SECONDS`] of finishing it amends the last record instead
    /// (only when it was the last one to close: an older record is left alone).
    /// Returns true when the history changed and has to be saved.
    pub fn report(&mut self, id: TaskId, status: &'static str, key: &str, args: &[&str]) -> bool {
        let summary = (key.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        if let Some(current) = self.find_mut(id) {
            let worse = task_history::worse(current.outcome, Some(status));
            if worse == Some(status) {
                current.outcome = Some(status);
                current.summary = Some(summary);
            }
            return false;
        }
        let recent = self
            .last_finished
            .is_some_and(|(last, at)| last == id && ((self.clock)() - at).num_seconds() <= LATE_REPORT_SECONDS);
        let Some(last) = self.history.first_mut().filter(|_| recent) else { return false };
        let merged = task_history::worse(last.status.as_deref(), Some(status)).map(str::to_string);
        if merged.as_deref() == Some(status) {
            last.status = merged;
            last.summary = Some(summary.0);
            last.summary_args = summary.1;
            if self.unseen.is_some() {
                self.unseen = last.status.clone();
            }
        }
        true
    }

    /// Closes the task, frees what it held and files it in the history. Returns the record.
    pub fn finish(&mut self, id: TaskId) -> Option<TaskRecord> {
        let index = self.running.iter().position(|c| c.task.id == id)?;
        let current = self.running.remove(index);
        let status = if current.stop_requested {
            task_history::worse(current.outcome, Some(STOPPED)).unwrap_or(STOPPED)
        } else {
            current.outcome.unwrap_or(OK)
        };
        let now = (self.clock)();
        let (summary, summary_args) = current.summary.map_or((None, Vec::new()), |(k, a)| (Some(k), a));
        let record = TaskRecord {
            status: Some(status.to_string()),
            title: Some(current.task.title),
            origin: Some(current.task.origin),
            summary,
            summary_args,
            finished_at: Some(clock::stamp(now)),
        };
        task_history::add(&mut self.history, record.clone());
        self.last_finished = Some((id, now));
        self.unseen = if self.window_open { None } else { Some(status.to_string()) };
        Some(record)
    }

    /// A start that never got going (bad input): nothing to keep.
    pub fn fail_to_start(&mut self, id: TaskId, key: &str, args: &[&str]) -> Option<TaskRecord> {
        self.report(id, FAILED, key, args);
        self.finish(id)
    }

    // === THE WINDOW ===

    pub fn set_window_open(&mut self, open: bool) {
        self.window_open = open;
        if open {
            self.unseen = None;
        }
    }

    pub fn mark_seen(&mut self) {
        self.unseen = None;
    }

    /// «Borrar terminadas»: never touches a task in progress.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_history::WARNING;
    use std::sync::{Arc, Mutex};

    fn tasks_at(stamp: &str) -> (BackgroundTasks, Arc<Mutex<NaiveDateTime>>) {
        let now = Arc::new(Mutex::new(NaiveDateTime::parse_from_str(stamp, clock::STAMP_FORMAT).unwrap()));
        let shared = now.clone();
        (BackgroundTasks::new(Arc::new(move || *shared.lock().unwrap()), Vec::new()), now)
    }

    fn advance(now: &Mutex<NaiveDateTime>, seconds: i64) {
        let mut at = now.lock().unwrap();
        *at += chrono::Duration::seconds(seconds);
    }

    /// A planner with room for `limit` at once (the app has one).
    fn planner(limit: usize) -> BackgroundTasks {
        tasks_at("2026-11-03 10:00").0.with_limit(limit)
    }

    fn folder(name: &str) -> Vec<ResourceKey> {
        vec![ResourceKey::folder(name)]
    }

    fn titles(tasks: &BackgroundTasks) -> Vec<String> {
        tasks.view().running.into_iter().map(|r| r.title).collect()
    }

    // === THE QUEUE (one at a time, as in the app) ===

    #[test]
    fn tasks_wait_in_order_and_start_one_at_a_time() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let a = tasks.enqueue("A", ORIGIN_MANAGER, Some("/a/data.json"), Vec::new());
        let b = tasks.enqueue("B", ORIGIN_URL, None, Vec::new());
        assert_eq!(tasks.view().queued.iter().map(|q| q.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);

        assert_eq!(tasks.start_ready(), vec![a], "the app runs one at a time");
        let running = tasks.view().running;
        assert_eq!(running.len(), 1);
        assert_eq!((running[0].title.as_str(), running[0].block_key.as_deref()), ("A", Some("/a/data.json")), "the block is known from the start");
        assert!(tasks.start_ready().is_empty(), "B waits for A");

        tasks.finish(a);
        assert_eq!(tasks.start_ready(), vec![b]);
        assert_eq!(titles(&tasks), ["B"]);
        assert!(tasks.view().queued.is_empty());
        assert!(tasks.start_ready().is_empty(), "an empty queue starts nothing");
    }

    #[test]
    fn a_waiting_task_can_be_removed_but_not_a_started_one() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let a = tasks.enqueue("A", ORIGIN_URL, None, Vec::new());
        let b = tasks.enqueue("B", ORIGIN_URL, None, Vec::new());
        let c = tasks.enqueue("C", ORIGIN_URL, None, Vec::new());
        assert!(tasks.remove_queued(b));
        assert!(!tasks.remove_queued(b), "already gone");
        assert_eq!(tasks.start_ready(), vec![a]);
        assert!(!tasks.remove_queued(a), "it started");
        assert_eq!(tasks.clear_queue(), vec![c]);
        assert!(tasks.view().queued.is_empty());
        assert!(tasks.is_running(), "clearing the queue leaves the running task alone");
    }

    #[test]
    fn ids_are_not_reused_and_a_block_is_busy_while_it_runs_or_waits() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let a = tasks.enqueue("A", ORIGIN_MANAGER, Some("/a/data.json"), Vec::new());
        assert!(tasks.is_busy_with("/a/data.json"));
        assert!(!tasks.is_busy_with("/b/data.json"));
        tasks.start_ready();
        assert!(tasks.is_busy_with("/a/data.json"), "running counts");
        tasks.finish(a);
        assert!(!tasks.is_busy_with("/a/data.json"));
        let again = tasks.enqueue("A", ORIGIN_MANAGER, Some("/a/data.json"), Vec::new());
        assert!(again > a);
    }

    #[test]
    fn the_view_names_each_running_task_by_its_id() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let a = tasks.start("A", ORIGIN_URL);
        let view = tasks.view();
        assert_eq!(view.running[0].id, a);
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["running"][0]["id"], a);
        assert!(json["running"].is_array());
        tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        let json = serde_json::to_value(tasks.view()).unwrap();
        assert!(json["queued"][0].get("resources").is_none(), "what a task holds is not the front's business");
    }

    #[test]
    fn clearing_the_history_keeps_the_task_in_progress() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let first = tasks.start("Clásicos", ORIGIN_MANAGER);
        tasks.finish(first);
        tasks.start("Otra", ORIGIN_URL);

        tasks.clear_history();

        assert!(tasks.history().is_empty());
        assert_eq!(titles(&tasks), ["Otra"]);
    }

    #[test]
    fn the_block_being_downloaded_is_known_only_while_it_runs() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.set_block(99, "/archives/pl_P/mp3/data.json");
        assert!(tasks.view().running.is_empty(), "nothing runs: nothing to outline");

        let p = tasks.start("P", ORIGIN_MANAGER);
        assert_eq!(tasks.view().running[0].block_key, None, "unknown until the engine says it");
        tasks.set_block(p, "/archives/pl_P/mp3/data.json");
        assert_eq!(tasks.view().running[0].block_key.as_deref(), Some("/archives/pl_P/mp3/data.json"));

        tasks.finish(p);
        tasks.start("Otra", ORIGIN_URL);
        assert_eq!(tasks.view().running[0].block_key, None, "a new task does not inherit it");
    }

    #[test]
    fn a_task_with_no_notice_finishes_ok_and_is_unseen() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("https://youtu.be/x", ORIGIN_URL);
        tasks.offer_title(id, "Clásicos");
        let record = tasks.finish(id).unwrap();
        assert_eq!(record.status.as_deref(), Some(OK));
        assert_eq!(record.title.as_deref(), Some("Clásicos"));
        assert_eq!(tasks.view().unseen.as_deref(), Some(OK));
        assert!(tasks.finish(id).is_none(), "a task closes once");
    }

    #[test]
    fn the_worse_notice_wins_while_running() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("P", ORIGIN_MANAGER);
        tasks.report(id, FAILED, "activity.botcheck.notice", &[]);
        tasks.report(id, WARNING, "activity.retry.summaryOne", &["a", "b"]);
        let record = tasks.finish(id).unwrap();
        assert_eq!(record.status.as_deref(), Some(FAILED));
        assert_eq!(record.summary.as_deref(), Some("activity.botcheck.notice"));
    }

    #[test]
    fn a_stop_request_ends_stopped_unless_it_had_failed() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("P", ORIGIN_URL);
        tasks.report(id, WARNING, "activity.retry.summaryOne", &["a", "b"]);
        assert!(tasks.request_stop(id));
        assert_eq!(tasks.finish(id).unwrap().status.as_deref(), Some(STOPPED));

        let id = tasks.start("P", ORIGIN_URL);
        tasks.report(id, FAILED, "activity.botcheck.notice", &[]);
        tasks.request_stop(id);
        assert_eq!(tasks.finish(id).unwrap().status.as_deref(), Some(FAILED));
        assert!(!tasks.request_stop(id), "it is not running any more");
    }

    #[test]
    fn a_late_notice_amends_the_last_record_only_for_a_few_seconds() {
        let (mut tasks, now) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("P", ORIGIN_MANAGER);
        tasks.finish(id);

        advance(&now, 2);
        assert!(tasks.report(id, WARNING, "activity.retry.summaryOne", &["a", "b"]));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(WARNING));
        assert_eq!(tasks.history()[0].summary_args, ["a", "b"]);
        assert_eq!(tasks.view().unseen.as_deref(), Some(WARNING));

        advance(&now, 10);
        assert!(!tasks.report(id, FAILED, "activity.botcheck.notice", &[]));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(WARNING));
    }

    #[test]
    fn with_the_window_open_the_result_is_already_seen() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.set_window_open(true);
        let id = tasks.start("P", ORIGIN_URL);
        tasks.finish(id);
        assert_eq!(tasks.view().unseen, None);
    }

    #[test]
    fn progress_is_clamped_and_preparing_has_none() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("P", ORIGIN_URL);
        assert_eq!(tasks.view().running[0].fraction, None);
        tasks.set_step(id, Step::Download, 12);
        tasks.set_progress(id, 1.7);
        let running = tasks.view().running.remove(0);
        assert_eq!((running.fraction, running.total), (Some(1.0), 12));
    }

    // === ISOLATION AND RESERVATIONS (the planner with room for several) ===

    #[test]
    fn the_app_runs_one_task_at_a_time_even_when_nothing_is_shared() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        assert_eq!(tasks.limit(), MAX_RUNNING);
        tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        assert_eq!(tasks.start_ready().len(), 1);
        assert_eq!(tasks.running_count(), 1);
    }

    #[test]
    fn unrelated_tasks_run_together_up_to_the_limit() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        let b = tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        let c = tasks.enqueue("C", ORIGIN_URL, None, folder("/music/c"));
        assert_eq!(tasks.start_ready(), vec![a, b]);
        assert_eq!(titles(&tasks), ["A", "B"]);
        assert_eq!(tasks.view().queued.iter().map(|q| q.id).collect::<Vec<_>>(), vec![c], "no room for a third");

        tasks.finish(a);
        assert_eq!(tasks.start_ready(), vec![c], "a place frees up");
    }

    #[test]
    fn a_task_waits_for_the_one_holding_its_folder_even_with_room() {
        let mut tasks = planner(3);
        let a = tasks.enqueue("A", ORIGIN_MANAGER, Some("/a/data.json"), vec![ResourceKey::archive("/a/data.json"), ResourceKey::folder("/music/x")]);
        let b = tasks.enqueue("B", ORIGIN_URL, None, folder("/MUSIC/X/"));
        assert_eq!(tasks.start_ready(), vec![a], "the same folder spelled another way is the same folder");
        assert!(tasks.start_ready().is_empty());

        tasks.finish(a);
        assert_eq!(tasks.start_ready(), vec![b], "finishing frees what it held");
    }

    #[test]
    fn nobody_overtakes_a_waiting_task_it_shares_something_with() {
        let mut tasks = planner(4);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/x"));
        let b = tasks.enqueue("B", ORIGIN_URL, None, vec![ResourceKey::folder("/music/x"), ResourceKey::folder("/music/y")]);
        let c = tasks.enqueue("C", ORIGIN_URL, None, folder("/music/y"));
        let d = tasks.enqueue("D", ORIGIN_URL, None, folder("/music/z"));
        assert_eq!(tasks.start_ready(), vec![a, d], "C would take Y from B, which came first; D shares nothing");

        tasks.finish(a);
        assert_eq!(tasks.start_ready(), vec![b], "B goes before C");
        tasks.finish(b);
        assert_eq!(tasks.start_ready(), vec![c]);
    }

    #[test]
    fn the_phone_takes_one_copy_at_a_time() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, vec![ResourceKey::Device]);
        let b = tasks.enqueue("B", ORIGIN_MANAGER, Some("/b/data.json"), vec![ResourceKey::archive("/b/data.json"), ResourceKey::Device]);
        let c = tasks.enqueue("C", ORIGIN_URL, None, folder("/music/c"));
        assert_eq!(tasks.start_ready(), vec![a, c], "a local download does not need the phone");
        tasks.finish(a);
        assert_eq!(tasks.start_ready(), vec![b]);
    }

    // === THE PROCESS LIMIT (all the tasks together) ===

    #[test]
    fn the_limit_can_be_changed_and_only_decides_what_starts_next() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        assert_eq!(tasks.start_ready(), vec![a]);
        tasks.set_limit(MAX_RUNNING_PARALLEL);
        assert_eq!(tasks.start_ready().len(), 1, "B can start now");
        assert_eq!(tasks.running_count(), 2);
        tasks.set_limit(1);
        assert_eq!(tasks.running_count(), 2, "going back to one does not stop what runs");
        tasks.set_limit(0);
        assert_eq!(tasks.limit(), 1, "at least one");
    }

    #[test]
    fn a_second_process_is_lent_only_when_the_total_stays_within_the_limit() {
        let mut tasks = planner(2);
        let a = tasks.start("A", ORIGIN_URL);
        assert!(tasks.claim_extra_process(a));
        assert_eq!(tasks.processes_in_use(), 2);
        assert!(!tasks.claim_extra_process(a), "it already has two");

        // With A holding both, B has to wait even though there is room for another task.
        let b = tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        assert!(tasks.start_ready().is_empty());
        tasks.release_extra_process(a);
        assert_eq!(tasks.processes_in_use(), 1);
        assert_eq!(tasks.start_ready(), vec![b], "giving the process back lets B start");
        assert_eq!(tasks.processes_in_use(), 2);
        assert!(!tasks.claim_extra_process(b), "two tasks, one process each: nothing to lend");
    }

    #[test]
    fn a_task_that_could_start_goes_before_a_second_process_for_another() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        assert_eq!(tasks.start_ready(), vec![a]);
        tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        assert!(!tasks.claim_extra_process(a), "B could use that process");
        tasks.start_ready();
        assert_eq!(tasks.running_count(), 2);
    }

    #[test]
    fn a_waiting_task_that_cannot_start_anyway_does_not_stop_the_second_process() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        tasks.start_ready();
        tasks.enqueue("A again", ORIGIN_URL, None, folder("/music/a"));
        assert!(tasks.claim_extra_process(a), "the one waiting needs A's folder: it waits for A whatever happens");

        let mut single = planner(1);
        let s = single.start("S", ORIGIN_URL);
        single.enqueue("T", ORIGIN_URL, None, folder("/music/t"));
        assert!(single.claim_extra_process(s), "with one task at a time there is no one to lend it to");
    }

    #[test]
    fn removing_a_waiting_task_frees_its_place_in_line() {
        let mut tasks = planner(3);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/x"));
        let b = tasks.enqueue("B", ORIGIN_URL, None, vec![ResourceKey::folder("/music/x"), ResourceKey::folder("/music/y")]);
        let c = tasks.enqueue("C", ORIGIN_URL, None, folder("/music/y"));
        assert_eq!(tasks.start_ready(), vec![a]);
        assert!(tasks.remove_queued(b));
        assert_eq!(tasks.start_ready(), vec![c], "B no longer holds Y");
    }

    #[test]
    fn each_task_keeps_its_own_title_progress_block_and_stop() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        let b = tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        tasks.start_ready();

        tasks.offer_title(b, "Rock");
        tasks.set_block(b, "/archives/pl_rock/mp3/data.json");
        tasks.set_step(b, Step::Download, 5);
        tasks.set_progress(b, 0.4);
        let view = tasks.view();
        assert_eq!((view.running[0].title.as_str(), view.running[0].block_key.as_deref(), view.running[0].fraction), ("A", None, None));
        assert_eq!((view.running[1].title.as_str(), view.running[1].fraction, view.running[1].total), ("Rock", Some(0.4), 5));
        assert!(tasks.is_busy_with("/archives/pl_rock/mp3/data.json"));

        tasks.report(a, FAILED, "activity.botcheck.notice", &[]);
        tasks.report(b, WARNING, "activity.retry.summaryOne", &["a", "b"]);
        assert!(tasks.request_stop(b));
        assert_eq!(tasks.finish(a).unwrap().status.as_deref(), Some(FAILED), "B's stop is not A's");
        assert_eq!(tasks.finish(b).unwrap().status.as_deref(), Some(STOPPED));
        assert_eq!(tasks.history().len(), 2);
    }

    #[test]
    fn stopping_all_marks_every_running_task_but_not_the_waiting_ones() {
        let mut tasks = planner(2);
        let a = tasks.enqueue("A", ORIGIN_URL, None, folder("/music/a"));
        let b = tasks.enqueue("B", ORIGIN_URL, None, folder("/music/b"));
        let c = tasks.enqueue("C", ORIGIN_URL, None, folder("/music/c"));
        tasks.start_ready();
        assert_eq!(tasks.request_stop_all(), vec![a, b]);
        assert_eq!(tasks.finish(a).unwrap().status.as_deref(), Some(STOPPED));
        assert_eq!(tasks.finish(b).unwrap().status.as_deref(), Some(STOPPED));
        assert_eq!(tasks.start_ready(), vec![c]);
        assert_eq!(tasks.finish(c).unwrap().status.as_deref(), Some(OK), "the stop does not leak to the next one");
    }

    #[test]
    fn a_late_notice_only_amends_its_own_task_and_only_if_it_closed_last() {
        let (tasks, now) = tasks_at("2026-11-03 10:00");
        let mut tasks = tasks.with_limit(2);
        let a = tasks.start("A", ORIGIN_URL);
        let b = tasks.start("B", ORIGIN_URL);
        tasks.finish(a);
        tasks.finish(b);

        advance(&now, 1);
        assert!(!tasks.report(a, FAILED, "activity.botcheck.notice", &[]), "A is not the record on top any more");
        assert_eq!(tasks.history()[0].title.as_deref(), Some("B"));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(OK));
        assert!(tasks.report(b, WARNING, "activity.retry.summaryOne", &["a", "b"]));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(WARNING));
        assert_eq!(tasks.history()[1].status.as_deref(), Some(OK), "the other record is untouched");
    }

    #[test]
    fn the_notices_of_a_task_that_is_gone_change_nothing() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        assert!(!tasks.report(42, FAILED, "x", &[]));
        tasks.offer_title(42, "x");
        tasks.set_progress(42, 0.5);
        assert!(!tasks.request_stop(42));
        assert!(tasks.finish(42).is_none());
        assert!(tasks.history().is_empty());
    }

    #[test]
    fn a_start_that_never_got_going_is_filed_as_failed() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        let id = tasks.start("x", ORIGIN_URL);
        let record = tasks.fail_to_start(id, "activity.error.emptyInput", &[]).unwrap();
        assert_eq!(record.status.as_deref(), Some(FAILED));
        assert!(!tasks.is_running_id(id));
    }
}
