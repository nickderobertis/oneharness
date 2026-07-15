import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const install = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-package-"));
const executableSuffix = process.platform === "win32" ? ".exe" : "";

function runNpm(args, options) {
	if (process.platform !== "win32") return execFileSync("npm", args, options);
	const npmCli = resolve(
		dirname(process.execPath),
		"node_modules/npm/bin/npm-cli.js",
	);
	return execFileSync(process.execPath, [npmCli, ...args], options);
}

const packageDir = execFileSync(process.execPath, ["scripts/sdk-pack.mjs"], {
	cwd: root,
	encoding: "utf8",
}).trim();
const tarball = runNpm(
	["pack", "--silent", "--pack-destination", install],
	{ cwd: packageDir, encoding: "utf8" },
).trim();
writeFileSync(
	resolve(install, "package.json"),
	`${JSON.stringify({ private: true, type: "module" })}\n`,
);
execFileSync("bun", ["add", "--offline", `./${tarball}`], {
	cwd: install,
	stdio: "pipe",
});
writeFileSync(
	resolve(install, "consume.mjs"),
	`import { OneHarness } from "@oneharness/sdk";
const executable = process.env.ONEHARNESS_TEST_BIN;
const paidProvider = process.env.ONEHARNESS_TEST_MOCK;
if (!executable || !paidProvider) throw new Error("package e2e requires ONEHARNESS_TEST_BIN and ONEHARNESS_TEST_MOCK");
const sdk = new OneHarness({ executable, env: { ONEHARNESS_NO_CONFIG: "1" } });
// llmlint: ignore-block[e2e_not_mocked] this packaged-user test crosses the real Node package -> SDK -> built CLI -> oneharness subprocess boundary; only the paid Claude model is replaced through oneharness's deterministic provider seam.
const report = await sdk.run({ prompt: "installed package", harnesses: ["claude-code"], mode: "bypass", env: { MOCK_STDOUT: '{"result":"installed sdk works"}' }, bins: { "claude-code": paidProvider } });
// llmlint: ignore-end[e2e_not_mocked]
if (report.results[0]?.text !== "installed sdk works") throw new Error(JSON.stringify(report));
`,
);
const missingProvider = spawnSync(process.execPath, ["consume.mjs"], {
	cwd: install,
	env: {
		...process.env,
		ONEHARNESS_TEST_BIN: resolve(
			root,
			`target/debug/oneharness${executableSuffix}`,
		),
		ONEHARNESS_TEST_MOCK: "",
	},
	encoding: "utf8",
});
if (
	missingProvider.status === 0 ||
	!missingProvider.stderr.includes("package e2e requires ONEHARNESS_TEST_BIN")
) {
	throw new Error(`missing provider was not rejected: ${missingProvider.stderr}`);
}
execFileSync(process.execPath, ["consume.mjs"], {
	cwd: install,
	env: {
		...process.env,
		ONEHARNESS_TEST_BIN: resolve(
			root,
			`target/debug/oneharness${executableSuffix}`,
		),
		ONEHARNESS_TEST_MOCK: resolve(
			root,
			`target/debug/oneharness-mock-harness${executableSuffix}`,
		),
	},
	stdio: "pipe",
});

const installed = JSON.parse(
	readFileSync(
		resolve(install, "node_modules/@oneharness/sdk/package.json"),
		"utf8",
	),
);
if (installed.version === "0.0.0-managed") {
	throw new Error("packed SDK retained its development placeholder version");
}
