//! The shared Codex app-server's pure rules: which environment the server is
//! launched with, what keys its pool entry, how a dispatch's own environment
//! reaches its thread's commands, and which frames on a shared connection are a
//! dispatch's own.
//!
//! A shared server's environment reaches every thread's commands, whichever
//! dispatch they belong to, so it is launched with a scrubbed one: the
//! platform's base variables plus [`CODEX_SERVER_ENV`] as the launching
//! dispatch's variant environment resolves them. Everything else a dispatch
//! carries travels per thread, in `thread/start`'s and `thread/resume`'s
//! `config` map as `shell_environment_policy.set`, and a server variable the
//! dispatch lacks travels as `shell_environment_policy.exclude` — so its
//! thread's commands see exactly its environment, never another dispatch's.
//!
//! No I/O here: the caller hands in the environments it read and the version
//! output it probed.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::domain::config::{
    APP_SERVER_BASE_ENV_UNIX, APP_SERVER_BASE_ENV_UNIX_PREFIX, APP_SERVER_BASE_ENV_WINDOWS,
    CODEX_SERVER_ENV, CODEX_SERVER_ENV_PREFIX,
};
use crate::domain::control::{pool_key as control_pool_key, PoolKey};

/// The variable naming a dispatch's own commands: a fresh UUID per dispatch,
/// carried in its thread's `shell_environment_policy.set`, so every process its
/// turn starts can be found and stopped by it.
pub const DISPATCH_MARKER_VAR: &str = "ONEHARNESS_DISPATCH_MARKER";

/// The variable naming a shared server's own processes (the launcher shim and
/// the native server it starts): a fresh UUID per server, recorded in the pool
/// entry's `server.json`.
pub const SERVER_MARKER_VAR: &str = "ONEHARNESS_APP_SERVER_MARKER";

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Real Codex cannot be read in the deterministic gate; the live phases scripts/e2e-control.sh runs on every live-control leg drive these requests against it — pooled-reuse sends thread/start, thread/shellCommand, thread/resume, thread/unsubscribe and thread/loaded/list and fails naming a released thread still loaded, and pooled-env and pooled-kill drive the thread lifecycle and its environment — which is this contract's drift alarm.
/// The JSON-RPC request that releases a dispatch's thread from the server once
/// its lease ends. Codex keeps a finished thread loaded until its last
/// subscriber unsubscribes (`thread/unsubscribe` in codex-rs's app-server v2
/// protocol, `ThreadUnsubscribeParams { threadId }`), then unloads it and emits
/// `thread/closed`.
pub const RELEASE_THREAD_METHOD: &str = "thread/unsubscribe";
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

/// Which platform's environment rules apply: the base variable set, and whether
/// names compare case-insensitively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPlatform {
    Unix,
    Windows,
}

impl EnvPlatform {
    /// The platform this build runs on. A compile-time fact, not a read.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(windows) {
            EnvPlatform::Windows
        } else {
            EnvPlatform::Unix
        }
    }

    fn same_name(self, a: &str, b: &str) -> bool {
        match self {
            EnvPlatform::Unix => a == b,
            EnvPlatform::Windows => a.eq_ignore_ascii_case(b),
        }
    }

    /// Whether `name` is one of this platform's base variables.
    fn is_base(self, name: &str) -> bool {
        match self {
            EnvPlatform::Unix => {
                APP_SERVER_BASE_ENV_UNIX.contains(&name)
                    || name.starts_with(APP_SERVER_BASE_ENV_UNIX_PREFIX)
            }
            EnvPlatform::Windows => APP_SERVER_BASE_ENV_WINDOWS
                .iter()
                .any(|base| base.eq_ignore_ascii_case(name)),
        }
    }
}

/// Whether a shared server keeps `name` from its launching dispatch's
/// environment: a base variable, or one Codex reads for itself.
#[must_use]
pub fn in_server_env(name: &str, platform: EnvPlatform) -> bool {
    platform.is_base(name)
        || CODEX_SERVER_ENV
            .iter()
            .any(|codex| platform.same_name(codex, name))
        || match platform {
            EnvPlatform::Unix => name.starts_with(CODEX_SERVER_ENV_PREFIX),
            EnvPlatform::Windows => name
                .get(..CODEX_SERVER_ENV_PREFIX.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(CODEX_SERVER_ENV_PREFIX)),
        }
}

/// The environment a shared server is launched with, from the launching
/// dispatch's resolved one: only the variables [`in_server_env`] keeps, sorted
/// by name so two dispatches that resolved the same values in a different order
/// describe one server. The server marker is added by the launcher, not here —
/// it is fresh per server and must not move the pool key.
#[must_use]
pub fn server_env(
    dispatch_env: &[(String, String)],
    platform: EnvPlatform,
) -> Vec<(String, String)> {
    let mut kept: Vec<(String, String)> = dispatch_env
        .iter()
        .filter(|(name, _)| in_server_env(name, platform))
        .cloned()
        .collect();
    kept.sort();
    kept.dedup_by(|a, b| platform.same_name(&a.0, &b.0));
    kept
}

/// What one dispatch's thread carries so its commands see exactly its own
/// environment on a server launched with another's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellEnvironmentPolicy {
    /// Every variable of the dispatch's environment absent from or differing
    /// from the server's, plus its [`DISPATCH_MARKER_VAR`].
    pub set: BTreeMap<String, String>,
    /// Every server variable the dispatch lacks (or removed through
    /// `unset_env`), the server marker among them.
    pub exclude: Vec<String>,
}

impl ShellEnvironmentPolicy {
    /// The policy for a dispatch whose environment is `dispatch_env`, on a
    /// server whose environment is `server_env` (the marker included), with
    /// `marker` as the dispatch's own marker value.
    #[must_use]
    pub fn for_dispatch(
        dispatch_env: &[(String, String)],
        server_env: &[(String, String)],
        marker: &str,
        platform: EnvPlatform,
    ) -> Self {
        let server_value = |name: &str| {
            server_env
                .iter()
                .find(|(server, _)| platform.same_name(server, name))
                .map(|(_, value)| value.as_str())
        };
        let mut set = BTreeMap::new();
        for (name, value) in dispatch_env {
            if name == DISPATCH_MARKER_VAR || name == SERVER_MARKER_VAR {
                continue;
            }
            if server_value(name) != Some(value.as_str()) {
                set.insert(name.clone(), value.clone());
            }
        }
        set.insert(DISPATCH_MARKER_VAR.to_string(), marker.to_string());
        let mut exclude: Vec<String> = server_env
            .iter()
            .filter(|(name, _)| {
                name == SERVER_MARKER_VAR
                    || !dispatch_env
                        .iter()
                        .any(|(dispatch, _)| platform.same_name(dispatch, name))
            })
            .map(|(name, _)| name.clone())
            .collect();
        exclude.sort();
        exclude.dedup();
        ShellEnvironmentPolicy { set, exclude }
    }

    /// The `config` map a `thread/start` / `thread/resume` carries it in.
    #[must_use]
    pub fn thread_config(&self) -> Value {
        json!({
            "shell_environment_policy": {
                "set": self.set,
                "exclude": self.exclude,
            }
        })
    }
}

/// Everything fixed at a shared server's start, and so its pool key: the
/// executable actually run and what it says it is, the environment it is
/// launched with, every process-level argument after `app-server`, and the
/// resolved `plugins`. Thread-level settings (cwd, model, sandbox, approval
/// policy, instructions) travel per thread and per turn, so dispatches that
/// differ only in those share one server.
#[must_use]
pub fn pool_key(
    executable: &str,
    version_output: &str,
    server_env: &[(String, String)],
    process_args: &[String],
    plugins: bool,
) -> PoolKey {
    let mut material: Vec<(String, Option<String>)> = vec![
        ("executable".to_string(), Some(executable.to_string())),
        (
            "version".to_string(),
            Some(version_output.trim().to_string()),
        ),
        ("plugins".to_string(), Some(plugins.to_string())),
    ];
    material.extend(
        server_env
            .iter()
            .map(|(name, value)| (format!("env:{name}"), Some(value.clone()))),
    );
    control_pool_key("codex-app-server", &material, process_args)
}

/// The arguments that put a server on the socket the pool picked, with the
/// path still spelled `{address}` for the pool to render.
pub const LISTEN_ARGS: [&str; 2] = ["--listen", "unix://{address}"];

/// The thread a server frame belongs to — `params.threadId`, or
/// `params.thread.id` on a frame that carries the whole thread — or `None` for
/// a frame scoped to the connection or the server rather than to a thread.
#[must_use]
pub fn frame_thread(frame: &Value) -> Option<&str> {
    let params = frame.get("params")?;
    params.get("threadId").and_then(Value::as_str).or_else(|| {
        params
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
    })
}

/// Whether a frame a dispatch received on its shared connection is its own:
/// one scoped to no thread (a response to its own request, or a connection- or
/// server-wide notice), or one naming its thread. Before the dispatch knows its
/// thread, a frame naming any thread is not yet known to be its own.
#[must_use]
pub fn is_own_frame(frame: &Value, own_thread: Option<&str>) -> bool {
    match frame_thread(frame) {
        None => true,
        Some(thread) => own_thread == Some(thread),
    }
}

/// How a server answered an interrupt, read off its JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterruptReply {
    /// The interrupt was accepted.
    Accepted,
    /// The server no longer has the thread: `thread not found` or
    /// `thread not loaded` — nothing is running it.
    ThreadGone(String),
    /// Any other refusal, verbatim.
    Refused(String),
}

impl InterruptReply {
    /// Read a `turn/interrupt` response.
    #[must_use]
    pub fn of(response: &Value) -> Self {
        let Some(error) = response.get("error") else {
            return if response.get("result").is_some() {
                InterruptReply::Accepted
            } else {
                InterruptReply::Refused(
                    "the app-server answered the interrupt with neither a result nor an error"
                        .to_string(),
                )
            };
        };
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the app-server refused the interrupt")
            .to_string();
        if message.contains("thread not found") || message.contains("thread not loaded") {
            InterruptReply::ThreadGone(message)
        } else {
            InterruptReply::Refused(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn the_server_keeps_only_base_and_codex_variables() {
        let dispatch = env(&[
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("LC_ALL", "C"),
            ("CODEX_HOME", "/h/.codex"),
            ("CODEX_FOO", "1"),
            ("OPENAI_API_KEY", "k"),
            ("https_proxy", "http://p"),
            ("SSL_CERT_FILE", "/c"),
            ("ONEPIPELINE_RUN_ID", "run-a"),
            ("XDG_RUNTIME_DIR", "/run/a"),
            ("MOCK_KNOB", "1"),
        ]);
        let kept = server_env(&dispatch, EnvPlatform::Unix);
        let names: Vec<&str> = kept.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "CODEX_FOO",
                "CODEX_HOME",
                "HOME",
                "LC_ALL",
                "OPENAI_API_KEY",
                "PATH",
                "SSL_CERT_FILE",
                "https_proxy"
            ]
        );
    }

    #[test]
    fn windows_matches_base_and_codex_names_case_insensitively() {
        let dispatch = env(&[
            ("Path", "C:\\bin"),
            ("SYSTEMROOT", "C:\\Windows"),
            ("codex_home", "C:\\c"),
            ("HOME", "C:\\h"),
        ]);
        let kept = server_env(&dispatch, EnvPlatform::Windows);
        let names: Vec<&str> = kept.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Path", "SYSTEMROOT", "codex_home"]);
    }

    #[test]
    fn a_dispatch_sets_what_differs_and_excludes_what_it_lacks() {
        let server = env(&[
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("CODEX_HOME", "/h/.codex"),
            ("CODEX_ONLY_FIRST", "1"),
            (SERVER_MARKER_VAR, "server-uuid"),
        ]);
        let dispatch = env(&[
            ("PATH", "/bin"),
            ("HOME", "/other"),
            ("CODEX_HOME", "/h/.codex"),
            ("ONEPIPELINE_RUN_ID", "run-b"),
        ]);
        let policy = ShellEnvironmentPolicy::for_dispatch(
            &dispatch,
            &server,
            "lease-uuid",
            EnvPlatform::Unix,
        );
        assert_eq!(
            policy.set,
            BTreeMap::from([
                ("HOME".to_string(), "/other".to_string()),
                ("ONEPIPELINE_RUN_ID".to_string(), "run-b".to_string()),
                (DISPATCH_MARKER_VAR.to_string(), "lease-uuid".to_string()),
            ])
        );
        assert_eq!(policy.exclude, ["CODEX_ONLY_FIRST", SERVER_MARKER_VAR]);
        assert_eq!(
            policy.thread_config(),
            json!({"shell_environment_policy": {
                "set": {"HOME": "/other", "ONEPIPELINE_RUN_ID": "run-b", DISPATCH_MARKER_VAR: "lease-uuid"},
                "exclude": ["CODEX_ONLY_FIRST", SERVER_MARKER_VAR],
            }})
        );
    }

    #[test]
    fn a_dispatch_cannot_smuggle_a_marker_of_its_own() {
        let policy = ShellEnvironmentPolicy::for_dispatch(
            &env(&[
                (DISPATCH_MARKER_VAR, "forged"),
                (SERVER_MARKER_VAR, "forged"),
            ]),
            &env(&[(SERVER_MARKER_VAR, "real")]),
            "mine",
            EnvPlatform::Unix,
        );
        assert_eq!(policy.set[DISPATCH_MARKER_VAR], "mine");
        assert!(!policy.set.contains_key(SERVER_MARKER_VAR));
        assert_eq!(policy.exclude, [SERVER_MARKER_VAR]);
    }

    #[test]
    fn the_pool_key_moves_with_every_process_level_input_and_nothing_else() {
        let base_env = env(&[("PATH", "/bin"), ("CODEX_HOME", "/a")]);
        let args = vec!["-c".to_string(), "features.plugins=false".to_string()];
        let key = pool_key("/usr/bin/codex", "codex-cli 1.0\n", &base_env, &args, false);
        assert!(key.as_str().starts_with("codex-app-server-"), "{key}");
        assert_eq!(
            key,
            pool_key("/usr/bin/codex", "codex-cli 1.0", &base_env, &args, false),
            "trailing whitespace of the version output is not a different release"
        );
        for other in [
            pool_key("/opt/codex", "codex-cli 1.0", &base_env, &args, false),
            pool_key("/usr/bin/codex", "codex-cli 1.1", &base_env, &args, false),
            pool_key(
                "/usr/bin/codex",
                "codex-cli 1.0",
                &env(&[("PATH", "/bin"), ("CODEX_HOME", "/b")]),
                &args,
                false,
            ),
            pool_key("/usr/bin/codex", "codex-cli 1.0", &base_env, &[], false),
            pool_key("/usr/bin/codex", "codex-cli 1.0", &base_env, &args, true),
        ] {
            assert_ne!(key, other);
        }
    }

    #[test]
    fn a_frame_belongs_to_the_thread_it_names_or_to_the_connection() {
        let own = Some("t-1");
        assert!(is_own_frame(&json!({"id": 3, "result": {}}), own));
        assert!(is_own_frame(
            &json!({"method": "account/rateLimits/updated", "params": {}}),
            own
        ));
        assert!(is_own_frame(
            &json!({"method": "turn/started", "params": {"threadId": "t-1"}}),
            own
        ));
        assert!(is_own_frame(
            &json!({"method": "thread/started", "params": {"thread": {"id": "t-1"}}}),
            own
        ));
        assert!(!is_own_frame(
            &json!({"method": "thread/status/changed", "params": {"threadId": "t-2"}}),
            own
        ));
        assert!(!is_own_frame(
            &json!({"id": 9, "method": "item/commandExecution/requestApproval", "params": {"threadId": "t-2"}}),
            own
        ));
        assert!(!is_own_frame(
            &json!({"method": "turn/started", "params": {"threadId": "t-1"}}),
            None
        ));
    }

    #[test]
    fn an_interrupt_reply_names_a_thread_the_server_no_longer_has() {
        assert_eq!(
            InterruptReply::of(&json!({"id": 1, "result": {}})),
            InterruptReply::Accepted
        );
        assert!(matches!(
            InterruptReply::of(&json!({"id": 1})),
            InterruptReply::Refused(_)
        ));
        assert!(matches!(
            InterruptReply::of(
                &json!({"id": 1, "error": {"code": -32600, "message": "thread not found: t"}})
            ),
            InterruptReply::ThreadGone(_)
        ));
        assert!(matches!(
            InterruptReply::of(&json!({"id": 1, "error": {"message": "thread not loaded"}})),
            InterruptReply::ThreadGone(_)
        ));
        assert_eq!(
            InterruptReply::of(
                &json!({"id": 1, "error": {"message": "expected active turn id x"}})
            ),
            InterruptReply::Refused("expected active turn id x".to_string())
        );
    }
}
