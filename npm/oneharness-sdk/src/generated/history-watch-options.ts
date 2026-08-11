/* Generated from oneharness-core. Do not edit. */

/**
 * Options accepted by the language SDKs' continuous history iterators.
 *
 * The CLI spells `labels` as repeated `--label key=value` arguments, while an
 * SDK can expose the validated map directly. Unknown fields remain a boundary
 * error, as they are for every other SDK input contract.
 */
export interface HistoryWatchOptions {
  after?: string | undefined;
  allProjects?: boolean | undefined;
  /**
   * Load configuration from exactly this file, skipping user/project
   * discovery.
   */
  config?: string | undefined;
  events?: boolean | undefined;
  historyDir?: string | undefined;
  labels?: HistoryLabels | undefined;
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
export interface HistoryLabels {
  [k: string]: string;
}
