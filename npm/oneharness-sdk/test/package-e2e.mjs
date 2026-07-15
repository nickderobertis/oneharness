import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const install = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-package-"));
const executableSuffix = process.platform === "win32" ? ".exe" : "";
const target = {
	"darwin-arm64": "aarch64-apple-darwin",
	"darwin-x64": "x86_64-apple-darwin",
	"linux-arm64": "aarch64-unknown-linux-gnu",
	"linux-x64": "x86_64-unknown-linux-gnu",
	"win32-x64": "x86_64-pc-windows-msvc",
}[`${process.platform}-${process.arch}`];
if (!target) {
	throw new Error(`unsupported package test host ${process.platform}-${process.arch}`);
}

function runNpm(args, options) {
	if (process.platform !== "win32") return execFileSync("npm", args, options);
	const npmCli = resolve(
		dirname(process.execPath),
		"node_modules/npm/bin/npm-cli.js",
	);
	return execFileSync(process.execPath, [npmCli, ...args], options);
}

function pack(packageDir) {
	return runNpm(
		["pack", "--silent", "--pack-destination", install],
		{ cwd: packageDir, encoding: "utf8" },
	).trim();
}

const sdkPackageDir = execFileSync(process.execPath, ["scripts/sdk-pack.mjs"], {
	cwd: root,
	encoding: "utf8",
}).trim();
// Package the current launcher and workspace binary exactly as a release does;
// the SDK must still discover them through its normal installed dependency path.
const packageOut = resolve(install, "packages");
const platformPackageDir = execFileSync(
	process.execPath,
	[
		"scripts/npm-build.mjs",
		"platform",
		"--target",
		target,
		"--binary",
		resolve(root, `target/debug/oneharness${executableSuffix}`),
		"--out",
		packageOut,
	],
	{ cwd: root, encoding: "utf8" },
).trim();
const launcherPackageDir = execFileSync(
	process.execPath,
	["scripts/npm-build.mjs", "launcher", "--out", packageOut],
	{ cwd: root, encoding: "utf8" },
).trim();
const sdkTarball = pack(sdkPackageDir);
const launcherTarball = pack(launcherPackageDir);
const platformTarball = pack(platformPackageDir);
const platformPackage = JSON.parse(
	readFileSync(resolve(platformPackageDir, "package.json"), "utf8"),
).name;
// Overrides prevent Bun from satisfying transitive exact versions with cached
// registry copies instead of the local launcher and platform tarballs.
writeFileSync(
	resolve(install, "package.json"),
	`${JSON.stringify({
		private: true,
		type: "module",
		dependencies: {
			"@oneharness/sdk": `file:./${sdkTarball}`,
			"oneharness-cli": `file:./${launcherTarball}`,
			[platformPackage]: `file:./${platformTarball}`,
		},
		overrides: {
			"oneharness-cli": `file:./${launcherTarball}`,
			[platformPackage]: `file:./${platformTarball}`,
		},
	})}\n`,
);
// Bun performs the install; npm is used above only to create real tarballs.
execFileSync("bun", ["install", "--offline"], {
	cwd: install,
	stdio: "pipe",
});
const consumerEnv = {
	...process.env,
	ONEHARNESS_NO_CONFIG: "1",
	ONEHARNESS_TEST_MOCK: resolve(
		root,
		`target/debug/oneharness-mock-harness${executableSuffix}`,
	),
};
// An ambient override would let the workspace binary mask a broken installed
// launcher, so the consumer journey must exercise normal package resolution.
delete consumerEnv.ONEHARNESS_BIN;

const installedLauncher = resolve(
	install,
	"node_modules/.bin",
	// Bun uses a native .exe/.bunx bin shim on Windows, not npm's .cmd shim.
	process.platform === "win32" ? "oneharness.exe" : "oneharness",
);
const launcherArgs = ["list", "--compact"];
const launcherOutput = execFileSync(installedLauncher, launcherArgs, {
	cwd: install,
	env: consumerEnv,
	encoding: "utf8",
});
const list = JSON.parse(launcherOutput);
if (!list.harnesses?.some(({ id }) => id === "claude-code")) {
	throw new Error(`installed launcher returned an invalid list: ${launcherOutput}`);
}

writeFileSync(
	resolve(install, "consume.mjs"),
	`import { OneHarness } from "@oneharness/sdk";
const sdk = new OneHarness({ env: { ONEHARNESS_NO_CONFIG: "1" } });
const report = await sdk.run({ prompt: "installed package", harnesses: ["claude-code"], mode: "bypass", env: { MOCK_STDOUT: '{"result":"installed sdk works"}' }, bins: { "claude-code": process.env.ONEHARNESS_TEST_MOCK } });
if (report.results[0]?.text !== "installed sdk works") throw new Error(JSON.stringify(report));
`,
);
execFileSync(process.execPath, ["consume.mjs"], {
	cwd: install,
	env: consumerEnv,
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
