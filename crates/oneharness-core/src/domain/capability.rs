//! The declared capability surface every consumer surface is measured against.
//!
//! This is the single source the parity gate reconciles three ways:
//!
//! * against **clap** — `tests/capability.rs` in the binary crate walks the real
//!   command tree, so a flag added to `src/cli.rs` without a decision here (bind
//!   it, or declare it uncovered with a reason) fails the build;
//! * against the **language SDKs** — `sdk_schema::bundle` emits this manifest
//!   into each generated package, and `scripts/sdk-coverage.mjs` asserts every
//!   capability has a method whose argv builder emits every bound flag;
//! * against **`oneharness-core`** — each capability names the library entry
//!   point a Rust consumer calls instead of spawning the CLI, and
//!   `tests/library_surface.rs` exercises every one of them.
//!
//! It is deliberately data rather than a hand-maintained checklist: the
//! bindings ARE how the SDKs build their argv, so a binding that is wrong is a
//! broken call rather than a stale note.

use serde::Serialize;

/// How one SDK option reaches the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlagKind {
    /// A bare positional argument, appended in binding order.
    Positional,
    /// `--flag VALUE`, once.
    Value,
    /// `--flag VALUE`, once per array element.
    Repeated,
    /// `--flag`, present only when the option is true.
    Switch,
    /// `--flag KEY=VALUE`, once per map entry.
    KeyValue,
    /// Every array element appended verbatim after a `--` separator.
    Trailing,
}

/// One SDK option and the CLI flag it renders to.
#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
pub struct OptionBinding {
    /// The option's name in the SDK input contract (camelCase; the Python SDK
    /// accepts the same key, so one spelling serves both).
    pub option: &'static str,
    /// The CLI spelling it renders to. `""` for [`FlagKind::Positional`] and
    /// [`FlagKind::Trailing`], which have no flag.
    pub flag: &'static str,
    pub kind: FlagKind,
    /// Another option whose truth suppresses this one, or `""`.
    ///
    /// The one thing a flat binding list cannot say by itself: `history show`
    /// takes `--last` OR a session name and clap refuses both, so a lookup
    /// carrying `{session, last: true}` — which the union deliberately accepts,
    /// resolving to "the most recent" — must render only `--last`. Declaring the
    /// suppression keeps that rule in the manifest instead of in each SDK.
    pub unless: &'static str,
}

/// A CLI flag no SDK option renders, and why that is correct.
///
/// Silence is not an option here: a flag that is neither bound nor listed with
/// a reason fails the clap reconciliation.
#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
pub struct UncoveredFlag {
    pub flag: &'static str,
    pub reason: &'static str,
}

/// How a verb's stdout reaches an SDK caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StdoutShape {
    /// One JSON document, validated against the capability's output schema.
    Json,
    /// One validated JSON document per line, yielded as it arrives.
    Jsonl,
    /// Not a contract at all. `init` writes a human confirmation line by
    /// design — its deliverable is the file — and `gate`/`mock` answer with a
    /// harness's own native verdict, or with nothing at all when the call is
    /// allowed through. The SDK returns the text (or `null`) rather than
    /// pretending it parsed a document.
    Text,
}

/// One thing this CLI can do, and how each consumer surface reaches it.
#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
pub struct Capability {
    /// The SDK method name in camelCase. The Python SDK snake-cases it, so
    /// `historyList` is `history_list` there — one declaration, both spellings.
    pub method: &'static str,
    /// The verb path this capability invokes, e.g. `["history", "list"]`.
    pub argv: &'static [&'static str],
    /// The schema root of its input contract, or `None` for a verb that takes
    /// no options at all.
    pub options: Option<&'static str>,
    /// The schema root of its output contract, or `None` when the verb's stdout
    /// is not one ([`StdoutShape::Text`]).
    pub output: Option<&'static str>,
    /// How the SDK reads the verb's stdout.
    pub stdout: StdoutShape,
    /// Whether the call writes a payload to the CLI's stdin (the hook verbs).
    pub stdin: bool,
    /// The `oneharness-core` entry point a Rust consumer calls instead of
    /// spawning. `tests/library_surface.rs` exercises every one of them, and
    /// `scripts/check-capability-surface.sh` holds the two to each other.
    pub rust: &'static str,
    /// Argv fragments this capability always emits, whatever its options say —
    /// `--compact` for the JSON verbs, `--stream` for the streaming run. Counted
    /// as covered, because the method does emit them.
    pub always: &'static [&'static str],
    pub bindings: &'static [OptionBinding],
    pub uncovered: &'static [UncoveredFlag],
}

impl Capability {
    /// The Python SDK's spelling of [`Capability::method`].
    #[must_use]
    pub fn python_method(&self) -> String {
        let mut out = String::with_capacity(self.method.len() + 2);
        for ch in self.method.chars() {
            if ch.is_ascii_uppercase() {
                out.push('_');
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push(ch);
            }
        }
        out
    }
}

/// Shorthand for a bound option.
const fn bind(option: &'static str, flag: &'static str, kind: FlagKind) -> OptionBinding {
    OptionBinding {
        option,
        flag,
        kind,
        unless: "",
    }
}

/// Shorthand for a bound option another option suppresses. See
/// [`OptionBinding::unless`].
const fn bind_unless(
    option: &'static str,
    flag: &'static str,
    kind: FlagKind,
    unless: &'static str,
) -> OptionBinding {
    OptionBinding {
        option,
        flag,
        kind,
        unless,
    }
}

/// Shorthand for a flag no option renders, with the reason it need not.
const fn skip(flag: &'static str, reason: &'static str) -> UncoveredFlag {
    UncoveredFlag { flag, reason }
}

/// Why a `--format text` flag is never an option: the SDKs consume the JSON
/// contract, and `text` is the human-readable view of the same data.
const TEXT_FORMAT: &str =
    "the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not";

/// Bindings shared by `run` and `runStream`: both drive the same verb, and the
/// two methods differ only in how they read its stdout.
const RUN_BINDINGS: &[OptionBinding] = &[
    bind("prompt", "--prompt", FlagKind::Value),
    bind("batchPrompts", "--prompt", FlagKind::Repeated),
    bind("promptFiles", "--prompt-file", FlagKind::Repeated),
    bind("harnesses", "--harness", FlagKind::Repeated),
    bind("mockHarnesses", "--mock-harness", FlagKind::Repeated),
    bind_unless("all", "--all", FlagKind::Switch, "harnesses"),
    bind("exclude", "--exclude", FlagKind::Repeated),
    bind("models", "--model", FlagKind::Repeated),
    bind("system", "--system", FlagKind::Value),
    bind_unless("systemFile", "--system-file", FlagKind::Value, "system"),
    bind("reasoning", "--reasoning", FlagKind::Value),
    bind("resume", "--resume", FlagKind::Value),
    bind("fork", "--fork", FlagKind::Switch),
    bind("session", "--session", FlagKind::Value),
    bind("sessionDir", "--session-dir", FlagKind::Value),
    bind("control", "--control", FlagKind::Switch),
    bind("outputFormat", "--output-format", FlagKind::Value),
    bind("events", "--events", FlagKind::Switch),
    bind("mockRules", "--mock-rules", FlagKind::Value),
    bind("spyFile", "--spy-file", FlagKind::Value),
    bind("schema", "--schema", FlagKind::Value),
    bind("schemaMaxRetries", "--schema-max-retries", FlagKind::Value),
    bind("outputDir", "--output-dir", FlagKind::Value),
    bind("timeoutSeconds", "--timeout", FlagKind::Value),
    bind("cwd", "--cwd", FlagKind::Value),
    bind("env", "--env", FlagKind::KeyValue),
    bind("mode", "--mode", FlagKind::Value),
    bind("permitPrompts", "--permit-prompts", FlagKind::Switch),
    bind_unless("config", "--config", FlagKind::Value, "noConfig"),
    bind("noConfig", "--no-config", FlagKind::Switch),
    bind("maxParallel", "--max-parallel", FlagKind::Value),
    bind("batchStrategy", "--batch-strategy", FlagKind::Value),
    bind("runMode", "--run-mode", FlagKind::Value),
    bind("printCommand", "--print-command", FlagKind::Switch),
    bind("bins", "--bin", FlagKind::KeyValue),
    bind("requireAvailable", "--require-available", FlagKind::Switch),
    bind_unless("history", "--history", FlagKind::Switch, "noHistory"),
    bind("noHistory", "--no-history", FlagKind::Switch),
    bind("historyDir", "--history-dir", FlagKind::Value),
    bind("historyName", "--history-name", FlagKind::Value),
    bind("historyLabels", "--history-label", FlagKind::KeyValue),
    bind("passthrough", "", FlagKind::Trailing),
];

/// The `run` flags neither run method binds. The two `--mode` shorthands are
/// refused for the same reason on both — one setting with two spellings is how
/// a caller ends up passing both, which clap then refuses — and each method
/// declines the streaming half the other one owns.
const RUN_UNCOVERED: &[UncoveredFlag] = &[
    skip("--bypass", "`mode: \"bypass\"` is the same request"),
    skip("--no-bypass", "`mode: \"default\"` is the same request"),
    skip(
        "--stream",
        "`runStream()` is the streaming method; this one returns one report",
    ),
];

const RUN_STREAM_UNCOVERED: &[UncoveredFlag] = &[
    skip("--bypass", "`mode: \"bypass\"` is the same request"),
    skip("--no-bypass", "`mode: \"default\"` is the same request"),
    skip(
        "--no-stream",
        "this method streams by definition, so the negative half cannot apply",
    ),
];

/// Every capability this CLI exposes, and how each consumer surface reaches it.
///
/// Adding a verb or a flag means adding a row (or a binding) here: the clap
/// reconciliation in `tests/capability.rs` fails on a flag with no decision, and
/// `scripts/check-capability-surface.sh` fails on a capability an SDK does not
/// implement. Neither can be satisfied by editing a checklist — the bindings ARE
/// how the SDKs build their argv.
pub const CAPABILITIES: &[Capability] = &[
    Capability {
        method: "run",
        argv: &["run"],
        options: Some("run_options"),
        output: Some("run_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::run::run",
        always: &["--compact", "--no-stream"],
        bindings: RUN_BINDINGS,
        uncovered: RUN_UNCOVERED,
    },
    Capability {
        method: "runStream",
        argv: &["run"],
        options: Some("run_options"),
        output: Some("run_stream_envelope"),
        stdout: StdoutShape::Jsonl,
        stdin: false,
        rust: "oneharness_core::io::run::run",
        always: &["--compact", "--stream"],
        bindings: RUN_BINDINGS,
        uncovered: RUN_STREAM_UNCOVERED,
    },
    Capability {
        method: "list",
        // The one verb with no options at all: it describes the registry, and
        // the CLI gives it nothing to narrow or steer.
        argv: &["list"],
        options: None,
        output: Some("list_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::registry::list",
        always: &["--compact"],
        bindings: &[],
        uncovered: &[],
    },
    Capability {
        method: "detect",
        argv: &["detect"],
        options: Some("detect_options"),
        output: Some("detect_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::detect::detect",
        always: &["--compact"],
        bindings: &[
            bind("harnesses", "--harness", FlagKind::Repeated),
            bind_unless("all", "--all", FlagKind::Switch, "harnesses"),
            bind("exclude", "--exclude", FlagKind::Repeated),
            bind("bins", "--bin", FlagKind::KeyValue),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
            bind("requireAvailable", "--require-available", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "config",
        argv: &["config"],
        options: Some("config_options"),
        output: Some("config_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::domain::config::explain",
        always: &["--compact"],
        bindings: &[
            bind("cwd", "--cwd", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "sync",
        argv: &["sync"],
        options: Some("sync_options"),
        output: Some("sync_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::sync::sync",
        always: &["--compact"],
        bindings: &[
            bind("cwd", "--cwd", FlagKind::Value),
            bind("harnesses", "--harness", FlagKind::Repeated),
            bind("check", "--check", FlagKind::Switch),
            bind("global", "--global", FlagKind::Switch),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "init",
        argv: &["init"],
        options: Some("init_options"),
        output: None,
        stdout: StdoutShape::Text,
        stdin: false,
        rust: "oneharness_core::io::init::init",
        always: &[],
        bindings: &[
            bind("path", "", FlagKind::Positional),
            bind("force", "--force", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "usage",
        argv: &["usage"],
        options: Some("usage_options"),
        output: Some("usage_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::usage::report",
        always: &["--compact"],
        bindings: &[
            bind("harnesses", "--harness", FlagKind::Repeated),
            bind_unless("all", "--all", FlagKind::Switch, "harnesses"),
            bind("exclude", "--exclude", FlagKind::Repeated),
            bind("bins", "--bin", FlagKind::KeyValue),
            bind("cwd", "--cwd", FlagKind::Value),
            bind("timeoutSeconds", "--timeout", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[skip("--format", TEXT_FORMAT)],
    },
    Capability {
        method: "gate",
        argv: &["gate"],
        options: Some("gate_options"),
        output: None,
        stdout: StdoutShape::Text,
        stdin: true,
        rust: "oneharness_core::domain::gate::render_deny",
        always: &[],
        bindings: &[
            bind("harness", "", FlagKind::Positional),
            bind("denyIfContains", "--deny-if-contains", FlagKind::Value),
            bind("reason", "--reason", FlagKind::Value),
        ],
        uncovered: &[],
    },
    Capability {
        method: "mock",
        argv: &["mock"],
        options: Some("mock_options"),
        output: None,
        stdout: StdoutShape::Text,
        stdin: true,
        rust: "oneharness_core::domain::mock::decide",
        always: &[],
        bindings: &[
            bind("harness", "", FlagKind::Positional),
            bind("rules", "--rules", FlagKind::Value),
            bind("spyFile", "--spy-file", FlagKind::Value),
        ],
        uncovered: &[],
    },
    Capability {
        method: "interrupt",
        argv: &["interrupt"],
        options: Some("interrupt_options"),
        output: Some("control_response"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::control::interrupt",
        always: &["--compact"],
        bindings: &[
            bind("session", "--session", FlagKind::Value),
            bind("input", "--input", FlagKind::Value),
            bind("sessionDir", "--session-dir", FlagKind::Value),
            bind("cwd", "--cwd", FlagKind::Value),
        ],
        uncovered: &[],
    },
    Capability {
        method: "history",
        argv: &["history", "show"],
        options: Some("history_lookup"),
        output: Some("history_records"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::history::read_session",
        always: &["--compact"],
        bindings: &[
            bind_unless("session", "", FlagKind::Positional, "last"),
            bind("last", "--last", FlagKind::Switch),
            bind("all", "--all", FlagKind::Switch),
            bind_unless("project", "--project", FlagKind::Value, "allProjects"),
            bind("allProjects", "--all-projects", FlagKind::Switch),
            bind("historyDir", "--history-dir", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[skip("--format", TEXT_FORMAT)],
    },
    Capability {
        method: "historyList",
        argv: &["history", "list"],
        options: Some("history_list_options"),
        output: Some("history_list"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::history::list_sessions",
        always: &["--compact"],
        bindings: &[
            bind("variant", "--variant", FlagKind::Value),
            bind_unless("project", "--project", FlagKind::Value, "allProjects"),
            bind("allProjects", "--all-projects", FlagKind::Switch),
            bind("historyDir", "--history-dir", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[skip("--format", TEXT_FORMAT)],
    },
    Capability {
        method: "historyWatch",
        argv: &["history", "watch"],
        options: Some("history_watch_options"),
        output: Some("history_stream_envelope"),
        stdout: StdoutShape::Jsonl,
        stdin: false,
        rust: "oneharness_core::io::history::HistoryWatcher",
        always: &["--format", "jsonl"],
        bindings: &[
            bind("after", "--after", FlagKind::Value),
            bind("labels", "--label", FlagKind::KeyValue),
            bind("variant", "--variant", FlagKind::Value),
            bind_unless("project", "--project", FlagKind::Value, "allProjects"),
            bind("allProjects", "--all-projects", FlagKind::Switch),
            bind("historyDir", "--history-dir", FlagKind::Value),
            bind("events", "--events", FlagKind::Switch),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "historyClear",
        argv: &["history", "clear"],
        options: Some("history_clear_options"),
        output: Some("history_clear_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::history::remove_sessions",
        always: &["--compact"],
        bindings: &[
            bind_unless("project", "--project", FlagKind::Value, "allProjects"),
            bind("allProjects", "--all-projects", FlagKind::Switch),
            bind("yes", "--yes", FlagKind::Switch),
            bind("historyDir", "--history-dir", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[],
    },
    Capability {
        method: "historyMigrate",
        argv: &["history", "migrate"],
        options: Some("history_migrate_options"),
        output: Some("history_migrate_report"),
        stdout: StdoutShape::Json,
        stdin: false,
        rust: "oneharness_core::io::history::migrate",
        always: &["--compact"],
        bindings: &[
            bind("historyDir", "--history-dir", FlagKind::Value),
            bind_unless("config", "--config", FlagKind::Value, "noConfig"),
            bind("noConfig", "--no-config", FlagKind::Switch),
        ],
        uncovered: &[],
    },
];
