# AGENTS (python-sdk)

Subtree rules for the typed Python SDK (`oneharness-sdk`).

- **Python 3.9 is the floor, and the gate runs on it** (`python/.python-version`
  for the uv workspace rooted at `python/`): code and dev pins must keep
  resolving and passing there.
- **ty is the type checker** (`typecheck`; `[tool.ty]` in `pyproject.toml`),
  over `src`, `scripts` and `test` on Python 3.9. ty has no strict switch, so
  `all = "error"` turns on every rule, the off-by-default soundness ones
  included; none is off. ty is pre-1.0 and a pin bump can add rules: fix what
  they find, and record any rule turned off here with its reason. A `cast` or
  `ty: ignore` carries a `# cast:`/reason comment beside it; prefer the
  narrowing (`assert isinstance`, `is not None`) that makes one unnecessary.
- **What ty does not check that mypy's `strict` + `warn_unreachable` did**
  (the configuration it replaced). Covered: missing type arguments
  (`missing-type-argument`), untyped decorators
  (`dynamic-function-decorator-return`), redundant casts, unused ignores,
  returning `Any` (`unsound-return-statement`, plus `unsound-assignment`),
  bytes promotion (never applied), unannotated bodies (always checked), unknown
  config keys (refused outright). No counterpart: `disallow_untyped_defs`,
  `disallow_incomplete_defs` and `disallow_untyped_calls` (ty infers an
  unannotated def rather than refusing it); `disallow_subclassing_any`;
  `no_implicit_reexport` (ty applies it to stubs only); `strict_equality`;
  `extra_checks`; and `warn_unreachable`'s statement report — ty flags the
  always-true/false condition that strands a branch (`redundant-condition`,
  `redundant-condition-strict`), not code after a `return` or `raise`.
- `coverage[toml]` is what lets coverage read `pyproject.toml` on 3.9 (tomli);
  it used to arrive only through mypy.
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
