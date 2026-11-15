//! Counters of what Connect does, written to the log as ONE summary line every [`WINDOW`] (and as a warning when
//! a number looks like a storm). The log used to say WHAT happened but not how often: a bounce of connections once
//! hid in hundreds of identical lines. Process-wide on purpose (there is one Connect per app, and plumbing a
//! handle to every thread would add more than it gives).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The first characters of a device id: enough to tell two devices apart in a log line.
pub fn short(id: &str) -> &str {
    id.get(..6).unwrap_or(id)
}

/// How often the summary is written.
pub const WINDOW: Duration = Duration::from_secs(30);

/// Per window: more of these than this is written as a warning, not as information.
const WARN_OPENED: u64 = 20;
const WARN_DROPS: u64 = 3;
pub const WARN_STATES: u64 = 600;

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        pub struct Snapshot { $(pub $name: u64),* }

        struct Counters { $($name: AtomicU64),* }

        static COUNTERS: Counters = Counters { $($name: AtomicU64::new(0)),* };

        $(
            #[allow(dead_code)]
            pub fn $name(n: u64) {
                COUNTERS.$name.fetch_add(n, Ordering::Relaxed);
            }
        )*

        /// What was counted since the last call; starts over.
        pub fn take() -> Snapshot {
            Snapshot { $($name: COUNTERS.$name.swap(0, Ordering::Relaxed)),* }
        }
    };
}

counters!(
    // connections this device accepted (a controller connected here) and the ones it opened to others
    accepted, opened, closed,
    // controllers let go of: because their queue was full (they did not keep up) or because they were already gone
    dropped_full, dropped_gone,
    // messages
    states_sent, queues_sent, msgs_out, msgs_in,
    // states that ARRIVED from the devices this one controls (a flood here is somebody else's loop)
    states_received,
    // attempts to reach a device again after losing it
    reconnects,
);

impl Snapshot {
    pub fn is_quiet(&self) -> bool {
        *self == Snapshot::default()
    }

    /// Whether the numbers look like a storm (connections coming and going, controllers dropped, states flooding).
    pub fn is_suspicious(&self) -> bool {
        self.opened + self.accepted > WARN_OPENED || self.dropped_full >= WARN_DROPS || self.states_sent > WARN_STATES || self.states_received > WARN_STATES
    }

    /// The summary line (`secs` = the length of the window it covers).
    pub fn line(&self, secs: u64, controllers: usize, links: usize) -> String {
        format!(
            "stats ({secs}s): accepted={} opened={} closed={} reconnects={} dropped_full={} dropped_gone={} states_sent={} queues_sent={} msgs_in={} msgs_out={} states_received={} | now: controllers={controllers} links={links}",
            self.accepted, self.opened, self.closed, self.reconnects, self.dropped_full, self.dropped_gone, self.states_sent, self.queues_sent, self.msgs_in, self.msgs_out, self.states_received
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_storm_is_suspicious_and_normal_traffic_is_not() {
        let calm = Snapshot { opened: 2, accepted: 2, states_sent: 40, ..Snapshot::default() };
        assert!(!calm.is_suspicious());
        assert!(Snapshot { opened: 15, accepted: 15, ..Snapshot::default() }.is_suspicious());
        assert!(Snapshot { dropped_full: 3, ..Snapshot::default() }.is_suspicious());
        assert!(Snapshot { states_sent: 900, ..Snapshot::default() }.is_suspicious());
    }

    #[test]
    fn the_line_names_every_number() {
        let line = Snapshot { opened: 1, ..Snapshot::default() }.line(30, 2, 3);
        assert!(line.contains("opened=1") && line.contains("controllers=2") && line.contains("links=3"));
    }
}
