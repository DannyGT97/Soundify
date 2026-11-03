//! What runs in the background (one download or sync at a time) and what
//! already finished: the state behind the menu's indicator and the «Tareas en
//! segundo plano» window (the Java `BackgroundTasks`).
//!
//! The engine does not say what a task is: whoever launches it notes it down
//! ([`BackgroundTasks::start`]). How it ended comes from the engine's notices,
//! and a notice that arrives a moment AFTER the task closed still corrects the
//! last record ([`LATE_REPORT_SECONDS`]).

use chrono::NaiveDateTime;
use serde::Serialize;

use crate::clock::{self, Clock};
use crate::model::TaskRecord;
use crate::task_history::{self, FAILED, OK, STOPPED};

/// How long after finishing a notice still amends the last record: the
/// engine's "all downloaded" / "blocked" notices can land just after IDLE.
pub const LATE_REPORT_SECONDS: i64 = 5;

pub const ORIGIN_URL: &str = "tasks.origin.url";
pub const ORIGIN_MANAGER: &str = "tasks.origin.manager";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Prepare,
    Download,
    Retry,
}

/// The task in progress, as the front shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningTask {
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

/// Everything the front needs, in one piece.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksView {
    pub running: Option<RunningTask>,
    /// The status of a result nobody has looked at yet (the indicator's mark).
    pub unseen: Option<String>,
    pub history: Vec<TaskRecord>,
}

struct Current {
    task: RunningTask,
    outcome: Option<&'static str>,
    summary: Option<(String, Vec<String>)>,
    stop_requested: bool,
}

pub struct BackgroundTasks {
    clock: Clock,
    current: Option<Current>,
    last_finished_at: Option<NaiveDateTime>,
    window_open: bool,
    unseen: Option<String>,
    history: Vec<TaskRecord>,
}

impl BackgroundTasks {
    pub fn new(clock: Clock, history: Vec<TaskRecord>) -> Self {
        Self { clock, current: None, last_finished_at: None, window_open: false, unseen: None, history }
    }

    pub fn view(&self) -> TasksView {
        TasksView {
            running: self.current.as_ref().map(|c| c.task.clone()),
            unseen: self.unseen.clone(),
            history: self.history.clone(),
        }
    }

    pub fn history(&self) -> &[TaskRecord] {
        &self.history
    }

    pub fn is_running(&self) -> bool {
        self.current.is_some()
    }

    // === LIFECYCLE ===

    /// A new task: its title may still be provisional (a URL) until
    /// [`BackgroundTasks::offer_title`] gives the real one.
    pub fn start(&mut self, title: &str, origin: &str) {
        self.current = Some(Current {
            task: RunningTask { title: title.to_string(), origin: origin.to_string(), step: Step::Prepare, fraction: None, total: 1, block_key: None },
            outcome: None,
            summary: None,
            stop_requested: false,
        });
        self.unseen = None;
    }

    pub fn offer_title(&mut self, title: &str) {
        if let Some(current) = &mut self.current
            && !title.trim().is_empty()
        {
            current.task.title = title.to_string();
        }
    }

    /// The archive the running task works on: the Gestor outlines its block.
    pub fn set_block(&mut self, block_key: &str) {
        if let Some(current) = &mut self.current {
            current.task.block_key = Some(block_key.to_string());
        }
    }

    pub fn set_step(&mut self, step: Step, total: u32) {
        if let Some(current) = &mut self.current {
            current.task.step = step;
            current.task.total = total.max(1);
            current.task.fraction = if step == Step::Prepare { None } else { Some(0.0) };
        }
    }

    pub fn set_progress(&mut self, fraction: f64) {
        if let Some(current) = &mut self.current {
            current.task.fraction = Some(fraction.clamp(0.0, 1.0));
        }
    }

    pub fn request_stop(&mut self) {
        if let Some(current) = &mut self.current {
            current.stop_requested = true;
        }
    }

    /// How the task is going: while running the worse outcome wins; within
    /// [`LATE_REPORT_SECONDS`] of finishing it amends the last record instead.
    /// Returns true when the history changed and has to be saved.
    pub fn report(&mut self, status: &'static str, key: &str, args: &[&str]) -> bool {
        let summary = (key.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        if let Some(current) = &mut self.current {
            let worse = task_history::worse(current.outcome, Some(status));
            if worse == Some(status) {
                current.outcome = Some(status);
                current.summary = Some(summary);
            }
            return false;
        }
        let recent = self
            .last_finished_at
            .is_some_and(|at| ((self.clock)() - at).num_seconds() <= LATE_REPORT_SECONDS);
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

    /// Closes the task and files it in the history. Returns the record.
    pub fn finish(&mut self) -> Option<TaskRecord> {
        let current = self.current.take()?;
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
        self.last_finished_at = Some(now);
        self.unseen = if self.window_open { None } else { Some(status.to_string()) };
        Some(record)
    }

    /// A start that never got going (bad input): nothing to keep.
    pub fn fail_to_start(&mut self, key: &str, args: &[&str]) -> Option<TaskRecord> {
        self.report(FAILED, key, args);
        self.finish()
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

    /// «Borrar terminadas»: never touches the task in progress.
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

    #[test]
    fn clearing_the_history_keeps_the_task_in_progress() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.start("Clásicos", ORIGIN_MANAGER);
        tasks.finish();
        tasks.start("Otra", ORIGIN_URL);

        tasks.clear_history();

        assert!(tasks.history().is_empty());
        assert_eq!(tasks.view().running.map(|r| r.title), Some("Otra".to_string()));
    }

    #[test]
    fn the_block_being_downloaded_is_known_only_while_it_runs() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.set_block("/archives/pl_P/mp3/data.json");
        assert!(tasks.view().running.is_none(), "nothing runs: nothing to outline");

        tasks.start("P", ORIGIN_MANAGER);
        assert_eq!(tasks.view().running.unwrap().block_key, None, "unknown until the engine says it");
        tasks.set_block("/archives/pl_P/mp3/data.json");
        assert_eq!(tasks.view().running.unwrap().block_key.as_deref(), Some("/archives/pl_P/mp3/data.json"));

        tasks.finish();
        tasks.start("Otra", ORIGIN_URL);
        assert_eq!(tasks.view().running.unwrap().block_key, None, "a new task does not inherit it");
    }

    #[test]
    fn a_task_with_no_notice_finishes_ok_and_is_unseen() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.start("https://youtu.be/x", ORIGIN_URL);
        tasks.offer_title("Clásicos");
        let record = tasks.finish().unwrap();
        assert_eq!(record.status.as_deref(), Some(OK));
        assert_eq!(record.title.as_deref(), Some("Clásicos"));
        assert_eq!(tasks.view().unseen.as_deref(), Some(OK));
    }

    #[test]
    fn the_worse_notice_wins_while_running() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.start("P", ORIGIN_MANAGER);
        tasks.report(FAILED, "activity.botcheck.notice", &[]);
        tasks.report(WARNING, "activity.retry.summaryOne", &["a", "b"]);
        let record = tasks.finish().unwrap();
        assert_eq!(record.status.as_deref(), Some(FAILED));
        assert_eq!(record.summary.as_deref(), Some("activity.botcheck.notice"));
    }

    #[test]
    fn a_stop_request_ends_stopped_unless_it_had_failed() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.start("P", ORIGIN_URL);
        tasks.report(WARNING, "activity.retry.summaryOne", &["a", "b"]);
        tasks.request_stop();
        assert_eq!(tasks.finish().unwrap().status.as_deref(), Some(STOPPED));

        tasks.start("P", ORIGIN_URL);
        tasks.report(FAILED, "activity.botcheck.notice", &[]);
        tasks.request_stop();
        assert_eq!(tasks.finish().unwrap().status.as_deref(), Some(FAILED));
    }

    #[test]
    fn a_late_notice_amends_the_last_record_only_for_a_few_seconds() {
        let (mut tasks, now) = tasks_at("2026-11-03 10:00");
        tasks.start("P", ORIGIN_MANAGER);
        tasks.finish();

        advance(&now, 2);
        assert!(tasks.report(WARNING, "activity.retry.summaryOne", &["a", "b"]));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(WARNING));
        assert_eq!(tasks.history()[0].summary_args, ["a", "b"]);
        assert_eq!(tasks.view().unseen.as_deref(), Some(WARNING));

        advance(&now, 10);
        assert!(!tasks.report(FAILED, "activity.botcheck.notice", &[]));
        assert_eq!(tasks.history()[0].status.as_deref(), Some(WARNING));
    }

    #[test]
    fn with_the_window_open_the_result_is_already_seen() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.set_window_open(true);
        tasks.start("P", ORIGIN_URL);
        tasks.finish();
        assert_eq!(tasks.view().unseen, None);
    }

    #[test]
    fn progress_is_clamped_and_preparing_has_none() {
        let (mut tasks, _) = tasks_at("2026-11-03 10:00");
        tasks.start("P", ORIGIN_URL);
        assert_eq!(tasks.view().running.unwrap().fraction, None);
        tasks.set_step(Step::Download, 12);
        tasks.set_progress(1.7);
        let running = tasks.view().running.unwrap();
        assert_eq!((running.fraction, running.total), (Some(1.0), 12));
    }
}
