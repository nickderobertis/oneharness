import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
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
    for (const login of ["github-actions[bot]", "attacker"])
      assert.equal(
        run({
          ...event,
          pull_request: { ...event.pull_request, user: { login } },
        }).macosRequired,
        true,
      );
    assert.equal(
      run({
        ...event,
        pull_request: {
          ...event.pull_request,
          head: { ...event.pull_request.head, ref: "release-plz-forged" },
        },
      }).macosRequired,
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
    assert.equal(run("{").macosRequired, true);
    assert.equal(run({ event_name: "push" }).macosRequired, false);
    for (const event_name of ["pull_requset","not-an-event",""]) assert.equal(run({...event,event_name}).macosRequired,true);
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
// llmlint: ignore-end[shell_test_tiers_stay_split]
