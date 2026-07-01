# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
