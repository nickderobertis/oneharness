//! Pure core: no process, filesystem, env, clock, or terminal I/O lives here.
//! These modules build commands, shape the report, and parse output; all real
//! I/O stays behind the `io` and `commands` boundaries.

pub mod config;
pub mod harness;
pub mod normalize;
pub mod report;
pub mod signals;
