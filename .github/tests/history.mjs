import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
// llmlint: ignore-block[shell_test_tiers_stay_split] The required offline real Git/Python workflow suite runs in github-workflows:test, measured about 10 seconds, with no installs or model calls; artifact installation remains in the separate integration tier.
export function history() {
  const cwd = mkdtempSync(
    join(
      process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(),
      "macos-history-",
    ),
  );
  const git = (...args) =>
    execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  try {
    git("init", "-q");
    git("config", "user.name", "Test");
    git("config", "user.email", "test@example.invalid");
    const manifest = (v) =>
      `[package]\nname="oneharness"\nversion="${v}"\nedition="2021"\n[dependencies]\nserde="1"\n`;
    writeFileSync(join(cwd, "Cargo.toml"), manifest("1.0.0"));
    git("add", "Cargo.toml");
    git("commit", "-qm", "base");
    git("tag", "base");
    writeFileSync(join(cwd, "Cargo.toml"), manifest("1.0.1"));
    git("add", "Cargo.toml");
    git("commit", "-qm", "version");
    git("tag", "version-only");
    writeFileSync(join(cwd, "source.rs"), "fn changed() {}");
    git("add", "source.rs");
    git("commit", "-qm", "source");
    git("tag", "source");
    git("checkout", "-q", "version-only");
    writeFileSync(
      join(cwd, "Cargo.toml"),
      manifest("1.0.1").replace('serde="1"', 'serde="2"'),
    );
    git("add", "Cargo.toml");
    git("commit", "-qm", "dependency");
    git("tag", "dependency-content");
    return {
      cwd,
      git,
      dispose: () => rmSync(cwd, { recursive: true, force: true }),
      classify(diff, fork = false, eventName = "pull_request", automation = true, action = "synchronize") {
        const event = {
          event_name: eventName,
          action,
          repository: { full_name: "nickderobertis/oneharness" },
          pull_request: {
            user: { login: automation ? "nickderobertis" : "ordinary-contributor" },
            head: {
              ref: automation ? "release-plz-2026-10-12T13-43-26Z" : "feature",
              repo: {
                full_name: fork ? "fork/repo" : "nickderobertis/oneharness",
              },
            },
          },
        };
        const path = join(cwd, "event.json");
        writeFileSync(path, JSON.stringify(event));
        return JSON.parse(
          execFileSync(
            process.execPath,
            [
              resolve(".github/macos-policy.mjs"),
              "classify",
              "--event",
              path,
              "--base",
              "base",
              "--head",
              diff,
            ],
            { cwd, encoding: "utf8" },
          ),
        );
      },
    };
  } catch (error) {
    rmSync(cwd, { recursive: true, force: true });
    throw error;
  }
}
// llmlint: ignore-end[shell_test_tiers_stay_split]
