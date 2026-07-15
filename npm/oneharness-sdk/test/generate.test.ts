import { expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import {
	copyFileSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { generatedFileMatches } from "../scripts/generated-file.mjs";

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

test("a missing generated contract is reported as stale", () => {
	const missing = resolve(
		tmpdir(),
		`oneharness-missing-${crypto.randomUUID()}`,
	);
	expect(generatedFileMatches(missing, Buffer.from("expected"))).toBe(false);
});

test("SDK packing reports a missing Cargo version without a stack trace", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-pack-"));
	mkdirSync(resolve(checkout, "scripts"));
	copyFileSync(
		resolve(root, "scripts/sdk-pack.mjs"),
		resolve(checkout, "scripts/sdk-pack.mjs"),
	);
	writeFileSync(
		resolve(checkout, "Cargo.toml"),
		'[package]\nname = "broken"\n',
	);

	try {
		execFileSync(process.execPath, ["scripts/sdk-pack.mjs"], {
			cwd: checkout,
			stdio: "pipe",
		});
		throw new Error("sdk-pack unexpectedly accepted a missing version");
	} catch (error) {
		if (!(error instanceof Error) || !("stderr" in error)) throw error;
		const stderr = String(error.stderr);
		expect(stderr).toContain("Cargo.toml has no [package] version");
		expect(stderr).toContain("restore the root manifest");
		expect(stderr).not.toContain("at file:");
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
});
