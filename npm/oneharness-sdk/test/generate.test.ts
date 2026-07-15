import { expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const generatedDirectory = "npm/oneharness-sdk/src/generated";

test("generated contracts stay LF under Windows checkout semantics", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-checkout-"));
	const generated = readdirSync(resolve(root, generatedDirectory)).map(
		(name) => `${generatedDirectory}/${name}`,
	);

	try {
		execFileSync(
			"git",
			[
				"-c",
				"core.autocrlf=true",
				"-c",
				"core.eol=crlf",
				"checkout-index",
				`--prefix=${checkout.replaceAll("\\", "/")}/`,
				"--",
				...generated,
			],
			{ cwd: root },
		);

		for (const path of generated) {
			const content = readFileSync(resolve(checkout, path));
			expect(content.includes(Buffer.from("\r\n"))).toBe(false);
			expect(content.at(-1)).toBe("\n".charCodeAt(0));
		}
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
});
