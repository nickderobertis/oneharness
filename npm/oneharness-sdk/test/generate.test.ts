import { expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const sdkDirectory = "npm/oneharness-sdk";
const generatedDirectory = `${sdkDirectory}/src/generated`;
const lfSdkFile = /\.(?:m?js|ts|json)$/u;

test("SDK inputs stay LF under Windows checkout semantics", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-checkout-"));
	const checkoutInputs = execFileSync("git", ["ls-files", "--", sdkDirectory], {
		cwd: root,
		encoding: "utf8",
	})
		.trimEnd()
		.split("\n")
		.filter((path) => lfSdkFile.test(path));
	const generated = checkoutInputs.filter((path) =>
		path.startsWith(`${generatedDirectory}/`),
	);
	const authored = checkoutInputs.filter(
		(path) => !path.startsWith(`${generatedDirectory}/`),
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
				...checkoutInputs,
			],
			{ cwd: root },
		);

		expect(generated.length).toBeGreaterThan(0);
		expect(authored.length).toBeGreaterThan(0);
		for (const path of checkoutInputs) {
			const content = readFileSync(resolve(checkout, path));
			expect(content.includes(Buffer.from("\r\n"))).toBe(false);
			expect(content.at(-1)).toBe("\n".charCodeAt(0));
		}
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
});
