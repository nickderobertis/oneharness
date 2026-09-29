/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `detect()`.
 */
export interface DetectOptions {
  all?: boolean | undefined;
  bins?: {
    [k: string]: string;
  } | undefined;
  /**
   * Load configuration from exactly these files, in order, skipping
   * user/project discovery; each later file overrides the earlier ones.
   */
  config?: string[] | undefined;
  exclude?: string[] | undefined;
  harnesses?: string[] | undefined;
  /**
   * Ignore every configuration file and `ONEHARNESS_*` override. `true`
   * beside a non-empty `config` is refused before anything runs.
   */
  noConfig?: boolean | undefined;
  /**
   * Exit non-zero if any probed harness is not installed. The SDKs surface
   * that as a thrown process error rather than a report a caller must
   * re-check.
   */
  requireAvailable?: boolean | undefined;
}
