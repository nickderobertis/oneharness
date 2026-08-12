/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `mock()`.
 */
export interface MockOptions {
  /**
   * The harness's pre-tool hook event, written to the responder's stdin.
   */
  event: string;
  harness: string;
  rules?: string | undefined;
  spyFile?: string | undefined;
}
