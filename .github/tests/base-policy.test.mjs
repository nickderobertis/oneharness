import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { parse } from "yaml";

const POLICY = [
  ".github/macos-route.mjs",
  ".github/macos-policy.mjs",
  ".github/git-input.mjs",
  "tools/release-tooling/payload-contract/contract.mjs",
];
const step = parse(readFileSync(".github/workflows/ci.yml", "utf8")).jobs["macos-policy"].steps.find(
  (s) => s.id === "policy",
);
const manifest = (v) => `[package]\nname="oneharness"\nversion="${v}"\nedition="2021"\n`;
const lock = (v) => `version=4\n[[package]]\nname="oneharness"\nversion="${v}"\n`;
const releasePr = {
  repository: { full_name: "nickderobertis/oneharness" },
  pull_request: {
    head: { ref: "release-plz-2026-10-14T00-00-00Z", repo: { full_name: "nickderobertis/oneharness" } },
  },
};

// A repository whose base commit carries (or lacks) the real policy files and
// whose head bumps only the version, then optionally swaps in a classifier
// that calls every pull request release-only.
function repository(root, { basePolicy, tamper }) {
  const cwd = join(root, `repo-${basePolicy}-${tamper}`);
  mkdirSync(cwd);
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  const write = (path, text) => {
    mkdirSync(dirname(join(cwd, path)), { recursive: true });
    writeFileSync(join(cwd, path), text);
  };
  git("init", "-q");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.invalid");
  write("Cargo.toml", manifest("1.0.0"));
  write("Cargo.lock", lock("1.0.0"));
  if (basePolicy)
    for (const path of POLICY) {
      mkdirSync(dirname(join(cwd, path)), { recursive: true });
      copyFileSync(path, join(cwd, path));
    }
  git("add", "-A");
  git("commit", "-qm", "base");
  const base = git("rev-parse", "HEAD");
  write("Cargo.toml", manifest("1.0.1"));
  write("Cargo.lock", lock("1.0.1"));
  if (!basePolicy)
    for (const path of POLICY) {
      mkdirSync(dirname(join(cwd, path)), { recursive: true });
      copyFileSync(path, join(cwd, path));
    }
  if (tamper)
    write(
      ".github/macos-policy.mjs",
      'export function classify() { return { isReleaseOnly: true, macosRequired: false, reason: "tampered" }; }\n',
    );
  git("add", "-A");
  git("commit", "-qm", "head");
  return { cwd, base, head: git("rev-parse", "HEAD") };
}

// llmlint: ignore-block[shell_test_tiers_stay_split] The required offline real Git workflow suite runs in github-workflows:test with no installs or model calls; artifact installation remains in the separate integration tier.
test("the macos-policy step classifies a pull request with its base commit's policy, never the one it carries", () => {
  const root = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "macos-base-policy-"));
  try {
    const route = (repo, { event = "pull_request", base = repo.base } = {}) => {
      const runner = mkdtempSync(join(root, "runner-"));
      const output = join(runner, "output");
      const payload = join(runner, "event.json");
      writeFileSync(output, "");
      writeFileSync(payload, JSON.stringify(releasePr));
      const log = execFileSync("bash", ["--noprofile", "--norc", "-eo", "pipefail", "-c", step.run], {
        cwd: repo.cwd,
        encoding: "utf8",
        env: {
          ...process.env,
          EVENT_NAME: event,
          BASE_SHA: base,
          HEAD_SHA: repo.head,
          GITHUB_EVENT_PATH: payload,
          GITHUB_OUTPUT: output,
          RUNNER_TEMP: runner,
        },
      });
      const outputs = Object.fromEntries(
        readFileSync(output, "utf8").trim().split("\n").map((line) => line.split("=")),
      );
      return { ...outputs, log };
    };

    const honest = repository(root, { basePolicy: true, tamper: false });
    assert.deepEqual(
      { ...route(honest), log: undefined },
      { macos_required: "false", is_release_only: "true", log: undefined },
      "the base policy still admits a genuine version-only release pull request",
    );

    const tampered = repository(root, { basePolicy: true, tamper: true });
    // The classifier the pull request carries would skip Mac on its own …
    const carried = join(root, "carried-output");
    writeFileSync(carried, "");
    execFileSync(process.execPath, [".github/macos-route.mjs"], {
      cwd: tampered.cwd,
      env: { ...process.env, EVENT_NAME: "pull_request", GITHUB_EVENT_PATH: join(root, "missing.json"), GITHUB_OUTPUT: carried },
    });
    assert.match(readFileSync(carried, "utf8"), /macos_required=false/);
    // … but the step runs the base commit's, which refuses the policy edit as
    // not release metadata.
    const judged = route(tampered);
    assert.equal(judged.macos_required, "true");
    assert.equal(judged.is_release_only, "false");
    assert.match(judged.log, /\.github\/macos-policy\.mjs/);

    const introduced = repository(root, { basePolicy: false, tamper: false });
    const absent = route(introduced);
    assert.equal(absent.macos_required, "true", "a base without the policy requires Mac");
    assert.match(absent.log, /No base classifier at base commit/);

    for (const base of ["", "HEAD~1", "main", `${honest.base} `])
      assert.equal(route(honest, { base }).macos_required, "true", `${JSON.stringify(base)}: not a commit id`);

    // Outside pull requests the checkout is already-merged code and routes
    // itself, with no base to read.
    assert.equal(route(honest, { event: "push", base: "" }).macos_required, "false");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
// llmlint: ignore-end[shell_test_tiers_stay_split]
