/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `detect()`.
 */
export interface DetectOptions {
  all?: boolean | undefined;
  bins?: {
    [k: string]: string;
  } | undefined;
  config?: string | undefined;
  exclude?: string[] | undefined;
  harnesses?: string[] | undefined;
  noConfig?: boolean | undefined;
  /**
   * Exit non-zero if any probed harness is not installed. The SDKs surface
   * that as a thrown process error rather than a report a caller must
   * re-check.
   */
  requireAvailable?: boolean | undefined;
}
