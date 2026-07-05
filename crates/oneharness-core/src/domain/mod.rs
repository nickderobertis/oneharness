//! Pure core: no process, filesystem, env, clock, or terminal I/O lives here.
//! These modules build commands, shape the report, and parse output; all real
//! I/O stays behind the `io` and `commands` boundaries.

pub mod batch;
pub mod config;
pub mod events;
pub mod gate;
pub mod harness;
pub mod hooks;
pub mod mode;
pub mod normalize;
pub mod report;
pub mod shim;
pub mod signals;
pub mod structured;
pub mod sync;
