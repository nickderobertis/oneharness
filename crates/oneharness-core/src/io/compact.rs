//! Compact one named session now — `oneharness compact` as a library call.
//!
//! [`compact_session`] resolves the session exactly as `run --session` does
//! (the store, the bound harness identity, that identity's environment),
//! drives the harness's own compaction of a **fork** of the session (the
//! translation in [`crate::domain::lean`]), and rebinds the session name to the
//! fork only when the compaction completed. A cancel through
//! [`RunControls::cancel`] ends the harness process tree and answers
//! `cancelled`; the session's stored files are then exactly as they were,
//! because nothing ever wrote to them.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::domain::compact::{self, CodexDialogue, CodexStep, CompactOutcome, CompactStatus};
use crate::domain::harness::{self, BuildCtx, HarnessIdentity, PromptDelivery};
use crate::domain::lean::{self, Setting};
use crate::domain::mode::PermissionMode;
use crate::domain::report::{OutputFormat, SCHEMA_VERSION};
use crate::domain::sdk::NonEmptyString;
use crate::domain::session;
use crate::domain::signals::Usage;
use crate::errors::OneharnessError;
use crate::io::cancel::{self, CancelToken};
use crate::io::config as config_io;
use crate::io::detect::{self, BinOverrides};
use crate::io::identity::variant_environment;
use crate::io::process::{Finish, PipeEvent, Process};
use crate::io::run::RunControls;
use crate::io::runner::{ProcessSupervisor, CANCEL_POLL_SLICE};
use crate::io::session as session_io;

/// What names and reaches the session to compact, as on
/// [`crate::io::run::RunRequest`].
#[derive(Debug, Clone)]
pub struct CompactRequest {
    /// The harness identity (`base` or `base:variant`) expected to hold the
    /// session. `None` takes the identity the store bound it to; a different
    /// one is refused as `run --session` refuses it.
    pub harness: Option<HarnessIdentity>,
    /// The caller-owned session name (`run --session <name>`).
    pub session: NonEmptyString,
    /// Where the session store lives (`--session-dir`).
    pub session_dir: Option<PathBuf>,
    /// The project the session belongs to and where the harness runs; the
    /// process's current directory when `None`.
    pub cwd: Option<PathBuf>,
    /// Extra `KEY=VALUE` environment for the harness process.
    pub env: Vec<String>,
    /// The model the compaction runs under; `None` defers to config.
    pub model: Option<String>,
    /// Harness binary overrides, `ID=PATH`.
    pub bin: Vec<String>,
    /// Load configuration from exactly these files (`--config`).
    // llmlint: ignore[invalid_states_unrepresentable] The same config/no_config pair `RunRequest` carries, with the same precedence: no_config wins.
    pub config: Vec<PathBuf>,
    /// Ignore every configuration file and `ONEHARNESS_*` override.
    pub no_config: bool,
    /// Seconds before the compaction is abandoned as `failed`; `None` defers
    /// to config `timeout`, and zero or unset means no deadline.
    pub timeout: Option<u64>,
}

impl CompactRequest {
    /// A request to compact `session` with everything else at its default:
    /// the store's own binding, the platform session store, this process's
    /// directory and environment, and configuration discovery.
    #[must_use]
    pub fn new(session: NonEmptyString) -> Self {
        Self {
            harness: None,
            session,
            session_dir: None,
            cwd: None,
            env: Vec::new(),
            model: None,
            bin: Vec::new(),
            config: Vec::new(),
            no_config: false,
            timeout: None,
        }
    }
}

/// Compact the named session now.
///
/// A harness's own behaviour is never an `Err`: no compaction is
/// `unsupported`, a refusal, non-zero exit or timeout is `failed`, and a cancel
/// is `cancelled`, each with the session untouched.
///
/// # Errors
///
/// Usage and configuration faults, before anything spawns: an unreadable
/// config, a malformed `env`/`bin`, a session unknown to the store
/// ([`OneharnessError::CompactSessionUnknown`]) or bound to another identity
/// ([`OneharnessError::SessionHarnessConflict`]).
pub fn compact_session(
    request: &CompactRequest,
    controls: RunControls<'_>,
    supervisor: Option<&dyn ProcessSupervisor>,
) -> Result<CompactOutcome, OneharnessError> {
    let RunControls {
        cancel,
        signal_cancel,
        ..
    } = controls;
    let project = match &request.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let loaded = config_io::load(&request.config, request.no_config, &project)?;
    let cfg = &loaded.config;
    let configured = match request.session_dir.as_deref() {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| OneharnessError::SessionDirInvalid {
                    path: path.display().to_string(),
                })?,
        ),
        None => None,
    };
    let dir = session_io::resolve_dir(configured).ok_or(OneharnessError::SessionNoStore)?;
    let path = session_io::session_path(&dir, &project, request.session.as_str());
    let Some(record) = session::resumable(session_io::read(&path)) else {
        return Err(OneharnessError::CompactSessionUnknown {
            name: request.session.to_string(),
            path: path.display().to_string(),
        });
    };
    let identity: HarnessIdentity = match &request.harness {
        Some(identity) => {
            if let Some(was) = session::harness_conflict(Some(&record), identity) {
                return Err(OneharnessError::SessionHarnessConflict {
                    name: request.session.to_string(),
                    was: was.to_string(),
                    now: identity.to_string(),
                });
            }
            identity.clone()
        }
        None => record.harness.clone(),
    };
    let spec = identity.spec();
    // The caller's own inputs are checked before any answer is given, so a
    // malformed `env` or `bin` is a usage error whatever the harness can do.
    let cli_env = crate::io::run::parse_env(&request.env)?;
    let bin_overrides = BinOverrides::parse(&request.bin)?;
    let outcome = |status: CompactStatus, error: Option<String>| CompactOutcome {
        schema_version: SCHEMA_VERSION.to_string(),
        status,
        session: request.session.to_string(),
        harness: identity.to_string(),
        context_tokens_before: None,
        context_tokens_after: None,
        usage: Usage::default(),
        token: Some(record.token.clone()),
        error,
    };
    let t = lean::translation(spec.id, Setting::CompactNow);
    if !t.supported() {
        return Ok(outcome(
            CompactStatus::Unsupported,
            Some(format!(
                "{} {} has no compaction oneharness can drive ({})",
                spec.id, t.version, t.source
            )),
        ));
    }

    let config_bins: HashMap<String, String> = cfg
        .bin_for(identity.as_str())
        .map(|bin| (identity.as_str().to_string(), bin.to_string()))
        .into_iter()
        .collect();
    let overrides = bin_overrides.with_config_bins(config_bins);
    let resolved = detect::resolve_named(spec, identity.as_str(), &overrides);
    if !resolved.available {
        return Ok(outcome(
            CompactStatus::Failed,
            Some(format!(
                "`{}` not found on PATH. Install it: {}",
                resolved.bin, spec.install_hint
            )),
        ));
    }
    let mut env: Vec<(String, String)> = spec
        .default_env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    env.extend(variant_environment(cfg, identity.as_str(), &project)?);
    env.extend(cli_env);
    let env_remove = cfg
        .variant_for(identity.as_str())
        .map_or_else(Vec::new, |variant| variant.unset_env.clone());
    let model = request
        .model
        .clone()
        .or_else(|| cfg.model_for(identity.as_str()).map(str::to_string));
    let deadline = match request.timeout.or(cfg.timeout).filter(|secs| *secs > 0) {
        Some(seconds) => Some(
            Instant::now()
                .checked_add(Duration::from_secs(seconds))
                .ok_or(OneharnessError::TimeoutOutOfRange { seconds })?,
        ),
        None => None,
    };
    if signal_cancel {
        if let Err(err) = cancel::install_signal_cancel() {
            eprintln!(
                "oneharness: warning: could not install the cancellation signal handler ({err})"
            );
        }
    }

    let launch = Launch {
        cwd: request.cwd.clone(),
        env,
        env_remove,
        deadline,
        cancel: &cancel,
        supervisor,
    };
    let (status, mut out) = match spec.id {
        "claude-code" => claude(&resolved.bin, &record.token, model.as_deref(), &launch),
        "codex" => codex(
            &resolved.bin,
            &record.token,
            model.as_deref(),
            cfg,
            identity.as_str(),
            &launch,
        ),
        other => unreachable!("the translation table supports compaction on `{other}`"),
    };
    out.schema_version = SCHEMA_VERSION.to_string();
    out.session = request.session.to_string();
    out.harness = identity.to_string();
    out.status = status;
    if status == CompactStatus::Completed {
        let fork = out
            .token
            .clone()
            .expect("a completed compaction names its fork");
        if let Err(err) = session_io::write(
            &path,
            &project,
            &identity,
            request.session.as_str(),
            &fork,
            Some(&record),
        ) {
            return Ok(CompactOutcome {
                status: CompactStatus::Failed,
                token: Some(record.token.clone()),
                error: Some(format!(
                    "the compacted fork `{fork}` could not be bound to session `{}` ({}): {err}",
                    request.session,
                    path.display()
                )),
                ..out
            });
        }
    } else {
        out.token = Some(record.token.clone());
    }
    Ok(out)
}

struct Launch<'a> {
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    env_remove: Vec<String>,
    deadline: Option<Instant>,
    cancel: &'a CancelToken,
    supervisor: Option<&'a dyn ProcessSupervisor>,
}

fn blank() -> CompactOutcome {
    CompactOutcome {
        schema_version: String::new(),
        status: CompactStatus::Failed,
        session: String::new(),
        harness: String::new(),
        context_tokens_before: None,
        context_tokens_after: None,
        usage: Usage::default(),
        token: None,
        error: None,
    }
}

/// `claude -p /compact --resume <token> --fork-session --output-format
/// stream-json --verbose`, built by the registry's own adapter.
fn claude(
    bin: &str,
    token: &str,
    model: Option<&str>,
    launch: &Launch<'_>,
) -> (CompactStatus, CompactOutcome) {
    let spec = harness::by_id("claude-code").expect("registered");
    let command = lean::translation(spec.id, Setting::CompactNow)
        .knobs
        .iter()
        .find_map(|k| match k.knob {
            lean::Knob::PromptCommand(command) => Some(command),
            _ => None,
        })
        .unwrap_or("/compact");
    let argv = (spec.build_argv)(&BuildCtx {
        bin,
        prompt: command,
        model,
        system: None,
        resume: Some(token),
        fork: true,
        mode: PermissionMode::Default,
        output_format: OutputFormat::StreamJson,
        schema: None,
        system_file: None,
        delivery: PromptDelivery::Argv,
        plugins: false,
        apps: false,
        analytics: false,
        model_catalog: None,
        tools: None,
    });
    let driven = drive(&argv, launch, &[], |_| Reply::Frames(Vec::new()));
    let read = compact::read_claude(&driven.stdout);
    let mut out = blank();
    out.context_tokens_before = read.before;
    out.context_tokens_after = read.after;
    out.usage = read.usage.clone();
    let status = match driven.end {
        End::Cancelled => CompactStatus::Cancelled,
        End::Exited(Some(0)) if read.result == compact::ClaudeResult::Succeeded => {
            match read.session_id.filter(|id| id != token) {
                Some(fork) => {
                    out.token = Some(fork);
                    CompactStatus::Completed
                }
                None => {
                    out.error =
                        Some("claude-code compacted without naming a forked session".into());
                    CompactStatus::Failed
                }
            }
        }
        end => {
            out.error = Some(match read.result {
                compact::ClaudeResult::Failed(Some(said)) => said,
                _ => end.describe(&driven.stderr),
            });
            CompactStatus::Failed
        }
    };
    (status, out)
}

/// `codex app-server` with the launch settings every Codex start carries, then
/// the fork-then-compact dialogue.
fn codex(
    bin: &str,
    token: &str,
    model: Option<&str>,
    cfg: &crate::domain::config::FileConfig,
    id: &str,
    launch: &Launch<'_>,
) -> (CompactStatus, CompactOutcome) {
    let spec = harness::by_id("codex").expect("registered");
    let argv = (spec.build_argv)(&BuildCtx {
        bin,
        prompt: "",
        model: None,
        system: None,
        resume: None,
        fork: false,
        mode: PermissionMode::Default,
        output_format: OutputFormat::Json,
        schema: None,
        system_file: None,
        delivery: PromptDelivery::ControlStream,
        plugins: cfg.plugins_for(id),
        apps: cfg.apps_for(id),
        analytics: cfg.analytics_for(id),
        model_catalog: None,
        tools: None,
    });
    let mut dialogue = CodexDialogue::new(token, model);
    let mut ended = None;
    let opening = dialogue.open();
    let driven = drive(&argv, launch, &opening, |line| {
        match dialogue.on_line(line) {
            CodexStep::Send(frames) => Reply::Frames(frames),
            CodexStep::Wait => Reply::Frames(Vec::new()),
            CodexStep::Done(end) => {
                ended = Some(end);
                Reply::Close
            }
        }
    });
    let mut out = blank();
    let status = match (driven.end, ended) {
        (End::Cancelled, _) => CompactStatus::Cancelled,
        // Only a server that then exits cleanly is taken at its word: one
        // that dies or hangs after announcing completion may not have
        // persisted the fork, so the session is not rebound to it.
        (End::Exited(Some(0)), Some(compact::CodexCompaction::Completed { fork, usage })) => {
            out.usage = usage;
            out.token = Some(fork);
            CompactStatus::Completed
        }
        (end, Some(compact::CodexCompaction::Completed { .. })) => {
            out.error = Some(format!(
                "codex reported the compaction complete, but {}; the session was not rebound",
                end.describe(&driven.stderr)
                    .trim_start_matches("the harness ")
            ));
            CompactStatus::Failed
        }
        (_, Some(compact::CodexCompaction::Failed { error })) => {
            out.error = Some(error);
            CompactStatus::Failed
        }
        (end, None) => {
            out.error = Some(end.describe(&driven.stderr));
            CompactStatus::Failed
        }
    };
    (status, out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Exited(Option<i32>),
    Cancelled,
    TimedOut,
    SpawnFailed,
}

impl End {
    fn describe(self, stderr: &str) -> String {
        let said = stderr.trim();
        let said = if said.is_empty() {
            String::new()
        } else {
            format!(": {}", said.lines().last().unwrap_or(said))
        };
        match self {
            End::Exited(code) => format!(
                "the harness exited {} before the compaction completed{said}",
                code.map_or_else(|| "on a signal".to_string(), |c| format!("with code {c}"))
            ),
            End::TimedOut => format!("the compaction exceeded its timeout{said}"),
            End::SpawnFailed => format!("the harness could not be started{said}"),
            End::Cancelled => "cancelled".to_string(),
        }
    }
}

struct Driven {
    end: End,
    stdout: String,
    stderr: String,
}

/// What the driver does after a stdout line.
enum Reply {
    /// Write these frames (possibly none) and keep reading.
    Frames(Vec<String>),
    /// The conversation is over: close stdin, so a server exits on EOF.
    Close,
}

/// After the launcher exits, how long lines it already wrote are still read
/// for, so a reply buffered in the pipe is not lost — and a descendant that
/// kept stdout open cannot hold the compaction past it.
const EXIT_DRAIN_GRACE: Duration = Duration::from_millis(500);

/// Run `argv` with `opening` written to its stdin, answering each stdout line
/// with `on_line`'s [`Reply`]. Bounded by the launch's deadline and cancel
/// token, both of which end the whole process tree, and by the launcher's
/// own exit: once it exits, stdout is read only for [`EXIT_DRAIN_GRACE`].
fn drive(
    argv: &[String],
    launch: &Launch<'_>,
    opening: &[String],
    mut on_line: impl FnMut(&str) -> Reply,
) -> Driven {
    let mut command = Command::new(crate::io::process::resolve_program(&argv[0]));
    command
        .args(&argv[1..])
        .stdin(if opening.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &launch.cwd {
        command.current_dir(dir);
    }
    for key in &launch.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &launch.env {
        command.env(key, value);
    }
    let mut process = match Process::spawn_supervised(command, launch.supervisor) {
        Ok(process) => process,
        Err(err) => {
            return Driven {
                end: End::SpawnFailed,
                stdout: String::new(),
                stderr: err.to_string(),
            }
        }
    };
    let mut stdin = process.take_stdin();
    let write = |stdin: &mut Option<std::process::ChildStdin>, frames: &[String]| {
        for frame in frames {
            if let Some(pipe) = stdin.as_mut() {
                if pipe
                    .write_all(frame.as_bytes())
                    .and_then(|()| pipe.flush())
                    .is_err()
                {
                    *stdin = None;
                    return;
                }
            }
        }
    };
    write(&mut stdin, opening);
    let mut pending: Vec<u8> = Vec::new();
    let mut stdout_open = true;
    // The launcher's exit code once it has exited, and until when its
    // already-written lines are still read.
    let mut exited: Option<(Option<i32>, Instant)> = None;
    let end = loop {
        if cancel::cancellation_requested(launch.cancel) {
            break End::Cancelled;
        }
        let now = Instant::now();
        if launch.deadline.is_some_and(|deadline| now >= deadline) {
            break End::TimedOut;
        }
        if let Some((code, until)) = exited {
            if !stdout_open || now >= until {
                break End::Exited(code);
            }
        } else {
            match process.wait_until(now + Duration::from_millis(1)) {
                Ok(Some(status)) => exited = Some((status.code(), now + EXIT_DRAIN_GRACE)),
                Ok(None) => {}
                Err(_) => break End::Exited(None),
            }
        }
        let slice = now + CANCEL_POLL_SLICE;
        let slice = launch
            .deadline
            .map_or(slice, |deadline| slice.min(deadline));
        let slice = exited.map_or(slice, |(_, until)| slice.min(until));
        if !stdout_open {
            std::thread::sleep(slice.saturating_duration_since(Instant::now()));
            continue;
        }
        match process.recv_stdout_until(slice) {
            PipeEvent::Data(bytes) => {
                pending.extend_from_slice(&bytes);
                while let Some(at) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=at).collect();
                    match on_line(&String::from_utf8_lossy(&line)) {
                        Reply::Frames(frames) => write(&mut stdin, &frames),
                        Reply::Close => stdin = None,
                    }
                }
            }
            PipeEvent::Closed => stdout_open = false,
            PipeEvent::Deadline => {}
        }
    };
    drop(stdin);
    let finished = process.finish(match end {
        End::Exited(_) => Finish::Exited,
        _ => Finish::Terminate,
    });
    Driven {
        end,
        stdout: finished.stdout,
        stderr: finished.stderr,
    }
}
