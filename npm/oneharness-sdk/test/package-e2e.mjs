import { execFileSync } from "node:child_process";
import {
	existsSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	renameSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, resolve } from "node:path";
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

function childEnvironment(extra = {}) {
	const environment = { ...process.env, ...extra };
	delete environment.ONEHARNESS_BIN;
	if ("ONEHARNESS_BIN" in environment) {
		throw new Error("ONEHARNESS_BIN must be absent from package test children");
	}
	return environment;
}

function resolveNpmCli() {
	const candidates = [
		process.env.npm_execpath,
		resolve(dirname(process.execPath), "node_modules/npm/bin/npm-cli.js"),
		resolve(dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
		resolve(dirname(process.execPath), "../share/nodejs/npm/bin/npm-cli.js"),
	];
	const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";
	for (const pathEntry of (process.env.PATH ?? "").split(delimiter)) {
		if (!pathEntry) continue;
		const command = resolve(pathEntry, npmCommand);
		if (!existsSync(command)) continue;
		if (process.platform !== "win32") candidates.push(realpathSync(command));
		candidates.push(
			resolve(pathEntry, "node_modules/npm/bin/npm-cli.js"),
			resolve(pathEntry, "../lib/node_modules/npm/bin/npm-cli.js"),
		);
	}
	const npmCli = candidates.find(
		(candidate) => candidate?.endsWith("npm-cli.js") && existsSync(candidate),
	);
	if (!npmCli) throw new Error("could not resolve npm CLI for package test");
	return npmCli;
}

const baseEnv = childEnvironment();
const npmCache = resolve(install, "npm-install-cache");
function isolatedNpmEnvironment(cache) {
	return childEnvironment({
		npm_config_audit: "false",
		npm_config_cache: cache,
		npm_config_fetch_retries: "0",
		npm_config_fund: "false",
		npm_config_offline: "true",
		npm_config_registry: "http://127.0.0.1:9/",
		npm_config_update_notifier: "false",
	});
}
const npmPackEnv = isolatedNpmEnvironment(resolve(install, "npm-pack-cache"));
const npmInstallEnv = isolatedNpmEnvironment(npmCache);
const npmCli = resolveNpmCli();

function runNpm(args, options) {
	return execFileSync(process.execPath, [npmCli, ...args], options);
}

function pack(packageDir) {
	return runNpm(
		["pack", "--silent", "--pack-destination", install],
		{ cwd: packageDir, encoding: "utf8", env: npmPackEnv },
	).trim();
}

const sdkPackageDir = execFileSync(process.execPath, ["scripts/sdk-pack.mjs"], {
	cwd: root,
	encoding: "utf8",
	env: baseEnv,
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
	{ cwd: root, encoding: "utf8", env: baseEnv },
).trim();
const launcherPackageDir = execFileSync(
	process.execPath,
	["scripts/npm-build.mjs", "launcher", "--out", packageOut],
	{ cwd: root, encoding: "utf8", env: baseEnv },
).trim();
const sdkTarball = pack(sdkPackageDir);
const launcherTarball = pack(launcherPackageDir);
const platformTarball = pack(platformPackageDir);
const platformPackage = JSON.parse(
	readFileSync(resolve(platformPackageDir, "package.json"), "utf8"),
).name;
// Pack the SDK's third-party runtime closure too. Root file dependencies let
// npm satisfy every transitive edge without a registry or ambient cache.
const runtimePackages = [
	"ajv",
	"fast-deep-equal",
	"fast-uri",
	"json-schema-traverse",
	"require-from-string",
];
const runtimeTarballs = Object.fromEntries(
	runtimePackages.map((packageName) => [
		packageName,
		pack(resolve(root, "npm/oneharness-sdk/node_modules", packageName)),
	]),
);
const dependencies = {
	"@oneharness/sdk": `file:./${sdkTarball}`,
	"oneharness-cli": `file:./${launcherTarball}`,
	[platformPackage]: `file:./${platformTarball}`,
};
for (const [packageName, tarball] of Object.entries(runtimeTarballs)) {
	dependencies[packageName] = `file:./${tarball}`;
}
writeFileSync(
	resolve(install, "package.json"),
	`${JSON.stringify({
		private: true,
		type: "module",
		dependencies,
	})}\n`,
);
// The empty isolated cache and unreachable registry make a missing local
// artifact fail loudly even when the developer's normal npm cache is warm.
runNpm(
	[
		"install",
		"--offline",
		"--cache",
		npmCache,
		"--registry",
		"http://127.0.0.1:9/",
		"--ignore-scripts",
		"--no-audit",
		"--no-fund",
		"--package-lock=false",
	],
	{
		cwd: install,
		env: npmInstallEnv,
		encoding: "utf8",
		stdio: "pipe",
	},
);
const consumerEnv = childEnvironment({
	ONEHARNESS_NO_CONFIG: "1",
	ONEHARNESS_TEST_MOCK: resolve(
		root,
		`target/debug/oneharness-mock-harness${executableSuffix}`,
	),
});

const installedLauncher = resolve(
	install,
	"node_modules/.bin",
	process.platform === "win32" ? "oneharness.cmd" : "oneharness",
);
if (!existsSync(installedLauncher)) {
	throw new Error(`installed launcher does not exist: ${installedLauncher}`);
}
if (process.platform === "win32") {
	if (!installedLauncher.toLowerCase().endsWith("\\node_modules\\.bin\\oneharness.cmd")) {
		throw new Error(`Windows launcher is not the consumer .cmd: ${installedLauncher}`);
	}
	const shim = readFileSync(installedLauncher, "utf8");
	if (!shim.includes("oneharness-cli") || !shim.includes("oneharness.js")) {
		throw new Error(`Windows .cmd does not target the installed launcher: ${shim}`);
	}
}
const launcherArgs = ["list", "--compact"];
function runInstalledLauncher() {
	const command =
		process.platform === "win32"
			? process.env.ComSpec ?? "cmd.exe"
			: installedLauncher;
	const args =
		process.platform === "win32"
			? ["/d", "/s", "/c", installedLauncher, ...launcherArgs]
			: launcherArgs;
	return execFileSync(command, args, {
		cwd: install,
		env: consumerEnv,
		encoding: "utf8",
		stdio: "pipe",
	});
}
const launcherOutput = runInstalledLauncher();
const list = JSON.parse(launcherOutput);
if (!list.harnesses?.some(({ id }) => id === "claude-code")) {
	throw new Error(`installed launcher returned an invalid list: ${launcherOutput}`);
}

const installedPlatformDir = resolve(install, "node_modules", platformPackage);
const binaryName = `oneharness${executableSuffix}`;
const packedPlatformBinary = resolve(platformPackageDir, "bin", binaryName);
const installedPlatformBinary = resolve(installedPlatformDir, "bin", binaryName);
if (
	!existsSync(installedPlatformBinary) ||
	!readFileSync(installedPlatformBinary).equals(readFileSync(packedPlatformBinary))
) {
	throw new Error("installed host binary did not come from the platform tarball");
}

writeFileSync(
	resolve(install, "consume.mjs"),
	`import { OneHarness } from "@oneharness/sdk";
const sdk = new OneHarness({ env: { ONEHARNESS_NO_CONFIG: "1" } });
// This bin override is the provider fixture, not the oneharness executable.
const report = await sdk.run({ prompt: "installed package", harnesses: ["claude-code"], mode: "bypass", env: { MOCK_STDOUT: '{"result":"installed sdk works"}' }, bins: { "claude-code": process.env.ONEHARNESS_TEST_MOCK } });
if (report.results[0]?.text !== "installed sdk works") throw new Error(JSON.stringify(report));
`,
);
function runConsumer() {
	return execFileSync(process.execPath, ["consume.mjs"], {
		cwd: install,
		env: consumerEnv,
		encoding: "utf8",
		stdio: "pipe",
	});
}

function expectInvocationFailure(label, invoke, expected) {
	try {
		invoke();
	} catch (error) {
		if (error.code !== "ENOENT" && !(Number.isInteger(error.status) && error.status !== 0)) {
			throw new Error(`${label} did not fail at the subprocess boundary`);
		}
		const diagnostic = [error.message, error.stdout, error.stderr]
			.filter(Boolean)
			.map(String)
			.join("\n");
		if (!diagnostic.includes(expected)) {
			throw new Error(`${label} failed for the wrong reason: ${diagnostic}`);
		}
		return;
	}
	throw new Error(`${label} unexpectedly succeeded`);
}

runConsumer();

const installedLauncherDir = resolve(install, "node_modules/oneharness-cli");
const removedLauncherDir = `${installedLauncherDir}.removed`;
renameSync(installedLauncherDir, removedLauncherDir);
try {
	expectInvocationFailure(
		"installed command without its launcher package",
		runInstalledLauncher,
		process.platform === "win32" ? "oneharness.js" : "ENOENT",
	);
	expectInvocationFailure(
		"installed SDK without its launcher package",
		runConsumer,
		"oneharness-cli",
	);
} finally {
	renameSync(removedLauncherDir, installedLauncherDir);
}
runInstalledLauncher();
runConsumer();

const removedPlatformDir = `${installedPlatformDir}.removed`;
renameSync(installedPlatformDir, removedPlatformDir);
try {
	expectInvocationFailure(
		"installed command without its host platform package",
		runInstalledLauncher,
		`platform package ${platformPackage} is not installed`,
	);
	expectInvocationFailure(
		"installed SDK without its host platform package",
		runConsumer,
		`platform package ${platformPackage} is not installed`,
	);
} finally {
	renameSync(removedPlatformDir, installedPlatformDir);
}
runInstalledLauncher();
runConsumer();

const installed = JSON.parse(
	readFileSync(
		resolve(install, "node_modules/@oneharness/sdk/package.json"),
		"utf8",
	),
);
if (installed.version === "0.0.0-managed") {
	throw new Error("packed SDK retained its development placeholder version");
}
