//! MPRIS2 D-Bus integration module
//! Only compiled on Unix platforms (requires D-Bus / zbus).

#[cfg(unix)]
pub mod server;
