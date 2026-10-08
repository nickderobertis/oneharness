# AGENTS (oneharness-mock-harness)

Subtree rules for the deterministic fake harness the hermetic suites spawn
through a `--bin ID=PATH` override (or an `ONEHARNESS_BIN_<ID>` env var). The
responder itself ships inside the CLI (`oneharness mock-harness`, from
`tests/support/mock_harness.rs`, the `mock-responder` project); this crate is only the standalone executable
around it, `publish = false`, so it never reaches `cargo install`.

- **Script the mock through its env vars** (`MOCK_STDOUT`, `MOCK_STDERR`,
  `MOCK_EXIT`, `MOCK_SLEEP_MS`, `MOCK_ARGV_FILE`) — do not add bespoke fixtures
  when an env knob already expresses the case.
- **The mock needs its build.** Run a suite that spawns it through its Nx target
  (`just test`, `just e2e`, `just check`), which selects this crate beside the
  suite so cargo builds the fixture into the same profile directory, and passes
  `--features oneharness/mock-harness` for the engine's usage-sweep fault
  injectors; a bare `cargo test` builds neither, and the suites fail fast with a
  clear message.
