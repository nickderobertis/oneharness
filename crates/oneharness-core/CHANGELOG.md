# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.9.0...oneharness-core-v0.10.0) - 2026-08-16

### Added

- [**breaking**] fall through a precondition refusal and bound a control socket address ([#1256](https://github.com/nickderobertis/oneharness/pull/1256))

## [0.9.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.8.0...oneharness-core-v0.9.0) - 2026-08-15

### Added

- [**breaking**] refuse a contradictory option pair instead of editing it out ([#1255](https://github.com/nickderobertis/oneharness/pull/1255))

## [0.8.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.7.1...oneharness-core-v0.8.0) - 2026-08-14

### Fixed

- [**breaking**] bind a controlled turn's mechanism to the candidate serving it ([#1253](https://github.com/nickderobertis/oneharness/pull/1253))

## [0.7.1](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.7.0...oneharness-core-v0.7.1) - 2026-08-13

### Fixed

- bind a controllable turn to a fallback chain ([#1251](https://github.com/nickderobertis/oneharness/pull/1251))

## [0.7.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.14...oneharness-core-v0.7.0) - 2026-08-13

### Added

- [**breaking**] run a turn with no timeout unless one is asked for ([#1246](https://github.com/nickderobertis/oneharness/pull/1246))
- let a run opt out of the per-harness timeout entirely ([#1244](https://github.com/nickderobertis/oneharness/pull/1244))
- close the SDK gaps against the CLI and gate the drift ([#1241](https://github.com/nickderobertis/oneharness/pull/1241))
- carry a redirection with an out-of-band interrupt ([#1230](https://github.com/nickderobertis/oneharness/pull/1230))
- out-of-band turn control via a socket and `oneharness interrupt` ([#1225](https://github.com/nickderobertis/oneharness/pull/1225))
- cancel a run's harness tree on signal and expose telemetry on results ([#1222](https://github.com/nickderobertis/oneharness/pull/1222))
- stream config key and absent-home auth fallthrough ([#1220](https://github.com/nickderobertis/oneharness/pull/1220))
- stream a fallback chain instead of refusing it ([#1213](https://github.com/nickderobertis/oneharness/pull/1213))
- [**breaking**] omit absent usage fields from the wire and pin the v0.1 golden;… ([#1198](https://github.com/nickderobertis/oneharness/pull/1198))
- record observed tool timing for Anthropic-envelope harnesses ([#1196](https://github.com/nickderobertis/oneharness/pull/1196))
- add first-class harness variants ([#1186](https://github.com/nickderobertis/oneharness/pull/1186))
- incremental event persistence and live event-level history watch ([#1175](https://github.com/nickderobertis/oneharness/pull/1175))
- [**breaking**] add event-sourced history migration; define event-sourced histor… ([#1173](https://github.com/nickderobertis/oneharness/pull/1173))
- ship deterministic mock harness for CLI + all SDKs ([#1171](https://github.com/nickderobertis/oneharness/pull/1171))
- add resumable labeled history and SDK parity ([#1146](https://github.com/nickderobertis/oneharness/pull/1146))
- *(sdk)* export generated Zod schemas ([#1139](https://github.com/nickderobertis/oneharness/pull/1139))
- detect and classify claude-code deferred-tool dead-ends (tool_deferred)
- add init subcommand to scaffold a starter config ([#1129](https://github.com/nickderobertis/oneharness/pull/1129))
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

- *(core)* release process-aware readiness API ([#1239](https://github.com/nickderobertis/oneharness/pull/1239))
- record history for a run that failed before it could be measured ([#1217](https://github.com/nickderobertis/oneharness/pull/1217))
- classify a zero-work 429 as quota so a chain falls through ([#1215](https://github.com/nickderobertis/oneharness/pull/1215))
- *(fallback)* fall through a zero-work session-limit rejection ([#1211](https://github.com/nickderobertis/oneharness/pull/1211))
- *(fallback)* classify codex usage limits as quota ([#1208](https://github.com/nickderobertis/oneharness/pull/1208))
- *(fallback)* classify Claude subscription limits as quota ([#1204](https://github.com/nickderobertis/oneharness/pull/1204))
- *(usage)* wait for codex's rate-limit answer instead of closing its stdin ([#1202](https://github.com/nickderobertis/oneharness/pull/1202))
- close harness variant CI regressions ([#1191](https://github.com/nickderobertis/oneharness/pull/1191))
- emit complete v1.0 telemetry for boundaried file_change items; loc… ([#1183](https://github.com/nickderobertis/oneharness/pull/1183))
- *(history)* address llmlint findings in event-sourced history ([#1177](https://github.com/nickderobertis/oneharness/pull/1177))
- *(sdk)* accept unavailable history timing ([#1164](https://github.com/nickderobertis/oneharness/pull/1164))
- degrade unsupported history telemetry gracefully ([#1161](https://github.com/nickderobertis/oneharness/pull/1161))
- *(history)* harden macOS timing test; add e2e; rename fixture ([#1157](https://github.com/nickderobertis/oneharness/pull/1157))
- terminate timed-out process trees and preserve telemetry ([#1147](https://github.com/nickderobertis/oneharness/pull/1147))
- harden SDK release lifecycle ([#1137](https://github.com/nickderobertis/oneharness/pull/1137))
- allow --session in fallback run mode ([#1127](https://github.com/nickderobertis/oneharness/pull/1127))
- *(cursor)* deliver reasoning as a model-id tier suffix ([#1125](https://github.com/nickderobertis/oneharness/pull/1125))
- spawn multi-line args against Windows .cmd-shim harnesses ([#1075](https://github.com/nickderobertis/oneharness/pull/1075))
- *(hooks)* honor Claude hook timeout and make the Goose manifest description caller-brandable

## [0.6.14](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.13...oneharness-core-v0.6.14) - 2026-08-13

### Added

- let a run opt out of the per-harness timeout entirely ([#1244](https://github.com/nickderobertis/oneharness/pull/1244))

## [0.6.13](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.12...oneharness-core-v0.6.13) - 2026-08-12

### Added

- close the SDK gaps against the CLI and gate the drift ([#1241](https://github.com/nickderobertis/oneharness/pull/1241))

## [0.6.12](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.11...oneharness-core-v0.6.12) - 2026-08-11

### Fixed

- *(core)* release process-aware readiness API ([#1239](https://github.com/nickderobertis/oneharness/pull/1239))

## [0.6.11](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.10...oneharness-core-v0.6.11) - 2026-08-10

### Added

- carry a redirection with an out-of-band interrupt ([#1230](https://github.com/nickderobertis/oneharness/pull/1230))

## [0.6.10](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.9...oneharness-core-v0.6.10) - 2026-08-09

### Added

- out-of-band turn control via a socket and `oneharness interrupt` ([#1225](https://github.com/nickderobertis/oneharness/pull/1225))

## [0.6.9](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.8...oneharness-core-v0.6.9) - 2026-08-08

### Added

- cancel a run's harness tree on signal and expose telemetry on results ([#1222](https://github.com/nickderobertis/oneharness/pull/1222))

## [0.6.8](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.7...oneharness-core-v0.6.8) - 2026-08-08

### Added

- stream config key and absent-home auth fallthrough ([#1220](https://github.com/nickderobertis/oneharness/pull/1220))

## [0.6.7](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.6...oneharness-core-v0.6.7) - 2026-08-05

### Fixed

- record history for a run that failed before it could be measured ([#1217](https://github.com/nickderobertis/oneharness/pull/1217))

## [0.6.6](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.5...oneharness-core-v0.6.6) - 2026-08-03

### Fixed

- classify a zero-work 429 as quota so a chain falls through ([#1215](https://github.com/nickderobertis/oneharness/pull/1215))

## [0.6.5](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.4...oneharness-core-v0.6.5) - 2026-08-02

### Added

- stream a fallback chain instead of refusing it ([#1213](https://github.com/nickderobertis/oneharness/pull/1213))

## [0.6.4](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.3...oneharness-core-v0.6.4) - 2026-08-01

### Fixed

- *(fallback)* fall through a zero-work session-limit rejection ([#1211](https://github.com/nickderobertis/oneharness/pull/1211))

## [0.6.3](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.2...oneharness-core-v0.6.3) - 2026-07-31

### Fixed

- *(fallback)* classify codex usage limits as quota ([#1208](https://github.com/nickderobertis/oneharness/pull/1208))

## [0.6.2](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.1...oneharness-core-v0.6.2) - 2026-07-31

### Fixed

- *(fallback)* classify Claude subscription limits as quota ([#1204](https://github.com/nickderobertis/oneharness/pull/1204))

## [0.6.1](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.6.0...oneharness-core-v0.6.1) - 2026-07-31

### Fixed

- *(usage)* wait for codex's rate-limit answer instead of closing its stdin ([#1202](https://github.com/nickderobertis/oneharness/pull/1202))

## [0.6.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.6...oneharness-core-v0.6.0) - 2026-07-30

### Added

- [**breaking**] omit absent usage fields from the wire and pin the v0.1 golden;… ([#1198](https://github.com/nickderobertis/oneharness/pull/1198))

## [0.5.6](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.5...oneharness-core-v0.5.6) - 2026-07-26

### Added

- record observed tool timing for Anthropic-envelope harnesses ([#1196](https://github.com/nickderobertis/oneharness/pull/1196))

## [0.5.5](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.4...oneharness-core-v0.5.5) - 2026-07-26

### Fixed

- close harness variant CI regressions ([#1191](https://github.com/nickderobertis/oneharness/pull/1191))

## [0.5.4](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.3...oneharness-core-v0.5.4) - 2026-07-25

### Added

- add first-class harness variants ([#1186](https://github.com/nickderobertis/oneharness/pull/1186))

## [0.5.3](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.2...oneharness-core-v0.5.3) - 2026-07-24

### Fixed

- emit complete v1.0 telemetry for boundaried file_change items; loc… ([#1183](https://github.com/nickderobertis/oneharness/pull/1183))

## [0.5.2](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.1...oneharness-core-v0.5.2) - 2026-07-22

### Fixed

- *(history)* address llmlint findings in event-sourced history ([#1177](https://github.com/nickderobertis/oneharness/pull/1177))

## [0.5.1](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.5.0...oneharness-core-v0.5.1) - 2026-07-22

### Added

- incremental event persistence and live event-level history watch ([#1175](https://github.com/nickderobertis/oneharness/pull/1175))

## [0.5.0](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.12...oneharness-core-v0.5.0) - 2026-07-22

### Added

- [**breaking**] add event-sourced history migration; define event-sourced histor… ([#1173](https://github.com/nickderobertis/oneharness/pull/1173))

## [0.4.12](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.11...oneharness-core-v0.4.12) - 2026-07-21

### Added

- ship deterministic mock harness for CLI + all SDKs ([#1171](https://github.com/nickderobertis/oneharness/pull/1171))

## [0.4.11](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.10...oneharness-core-v0.4.11) - 2026-07-20

### Fixed

- *(sdk)* accept unavailable history timing ([#1164](https://github.com/nickderobertis/oneharness/pull/1164))

## [0.4.10](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.9...oneharness-core-v0.4.10) - 2026-07-20

### Fixed

- degrade unsupported history telemetry gracefully ([#1161](https://github.com/nickderobertis/oneharness/pull/1161))

## [0.4.9](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.8...oneharness-core-v0.4.9) - 2026-07-20

### Fixed

- *(history)* harden macOS timing test; add e2e; rename fixture ([#1157](https://github.com/nickderobertis/oneharness/pull/1157))

## [0.4.8](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.7...oneharness-core-v0.4.8) - 2026-07-17

### Fixed

- capture Codex and Qwen session ids so `--session` resumes the prior conversation instead of silently starting a cold one; Codex now defaults to JSON, session capability is derived from each harness's session-bearing formats, and session requests automatically select a compatible format ([#1150](https://github.com/nickderobertis/oneharness/pull/1150))

### Changed

- migration: Codex and Qwen runs that explicitly combine `--session` with `--output-format text` now fail with a usage error; remove the explicit text format to let oneharness select a session-capable format ([#1150](https://github.com/nickderobertis/oneharness/pull/1150))

## [0.4.7](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.6...oneharness-core-v0.4.7) - 2026-07-17

### Added

- add resumable labeled history and SDK parity ([#1146](https://github.com/nickderobertis/oneharness/pull/1146))

## [0.4.6](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.5...oneharness-core-v0.4.6) - 2026-07-17

### Fixed

- terminate timed-out process trees and preserve telemetry ([#1147](https://github.com/nickderobertis/oneharness/pull/1147))

## [0.4.5](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.4...oneharness-core-v0.4.5) - 2026-07-16

### Added

- *(sdk)* export generated Zod schemas ([#1139](https://github.com/nickderobertis/oneharness/pull/1139))

### Fixed

- harden SDK release lifecycle ([#1137](https://github.com/nickderobertis/oneharness/pull/1137))

## [0.4.4](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.3...oneharness-core-v0.4.4) - 2026-07-13

### Added

- detect and classify claude-code deferred-tool dead-ends (tool_deferred)

## [0.4.3](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.2...oneharness-core-v0.4.3) - 2026-07-12

### Added

- add init subcommand to scaffold a starter config ([#1129](https://github.com/nickderobertis/oneharness/pull/1129))

## [0.4.2](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.1...oneharness-core-v0.4.2) - 2026-07-12

### Fixed

- allow --session in fallback run mode ([#1127](https://github.com/nickderobertis/oneharness/pull/1127))

## [0.4.1](https://github.com/nickderobertis/oneharness/compare/oneharness-core-v0.4.0...oneharness-core-v0.4.1) - 2026-07-11

### Fixed

- *(cursor)* deliver reasoning as a model-id tier suffix ([#1125](https://github.com/nickderobertis/oneharness/pull/1125))

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
