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
pub mod connect;
pub mod cover_art;
pub mod cover_embed;
pub mod device;
pub mod downloader;
pub mod engine;
pub mod http;
pub mod library;
pub mod location;
pub mod log_format;
pub mod logs;
pub mod model;
pub mod paths;
pub mod settings;
pub mod storage;
pub mod task_history;
pub mod tools;
pub mod transfer;
pub mod updater;
pub mod version;
pub mod ytdlp;

pub use location::Location;
