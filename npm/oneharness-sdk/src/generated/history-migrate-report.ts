/* Generated from oneharness-core. Do not edit. */

/**
 * The `oneharness history migrate` output contract.
 *
 * A type rather than the inline `json!` literal it replaced, because an SDK
 * cannot validate a document that has no schema — which is what left this
 * capability's `output` unbacked in the capability manifest.
 */
export interface HistoryMigrateReport {
  /**
   * One entry per session file the migration touched.
   */
  files: MigrationSummary[];
  /**
   * `files.len()`, carried so a consumer reading only the summary does not
   * have to walk the array.
   */
  files_processed: number;
  [k: string]: unknown;
}
/**
 * Outcome for one session file processed by [`migrate`]. Counts refer to
 * whole legacy records and current v1.0 lines; unreadable lines are preserved
 * byte-for-byte and reported as skipped.
 */
export interface MigrationSummary {
  already_current: number;
  path: string;
  records_migrated: number;
  skipped: number;
  [k: string]: unknown;
}
