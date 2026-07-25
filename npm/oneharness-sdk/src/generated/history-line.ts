/* Generated from oneharness-core. Do not edit. */

/**
 * One event-sourced history JSONL line.
 */
export type HistoryLine =
  | {
      event: ActionEvent;
      harness: string;
      harness_id?: string | null | undefined;
      run_id: string;
      schema_version: "1.0" | "1.1";
      type: "event";
      variant?: string | null | undefined;
      [k: string]: unknown;
    }
  | (
      | {
          duration_ms: number;
          exit_code: number | null;
          failure_kind: FailureKind | null;
          finished_at: string;
          harness: string;
          harness_id?: string | null | undefined;
          history_id: string;
          labels?: HistoryLabels | undefined;
          model: string | null;
          model_ms: number;
          name: string;
          permission_mode: PermissionMode;
          project: string;
          prompt: string;
          schema_version: "1.0" | "1.1";
          session: string;
          session_id: string | null;
          started_at: string;
          status: "ok" | "nonzero";
          text: string | null;
          text_source: string | null;
          time_to_first_token_ms?: number | null | undefined;
          timestamp: string;
          tool_ms: number;
          type: "run";
          usage: Usage;
          variant?: string | null | undefined;
          [k: string]: unknown;
        }
      | {
          duration_ms: number;
          exit_code: number | null;
          failure_kind: FailureKind | null;
          finished_at: string | null;
          harness: string;
          harness_id?: string | null | undefined;
          history_id: string;
          labels?: HistoryLabels | undefined;
          model: string | null;
          model_ms: number;
          name: string;
          permission_mode: PermissionMode;
          project: string;
          prompt: string;
          schema_version: "1.0" | "1.1";
          session: string;
          session_id: string | null;
          started_at: string;
          status: "timeout" | "spawn-error" | "skipped" | "planned";
          text: string | null;
          text_source: string | null;
          time_to_first_token_ms?: number | null | undefined;
          timestamp: string;
          tool_ms: number;
          type: "run";
          usage: Usage;
          variant?: string | null | undefined;
          [k: string]: unknown;
        }
      | {
          duration_ms: number | null;
          exit_code: number | null;
          failure_kind: FailureKind | null;
          finished_at: null;
          harness: string;
          harness_id?: string | null | undefined;
          history_id: string;
          labels?: HistoryLabels | undefined;
          model: string | null;
          model_ms?: never | undefined;
          name: string;
          permission_mode: PermissionMode;
          project: string;
          prompt: string;
          schema_version: "1.0" | "1.1";
          session: string;
          session_id: string | null;
          started_at?: never | undefined;
          status: Status;
          text: string | null;
          text_source: string | null;
          time_to_first_token_ms?: never | undefined;
          timestamp: string;
          tool_ms?: never | undefined;
          type: "run";
          usage: Usage;
          variant?: string | null | undefined;
          [k: string]: unknown;
        }
    );
export type ToolCallStatus = "completed" | "failed" | "timeout" | "interrupted";
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
 * The unified approval mode, from least to most autonomy. A harness may not
 * support every value (see [`crate::domain::harness::HarnessSpec::mode`]); the
 * command layer refuses an unsupported one before spawning, never silently
 * downgrading it.
 */
export type PermissionMode = "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
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
   * Stable call identity within the session. Present on tool calls and their
   * matching results when the provider exposes an identity; history fills a
   * deterministic run-local identity for providers that do not.
   */
  tool_call_id: string | null;
  [k: string]: unknown;
}
export interface HistoryLabels {
  [k: string]: string;
}
/**
 * Normalized token/cost accounting. Every field is best-effort and independently
 * nullable: a harness may report tokens but not dollar cost (cost is commonly
 * absent on subscription auth), or report nothing at all (plain-text harnesses).
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
