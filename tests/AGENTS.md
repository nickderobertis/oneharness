# AGENTS (tests)

Subtree rules for tests. Root `AGENTS.md` still applies.

- **Hermetic by construction.** Tests never call a real harness CLI, the network,
  or an authenticated session. The subprocess path is exercised through the
  `oneharness-mock-harness` fixture (`tests/support/mock_harness.rs`), wired in via
  a `--bin ID=PATH` flag or an `ONEHARNESS_BIN_<ID>` env var.
- **Script the mock through its env vars** (`MOCK_STDOUT`, `MOCK_STDERR`,
  `MOCK_EXIT`, `MOCK_SLEEP_MS`, `MOCK_ARGV_FILE`) — do not add bespoke fixtures
  when an env knob already expresses the case.
- **The mock needs its feature.** Run tests via `just test`/`just check`, which
  pass `--features mock-harness`; a bare `cargo test` will not build the fixture
  and the e2e tests will fail fast with a clear message.
- **Pin command construction with `--print-command`.** Every harness adapter has
  an argv assertion in `cli.rs`; that dry-run path is the deterministic proof and
  needs no binary at all. Add one when you add a harness.
- **Assert the contract, not the prose.** Parse the JSON and assert on fields
  (`status`, `exit_code`, `text`, `text_source`); never grep human stderr except
  when the test is specifically about a usage-error message.
- **A printed-path assertion canonicalizes BOTH sides.** oneharness resolves most
  of the paths it prints (a history store, a history project, a bound socket
  address), while the path a test spelled is whatever the caller wrote — on macOS
  the two differ for every temp path. So compare `resolved(actual)` with
  `resolved(expected)` (`cli.rs`'s helper), never the expected side alone:
  canonicalizing only what the test spelled still pins the product's own
  spelling. An assertion deliberately pinning that a path is echoed **as the
  caller spelled it** (the `config_files` chain) is the other contract and stays
  raw on both sides; `just test-symlinked-tmp` is what tells the two apart.
- Keep tests deterministic and isolated (temp paths, no shared global state) so
  they pass under parallel execution on Linux, macOS, and Windows.
