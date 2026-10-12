import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { parse } from "yaml";
import { readPlatforms } from "../../scripts/release-payload.mjs";
import { expand, workflows, resolveJobs } from "../workflow-graph.mjs";
// Non-native products require their declared release runners. The native
// builder/install/transport journey runs at the separate integration edge; this contract
// proves non-native platform declarations and the Mac commands before publishing.
test("production release graph retains every declared architecture/artifact and Darwin install proofs before uploads", () => {
  const platforms = JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        'import tomllib,json; print(json.dumps(tomllib.load(open("release-platforms.toml","rb"))["platform"]))',
      ],
      { encoding: "utf8" },
    ),
  );
  const release = parse(readFileSync(".github/workflows/release.yml", "utf8"));
  const darwin = release.jobs["build-darwin"];
  assert.match(darwin["runs-on"], /^macos-/);
  const build = darwin.steps.find(
    (s) => s.name === "Compile each Darwin target once and package its bytes",
  ).run;
  const targets = build.match(/for target in (.*); do/)[1].split(" ");
  assert.deepEqual(
    targets.sort(),
    platforms
      .filter((p) => p.target.includes("apple-darwin"))
      .map((p) => p.target)
      .sort(),
  );
  assert.match(build, /just release-bundle/);
  const installers = darwin.steps.filter((s) =>
    s.run?.includes("scripts/release-install.mjs"),
  );
  assert.ok(installers.some((s) => s.run.startsWith("node ")));
  assert.ok(installers.some((s) => s.run.startsWith("arch -x86_64 node ")));
  assert.ok(darwin.steps.some((s) => s.run?.includes("--install-rosetta")));
  for (const job of ["upload", "build-wheels", "build-npm"]) {
    const rows = expand(release.jobs[job].strategy.matrix, {});
    const shipped = rows.map((r) => r.target);
    if (job !== "upload") shipped.push(...targets);
    assert.deepEqual(
      shipped.sort(),
      platforms.map((p) => p.target).sort(),
      `${job} platform inventory`,
    );
    assert.ok(
      rows.every((r) => !r.os.startsWith("macos-")),
      `${job} must not consume a Mac runner`,
    );
  }
  for (const job of ["publish-pypi", "publish-npm"]) {
    assert.ok(release.jobs[job].needs.includes("build-darwin"));
    assert.equal(release.jobs[job]["runs-on"], "ubuntu-latest");
    assert.ok(
      release.jobs[job].steps.some((s) => s.with?.name === "darwin-release"),
    );
  }
  assert.ok(
    release.jobs.upload.steps.some(
      (s) => s.uses === "actions/attest-build-provenance@v2",
    ),
  );
  assert.ok(
    release.jobs.upload.steps.some((s) =>
      s.env?.BUNDLE?.includes("bundle-path"),
    ),
  );
  assert.ok(release.jobs.upload.needs.includes("build-darwin"));
  for (const job of ["upload", "publish-pypi", "publish-npm"])
    assert.ok(
      release.jobs[job].steps.some((s) => s.with?.name === "darwin-release"),
      `${job} must consume this run's exact Darwin artifact`,
    );
  for (const name of ["verify-pypi-windows-arm64", "verify-npm-windows-arm64"])
    assert.equal(release.jobs[name]["runs-on"], "windows-11-arm");
});
test("real release builder rejects target/tag mismatch before compiling or publishing", () => {
  for (const args of [
    ["--target", "unknown"],
    ["--target", "x86_64-apple-darwin", "--tag", "v0.0.0"],
  ]) {
    const result = spawnSync(
      process.execPath,
      ["scripts/release-bundle.mjs", ...args],
      { encoding: "utf8" },
    );
    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /Unsupported release target|Release tag differs/,
    );
  }
});

test('inventory command derives baseline/final Mac counts and all artifact declarations',()=>{
 const report=execFileSync(process.execPath,['.github/describe-policy.mjs','HEAD'],{encoding:'utf8'});
 const current=workflows();
 const baseline=Object.fromEntries(Object.keys(current).map(file=>[file,parse(execFileSync('git',['show',`HEAD:.github/workflows/${file}`],{encoding:'utf8'}))]));
 const scenario={event:'release',action:'published',branch:'main',publish:'both',manual:false,macosRequired:false,releaseOnly:false,fork:false};
 const before=resolveJobs('release.yml',scenario,baseline).filter(row=>row.macos).length;
 const after=resolveJobs('release.yml',scenario,current).filter(row=>row.macos).length;
 assert.ok(report.includes(`release.yml | release: ${before} | release: ${after}`));
 for(const p of readPlatforms()) assert.ok(report.includes(`| ${p.target} | ${p.archive} + sha256 + Sigstore bundle | platform wheel | @oneharness/cli-${p.npm} |`));
 assert.match(report,/modeled, not published/);
});
