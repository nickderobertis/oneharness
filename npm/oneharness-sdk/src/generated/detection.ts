/* Generated from oneharness. Do not edit. */

export interface DetectReport {
  detected: DetectInfo[];
  schema_version: string;
}
export interface DetectInfo {
  available: boolean;
  bin: string;
  id: string;
  path?: string | null;
  version?: string | null;
}
