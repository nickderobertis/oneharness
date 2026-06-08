//! A fake harness binary the e2e tests drive via a `--bin` override, so the
//! spawn / capture / parallel / parse path is exercised hermetically and
//! cross-platform — no real CLI, no network. Built only behind the
//! `mock-harness` feature, so it never ships in `cargo install`.
//!
//! Behavior is scripted entirely through environment variables:
//!   MOCK_STDOUT     bytes written to stdout (default: a JSON `result` doc)
//!   MOCK_STDERR     bytes written to stderr
//!   MOCK_EXIT       process exit code (default: 0)
//!   MOCK_SLEEP_MS   milliseconds to sleep before exiting (to force a timeout)
//!   MOCK_ARGV_FILE  if set, the received argv (one per line) is written here

use std::io::Write;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    if let Ok(path) = std::env::var("MOCK_ARGV_FILE") {
        let _ = std::fs::write(path, argv.join("\n"));
    }

    if let Ok(ms) = std::env::var("MOCK_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = write!(std::io::stderr(), "{text}");
    }

    let stdout =
        std::env::var("MOCK_STDOUT").unwrap_or_else(|_| "{\"result\":\"mock ok\"}".to_string());
    let _ = write!(std::io::stdout(), "{stdout}");
    let _ = std::io::stdout().flush();

    let code = std::env::var("MOCK_EXIT")
        .ok()
        .and_then(|c| c.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
