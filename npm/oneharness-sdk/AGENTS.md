# AGENTS (node-sdk)

Subtree rules for the typed Node SDK (`@oneharness/sdk`). Root `AGENTS.md` still applies.

- `just sdk-check` — every gate target of this project: generated-contract drift,
  strict lint/type/test coverage (bun's `coverageThreshold = 0.95`), the build,
  and the packed-artifact subprocess e2e (`e2e`, which `test` does not include).
