//! Thin entry point: parse args (clap handles --help/--version/usage errors),
//! dispatch, and exit with the returned code. No behavior lives here.

use oneharness::{dispatch, Cli};

fn main() {
    if std::env::var_os("ONEHARNESS_INTERNAL_MOCK_HARNESS").is_some() {
        oneharness::mock_harness::run();
    }
    let cli = Cli::parse_or_exit();
    std::process::exit(dispatch(cli));
}
