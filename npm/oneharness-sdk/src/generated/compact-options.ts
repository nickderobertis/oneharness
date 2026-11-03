/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `compact()`: compact a named
 * session now (`oneharness compact`).
 */
export interface CompactOptions {
  bins?: {
    [k: string]: string;
  } | undefined;
  config?: string[] | undefined;
  cwd?: string | undefined;
  env?: {
    [k: string]: string;
  } | undefined;
  /**
   * The harness identity the session is expected to be bound to.
   */
  harness?: string | undefined;
  model?: string | undefined;
  noConfig?: boolean | undefined;
  /**
   * The caller-owned session name (`run`'s `session`).
   */
  session: string;
  sessionDir?: string | undefined;
  timeoutSeconds?: number | undefined;
}
