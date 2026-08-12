/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by `OneHarness.historyList()` in the published Node SDK.
 */
export interface HistoryListOptions {
  allProjects?: boolean | undefined;
  /**
   * Load configuration from exactly this file, skipping user/project
   * discovery.
   */
  config?: string | undefined;
  historyDir?: string | undefined;
  /**
   * Ignore every configuration file.
   */
  noConfig?: boolean | undefined;
  project?: string | undefined;
  /**
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
}
