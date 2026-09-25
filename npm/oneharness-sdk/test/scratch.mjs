// Scratch directories that the test framework removes, however a test ends.
//
// `removeScratchAsync` is what each suite registers with `afterEach` — through
// `registerScratchCleanup` in `scratch-hook.mjs`, which carries the one timeout
// that removal is allowed — so a test that throws cleans up exactly like one
// that passes, which a `finally` per call site has to earn again every time.

import { randomBytes } from "node:crypto";
import { mkdirSync, realpathSync, rmSync } from "node:fs";
import { mkdir, rm } from "node:fs/promises";
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
 * A fresh path for `tag`, ending in this process's id — which is how
 * `scripts/check-temp-leaks.sh` tells another checkout's live directory from
 * one this run left behind. `mkdtemp` cannot put anything after its random
 * part, so the caller makes the directory with an exclusive `mkdir` instead.
 *
 * @param {string} tag
 * @param {string} [root] the directory it goes under; the temp dir by default
 * @returns {string}
 */
function scratchPath(tag, root = tmpdir()) {
	const unique = randomBytes(6).toString("hex");
	return resolve(root, `${PREFIX}${tag}-${unique}-${process.pid}`);
}

/**
 * A private directory for one test, removed when that test ends.
 *
 * @param {string} tag distinguishes one case's directory from another's
 * @returns {Promise<string>}
 */
export async function scratch(tag) {
	const directory = scratchPath(tag);
	await mkdir(directory, { mode: 0o700 });
	held.push(directory);
	return directory;
}

/**
 * A scratch session store, for a test whose store path becomes a control socket
 * address: `<store>/control/<name>.sock`.
 *
 * Rooted at the canonical `/tmp` on unix rather than the temp dir, as the Rust
 * suite's `control_store_root` is, because that address has a `sun_path` budget
 * of 104 bytes on macOS and its per-user `$TMPDIR` spends 49 of them before the
 * store's own name begins — a name ending in the pid then leaves the socket no
 * room. Windows has no `sun_path`, so the temp dir serves there.
 *
 * @param {string} tag
 * @returns {Promise<string>}
 */
export async function controlScratch(tag) {
	const root = process.platform === "win32" ? tmpdir() : realpathSync("/tmp");
	const directory = scratchPath(tag, root);
	await mkdir(directory, { mode: 0o700 });
	held.push(directory);
	return directory;
}

/**
 * The same as `scratch`, for a caller with no `await` to spend.
 *
 * @param {string} tag
 * @returns {string}
 */
export function scratchSync(tag) {
	const directory = scratchPath(tag);
	mkdirSync(directory, { mode: 0o700 });
	held.push(directory);
	return directory;
}

/**
 * Remove every scratch directory taken since the last call.
 *
 * Best-effort per directory, and synchronous for the one caller that cannot
 * await: a `process.on("exit")` handler. Every hook a test framework runs uses
 * `removeScratchAsync` instead, so a slow disk blocks nothing.
 */
export function removeScratch() {
	for (const directory of held.splice(0)) {
		rmSync(directory, { recursive: true, force: true });
	}
}

/**
 * The same, awaited rather than blocking the thread that runs the tests.
 *
 * This is the removal every `afterEach` here registers: a synchronous `rmSync`
 * holds the runner's loop for the whole removal, and on a contended disk that
 * is what ran an otherwise-passing test out of its teardown budget.
 *
 * @returns {Promise<void>}
 */
export async function removeScratchAsync() {
	await Promise.all(
		held
			.splice(0)
			.map((directory) => rm(directory, { recursive: true, force: true })),
	);
}
