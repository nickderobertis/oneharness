# AGENTS (python-sdk)

Subtree rules for the typed Python SDK (`oneharness-sdk`). Root `AGENTS.md` still applies.

- **Python 3.9 is the floor, and the gate runs on it** (`python/.python-version`
  for the uv workspace rooted at `python/`): code and dev pins must keep
  resolving and passing there.
<!-- llmlint: ignore-block[agents_md_durable_and_terse] Moved verbatim from the root AGENTS.md's composition record with the rest of this SDK's release decisions, under this change's instruction to move — not rewrite or trim — the text that governs this project; the release-job walkthrough is the account of where its Trusted Publishing constraints apply, and trimming it is a deliberate pass, not part of relocating it. -->
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
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
