#!/usr/bin/env node
// Hold the Nx project graph to the boundaries this repository declares, reading
// the graph Nx itself computes — implicit dependencies and the edges it infers
// from package manifests and imports alike — rather than the project files.
//
//   node scripts/check-nx-graph.mjs
//
// Three properties, each failing with the edge or project it is about:
//
//   * every project carries exactly one `type:*` tag that
//     tools/workspace/boundaries.json declares;
//   * every dependency edge goes from a type to a type its `allow` list names —
//     so nothing depends on a live, exploration, e2e or other leaf project, and
//     the SDK contract depends on no consumer;
//   * every Cargo path dependency between workspace crates is an edge in the
//     graph. Nx does not read Cargo manifests, so each Rust project restates its
//     crate edges as `implicitDependencies`; a restatement nothing reconciles
//     would let affected selection silently skip a dependent crate.
//
// And three restatements of the project set are held to it: the root AGENTS.md's
// "Projects in the graph" record, the Rust `test` runs rust-coverage depends on —
// a Rust project missing there would leave its crate's coverage silently out of
// the floor — and the shell test steps shell-coverage depends on, likewise.
//
// Quiet on success: one line. Node built-ins only.
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const rulesPath = join(root, "tools/workspace/boundaries.json");
const failures = [];

function die(message) {
	console.error(`check-nx-graph: ${message}`);
	process.exit(1);
}

let rules;
try {
	rules = JSON.parse(readFileSync(rulesPath, "utf8"));
} catch (error) {
	die(`cannot read ${relative(root, rulesPath)} (${error.message}); restore it.`);
}
const allow = rules.allow ?? {};

const nxPackage = createRequire(join(root, "package.json")).resolve("nx/package.json");
const nxBin = join(dirname(nxPackage), JSON.parse(readFileSync(nxPackage, "utf8")).bin.nx);

// `die` exits on the spot, which would skip a `finally`; so a failed graph is
// recorded and reported only after the scratch directory is gone.
const scratch = mkdtempSync(join(tmpdir(), "check-nx-graph-"));
let graph;
let graphError;
try {
	const file = join(scratch, "graph.json");
	try {
		execFileSync(process.execPath, [nxBin, "graph", `--file=${file}`], {
			cwd: root,
			stdio: ["ignore", "ignore", "pipe"],
			env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true", NX_TUI: "false" },
		});
		graph = JSON.parse(readFileSync(file, "utf8")).graph;
	} catch (error) {
		graphError = String(error.stderr ?? error.message).trim();
	}
} finally {
	rmSync(scratch, { recursive: true, force: true });
}
if (graphError !== undefined) {
	die(
		`Nx could not compute the project graph:\n${graphError}\n  fix: run 'bash scripts/nx show projects' to see the same error, repair the project definition it names, and re-run.`,
	);
}

const typeOf = new Map();
for (const [name, node] of Object.entries(graph.nodes)) {
	const types = (node.data.tags ?? []).filter((tag) => tag.startsWith("type:"));
	if (types.length !== 1) {
		failures.push(`${name} carries ${types.length} type tags (${types.join(", ") || "none"}); give it exactly one`);
	} else if (!(types[0] in allow)) {
		failures.push(`${name} is tagged ${types[0]}, which tools/workspace/boundaries.json does not declare; retag it with a declared type or declare this one there`);
	} else {
		typeOf.set(name, types[0]);
	}
}

const edges = new Set();
for (const [source, dependencies] of Object.entries(graph.dependencies)) {
	if (!(source in graph.nodes)) continue;
	for (const { target } of dependencies) {
		if (!(target in graph.nodes)) continue;
		edges.add(`${source}->${target}`);
		const from = typeOf.get(source);
		const to = typeOf.get(target);
		if (from === undefined || to === undefined) continue;
		if (!allow[from].includes(to)) {
			failures.push(
				`${source} (${from}) -> ${target} (${to}) is a dependency ${from} may not have; ${from} may depend only on ${allow[from].join(", ") || "nothing"} (tools/workspace/boundaries.json), so remove the edge or move the shared code into a project it may depend on`,
			);
		}
	}
}

// Cargo: which project each workspace manifest belongs to.
const projectByManifest = new Map();
for (const [name, node] of Object.entries(graph.nodes)) {
	const manifest = rules.cargoManifests?.[name] ?? join(node.data.root, "Cargo.toml");
	if (existsSync(join(root, manifest))) projectByManifest.set(resolve(root, manifest), name);
}
let metadata;
try {
	metadata = JSON.parse(
		execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--offline", "--locked"], {
			cwd: root,
			encoding: "utf8",
			stdio: ["ignore", "pipe", "pipe"],
			maxBuffer: 64 * 1024 * 1024,
		}),
	);
} catch (error) {
	die(
		`cargo metadata failed:\n${String(error.stderr ?? error.message).trim()}\nNext: fix the Cargo.toml or Cargo.lock error above (run \`cargo fetch --locked\` if a dependency is missing offline), then re-run this check.`,
	);
}
for (const pkg of metadata.packages) {
	const project = projectByManifest.get(resolve(pkg.manifest_path));
	if (project === undefined) {
		failures.push(`crate ${pkg.name} (${relative(root, pkg.manifest_path)}) belongs to no Nx project; give its directory a project.json`);
		continue;
	}
	for (const dependency of pkg.dependencies) {
		if (dependency.path === undefined) continue;
		const target = projectByManifest.get(resolve(dependency.path, "Cargo.toml"));
		if (target === undefined || target === project) continue;
		if (!edges.has(`${project}->${target}`)) {
			failures.push(
				`crate ${pkg.name} depends on ${dependency.name} by path, but the graph has no ${project} -> ${target} edge; add "${target}" to ${project}'s implicitDependencies`,
			);
		}
	}
}

// The Rust floor covers every Rust project's `test`, by project and by record.
const recordOf = (name) => {
	const options = graph.nodes[name].data.targets.test?.options ?? {};
	const first = (options.commands ?? [options.command ?? ""])[0] ?? "";
	const match = first.match(/^bash scripts\/cargo-test\.sh ([a-z0-9_-]+)(?:.* --record ([a-z0-9_-]+))?/u);
	return match ? (match[2] ?? match[1]) : undefined;
};
const rustTests = Object.keys(graph.nodes)
	.filter((name) => (graph.nodes[name].data.tags ?? []).includes("lang:rust") && graph.nodes[name].data.targets.test)
	.sort();
const coverage = graph.nodes["rust-coverage"]?.data.targets.coverage;
if (coverage === undefined) {
	failures.push("there is no rust-coverage:coverage target to enforce the Rust floor; restore it in tools/rust-coverage/project.json");
} else {
	const covered = [...(coverage.dependsOn?.[0]?.projects ?? [])].sort();
	for (const name of rustTests.filter((n) => !covered.includes(n))) {
		failures.push(`rust-coverage:coverage does not depend on ${name}:test, so that crate's coverage is outside the floor; add it to the dependsOn projects`);
	}
	for (const name of covered.filter((n) => !rustTests.includes(n))) {
		failures.push(`rust-coverage:coverage depends on ${name}:test, which is not a Rust project's test; remove it from the dependsOn projects`);
	}
	// scripts/rust-coverage.sh reads each run's profile record off the same
	// project definitions, so a run that is not cargo-test.sh leaves none.
	for (const name of rustTests.filter((n) => recordOf(n) === undefined)) {
		failures.push(`${name}:test does not run scripts/cargo-test.sh, so it leaves no coverage profile for the Rust floor; make its first test command 'bash scripts/cargo-test.sh <crate>', as every Rust project's is`);
	}
}

// The shell floor covers every project whose `test` runs a shell test step.
const shellTests = Object.keys(graph.nodes)
	.filter((name) => {
		const options = graph.nodes[name].data.targets.test?.options ?? {};
		return [...(options.commands ?? []), options.command ?? ""].some((c) => /^bash scripts\/shell-test\.sh /u.test(c));
	})
	.sort();
const shellCoverage = graph.nodes["shell-coverage"]?.data.targets.coverage;
if (shellCoverage === undefined) {
	failures.push("there is no shell-coverage:coverage target to enforce the shell floor; restore it in tools/shell-coverage/project.json");
} else {
	const covered = [...(shellCoverage.dependsOn?.[0]?.projects ?? [])].sort();
	for (const name of shellTests.filter((n) => !covered.includes(n))) {
		failures.push(`shell-coverage:coverage does not depend on ${name}:test, so its shell test steps may not have run when the floor reads them; add it to the dependsOn projects`);
	}
	for (const name of covered.filter((n) => !shellTests.includes(n))) {
		failures.push(`shell-coverage:coverage depends on ${name}:test, which runs no scripts/shell-test.sh step; remove it from the dependsOn projects`);
	}
}

// The root AGENTS.md's "Projects in the graph" record names exactly the graph.
const agents = readFileSync(join(root, "AGENTS.md"), "utf8");
const record = agents.match(/^- \*\*Projects in the graph:\*\*(?<body>[\s\S]*?)(?=\n(?:- |<!--|\n))/mu);
if (!record) {
	failures.push(`AGENTS.md has no "- **Projects in the graph:**" record of the project set; restore it`);
} else {
	const listed = new Set(
		[...record.groups.body.matchAll(/`([a-z0-9-]+)`/gu)].map((match) => match[1]),
	);
	for (const [family, noun] of [["live", "suites"], ["explore", "probes"]]) {
		const group = record.groups.body.match(new RegExp(`\`${family}-\\*\`\\s+${noun}\\s+\\(([^)]*)\\)`, "u"));
		for (const member of (group?.[1] ?? "").split(",")) {
			if (member.trim()) listed.add(`${family}-${member.trim()}`);
		}
	}
	for (const name of Object.keys(graph.nodes).filter((n) => !listed.has(n)).sort()) {
		failures.push(`AGENTS.md's "Projects in the graph" record does not name ${name}; add it`);
	}
	for (const name of [...listed].filter((n) => !(n in graph.nodes)).sort()) {
		failures.push(`AGENTS.md's "Projects in the graph" record names ${name}, which is not a project in the graph; remove it`);
	}
}

if (failures.length > 0) {
	for (const failure of failures) console.error(`check-nx-graph: ${failure}`);
	console.error(`check-nx-graph: ${failures.length} boundary violation(s) in the project graph; fix each as it says, then re-run 'node scripts/check-nx-graph.mjs'`);
	process.exit(1);
}
console.log(`check-nx-graph: ok (${Object.keys(graph.nodes).length} projects, ${edges.size} edges within their boundaries)`);
