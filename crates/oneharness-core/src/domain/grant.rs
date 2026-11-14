//! The run-level allowlist: `allowed_tools` outside `[sync]`, applied by
//! `oneharness run` to that one run and written nowhere that outlives it.
//!
//! This module is the pure half. It resolves each configured rule against the
//! run's own environment ([`resolve`]), refuses what a mode cannot grant, and
//! plans how one harness receives the result ([`plan`]): the Claude Code
//! `--allowedTools` entries, the Codex `prefix_rule` statements of a per-run
//! rules file, the statement the turn's system instructions carry, and the
//! diagnostic a harness that withholds the grant reports. Creating the per-run
//! overlay that holds a rules file is the `io` layer's (`io::run_overlay`).
//!
//! Two findings of the `spike-command-grant` measurements shape it. A rule over
//! a variable's *text* matches nothing on Claude Code and Codex and widens to
//! any executable on OpenCode, so a command written `$NAME` is resolved to the
//! absolute path the variable holds before any harness rule is built, and the
//! agent is told to type that path. And under `read-only` only a plain command
//! naming one executable is a grant every enforcing harness re-checks after
//! shell parsing, so that mode accepts nothing else.

use crate::domain::lean::{Diagnostic, Setting};
use crate::domain::mode::PermissionMode;
use crate::domain::sync::exec_policy_pattern;

/// Claude Code's registry id.
const CLAUDE: &str = "claude-code";
/// Codex's registry id.
const CODEX: &str = "codex";

/// One configured run-level rule, with any `$NAME` command resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    /// The rule exactly as the config spells it.
    pub written: String,
    /// The rule with a `$NAME` command replaced by the path it resolved to;
    /// equal to [`Self::written`] when the rule names no variable.
    pub resolved: String,
    /// The absolute path of the one executable a `Bash(<abs>)` or
    /// `Bash(<abs>:*)` rule names, after resolution; `None` for any other rule.
    pub executable: Option<String>,
}

/// Why a run-level rule cannot be applied. Every variant names the rule as
/// written, so the usage error points at the config line to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefusal {
    /// A `$NAME` command whose variable is unset, or set to the empty string,
    /// in the run's environment.
    VariableUnset { rule: String, name: String },
    /// A `$NAME` command whose variable holds something other than one
    /// absolute path.
    VariableNotAbsolute {
        rule: String,
        name: String,
        value: String,
    },
    /// A rule the mode cannot grant: under `read-only` only `Bash(<abs>)` and
    /// `Bash(<abs>:*)` naming one executable are accepted.
    NotAnExecutable {
        rule: String,
        mode: PermissionMode,
        why: &'static str,
    },
}

impl GrantRefusal {
    /// The rule the refusal is about, as the config spells it.
    #[must_use]
    pub fn rule(&self) -> &str {
        match self {
            GrantRefusal::VariableUnset { rule, .. }
            | GrantRefusal::VariableNotAbsolute { rule, .. }
            | GrantRefusal::NotAnExecutable { rule, .. } => rule,
        }
    }

    /// What is wrong with the rule, as one sentence fragment.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            GrantRefusal::VariableUnset { name, .. } => format!(
                "`${name}` is unset or empty in the run's environment (after `env`, `env_from` \
                 and `unset_env`), so it names no executable"
            ),
            GrantRefusal::VariableNotAbsolute { name, value, .. } => format!(
                "`${name}` holds `{value}`, which is not one absolute path; a run-level rule \
                 grants an executable by its absolute path"
            ),
            GrantRefusal::NotAnExecutable { mode, why, .. } => format!(
                "under `--mode {}` a run-level rule may only name one executable, as \
                 `Bash(<absolute path>)` or `Bash(<absolute path>:*)` (`<absolute path>` may be \
                 written `$NAME`): {why}",
                mode.as_str()
            ),
        }
    }
}

/// Why a rule is not an executable grant, for [`GrantRefusal::NotAnExecutable`].
mod why {
    pub const NOT_BASH: &str = "it is not a `Bash(...)` rule, so it would grant a tool rather \
                                than an executable";
    pub const WHOLE_SHELL: &str = "a bare `Bash` grants the whole shell";
    pub const EMPTY: &str = "it names no command";
    pub const MULTI_WORD: &str = "it names a command prefix of several words, which a harness \
                                  matches as text rather than as one executable";
    pub const WILDCARD: &str = "it holds a wildcard, which matches more than one executable";
    pub const RELATIVE: &str = "its command is not an absolute path, so it does not name one \
                                executable";
    pub const SHELL_SYNTAX: &str = "its command holds shell syntax, which no harness re-checks \
                                    as one executable";
}

/// The characters that make a command more than one plain word to a shell.
const SHELL_SYNTAX: &[char] = &[
    ';', '&', '|', '<', '>', '(', ')', '`', '$', '"', '\'', '\\', '{', '}', '#', '~', '!', '=',
];

/// Resolve every rule of a run's allowlist against its environment.
///
/// `lookup` answers a variable's value in the environment the harness will be
/// spawned with — after `env`, `env_from` and `unset_env` apply — so a `$NAME`
/// command resolves to exactly what the run would see. Pure: the caller hands
/// in the lookup.
///
/// A rule `Bash(<cmd>)` or `Bash(<cmd>:*)` whose `<cmd>` opens with a `$NAME`
/// word has that word replaced by the variable's value, which must be one
/// absolute path. Under [`PermissionMode::ReadOnly`] every rule must name
/// exactly one executable by its absolute path; any other rule is refused.
///
/// # Errors
///
/// The first rule that cannot be applied, with why.
pub fn resolve(
    rules: &[String],
    mode: PermissionMode,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<ResolvedRule>, GrantRefusal> {
    rules
        .iter()
        .map(|rule| resolve_one(rule, mode, lookup))
        .collect()
}

fn resolve_one(
    rule: &str,
    mode: PermissionMode,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedRule, GrantRefusal> {
    let refuse = |why: &'static str| GrantRefusal::NotAnExecutable {
        rule: rule.to_string(),
        mode,
        why,
    };
    let read_only = mode == PermissionMode::ReadOnly;
    let Some(inner) = bash_inner(rule) else {
        if read_only {
            return Err(refuse(if rule.trim() == "Bash" {
                why::WHOLE_SHELL
            } else {
                why::NOT_BASH
            }));
        }
        return Ok(ResolvedRule {
            written: rule.to_string(),
            resolved: rule.to_string(),
            executable: None,
        });
    };
    let (command, suffix) = match inner.strip_suffix(":*") {
        Some(command) => (command, ":*"),
        None => (inner, ""),
    };
    let command = command.trim();
    let (first, rest) = match command.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim_start()),
        None => (command, ""),
    };
    let first = match first.strip_prefix('$') {
        Some(name) => resolve_variable(rule, name, lookup)?,
        None => first.to_string(),
    };
    let resolved_command = if rest.is_empty() {
        first.clone()
    } else {
        format!("{first} {rest}")
    };
    let resolved = format!("Bash({resolved_command}{suffix})");
    let single_absolute = rest.is_empty() && is_absolute(&first) && !holds_syntax(&first);
    if read_only {
        if first.is_empty() {
            return Err(refuse(why::EMPTY));
        }
        if !rest.is_empty() {
            return Err(refuse(why::MULTI_WORD));
        }
        if first.contains(['*', '?', '[']) {
            return Err(refuse(why::WILDCARD));
        }
        if holds_syntax(&first) {
            return Err(refuse(why::SHELL_SYNTAX));
        }
        if !is_absolute(&first) {
            return Err(refuse(why::RELATIVE));
        }
    }
    Ok(ResolvedRule {
        written: rule.to_string(),
        resolved,
        executable: single_absolute.then_some(first),
    })
}

/// The text inside `Bash(...)`, or `None` for any other rule.
fn bash_inner(rule: &str) -> Option<&str> {
    rule.trim()
        .strip_prefix("Bash(")
        .and_then(|rest| rest.strip_suffix(')'))
}

fn resolve_variable(
    rule: &str,
    name: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<String, GrantRefusal> {
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(GrantRefusal::NotAnExecutable {
            rule: rule.to_string(),
            mode: PermissionMode::ReadOnly,
            why: why::SHELL_SYNTAX,
        });
    }
    let value = lookup(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GrantRefusal::VariableUnset {
            rule: rule.to_string(),
            name: name.to_string(),
        })?;
    if !is_absolute(&value)
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
        || holds_syntax(&value)
    {
        return Err(GrantRefusal::VariableNotAbsolute {
            rule: rule.to_string(),
            name: name.to_string(),
            value,
        });
    }
    Ok(value)
}

/// Whether `path` is absolute on the host's own convention. Pure: it reads
/// only the string.
fn is_absolute(path: &str) -> bool {
    std::path::Path::new(path).is_absolute()
}

fn holds_syntax(word: &str) -> bool {
    word.contains(SHELL_SYNTAX)
}

/// How one harness receives a run's resolved allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantPlan {
    /// Claude Code: the entries of the run's `--allowedTools`. Empty for every
    /// other harness, and when nothing is granted.
    pub claude_allowed: Vec<String>,
    /// Codex: the per-run rules file's whole text, one `prefix_rule` per
    /// translatable rule; `None` for every other harness, and when no rule
    /// translates.
    pub codex_rules: Option<String>,
    /// The statement the turn's system instructions gain: each granted
    /// executable by its absolute path and how to type it. `None` when no
    /// `Bash` rule naming an executable is applied.
    pub statement: Option<String>,
    /// What the run could not apply: the whole grant on a harness that
    /// withholds it, or each rule its harness cannot express.
    pub diagnostics: Vec<Diagnostic>,
}

impl GrantPlan {
    /// Whether any rule reaches the harness.
    #[must_use]
    pub fn applies(&self) -> bool {
        !self.claude_allowed.is_empty() || self.codex_rules.is_some()
    }
}

/// The comment the per-run Codex rules file opens with.
pub const RUN_RULES_HEADER: &str = "# Written by `oneharness run` for this run alone, from the \
run-level `allowed_tools`.\n# It lives in the run's overlay and is removed when the run ends.\n";

/// The file name of the per-run Codex rules file inside the overlay's `rules/`.
pub const RUN_RULES_FILE: &str = "oneharness-run.rules";

/// Why each harness that has no per-run mechanism proven against its real CLI
/// withholds the run-level allowlist, by base id. Claude Code and Codex grant
/// it (Codex subject to the overlay checks the `io` layer makes).
#[must_use]
pub fn withheld_reason(id: &str) -> Option<&'static str> {
    Some(match id {
        CLAUDE | CODEX => return None,
        "opencode" => {
            "its `read-only` mapping (`--agent plan`) is not a no-mutation guarantee on \
             opencode 1.18.22 — a redirection or any executable writes the worktree under it — \
             and a permission override reaches only the plan agent, not a subagent it \
             delegates to, so a grant cannot be held to exactly the named executables"
        }
        _ => {
            "it has no per-run mechanism that a test against its real CLI proves applies the \
             allowlist to this run alone and nothing wider"
        }
    })
}

/// Plan how harness `id` (a base id) receives `rules` under `mode`.
///
/// `withhold` is a reason the `io` layer found that this run cannot carry the
/// grant on this harness (Codex credentials in the keyring, a missing home);
/// with one, or with a harness that has no per-run mechanism, the plan applies
/// nothing and carries one diagnostic naming the withheld rules.
#[must_use]
pub fn plan(
    id: &str,
    mode: PermissionMode,
    rules: &[ResolvedRule],
    withhold: Option<&str>,
) -> GrantPlan {
    let mut out = GrantPlan::default();
    if rules.is_empty() {
        return out;
    }
    if let Some(reason) = withhold.or_else(|| withheld_reason(id)) {
        out.diagnostics.push(Diagnostic::new(
            id,
            Setting::AllowedTools,
            format!(
                "the run-level allowlist ({}) was withheld on `{id}`, which ran as it does with \
                 none: {reason}",
                quoted(rules.iter().map(|r| r.written.as_str()))
            ),
        ));
        return out;
    }
    let read_only = mode == PermissionMode::ReadOnly;
    let mut granted: Vec<&str> = Vec::new();
    match id {
        CLAUDE => {
            for rule in rules {
                match (&rule.executable, read_only) {
                    // Under read-only a rule grants its executable with any
                    // arguments, which Claude Code spells `:*`.
                    (Some(path), true) => out.claude_allowed.push(format!("Bash({path}:*)")),
                    _ => out.claude_allowed.push(rule.resolved.clone()),
                }
                if let Some(path) = &rule.executable {
                    granted.push(path);
                }
            }
        }
        CODEX => {
            let mut text = String::from(RUN_RULES_HEADER);
            let mut unmapped: Vec<&str> = Vec::new();
            for rule in rules {
                let tokens = match (&rule.executable, read_only) {
                    (Some(path), true) => Ok(vec![path.clone()]),
                    _ => exec_policy_pattern(&rule.resolved),
                };
                match tokens {
                    Ok(tokens) => {
                        text.push_str(&format!("\n# allowed_tools: {}\n", rule.resolved));
                        text.push_str(&crate::domain::sync::exec_policy_statement_for(
                            &tokens, "allow",
                        ));
                        text.push('\n');
                        if let Some(path) = &rule.executable {
                            granted.push(path);
                        }
                    }
                    Err(_) => unmapped.push(&rule.written),
                }
            }
            if !unmapped.is_empty() {
                out.diagnostics.push(Diagnostic::new(
                    id,
                    Setting::AllowedTools,
                    format!(
                        "Codex cannot express {} as a per-run exec-policy rule, so those rules \
                         were not applied; the rest were",
                        quoted(unmapped.iter().copied())
                    ),
                ));
            }
            if unmapped.len() < rules.len() {
                out.codex_rules = Some(text);
            }
        }
        _ => unreachable!("withheld_reason answers every harness but claude-code and codex"),
    }
    granted.dedup();
    out.statement = statement(&granted);
    out
}

/// The statement a turn's system instructions gain for `paths`, or `None`
/// when no executable is granted. The wording is oneharness's own; what a
/// consumer may rely on is that each path appears in it.
#[must_use]
pub fn statement(paths: &[&str]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let list = paths
        .iter()
        .map(|path| format!("`{path}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "This run may execute these commands: {list}. To run one, type its absolute path \
         exactly as written here, as one plain command on its own — not through a variable, \
         alias or function, and with no `;`, `&&`, `||`, pipe, redirection, command \
         substitution or second line — and pass any text it needs as one quoted argument \
         (single quotes, or double quotes when the text holds an apostrophe), which may span \
         several lines."
    ))
}

/// The system text a granted turn carries: the configured `system`, then the
/// grant's statement, or whichever of the two exists.
#[must_use]
pub fn with_statement(system: Option<&str>, statement: Option<&str>) -> Option<String> {
    match (system, statement) {
        (Some(system), Some(statement)) => Some(format!("{system}\n\n{statement}")),
        (system, statement) => system.or(statement).map(str::to_string),
    }
}

fn quoted<'a>(rules: impl Iterator<Item = &'a str>) -> String {
    rules
        .map(|rule| format!("`{rule}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "TOOL" => Some("/opt/bin/decline".into()),
            "EMPTY" => Some(String::new()),
            "REL" => Some("bin/decline".into()),
            "SPACED" => Some("/opt/my tool".into()),
            _ => None,
        }
    }

    fn one(rule: &str, mode: PermissionMode) -> Result<ResolvedRule, GrantRefusal> {
        resolve(&[rule.to_string()], mode, &env).map(|mut v| v.remove(0))
    }

    #[test]
    fn a_variable_command_resolves_to_its_absolute_path_in_every_spelling() {
        for (rule, resolved) in [
            ("Bash($TOOL)", "Bash(/opt/bin/decline)"),
            ("Bash($TOOL:*)", "Bash(/opt/bin/decline:*)"),
        ] {
            for mode in [PermissionMode::ReadOnly, PermissionMode::Default] {
                let r = one(rule, mode).unwrap();
                assert_eq!(r.resolved, resolved);
                assert_eq!(r.executable.as_deref(), Some("/opt/bin/decline"));
                assert_eq!(r.written, rule);
            }
        }
        let prefix = one("Bash($TOOL status:*)", PermissionMode::Default).unwrap();
        assert_eq!(prefix.resolved, "Bash(/opt/bin/decline status:*)");
        assert_eq!(prefix.executable, None);
    }

    #[test]
    fn an_unusable_variable_is_refused_naming_the_rule() {
        for name in ["UNSET", "EMPTY"] {
            let rule = format!("Bash(${name}:*)");
            let err = one(&rule, PermissionMode::Default).unwrap_err();
            assert_eq!(
                err,
                GrantRefusal::VariableUnset {
                    rule: rule.clone(),
                    name: name.into()
                }
            );
            assert!(err.reason().contains(&format!("`${name}`")));
        }
        for name in ["REL", "SPACED"] {
            let err = one(&format!("Bash(${name})"), PermissionMode::ReadOnly).unwrap_err();
            assert!(
                matches!(err, GrantRefusal::VariableNotAbsolute { .. }),
                "{err:?}"
            );
            assert!(err.reason().contains("not one absolute path"));
        }
        let odd = one("Bash(${TOOL}:*)", PermissionMode::Default).unwrap_err();
        assert!(matches!(odd, GrantRefusal::NotAnExecutable { .. }));
    }

    #[test]
    fn read_only_accepts_only_one_absolute_executable() {
        for rule in [
            "Edit",
            "Write",
            "Read",
            "Bash",
            "Bash(*)",
            "Bash(/opt/bin/*)",
            "Bash(git status:*)",
            "Bash(/opt/bin/decline --ruling:*)",
            "Bash(decline:*)",
            "Bash(:*)",
            "Bash(/opt/bin/a;b)",
        ] {
            let err = one(rule, PermissionMode::ReadOnly).unwrap_err();
            assert_eq!(err.rule(), rule);
            assert!(err.reason().contains("--mode read-only"), "{rule}: {err:?}");
        }
        let ok = one("Bash(/opt/bin/decline)", PermissionMode::ReadOnly).unwrap();
        assert_eq!(ok.executable.as_deref(), Some("/opt/bin/decline"));
    }

    #[test]
    fn other_modes_pass_any_rule_through_resolved() {
        for rule in ["Edit", "Bash(git status:*)", "Bash(decline:*)", "Bash"] {
            let r = one(rule, PermissionMode::Default).unwrap();
            assert_eq!(r.resolved, rule);
            assert_eq!(r.executable, None);
        }
    }

    fn rules(list: &[&str], mode: PermissionMode) -> Vec<ResolvedRule> {
        let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        resolve(&list, mode, &env).unwrap()
    }

    #[test]
    fn claude_gets_each_rule_on_allowed_tools_and_read_only_grants_any_arguments() {
        let ro = rules(&["Bash($TOOL)"], PermissionMode::ReadOnly);
        let p = plan(CLAUDE, PermissionMode::ReadOnly, &ro, None);
        assert_eq!(p.claude_allowed, ["Bash(/opt/bin/decline:*)"]);
        assert!(p.statement.unwrap().contains("`/opt/bin/decline`"));
        assert!(p.diagnostics.is_empty());
        let def = rules(&["Bash($TOOL)", "Edit"], PermissionMode::Default);
        let p = plan(CLAUDE, PermissionMode::Default, &def, None);
        assert_eq!(p.claude_allowed, ["Bash(/opt/bin/decline)", "Edit"]);
    }

    #[test]
    fn codex_gets_one_allow_prefix_rule_per_translatable_rule() {
        let ro = rules(
            &["Bash($TOOL)", "Bash(/usr/bin/x:*)"],
            PermissionMode::ReadOnly,
        );
        let p = plan(CODEX, PermissionMode::ReadOnly, &ro, None);
        let text = p.codex_rules.unwrap();
        assert!(text.starts_with(RUN_RULES_HEADER));
        assert!(text.contains("prefix_rule(pattern=[\"/opt/bin/decline\"], decision=\"allow\")"));
        assert!(text.contains("prefix_rule(pattern=[\"/usr/bin/x\"], decision=\"allow\")"));
        assert!(!text.contains('$'));
        let s = p.statement.unwrap();
        assert!(s.contains("`/opt/bin/decline`") && s.contains("`/usr/bin/x`"));

        let def = rules(
            &["Bash($TOOL)", "Edit", "Bash(git status:*)"],
            PermissionMode::Default,
        );
        let p = plan(CODEX, PermissionMode::Default, &def, None);
        let text = p.codex_rules.unwrap();
        assert!(text.contains("pattern=[\"git\", \"status\"]"));
        assert!(!text.contains("decline"));
        assert_eq!(p.diagnostics.len(), 1);
        assert!(p.diagnostics[0].message.contains("`Bash($TOOL)`, `Edit`"));
        assert_eq!(p.statement, None);
    }

    #[test]
    fn a_harness_without_a_proven_mechanism_withholds_and_says_so() {
        let ro = rules(&["Bash($TOOL)"], PermissionMode::ReadOnly);
        for id in ["opencode", "copilot", "cursor", "qwen", "goose", "crush"] {
            let p = plan(id, PermissionMode::ReadOnly, &ro, None);
            assert!(!p.applies(), "{id}");
            assert_eq!(p.statement, None);
            assert_eq!(p.diagnostics.len(), 1);
            assert_eq!(p.diagnostics[0].setting, Setting::AllowedTools);
            assert!(p.diagnostics[0].message.contains("`Bash($TOOL)`"));
            assert!(p.diagnostics[0].message.contains(&format!("`{id}`")));
        }
        let p = plan(
            CODEX,
            PermissionMode::ReadOnly,
            &ro,
            Some("its login is in the keyring"),
        );
        assert!(!p.applies());
        assert!(p.diagnostics[0].message.contains("keyring"));
        assert_eq!(
            plan(CLAUDE, PermissionMode::ReadOnly, &[], None),
            GrantPlan::default()
        );
    }
}
