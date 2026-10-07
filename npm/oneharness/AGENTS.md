# AGENTS (npm-launcher)

Subtree rules for the `oneharness-cli` npm launcher package. Root `AGENTS.md` still applies.

<!-- llmlint: ignore-block[agents_md_durable_and_terse] Moved verbatim from the root AGENTS.md under this change's instruction to move — not rewrite or trim — the text that governs this project (standalone trimming of AGENTS.md is outside its scope); only references to where its checks now run were updated. Its account of the release jobs is where the constraints it states apply; a durability pass over it is a change of its own. -->
- **npm packages** (the direct analogue of the PyPI wheels). The npm
  distribution is **`oneharness-cli`** too (same bare-name reasoning), and the
  command it installs is still `oneharness`.
  `npm/oneharness/` is the committed **launcher** package: its `bin/oneharness.js`
  shim resolves and execs the prebuilt binary, which is carried in a per-platform
  package `@oneharness/cli-<platform>-<arch>` declared as an **optional
  dependency** (with `os`/`cpu` set) so npm installs only the one matching the
  host — the same "carry the native binary, no compile" pattern as
  esbuild/@biomejs and the exact npm mirror of maturin's per-platform wheels.
  `scripts/npm-build.mjs` assembles both shapes: `platform` wraps a target's
  binary in its package; `launcher` stamps the version into the launcher's own
  version *and* every optionalDependency (so they stay in lockstep). The version
  comes from `Cargo.toml` by default (release-plz stays the single version driver,
  like the wheels' `dynamic` version) — never hand-set it in a committed
  `package.json` (the committed versions are the `0.0.0-managed` placeholder,
  replaced at publish). The platform set is declared ONCE, in
  `release-platforms.toml`; adding a platform starts there, and
  `scripts/check-release-targets.sh` (in `release-tooling`'s `test`) holds every matrix
  and npm list to it and names each one still missing it.
  `release.yml`'s `build-npm` job runs on every release (packaging-break alarm,
  like `build-wheels`); `publish-npm` publishes the platform packages first then
  the launcher, authenticating with an **npm token** (the `NPM_TOKEN` secret — an
  automation/granular-access token with publish rights to `oneharness-cli` and the
  `@oneharness` scope, wired through `NODE_AUTH_TOKEN`), and stays dormant until
  the `NPM_PUBLISH` repo variable is `true`; `verify-npm` then proves the
  published version is `npm install -g`-able. (Token, not Trusted Publishing — a
  deliberate choice, unlike PyPI's keyless OIDC.) The launcher's resolve-and-exec
  logic is drift-alarmed hermetically by `scripts/npm-e2e.sh` (assemble the host
  package from the built binary, stage it under the launcher exactly as npm's
  optional-dependency resolution would, run the shim end to end), which this
  project's `test` runs through `scripts/smoke.sh --npm` whenever Node is present
  (Node-gated like an external tool — GitHub runners ship Node, a node-less clone
  skips with a notice). `just npm-e2e` runs it standalone. The launcher is a
  member of the root Bun workspace, so an install links whatever
  `@oneharness/cli-*` it already resolved into `npm/oneharness/node_modules`;
  `npm-build.mjs launcher` copies everything but that directory.
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
