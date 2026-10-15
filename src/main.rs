//! Thin entry point: parse args (clap handles --help/--version/usage errors),
//! dispatch, and exit with the returned code. No behavior lives here.

use oneharness::{dispatch, Cli};

fn main() {
    // A process the shared Codex app-server pool started as a pool entry's
    // sweeper runs as one and exits; any other run records that this binary
    // can host one.
    oneharness_core::io::app_server::serve_sweeper_if_requested();
    if std::env::var_os("ONEHARNESS_INTERNAL_MOCK_HARNESS").is_some() {
        oneharness::mock_harness::run();
    }
    let cli = Cli::parse_or_exit();
    std::process::exit(dispatch(cli));
}
