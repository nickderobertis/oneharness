/* Generated from oneharness. Do not edit. */

/**
 * The `oneharness detect` output contract.
 */
export interface DetectReport {
  detected: DetectInfo[];
  schema_version: string;
  [k: string]: unknown;
}
/**
 * One probed harness identity.
 */
export interface DetectInfo {
  available: boolean;
  bin: string;
  id: string;
  path: string | null;
  version: string | null;
  [k: string]: unknown;
}
