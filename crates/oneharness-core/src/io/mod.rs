//! I/O boundary: PATH resolution, version probing, and subprocess execution.
//! Everything that touches the process, filesystem, env, or clock lives here or
//! in `commands`, never in `domain`.

pub mod config;
pub mod detect;
pub mod history;
pub mod hooks;
mod process;
pub mod runner;
pub mod session;
pub mod sync;
pub mod usage;
