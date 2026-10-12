//! Windows API layer. Everything here is `#[cfg(windows)]`; the pure logic in
//! the rest of the crate never touches it.

pub mod app;
pub mod console;
pub mod evtlog;
pub mod iphlp;
pub mod sysinfo;
pub mod util;
pub mod wlan;
