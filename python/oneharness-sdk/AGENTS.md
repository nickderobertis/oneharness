# AGENTS (python-sdk)

Subtree rules for the typed Python SDK (`oneharness-sdk`). Root `AGENTS.md` still applies.

- `just python-sdk-check` — every gate target of this project: generated-contract
  drift, ruff, mypy, the unit suite under 95% branch-inclusive coverage, and the
  packed-artifact subprocess e2e. The Python gate runs on the oldest supported
  Python 3.9 (`python/.python-version`), from the uv workspace rooted at
  `python/` (one `python/uv.lock`).
- The typed Python client is a separate pure-Python **`oneharness-sdk`**
  distribution (imported as `oneharness_sdk`, Python 3.9+). Its checked-in
  schemas and types are generated from `sdk_schema::bundle`; runtime inputs are
  strict while output validation preserves additive fields. `scripts/python-sdk-pack.mjs`
  stamps both its package version and exact `oneharness-cli==X.Y.Z` dependency
  from the root `Cargo.toml`, keeping Rust/CLI/Node/Python releases aligned. The
  release workflow builds wheel + sdist on every release, then publishes through
  the already-registered PyPI Trusted Publisher in an environment-free,
  `id-token: write` job only after `oneharness-cli` publishes; no PyPI token is
  stored. `verify-python-sdk` installs the real release and drives `list()`
  through the packaged CLI dependency.
