/* Generated from oneharness-core. Do not edit. */

/**
 * One control request the run handled, recorded in the report so a consumer
 * can tell an interrupted turn from one that simply ended.
 *
 * A sum type discriminated by `outcome`, so a record can never say "served,
 * and here is the refusal reason": the reason exists exactly when there was a
 * refusal. Serialized flat — `{"outcome":"served","verb":…,"at":…}` /
 * `{"outcome":"refused","verb":…,"at":…,"reason":…}`.
 */
export type ControlEvent =
  | {
      /**
       * When the request was handled.
       */
      at: string;
      outcome: "served";
      /**
       * Whether a redirection was committed with the abort. Omitted when it
       * was a plain stop, so an older consumer reads the same record it
       * always did.
       */
      redirected?: boolean | undefined;
      /**
       * The verb requested.
       */
      verb: "interrupt";
      [k: string]: unknown;
    }
  | {
      at: UtcInstant;
      outcome: "refused";
      reason: ControlReason;
      verb: ControlVerb;
      [k: string]: unknown;
    };
/**
 * An RFC 3339 timestamp in UTC (`Z`), in oneharness's canonical spelling.
 */
export type UtcInstant = string;
/**
 * Why a control request could not be served. Distinct reasons because a
 * supervisor reacts differently to each: `unsupported` is permanent for the
 * harness, `not_running` means the dispatch is gone, `no_active_turn` means
 * the run is alive but between turns.
 */
export type ControlReason = "unsupported" | "no_active_turn" | "not_running";
/**
 * The control verb a supervisor sends. Only `interrupt` exists today; the
 * frame's `v` is what leaves room for `steer` later.
 */
export type ControlVerb = "interrupt";
/**
 * Why a candidate fell through — the closed set [`startup_failure_reason`]
 * decides and [`crate::domain::report::FallThrough`] reports.
 *
 * A type rather than a token, because the set is closed and every reader
 * downstream branches on it: a value no classifier produced cannot be built,
 * and the JSON spelling below is the schema's rather than each call site's. A
 * new variant is a report `schema_version` bump like any other enum value, since
 * a consumer matching exhaustively learns of it only from the version.
 */
export type FallThroughReason =
  | "not-installed"
  | "spawn-error"
  | "auth"
  | "quota"
  | "session-not-found"
  | "untrusted-directory"
  | "input-too-large"
  | "model-not-found"
  | "rate-limit";
export type ToolCallStatus = "completed" | "failed" | "timeout" | "interrupted";
/**
 * How a normalized tool interval was obtained.
 */
export type TimingSource = "provider_measured" | "stdout_observed";
/**
 * The normalized, closed set of failure reasons oneharness can classify from a
 * harness's output. It is the single source for the `failure_kind` contract
 * value: serialized as the snake_case token a consumer reads in the report
 * (`auth`, `rate_limit`, `model_not_found`, `quota`, `session_not_found`,
 * `tool_deferred`), so the wire shape is unchanged — modeling it as an enum
 * keeps a misspelled or invalid kind unrepresentable and gives every
 * producer/consumer (classifier, `is_failure`, the fallback fall-through rule,
 * the report, history) one definition to share instead of scattered string
 * literals.
 */
export type FailureKind =
  | "auth"
  | "rate_limit"
  | "model_not_found"
  | "quota"
  | "session_not_found"
  | "tool_deferred"
  | "untrusted_directory"
  | "input_too_large";
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
export type Status = "ok" | "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped" | "planned";
/**
 * Measured execution telemetry for one harness run: when the invocation ran,
 * and — when the harness's transcript supports it — how its wall clock split
 * between provider latency and tool work.
 *
 * Carried from the runner/parser to the history writer *and*, since report
 * schema `0.5`, serialized on [`RunResult::telemetry`]. Exposing it there is
 * what lets a consumer read the numbers off the run it just made instead of
 * re-opening the history file the same run wrote.
 *
 * Internally tagged by `source`, so the variant is a value a consumer switches
 * on rather than a shape it has to sniff. Every variant states only what was
 * actually measured — there is no variant meaning "no telemetry"; that is a
 * `null` field.
 */
export type ExecutionTelemetry =
  | {
      finished_at: string | null;
      model_ms: number | null;
      source: "provider_measured";
      started_at: string;
      time_to_first_token_ms: number | null;
      tool_ms: number | null;
      [k: string]: unknown;
    }
  | {
      source: "partial_invocation";
      started_at: string;
      [k: string]: unknown;
    }
  | {
      source: "stdout_observed";
      tool_ms: number;
      [k: string]: unknown;
    };

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
  /**
   * Out-of-band turn control for this run (`--control`), or `null` when none
   * was requested — which is every ordinary run. It records the socket a
   * separate `oneharness interrupt` process could address, the harness
   * mechanism behind it, and each request served, so a consumer can tell an
   * interrupted turn from one that simply ended.
   */
  control: ControlReport | null;
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
 * The run report's `control` block: where the socket lived, which mechanism
 * backed it, and every request served over the run's lifetime.
 */
export interface ControlReport {
  /**
   * Every control request served, in order.
   */
  interrupts: ControlEvent[];
  /**
   * The harness mechanism backing it.
   */
  mechanism: "claude-control-request" | "codex-app-server" | "opencode-http" | "acp-cancel" | "crush-http";
  /**
   * Absolute path of the socket this run listened on.
   */
  socket: string;
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
   * `quota`, `session-not-found`, `untrusted-directory`, `input-too-large`,
   * and — on a model fan-out — `model-not-found` / `rate-limit`; see
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
   * This candidate's own account of why it could not run: its
   * [`RunResult::error`], copied — one value with one source, not a second
   * rendering of it. `null` when the candidate said nothing beyond its status.
   *
   * So when the provider named the cause in machine-readable terms, what
   * arrives here is the sentence naming the harness with that object **quoted
   * inside it** (`… refused the request before running it:
   * {"input_error_code":"input_too_large",…}`) — the object verbatim, not the
   * object alone. A reader extracts it; nothing paraphrases it on the way.
   *
   * The `reason` above is oneharness's classification and stays a short,
   * closed token; this is the cause underneath it, carried up so a supervisor
   * reading only the fallback block never has to re-parse a fallen-through
   * candidate's raw stdout to learn what the provider already said. Added in
   * report schema `0.8`.
   */
  detail: string | null;
  /**
   * Canonical harness id.
   */
  harness: string;
  reason: FallThroughReason;
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
   * `model_not_found`, `quota`, `session_not_found`), and `tool_deferred` — a run that exited
   * *cleanly* but only deferred a builtin tool call instead of executing it
   * (Claude Code bridge/managed deployments), so it did no useful work. The
   * deferred case is the only `failure_kind` that can appear on a `status: ok`
   * run, and it also marks the run as failed for exit-code purposes.
   * Serialized as its snake_case token (see [`FailureKind`]).
   */
  failure_kind: FailureKind | null;
  /**
   * Where `failure_kind` was read (`stderr`/`stdout`, or `config:env_from`
   * for a candidate refused before spawning); `null` when absent.
   */
  failure_kind_source: string | null;
  /**
   * Canonical harness id (e.g. `claude-code`).
   */
  harness: string;
  /**
   * Base id or `<base>:<variant>`, suitable for selecting the same candidate.
   */
  harness_id: string;
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
   * Measured execution telemetry for this run: the invocation bounds and, when
   * the harness's transcript carried a trace to read them from, the
   * model/tool split. `null` when nothing was measured — never estimated.
   * Added in report schema `0.5`; before that a consumer had to re-read the
   * history file for numbers the run itself already had.
   */
  telemetry: ExecutionTelemetry | null;
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
  /**
   * Named preset, when this result came from a composed harness id.
   */
  variant: string | null;
  [k: string]: unknown;
}
/**
 * One normalized action a harness took, harness-agnostic so a single consumer
 * assertion works across harnesses. Every field is always serialized (null when
 * absent) so the shape is stable, mirroring the `usage` contract.
 */
export interface ActionEvent {
  /**
   * Monotonic elapsed tool time. `None` means no terminal boundary was seen.
   */
  duration_ms: number | null;
  finished_at: string | null;
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
  /**
   * UTC interval bounds for tool execution, populated on history records.
   */
  started_at: string | null;
  /**
   * Terminal tool state, populated on history tool-call events.
   */
  status: ToolCallStatus | null;
  /**
   * Provenance for the tool interval. Omitted when timing is unavailable.
   */
  timing_source?: TimingSource | null | undefined;
  /**
   * Stable call identity within the session. Present on tool calls and their
   * matching results when the provider exposes an identity; history fills a
   * deterministic run-local identity for providers that do not.
   */
  tool_call_id: string | null;
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
