# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.222](https://github.com/nickderobertis/oneharness/compare/v0.2.221...v0.2.222) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.221](https://github.com/nickderobertis/oneharness/compare/v0.2.220...v0.2.221) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.220](https://github.com/nickderobertis/oneharness/compare/v0.2.219...v0.2.220) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.219](https://github.com/nickderobertis/oneharness/compare/v0.2.218...v0.2.219) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.218](https://github.com/nickderobertis/oneharness/compare/v0.2.217...v0.2.218) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.217](https://github.com/nickderobertis/oneharness/compare/v0.2.216...v0.2.217) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.216](https://github.com/nickderobertis/oneharness/compare/v0.2.215...v0.2.216) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.215](https://github.com/nickderobertis/oneharness/compare/v0.2.214...v0.2.215) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.214](https://github.com/nickderobertis/oneharness/compare/v0.2.213...v0.2.214) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.213](https://github.com/nickderobertis/oneharness/compare/v0.2.212...v0.2.213) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.212](https://github.com/nickderobertis/oneharness/compare/v0.2.211...v0.2.212) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.211](https://github.com/nickderobertis/oneharness/compare/v0.2.210...v0.2.211) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.210](https://github.com/nickderobertis/oneharness/compare/v0.2.209...v0.2.210) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.209](https://github.com/nickderobertis/oneharness/compare/v0.2.208...v0.2.209) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.208](https://github.com/nickderobertis/oneharness/compare/v0.2.207...v0.2.208) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.207](https://github.com/nickderobertis/oneharness/compare/v0.2.206...v0.2.207) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.206](https://github.com/nickderobertis/oneharness/compare/v0.2.205...v0.2.206) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.205](https://github.com/nickderobertis/oneharness/compare/v0.2.204...v0.2.205) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.204](https://github.com/nickderobertis/oneharness/compare/v0.2.203...v0.2.204) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.203](https://github.com/nickderobertis/oneharness/compare/v0.2.202...v0.2.203) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.202](https://github.com/nickderobertis/oneharness/compare/v0.2.201...v0.2.202) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.201](https://github.com/nickderobertis/oneharness/compare/v0.2.200...v0.2.201) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.200](https://github.com/nickderobertis/oneharness/compare/v0.2.199...v0.2.200) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.199](https://github.com/nickderobertis/oneharness/compare/v0.2.198...v0.2.199) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.198](https://github.com/nickderobertis/oneharness/compare/v0.2.197...v0.2.198) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.197](https://github.com/nickderobertis/oneharness/compare/v0.2.196...v0.2.197) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.196](https://github.com/nickderobertis/oneharness/compare/v0.2.195...v0.2.196) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.195](https://github.com/nickderobertis/oneharness/compare/v0.2.194...v0.2.195) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.194](https://github.com/nickderobertis/oneharness/compare/v0.2.193...v0.2.194) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.193](https://github.com/nickderobertis/oneharness/compare/v0.2.192...v0.2.193) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.192](https://github.com/nickderobertis/oneharness/compare/v0.2.191...v0.2.192) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.191](https://github.com/nickderobertis/oneharness/compare/v0.2.190...v0.2.191) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.190](https://github.com/nickderobertis/oneharness/compare/v0.2.189...v0.2.190) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.189](https://github.com/nickderobertis/oneharness/compare/v0.2.188...v0.2.189) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.188](https://github.com/nickderobertis/oneharness/compare/v0.2.187...v0.2.188) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.187](https://github.com/nickderobertis/oneharness/compare/v0.2.186...v0.2.187) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.186](https://github.com/nickderobertis/oneharness/compare/v0.2.185...v0.2.186) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.185](https://github.com/nickderobertis/oneharness/compare/v0.2.184...v0.2.185) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.184](https://github.com/nickderobertis/oneharness/compare/v0.2.183...v0.2.184) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.183](https://github.com/nickderobertis/oneharness/compare/v0.2.182...v0.2.183) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.182](https://github.com/nickderobertis/oneharness/compare/v0.2.181...v0.2.182) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.181](https://github.com/nickderobertis/oneharness/compare/v0.2.180...v0.2.181) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.180](https://github.com/nickderobertis/oneharness/compare/v0.2.179...v0.2.180) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.179](https://github.com/nickderobertis/oneharness/compare/v0.2.178...v0.2.179) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.178](https://github.com/nickderobertis/oneharness/compare/v0.2.177...v0.2.178) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.177](https://github.com/nickderobertis/oneharness/compare/v0.2.176...v0.2.177) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.176](https://github.com/nickderobertis/oneharness/compare/v0.2.175...v0.2.176) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.175](https://github.com/nickderobertis/oneharness/compare/v0.2.174...v0.2.175) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.174](https://github.com/nickderobertis/oneharness/compare/v0.2.173...v0.2.174) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.173](https://github.com/nickderobertis/oneharness/compare/v0.2.172...v0.2.173) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.172](https://github.com/nickderobertis/oneharness/compare/v0.2.171...v0.2.172) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.171](https://github.com/nickderobertis/oneharness/compare/v0.2.170...v0.2.171) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.170](https://github.com/nickderobertis/oneharness/compare/v0.2.169...v0.2.170) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.169](https://github.com/nickderobertis/oneharness/compare/v0.2.168...v0.2.169) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.168](https://github.com/nickderobertis/oneharness/compare/v0.2.167...v0.2.168) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.167](https://github.com/nickderobertis/oneharness/compare/v0.2.166...v0.2.167) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.166](https://github.com/nickderobertis/oneharness/compare/v0.2.165...v0.2.166) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.165](https://github.com/nickderobertis/oneharness/compare/v0.2.164...v0.2.165) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.164](https://github.com/nickderobertis/oneharness/compare/v0.2.163...v0.2.164) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.163](https://github.com/nickderobertis/oneharness/compare/v0.2.162...v0.2.163) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.162](https://github.com/nickderobertis/oneharness/compare/v0.2.161...v0.2.162) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.161](https://github.com/nickderobertis/oneharness/compare/v0.2.160...v0.2.161) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.160](https://github.com/nickderobertis/oneharness/compare/v0.2.159...v0.2.160) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.159](https://github.com/nickderobertis/oneharness/compare/v0.2.158...v0.2.159) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.158](https://github.com/nickderobertis/oneharness/compare/v0.2.157...v0.2.158) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.157](https://github.com/nickderobertis/oneharness/compare/v0.2.156...v0.2.157) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.156](https://github.com/nickderobertis/oneharness/compare/v0.2.155...v0.2.156) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.155](https://github.com/nickderobertis/oneharness/compare/v0.2.154...v0.2.155) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.154](https://github.com/nickderobertis/oneharness/compare/v0.2.153...v0.2.154) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.153](https://github.com/nickderobertis/oneharness/compare/v0.2.152...v0.2.153) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.152](https://github.com/nickderobertis/oneharness/compare/v0.2.151...v0.2.152) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.151](https://github.com/nickderobertis/oneharness/compare/v0.2.150...v0.2.151) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.150](https://github.com/nickderobertis/oneharness/compare/v0.2.149...v0.2.150) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.149](https://github.com/nickderobertis/oneharness/compare/v0.2.148...v0.2.149) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.148](https://github.com/nickderobertis/oneharness/compare/v0.2.147...v0.2.148) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.147](https://github.com/nickderobertis/oneharness/compare/v0.2.146...v0.2.147) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.146](https://github.com/nickderobertis/oneharness/compare/v0.2.145...v0.2.146) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.145](https://github.com/nickderobertis/oneharness/compare/v0.2.144...v0.2.145) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.144](https://github.com/nickderobertis/oneharness/compare/v0.2.143...v0.2.144) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.143](https://github.com/nickderobertis/oneharness/compare/v0.2.142...v0.2.143) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.142](https://github.com/nickderobertis/oneharness/compare/v0.2.141...v0.2.142) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.141](https://github.com/nickderobertis/oneharness/compare/v0.2.140...v0.2.141) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.140](https://github.com/nickderobertis/oneharness/compare/v0.2.139...v0.2.140) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.139](https://github.com/nickderobertis/oneharness/compare/v0.2.138...v0.2.139) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.138](https://github.com/nickderobertis/oneharness/compare/v0.2.137...v0.2.138) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.137](https://github.com/nickderobertis/oneharness/compare/v0.2.136...v0.2.137) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.136](https://github.com/nickderobertis/oneharness/compare/v0.2.135...v0.2.136) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.135](https://github.com/nickderobertis/oneharness/compare/v0.2.134...v0.2.135) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.134](https://github.com/nickderobertis/oneharness/compare/v0.2.133...v0.2.134) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.133](https://github.com/nickderobertis/oneharness/compare/v0.2.132...v0.2.133) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.132](https://github.com/nickderobertis/oneharness/compare/v0.2.131...v0.2.132) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.131](https://github.com/nickderobertis/oneharness/compare/v0.2.130...v0.2.131) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.130](https://github.com/nickderobertis/oneharness/compare/v0.2.129...v0.2.130) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.129](https://github.com/nickderobertis/oneharness/compare/v0.2.128...v0.2.129) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.128](https://github.com/nickderobertis/oneharness/compare/v0.2.127...v0.2.128) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.127](https://github.com/nickderobertis/oneharness/compare/v0.2.126...v0.2.127) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.126](https://github.com/nickderobertis/oneharness/compare/v0.2.125...v0.2.126) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.125](https://github.com/nickderobertis/oneharness/compare/v0.2.124...v0.2.125) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.124](https://github.com/nickderobertis/oneharness/compare/v0.2.123...v0.2.124) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.123](https://github.com/nickderobertis/oneharness/compare/v0.2.122...v0.2.123) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.122](https://github.com/nickderobertis/oneharness/compare/v0.2.121...v0.2.122) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.121](https://github.com/nickderobertis/oneharness/compare/v0.2.120...v0.2.121) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.120](https://github.com/nickderobertis/oneharness/compare/v0.2.119...v0.2.120) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.119](https://github.com/nickderobertis/oneharness/compare/v0.2.118...v0.2.119) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.118](https://github.com/nickderobertis/oneharness/compare/v0.2.117...v0.2.118) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.117](https://github.com/nickderobertis/oneharness/compare/v0.2.116...v0.2.117) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.116](https://github.com/nickderobertis/oneharness/compare/v0.2.115...v0.2.116) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.115](https://github.com/nickderobertis/oneharness/compare/v0.2.114...v0.2.115) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.114](https://github.com/nickderobertis/oneharness/compare/v0.2.113...v0.2.114) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.113](https://github.com/nickderobertis/oneharness/compare/v0.2.112...v0.2.113) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.112](https://github.com/nickderobertis/oneharness/compare/v0.2.111...v0.2.112) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.111](https://github.com/nickderobertis/oneharness/compare/v0.2.110...v0.2.111) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.110](https://github.com/nickderobertis/oneharness/compare/v0.2.109...v0.2.110) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.109](https://github.com/nickderobertis/oneharness/compare/v0.2.108...v0.2.109) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.108](https://github.com/nickderobertis/oneharness/compare/v0.2.107...v0.2.108) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.107](https://github.com/nickderobertis/oneharness/compare/v0.2.106...v0.2.107) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.106](https://github.com/nickderobertis/oneharness/compare/v0.2.105...v0.2.106) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.105](https://github.com/nickderobertis/oneharness/compare/v0.2.104...v0.2.105) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.104](https://github.com/nickderobertis/oneharness/compare/v0.2.103...v0.2.104) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.103](https://github.com/nickderobertis/oneharness/compare/v0.2.102...v0.2.103) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.102](https://github.com/nickderobertis/oneharness/compare/v0.2.101...v0.2.102) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.101](https://github.com/nickderobertis/oneharness/compare/v0.2.100...v0.2.101) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.100](https://github.com/nickderobertis/oneharness/compare/v0.2.99...v0.2.100) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.99](https://github.com/nickderobertis/oneharness/compare/v0.2.98...v0.2.99) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.98](https://github.com/nickderobertis/oneharness/compare/v0.2.97...v0.2.98) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.97](https://github.com/nickderobertis/oneharness/compare/v0.2.96...v0.2.97) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.96](https://github.com/nickderobertis/oneharness/compare/v0.2.95...v0.2.96) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.95](https://github.com/nickderobertis/oneharness/compare/v0.2.94...v0.2.95) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.94](https://github.com/nickderobertis/oneharness/compare/v0.2.93...v0.2.94) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.93](https://github.com/nickderobertis/oneharness/compare/v0.2.92...v0.2.93) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.92](https://github.com/nickderobertis/oneharness/compare/v0.2.91...v0.2.92) - 2026-06-15

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.91](https://github.com/nickderobertis/oneharness/compare/v0.2.90...v0.2.91) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.90](https://github.com/nickderobertis/oneharness/compare/v0.2.89...v0.2.90) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.89](https://github.com/nickderobertis/oneharness/compare/v0.2.88...v0.2.89) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.88](https://github.com/nickderobertis/oneharness/compare/v0.2.87...v0.2.88) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.87](https://github.com/nickderobertis/oneharness/compare/v0.2.86...v0.2.87) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.86](https://github.com/nickderobertis/oneharness/compare/v0.2.85...v0.2.86) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.85](https://github.com/nickderobertis/oneharness/compare/v0.2.84...v0.2.85) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.84](https://github.com/nickderobertis/oneharness/compare/v0.2.83...v0.2.84) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.83](https://github.com/nickderobertis/oneharness/compare/v0.2.82...v0.2.83) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.82](https://github.com/nickderobertis/oneharness/compare/v0.2.81...v0.2.82) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.81](https://github.com/nickderobertis/oneharness/compare/v0.2.80...v0.2.81) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.80](https://github.com/nickderobertis/oneharness/compare/v0.2.79...v0.2.80) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.79](https://github.com/nickderobertis/oneharness/compare/v0.2.78...v0.2.79) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.78](https://github.com/nickderobertis/oneharness/compare/v0.2.77...v0.2.78) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.77](https://github.com/nickderobertis/oneharness/compare/v0.2.76...v0.2.77) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.76](https://github.com/nickderobertis/oneharness/compare/v0.2.75...v0.2.76) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.75](https://github.com/nickderobertis/oneharness/compare/v0.2.74...v0.2.75) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.74](https://github.com/nickderobertis/oneharness/compare/v0.2.73...v0.2.74) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.73](https://github.com/nickderobertis/oneharness/compare/v0.2.72...v0.2.73) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.72](https://github.com/nickderobertis/oneharness/compare/v0.2.71...v0.2.72) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.71](https://github.com/nickderobertis/oneharness/compare/v0.2.70...v0.2.71) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.70](https://github.com/nickderobertis/oneharness/compare/v0.2.69...v0.2.70) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.69](https://github.com/nickderobertis/oneharness/compare/v0.2.68...v0.2.69) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.68](https://github.com/nickderobertis/oneharness/compare/v0.2.67...v0.2.68) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.67](https://github.com/nickderobertis/oneharness/compare/v0.2.66...v0.2.67) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.66](https://github.com/nickderobertis/oneharness/compare/v0.2.65...v0.2.66) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.65](https://github.com/nickderobertis/oneharness/compare/v0.2.64...v0.2.65) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.64](https://github.com/nickderobertis/oneharness/compare/v0.2.63...v0.2.64) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.63](https://github.com/nickderobertis/oneharness/compare/v0.2.62...v0.2.63) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.62](https://github.com/nickderobertis/oneharness/compare/v0.2.61...v0.2.62) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.61](https://github.com/nickderobertis/oneharness/compare/v0.2.60...v0.2.61) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.60](https://github.com/nickderobertis/oneharness/compare/v0.2.59...v0.2.60) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.59](https://github.com/nickderobertis/oneharness/compare/v0.2.58...v0.2.59) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.58](https://github.com/nickderobertis/oneharness/compare/v0.2.57...v0.2.58) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.57](https://github.com/nickderobertis/oneharness/compare/v0.2.56...v0.2.57) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.56](https://github.com/nickderobertis/oneharness/compare/v0.2.55...v0.2.56) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.55](https://github.com/nickderobertis/oneharness/compare/v0.2.54...v0.2.55) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.54](https://github.com/nickderobertis/oneharness/compare/v0.2.53...v0.2.54) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.53](https://github.com/nickderobertis/oneharness/compare/v0.2.52...v0.2.53) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.52](https://github.com/nickderobertis/oneharness/compare/v0.2.51...v0.2.52) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.51](https://github.com/nickderobertis/oneharness/compare/v0.2.50...v0.2.51) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.50](https://github.com/nickderobertis/oneharness/compare/v0.2.49...v0.2.50) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.49](https://github.com/nickderobertis/oneharness/compare/v0.2.48...v0.2.49) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.48](https://github.com/nickderobertis/oneharness/compare/v0.2.47...v0.2.48) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.47](https://github.com/nickderobertis/oneharness/compare/v0.2.46...v0.2.47) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.46](https://github.com/nickderobertis/oneharness/compare/v0.2.45...v0.2.46) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.45](https://github.com/nickderobertis/oneharness/compare/v0.2.44...v0.2.45) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.43](https://github.com/nickderobertis/oneharness/compare/v0.2.42...v0.2.43) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.42](https://github.com/nickderobertis/oneharness/compare/v0.2.41...v0.2.42) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.41](https://github.com/nickderobertis/oneharness/compare/v0.2.40...v0.2.41) - 2026-06-14

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e
- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))
- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))
- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.40](https://github.com/nickderobertis/oneharness/compare/v0.2.39...v0.2.40) - 2026-06-13

### Fixed

- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable

## [0.2.39](https://github.com/nickderobertis/oneharness/compare/v0.2.38...v0.2.39) - 2026-06-13

### Added

- user-global hook install, runtime gate command, and live hook-enforcement e2e

## [0.2.38](https://github.com/nickderobertis/oneharness/compare/v0.2.37...v0.2.38) - 2026-06-13

### Added

- unified config management with file-synced policy across harnesses ([#87](https://github.com/nickderobertis/oneharness/pull/87))

## [0.2.37](https://github.com/nickderobertis/oneharness/compare/v0.2.36...v0.2.37) - 2026-06-11

### Added

- extract OpenCode final text and inject per-harness default env ([#85](https://github.com/nickderobertis/oneharness/pull/85))

## [0.2.36](https://github.com/nickderobertis/oneharness/compare/v0.2.35...v0.2.36) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.35](https://github.com/nickderobertis/oneharness/compare/v0.2.34...v0.2.35) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.34](https://github.com/nickderobertis/oneharness/compare/v0.2.33...v0.2.34) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.33](https://github.com/nickderobertis/oneharness/compare/v0.2.32...v0.2.33) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.32](https://github.com/nickderobertis/oneharness/compare/v0.2.31...v0.2.32) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.31](https://github.com/nickderobertis/oneharness/compare/v0.2.30...v0.2.31) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.30](https://github.com/nickderobertis/oneharness/compare/v0.2.29...v0.2.30) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.29](https://github.com/nickderobertis/oneharness/compare/v0.2.28...v0.2.29) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.28](https://github.com/nickderobertis/oneharness/compare/v0.2.27...v0.2.28) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.27](https://github.com/nickderobertis/oneharness/compare/v0.2.26...v0.2.27) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.26](https://github.com/nickderobertis/oneharness/compare/v0.2.25...v0.2.26) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.25](https://github.com/nickderobertis/oneharness/compare/v0.2.24...v0.2.25) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.24](https://github.com/nickderobertis/oneharness/compare/v0.2.23...v0.2.24) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.23](https://github.com/nickderobertis/oneharness/compare/v0.2.22...v0.2.23) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.22](https://github.com/nickderobertis/oneharness/compare/v0.2.21...v0.2.22) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.21](https://github.com/nickderobertis/oneharness/compare/v0.2.20...v0.2.21) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.20](https://github.com/nickderobertis/oneharness/compare/v0.2.19...v0.2.20) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.19](https://github.com/nickderobertis/oneharness/compare/v0.2.18...v0.2.19) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.18](https://github.com/nickderobertis/oneharness/compare/v0.2.17...v0.2.18) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.17](https://github.com/nickderobertis/oneharness/compare/v0.2.16...v0.2.17) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.16](https://github.com/nickderobertis/oneharness/compare/v0.2.15...v0.2.16) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.15](https://github.com/nickderobertis/oneharness/compare/v0.2.14...v0.2.15) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.14](https://github.com/nickderobertis/oneharness/compare/v0.2.13...v0.2.14) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.13](https://github.com/nickderobertis/oneharness/compare/v0.2.12...v0.2.13) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.12](https://github.com/nickderobertis/oneharness/compare/v0.2.11...v0.2.12) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.11](https://github.com/nickderobertis/oneharness/compare/v0.2.10...v0.2.11) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.10](https://github.com/nickderobertis/oneharness/compare/v0.2.9...v0.2.10) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.9](https://github.com/nickderobertis/oneharness/compare/v0.2.8...v0.2.9) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.8](https://github.com/nickderobertis/oneharness/compare/v0.2.7...v0.2.8) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.7](https://github.com/nickderobertis/oneharness/compare/v0.2.6...v0.2.7) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.6](https://github.com/nickderobertis/oneharness/compare/v0.2.5...v0.2.6) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.5](https://github.com/nickderobertis/oneharness/compare/v0.2.4...v0.2.5) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.4](https://github.com/nickderobertis/oneharness/compare/v0.2.3...v0.2.4) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.3](https://github.com/nickderobertis/oneharness/compare/v0.2.2...v0.2.3) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.2](https://github.com/nickderobertis/oneharness/compare/v0.2.1...v0.2.2) - 2026-06-11

### Added

- *(signals)* widen usage/session/resume coverage to OpenCode and Cursor ([#9](https://github.com/nickderobertis/oneharness/pull/9))
- *(run)* --resume to continue a harness session (single-harness) ([#8](https://github.com/nickderobertis/oneharness/pull/8))
- *(run)* normalize usage/session/failure signals and add --system ([#6](https://github.com/nickderobertis/oneharness/pull/6))
- *(run)* --output-format, -- passthrough, and --output-dir ([#1](https://github.com/nickderobertis/oneharness/pull/1))
- initial oneharness CLI for cross-harness agent runs

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))
- *(smoke)* test the freshest binary and reject a stale build ([#7](https://github.com/nickderobertis/oneharness/pull/7))
- *(run)* set $PWD to match --cwd for each harness ([#3](https://github.com/nickderobertis/oneharness/pull/3))

## [0.2.1](https://github.com/nickderobertis/oneharness/compare/v0.2.0...v0.2.1) - 2026-06-11

### Fixed

- *(harness)* deliver --system to every harness and modernize codex flags ([#12](https://github.com/nickderobertis/oneharness/pull/12))

## [0.2.0] - 2026-06-09

### Added

- Normalized best-effort signals on every `run` result, lifted out of each
  harness's bespoke stdout (schema-compatible additions — no `schema_version`
  bump):
  - `usage` (`{ input_tokens, output_tokens, cost_usd }`, each field nullable)
    and `usage_source`, so cross-harness cost/latency reporting is portable
    instead of per-harness. `usage_source` distinguishes the method: `json` for a
    whole-run total in one event (Claude Code), `json:summed-steps` for per-step
    usage summed across events (OpenCode's `step_finish` JSONL). Cursor does not
    emit token usage today, so its `usage` stays null rather than fabricated.
  - `session_id` — the continuation handle a harness exposes, read from both the
    snake_case `session_id` (Claude Code, Cursor) and camelCase `sessionID`
    (OpenCode), surfaced for multi-turn consumers and consumed by `run --resume`.
  - `failure_kind` (`auth`/`rate_limit`/`model_not_found`/`quota`) and
    `failure_kind_source` on a non-zero run, distinct from `status`, so callers
    can separate retryable conditions from a broken request.
- `run --system <text>` — a portable system prompt mapped to each harness that
  exposes one (Claude Code's `--append-system-prompt` to start); harnesses
  without such a flag ignore it.
- `run --resume <session>` — continue a prior session, sending the prompt as its
  next turn, for a faithful multi-turn against the real agent (pairs with the new
  `session_id` signal). Single-harness only (a session belongs to one harness)
  and only for harnesses that support it; any other selection is a usage error,
  never a silent fresh session. Mapped per harness onto its real continuation
  flag — Claude Code `--resume`, OpenCode `--session`, Cursor `--resume` — and
  `oneharness list` reports a `supports_resume` flag for each.
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
