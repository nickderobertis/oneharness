/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `config()`.
 */
export interface ConfigOptions {
  /**
   * Load configuration from exactly these files, in order, skipping
   * user/project discovery; each later file overrides the earlier ones.
   */
  config?: string[] | undefined;
  cwd?: string | undefined;
  noConfig?: boolean | undefined;
}
