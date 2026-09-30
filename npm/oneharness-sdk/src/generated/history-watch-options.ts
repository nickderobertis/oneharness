/* Generated from oneharness-core. Do not edit. */

/**
 * Which dates of the index a listing reads.
 *
 * It is also the one value an SDK history lookup, listing or watch takes as
 * its `window`, so "from this date" and "from the beginning" cannot both be
 * stated. On the wire it is externally tagged in camelCase —
 * `{"recent": {"days": 3}}`, `{"since": "2026-01-05"}`, or `"allTime"` — and
 * renders to the CLI's `--days`, `--since` or `--all-time`.
 */
export type HistoryWindow =
  | {
      recent: {
        /**
         * How many UTC days, today included; at least one.
         */
        days: number;
      };
    }
  | {
      since: string;
    }
  | "allTime";

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
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
  /**
   * Where the watch starts when no `after` cursor is given: the beginning
   * of the last `days` UTC days, of a UTC date, or of all time (the legacy
   * index an older oneharness kept included). Omitted, the current UTC
   * day. Refused beside `after`, the other answer to where a watch begins.
   */
  window?: HistoryWindow | null | undefined;
}
export interface HistoryLabels {
  [k: string]: string;
}
