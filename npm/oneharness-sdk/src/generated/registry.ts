/* Generated from oneharness. Do not edit. */

/**
 * How a harness emits its result, which decides how `text` is extracted.
 *
 * Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
 * binary) and a config-file value (`output_format`, via `Deserialize`). The
 * CLI parsing lives in the binary so this core crate stays free of `clap`.
 */
export type OutputFormat = "text" | "json" | "stream-json";

export interface ListReport {
  harnesses: HarnessInfo[];
  schema_version: string;
}
export interface HarnessInfo {
  default_bin: string;
  display: string;
  /**
   * The argv oneharness would build, with placeholders, so the adapter's
   * shape is visible without running anything.
   */
  example_command: string[];
  /**
   * Whether a forked run reuses the parent session's prompt-cache prefix, so a
   * fork-based `--batch-strategy min-tokens` run actually reduces tokens. `true`
   * only for Claude Code today; `false` (incl. OpenCode, whose fork re-sends the
   * prefix cold) means `min-tokens` only orders the calls (no saving).
   */
  fork_reuses_cache: boolean;
  id: string;
  install_hint: string;
  /**
   * The input-rewrite verdict shape `oneharness mock <id>` speaks for this
   * harness (`claude-nested`, `crush-flat`, `opencode-shim`); `null` when
   * the harness has no verified rewrite — a rewrite rule for it is then a
   * loud usage error (see the README mock support matrix).
   */
  mock_rewrite?: string | null;
  /**
   * The approval modes (`--mode`) this harness can express, each with its
   * headless behavior. Modes not listed are unsupported for the harness.
   */
  modes: ModeInfo[];
  output_format: OutputFormat;
  /**
   * Whether `run --session <name>` is supported — a uniform, caller-owned
   * handle oneharness maps to the harness's native session id. `true` only for
   * harnesses that expose a session id headlessly; `false` means `--session` is
   * a loud usage error (there is no id to bind a name to).
   */
  session_capable: boolean;
  /**
   * Whether the unified allow/deny rule lists and hooks table can be synced
   * into that file (see the README support matrix).
   */
  supports_allowed_tools: boolean;
  supports_denied_tools: boolean;
  /**
   * Whether `run --resume <session> --fork` is supported — branching a new
   * session from the resumed one. `false` means it resumes linearly only.
   */
  supports_fork: boolean;
  supports_hooks: boolean;
  /**
   * Whether `oneharness mock <id>` can express a pre-tool *deny* for this
   * harness (the same protocol `oneharness gate` speaks).
   */
  supports_mock_deny: boolean;
  /**
   * Whether `run --schema` is delivered through a native structured-output
   * flag for this harness (Claude Code's `--json-schema`). `false` means the
   * portable prompt-based path is used — structured output works either way;
   * oneharness always validates and retries.
   */
  supports_native_schema: boolean;
  /**
   * Whether a large user prompt can be delivered off the argv (piped to the
   * harness's stdin) so it never trips the OS argument ceiling (`E2BIG`).
   */
  supports_prompt_stdin: boolean;
  /**
   * Whether `run --reasoning <effort>` can be delivered on the argv for this
   * harness (Claude Code's `--effort`, Codex's `model_reasoning_effort`).
   * `false` means it has no headless reasoning flag — a reasoning request is
   * then a loud usage error (effort is provider/model config there).
   */
  supports_reasoning: boolean;
  /**
   * Whether `run --resume <session>` is supported for this harness.
   */
  supports_resume: boolean;
  /**
   * Whether a large system prompt can be delivered off the argv via a file
   * flag (Claude Code's `--append-system-prompt-file`). `false` does not mean a
   * large system is unhandled — for a harness whose system rides the prompt it
   * travels on stdin with the prompt; see the README large-prompt matrix.
   */
  supports_system_file: boolean;
  /**
   * The project-scoped config file `oneharness sync` writes for this
   * harness; `null` when it has none (sync settings are then rejected).
   */
  sync_file?: string | null;
}
/**
 * One supported approval mode for a harness, with its headless behavior, in
 * `oneharness list`. A [`PermissionMode`] absent from a harness's array is
 * unsupported for it (a `--mode` request would be refused).
 */
export interface ModeInfo {
  /**
   * `"clean"` (never blocks headless) or `"hangs"` (would block on an
   * approval prompt; refused without --permit-prompts).
   */
  headless: string;
  mode: string;
}
