// A test that fails on purpose, so its teardown is what can be observed.
//
// `scratch.test.ts` runs this file as its own `bun test` subprocess and asserts
// the directory below is gone afterwards — the property a passing test can never
// demonstrate about itself. Named `.fixture.ts` so bun's own test glob leaves it
// to that one caller.
import { afterEach, test } from "bun:test";
import { removeScratch, scratch } from "./scratch.mjs";

afterEach(removeScratch);

test("fails after taking scratch space", async () => {
	console.log(`scratch-fixture-directory ${await scratch("cleanup-probe")}`);
	throw new Error("the failing test this stands in for");
});
