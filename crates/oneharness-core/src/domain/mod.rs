//! Pure core: no process, filesystem, env, clock, or terminal I/O lives here.
//! These modules build commands, shape the report, and parse output; all real
//! I/O stays behind the `io` and `commands` boundaries.

pub mod batch;
pub mod config;
pub mod control;
pub mod dialogue;
pub mod events;
pub mod fallback;
pub mod gate;
pub mod harness;
pub mod history;
pub mod hooks;
pub mod http;
pub mod mock;
pub mod mode;
pub mod normalize;
pub mod report;
pub mod sdk;
pub mod select;
pub mod session;
pub mod shim;
pub mod signals;
pub mod structured;
pub mod sync;
pub mod usage;
