/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `gate()` — the pre-tool gate an
 * installed hook invokes, driven directly so a consumer hosting its own hook
 * runner never has to shell out and parse.
 */
export interface GateOptions {
  denyIfContains?: string | undefined;
  /**
   * The harness's pre-tool hook event, written to the gate's stdin.
   */
  event: string;
  /**
   * The harness whose hook protocol to speak.
   */
  harness: string;
  reason?: string | undefined;
}
