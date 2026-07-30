//! The zero-turn subscription-headroom probes.
//!
//! Every probe here reads a harness's plan headroom **without the harness taking
//! a model turn** — no user message is written and no turn is completed. That is
//! the property that makes `oneharness usage` usable as a pre-flight check
//! rather than a thing that costs what it measures, and it is a property of the
//! *invocations below*, so any change to one has to preserve it:
//!
//! - **claude-code** — driven in stream-json input/output mode with an empty
//!   tool set, sent exactly one `get_usage` control request, read for the
//!   matching control response, then terminated. Observed to report
//!   `num_turns: 0` / `total_cost_usd: 0` because no user message is ever sent.
//! - **codex** — `codex app-server --stdio`, driven `initialize` →
//!   `initialized` → `account/rateLimits/read`. Its two distinct authentication
//!   error strings are discriminated by [`crate::domain::usage`].
//! - **copilot** — an out-of-band authenticated `GET /copilot_internal/user`,
//!   which needs no Copilot binary at all: a GitHub bearer token is the entire
//!   credential requirement.
//! - **cursor** — `about --format json`, read for the plan tier only, and only
//!   from a **pre-existing** login (see [`CURSOR_LOGIN_ENVS`]).
//!
//! Parsing is pure and lives in [`crate::domain::usage`]; this module only
//! spawns, writes, reads, and hands bytes over. Every failure — a missing
//! binary, an unauthenticated harness, a malformed payload, a timeout — becomes
//! data in the report, never a panic and never a zero.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::domain::usage::{
    claude_control_response, claude_usage_drift, parse_claude_get_usage, parse_codex_rate_limits,
    parse_copilot_http, parse_cursor_about, without_control_chars, IdentitySelector, ParsedUsage,
    UnknownReason, UsageProbe,
};
use crate::io::process::{Finish, PipeEvent, Process};

/// The environment variable selecting a Claude Code identity. Per-process, and
/// observed returning different reset days and different credit histories for
/// two subscriptions on one host — which is what makes per-identity attribution
/// real rather than nominal.
pub const CLAUDE_IDENTITY_ENV: &str = "CLAUDE_CONFIG_DIR";
/// The environment variable selecting a codex identity, per-process like
/// [`CLAUDE_IDENTITY_ENV`].
pub const CODEX_IDENTITY_ENV: &str = "CODEX_HOME";
/// GitHub bearer tokens in the precedence Copilot itself documents. The first
/// non-empty one wins, and only its *name* ever reaches the report.
pub const COPILOT_TOKEN_ENVS: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];
/// Cursor's API-key variables, which the probe **removes** from its child.
///
/// This is a safety guard, not a preference. `--api-key`/`CURSOR_API_KEY` is not
/// a per-process selector: it performs a token exchange and calls
/// `setAuthentication`, persisting credentials to the shared store — observed
/// creating `~/.config/cursor/auth.json` where none existed and clobbering a
/// real user login. `about` ignores the key today; masking it means a future
/// release that honors it still cannot make this probe authenticate.
pub const CURSOR_LOGIN_ENVS: &[&str] = &["CURSOR_API_KEY"];

/// The GitHub API base for the Copilot probe. Overridable with
/// `ONEHARNESS_COPILOT_API_BASE` for a GitHub Enterprise host (and for the
/// hermetic tests, which point it at a local server).
pub const COPILOT_API_BASE_ENV: &str = "ONEHARNESS_COPILOT_API_BASE";
const COPILOT_API_BASE_DEFAULT: &str = "https://api.github.com";
/// The program that performs the Copilot HTTP GET. curl is used rather than an
/// in-process TLS stack so the engine keeps its lean dependency tree; when it is
/// absent the probe reports that as data, like any other probe failure.
const COPILOT_HTTP_CLIENT: &str = "curl";
/// Marks the status line curl appends after the body, so the two are separable
/// without a second request. Deliberately unlike anything the endpoint returns.
const COPILOT_STATUS_MARKER: &str = "oneharness-http-status:";

/// The single `get_usage` control request's id, echoed back in the response so a
/// concurrent control message can never be mistaken for the answer.
const CLAUDE_REQUEST_ID: &str = "oneharness-usage-1";
/// The JSON-RPC id of codex's `account/rateLimits/read` request.
const CODEX_RATE_LIMITS_ID: i64 = 2;
/// How long a child that has already answered gets to exit on its own before its
/// tree is terminated. Bounded so an idling harness cannot hold the probe.
const EXIT_GRACE: Duration = Duration::from_millis(500);
/// The longest a probe may wait, in seconds — the ceiling every caller gets,
/// not just the CLI. A probe is a pre-flight check whose harness answers in
/// single-digit seconds; an hour is already far past "something is wrong".
///
/// The CLI reads this same constant for its `--timeout` range, so the documented
/// maximum has one source. Enforcing it here as well is what makes it a real
/// boundary: [`probe`] is public API, and a library caller handing it a
/// multi-year `Duration` would otherwise hang a process, or — since
/// `Instant + Duration` **panics** on overflow — take one down.
pub const MAX_TIMEOUT_SECS: u64 = 3_600;
/// [`MAX_TIMEOUT_SECS`] as a [`Duration`].
pub const MAX_TIMEOUT: Duration = Duration::from_secs(MAX_TIMEOUT_SECS);

/// One identity to probe: which probe, which binary, and the exact environment
/// the child gets — the same variant environment `run` builds, so a usage
/// identity is selected by the machinery that already selects a run's identity.
pub struct UsageProbeRequest {
    pub probe: UsageProbe,
    /// The resolved harness binary (unused by [`UsageProbe::CopilotUserEndpoint`],
    /// which spawns no harness).
    pub bin: String,
    pub cwd: Option<PathBuf>,
    /// Environment applied to the child, last write winning — a variant's `env`,
    /// `env_file`, and `env_from`, exactly as `run` assembles them.
    pub env: Vec<(String, String)>,
    /// Variables masked from the child. Applied after [`Self::env`], so a
    /// removal wins over a set of the same name — matching the runner.
    pub env_remove: Vec<String>,
    /// How long to wait for this probe's answer. **Clamped to [`MAX_TIMEOUT`]**
    /// — see [`UsageProbeRequest::effective_timeout`], which is what the probe
    /// and its diagnostics actually use.
    pub timeout: Duration,
}

impl UsageProbeRequest {
    /// The timeout this request is actually run under: the requested value, or
    /// [`MAX_TIMEOUT`] when it exceeds the ceiling. Every deadline and every
    /// timeout message reads from here, so a clamped request is *reported* as
    /// the value it was run under rather than the one that was asked for.
    #[must_use]
    pub fn effective_timeout(&self) -> Duration {
        self.timeout.min(MAX_TIMEOUT)
    }
}

/// One probed identity: how it was selected, and what was learned.
pub struct ProbedIdentity {
    pub selector: IdentitySelector,
    pub parsed: ParsedUsage,
}

/// Probe one identity. Never fails loudly: every outcome is a [`ParsedUsage`].
#[must_use]
pub fn probe(request: &UsageProbeRequest) -> ProbedIdentity {
    match request.probe {
        UsageProbe::ClaudeGetUsage => probe_claude(request),
        UsageProbe::CodexAppServer => probe_codex(request),
        UsageProbe::CopilotUserEndpoint => probe_copilot(request),
        UsageProbe::CursorAbout => probe_cursor(request),
    }
}

/// The identity an unprobed or unavailable harness is attributed to, so its
/// entry still names the selector a probe *would* have used.
#[must_use]
pub fn selector_for(probe: Option<UsageProbe>, request_env: &EnvView<'_>) -> IdentitySelector {
    match probe {
        Some(UsageProbe::ClaudeGetUsage) => env_path_selector(request_env, CLAUDE_IDENTITY_ENV),
        Some(UsageProbe::CodexAppServer) => env_path_selector(request_env, CODEX_IDENTITY_ENV),
        Some(UsageProbe::CopilotUserEndpoint) => match copilot_token_env(request_env) {
            Some((env, _)) => IdentitySelector::EnvSecret {
                env: env.to_string(),
            },
            None => IdentitySelector::Ambient,
        },
        Some(UsageProbe::CursorAbout) | None => IdentitySelector::Ambient,
    }
}

/// The child's effective view of the environment: the request's overrides
/// layered over the ambient process environment, with removals applied last
/// (exactly the order [`crate::io::runner`] applies them to a `Command`).
pub struct EnvView<'a> {
    env: &'a [(String, String)],
    env_remove: &'a [String],
}

impl<'a> EnvView<'a> {
    #[must_use]
    pub fn new(env: &'a [(String, String)], env_remove: &'a [String]) -> Self {
        Self { env, env_remove }
    }

    /// The value the child would see for `name`, treating empty as unset (an
    /// empty `CLAUDE_CONFIG_DIR` selects nothing).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<String> {
        if self.env_remove.iter().any(|key| key == name) {
            return None;
        }
        self.env
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
            .filter(|value| !value.is_empty())
    }
}

impl UsageProbeRequest {
    fn env_view(&self) -> EnvView<'_> {
        EnvView::new(&self.env, &self.env_remove)
    }
}

fn env_path_selector(env: &EnvView<'_>, name: &str) -> IdentitySelector {
    match env.get(name) {
        Some(path) => IdentitySelector::EnvPath {
            env: name.to_string(),
            path,
        },
        None => IdentitySelector::Ambient,
    }
}

fn copilot_token_env(env: &EnvView<'_>) -> Option<(&'static str, String)> {
    COPILOT_TOKEN_ENVS
        .iter()
        .find_map(|name| env.get(name).map(|value| (*name, value)))
}

/// What one probe subprocess produced.
struct ProbeCapture {
    /// The answer an `on_line` callback recognized, if any.
    answer: Option<ParsedUsage>,
    stdout: String,
    stderr: String,
    /// Whether the deadline expired before the child answered or exited.
    timed_out: bool,
    /// A spawn failure, which short-circuits everything else.
    spawn_error: Option<String>,
}

impl ProbeCapture {
    /// The failure this capture represents when no answer was recognized:
    /// the spawn error, a timeout, or the child's own stderr.
    fn failure(&self, what: &str, timeout: Duration) -> ParsedUsage {
        if let Some(error) = &self.spawn_error {
            return ParsedUsage::unknown(UnknownReason::ProbeFailed {
                message: error.clone(),
            });
        }
        let message = if self.timed_out {
            format!(
                "{what} did not answer within {}s (raise --timeout if the harness is slow to start)",
                timeout.as_secs()
            )
        } else {
            let detail = first_meaningful_line(&self.stderr)
                .or_else(|| first_meaningful_line(&self.stdout))
                .unwrap_or_else(|| "no output".to_string());
            format!("{what} exited without an answer: {detail}")
        };
        ParsedUsage::unknown(UnknownReason::ProbeFailed { message })
    }
}

/// Bound on a diagnostic excerpt of a harness's own output, in **characters**
/// so a multi-byte line is never split mid-code-point.
const DIAGNOSTIC_CHARS: usize = 300;

fn first_meaningful_line(text: &str) -> Option<String> {
    let line = text.lines().find_map(|line| {
        let flat = without_control_chars(line).trim().to_string();
        (!flat.is_empty()).then_some(flat)
    })?;
    Some(match line.char_indices().nth(DIAGNOSTIC_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    })
}

/// Spawn `argv`, write `stdin_lines` (each newline-terminated) and close stdin,
/// then read stdout line by line until `on_line` recognizes an answer, the child
/// exits, or the deadline passes. The process tree is always terminated on the
/// way out, so a harness that would idle waiting for more input cannot outlive
/// the probe.
fn converse(
    request: &UsageProbeRequest,
    argv: &[String],
    stdin_lines: &[String],
    mut on_line: impl FnMut(&str) -> Option<ParsedUsage>,
) -> ProbeCapture {
    // Clamped at the boundary, so this addition cannot overflow whatever a
    // caller asked for.
    let deadline = Instant::now() + request.effective_timeout();
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &request.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &request.env {
        command.env(key, value);
    }
    for key in &request.env_remove {
        command.env_remove(key);
    }

    let mut process = match Process::spawn(command) {
        Ok(process) => process,
        Err(error) => {
            return ProbeCapture {
                answer: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
                spawn_error: Some(format!("failed to spawn `{}`: {error}", argv[0])),
            }
        }
    };

    // Written inline rather than on a helper thread: the payload is a handful of
    // short lines built here, orders of magnitude below any pipe buffer, so this
    // write cannot block. Dropping the handle closes the pipe, which is the EOF
    // both drivers this was sourced from relied on to let the child finish.
    if let Some(mut stdin) = process.take_stdin() {
        for line in stdin_lines {
            if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() {
                break;
            }
        }
        let _ = stdin.flush();
    }

    let mut answer = None;
    let mut timed_out = false;
    loop {
        match process.recv_stdout_until(deadline) {
            PipeEvent::Data(chunk) => {
                let line = String::from_utf8_lossy(&chunk);
                if let Some(parsed) = on_line(line.trim()) {
                    answer = Some(parsed);
                    break;
                }
            }
            PipeEvent::Closed => break,
            PipeEvent::Deadline => {
                timed_out = true;
                break;
            }
        }
    }

    // The probe has what it came for and the child's stdin is already closed, so
    // a well-behaved harness is on its way out: give it a bounded moment to exit
    // by itself rather than signalling a process that is mid-shutdown (which
    // costs it whatever it flushes at exit). A harness that idles anyway — or one
    // that blew the deadline — still gets its whole tree torn down.
    let finish = if timed_out {
        Finish::Terminate
    } else {
        match process.wait_until(Instant::now() + EXIT_GRACE) {
            Ok(Some(_)) => Finish::Exited,
            Ok(None) | Err(_) => Finish::Terminate,
        }
    };
    let finished = process.finish(finish);
    ProbeCapture {
        answer,
        stdout: finished.stdout,
        stderr: finished.stderr,
        timed_out,
        spawn_error: None,
    }
}

/// The exact zero-turn invocation: `-p` with stream-json in and out, an empty
/// tool set, and no prompt. The control request rides stdin; no user message is
/// ever sent, so the session completes zero turns.
fn claude_argv(bin: &str) -> Vec<String> {
    vec![
        bin.to_string(),
        "-p".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--tools".to_string(),
        String::new(),
    ]
}

fn claude_request_line() -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": CLAUDE_REQUEST_ID,
        "request": {"subtype": "get_usage"},
    })
    .to_string()
}

fn probe_claude(request: &UsageProbeRequest) -> ProbedIdentity {
    let selector = env_path_selector(&request.env_view(), CLAUDE_IDENTITY_ENV);
    let mut capture = converse(
        request,
        &claude_argv(&request.bin),
        &[claude_request_line()],
        |line| {
            let value: Value = serde_json::from_str(line).ok()?;
            if value
                .pointer("/response/request_id")
                .and_then(Value::as_str)
                != Some(CLAUDE_REQUEST_ID)
            {
                return None;
            }
            let payload = claude_control_response(&value)?;
            // The drift guard runs before the parser: with no schema to diff
            // against, a renamed field would otherwise take the parser's
            // absence-means-false branch and publish "no headroom" as fact.
            Some(match claude_usage_drift(payload) {
                Some(reason) => ParsedUsage::unknown(UnknownReason::ProbeFailed {
                    message: format!("claude-code's `get_usage` payload changed shape: {reason}"),
                }),
                None => parse_claude_get_usage(payload),
            })
        },
    );
    let parsed = capture.answer.take().unwrap_or_else(|| {
        capture.failure(
            "claude-code's `get_usage` control request",
            request.effective_timeout(),
        )
    });
    ProbedIdentity { selector, parsed }
}

fn codex_argv(bin: &str) -> Vec<String> {
    vec![
        bin.to_string(),
        "app-server".to_string(),
        "--stdio".to_string(),
    ]
}

/// `initialize`, the `initialized` notification, then the rate-limits read. All
/// three are written before reading, matching the driver this was sourced from;
/// the response is matched by JSON-RPC id, so ordering is not assumed.
///
/// Calling convention detail worth preserving: `account/rateLimits/read` takes
/// `params: null` (its sibling `account/read` instead *requires* `params: {}`).
fn codex_request_lines() -> Vec<String> {
    vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "oneharness",
                    "title": null,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": null,
            },
        })
        .to_string(),
        serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}).to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": CODEX_RATE_LIMITS_ID,
            "method": "account/rateLimits/read",
            "params": null,
        })
        .to_string(),
    ]
}

fn probe_codex(request: &UsageProbeRequest) -> ProbedIdentity {
    let selector = env_path_selector(&request.env_view(), CODEX_IDENTITY_ENV);
    let mut capture = converse(
        request,
        &codex_argv(&request.bin),
        &codex_request_lines(),
        {
            |line| {
                let value: Value = serde_json::from_str(line).ok()?;
                if value.get("id").and_then(Value::as_i64) != Some(CODEX_RATE_LIMITS_ID) {
                    return None;
                }
                Some(parse_codex_rate_limits(&value))
            }
        },
    );
    let parsed = capture.answer.take().unwrap_or_else(|| {
        capture.failure(
            "codex's `account/rateLimits/read`",
            request.effective_timeout(),
        )
    });
    ProbedIdentity { selector, parsed }
}

fn cursor_argv(bin: &str) -> Vec<String> {
    vec![
        bin.to_string(),
        "about".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ]
}

fn probe_cursor(request: &UsageProbeRequest) -> ProbedIdentity {
    // `about` prints one pretty-printed document, so it is parsed after exit
    // rather than line by line.
    let guarded = UsageProbeRequest {
        probe: request.probe,
        bin: request.bin.clone(),
        cwd: request.cwd.clone(),
        env: request.env.clone(),
        env_remove: request
            .env_remove
            .iter()
            .cloned()
            .chain(CURSOR_LOGIN_ENVS.iter().map(|name| (*name).to_string()))
            .collect(),
        timeout: request.timeout,
    };
    let capture = converse(&guarded, &cursor_argv(&request.bin), &[], |_| None);
    let parsed = match serde_json::from_str::<Value>(capture.stdout.trim()) {
        Ok(value) if value.is_object() => parse_cursor_about(&value),
        _ => capture.failure(
            "cursor's `about --format json`",
            request.effective_timeout(),
        ),
    };
    ProbedIdentity {
        selector: IdentitySelector::Ambient,
        parsed,
    }
}

fn probe_copilot(request: &UsageProbeRequest) -> ProbedIdentity {
    let env = request.env_view();
    let token = copilot_token_env(&env);
    let selector = match &token {
        Some((name, _)) => IdentitySelector::EnvSecret {
            env: (*name).to_string(),
        },
        None => IdentitySelector::Ambient,
    };
    let Some((_, token)) = token else {
        return ProbedIdentity {
            selector,
            parsed: ParsedUsage::unknown(UnknownReason::ProbeFailed {
                message: format!(
                    "no GitHub token to read Copilot quota with: set one of {}. \
                     Copilot's own stored OAuth login lives in the OS keyring, which \
                     oneharness cannot read",
                    COPILOT_TOKEN_ENVS.join(", ")
                ),
            }),
        };
    };
    let base = env
        .get(COPILOT_API_BASE_ENV)
        .unwrap_or_else(|| COPILOT_API_BASE_DEFAULT.to_string());
    let parsed = match copilot_config(&base, &token) {
        Ok(config) => copilot_fetch(request, config),
        Err(message) => ParsedUsage::unknown(UnknownReason::ProbeFailed { message }),
    };
    ProbedIdentity { selector, parsed }
}

/// Build the curl config read from stdin. The token never reaches the argv (and
/// so never reaches the process table); both it and the base URL are validated
/// first, because a quote or newline in either would otherwise be injected into
/// curl's own config grammar.
fn copilot_config(base: &str, token: &str) -> Result<String, String> {
    let base = base.trim_end_matches('/');
    if !(base.starts_with("https://") || base.starts_with("http://")) || !is_config_safe(base) {
        return Err(format!(
            "{COPILOT_API_BASE_ENV} must be an http(s) URL with no quotes, backslashes, \
             whitespace, or control characters (got `{base}`)"
        ));
    }
    if !is_config_safe(token) {
        // The value itself is never echoed.
        return Err(
            "the GitHub token contains quotes, backslashes, whitespace, or control characters \
             and cannot be forwarded safely"
                .to_string(),
        );
    }
    Ok(format!(
        "url = \"{base}/copilot_internal/user\"\n\
         header = \"Authorization: Bearer {token}\"\n\
         header = \"Accept: application/json\"\n\
         header = \"User-Agent: oneharness/{}\"\n\
         silent\n\
         show-error\n\
         write-out = \"\\n{COPILOT_STATUS_MARKER}%{{http_code}}\\n\"\n",
        env!("CARGO_PKG_VERSION")
    ))
}

fn is_config_safe(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '\\')
}

fn copilot_fetch(request: &UsageProbeRequest, config: String) -> ParsedUsage {
    let argv = vec![
        COPILOT_HTTP_CLIENT.to_string(),
        "--config".to_string(),
        "-".to_string(),
    ];
    let capture = converse(request, &argv, &[config], |_| None);
    match split_copilot_response(&capture.stdout) {
        Some((status, body)) => parse_copilot_http(status, &body),
        None => capture.failure(
            &format!("the Copilot quota request via `{COPILOT_HTTP_CLIENT}`"),
            request.effective_timeout(),
        ),
    }
}

/// Split curl's output into the response body and its status code. The marker
/// line is written by curl *after* the body, so the last one wins — a body that
/// happened to contain the marker cannot displace the real status.
fn split_copilot_response(stdout: &str) -> Option<(u16, String)> {
    let (at, line) = stdout
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with(COPILOT_STATUS_MARKER))
        .last()?;
    let status: u16 = line
        .trim()
        .trim_start_matches(COPILOT_STATUS_MARKER)
        .parse()
        .ok()?;
    let body = stdout.lines().take(at).collect::<Vec<_>>().join("\n");
    Some((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(env: &'a [(String, String)], remove: &'a [String]) -> EnvView<'a> {
        EnvView::new(env, remove)
    }

    fn request(timeout: Duration) -> UsageProbeRequest {
        UsageProbeRequest {
            probe: UsageProbe::CursorAbout,
            bin: "cursor-agent".to_string(),
            cwd: None,
            env: Vec::new(),
            env_remove: Vec::new(),
            timeout,
        }
    }

    #[test]
    fn a_library_caller_gets_the_documented_timeout_ceiling_too() {
        // `probe` is public API, so the ceiling has to be enforced here rather
        // than only by the CLI's flag range: a sibling tool depending on the
        // engine would otherwise hang its own process for as long as it asked.
        assert_eq!(
            request(Duration::from_secs(MAX_TIMEOUT_SECS * 24)).effective_timeout(),
            MAX_TIMEOUT,
            "an over-ceiling request runs under the ceiling"
        );
        assert_eq!(
            request(Duration::MAX).effective_timeout(),
            MAX_TIMEOUT,
            "including one that would overflow the deadline arithmetic outright"
        );

        // A deadline built from the clamped value cannot overflow, which is the
        // panic this boundary exists to make unreachable.
        assert!(Instant::now()
            .checked_add(request(Duration::MAX).effective_timeout())
            .is_some());

        // Anything at or under the ceiling is honored exactly — the clamp is a
        // ceiling, not a rewrite of every timeout.
        for secs in [1, 60, MAX_TIMEOUT_SECS] {
            assert_eq!(
                request(Duration::from_secs(secs)).effective_timeout(),
                Duration::from_secs(secs)
            );
        }
    }

    #[test]
    fn a_clamped_probe_reports_the_timeout_it_actually_waited() {
        // The diagnostic must not quote a duration nothing waited for: a caller
        // reading "did not answer within 157680000s" would go looking for an
        // hours-long hang that never happened.
        let capture = ProbeCapture {
            answer: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
            spawn_error: None,
        };
        let over_ceiling = request(Duration::from_secs(MAX_TIMEOUT_SECS * 10));

        let parsed = capture.failure("a probe", over_ceiling.effective_timeout());

        let UnknownReason::ProbeFailed { message } = (match parsed.availability {
            crate::domain::usage::UsageAvailability::Unknown { reason } => reason,
            other => panic!("expected a probe failure, got {other:?}"),
        }) else {
            panic!("expected a probe failure");
        };
        assert!(
            message.contains(&format!("{MAX_TIMEOUT_SECS}s")),
            "the message must quote the enforced timeout: {message}"
        );
    }

    #[test]
    fn a_removed_variable_is_invisible_to_the_child_even_when_also_set() {
        // The runner applies removals last, so a removal wins over a set of the
        // same name; the selector must describe what the child actually sees.
        let env = vec![(CLAUDE_IDENTITY_ENV.to_string(), "/tmp/a".to_string())];
        let remove = vec![CLAUDE_IDENTITY_ENV.to_string()];

        assert_eq!(view(&env, &remove).get(CLAUDE_IDENTITY_ENV), None);
        assert_eq!(
            view(&env, &[]).get(CLAUDE_IDENTITY_ENV),
            Some("/tmp/a".to_string())
        );
    }

    #[test]
    fn the_last_write_of_a_variable_wins_and_an_empty_value_is_unset() {
        let env = vec![
            (CODEX_IDENTITY_ENV.to_string(), "/tmp/one".to_string()),
            (CODEX_IDENTITY_ENV.to_string(), "/tmp/two".to_string()),
        ];
        assert_eq!(
            view(&env, &[]).get(CODEX_IDENTITY_ENV),
            Some("/tmp/two".to_string())
        );

        let empty = vec![(CODEX_IDENTITY_ENV.to_string(), String::new())];
        assert_eq!(view(&empty, &[]).get(CODEX_IDENTITY_ENV), None);
    }

    #[test]
    fn the_identity_selector_names_the_directory_never_a_credential() {
        let env = vec![(
            CLAUDE_IDENTITY_ENV.to_string(),
            "/home/u/.claude".to_string(),
        )];
        assert_eq!(
            selector_for(Some(UsageProbe::ClaudeGetUsage), &view(&env, &[])),
            IdentitySelector::EnvPath {
                env: CLAUDE_IDENTITY_ENV.to_string(),
                path: "/home/u/.claude".to_string(),
            }
        );

        let token = vec![("GH_TOKEN".to_string(), "ghs_secret".to_string())];
        let selector = selector_for(Some(UsageProbe::CopilotUserEndpoint), &view(&token, &[]));
        assert_eq!(
            selector,
            IdentitySelector::EnvSecret {
                env: "GH_TOKEN".to_string()
            }
        );
        assert!(
            !selector.key().contains("ghs_secret"),
            "a credential must never reach the report"
        );
    }

    #[test]
    fn copilot_token_precedence_follows_the_documented_order() {
        let all = vec![
            ("GITHUB_TOKEN".to_string(), "c".to_string()),
            ("GH_TOKEN".to_string(), "b".to_string()),
            ("COPILOT_GITHUB_TOKEN".to_string(), "a".to_string()),
        ];
        assert_eq!(
            copilot_token_env(&view(&all, &[])),
            Some(("COPILOT_GITHUB_TOKEN", "a".to_string()))
        );

        let removed = vec!["COPILOT_GITHUB_TOKEN".to_string()];
        assert_eq!(
            copilot_token_env(&view(&all, &removed)),
            Some(("GH_TOKEN", "b".to_string()))
        );
    }

    #[test]
    fn the_claude_probe_sends_one_control_request_and_no_user_message() {
        let argv = claude_argv("claude");
        assert_eq!(
            argv,
            vec![
                "claude",
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--tools",
                "",
            ],
            "the zero-turn invocation is load-bearing: an empty tool set and no prompt"
        );

        let line = claude_request_line();
        let value: Value = serde_json::from_str(&line).expect("a JSON line");
        assert_eq!(value["type"], "control_request");
        assert_eq!(value["request"]["subtype"], "get_usage");
        assert!(
            !line.contains("\"user\""),
            "sending a user message would make the probe cost a model turn"
        );
    }

    #[test]
    fn the_codex_probe_initializes_then_reads_rate_limits() {
        let lines = codex_request_lines();
        assert_eq!(lines.len(), 3);
        let parsed: Vec<Value> = lines
            .iter()
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect();
        assert_eq!(parsed[0]["method"], "initialize");
        assert_eq!(parsed[1]["method"], "initialized");
        assert_eq!(parsed[2]["method"], "account/rateLimits/read");
        assert_eq!(parsed[2]["id"], CODEX_RATE_LIMITS_ID);
        assert!(
            parsed[2]["params"].is_null(),
            "account/rateLimits/read takes params: null (account/read instead requires {{}})"
        );
    }

    #[test]
    fn the_copilot_config_keeps_the_token_off_the_argv() {
        let config = copilot_config("https://api.github.com/", "ghs_abc").expect("a safe config");

        assert!(config.contains("url = \"https://api.github.com/copilot_internal/user\""));
        assert!(config.contains("header = \"Authorization: Bearer ghs_abc\""));
        let argv = [
            COPILOT_HTTP_CLIENT.to_string(),
            "--config".to_string(),
            "-".to_string(),
        ];
        assert!(
            !argv.iter().any(|arg| arg.contains("ghs_abc")),
            "the token rides stdin so it never reaches the process table"
        );
    }

    #[test]
    fn an_injectable_token_or_base_is_refused_without_echoing_the_value() {
        let error = copilot_config("https://api.github.com", "gh\"\nheader = \"X: y")
            .expect_err("a token carrying config syntax is refused");
        assert!(!error.contains("header = "), "the value must not be echoed");

        assert!(copilot_config("ftp://elsewhere", "ghs_abc").is_err());
        assert!(copilot_config("https://a b", "ghs_abc").is_err());
    }

    #[test]
    fn the_status_marker_is_read_from_the_last_line_not_a_body_that_mimics_it() {
        let stdout = format!(
            "{{\"copilot_plan\":\"individual\",\"note\":\"{COPILOT_STATUS_MARKER}999\"}}\n\
             {COPILOT_STATUS_MARKER}200\n"
        );

        let (status, body) = split_copilot_response(&stdout).expect("a status and a body");

        assert_eq!(status, 200);
        assert!(body.contains("copilot_plan"));
        assert_eq!(split_copilot_response("no marker at all"), None);
    }
}
