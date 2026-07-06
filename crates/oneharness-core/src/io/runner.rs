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
#[derive(Clone)]
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
/// `.bat` *and* a multi-line argument — parse the npm shim and invoke its real
/// target directly: a node interpreter plus script, or — as for claude-code,
/// whose bin is `bin/claude.exe` — the wrapped executable itself. That target is
/// a real `.exe`, so std's ordinary argument encoding carries the newline
/// through (only a `.cmd`/`.bat` goes through cmd.exe). Anything else
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

/// What a streaming line callback asks the run to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStep {
    /// Keep reading the harness's output.
    Continue,
    /// Stop now and tear down the child — the consumer has gone away (e.g. a
    /// broken stdout pipe: it short-circuited on an observed action).
    Stop,
}

/// Run one job, invoking `on_line` for each complete line of stdout **as it
/// arrives**, and return the same [`Capture`] the batch path would (accumulated
/// stdout/stderr, status, timing) so the caller can still emit a final envelope.
///
/// This is the streaming counterpart to [`run_job`]: it exists so a consumer can
/// observe a harness's normalized events incrementally and short-circuit the
/// moment it sees a disallowed action — instead of paying for a whole turn before
/// judging it. The parsing stays out of this layer: `on_line` (a pure
/// domain-driven closure in the command layer) decides what to emit and returns
/// [`StreamStep::Stop`] to end early (the command layer returns `Stop` when its
/// write to the consumer fails, i.e. the consumer closed the stream). On `Stop`
/// or timeout the child is killed; on normal EOF its exit is awaited. Never
/// panics on harness behavior — same contract as [`run_job`].
pub fn run_job_streaming<F>(job: &Job, mut on_line: F) -> Capture
where
    F: FnMut(&str) -> StreamStep,
{
    use std::io::BufRead;
    use std::sync::mpsc;

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
        let pwd = if cwd.is_absolute() {
            cwd.clone()
        } else {
            std::env::current_dir()
                .map(|base| base.join(cwd))
                .unwrap_or_else(|_| cwd.clone())
        };
        command.env("PWD", pwd);
    }
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

    let out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let err_reader = std::thread::spawn(move || read_all(&mut err));

    // A reader thread turns blocking line reads into channel messages, so the
    // main loop can honor the wall-clock timeout (recv_timeout) between lines
    // without non-blocking I/O. Each message is one line *with* its terminator
    // preserved, so the accumulated stdout is byte-faithful to the batch path.
    let (tx, rx) = mpsc::channel::<String>();
    let out_reader = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(out);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if tx.send(line).is_err() {
                        break; // main loop gone
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = start + job.timeout;
    let mut stdout = String::new();
    let mut stopped = false;
    let mut timed_out = false;
    loop {
        let now = Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(line) => {
                stdout.push_str(&line);
                // Feed the callback the line without its trailing newline(s).
                if on_line(line.trim_end_matches(['\n', '\r'])) == StreamStep::Stop {
                    stopped = true;
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // EOF
        }
    }

    let (status, exit_code) = if stopped || timed_out {
        let _ = child.kill();
        let _ = child.wait();
        if timed_out {
            (Status::Timeout, None)
        } else {
            // Consumer-driven stop: not a harness failure. Report Ok so the
            // (best-effort) final envelope isn't misread as an error; the
            // consumer already has what it needed.
            (Status::Ok, None)
        }
    } else {
        match child.wait() {
            Ok(exit) => {
                let code = exit.code();
                let status = if code == Some(0) {
                    Status::Ok
                } else {
                    Status::Nonzero
                };
                (status, code)
            }
            Err(_) => (Status::SpawnError, None),
        }
    };

    // Drain any remaining buffered lines the reader already had, so the final
    // envelope's stdout is complete even if we broke out on stop/timeout.
    let _ = out_reader.join();
    while let Ok(line) = rx.try_recv() {
        stdout.push_str(&line);
    }
    let stderr = err_reader.join().unwrap_or_default();

    let error = if timed_out {
        Some(format!(
            "`{}` exceeded the {}s timeout and was killed. Suggestion: raise --timeout or simplify the prompt",
            job.argv[0],
            job.timeout.as_secs()
        ))
    } else {
        None
    };

    Capture {
        status,
        exit_code,
        duration_ms: Some(start.elapsed().as_millis()),
        stdout,
        stderr,
        error,
    }
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

    /// A portable job that prints three lines then exits 0 (`sh -c printf` on
    /// Unix, `cmd /c echo` on Windows), for exercising the streaming reader.
    fn three_line_job() -> Job {
        #[cfg(not(windows))]
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'a\\nb\\nc\\n'".to_string(),
        ];
        #[cfg(windows)]
        let argv = vec![
            "cmd".to_string(),
            "/c".to_string(),
            "echo a& echo b& echo c".to_string(),
        ];
        Job {
            argv,
            cwd: None,
            env: Vec::new(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn streaming_delivers_each_line_and_accumulates_stdout() {
        // The happy path: every line reaches the callback in order, and the final
        // capture's stdout is the byte-faithful accumulation, status Ok.
        let mut lines = Vec::new();
        let cap = run_job_streaming(&three_line_job(), |line| {
            lines.push(line.to_string());
            StreamStep::Continue
        });
        assert_eq!(cap.status, Status::Ok);
        assert_eq!(cap.exit_code, Some(0));
        assert_eq!(lines.len(), 3, "got {lines:?}");
        // Trim to tolerate any shell quirks; the content is a/b/c in order.
        let trimmed: Vec<_> = lines.iter().map(|l| l.trim()).collect();
        assert_eq!(trimmed, vec!["a", "b", "c"]);
        for token in ["a", "b", "c"] {
            assert!(cap.stdout.contains(token), "stdout: {:?}", cap.stdout);
        }
    }

    #[test]
    fn streaming_stops_and_tears_down_on_callback_stop() {
        // Returning Stop after the first line ends the run immediately (the child
        // is killed); the consumer-driven stop is reported as Ok, not a failure.
        let mut count = 0u32;
        let cap = run_job_streaming(&three_line_job(), |_| {
            count += 1;
            StreamStep::Stop
        });
        assert_eq!(count, 1, "should stop after the first line");
        assert_eq!(cap.status, Status::Ok);
    }

    #[test]
    fn streaming_spawn_error_is_data_not_a_panic() {
        // A missing binary surfaces as a SpawnError capture, same as run_job.
        let job = job(&["/no/such/oneharness-stream-binary"]);
        let cap = run_job_streaming(&job, |_| StreamStep::Continue);
        assert_eq!(cap.status, Status::SpawnError);
        assert!(cap.error.as_deref().unwrap_or_default().contains("spawn"));
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
