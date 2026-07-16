/* Generated from oneharness-core. Do not edit. */

/**
 * The session selector accepted by `OneHarness.history()` in the published Node
 * SDK.
 *
 * This type carries the *structural* contract only. `history()` also enforces a
 * semantic rule this schema deliberately does not express — a lookup must
 * select a session with either `session` or `last`. JSON Schema states that as
 * `anyOf` over required-key subschemas, which would accept `{"last": false}`
 * and reject the equivalent-but-explicit `{"session": "x", "last": false}`
 * differently than the SDK does: the SDK reads `last` for truthiness and falls
 * back to a non-empty `session`. Encoding a rule the SDK does not actually
 * apply would be worse than not encoding it, so the selector rule stays a
 * documented check in the SDK, run immediately after this structural
 * validation and before any field is read.
 */
export interface HistoryLookup {
  allProjects?: boolean | undefined;
  historyDir?: string | undefined;
  /**
   * Select the most recent session instead of naming one.
   */
  last?: boolean | undefined;
  project?: string | undefined;
  /**
   * The oneharness-derived session name recorded by `run --history`.
   */
  session?: string | undefined;
}
