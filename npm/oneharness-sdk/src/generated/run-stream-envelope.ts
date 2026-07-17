/* Generated from oneharness-core. Do not edit. */

/**
 * One line of `oneharness run --stream` output.
 *
 * Event lines carry normalized actions as they arrive. Exactly one terminal
 * result line carries the complete report unless the consumer closes the
 * stream early. This is an output contract, so deserialization deliberately
 * tolerates additive fields from newer producers.
 */
export type RunStreamEnvelope =
  | {
      event: ActionEvent;
      type: "event";
      [k: string]: unknown;
    }
  | {
      report: RunReport;
      type: "result";
      [k: string]: unknown;
    };
/**
 * The normalized, closed set of failure reasons oneharness can classify from a
 * harness's output. It is the single source for the `failure_kind` contract
 * value: serialized as the snake_case token a consumer reads in the report
 * (`auth`, `rate_limit`, `model_not_found`, `quota`, `tool_deferred`), so the
 * wire shape is unchanged — modeling it as an enum keeps a misspelled or
 * invalid kind unrepresentable and gives every producer/consumer (classifier,
 * `is_failure`, the fallback fall-through rule, the report, history) one
 * definition to share instead of scattered string literals.
 */
export type FailureKind = "auth" | "rate_limit" | "model_not_found" | "quota" | "tool_deferred";
/**
 * How a harness emits its result, which decides how `text` is extracted.
 *
 * Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
 * binary) and a config-file value (`output_format`, via `Deserialize`). The
 * CLI parsing lives in the binary so this core crate stays free of `clap`.
 */
export type OutputFormat = "text" | "json" | "stream-json";
/**
 * The outcome of attempting to run one harness.
 */
export type Status = "ok" | "nonzero" | "timeout" | "spawn-error" | "skipped" | "planned";

/**
 * One normalized action a harness took, harness-agnostic so a single consumer
 * assertion works across harnesses. Every field is always serialized (null when
 * absent) so the shape is stable, mirroring the `usage` contract.
 */
export interface ActionEvent {
  /**
   * Position of this event within the run, so "≤ N tool calls" and "did X
   * before Y" are expressible from a stable ordering (also array order).
   */
  index: number;
  /**
   * Structured, tool-shaped arguments (the command string, the file path),
   * so a consumer asserts on specific args without re-parsing; `null` when the
   * event carries none (e.g. a `tool_result`).
   */
  input: unknown;
  /**
   * The kind of event: `tool_call` (the model invoked a tool) or
   * `tool_result` (the observation returned to the model). Left open for
   * future kinds rather than an enum, so a new shape never breaks the field.
   */
  kind: string;
  /**
   * Normalized tool name where knowable (e.g. `bash`, `Edit`); `null` for a
   * `tool_result`, or when the harness did not name the tool.
   */
  name: string | null;
  /**
   * The result/observation text, when the trace exposes it; `null` otherwise.
   */
  output: string | null;
  [k: string]: unknown;
}
/**
 * The top-level `run` report written to stdout.
 */
export interface RunReport {
  /**
   * Same-prefix batch metadata when this run fanned **one** harness over more
   * than one prompt; `null` on an ordinary run. Its presence is the signal a
   * consumer keys on to read each result's own `prompt`.
   */
  batch: BatchReport | null;
  /**
   * Back-compat convenience: `true` exactly when `permission_mode` is
   * `bypass`. Retained so existing consumers keep working; new consumers
   * should read `permission_mode`.
   */
  bypass_permissions: boolean;
  /**
   * Config files that shaped this run, in layering order (user first,
   * project last); empty under `--no-config` or when none exist.
   */
  config_files: string[];
  dry_run: boolean;
  /**
   * Fallback-mode metadata when this run drove the selected harnesses in
   * priority order, stopping at the first that ran (`--run-mode fallback`);
   * `null` on a parallel run (and under `--print-command`, where nothing
   * executes). Its presence tells a consumer that `results` holds only the
   * harnesses actually *attempted* — the fallen-through ones in order, then
   * the one that ran — not every selected harness.
   */
  fallback: FallbackReport | null;
  /**
   * Whether the resumed session was forked (`--fork`) rather than appended to.
   * `false` unless `--resume` was given with `--fork`.
   */
  fork: boolean;
  /**
   * The history session file this run streamed normalized records to
   * (absolute); `null` when history was not enabled (or under `--print-command`,
   * where nothing runs). The programmatic handle a consumer captures to read the
   * session back later with `oneharness history show`.
   */
  history_file: string | null;
  /**
   * The parsed `--mock-rules` ruleset this run was intercepted with; `null`
   * when no mocking was requested. Present so a consumer can tell a mocked
   * run's report from a clean one without out-of-band state.
   */
  mock_rules: unknown;
  /**
   * The effective top-level model: the first of the fan-out `models` list when
   * one was given, else the single configured/CLI model, else `null`. Each
   * result's own `model` is authoritative on a fan-out run.
   */
  model: string | null;
  /**
   * The model fan-out list this run multiplied over (repeated `--model` /
   * config `models`), or `null` on an ordinary single-model run. Its presence
   * is the signal a consumer keys on to read each result's own `model`: in
   * `parallel` mode `results` holds one entry per (harness, model) pair; in
   * `fallback` mode the pairs were tried in priority order (harness-major,
   * model-minor).
   */
  models: string[] | null;
  oneharness_version: string;
  /**
   * The normalized approval mode requested for this run (see the README
   * support matrix). Each harness maps it to its own mechanism.
   */
  permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
  /**
   * The prompt sent. On an ordinary run this is *the* prompt every result
   * shares; on a **batch** run (see `batch`) it repeats the first prompt for
   * back-compat, and each result's own `prompt` field is authoritative.
   */
  prompt: string;
  results: RunResult[];
  /**
   * The session id being continued, when `--resume` was passed; else `null`.
   */
  resume: string | null;
  /**
   * The JSON Schema applied to this run (structured output), or `null` when
   * none was requested. Echoed so a consumer sees the exact constraint each
   * result was validated against.
   */
  schema: unknown;
  /**
   * Maximum retries allowed per harness under the validate/retry loop; `null`
   * when no schema was requested.
   */
  schema_max_retries: number | null;
  schema_version: string;
  /**
   * The uniform session handle in play (`--session <name>`), or `null` when
   * none was requested. Lets a consumer thread one stable name across turns
   * instead of extracting each harness's native session id. Distinct from the
   * low-level `resume` field above, which echoes an explicit `--resume` id.
   */
  session: SessionReport | null;
  /**
   * The spy-log path the mock hook appended tool-call records to (absolute);
   * `null` when none was requested.
   */
  spy_file: string | null;
  [k: string]: unknown;
}
/**
 * Metadata for a same-prefix batch run (one harness, N prompts sharing a
 * cacheable prefix). Present on [`RunReport::batch`] only in that mode.
 */
export interface BatchReport {
  /**
   * Whether the fan-out actually **forked** the warm-up's session to reuse its
   * cached prefix (`min-tokens` on a fork-capable harness whose warm-up exposed
   * a session id). `false` for `speed`, for `min-tokens` on a harness that
   * cannot fork, or when the warm-up exposed no session to fork. When `true`,
   * the fan-out results' `command` carries the resume/fork flags and their
   * `usage.cache_read_tokens` reflect the reused prefix.
   */
  forked: boolean;
  /**
   * How many prompts were run (equals `results.len()`).
   */
  prompt_count: number;
  /**
   * How the prompts were scheduled across the parallel runner.
   */
  strategy: "speed" | "min-tokens";
  [k: string]: unknown;
}
/**
 * Metadata for a fallback run (harnesses tried in priority order until one
 * runs). Present on [`RunReport::fallback`] only in that mode. The per-harness
 * detail lives in `results`; this block summarizes the outcome so a consumer
 * need not re-derive it from statuses.
 */
export interface FallbackReport {
  /**
   * The candidates fallen through because they could not run the task at all,
   * in priority order, each with why (`not-installed`, `spawn-error`, `auth`,
   * `quota`, and — on a model fan-out — `model-not-found` / `rate-limit`; see
   * [`crate::domain::fallback::startup_failure_reason`]).
   */
  fell_through: FallThrough[];
  /**
   * The harness that actually ran the task (the run stopped there), or `null`
   * when no candidate could run at all — every one was a startup failure.
   */
  ran: string | null;
  [k: string]: unknown;
}
/**
 * One candidate a fallback run fell through, with the reason it could not run.
 */
export interface FallThrough {
  /**
   * Canonical harness id.
   */
  harness: string;
  /**
   * Short reason token (`not-installed` / `spawn-error` / `auth` / `quota` /
   * `model-not-found` / `rate-limit`).
   */
  reason: string;
  [k: string]: unknown;
}
/**
 * One harness's entry in the report.
 */
export interface RunResult {
  /**
   * Whether that binary was found.
   */
  available: boolean;
  /**
   * The binary name or path oneharness resolved and would invoke.
   */
  bin: string;
  /**
   * The exact argv oneharness built (argv[0] is the binary).
   */
  command: string[];
  /**
   * Wall-clock duration of the run; `null` when not executed.
   */
  duration_ms: number | null;
  /**
   * Human-readable problem + suggested action; `null` on success.
   */
  error: string | null;
  /**
   * Best-effort normalized tool-call / action events the harness took (shell
   * commands, file edits, tool uses), in order — so consumers can assert on
   * *behavior*, not just the final `text`. `null` when the harness's output
   * exposes no machine-readable trace (a plain-text harness, or Claude Code's
   * single-document `json` result), distinct from `[]` — an empty array is not
   * currently emitted; absence is signalled by `null` + a null `events_source`.
   * Never fabricated. See [`crate::domain::events`].
   */
  events: ActionEvent[] | null;
  /**
   * How `events` was recovered (e.g. `json:opencode-parts`,
   * `stream-json:content-blocks`), parallel to `text_source`; `null` when no
   * events were found. Lets a consumer tell "harness doesn't support it" from
   * "no tools were used."
   */
  events_source: string | null;
  /**
   * Process exit code; `null` when not run, timed out, or signalled.
   */
  exit_code: number | null;
  /**
   * Best-effort failure reason; `null` when unclassified. Distinct from
   * `status`, which records oneharness's relationship to the process. Two
   * families: coarse reasons for a non-zero run (`auth`, `rate_limit`,
   * `model_not_found`, `quota`), and `tool_deferred` — a run that exited
   * *cleanly* but only deferred a builtin tool call instead of executing it
   * (Claude Code bridge/managed deployments), so it did no useful work. The
   * deferred case is the only `failure_kind` that can appear on a `status: ok`
   * run, and it also marks the run as failed for exit-code purposes.
   * Serialized as its snake_case token (see [`FailureKind`]).
   */
  failure_kind: FailureKind | null;
  /**
   * Where `failure_kind` was read (`stderr`/`stdout`); `null` when absent.
   */
  failure_kind_source: string | null;
  /**
   * Canonical harness id (e.g. `claude-code`).
   */
  harness: string;
  /**
   * The model this result ran with (the value oneharness put on the harness's
   * model flag), or `null` when no model was requested and the harness used its
   * own default. On a **model fan-out** run (`RunReport::models`), this is what
   * distinguishes results that share a harness — each entry is one (harness,
   * model) pair. The model is also visible in `command`; this field surfaces it
   * without parsing the argv.
   */
  model: string | null;
  output_format: OutputFormat;
  /**
   * The prompt this result ran, set only on a **batch** run (one harness
   * fanned over N prompts), where each result has its own prompt. `null` on an
   * ordinary run, where the single top-level `prompt` applies to every result.
   */
  prompt: string | null;
  /**
   * Structured-output run only: how many times this harness was invoked under
   * the validate/retry loop (1 + retries). `null` when no schema was
   * requested or the harness did not run.
   */
  schema_attempts: number | null;
  /**
   * Structured-output run only: the validation errors from the final attempt,
   * joined for display; `null` when valid or no schema was requested.
   */
  schema_error: string | null;
  /**
   * Structured-output run only: whether `structured` conformed to the schema
   * on the final attempt. `null` when no schema was requested (or the harness
   * did not run); `false` when a schema was requested but the result never
   * conformed (including "no JSON found").
   */
  schema_valid: boolean | null;
  /**
   * Best-effort harness session id for continuation; `null` when none is
   * exposed. Surfaced for a consumer to thread into `--resume`, and consumed
   * by oneharness itself when `--session` is in play (it is captured into the
   * session store to back the uniform handle — see [`RunReport::session`]).
   */
  session_id: string | null;
  status: Status;
  /**
   * Raw captured stderr (empty for skipped/planned).
   */
  stderr: string;
  /**
   * Raw captured stdout (empty for skipped/planned).
   */
  stdout: string;
  /**
   * Structured-output run only: the JSON value extracted from the final
   * answer and validated against the requested schema. `null` when no schema
   * was requested, or when no JSON value could be extracted. Carries the
   * last-attempted value even when it failed validation, so a consumer can
   * see what the harness produced.
   */
  structured: unknown;
  /**
   * Best-effort final assistant text; `null` when extraction is impossible.
   */
  text: string | null;
  /**
   * How `text` was extracted (e.g. `json:result`, `raw`); `null` when absent.
   */
  text_source: string | null;
  usage: Usage;
  /**
   * How `usage` was read (e.g. `json`); `null` when nothing was found.
   */
  usage_source: string | null;
  [k: string]: unknown;
}
/**
 * Best-effort token/cost accounting; every field is `null` when the harness
 * does not report it. Always present so consumers can read a stable shape.
 */
export interface Usage {
  /**
   * Prompt tokens served from the provider's prompt cache (a cheap read of a
   * previously-written prefix), when the harness reports them. `None` when the
   * harness does not surface cache counts — never `0` as a guess.
   */
  cache_read_tokens: number | null;
  /**
   * Prompt tokens written to the provider's prompt cache (a.k.a. cache
   * creation), when the harness reports them. `None` when not surfaced.
   */
  cache_write_tokens: number | null;
  /**
   * Total cost in USD, when the harness reports it (often absent on
   * subscription auth, where there is no per-call dollar figure).
   */
  cost_usd: number | null;
  /**
   * Prompt/input tokens billed, when the harness reports them.
   */
  input_tokens: number | null;
  /**
   * Completion/output tokens billed, when the harness reports them.
   */
  output_tokens: number | null;
  [k: string]: unknown;
}
/**
 * The uniform session handle for a run (`--session`). Present on
 * [`RunReport::session`] only when `--session <name>` was requested.
 */
export interface SessionReport {
  /**
   * The caller's stable handle (`--session <name>`, sanitized for the store).
   */
  name: string;
  /**
   * Whether this run created the named session (no prior token) or continued
   * an existing one.
   */
  phase: "create" | "continue";
  /**
   * The session store file backing the handle (absolute); the programmatic
   * handle to the persisted state.
   */
  store_file: string | null;
  /**
   * The harness native token now bound to the name: the id resumed on a
   * continue, or the id captured on a create. `null` only when a create run
   * exposed no session id (the handle then cannot be continued — a warning is
   * emitted), or under `--print-command` on a create (nothing ran).
   */
  token: string | null;
  [k: string]: unknown;
}
