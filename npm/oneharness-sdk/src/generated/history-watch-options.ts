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
  events?: boolean | undefined;
  historyDir?: string | undefined;
  labels?: HistoryLabels | undefined;
  project?: string | undefined;
}
export interface HistoryLabels {
  [k: string]: string;
}
