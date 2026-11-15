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
use crate::domain::sync::{exec_policy_pattern, exec_policy_statement_for, RuleList};

const CLAUDE: &str = "claude-code";
const CODEX: &str = "codex";

/// The characters that make a command more than one plain word to a shell.
const SHELL_SYNTAX: &[char] = &[
    ';', '&', '|', '<', '>', '(', ')', '`', '$', '"', '\'', '\\', '{', '}', '#', '~', '!', '=',
];

/// The absolute path of one executable a rule grants: absolute on the host's
/// convention, one word, and free of shell syntax — so it is exactly what an
/// agent types, and exactly what a harness's rule can match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutablePath(String);

impl ExecutablePath {
    /// `word` as an executable path, or `None` when it is not one.
    #[must_use]
    pub fn new(word: &str) -> Option<ExecutablePath> {
        let plain = !word.is_empty()
            && !word.contains(SHELL_SYNTAX)
            && !word.contains(['*', '?', '['])
            && !word.chars().any(|c| c.is_whitespace() || c.is_control());
        (plain && std::path::Path::new(word).is_absolute())
            .then(|| ExecutablePath(word.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which invocations of an executable a rule grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arguments {
    /// `Bash(<abs>)`: the bare command alone, outside `read-only`.
    Exact,
    /// `Bash(<abs>:*)`: the command with any arguments. Under `read-only`
    /// both spellings grant this.
    Any,
}

/// What one resolved run-level rule grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// `Bash(<abs>)` or `Bash(<abs>:*)`: one executable, by its path.
    Executable {
        path: ExecutablePath,
        arguments: Arguments,
    },
    /// Any other rule, outside `read-only` only, with a leading `$NAME`
    /// command word resolved, passed on as text.
    Other { resolved: String },
}

/// One configured run-level rule, resolved. Built only by [`resolve`], so a
/// rule's kind always agrees with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    written: String,
    kind: RuleKind,
}

impl ResolvedRule {
    /// The rule exactly as the config spells it.
    #[must_use]
    pub fn written(&self) -> &str {
        &self.written
    }

    #[must_use]
    pub fn kind(&self) -> &RuleKind {
        &self.kind
    }

    /// The rule with a `$NAME` command replaced by the path it resolved to.
    #[must_use]
    pub fn resolved(&self) -> String {
        match &self.kind {
            RuleKind::Executable { path, arguments } => format!(
                "Bash({}{})",
                path.as_str(),
                match arguments {
                    Arguments::Any => ":*",
                    Arguments::Exact => "",
                }
            ),
            RuleKind::Other { resolved } => resolved.clone(),
        }
    }

    /// The executable a `Bash(<abs>)`/`Bash(<abs>:*)` rule names.
    #[must_use]
    pub fn executable(&self) -> Option<&ExecutablePath> {
        match &self.kind {
            RuleKind::Executable { path, .. } => Some(path),
            RuleKind::Other { .. } => None,
        }
    }
}

/// Why a rule cannot be granted under `read-only`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotExecutable {
    /// Not a `Bash(...)` rule: it would grant a tool.
    NotBash,
    /// A bare `Bash`: the whole shell.
    WholeShell,
    /// `Bash()` or `Bash(:*)`.
    Empty,
    /// A command prefix of several words.
    MultiWord,
    /// A glob in the command.
    Wildcard,
    /// Shell syntax in the command.
    ShellSyntax,
    /// A command that is not an absolute path.
    Relative,
}

impl NotExecutable {
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            NotExecutable::NotBash => {
                "it is not a `Bash(...)` rule, so it would grant a tool rather than an executable"
            }
            NotExecutable::WholeShell => "a bare `Bash` grants the whole shell",
            NotExecutable::Empty => "it names no command",
            NotExecutable::MultiWord => {
                "it names a command prefix of several words, which a harness matches as text \
                 rather than as one executable"
            }
            NotExecutable::Wildcard => {
                "it holds a wildcard, which matches more than one executable"
            }
            NotExecutable::ShellSyntax => {
                "its command holds shell syntax, which no harness re-checks as one executable"
            }
            NotExecutable::Relative => {
                "its command is not an absolute path, so it does not name one executable"
            }
        }
    }
}

/// Why a run-level rule cannot be applied. Every variant names the rule as
/// written, so the usage error points at the config line to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefusal {
    /// A command `$<name>` whose `<name>` is no variable name.
    InvalidVariable { rule: String, name: String },
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
    /// A rule `read-only` cannot grant: only `Bash(<abs>)` and `Bash(<abs>:*)`
    /// naming one executable are accepted there.
    NotAnExecutable {
        rule: String,
        mode: PermissionMode,
        why: NotExecutable,
    },
}

impl GrantRefusal {
    /// The rule the refusal is about, as the config spells it.
    #[must_use]
    pub fn rule(&self) -> &str {
        match self {
            GrantRefusal::InvalidVariable { rule, .. }
            | GrantRefusal::VariableUnset { rule, .. }
            | GrantRefusal::VariableNotAbsolute { rule, .. }
            | GrantRefusal::NotAnExecutable { rule, .. } => rule,
        }
    }

    /// What is wrong with the rule, as one sentence fragment.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            GrantRefusal::InvalidVariable { name, .. } => format!(
                "`${name}` is not a variable name (letters, digits and `_`, not starting with a \
                 digit)"
            ),
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
                 written `$NAME`): {}",
                mode.as_str(),
                why.describe()
            ),
        }
    }
}

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
) -> Result<ResolvedRules, GrantRefusal> {
    let rules = rules
        .iter()
        .map(|rule| resolve_one(rule, mode, lookup))
        .collect::<Result<_, _>>()?;
    Ok(ResolvedRules { mode, rules })
}

/// A run's rules, resolved and checked under the one mode [`plan`] then
/// plans them for: built only by [`resolve`], so a rule another mode accepted
/// never reaches a `read-only` plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRules {
    mode: PermissionMode,
    rules: Vec<ResolvedRule>,
}

impl ResolvedRules {
    #[must_use]
    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    #[must_use]
    pub fn rules(&self) -> &[ResolvedRule] {
        &self.rules
    }
}

fn resolve_one(
    rule: &str,
    mode: PermissionMode,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedRule, GrantRefusal> {
    let refuse = |why: NotExecutable| GrantRefusal::NotAnExecutable {
        rule: rule.to_string(),
        mode,
        why,
    };
    let read_only = mode == PermissionMode::ReadOnly;
    let other = |resolved: String| ResolvedRule {
        written: rule.to_string(),
        kind: RuleKind::Other { resolved },
    };
    let Some(inner) = rule
        .trim()
        .strip_prefix("Bash(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        if read_only {
            return Err(refuse(if rule.trim() == "Bash" {
                NotExecutable::WholeShell
            } else {
                NotExecutable::NotBash
            }));
        }
        return Ok(other(rule.to_string()));
    };
    let (command, arguments) = match inner.strip_suffix(":*") {
        Some(command) => (command.trim(), Arguments::Any),
        None => (inner.trim(), Arguments::Exact),
    };
    let (first, rest) = match command.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim_start()),
        None => (command, ""),
    };
    let first = match first.strip_prefix('$') {
        Some(name) => resolve_variable(rule, name, lookup)?,
        None => first.to_string(),
    };
    if rest.is_empty() {
        if let Some(path) = ExecutablePath::new(&first) {
            return Ok(ResolvedRule {
                written: rule.to_string(),
                kind: RuleKind::Executable { path, arguments },
            });
        }
    }
    if read_only {
        let why = if first.is_empty() {
            NotExecutable::Empty
        } else if !rest.is_empty() {
            NotExecutable::MultiWord
        } else if first.contains(['*', '?', '[']) {
            NotExecutable::Wildcard
        } else if first.contains(SHELL_SYNTAX) {
            NotExecutable::ShellSyntax
        } else {
            NotExecutable::Relative
        };
        return Err(refuse(why));
    }
    let command = if rest.is_empty() {
        first
    } else {
        format!("{first} {rest}")
    };
    let suffix = match arguments {
        Arguments::Any => ":*",
        Arguments::Exact => "",
    };
    Ok(other(format!("Bash({command}{suffix})")))
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
        return Err(GrantRefusal::InvalidVariable {
            rule: rule.to_string(),
            name: name.to_string(),
        });
    }
    let value = lookup(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GrantRefusal::VariableUnset {
            rule: rule.to_string(),
            name: name.to_string(),
        })?;
    match ExecutablePath::new(&value) {
        Some(_) => Ok(value),
        None => Err(GrantRefusal::VariableNotAbsolute {
            rule: rule.to_string(),
            name: name.to_string(),
            value,
        }),
    }
}

/// Claude Code's `--allowedTools` entries for one run: never empty, and
/// built only by [`plan`] from resolved rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeGrant {
    entries: Vec<String>,
}

impl ClaudeGrant {
    #[must_use]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// A Codex run's per-run rules file: at least one `prefix_rule`, built only
/// by [`plan`] from resolved rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexGrant {
    rules: String,
}

impl CodexGrant {
    /// The rules file's whole text.
    #[must_use]
    pub fn rules(&self) -> &str {
        &self.rules
    }
}

/// How one harness receives a run's resolved allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GrantPlan {
    /// No run-level rule for this harness: the run is exactly as without.
    #[default]
    None,
    /// The harness withholds the whole grant, and this diagnostic says why.
    Withheld(Diagnostic),
    /// The harness applies grants per run, but none of these rules is one it
    /// can express; nothing reaches it, and this diagnostic names them.
    Unexpressible(Diagnostic),
    /// Claude Code: the run's `--allowedTools` entries.
    Claude {
        grant: ClaudeGrant,
        statement: Option<String>,
    },
    /// Codex: the per-run rules file, and the rules it could not express,
    /// when any.
    Codex {
        grant: CodexGrant,
        statement: Option<String>,
        unmapped: Option<Diagnostic>,
    },
}

impl GrantPlan {
    /// Claude Code's grant, for a Claude Code plan.
    #[must_use]
    pub fn claude(&self) -> Option<&ClaudeGrant> {
        match self {
            GrantPlan::Claude { grant, .. } => Some(grant),
            _ => None,
        }
    }

    /// The per-run Codex rules, for a Codex plan.
    #[must_use]
    pub fn codex(&self) -> Option<&CodexGrant> {
        match self {
            GrantPlan::Codex { grant, .. } => Some(grant),
            _ => None,
        }
    }

    /// The statement the turn's system instructions gain: each granted
    /// executable by its absolute path and how to type it. Only a plan that
    /// applies a `Bash` rule naming an executable has one.
    #[must_use]
    pub fn statement(&self) -> Option<&str> {
        match self {
            GrantPlan::Claude { statement, .. } | GrantPlan::Codex { statement, .. } => {
                statement.as_deref()
            }
            _ => None,
        }
    }

    /// What the run could not apply: the whole grant, or each rule its
    /// harness cannot express.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        match self {
            GrantPlan::Withheld(d)
            | GrantPlan::Unexpressible(d)
            | GrantPlan::Codex {
                unmapped: Some(d), ..
            } => vec![d.clone()],
            _ => Vec::new(),
        }
    }

    /// Whether any rule reaches the harness.
    #[must_use]
    pub fn applies(&self) -> bool {
        matches!(self, GrantPlan::Claude { .. } | GrantPlan::Codex { .. })
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

/// The one platform where the run-level allowlist is proven, as the docs and
/// every withheld diagnostic name it; `platform_withheld_reason`'s
/// `target_os = "linux"` gate is this, spelled for `cfg`.
pub const PROVEN_PLATFORM: &str = "Linux";

/// The platform this build targets, as a withheld diagnostic names it.
#[cfg(target_os = "macos")]
const PLATFORM: &str = "macOS";
#[cfg(windows)]
const PLATFORM: &str = "Windows";
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
const PLATFORM: &str = std::env::consts::OS;

/// Why every harness withholds the run-level allowlist on this platform, or
/// `None` where it is proven. Only Linux is: every test that drives a
/// harness's real CLI under a grant (`just live-run-allowlist`, the
/// live-control suite's run-overlay phase, `spike-command-grant`) has run on
/// Linux and on no other platform. A compile-time fact about the build, not a
/// probe of the host.
#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn platform_withheld_reason() -> Option<String> {
    Some(format!(
        "the per-run allowlist is withheld on {PLATFORM}, where no test against a harness's real \
         CLI proves it applies to this run alone and nothing wider; it is proven on \
         {PROVEN_PLATFORM} only"
    ))
}

/// Why every harness withholds the run-level allowlist on this platform:
/// never on Linux, where the tests against each granting harness's real CLI run.
#[cfg(target_os = "linux")]
#[must_use]
pub fn platform_withheld_reason() -> Option<String> {
    None
}

/// Plan how harness `id` (a base id) receives `resolved`, under the mode it
/// was resolved for.
///
/// `withhold` is a reason the `io` layer found that this run cannot carry the
/// grant on this harness (Codex credentials in the keyring, a missing home);
/// with one, or with a harness that has no per-run mechanism, the plan applies
/// nothing and carries one diagnostic naming the withheld rules.
#[must_use]
pub fn plan(id: &str, resolved: &ResolvedRules, withhold: Option<&str>) -> GrantPlan {
    let (mode, rules) = (resolved.mode, resolved.rules.as_slice());
    if rules.is_empty() {
        return GrantPlan::None;
    }
    if let Some(reason) = withhold.or_else(|| withheld_reason(id)) {
        return GrantPlan::Withheld(Diagnostic::new(
            id,
            Setting::AllowedTools,
            format!(
                "the run-level allowlist ({}) was withheld on `{id}`, which ran as it does with \
                 none: {reason}",
                quoted(rules.iter().map(ResolvedRule::written))
            ),
        ));
    }
    let read_only = mode == PermissionMode::ReadOnly;
    if id == CLAUDE {
        let entries = rules
            .iter()
            .map(|rule| match (rule.kind(), read_only) {
                // Under read-only a rule grants its executable with any
                // arguments, which Claude Code spells `:*`.
                (RuleKind::Executable { path, .. }, true) => format!("Bash({}:*)", path.as_str()),
                _ => rule.resolved(),
            })
            .collect();
        return GrantPlan::Claude {
            grant: ClaudeGrant { entries },
            statement: statement(rules.iter().filter_map(ResolvedRule::executable)),
        };
    }
    let mut text = String::from(RUN_RULES_HEADER);
    let mut unmapped: Vec<&str> = Vec::new();
    let mut granted: Vec<&ExecutablePath> = Vec::new();
    for rule in rules {
        let tokens = match (rule.kind(), read_only) {
            (RuleKind::Executable { path, .. }, true) => Ok(vec![path.as_str().to_string()]),
            _ => exec_policy_pattern(&rule.resolved()),
        };
        match tokens {
            Ok(tokens) => {
                text.push_str(&format!("\n# allowed_tools: {}\n", rule.resolved()));
                text.push_str(&exec_policy_statement_for(&tokens, RuleList::AllowedTools));
                text.push('\n');
                granted.extend(rule.executable());
            }
            Err(_) => unmapped.push(rule.written()),
        }
    }
    let unmapped_diagnostic = |rest: &str| {
        Diagnostic::new(
            id,
            Setting::AllowedTools,
            format!(
                "Codex cannot express {} as a per-run exec-policy rule, so {rest}",
                quoted(unmapped.iter().copied())
            ),
        )
    };
    if unmapped.len() == rules.len() {
        return GrantPlan::Unexpressible(unmapped_diagnostic(
            "nothing was applied and the run ran as it does with none",
        ));
    }
    GrantPlan::Codex {
        grant: CodexGrant { rules: text },
        statement: statement(granted.into_iter()),
        unmapped: (!unmapped.is_empty())
            .then(|| unmapped_diagnostic("those rules were not applied; the rest were")),
    }
}

/// The statement a turn's system instructions gain for `paths`, or `None`
/// when no executable is granted. The wording is oneharness's own; what a
/// consumer may rely on is that each path appears in it.
fn statement<'a>(paths: impl Iterator<Item = &'a ExecutablePath>) -> Option<String> {
    let mut list: Vec<String> = Vec::new();
    for path in paths {
        let quoted = format!("`{}`", path.as_str());
        if !list.contains(&quoted) {
            list.push(quoted);
        }
    }
    if list.is_empty() {
        return None;
    }
    Some(format!(
        "This run may execute these commands: {}. To run one, type its absolute path \
         exactly as written here, as one plain command on its own — not through a variable, \
         alias or function, and with no `;`, `&&`, `||`, pipe, redirection, command \
         substitution or second line — and pass any text it needs as one quoted argument \
         (single quotes, or double quotes when the text holds an apostrophe), which may span \
         several lines.",
        list.join(", ")
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

    /// `text` with each fixture path made absolute on the host's convention:
    /// unchanged on unix, drive-prefixed on Windows (`C:/opt/…`, still one
    /// plain word), so every assertion means the same thing on both.
    fn host(text: &str) -> String {
        if cfg!(windows) {
            text.replace("/opt/", "C:/opt/").replace("/usr/", "C:/usr/")
        } else {
            text.to_string()
        }
    }

    fn env(name: &str) -> Option<String> {
        match name {
            "TOOL" => Some(host("/opt/bin/decline")),
            "EMPTY" => Some(String::new()),
            "REL" => Some("bin/decline".into()),
            "SPACED" => Some(host("/opt/my tool")),
            _ => None,
        }
    }

    fn one(rule: &str, mode: PermissionMode) -> Result<ResolvedRule, GrantRefusal> {
        resolve(&[rule.to_string()], mode, &env).map(|v| v.rules()[0].clone())
    }

    fn path(rule: &ResolvedRule) -> Option<&str> {
        rule.executable().map(ExecutablePath::as_str)
    }

    #[test]
    fn a_variable_command_resolves_to_its_absolute_path_in_every_spelling() {
        for (rule, resolved) in [
            ("Bash($TOOL)", "Bash(/opt/bin/decline)"),
            ("Bash($TOOL:*)", "Bash(/opt/bin/decline:*)"),
        ] {
            for mode in [PermissionMode::ReadOnly, PermissionMode::Default] {
                let r = one(rule, mode).unwrap();
                assert_eq!(r.resolved(), host(resolved));
                assert_eq!(path(&r), Some(host("/opt/bin/decline").as_str()));
                assert_eq!(r.written(), rule);
            }
        }
        let prefix = one("Bash($TOOL status:*)", PermissionMode::Default).unwrap();
        assert_eq!(prefix.resolved(), host("Bash(/opt/bin/decline status:*)"));
        assert_eq!(path(&prefix), None);
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
        assert!(
            matches!(odd, GrantRefusal::InvalidVariable { .. }),
            "{odd:?}"
        );
        assert!(odd.reason().contains("is not a variable name"));
        assert_eq!(odd.rule(), "Bash(${TOOL}:*)");
    }

    #[test]
    fn read_only_accepts_only_one_absolute_executable() {
        for (rule, why) in [
            ("Edit", NotExecutable::NotBash),
            ("Write", NotExecutable::NotBash),
            ("Read", NotExecutable::NotBash),
            ("Bash", NotExecutable::WholeShell),
            ("Bash(*)", NotExecutable::Wildcard),
            ("Bash(/opt/bin/*)", NotExecutable::Wildcard),
            ("Bash(git status:*)", NotExecutable::MultiWord),
            (
                "Bash(/opt/bin/decline --ruling:*)",
                NotExecutable::MultiWord,
            ),
            ("Bash(decline:*)", NotExecutable::Relative),
            ("Bash(:*)", NotExecutable::Empty),
            ("Bash(/opt/bin/a;b)", NotExecutable::ShellSyntax),
        ] {
            let rule = host(rule);
            let rule = rule.as_str();
            let err = one(rule, PermissionMode::ReadOnly).unwrap_err();
            assert_eq!(err.rule(), rule);
            assert_eq!(
                err,
                GrantRefusal::NotAnExecutable {
                    rule: rule.into(),
                    mode: PermissionMode::ReadOnly,
                    why
                }
            );
            assert!(err.reason().contains("--mode read-only"), "{rule}: {err:?}");
            assert!(err.reason().contains(why.describe()));
        }
        let ok = one(&host("Bash(/opt/bin/decline)"), PermissionMode::ReadOnly).unwrap();
        assert_eq!(path(&ok), Some(host("/opt/bin/decline").as_str()));
    }

    #[test]
    fn other_modes_pass_any_rule_through_resolved() {
        for rule in ["Edit", "Bash(git status:*)", "Bash(decline:*)", "Bash"] {
            let r = one(rule, PermissionMode::Default).unwrap();
            assert_eq!(r.resolved(), rule);
            assert_eq!(path(&r), None);
        }
    }

    fn rules(list: &[&str], mode: PermissionMode) -> ResolvedRules {
        let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        resolve(&list, mode, &env).unwrap()
    }

    #[test]
    fn claude_gets_each_rule_on_allowed_tools_and_read_only_grants_any_arguments() {
        let ro = rules(&["Bash($TOOL)", "Bash($TOOL:*)"], PermissionMode::ReadOnly);
        let p = plan(CLAUDE, &ro, None);
        assert_eq!(
            p.claude().map(ClaudeGrant::entries).unwrap_or_default(),
            [
                host("Bash(/opt/bin/decline:*)"),
                host("Bash(/opt/bin/decline:*)")
            ]
        );
        let statement = p.statement().unwrap();
        assert_eq!(statement.matches(&host("`/opt/bin/decline`")).count(), 1);
        assert!(p.diagnostics().is_empty() && p.applies());
        let def = rules(&["Bash($TOOL)", "Edit"], PermissionMode::Default);
        let p = plan(CLAUDE, &def, None);
        assert_eq!(
            p.claude().map(ClaudeGrant::entries).unwrap_or_default(),
            [host("Bash(/opt/bin/decline)"), "Edit".to_string()]
        );
        assert_eq!(p.codex(), None);
    }

    #[test]
    fn codex_gets_one_allow_prefix_rule_per_translatable_rule() {
        let ro = rules(
            &["Bash($TOOL)", &host("Bash(/usr/bin/x:*)")],
            PermissionMode::ReadOnly,
        );
        let p = plan(CODEX, &ro, None);
        let text = p.codex().unwrap().rules();
        assert!(text.starts_with(RUN_RULES_HEADER));
        assert!(text.contains(&host(
            "prefix_rule(pattern=[\"/opt/bin/decline\"], decision=\"allow\")"
        )));
        assert!(text.contains(&host(
            "prefix_rule(pattern=[\"/usr/bin/x\"], decision=\"allow\")"
        )));
        assert!(!text.contains('$'));
        let s = p.statement().unwrap();
        assert!(s.contains(&host("`/opt/bin/decline`")) && s.contains(&host("`/usr/bin/x`")));
        assert!(p.claude().is_none());

        let def = rules(
            &["Bash($TOOL)", "Edit", "Bash(git status:*)"],
            PermissionMode::Default,
        );
        let p = plan(CODEX, &def, None);
        let text = p.codex().unwrap().rules();
        assert!(text.contains("pattern=[\"git\", \"status\"]"));
        assert!(!text.contains("decline"));
        let diagnostics = p.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("`Bash($TOOL)`, `Edit`"));
        assert!(diagnostics[0].message.contains("the rest were"));
        assert_eq!(p.statement(), None);
    }

    #[test]
    fn codex_with_no_expressible_rule_applies_nothing_and_names_them_all() {
        let def = rules(&["Edit", "Bash($TOOL)"], PermissionMode::Default);
        let p = plan(CODEX, &def, None);
        assert!(!p.applies());
        assert_eq!(p.codex(), None);
        assert_eq!(p.statement(), None);
        let message = &p.diagnostics()[0].message;
        assert!(message.contains("`Edit`, `Bash($TOOL)`"), "{message}");
        assert!(message.contains("nothing was applied"), "{message}");
    }

    #[test]
    fn a_harness_without_a_proven_mechanism_withholds_and_says_so() {
        let ro = rules(&["Bash($TOOL)"], PermissionMode::ReadOnly);
        for id in ["opencode", "copilot", "cursor", "qwen", "goose", "crush"] {
            let p = plan(id, &ro, None);
            assert!(!p.applies(), "{id}");
            assert_eq!(p.statement(), None);
            let diagnostics = p.diagnostics();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].setting, Setting::AllowedTools);
            assert!(diagnostics[0].message.contains("`Bash($TOOL)`"));
            assert!(diagnostics[0].message.contains(&format!("`{id}`")));
        }
        let p = plan(CODEX, &ro, Some("its login is in the keyring"));
        assert!(!p.applies());
        assert!(p.diagnostics()[0].message.contains("keyring"));
        let none = resolve(&[], PermissionMode::ReadOnly, &env).unwrap();
        assert_eq!(none.mode(), PermissionMode::ReadOnly);
        assert_eq!(plan(CLAUDE, &none, None), GrantPlan::None);
        assert!(GrantPlan::None.diagnostics().is_empty());
    }

    #[test]
    fn an_executable_path_is_one_absolute_plain_word() {
        assert!(ExecutablePath::new(&host("/opt/bin/decline")).is_some());
        for word in [
            "",
            "decline",
            "/opt/my tool",
            "/opt/a;b",
            "/opt/*",
            "/opt/a\nb",
        ] {
            assert!(ExecutablePath::new(&host(word)).is_none(), "{word:?}");
        }
    }

    /// The README's platform statements are this module's: the grant is
    /// proven on `PROVEN_PLATFORM` alone, which is exactly where
    /// `platform_withheld_reason` gives none, and the harnesses the
    /// approval-modes section names as enforcing it are those `withheld_reason`
    /// does not withhold.
    #[test]
    fn the_readme_states_the_platform_and_harnesses_the_grant_applies_on() {
        assert_eq!(
            platform_withheld_reason().is_none(),
            std::env::consts::OS == PROVEN_PLATFORM.to_lowercase()
        );
        let readme = include_str!("../../../../README.md").replace("\r\n", "\n");
        let flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
        let p = PROVEN_PLATFORM;
        for stated in [
            format!("**It applies on {p} only; on macOS, Windows and every other platform every harness withholds it.**"),
            format!("| Harness | run-level `allowed_tools` ({p}; every harness withholds it elsewhere) |"),
            format!("`--tools` gains `Bash` ({p}; withheld elsewhere) |"),
            format!("adds one `prefix_rule` per rule ({p}; withheld elsewhere) |"),
            format!("The table below is the {p} behaviour."),
        ] {
            assert!(flat.contains(&stated), "README no longer states: {stated}");
        }
        // The README's short names, as its translation matrix spells them.
        let readme_name = |id: &str| match id {
            CLAUDE => "Claude Code",
            CODEX => "Codex",
            other => panic!("{other} applies the grant but the README names no such harness"),
        };
        let enforcing: Vec<&str> = crate::domain::harness::all()
            .iter()
            .filter(|spec| withheld_reason(spec.id).is_none())
            .map(|spec| readme_name(spec.id))
            .collect();
        let named = format!("that enforce it ({}, on {p});", enforcing.join(" and "));
        assert!(flat.contains(&named), "README no longer states: {named}");
    }

    #[test]
    fn the_statement_follows_any_system_text() {
        assert_eq!(
            with_statement(Some("s"), Some("t")).as_deref(),
            Some("s\n\nt")
        );
        assert_eq!(with_statement(None, Some("t")).as_deref(), Some("t"));
        assert_eq!(with_statement(Some("s"), None).as_deref(), Some("s"));
        assert_eq!(with_statement(None, None), None);
    }
}
