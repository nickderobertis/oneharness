//! oneharness: one CLI across many agentic coding harnesses.
//!
//! This crate is the thin CLI shell. The reusable engine — the pure `domain`
//! layer and the `io` boundary — lives in the `oneharness-core` library crate;
//! here we add only the clap surface (`cli`) and the per-verb orchestration
//! (`commands`). `main` parses with clap and calls [`dispatch`].

pub mod cli;
pub mod commands;

pub use cli::{Cli, Command};
pub use oneharness_core::errors::OneharnessError;

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
        Command::Gate(args) => commands::gate::run(&args),
        Command::Mock(args) => commands::mock::run(&args),
    };
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("oneharness: {err}");
            2
        }
    }
}
