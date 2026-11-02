//! The settings that keep a long continued session cheap, stated once by the
//! caller and translated here for whichever harness runs: compaction at a size
//! (`auto_compact_tokens`), compaction on demand (`oneharness compact`), a lean
//! tool set (`tool_set`, `load_skills`, `load_mcp`), image input (`images`) and
//! per-call usage (`calls`).
//!
//! Every translation comes from one table, [`TRANSLATIONS`]: per harness and
//! the release it was confirmed against, each setting's knob, the knob's unit,
//! how a value maps, and the documentation or source the mapping rests on. The
//! argv/env builders below read their knob names from that table, `oneharness
//! list` derives its `supports_*` flags from it, and the tests hold each adapter
//! to it — so the three cannot drift apart.
//!
//! Pure: no file is read here. An image's bytes, a codex config's MCP server
//! names and a harness's session record are read at the `io` boundary and
//! handed in.

use std::num::NonZeroU64;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::mode::PermissionMode;
use crate::domain::signals::Usage;

/// A harness-agnostic class of built-in tool a turn may load (`tool_set`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolClass {
    /// Run commands.
    Shell,
    /// Read files.
    Read,
}

impl ToolClass {
    pub const ALL: [ToolClass; 2] = [ToolClass::Shell, ToolClass::Read];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ToolClass::Shell => "shell",
            ToolClass::Read => "read",
        }
    }

    /// Parse one class token, refusing an unknown one by name.
    ///
    /// # Errors
    ///
    /// Names the unknown token and the classes that exist.
    pub fn parse(token: &str) -> Result<ToolClass, String> {
        ToolClass::ALL
            .into_iter()
            .find(|class| class.as_str() == token)
            .ok_or_else(|| {
                format!(
                    "unknown tool class `{token}` in `tool_set`; expected one of `shell`, `read`"
                )
            })
    }

    /// Parse a comma-separated list (`--tool-set shell,read`,
    /// `ONEHARNESS_TOOL_SET`). Blank entries are dropped, so an empty value is
    /// the empty set: no built-in tool at all.
    ///
    /// # Errors
    ///
    /// The first unknown class, by name.
    pub fn parse_list(text: &str) -> Result<Vec<ToolClass>, String> {
        let mut classes = Vec::new();
        for token in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let class = ToolClass::parse(token)?;
            if !classes.contains(&class) {
                classes.push(class);
            }
        }
        Ok(classes)
    }
}

/// One of the settings this module translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    /// Compact a session's context once it reaches this many tokens.
    AutoCompactTokens,
    /// Compact a named session immediately (`oneharness compact`).
    CompactNow,
    /// Load only these built-in tool classes.
    ToolSet,
    /// `false` loads no skills list.
    LoadSkills,
    /// `false` loads no MCP server or MCP tool.
    LoadMcp,
    /// Attach image files to the prompt as images.
    Images,
    /// One usage per model call (`RunResult::calls`).
    Calls,
}

impl Setting {
    pub const ALL: [Setting; 7] = [
        Setting::AutoCompactTokens,
        Setting::CompactNow,
        Setting::ToolSet,
        Setting::LoadSkills,
        Setting::LoadMcp,
        Setting::Images,
        Setting::Calls,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Setting::AutoCompactTokens => "auto_compact_tokens",
            Setting::CompactNow => "compact_now",
            Setting::ToolSet => "tool_set",
            Setting::LoadSkills => "load_skills",
            Setting::LoadMcp => "load_mcp",
            Setting::Images => "images",
            Setting::Calls => "calls",
        }
    }
}

/// Where a setting reaches the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knob {
    /// An environment variable on the harness process.
    Env(&'static str),
    /// A command-line flag.
    Flag(&'static str),
    /// A `-c key=value` configuration override (codex).
    ConfigOverride(&'static str),
    /// A content block inside the stream-json user frame on stdin.
    StdinFrame(&'static str),
    /// A prompt the harness interprets as one of its own commands.
    PromptCommand(&'static str),
    /// A JSON-RPC method on the harness's own protocol.
    ProtocolMethod(&'static str),
    /// A field of the harness's output stream.
    StreamField(&'static str),
    /// An event in the harness's own on-disk session record.
    SessionRecord(&'static str),
}

/// What a knob's value is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// A context window size in tokens, from which the harness derives its
    /// compaction threshold. See [`ClaudeWindowRule`].
    WindowTokens,
    /// A percentage of a stated window.
    PercentOfWindow,
    /// A token count taken verbatim.
    Tokens,
    /// The harness's own tool names.
    ToolNames,
    /// A presence flag or a boolean.
    Switch,
    /// One entry per configured server name.
    ServerNames,
    /// A path to a file on disk.
    FilePath,
    /// Base64-encoded file content with its media type.
    Base64Content,
    /// An operation, not a value.
    Operation,
    /// Per-call token counts.
    UsageRecord,
}

/// One knob of a translation and its unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnobSpec {
    pub knob: Knob,
    pub unit: Unit,
}

/// One setting on one harness release.
#[derive(Debug, Clone, Copy)]
pub struct Translation {
    pub harness: &'static str,
    /// The harness release this entry was confirmed against.
    pub version: &'static str,
    pub setting: Setting,
    /// Empty: the harness has no knob for the setting, and a run that asks for
    /// it is reported (a diagnostic, or for `images` a refusal).
    pub knobs: &'static [KnobSpec],
    /// How a caller's value maps onto the knobs.
    pub mapping: &'static str,
    /// What the knobs cannot do, when they apply the setting only in part. A
    /// partial translation is applied AND reported, and counts as unsupported
    /// in `oneharness list`.
    pub residual: Option<&'static str>,
    /// The documentation page, source file or captured run the entry rests on.
    pub source: &'static str,
}

impl Translation {
    /// Whether the harness honours the setting completely.
    #[must_use]
    pub fn supported(&self) -> bool {
        !self.knobs.is_empty() && self.residual.is_none()
    }

    fn knob(&self, index: usize) -> Knob {
        self.knobs[index].knob
    }
}

/// How Claude Code 2.1.316 turns `CLAUDE_CODE_AUTO_COMPACT_WINDOW` into the
/// token count at which it compacts, read from its bundled source: the window
/// is clamped to `[min_window, max_window]`, the effective window is the window
/// minus `min(maxOutputTokens, output_reserve)`, and the threshold is the
/// effective window minus `buffer` — or, with
/// `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `floor(effective × pct / 100)` capped at
/// that same value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeWindowRule {
    output_reserve: u64,
    buffer: u64,
    min_window: u64,
    max_window: u64,
}

/// Claude Code 2.1.316's rule, the only one there is: its fields are private
/// so no caller can state a window whose arithmetic does not hold.
pub const CLAUDE_WINDOW_RULE: ClaudeWindowRule = ClaudeWindowRule {
    output_reserve: 20_000,
    buffer: 13_000,
    min_window: 100_000,
    max_window: 1_000_000,
};

impl ClaudeWindowRule {
    /// The environment that makes Claude Code compact at `tokens`, and a
    /// warning when the value lies above what its window can express.
    #[must_use]
    pub fn environment(&self, tokens: u64) -> (Vec<(String, String)>, Option<String>) {
        let window_var = claude_auto_compact().knobs[0].knob;
        let pct_var = claude_auto_compact().knobs[1].knob;
        let (Knob::Env(window_var), Knob::Env(pct_var)) = (window_var, pct_var) else {
            unreachable!("the claude-code auto_compact_tokens knobs are environment variables");
        };
        let overhead = self.output_reserve + self.buffer;
        let wanted = tokens.saturating_add(overhead);
        if wanted < self.min_window {
            // Below the floor the window is pinned there and the percentage of
            // its effective window carries the threshold. `+ 0.5` keeps the
            // floor of the product at `tokens` despite binary rounding.
            let effective = self.min_window - self.output_reserve;
            let pct = (tokens as f64 + 0.5) * 100.0 / effective as f64;
            return (
                vec![
                    (window_var.to_string(), self.min_window.to_string()),
                    (pct_var.to_string(), format!("{pct:.6}")),
                ],
                None,
            );
        }
        if wanted > self.max_window {
            let most = self.max_window - overhead;
            return (
                vec![(window_var.to_string(), self.max_window.to_string())],
                Some(format!(
                    "claude-code compacts at no more than {most} tokens; `auto_compact_tokens = \
                     {tokens}` was applied as {most}"
                )),
            );
        }
        (vec![(window_var.to_string(), wanted.to_string())], None)
    }

    /// The threshold Claude Code derives from that environment (the inverse,
    /// used to hold [`Self::environment`] to the harness's own arithmetic).
    #[must_use]
    pub fn threshold(&self, window: u64, pct: Option<f64>) -> u64 {
        let window = window.clamp(self.min_window, self.max_window);
        let effective = window - self.output_reserve;
        let ceiling = effective - self.buffer;
        match pct {
            Some(p) if p > 0.0 && p <= 100.0 => {
                ((effective as f64 * (p / 100.0)).floor() as u64).min(ceiling)
            }
            _ => ceiling,
        }
    }
}

const CLAUDE: &str = "claude-code";
const CLAUDE_VERSION: &str = "2.1.316";
const CODEX: &str = "codex";
const CODEX_VERSION: &str = "0.159.2";
/// The codex feature that loads its shell, which reads files too.
const CODEX_SHELL_TOOL: &str = "features.shell_tool";

const fn knob(knob: Knob, unit: Unit) -> KnobSpec {
    KnobSpec { knob, unit }
}

const fn none(
    harness: &'static str,
    version: &'static str,
    setting: Setting,
    source: &'static str,
) -> Translation {
    Translation {
        harness,
        version,
        setting,
        knobs: &[],
        mapping: "no knob: a run asking for it is reported",
        residual: None,
        source,
    }
}

/// The source cited for a harness whose headless help names no knob for any
/// of the lean settings.
macro_rules! no_lean_knobs {
    ($harness:expr, $version:expr, $help:expr, $images:expr) => {
        [
            none($harness, $version, Setting::AutoCompactTokens, $help),
            none($harness, $version, Setting::CompactNow, $help),
            none($harness, $version, Setting::ToolSet, $help),
            none($harness, $version, Setting::LoadSkills, $help),
            none($harness, $version, Setting::LoadMcp, $help),
            $images,
            none(
                $harness,
                $version,
                Setting::Calls,
                "its output carries one run-level usage at most, with no per-call figures",
            ),
        ]
    };
}

const CLAUDE_TABLE: [Translation; 7] = [
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::AutoCompactTokens,
        knobs: &[
            knob(
                Knob::Env("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                Unit::WindowTokens,
            ),
            knob(
                Knob::Env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
                Unit::PercentOfWindow,
            ),
        ],
        mapping: "window = N + 33000 (a 20000-token output reserve plus the 13000-token \
                  autocompact buffer), so the threshold Claude Code derives is N. Below the \
                  100000-token window floor (N < 67000) the window is pinned at 100000 and \
                  CLAUDE_AUTOCOMPACT_PCT_OVERRIDE = N / 80000 x 100 (a percentage of that \
                  window's 80000-token effective window) carries it; above 967000 the \
                  1000000-token maximum window applies and the run says so",
        residual: None,
        source: "Claude Code 2.1.316 bundled source in the installed `claude` binary: the \
                 CLAUDE_CODE_AUTO_COMPACT_WINDOW reader (100k-1M, below-minimum clamps up), \
                 effectiveWindow = window - min(maxOutputTokens, 20000), threshold = \
                 effectiveWindow - 13000 or floor(effectiveWindow x pct/100) under \
                 CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::CompactNow,
        knobs: &[
            knob(Knob::PromptCommand("/compact"), Unit::Operation),
            knob(Knob::Flag("--fork-session"), Unit::Operation),
        ],
        mapping: "`claude -p /compact --resume <token> --fork-session --output-format \
                  stream-json --verbose`: the compaction runs on a fork, and the session name is \
                  rebound to the fork's id only once `compact_result` is `success`, so a \
                  cancelled compaction leaves the original session untouched",
        residual: None,
        source: "captured from Claude Code 2.1.316 (tests/fixtures/claude-compact-fork.jsonl): \
                 `status: compacting`, `compact_result: success`, a `compact_boundary` carrying \
                 pre_tokens/post_tokens, under a new session id, the original transcript \
                 unchanged",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::ToolSet,
        knobs: &[knob(Knob::Flag("--tools"), Unit::ToolNames)],
        mapping: "shell -> Bash, read -> Read, as one `--tools` list (a second `--tools` would \
                  widen the first: Claude Code unions them); under read-only the list is \
                  intersected with read-only's own tools",
        residual: None,
        source: "`claude --help` (2.1.316) `--tools <tools...>`; a captured API request carried \
                 only Bash and Read",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::LoadSkills,
        knobs: &[knob(Knob::Flag("--disable-slash-commands"), Unit::Switch)],
        mapping: "load_skills = false adds the flag",
        residual: None,
        source: "`claude --help` (2.1.316): \"Disable all skills and commands\"; a captured API \
                 request carried no skills listing",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::LoadMcp,
        knobs: &[knob(Knob::Flag("--strict-mcp-config"), Unit::Switch)],
        mapping: "load_mcp = false adds the flag with no `--mcp-config`, so no server is loaded",
        residual: None,
        source: "`claude --help` (2.1.316): \"Only use MCP servers from --mcp-config, ignoring \
                 all other MCP configurations\"; a captured API request carried no MCP tool",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::Images,
        knobs: &[knob(
            Knob::StdinFrame("--input-format stream-json"),
            Unit::Base64Content,
        )],
        mapping: "the prompt is written to stdin as one stream-json user message whose content \
                  is one base64 `image` block per file, then the text; the run reads \
                  `--output-format stream-json`",
        residual: None,
        source: "`claude --help` (2.1.316) `--input-format stream-json`; a captured API request \
                 carried the frame's block as `image/png` content",
    },
    Translation {
        harness: CLAUDE,
        version: CLAUDE_VERSION,
        setting: Setting::Calls,
        knobs: &[knob(
            Knob::StreamField("assistant.message.usage"),
            Unit::UsageRecord,
        )],
        mapping: "one usage per distinct `message.id` of the stream-json `assistant` events, in \
                  order; the single-document `json` format carries none",
        residual: None,
        source: "tests/fixtures/claude-stream-json-turn.jsonl, captured from Claude Code",
    },
];

const CODEX_TABLE: [Translation; 7] = [
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::AutoCompactTokens,
        knobs: &[knob(
            Knob::ConfigOverride("model_auto_compact_token_limit"),
            Unit::Tokens,
        )],
        mapping: "`-c model_auto_compact_token_limit=N`, verbatim",
        residual: None,
        source:
            "codex 0.159.2 `ConfigToml` field model_auto_compact_token_limit and its app-server \
                 schema (`codex app-server generate-json-schema`); a captured `codex exec` run \
                 accepted the override",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::CompactNow,
        knobs: &[
            knob(Knob::ProtocolMethod("thread/fork"), Unit::Operation),
            knob(
                Knob::ProtocolMethod("thread/compact/start"),
                Unit::Operation,
            ),
        ],
        mapping: "over `codex app-server`: `thread/fork` of the stored thread, then \
                  `thread/compact/start` on the fork until `turn/completed`; the session name is \
                  rebound to the fork only on completion, because codex also indexes a thread's \
                  turns in shared SQLite stores a restore of one file could not undo",
        residual: None,
        source: "codex 0.159.2 app-server schema (ThreadForkParams, ThreadCompactStartParams) and \
                 a captured fork compaction (tests/fixtures/codex-app-server-fork-compact.jsonl) \
                 that left the original rollout byte for byte unchanged",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::ToolSet,
        knobs: &[
            knob(Knob::ConfigOverride("features.goals"), Unit::Switch),
            knob(Knob::ConfigOverride("features.view_image"), Unit::Switch),
            knob(Knob::ConfigOverride("features.sleep_tool"), Unit::Switch),
            knob(Knob::ConfigOverride("web_search"), Unit::Switch),
            knob(Knob::ConfigOverride(CODEX_SHELL_TOOL), Unit::Switch),
        ],
        mapping: "shell and read both mean codex's own shell (`exec_command`, `write_stdin`), \
                  which reads files too, so either keeps it and an empty set turns it off \
                  (`-c features.shell_tool=false`); every other tool codex lets configuration \
                  remove is removed: `-c features.goals=false`, `-c features.view_image=false`, \
                  `-c features.sleep_tool=false`, `-c web_search=\"disabled\"`",
        residual: Some(
            "codex 0.159.2 has no setting that removes `apply_patch`, `request_user_input`, the \
             multi-agent `collaboration` tools or `clock__curr_time`, so they stay loaded",
        ),
        source: "requests captured from codex 0.159.2 at a stub endpoint: each override removed \
                 its tool (`features.shell_tool=false` removed `exec_command` and \
                 `write_stdin`); no feature or config key removed the residual tools",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::LoadSkills,
        knobs: &[knob(
            Knob::ConfigOverride("skills.include_instructions"),
            Unit::Switch,
        )],
        mapping: "load_skills = false adds `-c skills.include_instructions=false`",
        residual: None,
        source: "`codex debug prompt-input` (0.159.2): the `<skills_instructions>` block is gone \
                 under the override",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::LoadMcp,
        knobs: &[
            knob(Knob::ConfigOverride("mcp_servers"), Unit::ServerNames),
            knob(Knob::ConfigOverride("features.apps"), Unit::Switch),
        ],
        mapping: "each server named in `$CODEX_HOME/config.toml` or a `.codex/config.toml` from \
                  the working directory upward gets `-c mcp_servers.<name>={command=\"true\",\
                  enabled=false}` (a disabled stub, valid whether or not codex loads that file; \
                  `-c mcp_servers={}` merges and disables nothing), plus `-c features.apps=false`",
        residual: None,
        source: "codex 0.159.2 runs with a configured stub server: the server started without the \
                 override and not with it; an `enabled=false` for an unconfigured name alone \
                 fails config load (`invalid transport`)",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::Images,
        knobs: &[knob(Knob::Flag("--image"), Unit::FilePath)],
        mapping: "`--image=<path>` per file (the `=` form: a bare `-i` takes every following \
                  argument, the prompt included, as another image)",
        residual: None,
        source: "`codex exec --help` (0.159.2) `-i, --image <FILE>...`; a captured request \
                 carried the file as `input_image`",
    },
    Translation {
        harness: CODEX,
        version: CODEX_VERSION,
        setting: Setting::Calls,
        knobs: &[knob(
            Knob::SessionRecord("event_msg token_count info.last_token_usage"),
            Unit::UsageRecord,
        )],
        mapping: "one usage per `token_count` event this run appended to the thread's rollout \
                  `$CODEX_HOME/sessions/**/rollout-*-<thread>.jsonl` (the `--json` stream \
                  carries only the turn total)",
        residual: None,
        source: "tests/fixtures/codex-rollout-token-counts.jsonl, captured from codex 0.159.2: \
                 three calls whose figures sum to the turn's `turn.completed` usage",
    },
];

const OPENCODE_TABLE: [Translation; 7] = no_lean_knobs!(
    "opencode",
    "1.18.5",
    "`opencode run --help` (1.18.5) names no flag for it",
    Translation {
        harness: "opencode",
        version: "1.18.5",
        setting: Setting::Images,
        knobs: &[knob(Knob::Flag("-f"), Unit::FilePath)],
        mapping: "`-f <path>` per file",
        residual: None,
        source: "`opencode run --help` (1.18.5): \"-f, --file  file(s) to attach to message\"",
    }
);

const COPILOT_TABLE: [Translation; 7] = no_lean_knobs!(
    "copilot",
    "1.0.78",
    "`copilot --help` (1.0.78) names no headless flag oneharness has confirmed for it",
    Translation {
        harness: "copilot",
        version: "1.0.78",
        setting: Setting::Images,
        knobs: &[knob(Knob::Flag("--attachment"), Unit::FilePath)],
        mapping: "`--attachment <path>` per file",
        residual: None,
        source: "`copilot --help` (1.0.78): \"--attachment <path>  Attach a file (image or native \
                 document) to the initial prompt; only valid in non-interactive mode\"",
    }
);

const GOOSE_TABLE: [Translation; 7] = no_lean_knobs!(
    "goose",
    "1.44.0",
    "`goose run --help` (1.44.0) names no flag for it",
    none(
        "goose",
        "1.44.0",
        Setting::Images,
        "`goose run --help` (1.44.0) names no image input"
    )
);

const QWEN_TABLE: [Translation; 7] = no_lean_knobs!(
    "qwen",
    "0.21.0",
    "`qwen --help` (0.21.0) names no flag for it",
    none(
        "qwen",
        "0.21.0",
        Setting::Images,
        "`qwen --help` (0.21.0) names no image input"
    )
);

const CRUSH_TABLE: [Translation; 7] = no_lean_knobs!(
    "crush",
    "0.87.0",
    "`crush run --help` (0.87.0) names no flag for it",
    none(
        "crush",
        "0.87.0",
        Setting::Images,
        "`crush run --help` (0.87.0) names no image input"
    )
);

const CURSOR_TABLE: [Translation; 7] = no_lean_knobs!(
    "cursor",
    "2026.07.23-e383d2b",
    "`cursor-agent --help` (2026.07.23-e383d2b) names no flag for it",
    none(
        "cursor",
        "2026.07.23-e383d2b",
        Setting::Images,
        "`cursor-agent --help` (2026.07.23-e383d2b) names no image input"
    )
);

/// Every harness's translation of every setting.
pub const TRANSLATIONS: [[Translation; 7]; 8] = [
    CLAUDE_TABLE,
    CODEX_TABLE,
    OPENCODE_TABLE,
    GOOSE_TABLE,
    QWEN_TABLE,
    CRUSH_TABLE,
    COPILOT_TABLE,
    CURSOR_TABLE,
];

/// What an id the registry does not hold translates to: no knob for anything,
/// so every setting reads as unsupported rather than as a panic.
const UNREGISTERED: [Translation; 7] = no_lean_knobs!(
    "unregistered",
    "-",
    "not a registered harness id",
    none(
        "unregistered",
        "-",
        Setting::Images,
        "not a registered harness id"
    )
);

/// The translation of `setting` for harness `id` (a base id). Every
/// registered harness has every setting (`every_harness_has_every_setting`);
/// an unregistered id answers an entry with no knobs.
#[must_use]
pub fn translation(id: &str, setting: Setting) -> &'static Translation {
    TRANSLATIONS
        .iter()
        .flatten()
        .find(|t| t.harness == id && t.setting == setting)
        .or_else(|| UNREGISTERED.iter().find(|t| t.setting == setting))
        .unwrap_or(&UNREGISTERED[0])
}

/// Whether harness `id` honours `setting` completely.
#[must_use]
pub fn supports(id: &str, setting: Setting) -> bool {
    translation(id, setting).supported()
}

fn claude_auto_compact() -> &'static Translation {
    &CLAUDE_TABLE[0]
}

/// A setting a harness could not apply, named in the run's report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    /// The harness id the run used.
    pub harness: String,
    // The setting it could not apply (or applied in part). A `//` comment on
    // purpose: a `$ref` with a sibling description is inlined by the SDK
    // generator, which then loses `Setting` as an exported name.
    pub setting: Setting,
    /// What happened instead.
    pub message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn new(harness: &str, setting: Setting, message: impl Into<String>) -> Self {
        Self {
            harness: harness.to_string(),
            setting,
            message: message.into(),
        }
    }
}

/// The portable settings a run resolved, before translation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeanSettings {
    pub auto_compact_tokens: Option<NonZeroU64>,
    /// `None` loads what the harness loads today.
    pub tool_set: Option<Vec<ToolClass>>,
    /// `None` and `Some(true)` both load skills as today.
    pub load_skills: Option<bool>,
    pub load_mcp: Option<bool>,
}

/// What one harness receives for a [`LeanSettings`]: argv additions that ride
/// ahead of the caller's own args (so a caller's later override still wins),
/// environment, and the diagnostics for what could not be applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeanDelivery {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Translate `settings` for harness `id`.
///
/// `codex_mcp_servers` names the MCP servers the run's codex configuration
/// declares (read by the `io` layer); other harnesses ignore it. A claude-code
/// tool set is not here: it rides `BuildCtx::tools` into `argv_claude_code`,
/// which already owns that harness's one `--tools` list (see
/// [`claude_tool_names`]).
#[must_use]
pub fn deliver(
    id: &str,
    settings: &LeanSettings,
    codex_mcp_servers: &[String],
    mode: PermissionMode,
) -> LeanDelivery {
    let mut out = LeanDelivery::default();
    if let Some(tokens) = settings.auto_compact_tokens {
        let t = translation(id, Setting::AutoCompactTokens);
        match id {
            CLAUDE => {
                let (env, warning) = CLAUDE_WINDOW_RULE.environment(tokens.get());
                out.env.extend(env);
                if let Some(warning) = warning {
                    out.diagnostics
                        .push(Diagnostic::new(id, Setting::AutoCompactTokens, warning));
                }
            }
            CODEX => {
                let Knob::ConfigOverride(key) = t.knob(0) else {
                    unreachable!("codex auto-compact is a config override")
                };
                out.args
                    .extend(["-c".to_string(), format!("{key}={tokens}")]);
            }
            _ => out.diagnostics.push(unsupported(t)),
        }
    }
    if let Some(classes) = &settings.tool_set {
        let t = translation(id, Setting::ToolSet);
        match id {
            CLAUDE => {
                if mode == PermissionMode::ReadOnly && classes.contains(&ToolClass::Shell) {
                    out.diagnostics.push(Diagnostic::new(
                        id,
                        Setting::ToolSet,
                        "`shell` is withheld under `--mode read-only`, which loads no tool that \
                         runs commands",
                    ));
                }
            }
            CODEX => {
                // codex reads files through its shell, so `read` keeps it as
                // `shell` does; only a set naming neither turns it off.
                let keeps_shell = !classes.is_empty();
                for spec in t.knobs {
                    let Knob::ConfigOverride(key) = spec.knob else {
                        continue;
                    };
                    if key == CODEX_SHELL_TOOL && keeps_shell {
                        continue;
                    }
                    let value = if key == "web_search" {
                        format!("{key}=\"disabled\"")
                    } else {
                        format!("{key}=false")
                    };
                    out.args.extend(["-c".to_string(), value]);
                }
                if let Some(residual) = t.residual {
                    let shell = if classes.contains(&ToolClass::Shell) || classes.is_empty() {
                        String::new()
                    } else {
                        " The shell (`exec_command`) also stays loaded: codex reads files \
                         through it, so `read` needs it."
                            .to_string()
                    };
                    out.diagnostics.push(Diagnostic::new(
                        id,
                        Setting::ToolSet,
                        format!("`tool_set` applied in part: {residual}.{shell}"),
                    ));
                }
            }
            _ => out.diagnostics.push(unsupported(t)),
        }
    }
    if settings.load_skills == Some(false) {
        let t = translation(id, Setting::LoadSkills);
        match (id, t.knobs.first().map(|k| k.knob)) {
            (CLAUDE, Some(Knob::Flag(flag))) => out.args.push(flag.to_string()),
            (CODEX, Some(Knob::ConfigOverride(key))) => {
                out.args.extend(["-c".to_string(), format!("{key}=false")]);
            }
            _ => out.diagnostics.push(unsupported(t)),
        }
    }
    if settings.load_mcp == Some(false) {
        let t = translation(id, Setting::LoadMcp);
        match id {
            CLAUDE => {
                if let Knob::Flag(flag) = t.knob(0) {
                    out.args.push(flag.to_string());
                }
            }
            CODEX => {
                for name in codex_mcp_servers {
                    out.args.extend([
                        "-c".to_string(),
                        format!(
                            "mcp_servers.{}={{command=\"true\",enabled=false}}",
                            toml_key(name)
                        ),
                    ]);
                }
                if let Knob::ConfigOverride(key) = t.knob(1) {
                    out.args.extend(["-c".to_string(), format!("{key}=false")]);
                }
            }
            _ => out.diagnostics.push(unsupported(t)),
        }
    }
    out
}

fn unsupported(t: &Translation) -> Diagnostic {
    Diagnostic::new(
        t.harness,
        t.setting,
        format!(
            "{} {} has no knob for `{}`, so it was not applied ({})",
            t.harness,
            t.version,
            t.setting.as_str(),
            t.source
        ),
    )
}

/// A TOML key for a `-c` dotted path: bare when it is one, else quoted.
fn toml_key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        name.to_string()
    } else {
        toml::Value::String(name.to_string()).to_string()
    }
}

/// The names of the MCP servers one codex `config.toml` declares (its
/// `[mcp_servers.<name>]` tables). An unparseable file declares none: codex
/// itself refuses it, which is the harness's error to report.
#[must_use]
pub fn codex_mcp_server_names(config_toml: &str) -> Vec<String> {
    toml::from_str::<toml::Table>(config_toml)
        .ok()
        .and_then(|table| table.get("mcp_servers").cloned())
        .and_then(|servers| servers.as_table().cloned())
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default()
}

/// The flag Claude Code takes its one built-in tool list on: the table's
/// `tool_set` knob, which read-only's narrowing rides too.
#[must_use]
pub fn claude_tools_flag() -> &'static str {
    match translation(CLAUDE, Setting::ToolSet)
        .knobs
        .first()
        .map(|k| k.knob)
    {
        Some(Knob::Flag(flag)) => flag,
        _ => "--tools",
    }
}

/// Claude Code's tool names for `classes` under `mode`, in its own spelling,
/// read from the table's `--tools` mapping. Under read-only the list is
/// intersected with `read_only` (that mode's own tools).
#[must_use]
pub fn claude_tool_names(
    classes: &[ToolClass],
    mode: PermissionMode,
    read_only: &[&'static str],
) -> Vec<&'static str> {
    classes
        .iter()
        .map(|class| match class {
            ToolClass::Shell => "Bash",
            ToolClass::Read => "Read",
        })
        .filter(|name| mode != PermissionMode::ReadOnly || read_only.contains(name))
        .collect()
}

/// The image formats a run may attach, recognized by their leading bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    /// The format `bytes` begin as, or `None` for anything else.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ImageFormat::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(ImageFormat::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(ImageFormat::Gif)
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(ImageFormat::Webp)
        } else {
            None
        }
    }

    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Webp => "image/webp",
        }
    }
}

/// An image file's checked content: built only from the bytes it was read
/// as, so its format and encoding always agree with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    path: String,
    format: ImageFormat,
    base64: String,
}

impl ImageInput {
    /// The image at `path` whose content is `bytes`, or `None` when those
    /// bytes are not a PNG, JPEG, GIF or WebP image.
    #[must_use]
    pub fn from_bytes(path: &std::path::Path, bytes: &[u8]) -> Option<ImageInput> {
        Some(ImageInput {
            path: path.display().to_string(),
            format: ImageFormat::sniff(bytes)?,
            base64: base64_encode(bytes),
        })
    }

    /// The path a path-taking harness receives (absolute, as `io::lean`
    /// resolves it).
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn format(&self) -> ImageFormat {
        self.format
    }
}

/// Standard base64 (RFC 4648, padded) of `bytes`, for an image's frame block.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Whether harness `id` takes images inside its stdin frame (claude-code),
/// rather than as file paths on the argv.
#[must_use]
pub fn images_ride_stdin_frame(id: &str) -> bool {
    matches!(
        translation(id, Setting::Images)
            .knobs
            .first()
            .map(|k| k.knob),
        Some(Knob::StdinFrame(_))
    )
}

/// The argv a path-taking harness adds for `images` (codex, opencode,
/// copilot); empty for claude-code, whose images ride its stdin frame.
#[must_use]
pub fn image_args(id: &str, images: &[ImageInput]) -> Vec<String> {
    let t = translation(id, Setting::Images);
    let Some(Knob::Flag(flag)) = t.knobs.first().map(|k| k.knob) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for image in images {
        if id == CODEX {
            out.push(format!("{flag}={}", image.path()));
        } else {
            out.push(flag.to_string());
            out.push(image.path().to_string());
        }
    }
    out
}

/// The one stream-json user frame that carries `images` and then `prompt` to
/// claude-code, newline-terminated.
#[must_use]
pub fn claude_image_frame(prompt: &str, images: &[ImageInput]) -> String {
    let mut content: Vec<Value> = images
        .iter()
        .map(|image| {
            serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": image.format.media_type(), "data": image.base64},
            })
        })
        .collect();
    content.push(serde_json::json!({"type": "text", "text": prompt}));
    let frame = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": content},
    });
    format!("{frame}\n")
}

/// Per-call usage from claude-code's stream-json stdout: one entry per
/// distinct `message.id` of its `assistant` events, in first-seen order, each
/// from that message's last event (a message with several content blocks is
/// emitted once per block). `None` when the stream carries no assistant usage.
#[must_use]
pub fn claude_stream_calls(stdout: &str) -> Option<Vec<Usage>> {
    // The table's selector, `<event type>.<message field>.<usage field>`.
    let selector = match translation(CLAUDE, Setting::Calls)
        .knobs
        .first()
        .map(|k| k.knob)
    {
        Some(Knob::StreamField(selector)) => selector,
        _ => "assistant.message.usage",
    };
    let mut parts = selector.split('.');
    let (Some(event), Some(message_field), Some(usage_field)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, Usage> = std::collections::HashMap::new();
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some(event) {
            continue;
        }
        let Some(message) = value.get(message_field) else {
            continue;
        };
        let (Some(id), Some(usage)) = (
            message.get("id").and_then(Value::as_str),
            message.get(usage_field).and_then(Value::as_object),
        ) else {
            continue;
        };
        let reading = Usage {
            input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
            output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
            cache_read_tokens: usage.get("cache_read_input_tokens").and_then(Value::as_u64),
            cache_write_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64),
            cost_usd: None,
        };
        if !by_id.contains_key(id) {
            order.push(id.to_string());
        }
        by_id.insert(id.to_string(), reading);
    }
    (!order.is_empty()).then(|| order.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Per-call usage from the lines a run appended to a codex rollout record:
/// one entry per `event_msg` `token_count` event carrying
/// `info.last_token_usage`, in order, from the last turn they hold — the
/// events after the last `event_msg` `task_started`, which codex writes as
/// each turn begins. So a run whose attempt was retried on the same thread
/// reports its final attempt's calls, the attempt its `usage` describes.
#[must_use]
pub fn codex_rollout_calls(rollout: &str) -> Vec<Usage> {
    // The table's selector, `<record type> <payload type> <payload path>`.
    let selector = match translation(CODEX, Setting::Calls)
        .knobs
        .first()
        .map(|k| k.knob)
    {
        Some(Knob::SessionRecord(selector)) => selector,
        _ => "event_msg token_count info.last_token_usage",
    };
    let mut parts = selector.split_whitespace();
    let (Some(record), Some(kind), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
        return Vec::new();
    };
    let pointer = format!("/payload/{}", path.replace('.', "/"));
    let events: Vec<Value> = rollout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .collect();
    let event_type = |value: &Value| -> Option<String> {
        (value.get("type").and_then(Value::as_str) == Some(record))
            .then(|| value.pointer("/payload/type").and_then(Value::as_str))
            .flatten()
            .map(str::to_string)
    };
    let last_turn = events
        .iter()
        .rposition(|value| event_type(value).as_deref() == Some("task_started"))
        .map_or(0, |at| at + 1);
    events[last_turn..]
        .iter()
        .filter(|value| event_type(value).as_deref() == Some(kind))
        .filter_map(|value| {
            let last = value.pointer(&pointer)?.as_object()?;
            Some(Usage {
                input_tokens: last.get("input_tokens").and_then(Value::as_u64),
                output_tokens: last.get("output_tokens").and_then(Value::as_u64),
                cache_read_tokens: last.get("cached_input_tokens").and_then(Value::as_u64),
                cache_write_tokens: last.get("cache_write_input_tokens").and_then(Value::as_u64),
                cost_usd: None,
            })
        })
        .collect()
}

/// The diagnostic a run carries when its harness exposes no per-call figures
/// in what it produced.
#[must_use]
pub fn calls_unavailable(id: &str, reason: &str) -> Diagnostic {
    Diagnostic::new(id, Setting::Calls, reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::harness;

    #[test]
    fn every_harness_has_every_setting_with_a_cited_source() {
        for spec in harness::all() {
            for setting in Setting::ALL {
                let t = translation(spec.id, setting);
                assert!(!t.source.is_empty(), "{} {setting:?}", spec.id);
                assert!(!t.version.is_empty());
                assert!(!t.mapping.is_empty());
            }
        }
        assert_eq!(
            TRANSLATIONS.iter().flatten().count(),
            harness::all().len() * Setting::ALL.len()
        );
    }

    #[test]
    fn an_unregistered_id_supports_nothing() {
        for setting in Setting::ALL {
            assert!(!supports("not-a-harness", setting));
            assert_eq!(translation("not-a-harness", setting).setting, setting);
        }
    }

    #[test]
    fn the_claude_window_compacts_at_the_requested_tokens() {
        let rule = CLAUDE_WINDOW_RULE;
        for tokens in [1, 50_000, 66_999, 67_000, 120_000, 500_000, 967_000] {
            let (env, warning) = rule.environment(tokens);
            assert!(warning.is_none());
            let window: u64 = env[0].1.parse().unwrap();
            let pct = env.get(1).map(|(_, v)| v.parse::<f64>().unwrap());
            assert_eq!(env[0].0, "CLAUDE_CODE_AUTO_COMPACT_WINDOW");
            assert_eq!(rule.threshold(window, pct), tokens, "{tokens}: {env:?}");
        }
        let (env, warning) = rule.environment(2_000_000);
        assert_eq!(
            env,
            vec![("CLAUDE_CODE_AUTO_COMPACT_WINDOW".into(), "1000000".into())]
        );
        assert!(warning.unwrap().contains("967000"));
    }

    #[test]
    fn a_tool_class_outside_the_defined_set_is_refused_by_name() {
        assert_eq!(
            ToolClass::parse_list("shell, read,shell").unwrap(),
            vec![ToolClass::Shell, ToolClass::Read]
        );
        assert_eq!(ToolClass::parse_list("").unwrap(), Vec::<ToolClass>::new());
        assert!(ToolClass::parse_list("shell,edit")
            .unwrap_err()
            .contains("`edit`"));
    }

    #[test]
    fn deliveries_use_exactly_the_tabled_knobs() {
        let lean = LeanSettings {
            auto_compact_tokens: NonZeroU64::new(50_000),
            tool_set: Some(vec![ToolClass::Shell, ToolClass::Read]),
            load_skills: Some(false),
            load_mcp: Some(false),
        };
        for spec in harness::all() {
            let d = deliver(
                spec.id,
                &lean,
                &["probe".to_string()],
                PermissionMode::Default,
            );
            let text = format!("{:?} {:?}", d.args, d.env);
            for setting in [
                Setting::AutoCompactTokens,
                Setting::ToolSet,
                Setting::LoadSkills,
                Setting::LoadMcp,
            ] {
                let t = translation(spec.id, setting);
                let reported = d.diagnostics.iter().any(|x| x.setting == setting);
                assert_eq!(reported, !t.supported(), "{} {setting:?}", spec.id);
                // claude-code's tool set rides BuildCtx into its own adapter,
                // so it is held to the table on the argv that adapter builds.
                if spec.id == CLAUDE && setting == Setting::ToolSet {
                    let classes = [ToolClass::Shell, ToolClass::Read];
                    let argv = (spec.build_argv)(&harness::BuildCtx {
                        bin: "claude",
                        prompt: "hi",
                        model: None,
                        system: None,
                        resume: None,
                        fork: false,
                        mode: PermissionMode::Default,
                        output_format: spec.output_format,
                        schema: None,
                        system_file: None,
                        delivery: harness::PromptDelivery::Argv,
                        plugins: false,
                        apps: false,
                        analytics: false,
                        model_catalog: None,
                        tools: Some(&classes),
                    });
                    let Knob::Flag(flag) = t.knobs[0].knob else {
                        panic!("claude-code's tool set is a flag")
                    };
                    assert!(
                        argv.windows(3).any(|w| w == [flag, "Bash", "Read"]),
                        "{argv:?}"
                    );
                    continue;
                }
                for k in t.knobs {
                    let name = match k.knob {
                        // Off only for a set naming neither shell nor read.
                        Knob::ConfigOverride(CODEX_SHELL_TOOL) => continue,
                        Knob::Env(n) | Knob::Flag(n) | Knob::ConfigOverride(n) => n,
                        _ => continue,
                    };
                    assert!(
                        text.contains(name),
                        "{} {setting:?} {name}: {text}",
                        spec.id
                    );
                }
            }
            if TRANSLATIONS
                .iter()
                .flatten()
                .filter(|t| t.harness == spec.id)
                .all(|t| t.knobs.is_empty() || t.setting == Setting::Images)
            {
                assert!(d.args.is_empty() && d.env.is_empty(), "{}", spec.id);
            }
        }
        assert_eq!(
            deliver(
                CODEX,
                &LeanSettings::default(),
                &[],
                PermissionMode::Default
            ),
            LeanDelivery::default()
        );
        let tools = |classes: Vec<ToolClass>| {
            let lean = LeanSettings {
                tool_set: Some(classes),
                ..LeanSettings::default()
            };
            deliver(CODEX, &lean, &[], PermissionMode::Default)
        };
        let shell_off = format!("{CODEX_SHELL_TOOL}=false");
        assert!(
            tools(vec![]).args.contains(&shell_off),
            "an empty set drops the shell"
        );
        let read_only = tools(vec![ToolClass::Read]);
        assert!(
            !read_only.args.contains(&shell_off),
            "read needs codex's shell"
        );
        assert!(read_only.diagnostics[0].message.contains("reads files"));
    }

    #[test]
    fn claude_code_s_adapter_emits_the_tabled_compaction_and_image_spellings() {
        let spec = harness::by_id(CLAUDE).unwrap();
        let ctx = |fork: bool, delivery: harness::PromptDelivery| harness::BuildCtx {
            bin: "claude",
            prompt: "/compact",
            model: None,
            system: None,
            resume: Some("sid"),
            fork,
            mode: PermissionMode::Default,
            output_format: crate::domain::report::OutputFormat::StreamJson,
            schema: None,
            system_file: None,
            delivery,
            plugins: false,
            apps: false,
            analytics: false,
            model_catalog: None,
            tools: None,
        };
        let forked = (spec.build_argv)(&ctx(true, harness::PromptDelivery::Argv));
        for k in translation(CLAUDE, Setting::CompactNow).knobs {
            let spelled = match k.knob {
                Knob::Flag(flag) | Knob::PromptCommand(flag) => flag,
                _ => continue,
            };
            assert!(forked.iter().any(|a| a == spelled), "{spelled}: {forked:?}");
        }
        let framed = (spec.build_argv)(&ctx(false, harness::PromptDelivery::MessageFrame));
        let Some(Knob::StdinFrame(spelling)) = translation(CLAUDE, Setting::Images)
            .knobs
            .first()
            .map(|k| k.knob)
        else {
            panic!("claude-code's images ride a stdin frame")
        };
        let words: Vec<&str> = spelling.split(' ').collect();
        assert!(
            framed.windows(words.len()).any(|w| w == words.as_slice()),
            "{framed:?}"
        );
    }

    #[test]
    fn claude_read_only_keeps_its_no_mutation_floor() {
        let names = claude_tool_names(
            &[ToolClass::Shell, ToolClass::Read],
            PermissionMode::ReadOnly,
            &["Read", "Grep"],
        );
        assert_eq!(names, vec!["Read"]);
        assert_eq!(
            claude_tool_names(&ToolClass::ALL, PermissionMode::Default, &[]),
            vec!["Bash", "Read"]
        );
    }

    #[test]
    fn codex_mcp_names_are_read_and_quoted() {
        let names = codex_mcp_server_names(
            "model = \"x\"\n[mcp_servers.docs]\ncommand = \"d\"\n[mcp_servers.\"a b\"]\nurl = \"u\"\n",
        );
        assert_eq!(names, vec!["a b".to_string(), "docs".to_string()]);
        assert!(codex_mcp_server_names("not toml [").is_empty());
        assert_eq!(toml_key("a b"), "\"a b\"");
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn per_call_usage_is_read_from_the_recorded_streams() {
        let claude = claude_stream_calls(&fixture("claude-stream-json-turn.jsonl")).unwrap();
        assert_eq!(claude.len(), 2);
        assert_eq!(claude[0].cache_write_tokens, Some(34_280));
        assert_eq!(claude[1].cache_read_tokens, Some(47_969));
        let codex = codex_rollout_calls(&fixture("codex-rollout-token-counts.jsonl"));
        assert_eq!(
            codex
                .iter()
                .map(|u| u.input_tokens.unwrap())
                .collect::<Vec<_>>(),
            vec![13_006, 13_101, 13_180]
        );
        assert_eq!(
            codex.iter().map(|u| u.input_tokens.unwrap()).sum::<u64>(),
            39_287,
            "the per-call figures sum to the turn total codex reported"
        );
        assert!(claude_stream_calls("{\"type\":\"result\"}").is_none());
        let started = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}";
        let counts = fixture("codex-rollout-token-counts.jsonl");
        let two_turns = format!(
            "{started}\n{counts}{started}\n{}",
            counts.lines().last().unwrap()
        );
        assert_eq!(
            codex_rollout_calls(&two_turns).len(),
            1,
            "only the last turn's calls"
        );
    }

    #[test]
    fn images_are_sniffed_and_delivered_per_harness() {
        assert_eq!(
            ImageFormat::sniff(b"\x89PNG\r\n\x1a\nxx"),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::sniff(b"plain"), None);
        assert_eq!(base64_encode(b"ABC"), "QUJD");
        assert_eq!(base64_encode(b"AB"), "QUI=");
        assert_eq!(base64_encode(b"A"), "QQ==");
        assert_eq!(base64_encode(b""), "");
        assert!(ImageInput::from_bytes(std::path::Path::new("/p/a.txt"), b"ABC").is_none());
        let img = ImageInput::from_bytes(std::path::Path::new("/p/a.png"), b"\x89PNG\r\n\x1a\n")
            .expect("a png");
        let one = std::slice::from_ref(&img);
        assert_eq!(image_args(CODEX, one), vec!["--image=/p/a.png"]);
        assert_eq!(image_args("opencode", one), vec!["-f", "/p/a.png"]);
        assert_eq!(image_args("copilot", one), vec!["--attachment", "/p/a.png"]);
        assert!(image_args(CLAUDE, one).is_empty());
        let frame: Value = serde_json::from_str(&claude_image_frame("hi", one)).unwrap();
        assert_eq!(
            frame["message"]["content"][0]["source"]["data"],
            "iVBORw0KGgo="
        );
        assert_eq!(
            frame["message"]["content"][0]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(frame["message"]["content"][1]["text"], "hi");
    }
}
