/* Generated from oneharness-core. Do not edit. */

/**
 * The session selector accepted by `OneHarness.history()` in the published Node
 * SDK.
 *
 * A lookup must select a session, so the contract is a union of the only two
 * ways to do that: ask for the most recent one ([`HistoryLookupByLast`]), or
 * name one ([`HistoryLookupBySession`]). A lookup that selects nothing — `{}`,
 * `{"session": ""}`, or `{"last": false}` — matches neither variant and fails
 * at the boundary, so `history()` never has to re-check for one.
 *
 * Variant order is load-bearing, because the variants overlap and an untagged
 * enum takes the first match. `Last` comes first so that `last: true` keeps the
 * priority the SDK has always given it: `{"session": "x", "last": true}`
 * selects the most recent session, not `x`. Dropping `last` to `false` is what
 * asks for the named session instead, so `{"session": "x", "last": false}`
 * selects `x` — the same reading as the previous `if (last) … else if (session)`
 * rule, now stated in the type rather than re-derived after parsing.
 */
export type HistoryLookup = HistoryLookupByLast | HistoryLookupBySession;

/**
 * Select the most recent session.
 */
export interface HistoryLookupByLast {
  allProjects?: boolean | undefined;
  historyDir?: string | undefined;
  /**
   * Select the most recent session. Only `true` selects, so this variant
   * accepts no other value.
   */
  last: true;
  project?: string | undefined;
  /**
   * A name may accompany `last: true` — it is what the caller would have
   * looked up otherwise — but `last` takes priority, so it does not select.
   */
  session?: string | undefined;
}
/**
 * Select the session named by `session`.
 */
export interface HistoryLookupBySession {
  allProjects?: boolean | undefined;
  historyDir?: string | undefined;
  /**
   * An explicit "not the most recent session". `last: true` takes priority
   * over a name, so it selects [`HistoryLookup::Last`] instead and cannot
   * appear here — which is why this is the literal `false`, not a `bool`.
   */
  last?: false | undefined;
  project?: string | undefined;
  /**
   * The oneharness-derived session name recorded by `run --history`.
   */
  session: string;
}
