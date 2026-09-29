/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `sync()`.
 */
export interface SyncOptions {
  /**
   * Report what would change and write nothing.
   */
  check?: boolean | undefined;
  config?: string | undefined;
  cwd?: string | undefined;
  /**
   * Hold each harness's allow/deny list to exactly the configured one:
   * stale and hand-added entries are removed, every other key is merged as
   * usual, and the report names each added and removed entry.
   */
  exact?: boolean | undefined;
  global?: boolean | undefined;
  harnesses?: string[] | undefined;
  noConfig?: boolean | undefined;
}
