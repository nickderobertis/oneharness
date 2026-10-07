# AGENTS (oneharness)

Subtree rules for the published binary crate — the `clap` surface (`cli.rs`) and
per-verb orchestration (`commands/`) over `oneharness-core` — and its unit tests.
Its `Cargo.toml` stays at the repository root (`cargo install --git`, maturin and
the release matrix name it there); the Nx project is rooted here so that it does
not own every unowned file. Root `AGENTS.md` still applies.

- **The library/CLI boundary is the engine's to state.** How `src/commands/run.rs`
  turns clap's arguments into a `RunRequest`, what this crate may print and what
  `io::run` never may, and why a new `run` flag is three edits are rules of
  `crates/oneharness-core/AGENTS.md` (*What this binary is*), since the engine
  is what every other surface drives.
- **Three tiers test this crate, each its own project.** The unit tests here
  (`oneharness:test`, `--lib --bins`), the in-process integration suites under
  `tests/` (`oneharness-integration`), and the built-binary journeys
  (`oneharness-e2e`). A change here reaches all three; a test edit reaches only
  its own tier.
- **The shipped mock responder is library code.** `tests/support/mock_harness.rs`
  compiles into this crate (`lib.rs`'s `#[path]`), so it is an input of this
  project's targets although `tests/` owns it.
