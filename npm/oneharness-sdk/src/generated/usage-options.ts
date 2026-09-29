/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `usage()`.
 */
export interface UsageOptions {
  all?: boolean | undefined;
  bins?: {
    [k: string]: string;
  } | undefined;
  /**
   * Load configuration from exactly these files, in order, skipping
   * user/project discovery; each later file overrides the earlier ones.
   */
  config?: string[] | undefined;
  cwd?: string | undefined;
  exclude?: string[] | undefined;
  harnesses?: string[] | undefined;
  noConfig?: boolean | undefined;
  timeoutSeconds?: number | undefined;
}
