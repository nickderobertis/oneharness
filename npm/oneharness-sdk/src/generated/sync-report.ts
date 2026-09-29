/* Generated from oneharness-core. Do not edit. */

/**
 * Which unified rule list an entry belongs to.
 */
export type RuleList = "allowed_tools" | "denied_tools";
/**
 * What applying a fragment did (or, under `check`, would do) to one file.
 *
 * Serialized as the report token itself, so the wire value and the variant a
 * consumer matches on cannot drift apart.
 */
export type FileStatus = "created" | "updated" | "unchanged";
/**
 * What one harness's permission/settings sync did (or would do).
 *
 * [`FileStatus`] plus the one outcome a *file* never has: a harness with no
 * permission/settings fragment to apply at all. Keeping them one closed set —
 * rather than the report's earlier free string — is what makes an unreachable
 * status unconstructible and lets the contract publish the four values.
 */
export type SyncStatus = "created" | "updated" | "unchanged" | "skipped";

/**
 * The `oneharness sync` output contract.
 */
export interface SyncReport {
  /**
   * True under `--check`: statuses describe what *would* happen.
   */
  check: boolean;
  /**
   * The oneharness config files the synced settings came from.
   */
  config_files: string[];
  /**
   * True under `--exact`: each permission list was held to exactly the
   * source's, and each result names the entries that added and removed.
   * Omitted when false.
   */
  exact?: boolean | undefined;
  results: SyncResult[];
  schema_version: string;
  [k: string]: unknown;
}
/**
 * What one harness's sync did (or would do).
 */
export interface SyncResult {
  /**
   * Under `--exact`: each entry the sync added (or would add) to this
   * harness's permission lists. Omitted when empty.
   */
  added_rules?: RuleChange[] | undefined;
  /**
   * The permission/settings config file written (or that would be written);
   * `null` when nothing of that kind is configured for this harness.
   */
  file: string | null;
  harness: string;
  /**
   * Normalized `[[hooks]]` files installed into this harness (a Goose hook
   * writes two). Empty when no `[[hooks]]` entry targets it.
   */
  hooks: HookFileResult[];
  /**
   * Under `--exact`: each entry the sync removed (or would remove) from
   * this harness's permission lists — stale and hand-added alike. Omitted
   * when empty.
   */
  removed_rules?: RuleChange[] | undefined;
  status: SyncStatus;
  /**
   * Top-level settings that have no mapping for this harness (e.g. a
   * top-level `allowed_tools` while the harness has no allow-list concept)
   * — visible here and warned on stderr, never silently dropped.
   */
  unmapped: string[];
  /**
   * Individual configured rules this harness cannot express (a Codex
   * execpolicy pattern has no wildcards and no exact-length form), each left
   * out of the file with the reason — never widened, and never silently
   * dropped. Omitted when empty.
   */
  unmapped_rules?: UnmappedRule[] | undefined;
  [k: string]: unknown;
}
/**
 * One entry an `--exact` sync added to, or removed from, a harness's rules.
 */
export interface RuleChange {
  /**
   * The list the entry belongs to; absent for a statement in a Codex rules
   * file that no unified list renders (a hand-written `decision="prompt"`).
   */
  list?: RuleList | null | undefined;
  /**
   * The entry as it appears in the harness's file: the rule string for a
   * JSON list, the whole `prefix_rule(...)` statement for a Codex rules file.
   */
  rule: string;
  [k: string]: unknown;
}
/**
 * One installed `[[hooks]]` file.
 */
export interface HookFileResult {
  file: string;
  status: FileStatus;
  [k: string]: unknown;
}
/**
 * A configured rule the harness cannot express, left out of what was written.
 */
export interface UnmappedRule {
  list: RuleList;
  /**
   * Why the harness cannot express it.
   */
  reason: string;
  /**
   * The rule exactly as configured.
   */
  rule: string;
  [k: string]: unknown;
}
