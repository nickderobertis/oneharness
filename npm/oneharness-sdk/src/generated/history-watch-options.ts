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
   * Start from the first record the index holds, the legacy index an
   * older oneharness kept included. Refused beside `after`.
   */
  allTime?: boolean | undefined;
  /**
   * Load configuration from exactly these files, in order, skipping
   * user/project discovery; each later file overrides the earlier ones.
   */
  config?: string[] | undefined;
  events?: boolean | undefined;
  historyDir?: string | undefined;
  labels?: HistoryLabels | undefined;
  /**
   * Ignore every configuration file and `ONEHARNESS_*` override. `true`
   * beside a non-empty `config` is refused before anything runs.
   */
  noConfig?: boolean | undefined;
  project?: string | undefined;
  /**
   * Follow one session: its id, or its name (the newest session so named
   * whose labels match every label filter, including one still running, or
   * the first to appear).
   */
  session?: string | null | undefined;
  /**
   * Start from this UTC date's index segments (`YYYY-MM-DD`) rather than
   * today's. Refused beside `after` or a true `allTime`.
   */
  since?: string | null | undefined;
  /**
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
}
export interface HistoryLabels {
  [k: string]: string;
}
