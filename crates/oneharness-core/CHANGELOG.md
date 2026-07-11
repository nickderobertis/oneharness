# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.8...oneharness-core-v0.4.0) - 2026-07-11

### Added

- configure reasoning/thinking effort per harness ([#1122](https://github.com/nickderobertis/oneharness/pull/1122))
- fan out over multiple models in parallel and fallback modes ([#1120](https://github.com/nickderobertis/oneharness/pull/1120))
- add fallback run mode (--run-mode fallback) ([#1118](https://github.com/nickderobertis/oneharness/pull/1118))
- deliver large prompts/system off the argv to harnesses ([#1115](https://github.com/nickderobertis/oneharness/pull/1115)) ([#1116](https://github.com/nickderobertis/oneharness/pull/1116))
- uniform --session handle ([#1112](https://github.com/nickderobertis/oneharness/pull/1112))
- add --system-file so a large system prompt bypasses the argv limit ([#1109](https://github.com/nickderobertis/oneharness/pull/1109))
- opt-in standardized run history + `history` view/manage verb ([#1101](https://github.com/nickderobertis/oneharness/pull/1101))
- mock/spy responder — per-tool-call deny/rewrite/stub with regex matching ([#1099](https://github.com/nickderobertis/oneharness/pull/1099))
- normalized tool-call events + streaming across the harness matrix ([#1097](https://github.com/nickderobertis/oneharness/pull/1097))
- same-prefix batch run mode (one harness over N prompts, cache-aware) ([#1088](https://github.com/nickderobertis/oneharness/pull/1088))
- surface prompt-cache token counts in normalized usage ([#1086](https://github.com/nickderobertis/oneharness/pull/1086))
- extend session continuation to all harnesses and add --fork
- [**breaking**] normalized --mode approval modes across all harnesses ([#1079](https://github.com/nickderobertis/oneharness/pull/1079))
- ONEHARNESS_&lt;FIELD&gt; environment config overrides ([#1077](https://github.com/nickderobertis/oneharness/pull/1077))
- structured output (JSON Schema) for run ([#1072](https://github.com/nickderobertis/oneharness/pull/1072))
- *(opencode)* forward session id from the plugin shim ([#1070](https://github.com/nickderobertis/oneharness/pull/1070))
- user-global hook install, runtime gate command, and live hook-enforcement e2e

### Fixed

- spawn multi-line args against Windows .cmd-shim harnesses ([#1075](https://github.com/nickderobertis/oneharness/pull/1075))
- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable

## [0.3.8](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.7...oneharness-core-v0.3.8) - 2026-07-11

### Added

- fan out over multiple models in parallel and fallback modes ([#1120](https://github.com/nickderobertis/oneharness/pull/1120))

## [0.3.7](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.6...oneharness-core-v0.3.7) - 2026-07-11

### Added

- add fallback run mode (--run-mode fallback) ([#1118](https://github.com/nickderobertis/oneharness/pull/1118))

## [0.3.6](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.5...oneharness-core-v0.3.6) - 2026-07-11

### Added

- deliver large prompts/system off the argv to harnesses ([#1115](https://github.com/nickderobertis/oneharness/pull/1115)) ([#1116](https://github.com/nickderobertis/oneharness/pull/1116))

## [0.3.5](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.4...oneharness-core-v0.3.5) - 2026-07-10

### Added

- uniform --session handle ([#1112](https://github.com/nickderobertis/oneharness/pull/1112))

## [0.3.4](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.3...oneharness-core-v0.3.4) - 2026-07-10

### Added

- add --system-file so a large system prompt bypasses the argv limit ([#1109](https://github.com/nickderobertis/oneharness/pull/1109))

## [0.3.3](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.2...oneharness-core-v0.3.3) - 2026-07-07

### Added

- opt-in standardized run history + `history` view/manage verb ([#1101](https://github.com/nickderobertis/oneharness/pull/1101))

## [0.3.2](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.1...oneharness-core-v0.3.2) - 2026-07-06

### Added

- mock/spy responder — per-tool-call deny/rewrite/stub with regex matching ([#1099](https://github.com/nickderobertis/oneharness/pull/1099))

## [0.3.1](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.3.0...oneharness-core-v0.3.1) - 2026-07-06

### Added

- normalized tool-call events + streaming across the harness matrix ([#1097](https://github.com/nickderobertis/oneharness/pull/1097))

## [0.3.0](https://github.com/nickderobertis/oneharness/compare/v0.2.38...v0.3.0) - 2026-07-01

### Added

- same-prefix batch run mode (one harness over N prompts, cache-aware) ([#1088](https://github.com/nickderobertis/oneharness/pull/1088))
- surface prompt-cache token counts in normalized usage ([#1086](https://github.com/nickderobertis/oneharness/pull/1086))
- extend session continuation to all harnesses and add --fork
- [**breaking**] normalized --mode approval modes across all harnesses ([#1079](https://github.com/nickderobertis/oneharness/pull/1079))
- ONEHARNESS_&lt;FIELD&gt; environment config overrides ([#1077](https://github.com/nickderobertis/oneharness/pull/1077))
- structured output (JSON Schema) for run ([#1072](https://github.com/nickderobertis/oneharness/pull/1072))
- *(opencode)* forward session id from the plugin shim ([#1070](https://github.com/nickderobertis/oneharness/pull/1070))
- user-global hook install, runtime gate command, and live hook-enforcement e2e

### Fixed

- spawn multi-line args against Windows .cmd-shim harnesses ([#1075](https://github.com/nickderobertis/oneharness/pull/1075))
- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable
