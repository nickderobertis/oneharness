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
 * Options accepted by `OneHarness.historyList()` in the published Node SDK.
 */
export interface HistoryListOptions {
  allProjects?: boolean | undefined;
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
   * Narrow to one configured harness identity (`claude-code:work`).
   */
  variant?: string | undefined;
  /**
   * Which dates of the index to read: the last `days` UTC days, from a
   * UTC date on, or all time (every dated segment plus the legacy index an
   * older oneharness kept). Omitted, the last 7 UTC days.
   */
  window?: HistoryWindow | null | undefined;
}
