/* Generated from oneharness-core. Do not edit. */

/**
 * One harness run, normalized and frozen for the history log. Serialized as one
 * JSONL line per harness run, appended as the run finalizes. Carries only the
 * normalized cross-harness signals — no raw stdout/stderr.
 */
export type HistoryRecord = (
  | {
      error?: null | undefined;
      [k: string]: unknown;
    }
  | ({
      error: string;
      schema_version?: "1.3" | "1.4" | undefined;
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
        status?: "ok" | "nonzero" | "timeout" | "spawn-error" | "skipped" | "planned" | undefined;
        [k: string]: unknown;
      }
    | {
        schema_version?: "1.4" | undefined;
        [k: string]: unknown;
      }
  ) &
  (
    | {
        duration_ms: number;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events:
          | (
              | (ActionEvent & {
                  duration_ms: number;
                  finished_at: string;
                  kind: "tool_call";
                  started_at: string;
                  status: "completed";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  duration_ms: number;
                  finished_at: string;
                  kind: "tool_call";
                  started_at: string;
                  status: "failed";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind: "tool_call";
                  started_at: string;
                  status: "timeout";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind: "tool_call";
                  started_at: string;
                  status: "interrupted";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind?: "tool_result" | undefined;
                  [k: string]: unknown;
                })
            )[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: string | null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms: number;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.2" | "1.3" | "1.4";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at: string;
        status: Status;
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: number | null | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms: number;
        usage: Usage;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events:
          | ((
              | (ActionEvent & {
                  duration_ms: number;
                  finished_at: string;
                  kind: "tool_call";
                  started_at: string;
                  status: "completed";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  duration_ms: number;
                  finished_at: string;
                  kind: "tool_call";
                  started_at: string;
                  status: "failed";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind: "tool_call";
                  started_at: string;
                  status: "timeout";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind: "tool_call";
                  started_at: string;
                  status: "interrupted";
                  tool_call_id: string;
                  [k: string]: unknown;
                })
              | (ActionEvent & {
                  kind?: "tool_result" | undefined;
                  [k: string]: unknown;
                })
            ) & {
              timing_source?: never | undefined;
              [k: string]: unknown;
            })[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: string | null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id?: string | undefined;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels1 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms: number;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.0" | "1.1";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at: string;
        status: Status;
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: number | null | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms: number;
        usage: Usage1;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number | null;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events:
          | {
              duration_ms?: null | undefined;
              finished_at?: null | undefined;
              index: number;
              input: unknown;
              kind: string;
              name: string | null;
              output: string | null;
              started_at?: null | undefined;
              status?: null | undefined;
              tool_call_id?: string | null | undefined;
              [k: string]: unknown;
            }[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels2 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.2" | "1.3" | "1.4";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at?: never | undefined;
        status: "ok" | "planned";
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: never | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage2;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events: ActionEvent[] | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels3 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms: number;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.2" | "1.3" | "1.4";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at?: never | undefined;
        status: Status;
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: never | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage3;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number | null;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events: ActionEvent[] | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels4 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.2" | "1.3" | "1.4";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at?: never | undefined;
        status: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped";
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: never | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage4;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events: ActionEvent[] | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels5 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.3" | "1.4";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at: string;
        status: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped";
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: number | null | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage5;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number | null;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events:
          | (ActionEvent & {
              timing_source?: never | undefined;
              [k: string]: unknown;
            })[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id?: string | undefined;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels6 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.0" | "1.1";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at?: never | undefined;
        status: "nonzero" | "timeout" | "cancelled" | "spawn-error" | "skipped";
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: never | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage6;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number | null;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        /**
         * Best-effort normalized tool-call events; `null` when the harness exposes
         * no machine-readable trace.
         */
        events:
          | ({
              duration_ms?: null | undefined;
              finished_at?: null | undefined;
              index: number;
              input: unknown;
              kind: string;
              name: string | null;
              output: string | null;
              started_at?: null | undefined;
              status?: null | undefined;
              tool_call_id?: string | null | undefined;
              [k: string]: unknown;
            } & {
              timing_source?: never | undefined;
              [k: string]: unknown;
            })[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at: null;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id?: string | undefined;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels7 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: never | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        observed_tool_ms?: never | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "1.0" | "1.1";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        started_at?: never | undefined;
        status: "ok" | "planned";
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: never | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: never | undefined;
        usage: Usage7;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
    | {
        duration_ms: number | null;
        /**
         * Best-effort normalized failure text for a run that did not succeed: the
         * harness's own diagnostic as oneharness captured it on stderr, or
         * oneharness's own message when it generated one (a spawn failure, a
         * timeout, a binary that is not installed). This is the *only* place a
         * record quotes the process's own bytes, and it is deliberately narrow —
         * trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
         * that failed. `failure_kind` says what class of failure it was; this says
         * what the harness actually reported, which is what an operator reads when
         * the class is unclassified. Never derived from stdout, so it can never
         * stand in for provider output the run did not produce. Omitted on the wire
         * when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
         */
        error?: string | null | undefined;
        events:
          | {
              index: number;
              input: unknown;
              kind: string;
              name: string | null;
              output: string | null;
              [k: string]: unknown;
            }[]
          | null;
        exit_code: number | null;
        /**
         * Best-effort classified failure reason (see [`FailureKind`]); `null` when
         * unclassified.
         */
        failure_kind: FailureKind | null;
        finished_at?: string | null | undefined;
        /**
         * Canonical harness id (e.g. `claude-code`).
         */
        harness: string;
        harness_id: string;
        /**
         * Globally unique, time-ordered record id. This is also the cursor accepted
         * by `history watch --after` and the exact id accepted by history lookup.
         */
        history_id: string;
        labels?: HistoryLabels8 | undefined;
        /**
         * The effective top-level model for the run, if any.
         */
        model: string | null;
        model_ms?: number | null | undefined;
        /**
         * The human-meaningful session name (see [`session_name`]); repeated on
         * every record so a reader can resolve a session by name from any line.
         */
        name: string;
        /**
         * Union of tool intervals observed at the stdout pipe. Unlike `tool_ms`,
         * this is not provider-measured and has no model-latency counterpart.
         */
        observed_tool_ms?: number | null | undefined;
        /**
         * The normalized approval mode requested for the run.
         */
        permission_mode: "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
        /**
         * The project directory the run operated in (the real path, not the
         * on-disk slug), so the list view can show where a session ran.
         */
        project: string;
        /**
         * The prompt this harness run received (its own, on a batch run; else the
         * run's single prompt).
         */
        prompt: string;
        schema_version: "0.1" | "0.2";
        /**
         * The oneharness session id this run belongs to (the history file's stem).
         */
        session: string;
        /**
         * The harness's own continuation id, when it exposed one; `null` otherwise.
         */
        session_id: string | null;
        /**
         * UTC invocation bounds and monotonic time attribution. The provider/tool
         * split is conservative when a transcript has tool calls but lacks native
         * boundaries: the observed invocation interval is attributed to the union
         * of those calls, never double-counted.
         */
        started_at?: string | null | undefined;
        status: Status;
        /**
         * Best-effort final assistant text; `null` when extraction was impossible.
         */
        text: string | null;
        /**
         * How `text` was extracted; `null` when absent.
         */
        text_source: string | null;
        time_to_first_token_ms?: number | null | undefined;
        /**
         * RFC3339 UTC instant the record was written (append time).
         */
        timestamp: string;
        tool_ms?: number | null | undefined;
        usage: Usage8;
        variant?: string | null | undefined;
        [k: string]: unknown;
      }
  );
export type ToolCallStatus = "completed" | "failed" | "timeout" | "interrupted";
/**
 * How a normalized tool interval was obtained.
 */
export type TimingSource = "provider_measured" | "stdout_observed";
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
/**
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels1 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage1 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels2 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage2 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels3 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage3 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels4 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage4 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels5 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage5 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels6 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage6 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels7 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage7 {
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
 * Caller-supplied metadata used to select related task-graph records.
 * Omitted on the wire when empty for additive compatibility.
 */
export interface HistoryLabels8 {
  [k: string]: string;
}
/**
 * Best-effort token/cost accounting (every field `null` when unreported).
 */
export interface Usage8 {
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
