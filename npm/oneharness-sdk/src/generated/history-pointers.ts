/* Generated from oneharness-core. Do not edit. */

/**
 * What [`read_pointers`] read: every well-formed [`HistoryPointer`] line in
 * file order, and how many lines were not one.
 */
export interface HistoryPointers {
  /**
   * The pointer lines, in the order they were appended.
   */
  pointers: HistoryPointer[];
  /**
   * Lines that were not one complete pointer object — a torn tail left by
   * an interrupted writer, or a foreign line — counted rather than failing
   * the read.
   */
  skipped: number;
  [k: string]: unknown;
}
/**
 * One line of a run's **pointer file**: where one harness run's history went.
 *
 * A consumer that starts many `oneharness` processes (or in-process runs) names
 * one file, and every run with history on appends one of these per harness run
 * it begins — so "which sessions did this run launch, and where are they" is a
 * read of that one small file rather than a scan of the whole store, wherever
 * the store lives. `history_dir` / `history_project` / `history_session` are
 * spelled as the `oneharness-session` artifact `oneagentgraph` publishes, so a
 * reader already resolving that through `io::history::find_session_path`
 * resolves these unchanged. Pure: the I/O layer fills the timestamp.
 */
export interface HistoryPointer {
  /**
   * The harness id's base, e.g. `claude-code`.
   */
  harness: string;
  /**
   * The whole configured id, e.g. `claude-code:primary`.
   */
  harness_id: string;
  /**
   * The store the session is under, absolute.
   */
  history_dir: string;
  /**
   * The session file, absolute; the same path the run report echoes.
   */
  history_file: string;
  /**
   * The history id of the record this harness run will close with — the
   * exact id `history show <history-id>` resolves.
   */
  history_id: string;
  /**
   * The project slug — the session file's parent directory name.
   */
  history_project: string;
  /**
   * The session id — the session file's stem.
   */
  history_session: string;
  labels?: HistoryLabels | undefined;
  /**
   * The session's human-meaningful name (see [`session_name`]).
   */
  name: string;
  /**
   * The project directory the run operates in, canonical.
   */
  project: string;
  /**
   * [`POINTER_SCHEMA_VERSION`].
   */
  schema_version: string;
  /**
   * RFC3339 UTC instant this harness run began.
   */
  started: string;
  /**
   * The variant, omitted for a bare harness.
   */
  variant?: string | null | undefined;
  [k: string]: unknown;
}
/**
 * The session's validated labels, omitted when empty.
 */
export interface HistoryLabels {
  [k: string]: string;
}
