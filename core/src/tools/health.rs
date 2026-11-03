//! The state of the external tools after startup, and the notice shown to the
//! user when something is not where it should be (the Java `ToolHealth`).
//!
//! It comes from a real case: on one user's portable EVERY HTTPS connection to
//! GitHub failed, so the JS runtime never got installed and yt-dlp went five
//! months without an update. The app carried on as if nothing were wrong: all
//! it said was one line in a log nobody looks at. The result: the 197 files of
//! that playlist failed and the second pass spent almost two hours retrying
//! them without them knowing why.
//!
//! **The message to the user is deliberately generic**: they have no reason to
//! know what Deno is, nor yt-dlp, nor an n-challenge. All they need to know is
//! that downloads may fail and what they can do about it. The technical detail
//! (which tool, which error, which version) goes to the log.

use std::collections::BTreeSet;
use std::sync::Mutex;

/// The i18n key of the notice; its `{0}` is the consequence ([`ToolHealth::consequence_key`]).
pub const MESSAGE_KEY: &str = "toolHealth.message";

/// What can go wrong. The user does not see these names, only the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// The update server could not be reached (network, firewall, TLS).
    UpdateCheckFailed,
    /// The installed yt-dlp has gone too long without an update.
    YtdlpOutdated,
    /// The JavaScript runtime yt-dlp needs for YouTube is missing.
    JsRuntimeMissing,
    /// The audio/video converter is missing.
    MediaToolMissing,
}

/// Receives the consequence key each time a NEW problem is noted down.
pub type Listener = Box<dyn Fn(&'static str) + Send + Sync>;

#[derive(Default)]
pub struct ToolHealth {
    problems: Mutex<BTreeSet<Problem>>,
    listener: Mutex<Option<Listener>>,
}

impl ToolHealth {
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes a problem down and tells whoever is listening.
    /// `technical_detail` is for the log only — it is never shown to the user.
    pub fn report(&self, problem: Problem, technical_detail: &str) {
        log::warn!("Tools: {problem:?} — {technical_detail}");
        let is_new = self.problems.lock().unwrap().insert(problem);
        if !is_new {
            return;
        }
        if let (Some(listener), Some(key)) = (self.listener.lock().unwrap().as_ref(), self.consequence_key()) {
            listener(key);
        }
    }

    /// Called when a tool that was broken has fixed itself.
    pub fn resolve(&self, problem: Problem) {
        self.problems.lock().unwrap().remove(&problem);
    }

    pub fn is_healthy(&self) -> bool {
        self.problems.lock().unwrap().is_empty()
    }

    /// The UI subscribes here. If problems were already noted down before
    /// subscribing (the updaters run on threads of their own and can get
    /// ahead), it is told straight away so they are not lost.
    pub fn on_problem(&self, listener: Listener) {
        if let Some(key) = self.consequence_key() {
            listener(key);
        }
        *self.listener.lock().unwrap() = Some(listener);
    }

    /// The `{0}` of [`MESSAGE_KEY`], or `None` while everything is fine. If the
    /// JS runtime is missing, downloads are not going to fail "sometimes" —
    /// they are all going to fail, so that case is put more plainly.
    pub fn consequence_key(&self) -> Option<&'static str> {
        let problems = self.problems.lock().unwrap();
        if problems.is_empty() {
            None
        } else if problems.contains(&Problem::JsRuntimeMissing) {
            Some("toolHealth.willFail")
        } else {
            Some("toolHealth.mayFail")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn recorder(health: &ToolHealth) -> Arc<Mutex<Vec<&'static str>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        health.on_problem(Box::new(move |key| sink.lock().unwrap().push(key)));
        seen
    }

    #[test]
    fn healthy_until_something_is_reported() {
        let health = ToolHealth::new();
        assert!(health.is_healthy());
        assert_eq!(health.consequence_key(), None);

        health.report(Problem::MediaToolMissing, "no ffmpeg");
        assert!(!health.is_healthy());
        assert_eq!(health.consequence_key(), Some("toolHealth.mayFail"));
    }

    #[test]
    fn a_missing_js_runtime_says_downloads_will_fail() {
        let health = ToolHealth::new();
        health.report(Problem::UpdateCheckFailed, "offline");
        health.report(Problem::JsRuntimeMissing, "no deno");
        assert_eq!(health.consequence_key(), Some("toolHealth.willFail"));
    }

    #[test]
    fn the_same_problem_twice_notifies_once() {
        let health = ToolHealth::new();
        let seen = recorder(&health);
        health.report(Problem::UpdateCheckFailed, "offline");
        health.report(Problem::UpdateCheckFailed, "still offline");
        assert_eq!(*seen.lock().unwrap(), ["toolHealth.mayFail"]);
    }

    #[test]
    fn a_late_listener_still_hears_what_happened_before() {
        let health = ToolHealth::new();
        health.report(Problem::JsRuntimeMissing, "no deno");
        let seen = recorder(&health);
        assert_eq!(*seen.lock().unwrap(), ["toolHealth.willFail"]);
    }

    #[test]
    fn resolving_the_only_problem_makes_it_healthy_again() {
        let health = ToolHealth::new();
        health.report(Problem::YtdlpOutdated, "old");
        health.resolve(Problem::YtdlpOutdated);
        assert!(health.is_healthy());
    }
}
