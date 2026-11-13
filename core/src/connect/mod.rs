//! Soundify Connect: one Soundify controls another on the same network (see
//! `PLAN-CONNECT.md`). Everything here is blocking and independent of Tauri.

pub mod beacon;
pub mod client;
pub mod discovery;
pub mod http;
pub mod manager;
pub mod pairing;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod tests;
