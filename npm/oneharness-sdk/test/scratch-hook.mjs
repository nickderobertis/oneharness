// The one scratch teardown every SDK suite registers, and the one timeout it
// carries.
//
// Kept apart from `scratch.mjs` because that module is also imported by
// `package-e2e.mjs`, which runs under plain node and has no `bun:test` to
// import.

import { afterEach } from "bun:test";
import { removeScratchAsync } from "./scratch.mjs";

/**
 * How long a scratch removal may take before bun calls the teardown failed.
 *
 * Sized for a contended disk rather than for the removal itself. Bun's own
 * default hook budget — five seconds — is a removal's time when nothing else is
 * using the volume; a parallel build, a container sharing it, or another suite
 * against the same temp directory has taken a removal past that and failed an
 * otherwise-passing test in its cleanup. Nothing waits this long when the disk
 * is free, because the hook ends as soon as the removal does, so the budget
 * costs an uncontended run nothing.
 */
export const SCRATCH_CLEANUP_TIMEOUT_MS = 60_000;

/**
 * How long the cleanup fixtures make a teardown take.
 *
 * It has to outlast bun's own default hook budget, which is the whole reason a
 * cleanup that finishes is attributable to `SCRATCH_CLEANUP_TIMEOUT_MS` rather
 * than to bun having been patient enough on its own. Bun owns that default and
 * can raise it, so the relationship is asserted instead of assumed:
 * `bun-hook-budget.fixture.ts` waits this same span under a hook registered
 * without the shared timeout, and `scratch.test.ts` requires bun to cut it off.
 * A raised default therefore turns this suite red rather than leaving a fixture
 * quietly proving nothing.
 */
export const SLOW_CLEANUP_MS = 6_000;

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
