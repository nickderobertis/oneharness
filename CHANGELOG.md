# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- Hermetic, cross-platform e2e tests driven by a mock harness fixture, and a
  Linux/macOS/Windows CI gate.
