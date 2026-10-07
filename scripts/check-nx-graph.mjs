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

const scratch = mkdtempSync(join(tmpdir(), "check-nx-graph-"));
let graph;
try {
	const file = join(scratch, "graph.json");
	try {
		execFileSync(process.execPath, [nxBin, "graph", `--file=${file}`], {
			cwd: root,
			stdio: ["ignore", "ignore", "pipe"],
			env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true", NX_TUI: "false" },
		});
	} catch (error) {
		die(
			`Nx could not compute the project graph:\n${String(error.stderr ?? error.message).trim()}\n  fix: run 'bash scripts/nx show projects' to see the same error, repair the project definition it names, and re-run.`,
		);
	}
	graph = JSON.parse(readFileSync(file, "utf8")).graph;
} finally {
	rmSync(scratch, { recursive: true, force: true });
}

const typeOf = new Map();
for (const [name, node] of Object.entries(graph.nodes)) {
	const types = (node.data.tags ?? []).filter((tag) => tag.startsWith("type:"));
	if (types.length !== 1) {
		failures.push(`${name} carries ${types.length} type tags (${types.join(", ") || "none"}); give it exactly one`);
	} else if (!(types[0] in allow)) {
		failures.push(`${name} is tagged ${types[0]}, which tools/workspace/boundaries.json does not declare`);
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
				`${source} (${from}) -> ${target} (${to}) is a dependency ${from} may not have; ${from} may depend only on ${allow[from].join(", ") || "nothing"} (tools/workspace/boundaries.json)`,
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
	die(`cargo metadata failed:\n${String(error.stderr ?? error.message).trim()}`);
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

if (failures.length > 0) {
	for (const failure of failures) console.error(`check-nx-graph: ${failure}`);
	console.error(`check-nx-graph: ${failures.length} boundary violation(s) in the project graph`);
	process.exit(1);
}
console.log(`check-nx-graph: ok (${Object.keys(graph.nodes).length} projects, ${edges.size} edges within their boundaries)`);
