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

/// One job's final result plus how many times it was invoked. `attempts` is 1
/// for a plain run, and more when a retry policy re-ran it (structured output).
pub struct Outcome {
    pub capture: Capture,
    pub attempts: u32,
}

/// Run `jobs` concurrently, at most `max_parallel` at a time, preserving order.
pub fn run_jobs(jobs: &[Job], max_parallel: usize) -> Vec<Capture> {
    run_jobs_with(jobs, max_parallel, |_, _, _| None)
        .into_iter()
        .map(|o| o.capture)
        .collect()
}

/// Like [`run_jobs`], but after each run consults `retry` to decide whether to
/// re-run the same job with a new argv. `retry(job_index, attempt, &capture)`
/// returns `Some(next_argv)` to run again (e.g. structured-output validation
/// failed, so re-prompt) or `None` to stop. `attempt` is the number of runs
/// completed so far (1 after the first), and the policy is responsible for its
/// own bound — the loop runs until it returns `None`. Only the argv changes
/// across attempts; cwd/env/timeout are reused. Returns each job's final capture
/// and total attempt count, in input order.
///
/// `retry` is `Sync` because it is shared across worker threads; the domain
/// validation it performs is pure, keeping this layer free of parsing logic.
pub fn run_jobs_with<F>(jobs: &[Job], max_parallel: usize, retry: F) -> Vec<Outcome>
where
    F: Fn(usize, u32, &Capture) -> Option<Vec<String>> + Sync,
{
    let n = jobs.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = max_parallel.clamp(1, n);
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Outcome>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let retry = &retry;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= n {
                    break;
                }
                let outcome = run_job_with_retry(&jobs[i], i, retry);
                *slots[i].lock().expect("slot mutex poisoned") = Some(outcome);
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

/// Run one job, then loop while `retry` asks for another attempt with a new argv.
fn run_job_with_retry<F>(job: &Job, index: usize, retry: &F) -> Outcome
where
    F: Fn(usize, u32, &Capture) -> Option<Vec<String>>,
{
    let mut capture = run_job(job);
    let mut attempts = 1u32;
    while let Some(next_argv) = retry(index, attempts, &capture) {
        let next = Job {
            argv: next_argv,
            cwd: job.cwd.clone(),
            env: job.env.clone(),
            timeout: job.timeout,
        };
        capture = run_job(&next);
        attempts += 1;
    }
    Outcome { capture, attempts }
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

/// Decide the program and argument list to actually spawn for `argv`.
///
/// Normally this is just the [`resolve_program`]'d binary plus `argv[1..]`. On
/// Windows there is one rewrite: since Rust 1.77, `Command` runs a `.cmd`/`.bat`
/// through cmd.exe but refuses (with `InvalidInput`) any argument it cannot
/// escape for cmd.exe — a newline is the trigger. npm installs every JS harness
/// as a `claude.cmd` shim, so a multi-line argument (a rendered `--system`)
/// fails to spawn. When that exact situation is detected — a resolved `.cmd`/
/// `.bat` *and* a multi-line argument — parse the npm shim and invoke its
/// underlying interpreter (node) and script directly; node is a real `.exe`, so
/// std's ordinary argument encoding carries the newline through. Anything else
/// (single-line args, an unparseable shim, a non-shim `.cmd`) falls through
/// unchanged, so the established spawn path — and its error reporting — is byte
/// for byte what it was. The function is platform-shaped on purpose: the rewrite
/// only exists on Windows, and the pure shim parsing it relies on is tested on
/// every platform in `domain::shim`.
fn spawn_target(argv: &[String]) -> (std::ffi::OsString, Vec<String>) {
    let resolved = resolve_program(&argv[0]);
    let rest = argv[1..].to_vec();
    #[cfg(windows)]
    {
        if let Some(plan) = windows_shim_plan(std::path::Path::new(&resolved), &rest) {
            return plan;
        }
    }
    (resolved, rest)
}

/// The Windows shim rewrite for [`spawn_target`]: `Some((interpreter, args))`
/// when `resolved` is a `.cmd`/`.bat`, some argument is multi-line, and the file
/// parses as an npm shim; `None` (fall through to the `.cmd`) otherwise.
#[cfg(windows)]
fn windows_shim_plan(
    resolved: &std::path::Path,
    args: &[String],
) -> Option<(std::ffi::OsString, Vec<String>)> {
    // Only act when std would actually refuse the spawn: a multi-line argument.
    if !args.iter().any(|a| a.contains('\n') || a.contains('\r')) {
        return None;
    }
    let ext = resolved.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "cmd" && ext != "bat" {
        return None;
    }
    let contents = std::fs::read_to_string(resolved).ok()?;
    let dir = resolved.parent()?.to_str()?;
    let target = crate::domain::shim::parse_cmd_shim(&contents, dir)?;
    let mut full = target.prefix_args;
    full.extend_from_slice(args);
    Some((resolve_program(&target.interpreter), full))
}

/// Run a single job, returning its raw capture. Never panics on harness behavior.
pub fn run_job(job: &Job) -> Capture {
    let start = Instant::now();
    let (program, args) = spawn_target(&job.argv);
    let mut command = Command::new(program);
    command
        .args(&args)
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
    fn run_jobs_with_retries_until_the_policy_stops() {
        // The structured-output loop in disguise: re-run with a fresh argv until
        // the policy returns None. Uses unspawnable binaries so it stays portable
        // and process-free, asserting on the attempt count and that the *final*
        // capture came from the last retry's argv (its error names that binary).
        let jobs = [job(&["/no/such/first"])];
        let outcomes = run_jobs_with(&jobs, 1, |i, attempt, cap| {
            assert_eq!(i, 0);
            assert_eq!(cap.status, Status::SpawnError);
            (attempt < 3).then(|| vec![format!("/no/such/retry-{attempt}")])
        });
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].attempts, 3);
        let err = outcomes[0].capture.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("retry-2"),
            "final capture should be last retry: {err}"
        );
    }

    #[test]
    fn run_jobs_with_a_no_op_policy_runs_once() {
        let jobs = [job(&["/no/such/binary"])];
        let outcomes = run_jobs_with(&jobs, 1, |_, _, _| None);
        assert_eq!(outcomes[0].attempts, 1);
        // Empty input is still the no-work fast path.
        assert!(run_jobs_with(&[], 4, |_, _, _| None).is_empty());
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
    fn windows_shim_plan_rewrites_a_cmd_only_for_multiline_args() {
        use std::io::Write;

        // A minimal npm-style `.cmd` shim written to a real temp file, so the
        // plan reads it the way `run_job` would.
        let dir = std::env::temp_dir().join(format!("oh-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cmd_path = dir.join("claude.cmd");
        let mut f = std::fs::File::create(&cmd_path).unwrap();
        write!(
            f,
            "SET \"_prog=node\"\r\n\"%_prog%\" \"%dp0%\\cli.js\" %*\r\n"
        )
        .unwrap();
        drop(f);

        let dir_str = dir.to_str().unwrap();
        let script = format!("{dir_str}\\cli.js");

        // A multi-line argument triggers the rewrite: node + script + the args.
        let multiline = vec!["-p".to_string(), "a\nb\nc".to_string()];
        let (prog, args) = windows_shim_plan(&cmd_path, &multiline).expect("multiline → rewrite");
        // `node` resolves to a real path ending in node.exe.
        assert!(
            std::path::Path::new(&prog)
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("node.exe"),
            "interpreter should resolve to node.exe, got {prog:?}"
        );
        assert_eq!(args, vec![script, "-p".to_string(), "a\nb\nc".to_string()]);

        // A single-line argument must NOT be rewritten — the `.cmd` spawns fine.
        let single = vec!["-p".to_string(), "hello".to_string()];
        assert!(windows_shim_plan(&cmd_path, &single).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(windows)]
    #[test]
    fn windows_shim_plan_ignores_non_batch_programs() {
        // A real `.exe` (or anything not `.cmd`/`.bat`) is never rewritten, even
        // with a multi-line argument — std spawns it directly without cmd.exe.
        let exe = resolve_program("where");
        let multiline = vec!["x\ny".to_string()];
        assert!(windows_shim_plan(std::path::Path::new(&exe), &multiline).is_none());
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
