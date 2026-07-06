//! The runtime *mock/spy responder*: given a harness's pre-tool hook event on
//! stdin and a caller-supplied ruleset, decide whether to intercept the call and
//! render that harness's native verdict — a **deny** (the model reads the
//! message as tool feedback) or an **input rewrite** (the call runs with
//! substituted arguments, so a shell command can be swapped for a stub that
//! prints canned output, or a file read redirected to a fixture).
//!
//! This is the read-write sibling of [`crate::domain::gate`] and rides the same
//! installed-hook loop: `oneharness sync` installs a hook that invokes
//! `oneharness mock <id> --rules <file>`; this module is what that invocation
//! runs. It is pure — rules in, verdict out — so the per-harness wire protocol
//! is unit-testable without a real harness. The thin stdin/stdout wrapper (and
//! the spy-log append, the one I/O) lives in the binary (`src/commands/mock.rs`).
//!
//! Verdict shapes are per-harness registry data
//! ([`crate::domain::harness::HarnessSpec::mock_rewrite`] for rewrites;
//! [`DenyShape`] for denies), sourced from each CLI's published hook protocol,
//! never guessed — and **loud when absent**: a ruleset asking for an action a
//! harness cannot express is a usage error, never a silent allow. Which
//! harnesses honor a rewrite live is drift-alarmed by the `oh_mock_enforce`
//! e2e phases; the `explore-hooks` probe is how a new shape gets sourced (see
//! `docs/mock-spy-design.md`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::gate::DenyShape;

/// How a harness expresses "allow this call, but with these rewritten
/// arguments" from a pre-tool hook. Every variant is live-verified (the
/// `explore-hooks` probe and/or an `oh_mock_enforce` phase — see
/// `docs/mock-spy-design.md` for the per-harness evidence); absent for a
/// harness whose protocol has no rewrite verdict (Goose), whose hooks never
/// fire headlessly (Copilot), or whose documented rewrite was live-refuted
/// (Qwen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteShape {
    /// Claude Code / Codex: `hookSpecificOutput.updatedInput` beside an
    /// `allow` permission decision. (Qwen documents this shape too but was
    /// live-refuted — see the registry.)
    ClaudeNested,
    /// Crush: a flat `{"version":1,"decision":"allow","updated_input":{…}}`
    /// (its `updated_input` is a shallow-merge patch of the tool input).
    CrushFlat,
    /// OpenCode: the oneharness plugin shim applies a flat
    /// `{"decision":"allow","updated_input":{…}}` reply by merging it into the
    /// tool's mutable `args` before execution.
    OpencodeShim,
    /// Cursor: a flat `{"permission":"allow","updated_input":{…}}` on its
    /// `preToolUse` event. Exactly the probe-verified reply — no reason slot
    /// (extra fields are unverified against its parser, so none are sent).
    CursorPermission,
}

impl RewriteShape {
    /// Stable token for JSON surfaces (`oneharness list`).
    pub fn as_str(self) -> &'static str {
        match self {
            RewriteShape::ClaudeNested => "claude-nested",
            RewriteShape::CrushFlat => "crush-flat",
            RewriteShape::OpencodeShim => "opencode-shim",
            RewriteShape::CursorPermission => "cursor-permission",
        }
    }
}

/// How `run --mock-rules` / `run --spy-file` delivers the mock hook to this
/// harness **for one invocation** — the single-flag ephemeral path. Every
/// variant is live-verified; `None` (qwen: project hooks don't fire headlessly;
/// copilot: hooks never fire headlessly at all) makes the flag a loud usage
/// error for the harness, never a silently inert install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockDelivery {
    /// The hook rides the argv via a per-run settings flag (Claude Code's
    /// `--settings <file>`, probe-verified to load hooks headlessly): zero
    /// workspace mutation — existing project/user config still applies, the
    /// mock hook is layered on top for this invocation only.
    SettingsFlag { flag: &'static str },
    /// The hook is installed into the project-scope config in the working
    /// directory via the non-destructive merge (existing keys and hooks
    /// preserved — layered on top), with every touched file snapshotted first
    /// and restored after the run. `extra_args` are appended to the harness's
    /// argv — how Codex's hooks engine is opted in per invocation.
    ProjectHooks { extra_args: &'static [&'static str] },
}

/// Render the hook command an installed mock hook runs: this binary's `mock`
/// verb with the ruleset and spy log wired in. Paths are embedded verbatim, so
/// the caller must have refused whitespace-bearing ones first (the OpenCode
/// shim tokenizes the command on spaces, and shell-run hooks would split too).
pub fn hook_command(exe: &str, id: &str, rules: Option<&str>, spy: Option<&str>) -> String {
    let mut command = format!("{exe} mock {id}");
    if let Some(rules) = rules {
        command.push_str(" --rules ");
        command.push_str(rules);
    }
    if let Some(spy) = spy {
        command.push_str(" --spy-file ");
        command.push_str(spy);
    }
    command
}

/// The settings JSON a [`MockDelivery::SettingsFlag`] harness receives: a
/// PreToolUse hook (no matcher — every tool) invoking `command`. Exactly the
/// shape the explore-hooks probe verified Claude Code loads from a per-run
/// `--settings` file.
pub fn settings_hooks_json(command: &str) -> String {
    json!({
        "hooks": {
            "PreToolUse": [
                { "hooks": [ { "type": "command", "command": command } ] }
            ]
        }
    })
    .to_string()
}

/// A parsed mock ruleset: the first rule whose `match` covers the event wins.
/// Deserialized from the JSON file `oneharness mock --rules <path>` reads;
/// unknown fields are rejected loudly (a typo must never become a silent
/// allow-everything).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockRules {
    pub rules: Vec<MockRule>,
}

/// One rule: match criteria plus the action to take when they hold.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockRule {
    #[serde(rename = "match")]
    pub matcher: MatchSpec,
    pub action: Action,
}

/// What a rule matches on. At least one criterion is required (an empty match
/// would silently intercept everything); both must hold when both are given.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// Case-insensitive exact match on the event's tool name. Tool names are
    /// per-harness (`Bash` on Claude Code, `bash` on OpenCode/Crush,
    /// `run_shell_command` on Qwen), so cross-harness rules usually prefer
    /// `event_contains`.
    #[serde(default)]
    pub tool: Option<String>,
    /// Substring match over the raw event JSON — harness-agnostic, because the
    /// tool's command/args always serialize into the event (the same principle
    /// as [`crate::domain::gate::should_deny`]).
    #[serde(default)]
    pub event_contains: Option<String>,
}

/// The interception to perform when a rule matches.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Block the call; the harness surfaces `message` to the model as the
    /// tool's feedback. Expressible wherever `oneharness gate` works.
    Deny { message: String },
    /// Allow the call with `input` substituted for the tool's arguments — the
    /// mock workhorse: rewrite a shell command to a stub that prints the canned
    /// output, or a read's path to a fixture. `input` is passed to the harness
    /// verbatim (each applies its own semantics; Crush shallow-merges).
    Rewrite {
        input: Value,
        /// Optional reason surfaced where the harness's shape carries one.
        #[serde(default)]
        message: Option<String>,
    },
}

impl Action {
    /// Stable token for the spy log and error messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Deny { .. } => "deny",
            Action::Rewrite { .. } => "rewrite",
        }
    }
}

/// Parse and validate a ruleset. Loud on any fault — an unparseable or invalid
/// ruleset must abort the run (usage error), never degrade to allow-everything.
pub fn parse_rules(text: &str) -> Result<MockRules, String> {
    let rules: MockRules = serde_json::from_str(text).map_err(|e| e.to_string())?;
    for (i, rule) in rules.rules.iter().enumerate() {
        let tool_empty = rule.matcher.tool.as_deref().is_none_or(str::is_empty);
        let needle_empty = rule
            .matcher
            .event_contains
            .as_deref()
            .is_none_or(str::is_empty);
        if rule.matcher.tool.as_deref() == Some("")
            || rule.matcher.event_contains.as_deref() == Some("")
        {
            return Err(format!(
                "rule {i}: empty `match` strings are not allowed (an empty needle would match everything)"
            ));
        }
        if tool_empty && needle_empty {
            return Err(format!(
                "rule {i}: `match` needs `tool` and/or `event_contains`"
            ));
        }
        if let Action::Rewrite { input, .. } = &rule.action {
            if !input.is_object() {
                return Err(format!(
                    "rule {i}: `rewrite.input` must be a JSON object (the substituted tool arguments)"
                ));
            }
        }
    }
    Ok(rules)
}

/// The action a ruleset uses that the harness cannot express, if any — checked
/// up front so an unrenderable ruleset is a loud usage error before any event
/// is read, never a silent downgrade at match time.
pub fn unsupported_action(
    rules: &MockRules,
    gate_deny: Option<DenyShape>,
    rewrite: Option<RewriteShape>,
) -> Option<&'static str> {
    for rule in &rules.rules {
        match rule.action {
            Action::Deny { .. } if gate_deny.is_none() => return Some("deny"),
            Action::Rewrite { .. } if rewrite.is_none() => return Some("rewrite"),
            _ => {}
        }
    }
    None
}

/// Decide which rule (if any) intercepts `event` — the raw hook JSON the
/// harness piped to stdin. First match wins; no match means allow-through
/// (empty stdout, the universal "no objection").
pub fn decide<'r>(event: &str, rules: &'r MockRules) -> Option<(usize, &'r Action)> {
    let tool = extract_tool_name(event);
    rules
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| rule_matches(rule, event, tool.as_deref()))
        .map(|(i, rule)| (i, &rule.action))
}

fn rule_matches(rule: &MockRule, event: &str, tool: Option<&str>) -> bool {
    if let Some(want) = rule.matcher.tool.as_deref() {
        // A `tool` criterion can only match an event that names its tool; an
        // empty want never matches (also rejected at parse time).
        match tool {
            Some(name) if !want.is_empty() && name.eq_ignore_ascii_case(want) => {}
            _ => return false,
        }
    }
    if let Some(needle) = rule.matcher.event_contains.as_deref() {
        if needle.is_empty() || !event.contains(needle) {
            return false;
        }
    }
    true
}

/// Best-effort tool name from a hook event: the field every gated harness (and
/// the oneharness OpenCode shim) uses is `tool_name`; `toolName` (Copilot) and
/// `tool` are accepted for robustness. `None` when the event is not JSON or
/// names no tool — a `tool` matcher then simply cannot match (never fabricated).
pub fn extract_tool_name(event: &str) -> Option<String> {
    let value: Value = serde_json::from_str(event.trim()).ok()?;
    for key in ["tool_name", "toolName", "tool"] {
        if let Some(name) = value.get(key).and_then(Value::as_str) {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Render the stdout that substitutes `input` for the tool's arguments,
/// carrying `reason` where the harness's shape has a slot for one. Pure: the
/// returned string is exactly the JSON the harness (or the OpenCode shim)
/// reads; the caller appends the trailing newline.
pub fn render_rewrite(shape: RewriteShape, input: &Value, reason: &str) -> String {
    let value = match shape {
        RewriteShape::ClaudeNested => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "permissionDecisionReason": reason,
                "updatedInput": input,
            }
        }),
        RewriteShape::CrushFlat => json!({
            "version": 1,
            "decision": "allow",
            "reason": reason,
            "updated_input": input,
        }),
        RewriteShape::OpencodeShim => json!({
            "decision": "allow",
            "reason": reason,
            "updated_input": input,
        }),
        // Cursor's probe-verified reply carries no reason slot; sending only
        // what was verified keeps its parser from rejecting the verdict.
        RewriteShape::CursorPermission => json!({
            "permission": "allow",
            "updated_input": input,
        }),
    };
    value.to_string()
}

/// One spy-log line: the observed hook event plus what the responder did with
/// it. Appended as JSONL by the command layer for every invocation — with or
/// without a ruleset — so a consumer gets the *original* tool call (pre-rewrite
/// intent), which the harness's own transcript `events` cannot show once an
/// input was substituted.
#[derive(Debug, Serialize)]
pub struct SpyRecord<'a> {
    pub harness: &'a str,
    /// The raw hook event, parsed when it is JSON (else the raw string, never
    /// dropped).
    pub event: Value,
    /// `allow` (no rule matched — the fall-through), `deny`, or `rewrite`.
    pub action: &'static str,
    /// Index of the matched rule in the ruleset; `null` on the fall-through.
    pub rule: Option<usize>,
}

/// Render the spy-log line for one invocation (no trailing newline).
pub fn spy_line(harness: &str, event: &str, decision: Option<(usize, &Action)>) -> String {
    let record = SpyRecord {
        harness,
        event: serde_json::from_str(event.trim())
            .unwrap_or_else(|_| Value::String(event.to_string())),
        action: decision.map_or("allow", |(_, action)| action.kind()),
        rule: decision.map(|(i, _)| i),
    };
    serde_json::to_string(&record).expect("SpyRecord serialization cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(json: &str) -> MockRules {
        parse_rules(json).expect("test ruleset must parse")
    }

    const REWRITE_RULES: &str = r#"{
        "rules": [
            {
                "match": {"tool": "Bash", "event_contains": "git push"},
                "action": {"deny": {"message": "pushes are mocked"}}
            },
            {
                "match": {"event_contains": "git status"},
                "action": {"rewrite": {"input": {"command": "printf clean"}}}
            }
        ]
    }"#;

    #[test]
    fn parse_accepts_valid_rules_and_rejects_faults_loudly() {
        assert_eq!(rules(REWRITE_RULES).rules.len(), 2);
        // Unknown fields are a loud parse error, not a silent skip.
        assert!(parse_rules(r#"{"rules":[],"extra":1}"#).is_err());
        assert!(parse_rules(
            r#"{"rules":[{"match":{"typo_contains":"x"},"action":{"deny":{"message":"m"}}}]}"#
        )
        .is_err());
        // An empty match would intercept everything — refused.
        let err = parse_rules(r#"{"rules":[{"match":{},"action":{"deny":{"message":"m"}}}]}"#)
            .unwrap_err();
        assert!(err.contains("tool` and/or `event_contains"), "{err}");
        // Empty strings are refused too (an empty needle matches everything).
        for m in [
            r#"{"tool": ""}"#,
            r#"{"event_contains": ""}"#,
            r#"{"tool": "", "event_contains": "x"}"#,
        ] {
            let text =
                format!(r#"{{"rules":[{{"match":{m},"action":{{"deny":{{"message":"m"}}}}}}]}}"#);
            assert!(parse_rules(&text).is_err(), "{m} must be rejected");
        }
        // A rewrite's input must be the substituted arguments object.
        let err = parse_rules(
            r#"{"rules":[{"match":{"tool":"Bash"},"action":{"rewrite":{"input":"echo"}}}]}"#,
        )
        .unwrap_err();
        assert!(err.contains("must be a JSON object"), "{err}");
        // Not JSON at all.
        assert!(parse_rules("not json").is_err());
    }

    #[test]
    fn decide_first_match_wins_and_falls_through() {
        let r = rules(REWRITE_RULES);
        // Rule 0: tool + substring both hold.
        let event = r#"{"tool_name":"Bash","tool_input":{"command":"git push origin"}}"#;
        let (i, action) = decide(event, &r).expect("must match");
        assert_eq!(i, 0);
        assert_eq!(action.kind(), "deny");
        // Rule 1 matches on substring alone (no tool criterion).
        let event = r#"{"tool_name":"shell","tool_input":{"command":"git status"}}"#;
        let (i, action) = decide(event, &r).expect("must match");
        assert_eq!(i, 1);
        assert_eq!(action.kind(), "rewrite");
        // Neither: fall through.
        assert!(decide(r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#, &r).is_none());
        // Rule 0 requires BOTH criteria: right tool, wrong substring -> rule 1
        // doesn't match either -> fall through.
        assert!(decide(r#"{"tool_name":"Bash","tool_input":{"command":"rm"}}"#, &r).is_none());
        // Wrong tool with rule 0's substring: rule 0 misses, no other rule
        // carries "git push" -> fall through (tool criterion is enforced).
        assert!(decide(
            r#"{"tool_name":"Edit","tool_input":{"command":"git push"}}"#,
            &r
        )
        .is_none());
    }

    #[test]
    fn tool_matching_is_case_insensitive_and_needs_a_named_tool() {
        let r = rules(r#"{"rules":[{"match":{"tool":"bash"},"action":{"deny":{"message":"m"}}}]}"#);
        assert!(decide(r#"{"tool_name":"Bash"}"#, &r).is_some());
        assert!(decide(r#"{"toolName":"BASH"}"#, &r).is_some());
        assert!(decide(r#"{"tool":"bash"}"#, &r).is_some());
        // No tool name in the event: a tool criterion cannot match (honest, not
        // fabricated) — and a non-JSON event can never satisfy it either.
        assert!(decide(r#"{"command":"bash stuff"}"#, &r).is_none());
        assert!(decide("not json", &r).is_none());
    }

    #[test]
    fn extract_tool_name_reads_known_fields_only() {
        assert_eq!(
            extract_tool_name(r#"{"tool_name":"Bash"}"#).as_deref(),
            Some("Bash")
        );
        assert_eq!(
            extract_tool_name(r#"{"toolName":"shell"}"#).as_deref(),
            Some("shell")
        );
        assert_eq!(
            extract_tool_name(r#"{"tool":"bash"}"#).as_deref(),
            Some("bash")
        );
        // Precedence: tool_name first (the field every gated harness uses).
        assert_eq!(
            extract_tool_name(r#"{"tool":"b","tool_name":"a"}"#).as_deref(),
            Some("a")
        );
        assert!(extract_tool_name(r#"{"tool_name":""}"#).is_none());
        assert!(extract_tool_name(r#"{"tool_name":42}"#).is_none());
        assert!(extract_tool_name("nope").is_none());
    }

    #[test]
    fn unsupported_action_reports_the_first_unrenderable_verb() {
        let r = rules(REWRITE_RULES);
        // Everything renderable: no complaint.
        assert!(unsupported_action(
            &r,
            Some(DenyShape::ClaudeNested),
            Some(RewriteShape::ClaudeNested)
        )
        .is_none());
        // No rewrite shape: the rewrite rule is unrenderable.
        assert_eq!(
            unsupported_action(&r, Some(DenyShape::ClaudeNested), None),
            Some("rewrite")
        );
        // No deny shape either: deny reported (it is the first offending rule).
        assert_eq!(unsupported_action(&r, None, None), Some("deny"));
        // A deny-only ruleset needs no rewrite shape.
        let deny_only =
            rules(r#"{"rules":[{"match":{"tool":"Bash"},"action":{"deny":{"message":"m"}}}]}"#);
        assert!(unsupported_action(&deny_only, Some(DenyShape::CopilotFlat), None).is_none());
    }

    #[test]
    fn render_rewrite_matches_each_protocol() {
        let input = json!({"command": "printf mocked"});
        let claude: Value =
            serde_json::from_str(&render_rewrite(RewriteShape::ClaudeNested, &input, "r")).unwrap();
        assert_eq!(claude["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(claude["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            claude["hookSpecificOutput"]["permissionDecisionReason"],
            "r"
        );
        assert_eq!(
            claude["hookSpecificOutput"]["updatedInput"]["command"],
            "printf mocked"
        );
        let crush: Value =
            serde_json::from_str(&render_rewrite(RewriteShape::CrushFlat, &input, "r")).unwrap();
        assert_eq!(crush["version"], 1);
        assert_eq!(crush["decision"], "allow");
        assert_eq!(crush["updated_input"]["command"], "printf mocked");
        let oc: Value =
            serde_json::from_str(&render_rewrite(RewriteShape::OpencodeShim, &input, "r")).unwrap();
        assert_eq!(oc["decision"], "allow");
        assert_eq!(oc["updated_input"]["command"], "printf mocked");
        assert!(oc.get("hookSpecificOutput").is_none());
        // Cursor: permission + updated_input ONLY — the probe-verified reply
        // carries no reason field, so none may be added.
        let cursor: Value =
            serde_json::from_str(&render_rewrite(RewriteShape::CursorPermission, &input, "r"))
                .unwrap();
        assert_eq!(
            cursor,
            json!({"permission": "allow", "updated_input": {"command": "printf mocked"}})
        );
    }

    #[test]
    fn hook_command_wires_rules_and_spy() {
        assert_eq!(
            hook_command("/bin/oh", "crush", Some("/w/r.json"), Some("/w/s.jsonl")),
            "/bin/oh mock crush --rules /w/r.json --spy-file /w/s.jsonl"
        );
        // Spy-only (no rules): a pure observer hook.
        assert_eq!(
            hook_command("/bin/oh", "goose", None, Some("/w/s.jsonl")),
            "/bin/oh mock goose --spy-file /w/s.jsonl"
        );
        assert_eq!(
            hook_command("oh", "claude-code", Some("r.json"), None),
            "oh mock claude-code --rules r.json"
        );
    }

    #[test]
    fn settings_hooks_json_matches_the_probe_verified_shape() {
        let v: Value = serde_json::from_str(&settings_hooks_json("oh mock claude-code")).unwrap();
        assert_eq!(
            v,
            json!({
                "hooks": {
                    "PreToolUse": [
                        { "hooks": [ { "type": "command", "command": "oh mock claude-code" } ] }
                    ]
                }
            })
        );
    }

    #[test]
    fn rewrite_shape_tokens_are_stable() {
        assert_eq!(RewriteShape::ClaudeNested.as_str(), "claude-nested");
        assert_eq!(RewriteShape::CrushFlat.as_str(), "crush-flat");
        assert_eq!(RewriteShape::OpencodeShim.as_str(), "opencode-shim");
        assert_eq!(RewriteShape::CursorPermission.as_str(), "cursor-permission");
    }

    #[test]
    fn spy_line_records_event_action_and_rule() {
        let r = rules(REWRITE_RULES);
        let event = r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#;
        let decision = decide(event, &r);
        let line: Value = serde_json::from_str(&spy_line("claude-code", event, decision)).unwrap();
        assert_eq!(line["harness"], "claude-code");
        assert_eq!(line["action"], "deny");
        assert_eq!(line["rule"], 0);
        // The event is embedded as parsed JSON, so consumers query it directly.
        assert_eq!(line["event"]["tool_input"]["command"], "git push");
        // Fall-through: action `allow`, rule null; a non-JSON event is kept as
        // a string, never dropped.
        let line: Value = serde_json::from_str(&spy_line("goose", "raw text", None)).unwrap();
        assert_eq!(line["action"], "allow");
        assert!(line["rule"].is_null());
        assert_eq!(line["event"], "raw text");
    }
}
