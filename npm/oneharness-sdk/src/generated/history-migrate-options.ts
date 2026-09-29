/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `historyMigrate()`.
 */
export interface HistoryMigrateOptions {
  /**
   * Load configuration from exactly these files, in order, skipping
   * user/project discovery; each later file overrides the earlier ones.
   */
  config?: string[] | undefined;
  historyDir?: string | undefined;
  /**
   * Ignore every configuration file and `ONEHARNESS_*` override. `true`
   * beside a non-empty `config` is refused before anything runs.
   */
  noConfig?: boolean | undefined;
}
