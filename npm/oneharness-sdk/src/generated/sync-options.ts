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
  global?: boolean | undefined;
  harnesses?: string[] | undefined;
  noConfig?: boolean | undefined;
}
