/* Generated from oneharness-core. Do not edit. */

/**
 * The `oneharness history clear` output contract.
 *
 * `clear` is a dry run until `--yes`, and the two phases have always printed
 * *different* documents: a real run reports `removed`, a dry run reports
 * `would_remove` plus the hint that makes it real. That difference is the
 * contract, so this is a sum of the two frames rather than one struct with
 * both counts optional — which would let a consumer read a deletion count off
 * a run that deleted nothing. Untagged, because `dry_run` is the discriminant
 * the published shape already carries.
 */
export type HistoryClearReport = HistoryClearRemoved | HistoryClearDryRun;

/**
 * The frame a `--yes` run prints: these files are gone.
 */
export interface HistoryClearRemoved {
  /**
   * Always `false` — the field is what tells the two frames apart, so it is
   * a literal rather than a flag either constructor could get wrong.
   */
  dry_run: false;
  files: string[];
  removed: number;
  [k: string]: unknown;
}
/**
 * The frame a run without `--yes` prints: these files *would* go.
 */
export interface HistoryClearDryRun {
  dry_run: true;
  files: string[];
  /**
   * How to make the dry run real. Constant text, carried in the document so
   * a JSON consumer gets the same guidance the text view prints.
   */
  hint: string;
  would_remove: number;
  [k: string]: unknown;
}
