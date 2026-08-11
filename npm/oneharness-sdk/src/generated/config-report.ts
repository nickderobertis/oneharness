/* Generated from oneharness-core. Do not edit. */

/**
 * The unified approval mode, from least to most autonomy. A harness may not
 * support every value (see [`crate::domain::harness::HarnessSpec::mode`]); the
 * command layer refuses an unsupported one before spawning, never silently
 * downgrading it.
 */
export type PermissionMode = "read-only" | "plan" | "default" | "edit" | "auto" | "bypass";
/**
 * How a harness emits its result, which decides how `text` is extracted.
 *
 * Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
 * binary) and a config-file value (`output_format`, via `Deserialize`). The
 * CLI parsing lives in the binary so this core crate stays free of `clap`.
 */
export type OutputFormat = "text" | "json" | "stream-json";
/**
 * How the selected harnesses are run. Accepted as a CLI value (`--run-mode`,
 * parsed in the `oneharness` binary) and a config-file value (`run_mode`, via
 * `Deserialize`); the CLI parsing lives in the binary so this core crate stays
 * free of `clap`.
 */
export type RunMode = "parallel" | "fallback";

/**
 * The `oneharness config` report: the fully layered configuration with the
 * provenance of every value, so a consumer can see exactly which file (or
 * default) shaped each setting of a run.
 */
export interface ConfigReport {
  all: Field;
  allowed_tools: Field2;
  bypass: Field;
  /**
   * The files consulted, in layering order (user first, project last).
   */
  config_files: string[];
  denied_tools: Field2;
  /**
   * Per-key provenance for the top-level `[env]`.
   */
  env: {
    [k: string]: Field3;
  };
  exclude: Field2;
  /**
   * Per-harness overrides, with per-field provenance.
   */
  harness: {
    [k: string]: HarnessReport;
  };
  harnesses: Field2;
  history: Field;
  history_dir: Field3;
  /**
   * Per-key provenance for history labels.
   */
  history_labels: {
    [k: string]: Field3;
  };
  hooks: Field10;
  max_parallel: Field8;
  mode: Field4;
  model: Field3;
  models: Field21;
  output_format: Field6;
  reasoning: Field31;
  require_available: Field;
  run_mode: Field9;
  schema_file: Field3;
  schema_max_retries: Field7;
  schema_version: string;
  stream: Field1;
  system: Field3;
  timeout: Field5;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field {
  source: string | null;
  value: boolean | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field2 {
  source: string | null;
  value: string[] | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field3 {
  source: string | null;
  value: string | null;
  [k: string]: unknown;
}
/**
 * One harness's `[harness.<id>]` overrides, with provenance.
 */
export interface HarnessReport {
  allowed_tools: Field2;
  args: Field2;
  bin: Field3;
  denied_tools: Field2;
  env: {
    [k: string]: Field3;
  };
  hooks: {
    [k: string]: unknown;
  };
  model: Field3;
  reasoning: Field3;
  settings: {
    [k: string]: unknown;
  };
  variant: {
    [k: string]: VariantReport;
  };
  [k: string]: unknown;
}
export interface VariantReport {
  allowed_tools: Field2;
  args: Field2;
  bin: Field3;
  denied_tools: Field2;
  env: {
    [k: string]: Field3;
  };
  env_file: Field3;
  env_from: {
    [k: string]: Field3;
  };
  hooks: {
    [k: string]: unknown;
  };
  model: Field3;
  reasoning: Field3;
  settings: {
    [k: string]: unknown;
  };
  unset_env: Field2;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field10 {
  source: string | null;
  value: HookEntry[] | null;
  [k: string]: unknown;
}
/**
 * One `[[hooks]]` entry: a pre-tool gate installed into each targeted harness.
 * The `command` may contain `{harness}`, replaced with the harness id when the
 * hook is built (so one entry can route `mygate hook {harness}` to every CLI).
 */
export interface HookEntry {
  /**
   * The command the harness runs before a tool call. `{harness}` is
   * substituted with the harness id. Required and non-empty.
   */
  command: string;
  /**
   * Restrict this entry to these harness ids; absent means every harness
   * being synced.
   */
  harnesses: string[] | null;
  /**
   * Tool-name matcher in the harness's dialect (most read it as a regex);
   * applied to every targeted harness as written. Absent means match-all.
   */
  matcher: string | null;
  /**
   * Identity for plugin-delivered harnesses (Goose, OpenCode) and Copilot's
   * per-owner file, so two tools' hooks never collide. Defaults to
   * `oneharness`.
   */
  plugin_name: string | null;
  /**
   * Timeout in seconds, for the harnesses whose hook schema carries one
   * (Goose, Crush); ignored by the others.
   */
  timeout: number | null;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field8 {
  source: string | null;
  value: number | null;
  [k: string]: unknown;
}
/**
 * The configured `mode`, if any. Unset when only the legacy `bypass` field
 * (or neither) is set — the effective mode then derives from `bypass`.
 */
export interface Field4 {
  source: string | null;
  value: PermissionMode | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field21 {
  source: string | null;
  value: string[] | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field6 {
  source: string | null;
  value: OutputFormat | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field31 {
  source: string | null;
  value: string | null;
  [k: string]: unknown;
}
/**
 * The configured run mode, if any. Unset falls back to `parallel` (the
 * built-in default) at run time.
 */
export interface Field9 {
  source: string | null;
  value: RunMode | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field7 {
  source: string | null;
  value: number | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field1 {
  source: string | null;
  value: boolean | null;
  [k: string]: unknown;
}
/**
 * One resolved field in the `oneharness config` report: the effective value
 * plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
 * are `null` when the field is unset everywhere and has no built-in default.
 */
export interface Field5 {
  source: string | null;
  value: number | null;
  [k: string]: unknown;
}
