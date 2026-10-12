//! Pure logic for the iairport Windows port. Everything here builds and
//! tests on any host. The Windows API layer lives in `win`.

pub mod association;
pub mod cli;
pub mod eventlog;
pub mod events;
pub mod format;
pub mod ie;
pub mod ip_state;
pub mod join_timing;
pub mod link;
pub mod monitor;
pub mod oui;
pub mod output;
pub mod reason_codes;
pub mod session_log;
pub mod throughput;

#[cfg(windows)]
pub mod win;

pub type Date = chrono::DateTime<chrono::Local>;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
