//! Everything about talking to yt-dlp, minus running it (that is the
//! [`crate::downloader::Downloader`]'s job): the command lines, how to read
//! what it prints, and how to clean up after it.

pub mod cleaning;
pub mod client;
pub mod command;
pub mod progress;
pub mod query;
pub mod unavailable;
