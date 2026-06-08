# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `scripts/smoke.sh` and the `just smoke` / `just smoke-live` recipes: an
  end-to-end smoke of the *built* binary. The hermetic mode (list, detect,
  `--print-command`, and one mock spawn) is part of `just check` and runs in CI
  on every platform; `smoke-live` opts in to real installed harnesses and is kept
  out of the gate.
- `just lint-sh`: shellcheck over the shell scripts, wired into `just check` and
  installed in CI (and the release gate) on every platform.
- `.tool-versions` pinning `just` so a clean clone resolves the command runner
  under asdf/mise.

## [0.1.1] - 2026-06-08

### Fixed

- `run --cwd <dir>` now also sets `$PWD` to `<dir>` for each harness process,
  mirroring a shell `cd`. `current_dir` alone only `chdir()`s the child and
  leaves the inherited `$PWD` stale; Bun-based CLIs (e.g. OpenCode) trust `$PWD`
  over `getcwd()` to locate the project, so a stale value sent their tool gate to
  the wrong directory. An explicit `--env PWD=…` still wins.

## [0.1.0] - 2026-06-08

### Added

- Initial `oneharness` CLI with three commands, all emitting JSON to stdout:
  - `run` — drive selected harnesses in parallel with per-harness timeouts and a
    stable result envelope (`status`, `exit_code`, `duration_ms`, `command`,
    `stdout`, `stderr`, best-effort `text`/`text_source`).
  - `detect` — probe installed harness binaries and versions.
  - `list` — describe the supported harness registry.
- Adapters for Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, GitHub
  Copilot CLI, and Cursor, with `--all`/`--harness`/`--exclude` selection.
- Binary overrides via `--bin ID=PATH` and `ONEHARNESS_BIN_<ID>`, a
  `--print-command` dry run, and `--no-bypass` to disable permission bypass.
- `run --output-format <text|json|stream-json>` to override the per-harness
  format (drives both the emitted flag and text extraction).
- `run -- <args…>` to append verbatim arguments to each harness command.
- `run --output-dir <dir>` to write each harness's raw stdout/stderr to files
  (`<harness>.stdout`/`.stderr`), preserving a file-based transcript contract.
- Hermetic, cross-platform e2e tests driven by a mock harness fixture, and a
  Linux/macOS/Windows CI gate.
