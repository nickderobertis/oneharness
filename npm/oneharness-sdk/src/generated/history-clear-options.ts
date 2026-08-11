/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' `historyClear()`.
 */
export interface HistoryClearOptions {
  allProjects?: boolean | undefined;
  config?: string | undefined;
  historyDir?: string | undefined;
  noConfig?: boolean | undefined;
  project?: string | undefined;
  /**
   * Actually delete. Absent or false reports what would be removed and
   * removes nothing, so a caller can always look first.
   */
  yes?: boolean | undefined;
}
