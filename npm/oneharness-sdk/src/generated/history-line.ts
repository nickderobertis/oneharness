/* Generated from oneharness-core. Do not edit. */

/**
 * One event-sourced history JSONL line.
 */
export type HistoryLine =
  | (
      | {
          event: ActionEvent;
          harness: string;
          harness_id?: string | null | undefined;
          run_id: string;
          schema_version: "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
          type: "event";
          variant?: string | null | undefined;
          [k: string]: unknown;
        }
      | {
          event: ActionEvent & {
            timing_source?: never | undefined;
            [k: string]: unknown;
          };
          harness: string;
          harness_id?: string | null | undefined;
          run_id: string;
          schema_version: "1.0" | "1.1";
          type: "event";
          variant?: string | null | undefined;
          [k: string]: unknown;
        }
    )
  | ((
      | {
          error?: null | undefined;
          [k: string]: unknown;
        }
      | ({
          error: string;
          schema_version?: "1.3" | "1.4" | "1.5" | "1.6" | "1.7" | undefined;
          [k: string]: unknown;
        } & (
          | {
              status?: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped" | undefined;
              [k: string]: unknown;
            }
          | {
              failure_kind: "tool_deferred";
              [k: string]: unknown;
            }
        ))
    ) &
      (
        | {
            work?: null | undefined;
            [k: string]: unknown;
          }
        | {
            schema_version?: "1.7" | undefined;
            status?: "nonzero" | "timeout" | "cancelled" | undefined;
            work: "done" | "none";
            [k: string]: unknown;
          }
      ) &
      (
        | {
            status?: "ok" | "nonzero" | "timeout" | "spawn-error" | "skipped" | "planned" | undefined;
            [k: string]: unknown;
          }
        | {
            schema_version?: "1.4" | "1.5" | "1.6" | "1.7" | undefined;
            [k: string]: unknown;
          }
      ) &
      ((
        | {
            failure_kind?:
              | "auth"
              | "rate_limit"
              | "model_not_found"
              | "quota"
              | "tool_deferred"
              | "untrusted_directory"
              | "input_too_large"
              | null;
            [k: string]: unknown;
          }
        | {
            schema_version?: "1.5" | "1.6" | "1.7" | undefined;
            [k: string]: unknown;
          }
      ) &
        (
          | {
              failure_kind?:
                "auth" | "rate_limit" | "model_not_found" | "quota" | "session_not_found" | "tool_deferred" | null;
              [k: string]: unknown;
            }
          | {
              schema_version?: "1.6" | "1.7" | undefined;
              [k: string]: unknown;
            }
        )) &
      (
        | {
            duration_ms: number;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms?: never | undefined;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
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
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
        | {
            duration_ms: number;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms?: never | undefined;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
            session: string;
            session_id: string | null;
            started_at: string;
            status: "timeout" | "cancelled" | "spawn-error" | "skipped" | "planned";
            text: string | null;
            text_source: string | null;
            time_to_first_token_ms?: number | null | undefined;
            timestamp: string;
            tool_ms: number;
            type: "run";
            usage: Usage;
            variant?: string | null | undefined;
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
        | {
            duration_ms: number;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms: number;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
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
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
        | {
            duration_ms: number | null;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms?: never | undefined;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
            session: string;
            session_id: string | null;
            started_at?: never | undefined;
            status: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped";
            text: string | null;
            text_source: string | null;
            time_to_first_token_ms?: never | undefined;
            timestamp: string;
            tool_ms?: never | undefined;
            type: "run";
            usage: Usage;
            variant?: string | null | undefined;
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
        | {
            duration_ms: number;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms?: never | undefined;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
            session: string;
            session_id: string | null;
            started_at: string;
            status: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped";
            text: string | null;
            text_source: string | null;
            time_to_first_token_ms?: number | null | undefined;
            timestamp: string;
            tool_ms?: never | undefined;
            type: "run";
            usage: Usage;
            variant?: string | null | undefined;
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
        | {
            duration_ms: number | null;
            /**
             * Normalized failure text for a run that did not succeed (see
             * [`HistoryRecord::error`]). Omitted on the wire when absent.
             */
            error?: string | null | undefined;
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
            observed_tool_ms?: never | undefined;
            permission_mode: PermissionMode;
            project: string;
            prompt: string;
            schema_version: "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6" | "1.7";
            session: string;
            session_id: string | null;
            started_at?: never | undefined;
            status: "ok" | "planned";
            text: string | null;
            text_source: string | null;
            time_to_first_token_ms?: never | undefined;
            timestamp: string;
            tool_ms?: never | undefined;
            type: "run";
            usage: Usage;
            variant?: string | null | undefined;
            /**
             * Work evidence for a failure nothing classified (see
             * [`HistoryRecord::work`]). Omitted on the wire when absent.
             */
            work?: RunWork | null | undefined;
            [k: string]: unknown;
          }
      ));
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
 * The unified approval mode, from least to most autonomy. A harness may not
 * support every value (see [`crate::domain::harness::HarnessSpec::mode`]); the
 * command layer refuses an unsupported one before spawning, never silently
 * downgrading it.
 */
export type PermissionMode = "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
/**
 * Whether a candidate's normalized result carries **evidence it did the task's
 * work** — the first thing [`startup_failure_reason`] consults.
 *
 * Two independent witnesses, either of which is decisive:
 *
 * - **Tool events.** A recorded tool call is the harness acting on the task.
 * - **Usage accounting.** [`Usage::reports_billed_work`][billed] — the same definition
 *   `signals::record_reports_work` classifies a raw harness record with, so the
 *   two readings of "billed" are one contract with one implementation.
 *
 * It is also a **published reading**, not only an internal one: a run that
 * failed with nothing to classify carries it as [`RunResult::work`] and in its
 * history record, because there the question "did this candidate do anything?"
 * is the only one left. One type for both, so the value a reader sees is the
 * same value the fall-through verdict consulted.
 *
 * [billed]: crate::domain::signals::Usage::reports_billed_work
 */
export type RunWork = "done" | "none";
/**
 * The outcome of attempting to run one harness.
 */
export type Status = "ok" | "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped" | "planned";

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
