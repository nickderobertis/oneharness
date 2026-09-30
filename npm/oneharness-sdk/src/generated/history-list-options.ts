/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by `OneHarness.historyList()` in the published Node SDK.
 */
export interface HistoryListOptions {
  allProjects?: boolean | undefined;
  /**
   * Read every dated index segment plus the legacy index an older
   * oneharness kept, rather than the last 7 UTC days.
   */
  allTime?: boolean | undefined;
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
   * Read the dated index from this UTC date on (`YYYY-MM-DD`) rather than
   * the last 7 UTC days. Refused beside a true `allTime`.
   */
  since?: string | null | undefined;
  /**
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
}
