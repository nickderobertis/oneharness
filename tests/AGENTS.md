# AGENTS (oneharness-integration)

Subtree rules for the binary crate's in-process integration tier (`tests/*.rs`,
the `oneharness-integration` project): suites that drive the library API — and
the mock harness as a subprocess — without the CLI binary. This directory also
holds the shipped mock responder (`support/mock_harness.rs`, compiled into the
`oneharness` library) and the captured harness output several suites read
(`fixtures/`). Root `AGENTS.md` still applies.

- **Hermetic by construction.** Tests never call a real harness CLI, the network,
  or an authenticated session. The subprocess path is exercised through the
  `oneharness-mock-harness` fixture (`tests/support/mock_harness.rs`), wired in via
  a `--bin ID=PATH` flag or an `ONEHARNESS_BIN_<ID>` env var.
- Keep tests deterministic and isolated (temp paths, no shared global state) so
  they pass under parallel execution on Linux, macOS, and Windows.
