// Preloaded into `scratch-churn.fixture.ts` to refuse, with Windows's EPERM,
// every rename of one scratch directory off the leak gate's sweep until a later
// directory has been placed. It stands in for a sibling run's gate holding
// that directory's marker open for longer than a rename's patience. Unix never
// refuses a rename for an open handle, so injecting the refusal at the `fs`
// boundary is what makes it happen the same way on every platform. Named
// `.fixture.ts` so bun's own test glob leaves it to `scratch.test.ts`.
import { mock } from "bun:test";
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { basename } from "node:path";
import { PREFIX } from "./scratch.mjs";

/** The tag of the directory whose retirement is refused, and of the one whose placement ends that. */
const [held, releasedBy] = [
	process.env.OH_SCRATCH_REFUSE_TAG,
	process.env.OH_SCRATCH_REFUSE_UNTIL_TAG,
];
if (!held || !releasedBy) {
	throw new Error(
		"scratch-refusal: set OH_SCRATCH_REFUSE_TAG and OH_SCRATCH_REFUSE_UNTIL_TAG",
	);
}
let released = false;

/** The EPERM a rename gets on Windows while a handle is open inside its source. */
function refusal(from: string, to: string): NodeJS.ErrnoException {
	return Object.assign(
		new Error(`EPERM: operation not permitted, rename '${from}' -> '${to}'`),
		{ code: "EPERM", errno: -4048, syscall: "rename", path: from, dest: to },
	);
}

/** Whether renaming `from` to `to` is refused, noting a placement that releases the hold. */
function refused(from: fs.PathLike, to: fs.PathLike): boolean {
	const [source, target] = [basename(String(from)), basename(String(to))];
	if (target.startsWith(`${PREFIX}${releasedBy}-`)) released = true;
	return (
		!released &&
		source.startsWith(`${PREFIX}${held}-`) &&
		target.endsWith(".gone")
	);
}

const { renameSync } = fs;
const { rename } = fsp;
mock.module("node:fs", () => ({
	...fs,
	renameSync(from: fs.PathLike, to: fs.PathLike) {
		if (refused(from, to)) throw refusal(String(from), String(to));
		return renameSync(from, to);
	},
}));
mock.module("node:fs/promises", () => ({
	...fsp,
	async rename(from: fs.PathLike, to: fs.PathLike) {
		if (refused(from, to)) throw refusal(String(from), String(to));
		return rename(from, to);
	},
}));
