// Scratch directories that the test framework removes, however a test ends.
//
// The shape this replaces took a `mkdtemp` and never gave it back, so every run
// of this suite left one directory per case on the host for good. Enough of them
// fill a root filesystem and take every program on it down.
//
// `removeScratch` is what each suite registers with `afterEach`, so a test that
// throws cleans up exactly like one that passes — the guarantee a `finally` per
// call site has to be rewritten for every time.

import { mkdtempSync, rmSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

/**
 * The prefix every scratch directory here carries.
 *
 * It must begin with `oneharness_core::io::scratch::PREFIX`, which is what
 * `scripts/check-temp-leaks.sh` sweeps for; `scripts/check-scratch-prefixes.sh`
 * holds the two in step, because a prefix that drifted out of the sweep would
 * leave the gate silently passing.
 */
export const PREFIX = "oneharness-sdk-";

/** @type {string[]} */
const held = [];

/**
 * A private directory for one test, removed when that test ends.
 *
 * @param {string} tag distinguishes one case's directory from another's
 * @returns {Promise<string>}
 */
export async function scratch(tag) {
	const directory = await mkdtemp(resolve(tmpdir(), `${PREFIX}${tag}-`));
	held.push(directory);
	return directory;
}

/**
 * The same, for a caller with no `await` to spend.
 *
 * @param {string} tag
 * @returns {string}
 */
export function scratchSync(tag) {
	const directory = mkdtempSync(resolve(tmpdir(), `${PREFIX}${tag}-`));
	held.push(directory);
	return directory;
}

/**
 * Remove every scratch directory taken since the last call.
 *
 * Best-effort per directory, and synchronous so a `process.on("exit")` caller
 * can use it too: an exit handler cannot await.
 */
export function removeScratch() {
	for (const directory of held.splice(0)) {
		rmSync(directory, { recursive: true, force: true });
	}
}
