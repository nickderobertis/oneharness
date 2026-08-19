// A test that fails without ever printing a scratch directory.
//
// `scratch.test.ts` runs this file as its own `bun test` subprocess to exercise
// the guard that reads a directory out of that output. A fixture that stopped
// printing — renamed marker, a throw before the log — would otherwise hand
// `undefined` to the existsSync assertion, which then passes for the wrong
// reason and reports the leak guard as working. Named `.fixture.ts` so bun's own
// test glob leaves it to that one caller.
import { test } from "bun:test";

test("fails before it can take scratch space", () => {
	throw new Error("the silent failure this stands in for");
});
