# AGENTS (history-compat)

Subtree rules for the history compatibility test: the PUBLISHED
`oneharness-core` 0.19.0 (oneharness v0.17.0's history reader) reading the
history this tree writes. Root `AGENTS.md` still applies.

- **The anchor is fixed by design.** The registry dependency is the
  compatibility anchor and never follows the workspace member; it lives in this
  `publish = false` crate because the binary crate carrying it made `cargo
  package` refuse to verify the tarball.
- **It spawns this tree's binaries**, which its `test` target builds beside it
  (`scripts/cargo-test.sh --with oneharness --with oneharness-mock-harness`).
