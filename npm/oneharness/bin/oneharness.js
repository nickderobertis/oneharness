#!/usr/bin/env node
// Launcher for the `oneharness` command installed from the `oneharness-cli` npm
// package.
//
// Like the PyPI wheels (maturin `bindings = "bin"`), the npm distribution
// carries the *prebuilt* Rust binary — no Rust toolchain, no compile, no
// download at install time. The platform-specific binary ships inside a
// per-platform package (`@oneharness/cli-<platform>-<arch>`) declared in this
// package's `optionalDependencies`; npm installs only the one whose `os`/`cpu`
// match the host, and this shim resolves it and execs it with the caller's argv.
//
// This file is committed source (the version + optionalDependency versions are
// stamped at publish time by scripts/npm-build.mjs). The per-platform packages
// are generated per target from the release binaries — see that script.

"use strict";

const { spawnSync } = require("node:child_process");

// process.platform-process.arch -> the platform package that carries the binary.
// The keys mirror the Rust target matrix in .github/workflows/release.yml and the
// optionalDependencies in package.json; keep the three in lockstep.
const PACKAGES = {
  "linux-x64": "@oneharness/cli-linux-x64",
  "linux-arm64": "@oneharness/cli-linux-arm64",
  "darwin-x64": "@oneharness/cli-darwin-x64",
  "darwin-arm64": "@oneharness/cli-darwin-arm64",
  "win32-x64": "@oneharness/cli-win32-x64",
};

function fail(message) {
  process.stderr.write(`oneharness: ${message}\n`);
  process.exit(1);
}

function binaryPath() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PACKAGES[key];
  if (!pkg) {
    fail(
      `unsupported platform ${key}. Prebuilt binaries exist for: ` +
        `${Object.keys(PACKAGES).join(", ")}. ` +
        "Install another way instead: 'pip install oneharness-cli', " +
        "'cargo install oneharness --locked', or the install script at " +
        "https://github.com/nickderobertis/oneharness#install"
    );
  }

  const binName = process.platform === "win32" ? "oneharness.exe" : "oneharness";
  try {
    // Resolve the platform package's manifest, then locate the binary beside it.
    // Resolving package.json (rather than the binary file directly) is portable
    // across Node resolution modes and does not require an `exports` entry for a
    // non-JS asset.
    const path = require("node:path");
    const manifest = require.resolve(`${pkg}/package.json`);
    return path.join(path.dirname(manifest), "bin", binName);
  } catch (_err) {
    fail(
      `the platform package ${pkg} is not installed. This usually means npm ` +
        "skipped optional dependencies (e.g. --no-optional / --omit=optional) " +
        `or the install was for a different platform. Reinstall with optional ` +
        "dependencies enabled, or install another way: 'pip install " +
        "oneharness-cli', 'cargo install oneharness --locked', or the install " +
        "script at https://github.com/nickderobertis/oneharness#install"
    );
  }
}

const result = spawnSync(binaryPath(), process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  fail(`failed to launch the oneharness binary: ${result.error.message}`);
}

// Re-raise a terminating signal so callers observe the true cause; otherwise
// propagate the child's exit code verbatim (the JSON report is on its stdout).
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status === null ? 1 : result.status);
