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
	readFileSync,
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

/** The pause between rename attempts, in milliseconds. */
const RENAME_PAUSE_MS = 5;

/**
 * How long a refused rename keeps trying, in milliseconds: two seconds on
 * Windows, and none elsewhere.
 *
 * Windows refuses to rename a directory while any handle is open inside it,
 * and a sibling run's leak gate opens each swept directory's marker to read
 * it. That refusal lasts only as long as the read. Given up on at once, it left
 * a directory under its swept name that this run's own gate then reported as a
 * leak, so a sibling's sweep alone could fail this run. A unix rename is not
 * refused by an open handle, so a failure there is final at once.
 */
const RENAME_PATIENCE_MS = process.platform === "win32" ? 2000 : 0;

/**
 * Whether a refused rename is worth trying again before `deadline`: a source
 * that is not there is the answer at once, since no sibling's read can end and
 * make it appear.
 *
 * @param {unknown} error
 * @param {number} deadline
 * @returns {boolean}
 */
function worthRetrying(error, deadline) {
	return (
		/** @type {{ code?: unknown }} */ (error)?.code !== "ENOENT" &&
		Date.now() < deadline
	);
}

/**
 * `rename`, tried again every few milliseconds until it succeeds or
 * `patience` milliseconds have passed, when the last attempt's error is thrown.
 * A missing source throws at once.
 * Exported so a test can drive a refusal that clears, and one that does not.
 *
 * @param {string} from
 * @param {string} to
 * @param {number} [patience]
 * @returns {Promise<void>}
 */
export async function renamePatiently(
	from,
	to,
	patience = RENAME_PATIENCE_MS,
) {
	const deadline = Date.now() + patience;
	for (;;) {
		try {
			return await rename(from, to);
		} catch (error) {
			if (!worthRetrying(error, deadline)) throw error;
			await new Promise((wake) => setTimeout(wake, RENAME_PAUSE_MS));
		}
	}
}

/**
 * The same as `renamePatiently`, for a caller with no `await` to spend.
 *
 * @param {string} from
 * @param {string} to
 * @param {number} [patience]
 */
export function renameSyncPatiently(from, to, patience = RENAME_PATIENCE_MS) {
	const deadline = Date.now() + patience;
	for (;;) {
		try {
			return renameSync(from, to);
		} catch (error) {
			if (!worthRetrying(error, deadline)) throw error;
			Atomics.wait(
				new Int32Array(new SharedArrayBuffer(4)),
				0,
				0,
				RENAME_PAUSE_MS,
			);
		}
	}
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
		await renamePatiently(staged, directory);
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
		renameSyncPatiently(staged, directory);
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
 * The run `directory`'s marker names, if it has one.
 *
 * @param {string} directory
 * @returns {string | undefined}
 */
function markerOf(directory) {
	try {
		return readFileSync(resolve(directory, RUN_MARKER), "utf8");
	} catch {
		return undefined;
	}
}

/**
 * Say that `directory` stays under its swept name, marked for its run, because
 * renaming it off the sweep failed with `error`: its run's gate reports it as a
 * leak, and this is the reason beside it.
 *
 * @param {string} directory
 * @param {unknown} error
 */
function leftInPlace(directory, error) {
	console.error(
		`scratch: left ${directory} in place, still marked for its run, because it could not be renamed off the leak gate's sweep: ${error}`,
	);
}

/**
 * Give back whatever a failed removal left at `gone`: marked for its run again
 * while still hidden, then under its swept name `directory`.
 *
 * @param {string} gone
 * @param {string} directory
 * @param {string | undefined} marker
 */
function returnToSweep(gone, directory, marker) {
	if (!existsSync(gone)) return;
	if (marker !== undefined) writeFileSync(resolve(gone, RUN_MARKER), marker);
	renameSyncPatiently(gone, directory);
}

/**
 * Remove `directory` without ever leaving it visible to the leak gate
 * unmarked: a sibling run's gate counts an unmarked directory as its own leak
 * on Windows, where it cannot read this process's environment. Prefer
 * `removeScratch` or `removeScratchAsync`; this is exported so a test can
 * retire one directory where removal is made to fail.
 *
 * It is renamed off the sweep first, and only then emptied. A marked directory
 * that cannot be renamed (Windows refuses while a handle stays open inside
 * past `renamePatiently`'s retries) is left whole and marked, a leak its own
 * run's gate still reports. An unmarked one has no ownership to lose and is
 * removed in place. Its marker is removed first, so whatever a failed removal leaves is the same in every order the
 * filesystem lists entries. That remainder gets its original marker back while
 * still hidden, then returns to the swept name.
 *
 * @param {string} directory
 * @returns {Promise<void>}
 */
export async function retireScratch(directory) {
	const marker = markerOf(directory);
	const gone = offSweep(directory, "gone");
	try {
		await renamePatiently(directory, gone);
	} catch (error) {
		if (marker !== undefined) {
			leftInPlace(directory, error);
		} else {
			await rm(directory, { recursive: true, force: true });
		}
		return;
	}
	try {
		await rm(resolve(gone, RUN_MARKER), { force: true });
		await rm(gone, { recursive: true, force: true });
	} finally {
		returnToSweep(gone, directory, marker);
	}
}

/**
 * The same as `retireScratch`, for a caller with no `await` to spend.
 *
 * @param {string} directory
 */
export function retireScratchSync(directory) {
	const marker = markerOf(directory);
	const gone = offSweep(directory, "gone");
	try {
		renameSyncPatiently(directory, gone);
	} catch (error) {
		if (marker !== undefined) {
			leftInPlace(directory, error);
		} else {
			rmSync(directory, { recursive: true, force: true });
		}
		return;
	}
	try {
		rmSync(resolve(gone, RUN_MARKER), { force: true });
		rmSync(gone, { recursive: true, force: true });
	} finally {
		returnToSweep(gone, directory, marker);
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
	for (const directory of held.splice(0)) retireScratchSync(directory);
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
	await Promise.all(held.splice(0).map(retireScratch));
}
