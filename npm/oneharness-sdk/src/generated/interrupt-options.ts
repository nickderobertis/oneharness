/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `interrupt()`.
 */
export interface InterruptOptions {
  cwd?: string | undefined;
  input?: string | undefined;
  /**
   * The caller-owned session handle the target run was started with.
   */
  session: string;
  sessionDir?: string | undefined;
}
