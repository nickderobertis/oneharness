/* Generated from oneharness-core. Do not edit. */

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
  /**
   * `created` / `updated` / `unchanged` for `file`, or `skipped` when no
   * permission/settings fragment applies. Hook files carry their own status.
   */
  status: string;
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
  status: string;
  [k: string]: unknown;
}
