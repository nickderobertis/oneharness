/* Generated from oneharness-core. Do not edit. */

/**
 * The unified approval mode, from least to most autonomy. A harness may not
 * support every value (see [`crate::domain::harness::HarnessSpec::mode`]); the
 * command layer refuses an unsupported one before spawning, never silently
 * downgrading it.
 */
export type PermissionMode = "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";

/**
 * Options accepted by `OneHarness.run()` in the published Node SDK.
 *
 * Unknown fields are rejected because the SDK cannot forward an option it does
 * not understand. This differs deliberately from output contracts, whose Zod
 * schemas preserve unknown fields for forward compatibility.
 */
export interface RunOptions {
  bins?: {
    [k: string]: string;
  } | undefined;
  cwd?: string | undefined;
  env?: {
    [k: string]: string;
  } | undefined;
  events?: boolean | undefined;
  fork?: boolean | undefined;
  harnesses?: readonly string[] | undefined;
  history?: boolean | undefined;
  historyDir?: string | undefined;
  historyName?: string | undefined;
  mode?: PermissionMode | undefined;
  models?: readonly string[] | undefined;
  /**
   * The user message sent to the selected harnesses.
   */
  prompt: string;
  reasoning?: string | undefined;
  resume?: string | undefined;
  session?: string | undefined;
  system?: string | undefined;
  timeoutSeconds?: number | undefined;
}
