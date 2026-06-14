//! Spawning harness subprocesses with timeouts and bounded parallelism.
//!
//! This layer only spawns and captures — it does no parsing, so the spawn path
//! and the extraction path stay independently testable. stdout and stderr are
//! drained on their own threads so a chatty harness can never deadlock the wait.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::domain::report::{Capture, Status};

/// A fully-specified subprocess to run.
pub struct Job {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

/// Run `jobs` concurrently, at most `max_parallel` at a time, preserving order.
pub fn run_jobs(jobs: &[Job], max_parallel: usize) -> Vec<Capture> {
    let n = jobs.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = max_parallel.clamp(1, n);
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Capture>>> = (0..n).map(|_| Mutex::new(None)).collect();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= n {
                    break;
                }
                let capture = run_job(&jobs[i]);
                *slots[i].lock().expect("slot mutex poisoned") = Some(capture);
            });
        }
    });

    slots
        .into_iter()
        .map(|m| {
            m.into_inner()
                .expect("slot mutex poisoned")
                .expect("slot unfilled")
        })
        .collect()
}

/// Resolve a program name to the binary actually spawned.
///
/// On Windows this is load-bearing: `CreateProcess` only auto-appends `.exe`, so
/// a bare name like `claude` never finds the `claude.cmd` shim that npm (and many
/// other installers) drop on `PATH`. Resolving via `which` is PATHEXT-aware, so it
/// locates the `.cmd`/`.bat` shim — and `std::process::Command`, given a path that
/// ends in `.cmd`/`.bat`, runs it through the command interpreter with the safe
/// argument escaping added in Rust 1.77 (CVE-2024-24576). Detection already uses
/// `which`, so a harness can be reported `available` yet still fail to spawn from a
/// bare name; this closes that gap. If resolution fails, fall back to the original
/// name so the spawn error stays accurate.
///
/// On non-Windows the name is returned unchanged: PATH lookup already handles
/// extensionless binaries, and we must not alter the established spawn behavior.
fn resolve_program(program: &str) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        if let Ok(path) = which::which(program) {
            return path.into_os_string();
        }
    }
    program.into()
}

/// Run a single job, returning its raw capture. Never panics on harness behavior.
pub fn run_job(job: &Job) -> Capture {
    let start = Instant::now();
    let mut command = Command::new(resolve_program(&job.argv[0]));
    command
        .args(&job.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &job.cwd {
        command.current_dir(cwd);
        // Mirror a shell `cd`: keep $PWD consistent with the working directory.
        // `current_dir` only chdir()s the child; the inherited $PWD stays stale.
        // Some tools (e.g. Bun-based CLIs like OpenCode) trust $PWD over getcwd()
        // to locate the project, so a stale $PWD points them at the wrong dir.
        // Use the logical path (no symlink resolution), like `cd` does.
        let pwd = if cwd.is_absolute() {
            cwd.clone()
        } else {
            std::env::current_dir()
                .map(|base| base.join(cwd))
                .unwrap_or_else(|_| cwd.clone())
        };
        command.env("PWD", pwd);
    }
    // Explicit --env entries win over the derived PWD above.
    for (key, value) in &job.env {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Capture {
                status: Status::SpawnError,
                exit_code: None,
                duration_ms: Some(start.elapsed().as_millis()),
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!(
                    "failed to spawn `{}`: {err}. Suggestion: check the binary exists and is executable (try `oneharness detect`)",
                    job.argv[0]
                )),
            };
        }
    };

    // Drain both pipes on their own threads so wait never blocks on a full buffer.
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || read_all(&mut out));
    let err_reader = std::thread::spawn(move || read_all(&mut err));

    let (status, exit_code, timed_out) = match child.wait_timeout(job.timeout) {
        Ok(Some(exit)) => {
            let code = exit.code();
            let status = if code == Some(0) {
                Status::Ok
            } else {
                Status::Nonzero
            };
            (status, code, false)
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            (Status::Timeout, None, true)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            (Status::SpawnError, None, false)
        }
    };

    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let duration_ms = Some(start.elapsed().as_millis());

    let error = if timed_out {
        Some(format!(
            "`{}` exceeded the {}s timeout and was killed. Suggestion: raise --timeout or simplify the prompt",
            job.argv[0],
            job.timeout.as_secs()
        ))
    } else if status == Status::SpawnError {
        Some(format!("`{}` could not be waited on", job.argv[0]))
    } else {
        None
    };

    Capture {
        status,
        exit_code,
        duration_ms,
        stdout,
        stderr,
        error,
    }
}

fn read_all<R: std::io::Read>(reader: &mut R) -> String {
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(reader, &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(argv: &[&str]) -> Job {
        Job {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn empty_jobs_returns_empty_without_spawning_workers() {
        // The no-work fast path: no jobs means no captures and no threads.
        assert!(run_jobs(&[], 4).is_empty());
    }

    #[test]
    fn spawn_error_is_data_not_a_panic() {
        // A binary that cannot be spawned must surface as a `SpawnError` capture
        // with a helpful message — never a crash. Run it through `run_jobs` so the
        // worker-pool path that fills the result slot is exercised too.
        let jobs = [job(&["/no/such/oneharness-binary-xyz", "arg"])];
        let captures = run_jobs(&jobs, 1);
        assert_eq!(captures.len(), 1);
        let cap = &captures[0];
        assert_eq!(cap.status, Status::SpawnError);
        assert!(cap.exit_code.is_none());
        assert!(cap.stdout.is_empty());
        assert!(cap.duration_ms.is_some());
        let msg = cap.error.as_deref().unwrap_or_default();
        assert!(msg.contains("failed to spawn"), "{msg}");
        assert!(msg.contains("oneharness-binary-xyz"), "{msg}");
    }

    #[test]
    fn resolve_program_falls_back_to_the_name_when_unresolvable() {
        // A name that PATH lookup cannot resolve must come back unchanged on every
        // platform, so the spawn attempt — and its error message — stay accurate.
        let name = "oneharness-no-such-binary-zzz";
        assert_eq!(resolve_program(name), std::ffi::OsString::from(name));
    }

    #[cfg(windows)]
    #[test]
    fn resolve_program_finds_a_cmd_shim_on_windows() {
        // `where` is a stock Windows console command living at a real path; the
        // bare name must resolve to an absolute, existing file (the gap that left
        // npm `.cmd` shims unspawnable from a bare name).
        let resolved = resolve_program("where");
        let path = std::path::Path::new(&resolved);
        assert!(
            path.is_absolute(),
            "expected an absolute path, got {resolved:?}"
        );
        assert!(path.exists(), "resolved path should exist: {resolved:?}");
    }
}
