// Scratch directories that the test framework removes, however a test ends.
//
// `removeScratchAsync` is what each suite registers with `afterEach` — through
// `registerScratchCleanup` in `scratch-hook.mjs`, which carries the one timeout
// that removal is allowed — so a test that throws cleans up exactly like one
// that passes, which a `finally` per call site has to earn again every time.

import { randomBytes } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	realpathSync,
	renameSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { mkdir, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, resolve } from "node:path";

/**
 * The prefix every scratch directory here carries.
 *
 * It must begin with `oneharness_core::io::scratch::PREFIX`, which is what
 * `scripts/check-temp-leaks.sh` sweeps for; `scripts/check-scratch-prefixes.sh`
 * holds the two in step, because a prefix that drifted out of the sweep would
 * leave the gate silently passing.
 */
export const PREFIX = "oneharness-sdk-";

/**
 * The variable `scripts/check-temp-leaks.sh` hands every process of its run,
 * naming that run, and the file each scratch directory here carries it in.
 *
 * Spelled as `oneharness_core::io::scratch` spells them, which is where the
 * gate reads them; `scripts/check-scratch-prefixes.sh` holds the two in step.
 * The marker is how the gate tells another live run's scratch from this run's
 * without reading this process's environment, which Windows does not expose.
 */
export const RUN_ENV = "OH_TEMP_LEAKS_RUN";
export const RUN_MARKER = ".oh-temp-leaks-run";

/** @type {string[]} */
const held = [];

/**
 * The run token to write into a new directory's marker, if a gate is watching.
 *
 * @returns {string | undefined}
 */
function runToken() {
	return process.env[RUN_ENV] || undefined;
}

/**
 * A fresh path for `tag`, ending in this process's id — which is how
 * `scripts/check-temp-leaks.sh` tells another checkout's live directory from
 * one this run left behind when it has no marker to read. `mkdtemp` cannot put
 * anything after its random part, so the caller makes the directory with an
 * exclusive `mkdir` instead.
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
	return placeScratch(scratchPath(tag));
}

/**
 * The name `directory` is made or removed under, out of the leak gate's sweep:
 * a leading dot means it no longer starts with `PREFIX`.
 *
 * @param {string} directory
 * @param {"new" | "gone"} stage
 * @returns {string}
 */
function offSweep(directory, stage) {
	return resolve(dirname(directory), `.${basename(directory)}.${stage}`);
}

/**
 * Make `directory`, marked for the run a gate is watching, if any, and hold it
 * for removal. Prefer `scratch`, `scratchSync` or `controlScratch`, which name
 * it; this is exported so a test can place one where something already is.
 *
 * A marked directory is made and marked under a name off the sweep, then
 * renamed into place. A sibling run's gate sweeping the same temp dir
 * therefore never sees it unmarked. An unmarked directory would fall back to
 * this process's environment, which Windows cannot read, so the gate would
 * count it as that run's own leak.
 *
 * @param {string} directory
 * @returns {Promise<string>}
 */
export async function placeScratch(directory) {
	const token = runToken();
	if (!token) {
		await mkdir(directory, { mode: 0o700 });
		held.push(directory);
		return directory;
	}
	const staged = offSweep(directory, "new");
	try {
		await mkdir(staged, { mode: 0o700 });
		await writeFile(resolve(staged, RUN_MARKER), token);
		await rename(staged, directory);
	} catch (error) {
		await rm(staged, { recursive: true, force: true });
		throw error;
	}
	held.push(directory);
	return directory;
}

/**
 * The same as `placeScratch`, for a caller with no `await` to spend.
 *
 * @param {string} directory
 * @returns {string}
 */
export function placeScratchSync(directory) {
	const token = runToken();
	if (!token) {
		mkdirSync(directory, { mode: 0o700 });
		held.push(directory);
		return directory;
	}
	const staged = offSweep(directory, "new");
	try {
		mkdirSync(staged, { mode: 0o700 });
		writeFileSync(resolve(staged, RUN_MARKER), token);
		renameSync(staged, directory);
	} catch (error) {
		rmSync(staged, { recursive: true, force: true });
		throw error;
	}
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
	return placeScratch(scratchPath(tag, root));
}

/**
 * The same as `scratch`, for a caller with no `await` to spend.
 *
 * @param {string} tag
 * @returns {string}
 */
export function scratchSync(tag) {
	return placeScratchSync(scratchPath(tag));
}

/**
 * Remove `directory`, first renaming it off the sweep. A directory
 * half-removed under its own name has lost its marker but is still visible to
 * a sibling run's gate. If the rename fails (Windows refuses it while a handle
 * is open inside), the directory is removed in place. One that cannot be
 * removed goes back to its swept name, so the gate still reports it.
 *
 * @param {string} directory
 * @returns {Promise<void>}
 */
async function retire(directory) {
	const gone = offSweep(directory, "gone");
	try {
		await rename(directory, gone);
	} catch {
		await rm(directory, { recursive: true, force: true });
		return;
	}
	try {
		await rm(gone, { recursive: true, force: true });
	} finally {
		if (existsSync(gone)) renameSync(gone, directory);
	}
}

/**
 * The same as `retire`, for a caller with no `await` to spend.
 *
 * @param {string} directory
 */
function retireSync(directory) {
	const gone = offSweep(directory, "gone");
	try {
		renameSync(directory, gone);
	} catch {
		rmSync(directory, { recursive: true, force: true });
		return;
	}
	try {
		rmSync(gone, { recursive: true, force: true });
	} finally {
		if (existsSync(gone)) renameSync(gone, directory);
	}
}

/**
 * Remove every scratch directory taken since the last call.
 *
 * Best-effort per directory, and synchronous for the one caller that cannot
 * await: a `process.on("exit")` handler. Every hook a test framework runs uses
 * `removeScratchAsync` instead, so a slow disk blocks nothing.
 */
export function removeScratch() {
	for (const directory of held.splice(0)) retireSync(directory);
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
	await Promise.all(held.splice(0).map(retire));
}
