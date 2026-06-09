# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Normalized best-effort signals on every `run` result, lifted out of each
  harness's bespoke stdout (schema-compatible additions — no `schema_version`
  bump):
  - `usage` (`{ input_tokens, output_tokens, cost_usd }`, each field nullable)
    and `usage_source`, so cross-harness cost/latency reporting is portable
    instead of per-harness. Coverage starts with Claude Code's JSON.
  - `session_id` — the continuation handle a harness exposes, surfaced for
    multi-turn consumers (not yet consumed by oneharness itself).
  - `failure_kind` (`auth`/`rate_limit`/`model_not_found`/`quota`) and
    `failure_kind_source` on a non-zero run, distinct from `status`, so callers
    can separate retryable conditions from a broken request.
- `run --system <text>` — a portable system prompt mapped to each harness that
  exposes one (Claude Code's `--append-system-prompt` to start); harnesses
  without such a flag ignore it.
- `scripts/smoke.sh` and the `just smoke` / `just smoke-live` recipes: an
  end-to-end smoke of the *built* binary. The hermetic mode (list, detect,
  `--print-command`, and one mock spawn) is part of `just check` and runs in CI
  on every platform; `smoke-live` opts in to real installed harnesses and is kept
  out of the gate.

### Fixed

- `scripts/smoke.sh` no longer silently smokes a stale artifact: it now resolves
  the *freshest* of the release/debug binaries (so the debug build `just check`
  produces wins over a leftover release), and hard-fails if the binary under test
  reports a different version than `Cargo.toml`. Previously a stale release binary
  shadowed the just-built one, masking changes from the gate.
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
