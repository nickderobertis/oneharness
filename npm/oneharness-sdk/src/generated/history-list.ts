/* Generated from oneharness-core. Do not edit. */

export type HistoryList = HistorySessionSummary[];

/**
 * A one-line summary of a session, for `oneharness history list`. Read from the
 * session's records: `name`/`project`/`started` come from the first record,
 * `harnesses` is the distinct set across all records.
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
   * The RFC3339 UTC start time (first record's timestamp); empty if unknown.
   */
  started: string;
}
