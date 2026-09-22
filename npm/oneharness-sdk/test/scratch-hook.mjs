// The one scratch teardown every SDK suite registers, and the one timeout it
// carries.
//
// Kept apart from `scratch.mjs` because that module is also imported by
// `package-e2e.mjs`, which runs under plain node and has no `bun:test` to
// import.

import { afterEach } from "bun:test";
import { removeScratchAsync } from "./scratch.mjs";

/**
 * Bun's own default budget for a hook, which this suite's cleanup must outlive.
 *
 * Named here because it is the number every constant below is measured against:
 * the shared timeout is what raises the cleanup past it, and the fixture that
 * proves the raise works delays a removal past it on purpose.
 */
export const BUN_DEFAULT_HOOK_TIMEOUT_MS = 5_000;

/**
 * How long a scratch removal may take before bun calls the teardown failed.
 *
 * Sized for a contended disk, not for the removal itself: five seconds is a
 * removal's time when nothing else is using the volume, and a parallel build, a
 * container sharing it, or another suite running against the same temp
 * directory has taken a removal past that and failed an otherwise-passing test
 * in its cleanup. Nothing waits this long when the disk is free — the hook ends
 * as soon as the removal does — so the budget costs an uncontended run nothing.
 */
export const SCRATCH_CLEANUP_TIMEOUT_MS = 60_000;

/**
 * Register the shared scratch teardown for the calling suite.
 *
 * @param {() => Promise<void>} [remove] the removal to await; the fixture that
 *   proves this registration really waits out a slow removal passes its own, so
 *   that what it exercises is this hook rather than a second one beside it
 * @returns {void}
 */
export function registerScratchCleanup(remove = removeScratchAsync) {
	afterEach(remove, SCRATCH_CLEANUP_TIMEOUT_MS);
}
