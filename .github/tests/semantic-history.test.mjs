import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { history } from "./history.mjs";
test("engine manifests, local lock references and parsed changelogs are classified through real git histories", () => {
  const fixture = history();
  const commit = () => {
    fixture.git("add", "Cargo.toml", "Cargo.lock", "crates", "CHANGELOG.md");
    fixture.git("commit", "-qm", "metadata");
  };
  try {
    fixture.git("checkout", "-q", "base");
    mkdirSync(join(fixture.cwd, "crates/oneharness-core"), { recursive: true });
    const core = (v) =>
      `[package]\nname="oneharness-core"\nversion="${v}"\nedition="2021"\n`;
    const root = (v) =>
      `[package]\nname="oneharness"\nversion="1.0.0"\n[dependencies]\noneharness-core={path="crates/oneharness-core",version="${v}"}\n`;
    const lock = (v) =>
      `version=4\n[[package]]\nname="oneharness"\nversion="1.0.0"\ndependencies=["oneharness-core ${v}"]\n[[package]]\nname="oneharness-core"\nversion="${v}"\n`;
    writeFileSync(join(fixture.cwd, "Cargo.toml"), root("1.0.0"));
    writeFileSync(join(fixture.cwd, "Cargo.lock"), lock("1.0.0"));
    writeFileSync(
      join(fixture.cwd, "crates/oneharness-core/Cargo.toml"),
      core("1.0.0"),
    );
    writeFileSync(
      join(fixture.cwd, "CHANGELOG.md"),
      "# Changes\n## 1.0.0\nOld\n",
    );
    commit();
    fixture.git("tag", "-f", "base");
    writeFileSync(join(fixture.cwd, "Cargo.toml"), root("1.0.1"));
    writeFileSync(join(fixture.cwd, "Cargo.lock"), lock("1.0.1"));
    writeFileSync(
      join(fixture.cwd, "crates/oneharness-core/Cargo.toml"),
      core("1.0.1"),
    );
    commit();
    assert.equal(fixture.classify("HEAD").isReleaseOnly, true);
    writeFileSync(
      join(fixture.cwd, "crates/oneharness-core/Cargo.toml"),
      core("1.0.1").replace("2021", "2024"),
    );
    commit();
    assert.equal(fixture.classify("HEAD").macosRequired, true);
    fixture.git("checkout", "-q", "base");
    writeFileSync(
      join(fixture.cwd, "CHANGELOG.md"),
      "# Changes\n## 1.0.1\nNew release\n",
    );
    commit();
    assert.equal(fixture.classify("HEAD").isReleaseOnly, true);
    writeFileSync(join(fixture.cwd, "CHANGELOG.md"), "not a parsed changelog");
    commit();
    assert.equal(fixture.classify("HEAD").macosRequired, true);
  } finally {
    fixture.dispose();
  }
});
