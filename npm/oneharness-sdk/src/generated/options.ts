/* Generated from oneharness-core. Do not edit. */

/**
 * How a batch of same-prefix prompts is scheduled across the parallel runner.
 * Also accepted as a CLI value (`--batch-strategy`, parsed in the `oneharness`
 * binary) — the parsing lives there so this core crate stays free of `clap`.
 */
export type BatchStrategy = "speed" | "min-tokens";
/**
 * The unified approval mode, from least to most autonomy. A harness may not
 * support every value (see [`crate::domain::harness::HarnessSpec::mode`]); the
 * command layer refuses an unsupported one before spawning, never silently
 * downgrading it.
 */
export type PermissionMode = "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
/**
 * How a harness emits its result, which decides how `text` is extracted.
 *
 * Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
 * binary) and a config-file value (`output_format`, via `Deserialize`). The
 * CLI parsing lives in the binary so this core crate stays free of `clap`.
 */
export type OutputFormat = "text" | "json" | "stream-json";
/**
 * How the selected harnesses are run. Accepted as a CLI value (`--run-mode`,
 * parsed in the `oneharness` binary) and a config-file value (`run_mode`, via
 * `Deserialize`); the CLI parsing lives in the binary so this core crate stays
 * free of `clap`.
 */
export type RunMode = "parallel" | "fallback";

/**
 * Options accepted by `OneHarness.run()` in the published Node SDK.
 *
 * Unknown fields are rejected because the SDK cannot forward an option it does
 * not understand. This differs deliberately from output contracts, whose Zod
 * schemas preserve unknown fields for forward compatibility.
 */
export interface RunOptions {
  /**
   * Run against every supported harness.
   */
  all?: boolean | undefined;
  /**
   * Further prompts, making this a **batch**: one harness fanned over each
   * prompt, sharing the cacheable `system`/model prefix. Combined order is
   * `prompt`, then these, then `promptFiles` — the CLI's own order.
   */
  batchPrompts?: readonly string[] | undefined;
  batchStrategy?: BatchStrategy | undefined;
  bins?: {
    [k: string]: string;
  } | undefined;
  config?: string | undefined;
  /**
   * Open the out-of-band turn-control socket, so a separate `interrupt()`
   * can abort the in-flight turn without killing this run.
   */
  control?: boolean | undefined;
  cwd?: string | undefined;
  env?: {
    [k: string]: string;
  } | undefined;
  events?: boolean | undefined;
  /**
   * Harness id(s) to drop from an all-harness run.
   */
  exclude?: readonly string[] | undefined;
  fork?: boolean | undefined;
  harnesses?: readonly string[] | undefined;
  history?: boolean | undefined;
  historyDir?: string | undefined;
  historyLabels?: HistoryLabels | undefined;
  historyName?: string | undefined;
  /**
   * Maximum harnesses (or, in a batch, prompts) to run concurrently.
   */
  maxParallel?: number | undefined;
  /**
   * Replace these selected harnesses' provider processes with oneharness's
   * deterministic `MOCK_*`-scripted responder.
   */
  mockHarnesses?: readonly string[] | undefined;
  /**
   * Mock/spy the selected harnesses' tool calls for this run only.
   */
  mockRules?: string | undefined;
  mode?: PermissionMode | undefined;
  models?: readonly string[] | undefined;
  noConfig?: boolean | undefined;
  /**
   * Do NOT record history for this run, overriding config or the
   * `ONEHARNESS_HISTORY` environment override.
   */
  noHistory?: boolean | undefined;
  /**
   * Write each harness's raw stdout/stderr under this directory.
   */
  outputDir?: string | undefined;
  outputFormat?: OutputFormat | undefined;
  /**
   * Extra arguments appended verbatim to each harness command, after `--`.
   */
  passthrough?: readonly string[] | undefined;
  /**
   * Silence the warning that the chosen mode may block on an approval prompt.
   */
  permitPrompts?: boolean | undefined;
  /**
   * Build and report each command without executing it.
   */
  printCommand?: boolean | undefined;
  /**
   * The user message sent to the selected harnesses.
   */
  prompt: string;
  /**
   * Files each holding one whole prompt, or `-` for stdin.
   */
  promptFiles?: readonly string[] | undefined;
  reasoning?: string | undefined;
  /**
   * Treat a not-installed harness as a failure.
   */
  requireAvailable?: boolean | undefined;
  resume?: string | undefined;
  runMode?: RunMode | undefined;
  /**
   * Constrain each harness's final answer to this JSON Schema file.
   */
  schema?: string | undefined;
  /**
   * Max retries when a response fails schema validation.
   */
  schemaMaxRetries?: number | undefined;
  session?: string | undefined;
  /**
   * Directory the `session` store lives in.
   */
  sessionDir?: string | undefined;
  /**
   * Append one JSONL record per observed tool call to this file.
   */
  spyFile?: string | undefined;
  system?: string | undefined;
  /**
   * Read the system prompt from a file — the counterpart to `system` for a
   * value too large to pass on the argv.
   */
  systemFile?: string | undefined;
  timeoutSeconds?: number | undefined;
}
export interface HistoryLabels {
  [k: string]: string;
}
