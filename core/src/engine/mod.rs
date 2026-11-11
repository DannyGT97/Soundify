//! The download engine: what runs a download from start to end, with no UI
//! (the Java `DownloadCoordinator`, `SafetyNetService`, `BackgroundTasks` and
//! `ActivityLog`).

pub mod activity;
pub mod download;
pub mod resources;
pub mod safety_net;
pub mod tasks;
