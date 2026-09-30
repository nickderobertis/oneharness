/* Generated from oneharness-core. Do not edit. */

/**
 * The `oneharness history reindex` output contract.
 */
export interface HistoryReindexReport {
  /**
   * The sum of `segments[].added`.
   */
  entries_added: number;
  /**
   * How many session files were read.
   */
  files_read: number;
  /**
   * One row per segment this run appended to, in file-name order; a
   * segment that already held every entry is not listed.
   */
  segments: SegmentReindexSummary[];
  /**
   * Every session file (or project directory) that could not be read,
   * with why. The rest are indexed regardless.
   */
  unreadable: UnreadableSessionFile[];
  [k: string]: unknown;
}
/**
 * What [`reindex`] appended to one segment.
 */
export interface SegmentReindexSummary {
  /**
   * How many entries this run appended to it.
   */
  added: number;
  /**
   * The segment's path, as a display string.
   */
  path: string;
  /**
   * The segment's file name (`runs-YYYY-MM-DD.ndjson` or
   * `events-YYYY-MM-DD.ndjson`).
   */
  segment: string;
  [k: string]: unknown;
}
/**
 * A session file [`reindex`] could not read.
 */
export interface UnreadableSessionFile {
  /**
   * Why it could not be read, as the operating system said it.
   */
  error: string;
  path: string;
  [k: string]: unknown;
}
