/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by `OneHarness.historyList()` in the published Node SDK.
 */
export interface HistoryListOptions {
  allProjects?: boolean | undefined;
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
  project?: string | undefined;
  /**
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
}
