//! Soundify's logic, with no Tauri and no platform in it.
//!
//! What works differently on desktop and on Android sits behind a trait, so
//! the rest never finds out which one it runs on:
//! - [`storage::Storage`]: files and folders (plain paths vs. SAF `content://` URIs)
//! - [`downloader::Downloader`]: running yt-dlp (a process vs. youtubedl-android)
//!
//! Playback is the third seam, but it lives in the front (`src/api/`): on
//! desktop the webview itself plays the files.

pub mod archive;
pub mod clock;
pub mod downloader;
pub mod library;
pub mod location;
pub mod log_format;
pub mod model;
pub mod paths;
pub mod settings;
pub mod storage;
pub mod task_history;
pub mod transfer;
pub mod version;
pub mod ytdlp;

pub use location::Location;
