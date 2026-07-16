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
 * The variants overlap on purpose, and the order is load-bearing: an untagged
 * enum takes the first match, so `Last` coming first is what gives `last: true`
 * priority over a name. `{"session": "x", "last": true}` satisfies *both*
 * variants and resolves to `Last`, selecting the most recent session rather
 * than `x`; `{"session": "x", "last": false}` fails `Last` and resolves to
 * `Session`. That is exactly the reading of the `if (last) … else if (session)`
 * rule this replaces, now decided by the union rather than re-derived after
 * parsing. Zod resolves its generated union in the same order, so the Node SDK
 * agrees with Rust by construction.
 *
 * Because `Last` ignores the name it carries, that name is a plain `String`:
 * `{"session": "", "last": true}` stays valid and still selects the most recent
 * session, as it always has. Only a name that actually selects has to be
 * non-empty.
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
   * looked up otherwise — but `last` takes priority, so it never selects.
   * It is therefore unconstrained: an empty name is meaningless here rather
   * than invalid, so `{"session": "", "last": true}` stays accepted.
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
   * Whether the most recent session was asked for instead. An ordinary
   * `bool`, so a caller holding a `boolean` can pass it straight through.
   * `true` here also satisfies [`HistoryLookup::Last`], which the union tries
   * first — so a lookup that reaches this variant always meant the name.
   */
  last?: boolean | undefined;
  project?: string | undefined;
  /**
   * The oneharness-derived session name recorded by `run --history`. This is
   * the name that selects, so it must be non-empty.
   */
  session: string;
}
