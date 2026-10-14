import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";

const cli = resolve(".github/macos-policy.mjs");
// llmlint: ignore-block[shell_test_tiers_stay_split] The required offline real Git/Python workflow suite runs in github-workflows:test, measured about 10 seconds, with no installs or model calls; artifact installation remains in the separate integration tier.
test("real git histories classify automation versions and fail closed on forged/mixed/unreadable PRs", () => {
  const cwd = mkdtempSync(
    join(
      process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(),
      "macos-classifier-",
    ),
  );
  const git = (...args) =>
    execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  const event = {
    event_name: "pull_request",
    repository: { full_name: "nickderobertis/oneharness" },
    pull_request: {
      user: { login: "nickderobertis" },
      head: {
        ref: "release-plz-2026-10-12T13-43-26Z",
        repo: { full_name: "nickderobertis/oneharness" },
      },
    },
  };
  const run = (data, head = "HEAD") => {
    writeFileSync(
      join(cwd, "event.json"),
      typeof data === "string" ? data : JSON.stringify(data),
    );
    return JSON.parse(
      execFileSync(
        process.execPath,
        [
          cli,
          "classify",
          "--event",
          join(cwd, "event.json"),
          "--base",
          "base",
          "--head",
          head,
        ],
        { cwd, encoding: "utf8" },
      ),
    );
  };
  try {
    git("init", "-q");
    git("config", "user.email", "test@example.invalid");
    git("config", "user.name", "Test");
    writeFileSync(
      join(cwd, "Cargo.toml"),
      '[package]\nname="oneharness"\nversion="1.0.0"\nedition="2021"\n',
    );
    writeFileSync(
      join(cwd, "Cargo.lock"),
      'version=4\n[[package]]\nname="oneharness"\nversion="1.0.0"\n[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n',
    );
    git("add", "Cargo.toml", "Cargo.lock");
    git("commit", "-qm", "base");
    git("tag", "base");
    writeFileSync(
      join(cwd, "Cargo.toml"),
      '[package]\nname="oneharness"\nversion="1.0.1"\nedition="2021"\n',
    );
    writeFileSync(
      join(cwd, "Cargo.lock"),
      'version=4\n[[package]]\nname="oneharness"\nversion="1.0.1"\n[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n',
    );
    git("add", "Cargo.toml", "Cargo.lock");
    git("commit", "-qm", "version");
    assert.equal(run(event).isReleaseOnly, true);
    assert.equal(
      run({ ...event, event_name: ["pull_request"] }).macosRequired,
      true,
      "malformed event type must require Mac",
    );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          head: {
            ...event.pull_request.head,
            ref: [event.pull_request.head.ref],
          },
        },
      }).macosRequired,
      true,
      "malformed branch must require Mac",
    );
    for (const login of ["github-actions[bot]", "attacker", "ordinary-contributor"])
      assert.equal(
        run({
          ...event,
          pull_request: { ...event.pull_request, user: { login } },
        }).isReleaseOnly,
        true,
      );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          user: { login: "github-actions[bot]" },
          head: { ...event.pull_request.head, ref: "feature" },
        },
      }).macosRequired,
      true,
      "bot identity alone cannot authorize Mac omission",
    );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          head: { ...event.pull_request.head, ref: "release-plz-custom" },
        },
      }).isReleaseOnly,
      true,
    );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          head: {
            ...event.pull_request.head,
            repo: { full_name: "fork/repo" },
          },
        },
      }).macosRequired,
      true,
    );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          title: "chore: release",
          head: { ...event.pull_request.head, ref: "feature" },
        },
      }).macosRequired,
      true,
    );
    assert.equal(run(event, "missing-ref").macosRequired, true);
    // Each expression names the release-only commit HEAD does, so only the
    // ref/SHA boundary itself can route these to Mac.
    for (const expression of ["HEAD~0", "HEAD^{commit}", "HEAD@{0}", "base..HEAD"]) {
      const refused = run(event, expression);
      assert.equal(refused.macosRequired, true, `${expression}: revision expression fails closed`);
      assert.match(refused.reason, /revision expression/, `${expression}: reason says why`);
    }
    assert.equal(run("{").macosRequired, true);
    assert.equal(run({ event_name: "push" }).macosRequired, false);
    for (const event_name of ["pull_requset", "not-an-event", ""]) {
      const unknown = run({ ...event, event_name });
      assert.equal(unknown.macosRequired, true, `${event_name}: unknown event fails closed`);
      assert.ok(unknown.reason.includes(JSON.stringify(event_name)), `${event_name}: reason names the event`);
    }
    // Every known non-PR automatic event stays Mac-free, whatever it carries.
    for (const event_name of ["push", "release", "workflow_run", "repository_dispatch", "schedule", "workflow_dispatch"]) {
      const known = run({ ...event, event_name });
      assert.deepEqual([known.isReleaseOnly, known.macosRequired], [false, false], `${event_name}: no Mac`);
    }
    // A registry dependency sharing the repository's own package name is told
    // apart by its source: bumping it is a dependency change, not a release.
    const withRegistryTwin = (version) =>
      'version=4\n[[package]]\nname="oneharness"\nversion="1.0.1"\n[[package]]\nname="oneharness-core"\n' +
      `version="${version}"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n` +
      '[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n';
    writeFileSync(join(cwd, "Cargo.lock"), withRegistryTwin("0.9.0"));
    git("add", "Cargo.lock");
    git("commit", "-qm", "registry twin baseline");
    git("tag", "twin-base");
    writeFileSync(join(cwd, "Cargo.lock"), withRegistryTwin("0.9.1"));
    git("add", "Cargo.lock");
    git("commit", "-qm", "registry twin bump");
    const twinRun = (head) => {
      writeFileSync(join(cwd, "event.json"), JSON.stringify(event));
      return JSON.parse(execFileSync(process.execPath, [cli, "classify", "--event", join(cwd, "event.json"), "--base", "twin-base", "--head", head], { cwd, encoding: "utf8" }));
    };
    const twin = twinRun("HEAD");
    assert.deepEqual([twin.isReleaseOnly, twin.macosRequired], [false, true], "same-named registry dependency bump requires Mac");
    git("checkout", "-q", "twin-base", "--", "Cargo.lock");
    git("commit", "-qm", "restore registry twin");
    writeFileSync(
      join(cwd, "Cargo.lock"),
      'version=4\n[[package]]\nname="oneharness"\nversion="1.0.1"\n[[package]]\nname="serde"\nversion="2.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n',
    );
    git("add", "Cargo.lock");
    git("commit", "-qm", "registry dependency");
    assert.equal(run(event).macosRequired, true);
    git("checkout", "HEAD~1", "--", "Cargo.lock");
    writeFileSync(
      join(cwd, "Cargo.toml"),
      '[package]\nname="oneharness"\nversion="1.0.1"\nedition="2024"\n',
    );
    git("add", "Cargo.toml", "Cargo.lock");
    git("commit", "-qm", "dependency content");
    assert.equal(run(event).macosRequired, true);
    git("checkout", "HEAD~1", "--", "Cargo.toml");
    writeFileSync(join(cwd, "source.rs"), "fn changed() {}");
    git("add", "source.rs", "Cargo.toml");
    git("commit", "-qm", "source");
    assert.equal(run(event).macosRequired, true);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
// Cargo.lock dependency references are resolved against package identities:
// only one proven to name this repository's source-less package is a version
// edit. A same-named registry package is someone else's release.
test("Cargo.lock references to same-named registry packages are dependency changes, not release versions", () => {
  const cwd = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "macos-lockrefs-"));
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  const registry = 'source = "registry+https://github.com/rust-lang/crates.io-index"';
  const lock = ({ local, consumerRef, ownRef = `oneharness-core ${local}` }) => [
    "version = 4",
    "",
    "[[package]]",
    'name = "consumer"',
    'version = "2.0.0"',
    registry,
    `dependencies = [${JSON.stringify(consumerRef)}]`,
    "",
    "[[package]]",
    'name = "oneharness"',
    `version = "${local}"`,
    `dependencies = ["consumer", ${JSON.stringify(ownRef)}]`,
    "",
    "[[package]]",
    'name = "oneharness-core"',
    `version = "${local}"`,
    "",
    // Unchanged on both sides: the registry's own oneharness-core at two versions.
    "[[package]]",
    'name = "oneharness-core"',
    'version = "0.9.0"',
    registry,
    "",
    "[[package]]",
    'name = "oneharness-core"',
    'version = "0.9.1"',
    registry,
    "",
  ].join("\n");
  const event = {
    event_name: "pull_request",
    repository: { full_name: "nickderobertis/oneharness" },
    pull_request: { head: { ref: "release-plz-2026-10-14T00-00-00Z", repo: { full_name: "nickderobertis/oneharness" } } },
  };
  writeFileSync(join(cwd, "event.json"), JSON.stringify(event));
  const commit = (text, tag) => {
    writeFileSync(join(cwd, "Cargo.lock"), text);
    git("add", "Cargo.lock");
    git("commit", "-qm", tag);
    git("tag", tag);
  };
  const classify = (base, head) =>
    JSON.parse(execFileSync(process.execPath, [cli, "classify", "--event", join(cwd, "event.json"), "--base", base, "--head", head], { cwd, encoding: "utf8" }));
  try {
    git("init", "-q");
    git("config", "user.email", "test@example.invalid");
    git("config", "user.name", "Test");
    commit(lock({ local: "1.0.0", consumerRef: "oneharness-core 0.9.0" }), "base");
    // The release bump alone: this repository's packages and its own references.
    commit(lock({ local: "1.0.1", consumerRef: "oneharness-core 0.9.0" }), "release");
    // The same bump plus the consumer moving between the registry versions.
    git("checkout", "-q", "base");
    commit(lock({ local: "1.0.1", consumerRef: "oneharness-core 0.9.1" }), "registry-move");
    // A reference that resolves to no package cannot be proven ours.
    git("checkout", "-q", "base");
    commit(lock({ local: "1.0.1", consumerRef: "oneharness-core 0.9.2" }), "dangling");
    // A bare name matching the local and both registry packages: ambiguous.
    git("checkout", "-q", "base");
    commit(lock({ local: "1.0.1", consumerRef: "oneharness-core 0.9.0", ownRef: "oneharness-core" }), "ambiguous");

    const release = classify("base", "release");
    assert.deepEqual([release.isReleaseOnly, release.macosRequired], [true, false], `workspace version bump stays release-only: ${release.reason}`);
    for (const head of ["registry-move", "dangling", "ambiguous"]) {
      const result = classify("base", head);
      assert.deepEqual([result.isReleaseOnly, result.macosRequired], [false, true], `${head}: ${result.reason}`);
    }
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
// Cargo.toml's `oneharness-core` dependency loses its version only when it is
// proved to be this repository's own crate by its workspace path; a same-named
// dependency resolved from a registry is someone else's release.
test("a same-named registry oneharness-core dependency bump in Cargo.toml requires Mac; the workspace-path bump does not", () => {
  const cwd = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "macos-manifest-dep-"));
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  const root = (own, engine) =>
    `[workspace]\nmembers = [".", "crates/oneharness-core"]\n\n[package]\nname = "oneharness"\nversion = "${own}"\nedition = "2021"\n\n[dependencies]\noneharness-core = ${engine}\n`;
  const core = (own) => `[package]\nname = "oneharness-core"\nversion = "${own}"\nedition = "2021"\n`;
  const event = {
    event_name: "pull_request",
    repository: { full_name: "nickderobertis/oneharness" },
    pull_request: { head: { ref: "release-plz-2026-10-14T00-00-00Z", repo: { full_name: "nickderobertis/oneharness" } } },
  };
  writeFileSync(join(cwd, "event.json"), JSON.stringify(event));
  const commit = (own, engine, tag) => {
    writeFileSync(join(cwd, "Cargo.toml"), root(own, engine));
    mkdirSync(join(cwd, "crates/oneharness-core"), { recursive: true });
    writeFileSync(join(cwd, "crates/oneharness-core/Cargo.toml"), core(own));
    git("add", "-A");
    git("commit", "-qm", tag);
    git("tag", tag);
  };
  const classify = (base, head) =>
    JSON.parse(execFileSync(process.execPath, [cli, "classify", "--event", join(cwd, "event.json"), "--base", base, "--head", head], { cwd, encoding: "utf8" }));
  try {
    git("init", "-q");
    git("config", "user.email", "test@example.invalid");
    git("config", "user.name", "Test");
    const workspace = (v) => `{ path = "crates/oneharness-core", version = "${v}" }`;
    const registry = (v) => `{ version = "${v}" }`;
    commit("1.0.0", workspace("1.0.0"), "path-base");
    commit("1.0.1", workspace("1.0.1"), "path-bump");
    const genuine = classify("path-base", "path-bump");
    assert.deepEqual([genuine.isReleaseOnly, genuine.macosRequired], [true, false], `workspace-path bump: ${genuine.reason}`);

    git("checkout", "-q", "--orphan", "registry");
    commit("1.0.0", registry("0.9.0"), "registry-base");
    // The release bump of this repository's packages, plus the registry crate
    // of the same name moving 0.9.0 -> 0.9.1.
    commit("1.0.1", registry("0.9.1"), "registry-bump");
    const twin = classify("registry-base", "registry-bump");
    assert.deepEqual([twin.isReleaseOnly, twin.macosRequired], [false, true], `registry dependency bump: ${twin.reason}`);
    // A path that is not this repository's crate proves nothing either.
    git("checkout", "-q", "--orphan", "elsewhere");
    const vendored = (v) => `{ path = "vendor/oneharness-core", version = "${v}" }`;
    commit("1.0.0", vendored("0.9.0"), "vendored-base");
    commit("1.0.1", vendored("0.9.1"), "vendored-bump");
    assert.equal(classify("vendored-base", "vendored-bump").macosRequired, true, "a foreign path dependency bump requires Mac");
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
// llmlint: ignore-end[shell_test_tiers_stay_split]
