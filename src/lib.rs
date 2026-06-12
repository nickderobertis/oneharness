//! oneharness: one CLI across many agentic coding harnesses.
//!
//! Architecture (mirrored in the module tree):
//! - `domain/` is pure — it builds argv, parses output, and shapes the report
//!   with no process / filesystem / env / clock I/O.
//! - `io/` performs the real I/O: PATH resolution, version probing, spawning.
//! - `commands/` orchestrates the two for each CLI verb.
//!
//! `main` parses with clap and calls [`dispatch`].

pub mod cli;
pub mod commands;
pub mod domain;
pub mod errors;
pub mod io;

pub use cli::{Cli, Command};
pub use errors::OneharnessError;

/// Run the parsed command and return the process exit code.
///
/// Usage / configuration faults are mapped to exit code 2 with a message on
/// stderr; a harness's own failure surfaces as exit code 1 from `run` (its
/// detail lives in the JSON report). Success is 0.
pub fn dispatch(cli: Cli) -> i32 {
    let result = match cli.command {
        Command::Run(args) => commands::run::run(&args),
        Command::List(args) => commands::list::run(&args),
        Command::Detect(args) => commands::detect::run(&args),
        Command::Config(args) => commands::config::run(&args),
        Command::Sync(args) => commands::sync::run(&args),
    };
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("oneharness: {err}");
            2
        }
    }
}
