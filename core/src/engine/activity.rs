//! The «Actividad» window's model: the last lines of what the download is
//! doing, kept in memory so a window opened mid-download still sees what came
//! before (the Java `ActivityLog`).
//!
//! The full record goes to the log files; this is only what is worth seeing
//! now, so it is capped and the oldest lines fall off.

use serde::Serialize;

/// Lines kept after a trim.
pub const MAX_LINES: usize = 500;
/// Slack before trimming: trimming repaints the whole view, so it is not done
/// on every line past the cap but once every [`TRIM_MARGIN`] lines.
pub const TRIM_MARGIN: usize = 200;

/// One line of Activity: yt-dlp's own output as it came, or a message of the
/// app as an i18n key, so the front shows it in the language in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActivityLine {
    Raw { text: String },
    Message { key: String, args: Vec<String> },
}

impl ActivityLine {
    pub fn raw(text: impl Into<String>) -> Self {
        Self::Raw { text: text.into() }
    }

    pub fn message(key: &str, args: &[&str]) -> Self {
        Self::Message { key: key.to_string(), args: args.iter().map(|a| a.to_string()).collect() }
    }
}

/// Whoever shows the lines. A new line is appended; a trim repaints it all,
/// because lines are dropped from the top.
pub trait View: Send {
    fn on_line(&mut self, line: &ActivityLine);
    fn on_reset(&mut self, all: &[ActivityLine]);
}

pub type ViewId = usize;

#[derive(Default)]
pub struct ActivityLog {
    lines: Vec<ActivityLine>,
    views: Vec<(ViewId, Box<dyn View>)>,
    next_id: ViewId,
}

impl ActivityLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, line: ActivityLine) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES + TRIM_MARGIN {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(..drop);
            for (_, view) in &mut self.views {
                view.on_reset(&self.lines);
            }
        } else if let Some(line) = self.lines.last() {
            for (_, view) in &mut self.views {
                view.on_line(line);
            }
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        for (_, view) in &mut self.views {
            view.on_reset(&[]);
        }
    }

    pub fn snapshot(&self) -> Vec<ActivityLine> {
        self.lines.clone()
    }

    /// A view opened mid-download gets what was already there straight away.
    pub fn add_view(&mut self, mut view: Box<dyn View>) -> ViewId {
        view.on_reset(&self.lines);
        let id = self.next_id;
        self.next_id += 1;
        self.views.push((id, view));
        id
    }

    /// A closed window must stop listening.
    pub fn remove_view(&mut self, id: ViewId) {
        self.views.retain(|(view_id, _)| *view_id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A fake view behaving like the real one: it appends or repaints.
    #[derive(Default)]
    struct Painted {
        lines: Vec<String>,
        resets: usize,
        appended: usize,
    }

    struct FakeView(Arc<Mutex<Painted>>);

    fn text(line: &ActivityLine) -> String {
        match line {
            ActivityLine::Raw { text } => text.clone(),
            ActivityLine::Message { key, .. } => key.clone(),
        }
    }

    impl View for FakeView {
        fn on_line(&mut self, line: &ActivityLine) {
            let mut p = self.0.lock().unwrap();
            p.lines.push(text(line));
            p.appended += 1;
        }

        fn on_reset(&mut self, all: &[ActivityLine]) {
            let mut p = self.0.lock().unwrap();
            p.lines = all.iter().map(text).collect();
            p.resets += 1;
        }
    }

    fn with_view() -> (ActivityLog, Arc<Mutex<Painted>>, ViewId) {
        let mut log = ActivityLog::new();
        let painted = Arc::new(Mutex::new(Painted::default()));
        let id = log.add_view(Box::new(FakeView(painted.clone())));
        (log, painted, id)
    }

    fn snapshot(log: &ActivityLog) -> Vec<String> {
        log.snapshot().iter().map(text).collect()
    }

    #[test]
    fn what_is_appended_reaches_the_view() {
        let (mut log, view, _) = with_view();
        log.append(ActivityLine::raw("primera"));
        log.append(ActivityLine::raw("segunda"));
        assert_eq!(view.lock().unwrap().lines, ["primera", "segunda"]);
        assert_eq!(snapshot(&log), ["primera", "segunda"]);
    }

    #[test]
    fn a_view_opened_mid_download_receives_what_was_already_there() {
        let (mut log, _, _) = with_view();
        log.append(ActivityLine::raw("antes de abrir la ventana"));
        let late = Arc::new(Mutex::new(Painted::default()));
        log.add_view(Box::new(FakeView(late.clone())));
        assert_eq!(late.lock().unwrap().lines, ["antes de abrir la ventana"]);
    }

    #[test]
    fn both_views_receive_the_same() {
        let (mut log, view, _) = with_view();
        let other = Arc::new(Mutex::new(Painted::default()));
        log.add_view(Box::new(FakeView(other.clone())));
        log.append(ActivityLine::message("activity.sync.done", &[]));
        assert_eq!(view.lock().unwrap().lines, ["activity.sync.done"]);
        assert_eq!(other.lock().unwrap().lines, ["activity.sync.done"]);
    }

    #[test]
    fn a_removed_view_stops_receiving() {
        // Otherwise a closed window would keep listening.
        let (mut log, view, id) = with_view();
        log.remove_view(id);
        log.append(ActivityLine::raw("ya no debería llegar"));
        assert!(view.lock().unwrap().lines.is_empty());
        assert_eq!(snapshot(&log), ["ya no debería llegar"]);
    }

    #[test]
    fn clearing_empties_model_and_views() {
        let (mut log, view, _) = with_view();
        log.append(ActivityLine::raw("algo"));
        log.clear();
        assert!(log.snapshot().is_empty());
        assert!(view.lock().unwrap().lines.is_empty());
    }

    #[test]
    fn trimming_leaves_the_cap_and_keeps_the_last_lines() {
        let (mut log, view, _) = with_view();
        for i in 1..=700 {
            log.append(ActivityLine::raw(format!("linea {i}")));
        }
        assert_eq!(view.lock().unwrap().lines.len(), 700, "it should not have trimmed yet");

        log.append(ActivityLine::raw("linea 701"));

        let lines = snapshot(&log);
        assert_eq!(lines.len(), 500);
        assert_eq!(lines.first().map(String::as_str), Some("linea 202"));
        assert_eq!(lines.last().map(String::as_str), Some("linea 701"));
        assert_eq!(view.lock().unwrap().lines, lines);
    }

    #[test]
    fn trimming_repaints_the_whole_view() {
        // Appending would not do: lines are dropped from the top.
        let (mut log, view, _) = with_view();
        for i in 1..=701 {
            log.append(ActivityLine::raw(format!("l{i}")));
        }
        assert!(view.lock().unwrap().resets >= 2, "add_view's reset plus the trim");
        assert_eq!(view.lock().unwrap().lines, snapshot(&log));
    }

    #[test]
    fn copes_with_many_lines_without_drifting() {
        let (mut log, view, _) = with_view();
        for i in 0..5000 {
            log.append(ActivityLine::raw(format!("linea {i}")));
        }
        assert_eq!(view.lock().unwrap().lines, snapshot(&log));
        assert!(log.snapshot().len() <= MAX_LINES + TRIM_MARGIN);
    }

    #[test]
    fn removing_an_unknown_view_does_nothing() {
        let (mut log, view, id) = with_view();
        log.remove_view(id + 42);
        log.append(ActivityLine::raw(""));
        assert_eq!(view.lock().unwrap().lines, [""]);
    }

    #[test]
    fn a_new_line_is_appended_not_repainted() {
        // That is what keeps the view cheap.
        let (mut log, view, _) = with_view();
        log.append(ActivityLine::raw("a"));
        assert_eq!(view.lock().unwrap().appended, 1);
        assert_eq!(view.lock().unwrap().resets, 1, "only add_view's initial reset");
    }

    #[test]
    fn a_message_serializes_with_its_key_and_arguments() {
        let json = serde_json::to_string(&ActivityLine::message("retry.trying", &["Título", "abc"])).unwrap();
        assert_eq!(json, r#"{"kind":"message","key":"retry.trying","args":["Título","abc"]}"#);
    }
}
