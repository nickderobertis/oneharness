# AGENTS (mock-responder)

Subtree rules for the deterministic harness responder: library code of the
binary crate (`src/lib.rs` includes `mock_harness.rs` by path) behind `oneharness
mock-harness`, and the body of the `oneharness-mock-harness` executable.

- **It ships, so it is held like product code.** It compiles into the `oneharness`
  library, so that crate's clippy and tests are what lint and exercise it; this
  project's own target only checks its formatting. Its scripted behaviour is the
  `MOCK_*` contract `docs/testing-patterns.md` documents for released consumers.
- **It lives under `tests/` for history, not for scope**: cargo-llvm-cov leaves
  `tests/` paths out of the Rust coverage report, so the floor has never measured
  it; moving it would change what the floor measures.
