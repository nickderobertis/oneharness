//! Thin entry point: parse args (clap handles --help/--version/usage errors),
//! dispatch, and exit with the returned code. No behavior lives here.

use clap::Parser;
use oneharness::{dispatch, Cli};

fn main() {
    let cli = Cli::parse();
    std::process::exit(dispatch(cli));
}
