/* Generated from oneharness-core. Do not edit. */

/**
 * How a compaction ended.
 */
export type CompactStatus = "completed" | "cancelled" | "unsupported" | "failed";

/**
 * The `oneharness compact` report.
 */
export interface CompactOutcome {
  /**
   * The context size after compaction, where the harness reports it
   * (claude-code's `compact_boundary.post_tokens`); `null` otherwise.
   */
  context_tokens_after: number | null;
  /**
   * The context size before compaction, where the harness reports it
   * (claude-code's `compact_boundary.pre_tokens`); `null` otherwise.
   */
  context_tokens_before: number | null;
  /**
   * Why the compaction is `unsupported` or `failed`; absent otherwise.
   */
  error?: string | null | undefined;
  /**
   * The harness identity the session is bound to.
   */
  harness: string;
  schema_version: string;
  /**
   * The session name compacted.
   */
  session: string;
  status: CompactStatus;
  /**
   * The native session token the name continues after this call: the
   * compacted fork's on `completed`, the untouched original's otherwise.
   * `null` when the request was refused before a session was resolved.
   */
  token?: string | null | undefined;
  usage: Usage;
  [k: string]: unknown;
}
/**
 * The compaction's own usage, every field `null` where the harness
 * reports none.
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
