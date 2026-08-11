/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `init()`.
 */
export interface InitOptions {
  force?: boolean | undefined;
  /**
   * Where to write the starter config. Absent means `oneharness.toml` in the
   * working directory, exactly as the CLI's own default.
   */
  path?: string | undefined;
}
