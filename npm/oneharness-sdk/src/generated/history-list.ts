/* Generated from oneharness-core. Do not edit. */

export type HistoryList = HistorySessionSummary[];

/**
 * A one-line summary of a session, for `oneharness history list`, read from
 * both its closing `run` records and its event lines. `name`/`project`/
 * `started` come from the first record; a session still in its first turn has
 * none yet, so they fall back to the events' `session_name`, the project
 * directory's slug, and the instant in the session id. `harnesses` is the
 * distinct set across records and events, and `running` says an event's run
 * has no closing record yet.
 */
export interface HistorySessionSummary {
  /**
   * The distinct harness ids the session touched, in first-seen order.
   */
  harnesses: string[];
  /**
   * The session id (the file stem), unique and sortable by start time.
   */
  id: string;
  labels?: HistoryLabels | undefined;
  /**
   * The human-meaningful session name (non-unique).
   */
  name: string;
  /**
   * The absolute path of the session file.
   */
  path: string;
  /**
   * The project directory the run operated in.
   */
  project: string;
  /**
   * How many harness-run records the session holds.
   */
  record_count: number;
  /**
   * Whether a harness run in this session has written events but not yet
   * its closing record — the run is still going (or ended without one: a
   * killed process leaves the same file). Omitted when false.
   */
  running?: boolean | undefined;
  /**
   * The RFC3339 UTC start time (first record's timestamp); empty if unknown.
   */
  started: string;
  [k: string]: unknown;
}
/**
 * Labels shared by every record in the session. Omitted when empty.
 */
export interface HistoryLabels {
  [k: string]: string;
}
