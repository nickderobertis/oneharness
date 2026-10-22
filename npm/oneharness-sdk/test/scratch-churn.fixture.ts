// Makes and removes scratch directories in a loop, with no test around it, so
// `scratch.test.ts` can watch them from another process the way a sibling
// run's leak gate does. Named `.fixture.ts` so bun's own test glob leaves it to
// that one caller. The temp dir and run token come from its environment, and
// one directory is left behind at the end, as a real leak would be.
import {
	removeScratch,
	removeScratchAsync,
	scratch,
	scratchSync,
} from "./scratch.mjs";

const cycles = Number(process.argv[2]);
for (let cycle = 0; cycle < cycles; cycle++) {
	scratchSync(`churn-sync-${cycle}`);
	removeScratch();
	await scratch(`churn-async-${cycle}`);
	await removeScratchAsync();
}
scratchSync("leaked");
