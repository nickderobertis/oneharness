// Makes and removes scratch directories in a loop, with no test around it, so
// `scratch.test.ts` can watch them from another process the way a sibling
// run's leak gate does. Named `.fixture.ts` so bun's own test glob leaves it to
// that one caller. The temp dir and run token come from its environment, and
// one directory is left behind at the end, as a real leak would be. A last
// removal before it gives back any directory a refused rename made the cycles
// leave in place, as a suite's next teardown would. That last directory is
// named on stdout and this process stays alive until its stdin closes, so the
// watcher's listing of a live directory is ordered rather than left to the
// scheduler.
import { text } from "node:stream/consumers";
import {
	removeScratch,
	removeScratchAsync,
	scratch,
	scratchSync,
} from "./scratch.mjs";

const cycles = Number(process.argv[2]);
if (!Number.isSafeInteger(cycles) || cycles < 0) {
	throw new Error(
		`scratch-churn: expected a whole number of cycles, got ${JSON.stringify(process.argv[2])}`,
	);
}
for (let cycle = 0; cycle < cycles; cycle++) {
	scratchSync(`churn-sync-${cycle}`);
	removeScratch();
	await scratch(`churn-async-${cycle}`);
	await removeScratchAsync();
}
await removeScratchAsync();
process.stdout.write(`${scratchSync("leaked")}\n`);
await text(process.stdin);
