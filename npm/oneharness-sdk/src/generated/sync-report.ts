/* Generated from oneharness-core. Do not edit. */

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
  results: SyncResult[];
  schema_version: string;
  [k: string]: unknown;
}
/**
 * What one harness's sync did (or would do).
 */
export interface SyncResult {
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
  status: SyncStatus;
  /**
   * Top-level settings that have no mapping for this harness (e.g. a
   * top-level `allowed_tools` while the harness has no allow-list concept)
   * — visible here and warned on stderr, never silently dropped.
   */
  unmapped: string[];
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
