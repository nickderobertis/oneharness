# AGENTS (node-sdk)

Subtree rules for the typed Node SDK (`@oneharness/sdk`).

- **`e2e` stays out of `test`.** `test` is the unit suite under bun's
  `coverageThreshold = 0.95`; the packed-artifact journey is `e2e`, so a run
  that wants only the fast tier never pays for packing and installing.
- **The launcher is a workspace sibling.** The SDK depends on `oneharness-cli` as
  `workspace:*`, so the root Bun workspace links `npm/oneharness`; the pack step
  (`scripts/sdk-pack.mjs`) stamps the exact release version a consumer installs.
