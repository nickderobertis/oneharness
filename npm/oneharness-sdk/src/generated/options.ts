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
  };
  cwd?: string;
  env?: {
    [k: string]: string;
  };
  events?: boolean;
  fork?: boolean;
  harnesses?: readonly string[];
  history?: boolean;
  historyDir?: string;
  historyName?: string;
  mode?: PermissionMode;
  models?: readonly string[];
  /**
   * The user message sent to the selected harnesses.
   */
  prompt: string;
  reasoning?: string;
  resume?: string;
  session?: string;
  system?: string;
  timeoutSeconds?: number;
}
