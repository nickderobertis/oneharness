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
//!   MOCK_ECHO_PWD   if set, write `PWD=<the inherited $PWD>` to stdout and exit
//!                   (used to assert that --cwd keeps $PWD consistent)
//!   MOCK_ECHO_ENV   if set to a variable NAME, write `NAME=<inherited value>`
//!                   to stdout and exit (used to assert per-harness env injection)
//!   MOCK_ATTEMPT_FILE  if set, a counter file: each invocation reads the prior
//!                   count, increments it, writes it back, and (when
//!                   MOCK_STDOUT_<n> is set for that 1-based attempt) emits that
//!                   instead of MOCK_STDOUT — used to script the structured-output
//!                   retry loop, where attempt 1 is invalid and a later one valid.
//!   MOCK_LOG_FILE   if set, an append-only run log: each invocation appends `S\n`
//!                   when it starts (before MOCK_SLEEP_MS) and `E\n` when it ends
//!                   (after the sleep). With a sleep that exceeds spawn latency,
//!                   the interleaving reveals scheduling: concurrent calls all
//!                   write `S` before the first `E`; a barrier (one call, then the
//!                   rest) shows `S E` before any further `S`. Used to pin the
//!                   batch `speed` vs `min-tokens` wave ordering. Small single-byte
//!                   lines + O_APPEND keep cross-process writes from interleaving.

use std::io::Write;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    if let Ok(path) = std::env::var("MOCK_ARGV_FILE") {
        let _ = std::fs::write(path, argv.join("\n"));
    }

    // Append one line to the run log (when configured), opening fresh each time so
    // concurrent invocations share the file via the OS's append semantics.
    let log_line = |line: &str| {
        if let Ok(path) = std::env::var("MOCK_LOG_FILE") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    };

    if std::env::var_os("MOCK_ECHO_PWD").is_some() {
        let pwd = std::env::var("PWD").unwrap_or_default();
        let _ = write!(std::io::stdout(), "PWD={pwd}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(name) = std::env::var("MOCK_ECHO_ENV") {
        let value = std::env::var(&name).unwrap_or_default();
        let _ = write!(std::io::stdout(), "{name}={value}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    log_line("S\n");
    if let Ok(ms) = std::env::var("MOCK_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    log_line("E\n");

    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = write!(std::io::stderr(), "{text}");
    }

    // Per-attempt scripting: with MOCK_ATTEMPT_FILE set, increment a counter and
    // prefer MOCK_STDOUT_<attempt> when present, so a test can make the first
    // response invalid and a later one valid to exercise the retry loop.
    let attempt = std::env::var("MOCK_ATTEMPT_FILE").ok().map(|path| {
        let prior = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let n = prior + 1;
        let _ = std::fs::write(&path, n.to_string());
        n
    });
    let attempt_stdout = attempt.and_then(|n| std::env::var(format!("MOCK_STDOUT_{n}")).ok());
    let stdout = attempt_stdout
        .or_else(|| std::env::var("MOCK_STDOUT").ok())
        .unwrap_or_else(|| "{\"result\":\"mock ok\"}".to_string());
    let _ = write!(std::io::stdout(), "{stdout}");
    let _ = std::io::stdout().flush();

    let code = std::env::var("MOCK_EXIT")
        .ok()
        .and_then(|c| c.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
